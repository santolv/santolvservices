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
use time::OffsetDateTime;
use uuid::Uuid;

use super::record_support::*;
pub const SERVICE: Service = Service {
    id: "tasks",
    schema: "tasks",
    page: "/tasks",
    html: include_str!("../../../../frontend/tasks/index.html"),
    uses_file_store: false,
    requires: None,
    worker: |_, _| Box::pin(async {}),
    routes,
};
fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/tasks", get(list_tasks).post(create_task))
        .route(
            "/api/v1/tasks/{id}",
            get(get_task).put(update_task).delete(delete_task),
        )
        .route(
            "/api/v1/notes-tasks/tasks",
            get(list_tasks).post(create_task),
        )
        .route(
            "/api/v1/notes-tasks/tasks/{id}",
            get(get_task).put(update_task).delete(delete_task),
        )
        .layer(DefaultBodyLimit::max(640 * 1024))
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
struct TasksQuery {
    status: Option<TaskStatus>,
    priority: Option<Priority>,
    note_id: Option<Uuid>,
    #[serde(flatten)]
    list: ListQuery,
}

fn due_at(value: Option<&str>) -> Result<Option<OffsetDateTime>> {
    value
        .map(|value| parse_date(value).ok_or_else(|| Error::bad("invalid_due_at")))
        .transpose()
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
         FROM tasks.tasks
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
    if let Some(note_id) = input.note_id {
        super::notes::require_link(&app, &user, note_id).await?;
    }
    let task = sqlx::query_as(
        "INSERT INTO tasks.tasks (owner_id, id, title, status, priority, due_at, note_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
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
         FROM tasks.tasks WHERE owner_id=$1 AND id=$2",
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
    if let Some(note_id) = input.note_id {
        let previous: Option<Option<Uuid>> =
            sqlx::query_scalar("SELECT note_id FROM tasks.tasks WHERE owner_id=$1 AND id=$2")
                .bind(user.id)
                .bind(id_from_path(&id)?)
                .fetch_optional(&app.db)
                .await?;
        if previous != Some(Some(note_id)) {
            super::notes::require_link(&app, &user, note_id).await?;
        }
    }
    let task = sqlx::query_as(
        "UPDATE tasks.tasks SET title=$3, status=$4, priority=$5, due_at=$6,
           note_id=$7, updated_at=statement_timestamp()
         WHERE owner_id=$1 AND id=$2
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
    let deleted = sqlx::query("DELETE FROM tasks.tasks WHERE owner_id=$1 AND id=$2")
        .bind(user.id)
        .bind(id_from_path(&id)?)
        .execute(&app.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(Error::denied());
    }
    Ok(StatusCode::NO_CONTENT)
}
