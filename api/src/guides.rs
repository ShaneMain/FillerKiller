//! User-authored skip guides: a curated per-episode verdict for a show that
//! other signed-in users can browse, share, and like. This is the human-authored
//! counterpart to the algorithmic guide in `scoring.rs`. Wire models + DB access;
//! the HTTP handlers live in `main.rs`.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

pub const MAX_TITLE: usize = 80;
pub const MAX_DESCRIPTION: usize = 500;
/// A user may publish at most this many guides per show.
pub const MAX_PUBLISHED_PER_SHOW: i64 = 5;
/// Hard cap on entries per guide (bounds the request body).
const MAX_ENTRIES: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Disposition {
    Watch,
    Optional,
    Skip,
}

impl Disposition {
    pub fn as_db(&self) -> &'static str {
        match self {
            Disposition::Watch => "WATCH",
            Disposition::Optional => "OPTIONAL",
            Disposition::Skip => "SKIP",
        }
    }

    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "WATCH" => Some(Disposition::Watch),
            "OPTIONAL" => Some(Disposition::Optional),
            "SKIP" => Some(Disposition::Skip),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideEntryInput {
    pub episode_id: Uuid,
    pub disposition: Disposition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideInput {
    pub title: String,
    pub description: Option<String>,
    pub entries: Vec<GuideEntryInput>,
    #[serde(default)]
    pub published: bool,
}

impl GuideInput {
    /// Validate and normalize the input. Returns the trimmed title and the cleaned
    /// description (None when blank), or a `BadRequest` describing the problem.
    fn clean(&self) -> Result<(String, Option<String>), AppError> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err(AppError::BadRequest("a title is required".into()));
        }
        if title.chars().count() > MAX_TITLE {
            return Err(AppError::BadRequest(format!(
                "title must be at most {MAX_TITLE} characters"
            )));
        }
        let description = match self.description.as_deref().map(str::trim) {
            Some(d) if !d.is_empty() => {
                if d.chars().count() > MAX_DESCRIPTION {
                    return Err(AppError::BadRequest(format!(
                        "description must be at most {MAX_DESCRIPTION} characters"
                    )));
                }
                Some(d.to_string())
            }
            _ => None,
        };
        if self.entries.is_empty() {
            return Err(AppError::BadRequest("add at least one episode".into()));
        }
        if self.entries.len() > MAX_ENTRIES {
            return Err(AppError::BadRequest("too many episodes".into()));
        }
        Ok((title.to_string(), description))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideSummary {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub author_name: Option<String>,
    pub like_count: i64,
    pub watch_count: i64,
    pub optional_count: i64,
    pub skip_count: i64,
    pub is_published: bool,
    /// Whether the viewer has liked this guide (false when signed out).
    pub my_like: bool,
    /// Whether the viewer is the author (false when signed out).
    pub mine: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideEntryView {
    pub episode_id: Uuid,
    pub season_number: i32,
    pub episode_number: i32,
    pub name: Option<String>,
    pub disposition: Disposition,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideDetail {
    pub id: Uuid,
    pub show_id: Uuid,
    pub show_slug: String,
    pub show_name: String,
    pub poster_path: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub author_name: Option<String>,
    pub like_count: i64,
    pub is_published: bool,
    pub my_like: bool,
    pub mine: bool,
    pub entries: Vec<GuideEntryView>,
}

/// A guide as shown in the author's own list (their account page) — includes
/// drafts, with the show it belongs to for linking.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyGuide {
    pub id: Uuid,
    pub title: String,
    pub is_published: bool,
    pub like_count: i64,
    pub show_slug: String,
    pub show_name: String,
}

/// Lightweight ownership/visibility metadata for authorization checks.
pub struct GuideMeta {
    pub author_id: Option<Uuid>,
    pub show_id: Uuid,
    pub is_published: bool,
}

pub async fn guide_meta(pool: &PgPool, guide_id: Uuid) -> Result<Option<GuideMeta>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT author_id, show_id, is_published FROM skip_guide WHERE id = $1",
        guide_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| GuideMeta {
        author_id: r.author_id,
        show_id: r.show_id,
        is_published: r.is_published,
    }))
}

/// Count how many guides the author has PUBLISHED for a show, optionally
/// excluding one guide (used when re-publishing during an edit). The cap built on
/// this is a soft, self-scoped UX limit (a user racing themselves could briefly
/// exceed it); it isn't an integrity boundary, so it's enforced best-effort
/// rather than under a lock.
async fn count_published(
    pool: &PgPool,
    show_id: Uuid,
    author_id: Uuid,
    exclude: Option<Uuid>,
) -> Result<i64, sqlx::Error> {
    let n = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "n!"
        FROM skip_guide
        WHERE show_id = $1 AND author_id = $2 AND is_published = TRUE
          AND ($3::uuid IS NULL OR id <> $3)
        "#,
        show_id,
        author_id,
        exclude,
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
}

/// Verify every provided episode id belongs to the show (and exists). Returns a
/// `BadRequest` otherwise. Also rejects duplicate episodes.
async fn validate_entries(
    pool: &PgPool,
    show_id: Uuid,
    input: &GuideInput,
) -> Result<(), AppError> {
    let mut ids: Vec<Uuid> = input.entries.iter().map(|e| e.episode_id).collect();
    let provided = ids.len();
    ids.sort();
    ids.dedup();
    if ids.len() != provided {
        return Err(AppError::BadRequest("an episode is listed more than once".into()));
    }
    let matched = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "n!" FROM episode WHERE id = ANY($1) AND show_id = $2"#,
        &ids,
        show_id,
    )
    .fetch_one(pool)
    .await?;
    if matched != ids.len() as i64 {
        return Err(AppError::BadRequest(
            "a listed episode doesn't belong to this show".into(),
        ));
    }
    Ok(())
}

/// Replace a guide's entries with the input set (used on create and update).
async fn write_entries(
    tx: &mut sqlx::PgConnection,
    guide_id: Uuid,
    input: &GuideInput,
) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM skip_guide_entry WHERE guide_id = $1", guide_id)
        .execute(&mut *tx)
        .await?;
    let episode_ids: Vec<Uuid> = input.entries.iter().map(|e| e.episode_id).collect();
    let dispositions: Vec<String> = input
        .entries
        .iter()
        .map(|e| e.disposition.as_db().to_string())
        .collect();
    sqlx::query!(
        r#"
        INSERT INTO skip_guide_entry (guide_id, episode_id, disposition)
        SELECT $1, ep, disp::guide_disposition
        FROM UNNEST($2::uuid[], $3::text[]) AS t(ep, disp)
        "#,
        guide_id,
        &episode_ids,
        &dispositions,
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Create a guide for a show authored by `author_id`. Validates input, the 5-per-
/// show publish cap, and that episodes belong to the show. Returns the new id.
pub async fn create_guide(
    pool: &PgPool,
    show_id: Uuid,
    author_id: Uuid,
    input: &GuideInput,
) -> Result<Uuid, AppError> {
    let (title, description) = input.clean()?;
    validate_entries(pool, show_id, input).await?;
    if input.published && count_published(pool, show_id, author_id, None).await? >= MAX_PUBLISHED_PER_SHOW {
        return Err(AppError::BadRequest(format!(
            "you can publish at most {MAX_PUBLISHED_PER_SHOW} guides per show"
        )));
    }

    let mut tx = pool.begin().await?;
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO skip_guide (show_id, author_id, title, description, is_published)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
        show_id,
        author_id,
        title,
        description,
        input.published,
    )
    .fetch_one(&mut *tx)
    .await?;
    write_entries(&mut tx, id, input).await?;
    tx.commit().await?;
    Ok(id)
}

/// Update a guide (author only). Returns NotFound/Forbidden as appropriate.
pub async fn update_guide(
    pool: &PgPool,
    guide_id: Uuid,
    user_id: Uuid,
    input: &GuideInput,
) -> Result<(), AppError> {
    let meta = guide_meta(pool, guide_id)
        .await?
        .ok_or_else(|| AppError::NotFound("guide not found".into()))?;
    if meta.author_id != Some(user_id) {
        return Err(AppError::Forbidden);
    }
    let (title, description) = input.clean()?;
    validate_entries(pool, meta.show_id, input).await?;
    if input.published
        && count_published(pool, meta.show_id, user_id, Some(guide_id)).await? >= MAX_PUBLISHED_PER_SHOW
    {
        return Err(AppError::BadRequest(format!(
            "you can publish at most {MAX_PUBLISHED_PER_SHOW} guides per show"
        )));
    }

    let mut tx = pool.begin().await?;
    sqlx::query!(
        r#"
        UPDATE skip_guide
        SET title = $2, description = $3, is_published = $4, updated_at = now()
        WHERE id = $1
        "#,
        guide_id,
        title,
        description,
        input.published,
    )
    .execute(&mut *tx)
    .await?;
    write_entries(&mut tx, guide_id, input).await?;
    tx.commit().await?;
    Ok(())
}

/// Delete a guide (author only). Entries and likes cascade.
pub async fn delete_guide(pool: &PgPool, guide_id: Uuid, user_id: Uuid) -> Result<(), AppError> {
    let meta = guide_meta(pool, guide_id)
        .await?
        .ok_or_else(|| AppError::NotFound("guide not found".into()))?;
    if meta.author_id != Some(user_id) {
        return Err(AppError::Forbidden);
    }
    sqlx::query!("DELETE FROM skip_guide WHERE id = $1", guide_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Published guides for a show, most-liked first, with per-viewer like/ownership.
pub async fn list_published(
    pool: &PgPool,
    show_id: Uuid,
    viewer: Option<Uuid>,
) -> Result<Vec<GuideSummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT
            g.id,
            g.title,
            g.description,
            g.is_published,
            g.like_count::bigint AS "like_count!",
            COALESCE(u.screen_name, u.display_name) AS author_name,
            COUNT(e.episode_id) FILTER (WHERE e.disposition = 'WATCH')::bigint    AS "watch_count!",
            COUNT(e.episode_id) FILTER (WHERE e.disposition = 'OPTIONAL')::bigint AS "optional_count!",
            COUNT(e.episode_id) FILTER (WHERE e.disposition = 'SKIP')::bigint     AS "skip_count!",
            EXISTS(SELECT 1 FROM skip_guide_like l WHERE l.guide_id = g.id AND l.user_id = $2) AS "my_like!",
            COALESCE(g.author_id = $2, FALSE) AS "mine!"
        FROM skip_guide g
        LEFT JOIN app_user u ON u.id = g.author_id
        LEFT JOIN skip_guide_entry e ON e.guide_id = g.id
        WHERE g.show_id = $1 AND g.is_published = TRUE
        GROUP BY g.id, u.screen_name, u.display_name
        ORDER BY g.like_count DESC, g.created_at DESC
        "#,
        show_id,
        viewer,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| GuideSummary {
            id: r.id,
            title: r.title,
            description: r.description,
            author_name: r.author_name,
            like_count: r.like_count,
            watch_count: r.watch_count,
            optional_count: r.optional_count,
            skip_count: r.skip_count,
            is_published: r.is_published,
            my_like: r.my_like,
            mine: r.mine,
        })
        .collect())
}

/// Full guide detail with its ordered entries. None if the guide doesn't exist.
pub async fn get_guide(
    pool: &PgPool,
    guide_id: Uuid,
    viewer: Option<Uuid>,
) -> Result<Option<GuideDetail>, sqlx::Error> {
    let header = sqlx::query!(
        r#"
        SELECT
            g.id,
            g.show_id,
            s.slug AS show_slug,
            s.name AS show_name,
            s.poster_path,
            g.title,
            g.description,
            g.is_published,
            g.like_count::bigint AS "like_count!",
            COALESCE(u.screen_name, u.display_name) AS author_name,
            EXISTS(SELECT 1 FROM skip_guide_like l WHERE l.guide_id = g.id AND l.user_id = $2) AS "my_like!",
            COALESCE(g.author_id = $2, FALSE) AS "mine!"
        FROM skip_guide g
        JOIN show s ON s.id = g.show_id
        LEFT JOIN app_user u ON u.id = g.author_id
        WHERE g.id = $1
        "#,
        guide_id,
        viewer,
    )
    .fetch_optional(pool)
    .await?;

    let Some(h) = header else { return Ok(None) };

    let entries = sqlx::query!(
        r#"
        SELECT e.episode_id, ep.season_number, ep.episode_number, ep.name,
               e.disposition::text AS "disposition!"
        FROM skip_guide_entry e
        JOIN episode ep ON ep.id = e.episode_id
        WHERE e.guide_id = $1
        ORDER BY ep.season_number, ep.episode_number
        "#,
        guide_id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .filter_map(|r| {
        Disposition::from_db(&r.disposition).map(|disposition| GuideEntryView {
            episode_id: r.episode_id,
            season_number: r.season_number,
            episode_number: r.episode_number,
            name: r.name,
            disposition,
        })
    })
    .collect();

    Ok(Some(GuideDetail {
        id: h.id,
        show_id: h.show_id,
        show_slug: h.show_slug,
        show_name: h.show_name,
        poster_path: h.poster_path,
        title: h.title,
        description: h.description,
        author_name: h.author_name,
        like_count: h.like_count,
        is_published: h.is_published,
        my_like: h.my_like,
        mine: h.mine,
        entries,
    }))
}

/// All guides authored by a user (published and drafts), newest-updated first,
/// for their account page.
pub async fn list_by_author(pool: &PgPool, author_id: Uuid) -> Result<Vec<MyGuide>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT g.id, g.title, g.is_published, g.like_count::bigint AS "like_count!",
               s.slug AS show_slug, s.name AS show_name
        FROM skip_guide g
        JOIN show s ON s.id = g.show_id
        WHERE g.author_id = $1
        ORDER BY g.is_published DESC, g.updated_at DESC
        "#,
        author_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| MyGuide {
            id: r.id,
            title: r.title,
            is_published: r.is_published,
            like_count: r.like_count,
            show_slug: r.show_slug,
            show_name: r.show_name,
        })
        .collect())
}

/// Add the user's like (idempotent). Returns the fresh like count.
pub async fn like_guide(pool: &PgPool, guide_id: Uuid, user_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query!(
        "INSERT INTO skip_guide_like (guide_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        guide_id,
        user_id,
    )
    .execute(pool)
    .await?;
    like_count(pool, guide_id).await
}

/// Remove the user's like (idempotent). Returns the fresh like count.
pub async fn unlike_guide(pool: &PgPool, guide_id: Uuid, user_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query!(
        "DELETE FROM skip_guide_like WHERE guide_id = $1 AND user_id = $2",
        guide_id,
        user_id,
    )
    .execute(pool)
    .await?;
    like_count(pool, guide_id).await
}

async fn like_count(pool: &PgPool, guide_id: Uuid) -> Result<i64, sqlx::Error> {
    let n = sqlx::query_scalar!(
        r#"SELECT like_count::bigint AS "n!" FROM skip_guide WHERE id = $1"#,
        guide_id
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
}

#[cfg(all(test, feature = "integration"))]
mod tests {
    use super::*;
    use crate::db;

    #[sqlx::test]
    async fn create_like_publish_cap_and_anonymize(pool: sqlx::PgPool) {
        let show = db::upsert_show(
            &pool, 9100, "Guide Test", "guide-test", Some(2020), None, None, None, None,
        )
        .await
        .unwrap();
        let season = db::upsert_season(&pool, show, 1, Some("S1")).await.unwrap();
        for n in 1i32..=3 {
            db::upsert_episode(
                &pool, show, season, 8100 + n as i64, 1, n,
                Some(&format!("E{n}")), None, None, None, None, None, None,
            )
            .await
            .unwrap();
        }
        let eps = db::episodes_with_scores(&pool, show, Some(1), None).await.unwrap();
        let (author, _) =
            db::resolve_oauth_user(&pool, "test", "sub-author", "author@test.local", Some("Author"))
                .await
                .unwrap();
        let (liker, _) =
            db::resolve_oauth_user(&pool, "test", "sub-liker", "liker@test.local", Some("Liker"))
                .await
                .unwrap();

        let input = |title: &str, published: bool| GuideInput {
            title: title.into(),
            description: Some("desc".into()),
            entries: eps
                .iter()
                .map(|e| GuideEntryInput { episode_id: e.id, disposition: Disposition::Watch })
                .collect(),
            published,
        };

        let gid = create_guide(&pool, show, author, &input("My Guide", true)).await.unwrap();

        // Published, with the right per-disposition counts, visible to others.
        let listed = list_published(&pool, show, Some(liker)).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].watch_count, 3);
        assert!(!listed[0].my_like);
        assert!(!listed[0].mine);

        // Like is idempotent and the trigger keeps the count current.
        assert_eq!(like_guide(&pool, gid, liker).await.unwrap(), 1);
        assert_eq!(like_guide(&pool, gid, liker).await.unwrap(), 1);
        assert_eq!(unlike_guide(&pool, gid, liker).await.unwrap(), 0);

        // Only the author can edit or delete.
        assert!(matches!(
            update_guide(&pool, gid, liker, &input("Hijack", true)).await,
            Err(AppError::Forbidden)
        ));
        assert!(matches!(
            delete_guide(&pool, gid, liker).await,
            Err(AppError::Forbidden)
        ));

        // Publish cap: 1 already published + 4 more = 5; the 6th is rejected.
        for i in 2..=5 {
            create_guide(&pool, show, author, &input(&format!("G{i}"), true)).await.unwrap();
        }
        assert!(matches!(
            create_guide(&pool, show, author, &input("G6", true)).await,
            Err(AppError::BadRequest(_))
        ));
        // A draft (unpublished) doesn't count against the cap.
        create_guide(&pool, show, author, &input("Draft", false)).await.unwrap();

        // Deleting the author DELETES their guides (FK ON DELETE CASCADE).
        db::delete_user(&pool, author).await.unwrap();
        assert!(get_guide(&pool, gid, None).await.unwrap().is_none());
    }
}
