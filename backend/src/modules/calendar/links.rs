//! Personal links do not expose private records to other calendar members.
use crate::{
    App,
    auth::User,
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
async fn access(app: &App, user: &User, event: Uuid, service: &str) -> Result<()> {
    if !matches!(service, "notes" | "tasks") {
        return Err(Error::bad("invalid_service"));
    }
    let visible:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM calendar.events e JOIN calendar.calendars c ON c.id=e.calendar_id LEFT JOIN calendar.members m ON m.calendar_id=c.id AND m.user_id=$1 WHERE e.id=$2 AND (c.owner_id=$1 OR m.user_id IS NOT NULL))").bind(user.id).bind(event).fetch_one(&app.db).await?;
    if !visible {
        return Err(Error::denied());
    }
    crate::core::manager::require_access(&app.db, service, user.id).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Link {
    id: Option<Uuid>,
}
pub(super) async fn get(
    State(app): State<App>,
    user: User,
    Path((event, service)): Path<(Uuid, String)>,
) -> Result<Json<Value>> {
    access(&app, &user, event, &service).await?;
    let row: Option<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT note_id,task_id FROM calendar.record_links WHERE event_id=$1 AND user_id=$2",
    )
    .bind(event)
    .bind(user.id)
    .fetch_optional(&app.db)
    .await?;
    let id = row.and_then(|(note, task)| if service == "notes" { note } else { task });
    if let Some(id) = id {
        if service == "notes" {
            super::super::notes::require_link(&app, &user, id).await?
        } else {
            super::super::tasks::require_link(&app, &user, id).await?
        }
    }
    Ok(Json(json!({"id":id})))
}
pub(super) async fn put(
    State(app): State<App>,
    user: User,
    Path((event, service)): Path<(Uuid, String)>,
    Json(input): Json<Link>,
) -> Result<Json<Value>> {
    access(&app, &user, event, &service).await?;
    if let Some(id) = input.id {
        if service == "notes" {
            super::super::notes::require_link(&app, &user, id).await?
        } else {
            super::super::tasks::require_link(&app, &user, id).await?
        }
    }
    let sql = if service == "notes" {
        "INSERT INTO calendar.record_links(event_id,user_id,note_id) VALUES($1,$2,$3) ON CONFLICT(event_id,user_id) DO UPDATE SET note_id=EXCLUDED.note_id"
    } else {
        "INSERT INTO calendar.record_links(event_id,user_id,task_id) VALUES($1,$2,$3) ON CONFLICT(event_id,user_id) DO UPDATE SET task_id=EXCLUDED.task_id"
    };
    sqlx::query(sql)
        .bind(event)
        .bind(user.id)
        .bind(input.id)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"id":input.id})))
}
