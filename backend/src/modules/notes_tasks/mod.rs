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
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::FromRow;
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use uuid::Uuid;

#[cfg(test)]
mod tests;

pub const SERVICE: Service = Service {
    id: "notes-tasks",
    schema: "notes_tasks",
    page: "/notes-tasks",
    html: include_str!("../../../../frontend/notes-tasks/index.html"),
    uses_file_store: false,
    requires: None,
    worker: |_, _| Box::pin(async {}),
    routes,
};

fn routes() -> Router<App> {
    Router::new()
        .route(
            "/api/v1/notes-tasks/notes",
            get(list_notes).post(create_note),
        )
        .route(
            "/api/v1/notes-tasks/notes/{id}",
            get(get_note).put(update_note),
        )
        .route(
            "/api/v1/notes-tasks/tasks",
            get(list_tasks).post(create_task),
        )
        .route(
            "/api/v1/notes-tasks/tasks/{id}",
            get(get_task).put(update_task).delete(delete_task),
        )
        // A Unicode scalar can take 12 wire bytes as a JSON surrogate pair.
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

#[derive(FromRow, Serialize)]
struct Task {
    id: Uuid,
    title: String,
    status: String,
    priority: String,
    #[serde(with = "time::serde::rfc3339::option")]
    due_at: Option<OffsetDateTime>,
    note_id: Option<Uuid>,
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

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    #[default]
    Pending,
    InProgress,
    Done,
}

impl TaskStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Done => "done",
        }
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Priority {
    Low,
    #[default]
    Normal,
    High,
}

impl Priority {
    fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewTask {
    title: String,
    #[serde(default)]
    status: TaskStatus,
    #[serde(default)]
    priority: Priority,
    due_at: Option<String>,
    note_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditTask {
    title: String,
    status: TaskStatus,
    priority: Priority,
    #[serde(deserialize_with = "nullable")]
    due_at: Option<String>,
    #[serde(deserialize_with = "nullable")]
    note_id: Option<Uuid>,
}

fn nullable<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    value: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::deserialize(value)
}

#[derive(Deserialize)]
struct ListQuery {
    q: Option<String>,
    cursor: Option<String>,
    limit: Option<String>,
}

#[derive(Deserialize)]
struct NotesQuery {
    #[serde(default)]
    archived: bool,
    #[serde(flatten)]
    list: ListQuery,
}

#[derive(Deserialize)]
struct TasksQuery {
    status: Option<TaskStatus>,
    priority: Option<Priority>,
    note_id: Option<Uuid>,
    #[serde(flatten)]
    list: ListQuery,
}

struct ListOptions {
    pattern: Option<String>,
    before_time: Option<OffsetDateTime>,
    before_id: Option<Uuid>,
    limit: i64,
}

impl ListQuery {
    fn options(self) -> Result<ListOptions> {
        let limit = self
            .limit
            .as_deref()
            .unwrap_or("50")
            .parse()
            .map_err(|_| Error::bad("invalid_limit"))?;
        if !(1..=100).contains(&limit) {
            return Err(Error::bad("invalid_limit"));
        }

        let pattern = self
            .q
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(search_pattern)
            .transpose()?;
        let (before_time, before_id) = match self.cursor {
            Some(cursor) => {
                let invalid = || Error::bad("invalid_cursor");
                let (date, id) = cursor.split_once('|').ok_or_else(invalid)?;
                let date = parse_date(date).ok_or_else(invalid)?;
                let id = Uuid::parse_str(id).map_err(|_| invalid())?;
                (Some(date), Some(id))
            }
            None => (None, None),
        };
        Ok(ListOptions {
            pattern,
            before_time,
            before_id,
            limit,
        })
    }
}

#[derive(Serialize)]
struct Page<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
}

fn page<T>(
    mut items: Vec<T>,
    limit: i64,
    key: impl Fn(&T) -> (OffsetDateTime, Uuid),
) -> Result<Json<Page<T>>> {
    let has_more = items.len() > limit as usize;
    items.truncate(limit as usize);
    let next_cursor = items
        .last()
        .filter(|_| has_more)
        .map(|item| {
            let (date, id) = key(item);
            date.format(&Rfc3339)
                .map(|date| format!("{date}|{id}"))
                .map_err(|_| Error::unavailable())
        })
        .transpose()?;
    Ok(Json(Page { items, next_cursor }))
}

fn search_pattern(q: &str) -> Result<String> {
    if q.chars().count() > 200 || q.contains('\0') {
        return Err(Error::bad("invalid_search"));
    }
    let mut pattern = String::from("%");
    for character in q.chars() {
        if matches!(character, '!' | '%' | '_') {
            pattern.push('!');
        }
        pattern.push(character);
    }
    pattern.push('%');
    Ok(pattern)
}

fn validate_title(title: &str) -> Result<&str> {
    let title = title.trim();
    if !(1..=200).contains(&title.chars().count()) || title.contains('\0') {
        return Err(Error::bad("invalid_title"));
    }
    Ok(title)
}

fn validate_content(content: &str) -> Result<()> {
    if content.chars().count() > 50_000 || content.contains('\0') {
        return Err(Error::bad("invalid_content"));
    }
    Ok(())
}

fn parse_date(value: &str) -> Option<OffsetDateTime> {
    let date = OffsetDateTime::parse(value, &Rfc3339)
        .ok()?
        .checked_to_offset(UtcOffset::UTC)?;
    (0..=9999).contains(&date.year()).then_some(date)
}

fn due_at(value: Option<&str>) -> Result<Option<OffsetDateTime>> {
    value
        .map(|value| parse_date(value).ok_or_else(|| Error::bad("invalid_due_at")))
        .transpose()
}

fn id_from_path(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| Error::bad("invalid_id"))
}

fn body<T>(value: std::result::Result<Json<T>, JsonRejection>) -> Result<T> {
    value
        .map(|Json(value)| value)
        .map_err(|rejection| match rejection.status() {
            StatusCode::PAYLOAD_TOO_LARGE => {
                Error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")
            }
            StatusCode::UNSUPPORTED_MEDIA_TYPE => {
                Error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required")
            }
            _ => Error::bad("invalid_json"),
        })
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
         FROM notes_tasks.notes
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
        "INSERT INTO notes_tasks.notes (id, owner_id, title, content) VALUES ($1, $2, $3, $4)
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
         FROM notes_tasks.notes WHERE owner_id=$1 AND id=$2",
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
        "UPDATE notes_tasks.notes SET title=$3, content=$4, updated_at=statement_timestamp(),
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

async fn list_tasks(
    State(app): State<App>,
    user: User,
    query: std::result::Result<Query<TasksQuery>, QueryRejection>,
) -> Result<Json<Page<Task>>> {
    let Query(query) = query.map_err(|_| Error::bad("invalid_query"))?;
    let options = query.list.options()?;
    let items = sqlx::query_as(
        "SELECT id, title, status, priority, due_at, note_id, created_at, updated_at
         FROM notes_tasks.tasks
         WHERE owner_id=$1 AND ($2::text IS NULL OR status=$2)
           AND ($3::text IS NULL OR priority=$3)
           AND ($4::uuid IS NULL OR note_id=$4)
           AND ($5::text IS NULL OR title ILIKE $5 ESCAPE '!')
           AND ($6::timestamptz IS NULL OR (created_at, id) < ($6, $7))
         ORDER BY created_at DESC, id DESC LIMIT $8",
    )
    .bind(user.id)
    .bind(query.status.map(TaskStatus::as_str))
    .bind(query.priority.map(Priority::as_str))
    .bind(query.note_id)
    .bind(options.pattern)
    .bind(options.before_time)
    .bind(options.before_id)
    .bind(options.limit + 1)
    .fetch_all(&app.db)
    .await?;
    page(items, options.limit, |task: &Task| {
        (task.created_at, task.id)
    })
}

async fn create_task(
    State(app): State<App>,
    user: User,
    input: std::result::Result<Json<NewTask>, JsonRejection>,
) -> Result<(StatusCode, Json<Task>)> {
    let input = body(input)?;
    let task = sqlx::query_as(
        "INSERT INTO notes_tasks.tasks (owner_id, id, title, status, priority, due_at, note_id)
         SELECT $1, $2, $3, $4, $5, $6, $7
         WHERE $7::uuid IS NULL OR EXISTS (
           SELECT 1 FROM notes_tasks.notes WHERE owner_id=$1 AND id=$7
         )
         RETURNING id, title, status, priority, due_at, note_id, created_at, updated_at",
    )
    .bind(user.id)
    .bind(Uuid::new_v4())
    .bind(validate_title(&input.title)?)
    .bind(input.status.as_str())
    .bind(input.priority.as_str())
    .bind(due_at(input.due_at.as_deref())?)
    .bind(input.note_id)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Error::denied)?;
    Ok((StatusCode::CREATED, Json(task)))
}

async fn get_task(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
) -> Result<Json<Task>> {
    let task = sqlx::query_as(
        "SELECT id, title, status, priority, due_at, note_id, created_at, updated_at
         FROM notes_tasks.tasks WHERE owner_id=$1 AND id=$2",
    )
    .bind(user.id)
    .bind(id_from_path(&id)?)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Error::denied)?;
    Ok(Json(task))
}

async fn update_task(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
    input: std::result::Result<Json<EditTask>, JsonRejection>,
) -> Result<Json<Task>> {
    let input = body(input)?;
    let task = sqlx::query_as(
        "UPDATE notes_tasks.tasks SET title=$3, status=$4, priority=$5, due_at=$6,
           note_id=$7, updated_at=statement_timestamp()
         WHERE owner_id=$1 AND id=$2 AND ($7::uuid IS NULL OR EXISTS (
           SELECT 1 FROM notes_tasks.notes WHERE owner_id=$1 AND id=$7
         ))
         RETURNING id, title, status, priority, due_at, note_id, created_at, updated_at",
    )
    .bind(user.id)
    .bind(id_from_path(&id)?)
    .bind(validate_title(&input.title)?)
    .bind(input.status.as_str())
    .bind(input.priority.as_str())
    .bind(due_at(input.due_at.as_deref())?)
    .bind(input.note_id)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Error::denied)?;
    Ok(Json(task))
}

async fn delete_task(
    State(app): State<App>,
    user: User,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let deleted = sqlx::query("DELETE FROM notes_tasks.tasks WHERE owner_id=$1 AND id=$2")
        .bind(user.id)
        .bind(id_from_path(&id)?)
        .execute(&app.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(Error::denied());
    }
    Ok(StatusCode::NO_CONTENT)
}
