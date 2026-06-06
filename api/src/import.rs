//! TMDB import-on-demand. The first time a show is opened we fetch the series,
//! its seasons, and their episodes from TMDB and upsert them. See the design notes and
//! the design notes. TMDB is the catalog source of truth; we only cache it.

use chrono::NaiveDate;
use uuid::Uuid;

use crate::db;
use crate::error::AppError;
use crate::AppState;

/// Resolve a show path param to our internal id.
///
/// `tmdb:<n>` triggers import-on-demand and returns the imported show's id.
/// A bare UUID is looked up; missing → 404.
pub async fn resolve_show_id(state: &AppState, id_param: &str) -> Result<Uuid, AppError> {
    if let Some(rest) = id_param.strip_prefix("tmdb:") {
        let tmdb_id: i64 = rest
            .parse()
            .map_err(|_| AppError::BadRequest(format!("invalid tmdb id: {rest:?}")))?;
        ensure_show_imported(state, tmdb_id).await
    } else {
        let id = Uuid::parse_str(id_param)
            .map_err(|_| AppError::BadRequest(format!("invalid show id: {id_param:?}")))?;
        match db::find_show_core(&state.pool, id).await? {
            Some(_) => Ok(id),
            None => Err(AppError::NotFound(format!("show {id} not found"))),
        }
    }
}

/// Ensure a TMDB show (and its seasons + episodes) is imported. Returns our id.
/// If already imported, returns the existing id without re-fetching.
///
/// All TMDB fetches happen first; only then do we write, atomically, in one
/// transaction. So a network failure mid-import leaves no partial state (which
/// would otherwise be served forever, since the re-import guard keys on the show
/// row's existence), and the DB transaction never spans network I/O.
pub async fn ensure_show_imported(state: &AppState, tmdb_id: i64) -> Result<Uuid, AppError> {
    if let Some(id) = db::find_show_id_by_tmdb(&state.pool, tmdb_id).await? {
        return Ok(id);
    }

    // 1. Fetch everything from TMDB up front.
    let detail = state.tmdb.get_show(tmdb_id).await?;
    let mut seasons = Vec::with_capacity(detail.seasons.len());
    for season in &detail.seasons {
        let full = state.tmdb.get_season(tmdb_id, season.season_number).await?;
        seasons.push((season, full));
    }

    // 2. Write atomically — rolls back as a whole if anything fails.
    let mut tx = state.pool.begin().await?;
    let show_id = db::upsert_show(
        &mut *tx,
        detail.id,
        &detail.name,
        detail.first_air_date.as_deref().and_then(parse_year),
        detail.poster_path.as_deref(),
        detail.overview.as_deref(),
    )
    .await?;

    for (season, full) in &seasons {
        let season_id =
            db::upsert_season(&mut *tx, show_id, season.season_number, Some(&season.name)).await?;
        for ep in &full.episodes {
            db::upsert_episode(
                &mut *tx,
                show_id,
                season_id,
                ep.id,
                ep.season_number,
                ep.episode_number,
                Some(&ep.name),
                ep.overview.as_deref(),
                ep.air_date.as_deref().and_then(parse_date),
                ep.still_path.as_deref(),
            )
            .await?;
        }
    }
    tx.commit().await?;

    tracing::info!("imported show tmdb:{tmdb_id} ({})", detail.name);
    Ok(show_id)
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
