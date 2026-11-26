use crate::{
    App,
    auth::User,
    core::manager::Service,
    error::{Error, Result},
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

use super::record_support::*;
#[cfg(test)]
mod tests;
pub const SERVICE: Service = Service {
    id: "notes",
    schema: "notes",
    page: "/notes",
    html: include_str!("../../../../frontend/notes/index.html"),
    uses_file_store: false,
    requires: None,
    worker: |_, _| Box::pin(async {}),
    routes,
};
fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/notes", get(list_notes).post(create_note))
        .route(
            "/api/v1/notes/{id}",
            get(get_note).put(update_note).delete(delete_note),
        )
        .route(
            "/api/v1/notes-tasks/notes",
            get(list_notes).post(create_note),
        )
        .route(
            "/api/v1/notes-tasks/notes/{id}",
            get(get_note).put(update_note).delete(delete_note),
        )
        .layer(DefaultBodyLimit::max(640 * 1024))
}
#[derive(FromRow, Serialize)]
struct Note {
    id: Uuid,
    title: String,
    content: String,
    #[serde(with = "time::serde::rfc3339::option")]
    archived_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: OffsetDateTime,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewNote {
    title: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditNote {
    title: String,
    content: String,
    archived: bool,
}

#[derive(Deserialize)]
struct NotesQuery {
    #[serde(default)]
    archived: bool,
    #[serde(flatten)]
    list: ListQuery,
}

fn validate_content(content: &str) -> Result<()> {
    if content.chars().count() > 50_000 || content.contains('\0') {
        return Err(Error::bad("invalid_content"));
    }
    Ok(())
}

async fn list_notes(
    State(app): State<App>,
    user: User,
    query: std::result::Result<Query<NotesQuery>, QueryRejection>,
) -> Result<Json<Page<Note>>> {
    let Query(query) = query.map_err(|_| Error::bad("invalid_query"))?;
    let options = query.list.options()?;
    let items = sqlx::query_as(
        "SELECT id, title, content, archived_at, created_at, updated_at
         FROM notes.notes
         WHERE owner_id=$1 AND (archived_at IS NOT NULL)=$2
           AND ($3::text IS NULL OR title ILIKE $3 ESCAPE '!' OR content ILIKE $3 ESCAPE '!')
           AND ($4::timestamptz IS NULL OR (created_at, id) < ($4, $5))
         ORDER BY created_at DESC, id DESC LIMIT $6",
    )
    .bind(user.id)
    .bind(query.archived)
    .bind(options.pattern)
    .bind(options.before_time)
    .bind(options.before_id)
    .bind(options.limit + 1)
    .fetch_all(&app.db)
    .await?;
    page(items, options.limit, |note: &Note| {
        (note.created_at, note.id)
    })
}

async fn create_note(
    State(app): State<App>,
    user: User,
    input: std::result::Result<Json<NewNote>, JsonRejection>,
) -> Result<(StatusCode, Json<Note>)> {
    let input = body(input)?;
    let title = validate_title(&input.title)?;
    validate_content(&input.content)?;
    let note = sqlx::query_as(
        "INSERT INTO notes.notes (id, owner_id, title, content) VALUES ($1, $2, $3, $4)
         RETURNING id, title, content, archived_at, created_at, updated_at",
    )
    .bind(Uuid::new_v4())
    .bind(user.id)
    .bind(title)
    .bind(input.content)
    .fetch_one(&app.db)
    .await?;
    Ok((StatusCode::CREATED, Json(note)))
}

async fn get_note(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
) -> Result<Json<Note>> {
    let note = sqlx::query_as(
        "SELECT id, title, content, archived_at, created_at, updated_at
         FROM notes.notes WHERE owner_id=$1 AND id=$2",
    )
    .bind(user.id)
    .bind(id_from_path(&id)?)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Error::denied)?;
    Ok(Json(note))
}

async fn update_note(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
    input: std::result::Result<Json<EditNote>, JsonRejection>,
) -> Result<Json<Note>> {
    let input = body(input)?;
    let title = validate_title(&input.title)?;
    validate_content(&input.content)?;
    let note = sqlx::query_as(
        "UPDATE notes.notes SET title=$3, content=$4, updated_at=statement_timestamp(),
           archived_at=CASE WHEN $5 THEN coalesce(archived_at, statement_timestamp()) END
         WHERE owner_id=$1 AND id=$2
         RETURNING id, title, content, archived_at, created_at, updated_at",
    )
    .bind(user.id)
    .bind(id_from_path(&id)?)
    .bind(title)
    .bind(input.content)
    .bind(input.archived)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Error::denied)?;
    Ok(Json(note))
}

pub(super) async fn require_link(app: &App, user: &User, id: Uuid) -> Result<()> {
    crate::core::manager::require_access(&app.db, "notes", user.id).await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM notes.notes WHERE owner_id=$1 AND id=$2)")
            .bind(user.id)
            .bind(id)
            .fetch_one(&app.db)
            .await?;
    if !exists {
        return Err(Error::denied());
    }
    Ok(())
}

async fn delete_note(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let id = id_from_path(&id)?;
    let mut tx = app.db.begin().await?;
    super::tasks::unlink_note(&mut tx, user.id, id).await?;
    let deleted = sqlx::query("DELETE FROM notes.notes WHERE owner_id=$1 AND id=$2")
        .bind(user.id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(Error::denied());
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
