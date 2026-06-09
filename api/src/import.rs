//! TMDB import-on-demand. The first time a show is opened we fetch the series,
//! its seasons, and their episodes from TMDB and upsert them. TMDB is the
//! catalog source of truth; we only cache it.

use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db;
use crate::error::AppError;
use crate::tmdb::TmdbClient;
use crate::AppState;

/// Resolve a show path param to our internal id.
///
/// Accepts, in order: `tmdb:<n>` (import-on-demand), a URL slug, or a bare UUID
/// (kept for back-compat with older links). Unknown → 404.
pub async fn resolve_show_id(state: &AppState, id_param: &str) -> Result<Uuid, AppError> {
    let id = if let Some(rest) = id_param.strip_prefix("tmdb:") {
        let tmdb_id: i64 = rest
            .parse()
            .map_err(|_| AppError::BadRequest(format!("invalid tmdb id: {rest:?}")))?;
        ensure_show_imported(state, tmdb_id).await?
    } else if let Some(id) = db::find_show_id_by_slug(&state.pool, id_param).await? {
        id
    } else if let Ok(uuid) = Uuid::parse_str(id_param) {
        // Legacy: a bare UUID still resolves so old links keep working.
        match db::find_show_core(&state.pool, uuid).await? {
            Some(_) => uuid,
            None => return Err(AppError::NotFound(format!("show {id_param:?} not found"))),
        }
    } else {
        return Err(AppError::NotFound(format!("show {id_param:?} not found")));
    };

    // Keep viewed shows fresh without blocking the response (TMDB ratings drift;
    // ongoing shows add episodes). The DB claim dedupes concurrent viewers.
    maybe_refresh_in_background(state, id);
    Ok(id)
}

/// Spawn a background, TTL-gated refresh of a viewed show. Adds nothing to the
/// request's latency: the claim and the refresh both run off the request path,
/// and `claim_show_refresh` atomically ensures only one viewer per TTL refreshes.
fn maybe_refresh_in_background(state: &AppState, show_id: Uuid) {
    let state = state.clone();
    tokio::spawn(async move {
        match db::claim_show_refresh(
            &state.pool,
            show_id,
            state.refresh_ttl_hours,
            state.refresh_ttl_hours_ended,
        )
        .await
        {
            Ok(Some((tmdb_id, cold))) => {
                // A cold show (no rating yet → predates the TMDB-rating fields) gets
                // a FULL re-import so every episode's rating is filled; a merely
                // stale show needs only the cheap incremental refresh.
                let result = if cold {
                    import_show(&state.tmdb, &state.pool, tmdb_id).await.map(|_| ())
                } else {
                    refresh_show(&state.tmdb, &state.pool, tmdb_id).await
                };
                if let Err(e) = result {
                    tracing::warn!("background refresh of show tmdb:{tmdb_id} failed: {e:?}");
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("refresh claim failed for show {show_id}: {e}"),
        }
    });
}

/// Incrementally refresh an already-imported show with MINIMAL TMDB calls: one
/// series-detail call (updates the show's metadata + overall rating and reveals
/// the current season list), then re-fetch ONLY seasons that are new or have
/// gained episodes. A stable/ended show costs a single call; an ongoing show
/// costs one more for the airing season. Episode ratings in unchanged seasons
/// drift until that season changes (or a full `refresh-catalog`). All fetches
/// happen before the write tx; slug and the vote/score layer are preserved.
pub async fn refresh_show(tmdb: &TmdbClient, pool: &PgPool, tmdb_id: i64) -> Result<(), AppError> {
    let detail = tmdb.get_show(tmdb_id).await?;
    let Some(show_id) = db::find_show_id_by_tmdb(pool, tmdb_id).await? else {
        return Ok(()); // deleted since the claim; nothing to refresh
    };
    let have: std::collections::HashMap<i32, i64> = db::season_episode_counts(pool, show_id)
        .await?
        .into_iter()
        .collect();

    // Fetch only seasons that are new or have grown — up front, before the tx.
    let mut fetched = Vec::new();
    for season in &detail.seasons {
        let have_count = have.get(&season.season_number).copied().unwrap_or(0);
        if have_count < season.episode_count as i64 {
            let full = tmdb.get_season(tmdb_id, season.season_number).await?;
            fetched.push((season, full));
        }
    }

    let mut tx = pool.begin().await?;
    // Update the show's metadata + overall rating. The slug arg is only used on
    // insert; this is an existing row, so ON CONFLICT keeps the original slug.
    db::upsert_show(
        &mut *tx,
        detail.id,
        &detail.name,
        &db::slugify(&detail.name),
        detail.first_air_date.as_deref().and_then(parse_year),
        detail.poster_path.as_deref(),
        detail.overview.as_deref(),
        detail.vote_average,
        detail.vote_count,
    )
    .await?;
    for (season, full) in &fetched {
        let season_id =
            db::upsert_season(&mut *tx, show_id, season.season_number, Some(&season.name)).await?;
        write_episodes(&mut *tx, show_id, season_id, &full.episodes).await?;
    }
    tx.commit().await?;
    if !fetched.is_empty() {
        tracing::info!(
            "refreshed show tmdb:{tmdb_id}: {} season(s) updated",
            fetched.len()
        );
    }
    Ok(())
}

/// Ensure a TMDB show (and its seasons + episodes) is imported. Returns our id.
/// If already imported, returns the existing id without re-fetching (refresh on
/// a TTL is handled separately).
///
/// All TMDB fetches happen first; only then do we write, atomically, in one
/// transaction. So a network failure mid-import leaves no partial state (which
/// would otherwise be served forever, since the re-import guard keys on the show
/// row's existence), and the DB transaction never spans network I/O.
pub async fn ensure_show_imported(state: &AppState, tmdb_id: i64) -> Result<Uuid, AppError> {
    if let Some(id) = db::find_show_id_by_tmdb(&state.pool, tmdb_id).await? {
        return Ok(id);
    }

    // Importing a not-yet-seen show fans out to TMDB (series + one call per
    // season). This path is unauthenticated, so bound how often it can run per
    // instance — already-imported shows short-circuit above and never hit this.
    crate::rate_limit::check_import(&state.import_limiter)?;
    import_show(&state.tmdb, &state.pool, tmdb_id).await
}

/// Fetch a show (series + seasons + episodes, with TMDB ratings) and upsert it.
/// Used both for first-time import and the `refresh-catalog` backfill, so it has
/// no "already imported" guard or rate limit of its own. All TMDB fetches happen
/// up front; the DB write is one transaction that never spans network I/O, so a
/// mid-import failure leaves no partial state. A re-import preserves the slug and
/// the opinion layer (votes/scores are never touched here).
pub async fn import_show(tmdb: &TmdbClient, pool: &PgPool, tmdb_id: i64) -> Result<Uuid, AppError> {
    let detail = tmdb.get_show(tmdb_id).await?;
    let mut seasons = Vec::with_capacity(detail.seasons.len());
    for season in &detail.seasons {
        let full = tmdb.get_season(tmdb_id, season.season_number).await?;
        seasons.push((season, full));
    }

    let mut tx = pool.begin().await?;
    let slug = db::pick_unique_slug(&mut tx, &detail.name, detail.id).await?;
    let show_id = db::upsert_show(
        &mut *tx,
        detail.id,
        &detail.name,
        &slug,
        detail.first_air_date.as_deref().and_then(parse_year),
        detail.poster_path.as_deref(),
        detail.overview.as_deref(),
        detail.vote_average,
        detail.vote_count,
    )
    .await?;

    for (season, full) in &seasons {
        let season_id =
            db::upsert_season(&mut *tx, show_id, season.season_number, Some(&season.name)).await?;
        write_episodes(&mut *tx, show_id, season_id, &full.episodes).await?;
    }
    tx.commit().await?;

    tracing::info!("imported show tmdb:{tmdb_id} ({})", detail.name);
    metrics::counter!("show_imports_total").increment(1);
    Ok(show_id)
}

/// Bulk-upsert a season's episodes (one statement) by encoding them as a JSONB
/// array. A no-op for an empty season. Empty TMDB air dates become null.
async fn write_episodes(
    executor: impl sqlx::PgExecutor<'_>,
    show_id: Uuid,
    season_id: Uuid,
    episodes: &[crate::tmdb::TmdbEpisode],
) -> Result<(), AppError> {
    if episodes.is_empty() {
        return Ok(());
    }
    let rows: Vec<serde_json::Value> = episodes
        .iter()
        .map(|ep| {
            serde_json::json!({
                "tmdb_id": ep.id,
                "season_number": ep.season_number,
                "episode_number": ep.episode_number,
                "name": ep.name,
                "overview": ep.overview,
                "air_date": ep.air_date.as_deref().and_then(parse_date),
                "still_path": ep.still_path,
                "tmdb_vote_average": ep.vote_average,
                "tmdb_vote_count": ep.vote_count,
            })
        })
        .collect();
    db::upsert_episodes(executor, show_id, season_id, &serde_json::Value::Array(rows)).await?;
    Ok(())
}

/// Parse the year from a TMDB date string like "2011-04-17".
pub(crate) fn parse_year(date: &str) -> Option<i32> {
    date.get(0..4).and_then(|y| y.parse::<i32>().ok())
}

/// Parse a TMDB date string. Empty strings (TMDB uses "") become None.
fn parse_date(date: &str) -> Option<NaiveDate> {
    if date.is_empty() {
        return None;
    }
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_year_extracts_leading_year() {
        assert_eq!(parse_year("2011-04-17"), Some(2011));
        assert_eq!(parse_year("1998"), Some(1998));
        assert_eq!(parse_year(""), None);
        assert_eq!(parse_year("n/a"), None);
    }

    #[test]
    fn parse_date_handles_empty_and_valid() {
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("not-a-date"), None);
        assert_eq!(
            parse_date("2011-04-17"),
            Some(NaiveDate::from_ymd_opt(2011, 4, 17).unwrap())
        );
    }
}
