//! Database access for the catalog. Uses `sqlx` compile-time-checked queries
//! (`query!`/`query_as!`) — verified against the schema at build time, with the
//! offline `.sqlx` cache committed so builds don't need a live DB.

use chrono::{DateTime, NaiveDate, Utc};
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
    pub tmdb_vote_average: Option<f64>,
    pub tmdb_vote_count: Option<i32>,
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

/// Claim a show for a background refresh when its cache is older than the TTL OR
/// it has no TMDB rating yet (a "cold" show imported before the rating fields
/// existed — it self-heals on first view). This is an atomic conditional bump of
/// `last_synced_at`, so when many viewers hit the same show at once only the
/// first wins the claim; the rest see it fresh and skip. Returns the show's
/// tmdb_id and whether it was cold (a cold show needs a FULL re-fetch to fill
/// every episode's rating; a merely-stale one needs only the incremental refresh).
pub async fn claim_show_refresh(
    pool: &PgPool,
    show_id: Uuid,
    active_ttl_hours: i32,
    ended_ttl_hours: i32,
) -> Result<Option<(i64, bool)>, sqlx::Error> {
    // The cadence is tiered by recency: a show whose latest episode aired within
    // ~2 years uses the active TTL; one that's been off the air longer (treated
    // as ended — rarely changes) uses the much longer ended TTL.
    let row = sqlx::query!(
        r#"
        UPDATE show SET last_synced_at = now()
        WHERE id = $1
          AND (
            tmdb_vote_average IS NULL
            OR last_synced_at < now() - make_interval(hours => CASE
                 WHEN (SELECT max(e.air_date) FROM episode e WHERE e.show_id = show.id)
                      < (now() - interval '2 years')::date
                 THEN $3::int
                 ELSE $2::int
               END)
          )
        RETURNING tmdb_id, (tmdb_vote_average IS NULL) AS "cold!"
        "#,
        show_id,
        active_ttl_hours,
        ended_ttl_hours,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.tmdb_id, r.cold)))
}

/// Our internal show id for a TMDB id, if the show has been imported.
pub async fn find_show_id_by_tmdb(pool: &PgPool, tmdb_id: i64) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query_scalar!("SELECT id FROM show WHERE tmdb_id = $1", tmdb_id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Imported episode count per season for a show. Drives the incremental refresh:
/// re-fetch only seasons that are new (absent here) or have grown (TMDB reports
/// more episodes than we hold).
pub async fn season_episode_counts(
    pool: &PgPool,
    show_id: Uuid,
) -> Result<Vec<(i32, i64)>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT season_number, COUNT(*) AS "n!" FROM episode WHERE show_id = $1 GROUP BY season_number"#,
        show_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| (r.season_number, r.n)).collect())
}

/// Our internal show id for a URL slug, if any.
pub async fn find_show_id_by_slug(pool: &PgPool, slug: &str) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query_scalar!("SELECT id FROM show WHERE slug = $1", slug)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// A show's sitemap entry: its slug and when it was last re-synced (used as the
/// sitemap `<lastmod>`, signalling freshness to crawlers).
pub struct SitemapShow {
    pub slug: String,
    pub last_synced_at: DateTime<Utc>,
}

/// Every show, for the sitemap, with a freshness timestamp. Ordered by name.
///
/// Bounded: each show emits two sitemap URLs, and the sitemap protocol caps a
/// file at 50k URLs — when the catalog approaches the limit, switch to a
/// sitemap index. The cap also keeps this unauthenticated endpoint from
/// materializing an unbounded response.
pub async fn sitemap_shows(pool: &PgPool) -> Result<Vec<SitemapShow>, sqlx::Error> {
    let rows = sqlx::query_as!(
        SitemapShow,
        "SELECT slug, last_synced_at FROM show ORDER BY name LIMIT 20000"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// A published user-authored guide's sitemap entry.
pub struct SitemapGuide {
    pub show_slug: String,
    pub id: Uuid,
    pub updated_at: DateTime<Utc>,
}

/// Every published user guide, joined to its show's slug, for the sitemap.
/// Drafts are excluded — they're not crawlable. Bounded like `sitemap_shows`
/// (newest guides win the budget).
pub async fn sitemap_guides(pool: &PgPool) -> Result<Vec<SitemapGuide>, sqlx::Error> {
    let rows = sqlx::query_as!(
        SitemapGuide,
        r#"SELECT s.slug AS show_slug, g.id, g.updated_at
           FROM skip_guide g
           JOIN show s ON s.id = g.show_id
           WHERE g.is_published
           ORDER BY g.updated_at DESC
           LIMIT 9000"#
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every show's TMDB id, for the `refresh-catalog` backfill.
pub async fn all_show_tmdb_ids(pool: &PgPool) -> Result<Vec<i64>, sqlx::Error> {
    let ids = sqlx::query_scalar!("SELECT tmdb_id FROM show ORDER BY tmdb_id")
        .fetch_all(pool)
        .await?;
    Ok(ids)
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
        "SELECT id, tmdb_id, name, slug, overview, poster_path, \
                tmdb_vote_average, tmdb_vote_count \
         FROM show WHERE id = $1",
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_show(
    executor: impl sqlx::PgExecutor<'_>,
    tmdb_id: i64,
    name: &str,
    slug: &str,
    first_air_year: Option<i32>,
    poster_path: Option<&str>,
    overview: Option<&str>,
    tmdb_vote_average: Option<f64>,
    tmdb_vote_count: Option<i32>,
) -> Result<Uuid, sqlx::Error> {
    // The slug is set only on insert; a re-import (rename) keeps the original
    // slug so existing links stay valid.
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO show (
            tmdb_id, name, slug, first_air_year, poster_path, overview,
            tmdb_vote_average, tmdb_vote_count, last_synced_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
        ON CONFLICT (tmdb_id) DO UPDATE SET
            name = EXCLUDED.name,
            first_air_year = EXCLUDED.first_air_year,
            poster_path = EXCLUDED.poster_path,
            overview = EXCLUDED.overview,
            tmdb_vote_average = EXCLUDED.tmdb_vote_average,
            tmdb_vote_count = EXCLUDED.tmdb_vote_count,
            last_synced_at = now()
        RETURNING id
        "#,
        tmdb_id,
        name,
        slug,
        first_air_year,
        poster_path,
        overview,
        tmdb_vote_average,
        tmdb_vote_count,
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

// Single-episode upsert, retained for tests (the import/refresh path uses the
// bulk `upsert_episodes`).
#[allow(clippy::too_many_arguments, dead_code)]
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

/// Bulk-upsert a season's episodes in TWO statements (constant DB round-trips
/// regardless of season size) — the import/refresh hot path. `episodes` is a
/// JSONB array of objects with the columns below (nulls allowed).
///
/// Re-import handles TMDB renumbering in two parts:
/// - A stale row occupying a slot (`show_id, season_number, episode_number`)
///   now claimed by a *different* tmdb_id, whose own tmdb_id has left the
///   incoming set, genuinely changed identity — it's deleted first (its votes
///   go with it).
/// - A *surviving* episode (same tmdb_id) that TMDB renumbered or shifted is
///   updated in place via `ON CONFLICT (tmdb_id)`, moving its season/number.
///   The slot uniqueness constraint is `DEFERRABLE INITIALLY DEFERRED` (see
///   migration 0011), so the transient duplicate a shift creates mid-statement
///   is tolerated and only the consistent final state is checked at COMMIT.
///   Rows are never recreated, so votes (which FK to the episode id) survive.
pub async fn upsert_episodes(
    conn: &mut sqlx::PgConnection,
    show_id: Uuid,
    season_id: Uuid,
    episodes: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        DELETE FROM episode old
        USING jsonb_to_recordset($2) AS e(
            tmdb_id bigint, season_number int, episode_number int
        )
        WHERE old.show_id = $1
          AND old.season_number = e.season_number
          AND old.episode_number = e.episode_number
          AND old.tmdb_id <> e.tmdb_id
          AND old.tmdb_id NOT IN (
              SELECT (x->>'tmdb_id')::bigint FROM jsonb_array_elements($2) x
          )
        "#,
        show_id,
        episodes,
    )
    .execute(&mut *conn)
    .await?;

    sqlx::query!(
        r#"
        INSERT INTO episode (
            show_id, season_id, tmdb_id, season_number, episode_number,
            name, overview, air_date, still_path, tmdb_vote_average, tmdb_vote_count
        )
        SELECT $1, $2, e.tmdb_id, e.season_number, e.episode_number, e.name, e.overview,
               e.air_date, e.still_path, e.tmdb_vote_average, e.tmdb_vote_count
        FROM jsonb_to_recordset($3) AS e(
            tmdb_id bigint, season_number int, episode_number int, name text,
            overview text, air_date date, still_path text,
            tmdb_vote_average float8, tmdb_vote_count int
        )
        ON CONFLICT (tmdb_id) DO UPDATE SET
            season_id = EXCLUDED.season_id,
            season_number = EXCLUDED.season_number,
            episode_number = EXCLUDED.episode_number,
            name = EXCLUDED.name,
            overview = EXCLUDED.overview,
            air_date = EXCLUDED.air_date,
            still_path = EXCLUDED.still_path,
            tmdb_vote_average = EXCLUDED.tmdb_vote_average,
            tmdb_vote_count = EXCLUDED.tmdb_vote_count
        "#,
        show_id,
        season_id,
        episodes,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Canonical form for emails: trimmed, lowercased. Applied before every
/// store/compare so case/whitespace variants of one mailbox can't become
/// distinct accounts (the DB backstops this with a unique index on
/// `lower(email)`).
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Resolve an OAuth login to our user, keyed on the provider's stable subject
/// id — NOT the email. Returns `(user_id, token_version)`.
///
/// Email (normalized) is profile data. It is consulted only on the FIRST login
/// from a given provider account, to auto-link it to an existing user with the
/// same address — safe today because both providers hand us only *verified*
/// emails. Any future provider must verify emails too, or skip this linking
/// step. After linking, the `(provider, subject)` row is authoritative: an
/// email change at the provider follows the account instead of forking it.
pub async fn resolve_oauth_user(
    pool: &PgPool,
    provider: &str,
    subject: &str,
    email: &str,
    display_name: Option<&str>,
) -> Result<(Uuid, i32), sqlx::Error> {
    let email = normalize_email(email);
    let mut tx = pool.begin().await?;

    // Known identity → its user. Keep the profile fresh: the OAuth name always
    // updates; the email updates only if no other account holds the new one
    // (then we keep the old address rather than fail the login).
    let known = sqlx::query!(
        "SELECT user_id FROM oauth_identity WHERE provider = $1 AND subject = $2",
        provider,
        subject,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(row) = known {
        let ver = sqlx::query_scalar!(
            r#"
            UPDATE app_user SET
                display_name = $2,
                email = CASE WHEN NOT EXISTS (
                    SELECT 1 FROM app_user other WHERE lower(other.email) = $3 AND other.id <> app_user.id
                ) THEN $3 ELSE email END
            WHERE id = $1
            RETURNING token_version
            "#,
            row.user_id,
            display_name,
            email,
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok((row.user_id, ver));
    }

    // First login from this provider account: link to the existing user with
    // the same verified email, else create a fresh one. (A concurrent first
    // login racing this can hit the email/identity unique constraints; the
    // login fails cleanly and the retry lands in the branch above.)
    let existing = sqlx::query!(
        "SELECT id, token_version FROM app_user WHERE lower(email) = $1",
        email
    )
    .fetch_optional(&mut *tx)
    .await?;
    let (user_id, ver) = match existing {
        Some(row) => {
            sqlx::query!(
                "UPDATE app_user SET display_name = $2 WHERE id = $1",
                row.id,
                display_name
            )
            .execute(&mut *tx)
            .await?;
            (row.id, row.token_version)
        }
        None => {
            let row = sqlx::query!(
                "INSERT INTO app_user (email, display_name) VALUES ($1, $2) RETURNING id, token_version",
                email,
                display_name,
            )
            .fetch_one(&mut *tx)
            .await?;
            (row.id, row.token_version)
        }
    };
    sqlx::query!(
        "INSERT INTO oauth_identity (provider, subject, user_id) VALUES ($1, $2, $3)",
        provider,
        subject,
        user_id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((user_id, ver))
}

/// The user's current token version — the session-revocation check. `None`
/// means the account no longer exists (its sessions are dead too).
pub async fn user_token_version(pool: &PgPool, user_id: Uuid) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT token_version FROM app_user WHERE id = $1",
        user_id
    )
    .fetch_optional(pool)
    .await
}

/// Invalidate every outstanding session for a user by bumping their token
/// version (issued JWTs carry the version they were minted with).
pub async fn bump_token_version(pool: &PgPool, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE app_user SET token_version = token_version + 1 WHERE id = $1",
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The effective display name for a user: their chosen screen name, else the
/// OAuth-provided name (None if neither is set).
pub async fn effective_display_name(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    let name = sqlx::query_scalar!(
        "SELECT COALESCE(screen_name, display_name) FROM app_user WHERE id = $1",
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(name)
}

/// Set (or clear, when `None`) a user's screen name. Returns the new effective
/// display name (the screen name, or the OAuth name when cleared).
pub async fn set_screen_name(
    pool: &PgPool,
    user_id: Uuid,
    screen_name: Option<&str>,
) -> Result<Option<String>, sqlx::Error> {
    let name = sqlx::query_scalar!(
        r#"
        UPDATE app_user SET screen_name = $2 WHERE id = $1
        RETURNING COALESCE(screen_name, display_name)
        "#,
        user_id,
        screen_name,
    )
    .fetch_one(pool)
    .await?;
    Ok(name)
}

/// Permanently delete a user account. Votes are retained anonymously (the
/// `vote.user_id` FK is `ON DELETE SET NULL`, so `episode_score` is unaffected),
/// but the user's authored skip guides ARE removed (that FK is `ON DELETE
/// CASCADE`). Returns rows deleted (0 or 1).
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
        -- Safety bound, not pagination: above any real show's episode count
        -- (the longest-running soaps sit under this), it only stops a
        -- pathological row from producing an unbounded response.
        LIMIT 20000
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
        let show = upsert_show(
            &pool, 9001, "Trigger Test", "trigger-test", Some(2020), None, None, None, None,
        )
        .await
        .unwrap();
        let season = upsert_season(&pool, show, 1, Some("Season 1")).await.unwrap();
        upsert_episode(
            &pool, show, season, 8001, 1, 1, Some("Ep 1"), None, None, None, None, None,
        )
        .await
        .unwrap();
        let ep_id = episodes_with_scores(&pool, show, Some(1), None).await.unwrap()[0].id;

        let (alice, _) = resolve_oauth_user(&pool, "test", "sub-alice", "Alice@Test.local ", Some("Alice"))
            .await
            .unwrap();
        let (bob, _) = resolve_oauth_user(&pool, "test", "sub-bob", "bob@test.local", Some("Bob"))
            .await
            .unwrap();

        // Identity is keyed on (provider, subject): the same subject logs into
        // the same account even if the provider reports a changed email...
        let (again, _) = resolve_oauth_user(&pool, "test", "sub-alice", "renamed@test.local", None)
            .await
            .unwrap();
        assert_eq!(again, alice);
        // ...and a DIFFERENT provider asserting Alice's (normalized) email links
        // to her account rather than forking a new one.
        let (linked, _) = resolve_oauth_user(&pool, "other", "sub-x", "renamed@test.local", None)
            .await
            .unwrap();
        assert_eq!(linked, alice);

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

    /// A re-import that renumbers/shifts episodes (matching by stable tmdb_id)
    /// keeps the rows — and their votes — intact, relying on the deferrable slot
    /// constraint from migration 0011. A genuinely replaced episode (its tmdb_id
    /// gone, a new one in its slot) is removed instead.
    #[cfg(feature = "integration")]
    #[sqlx::test]
    async fn reimport_renumber_preserves_votes(pool: sqlx::PgPool) {
        let show = upsert_show(
            &pool, 9100, "Renumber Test", "renumber-test", Some(2021), None, None, None, None,
        )
        .await
        .unwrap();
        let season = upsert_season(&pool, show, 1, Some("Season 1")).await.unwrap();

        // Initial import: three episodes E1(t1), E2(t2), E3(t3).
        let initial = serde_json::json!([
            {"tmdb_id": 7001, "season_number": 1, "episode_number": 1, "name": "A"},
            {"tmdb_id": 7002, "season_number": 1, "episode_number": 2, "name": "B"},
            {"tmdb_id": 7003, "season_number": 1, "episode_number": 3, "name": "C"},
        ]);
        upsert_episodes(&mut pool.acquire().await.unwrap(), show, season, &initial)
            .await
            .unwrap();

        // Vote on episode B (tmdb 7002) so we can prove the row survives the shift.
        let b_id = sqlx::query_scalar!("SELECT id FROM episode WHERE tmdb_id = 7002")
            .fetch_one(&pool)
            .await
            .unwrap();
        let (carol, _) = resolve_oauth_user(&pool, "test", "sub-carol", "carol@test.local", Some("Carol"))
            .await
            .unwrap();
        upsert_vote(&pool, carol, b_id, "FILLER").await.unwrap();

        // Re-import: a NEW episode (t99) inserts at E2, shifting B→E3 and C→E4.
        // Surviving episodes keep their tmdb_id, so they're updated in place; the
        // transient duplicate at E2/E3 is tolerated until COMMIT.
        let renumbered = serde_json::json!([
            {"tmdb_id": 7001, "season_number": 1, "episode_number": 1, "name": "A"},
            {"tmdb_id": 7099, "season_number": 1, "episode_number": 2, "name": "NEW"},
            {"tmdb_id": 7002, "season_number": 1, "episode_number": 3, "name": "B"},
            {"tmdb_id": 7003, "season_number": 1, "episode_number": 4, "name": "C"},
        ]);
        upsert_episodes(&mut pool.acquire().await.unwrap(), show, season, &renumbered)
            .await
            .unwrap();

        // B kept its identity (same row id) and its vote, now at episode 3.
        let b_after = sqlx::query!(
            "SELECT id, episode_number FROM episode WHERE tmdb_id = 7002"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(b_after.id, b_id, "renumbered episode kept its row id");
        assert_eq!(b_after.episode_number, 3);
        assert_eq!(episode_aggregate(&pool, b_id).await.unwrap(), (1, 0, 0));

        // Now a genuine replacement: t7001 (A) disappears; t7100 takes slot E1.
        let replaced = serde_json::json!([
            {"tmdb_id": 7100, "season_number": 1, "episode_number": 1, "name": "REPLACED"},
            {"tmdb_id": 7099, "season_number": 1, "episode_number": 2, "name": "NEW"},
            {"tmdb_id": 7002, "season_number": 1, "episode_number": 3, "name": "B"},
            {"tmdb_id": 7003, "season_number": 1, "episode_number": 4, "name": "C"},
        ]);
        upsert_episodes(&mut pool.acquire().await.unwrap(), show, season, &replaced)
            .await
            .unwrap();
        let gone = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM episode WHERE tmdb_id = 7001)")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(gone, Some(false), "vanished episode was deleted");
        let e1_name = sqlx::query_scalar!(
            "SELECT name FROM episode WHERE show_id = $1 AND season_number = 1 AND episode_number = 1",
            show
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(e1_name.as_deref(), Some("REPLACED"));
    }
}
