//! Database access for the catalog. Uses `sqlx` compile-time-checked queries
//! (`query!`/`query_as!`) — verified against the schema at build time, with the
//! offline `.sqlx` cache committed so builds don't need a live DB.

use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{EpisodeItem, EpisodeScoreView, SeasonSummary};
use crate::scoring;

/// Core show fields used to build a detail response.
pub struct ShowCore {
    pub id: Uuid,
    pub tmdb_id: i64,
    pub name: String,
    pub slug: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
}

/// A show imported from TMDB, keyed by tmdb id in search results.
pub struct ImportedShow {
    pub tmdb_id: i64,
    pub id: Uuid,
    pub slug: String,
}

/// URL slug for a show name: lowercase, runs of non-alphanumerics collapsed to a
/// single dash, dashes trimmed from the ends. Mirrors the SQL backfill in
/// `0004_show_slug.sql`. May return "" (caller falls back to the tmdb id).
pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut pending_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    slug
}

/// Pick a unique slug for a show: `slugify(name)` (or the tmdb id if that's
/// empty), then — if already taken by a *different* show — `-<tmdb_id>`, then
/// `-<tmdb_id>-N`, returning the first free one. Each candidate is checked
/// against the actual `slug` column, so this stays correct even when a suffixed
/// slug would collide with another show's bare slug (e.g. "Foo" #100 → "foo-100"
/// vs a show literally named "Foo 100"). Mirrors the backfill in
/// `0004_show_slug.sql`. Takes `&mut PgConnection` so it can probe in a loop.
pub async fn pick_unique_slug(
    conn: &mut sqlx::PgConnection,
    name: &str,
    tmdb_id: i64,
) -> Result<String, sqlx::Error> {
    let base = {
        let s = slugify(name);
        if s.is_empty() { tmdb_id.to_string() } else { s }
    };
    let mut candidate = base.clone();
    let mut n = 0u32;
    loop {
        let taken = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM show WHERE slug = $1 AND tmdb_id <> $2)",
            candidate,
            tmdb_id
        )
        .fetch_one(&mut *conn)
        .await?
        .unwrap_or(false);
        if !taken {
            return Ok(candidate);
        }
        n += 1;
        candidate = if n == 1 {
            format!("{base}-{tmdb_id}")
        } else {
            format!("{base}-{tmdb_id}-{n}")
        };
    }
}

/// Our internal show id for a TMDB id, if the show has been imported.
pub async fn find_show_id_by_tmdb(pool: &PgPool, tmdb_id: i64) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query_scalar!("SELECT id FROM show WHERE tmdb_id = $1", tmdb_id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Our internal show id for a URL slug, if any.
pub async fn find_show_id_by_slug(pool: &PgPool, slug: &str) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query_scalar!("SELECT id FROM show WHERE slug = $1", slug)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Every show's slug, for the sitemap. Ordered by name for a stable listing.
pub async fn all_show_slugs(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let slugs = sqlx::query_scalar!("SELECT slug FROM show ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(slugs)
}

/// Map a set of TMDB ids to the ones we already have imported.
pub async fn imported_show_ids(
    pool: &PgPool,
    tmdb_ids: &[i64],
) -> Result<Vec<ImportedShow>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT tmdb_id, id, slug FROM show WHERE tmdb_id = ANY($1)",
        tmdb_ids
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ImportedShow { tmdb_id: r.tmdb_id, id: r.id, slug: r.slug })
        .collect())
}

pub async fn find_show_core(pool: &PgPool, id: Uuid) -> Result<Option<ShowCore>, sqlx::Error> {
    let row = sqlx::query_as!(
        ShowCore,
        "SELECT id, tmdb_id, name, slug, overview, poster_path FROM show WHERE id = $1",
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn upsert_show(
    executor: impl sqlx::PgExecutor<'_>,
    tmdb_id: i64,
    name: &str,
    slug: &str,
    first_air_year: Option<i32>,
    poster_path: Option<&str>,
    overview: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    // The slug is set only on insert; a re-import (rename) keeps the original
    // slug so existing links stay valid.
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO show (tmdb_id, name, slug, first_air_year, poster_path, overview, last_synced_at)
        VALUES ($1, $2, $3, $4, $5, $6, now())
        ON CONFLICT (tmdb_id) DO UPDATE SET
            name = EXCLUDED.name,
            first_air_year = EXCLUDED.first_air_year,
            poster_path = EXCLUDED.poster_path,
            overview = EXCLUDED.overview,
            last_synced_at = now()
        RETURNING id
        "#,
        tmdb_id,
        name,
        slug,
        first_air_year,
        poster_path,
        overview,
    )
    .fetch_one(executor)
    .await?;
    Ok(id)
}

pub async fn upsert_season(
    executor: impl sqlx::PgExecutor<'_>,
    show_id: Uuid,
    season_number: i32,
    name: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO season (show_id, season_number, name)
        VALUES ($1, $2, $3)
        ON CONFLICT (show_id, season_number) DO UPDATE SET name = EXCLUDED.name
        RETURNING id
        "#,
        show_id,
        season_number,
        name,
    )
    .fetch_one(executor)
    .await?;
    Ok(id)
}

// NOTE (known gap): `episode` has two unique constraints — `tmdb_id` and
// `(show_id, season_number, episode_number)`. This upsert handles conflicts on
// `tmdb_id` only. On a *re-import* where TMDB has renumbered an episode into a
// slot already held by a different row, the second constraint can raise a unique
// violation. First-import of a fresh show cannot hit this. Resolve alongside the
// TTL-refresh feature — e.g. delete-then-insert within the import tx.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_episode(
    executor: impl sqlx::PgExecutor<'_>,
    show_id: Uuid,
    season_id: Uuid,
    tmdb_id: i64,
    season_number: i32,
    episode_number: i32,
    name: Option<&str>,
    overview: Option<&str>,
    air_date: Option<NaiveDate>,
    still_path: Option<&str>,
    tmdb_vote_average: Option<f64>,
    tmdb_vote_count: Option<i32>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO episode (
            show_id, season_id, tmdb_id, season_number, episode_number,
            name, overview, air_date, still_path, tmdb_vote_average, tmdb_vote_count
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (tmdb_id) DO UPDATE SET
            name = EXCLUDED.name,
            overview = EXCLUDED.overview,
            air_date = EXCLUDED.air_date,
            still_path = EXCLUDED.still_path,
            tmdb_vote_average = EXCLUDED.tmdb_vote_average,
            tmdb_vote_count = EXCLUDED.tmdb_vote_count
        "#,
        show_id,
        season_id,
        tmdb_id,
        season_number,
        episode_number,
        name,
        overview,
        air_date,
        still_path,
        tmdb_vote_average,
        tmdb_vote_count,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Find or create a user by email (the OAuth identity key). Updates
/// the display name on subsequent logins. Returns our user id.
pub async fn upsert_user_by_email(
    pool: &PgPool,
    email: &str,
    display_name: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO app_user (email, display_name)
        VALUES ($1, $2)
        ON CONFLICT (email) DO UPDATE SET display_name = EXCLUDED.display_name
        RETURNING id
        "#,
        email,
        display_name,
    )
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Permanently delete a user account. The user's votes are NOT deleted: the
/// `vote.user_id` FK is `ON DELETE SET NULL`, so they're retained anonymously and
/// the maintained `episode_score` is unaffected. Returns rows deleted (0 or 1).
pub async fn delete_user(pool: &PgPool, user_id: Uuid) -> Result<u64, sqlx::Error> {
    let res = sqlx::query!("DELETE FROM app_user WHERE id = $1", user_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// For each given show, the fraction of its episodes that have at least
/// `min_votes` total votes — i.e. how much of the show the community has rated
/// confidently. One row per show that has episodes; shows with no episodes are
/// omitted (the caller treats a missing show as 0.0 coverage).
pub async fn filler_coverage(
    pool: &PgPool,
    show_ids: &[Uuid],
    min_votes: i64,
) -> Result<Vec<(Uuid, f64)>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT
            e.show_id AS "show_id!",
            (COUNT(*) FILTER (
                WHERE COALESCE(es.filler_votes, 0)
                    + COALESCE(es.worth_watching_votes, 0)
                    + COALESCE(es.canon_votes, 0) >= $2::int8
            ))::float8 / NULLIF(COUNT(*), 0) AS "coverage!"
        FROM episode e
        LEFT JOIN episode_score es ON es.episode_id = e.id
        WHERE e.show_id = ANY($1)
        GROUP BY e.show_id
        "#,
        show_ids,
        min_votes,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| (r.show_id, r.coverage)).collect())
}

/// Seasons for a show with their imported-episode counts, ordered by number.
pub async fn seasons_with_counts(
    pool: &PgPool,
    show_id: Uuid,
) -> Result<Vec<SeasonSummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT s.id, s.season_number, s.name, COUNT(e.id) AS "episode_count!"
        FROM season s
        LEFT JOIN episode e ON e.season_id = s.id
        WHERE s.show_id = $1
        GROUP BY s.id
        ORDER BY s.season_number
        "#,
        show_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| SeasonSummary {
            id: r.id,
            season_number: r.season_number,
            name: r.name,
            episode_count: r.episode_count,
        })
        .collect())
}

/// Episodes for a show (optionally one season) with aggregated vote counts,
/// turned into the API view (status derived via the scoring module). When
/// `user_id` is Some, each episode's `myVote` reflects that user's vote.
///
/// Reads the maintained `episode_score` tally (one indexed row per episode,
/// kept current by triggers) rather than aggregating the `vote` table on every
/// request — the read-path scaling lever. A missing score row
/// (episode with no votes yet) COALESCEs to zero. `myVote` is per-user and
/// can't be precomputed, so it stays a single correlated lookup.
pub async fn episodes_with_scores(
    pool: &PgPool,
    show_id: Uuid,
    season: Option<i32>,
    user_id: Option<Uuid>,
) -> Result<Vec<EpisodeItem>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT
            e.id,
            e.season_number,
            e.episode_number,
            e.name,
            e.air_date,
            e.still_path,
            e.tmdb_vote_average,
            e.tmdb_vote_count,
            COALESCE(es.filler_votes, 0)::bigint         AS "filler_votes!",
            COALESCE(es.worth_watching_votes, 0)::bigint AS "worth_watching_votes!",
            COALESCE(es.canon_votes, 0)::bigint          AS "canon_votes!",
            (SELECT mv.value::text FROM vote mv
             WHERE mv.episode_id = e.id AND mv.user_id = $3) AS "my_vote?"
        FROM episode e
        LEFT JOIN episode_score es ON es.episode_id = e.id
        WHERE e.show_id = $1 AND ($2::int IS NULL OR e.season_number = $2)
        ORDER BY e.season_number, e.episode_number
        "#,
        show_id,
        season,
        user_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let (f, w, c) = (r.filler_votes, r.worth_watching_votes, r.canon_votes);
            EpisodeItem {
                id: r.id,
                season_number: r.season_number,
                episode_number: r.episode_number,
                name: r.name,
                air_date: r.air_date,
                still_path: r.still_path,
                tmdb_rating: r.tmdb_vote_average,
                tmdb_vote_count: r.tmdb_vote_count,
                score: EpisodeScoreView {
                    filler_votes: f,
                    worth_watching_votes: w,
                    canon_votes: c,
                    filler_score: scoring::filler_score(f, w, c),
                    status: scoring::status(f, w, c),
                    my_vote: r.my_vote.as_deref().and_then(scoring::VoteValue::from_db),
                },
            }
        })
        .collect())
}

/// Whether an episode exists (for a clean 404 before a vote write).
pub async fn episode_exists(pool: &PgPool, episode_id: Uuid) -> Result<bool, sqlx::Error> {
    let exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM episode WHERE id = $1)",
        episode_id
    )
    .fetch_one(pool)
    .await?;
    Ok(exists.unwrap_or(false))
}

/// Cast or change a user's vote on an episode (one row per user+episode).
pub async fn upsert_vote(
    pool: &PgPool,
    user_id: Uuid,
    episode_id: Uuid,
    value: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO vote (user_id, episode_id, value)
        VALUES ($1, $2, ($3::text)::vote_value)
        ON CONFLICT (user_id, episode_id)
        DO UPDATE SET value = EXCLUDED.value, updated_at = now()
        "#,
        user_id,
        episode_id,
        value,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a user's vote. Returns the number of rows deleted (0 if none).
pub async fn delete_vote(
    pool: &PgPool,
    user_id: Uuid,
    episode_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query!(
        "DELETE FROM vote WHERE user_id = $1 AND episode_id = $2",
        user_id,
        episode_id,
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Aggregate (filler, worth_watching, canon) vote counts for one episode, read
/// from the maintained `episode_score` tally. A vote write fires the trigger
/// that refreshes this row in the same transaction, so the vote endpoints read
/// back fresh counts. A missing row (no votes) is reported as zeros.
pub async fn episode_aggregate(
    pool: &PgPool,
    episode_id: Uuid,
) -> Result<(i64, i64, i64), sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT
            filler_votes::bigint         AS "filler!",
            worth_watching_votes::bigint AS "worth_watching!",
            canon_votes::bigint          AS "canon!"
        FROM episode_score WHERE episode_id = $1
        "#,
        episode_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map_or((0, 0, 0), |r| (r.filler, r.worth_watching, r.canon)))
}

/// Rebuild the entire `episode_score` table from the source `vote` rows. The
/// triggers keep it consistent in normal operation; this is the drift-correction
/// / backfill path, run as the `recompute-scores` subcommand (e.g. on a
/// schedule or after a bulk import). Returns the number of episodes written.
pub async fn recompute_all_scores(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query!(
        r#"
        INSERT INTO episode_score (
            episode_id, filler_votes, worth_watching_votes, canon_votes,
            filler_score, updated_at
        )
        SELECT
            e.id,
            COUNT(v.id) FILTER (WHERE v.value = 'FILLER'),
            COUNT(v.id) FILTER (WHERE v.value = 'WORTH_WATCHING'),
            COUNT(v.id) FILTER (WHERE v.value = 'CANON'),
            CASE WHEN COUNT(v.id) = 0 THEN NULL
                 ELSE COUNT(v.id) FILTER (WHERE v.value = 'FILLER')::float8 / COUNT(v.id)
            END,
            now()
        FROM episode e
        LEFT JOIN vote v ON v.episode_id = e.id
        GROUP BY e.id
        ON CONFLICT (episode_id) DO UPDATE SET
            filler_votes         = EXCLUDED.filler_votes,
            worth_watching_votes = EXCLUDED.worth_watching_votes,
            canon_votes          = EXCLUDED.canon_votes,
            filler_score         = EXCLUDED.filler_score,
            updated_at           = EXCLUDED.updated_at
        "#,
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Star Trek: The Next Generation"), "star-trek-the-next-generation");
        assert_eq!(slugify("Marvel's Agents of S.H.I.E.L.D."), "marvel-s-agents-of-s-h-i-e-l-d");
        assert_eq!(slugify("WandaVision"), "wandavision");
    }

    #[test]
    fn slugify_collapses_and_trims() {
        assert_eq!(slugify("  Hello —— World!!  "), "hello-world");
        assert_eq!(slugify("12 Monkeys"), "12-monkeys");
        assert_eq!(slugify("---"), "");
        assert_eq!(slugify("ＡＢＣ"), ""); // non-ascii letters drop → caller falls back to tmdb id
    }

    /// End-to-end check of the `episode_score` denormalization: the trigger keeps
    /// the tally current across INSERT/UPDATE/DELETE of votes, `recompute_all_scores`
    /// agrees with it, and deleting a user retains (anonymizes) their votes rather
    /// than dropping the counts. Runs against a throwaway DB (`sqlx::test`).
    #[cfg(feature = "integration")]
    #[sqlx::test]
    async fn episode_score_trigger_and_anonymize(pool: sqlx::PgPool) {
        let show = upsert_show(&pool, 9001, "Trigger Test", "trigger-test", Some(2020), None, None)
            .await
            .unwrap();
        let season = upsert_season(&pool, show, 1, Some("Season 1")).await.unwrap();
        upsert_episode(
            &pool, show, season, 8001, 1, 1, Some("Ep 1"), None, None, None, None, None,
        )
        .await
        .unwrap();
        let ep_id = episodes_with_scores(&pool, show, Some(1), None).await.unwrap()[0].id;

        let alice = upsert_user_by_email(&pool, "alice@test.local", Some("Alice")).await.unwrap();
        let bob = upsert_user_by_email(&pool, "bob@test.local", Some("Bob")).await.unwrap();

        // Trigger maintains the tally on INSERT.
        upsert_vote(&pool, alice, ep_id, "FILLER").await.unwrap();
        upsert_vote(&pool, bob, ep_id, "CANON").await.unwrap();
        assert_eq!(episode_aggregate(&pool, ep_id).await.unwrap(), (1, 0, 1));

        // ...on UPDATE (a user changing their vote).
        upsert_vote(&pool, alice, ep_id, "WORTH_WATCHING").await.unwrap();
        assert_eq!(episode_aggregate(&pool, ep_id).await.unwrap(), (0, 1, 1));

        // ...and on DELETE.
        delete_vote(&pool, bob, ep_id).await.unwrap();
        assert_eq!(episode_aggregate(&pool, ep_id).await.unwrap(), (0, 1, 0));

        // A full rebuild agrees with the trigger-maintained tally.
        recompute_all_scores(&pool).await.unwrap();
        assert_eq!(episode_aggregate(&pool, ep_id).await.unwrap(), (0, 1, 0));

        // Deleting Alice keeps her vote (anonymized): the count is unchanged.
        assert_eq!(delete_user(&pool, alice).await.unwrap(), 1);
        assert_eq!(episode_aggregate(&pool, ep_id).await.unwrap(), (0, 1, 0));
    }
}
