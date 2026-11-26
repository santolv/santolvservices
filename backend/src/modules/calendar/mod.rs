use crate::{
    App,
    auth::User,
    error::{Error, Result},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    routing::{get, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

mod events;
mod links;

pub const SERVICE: crate::core::manager::Service = crate::core::manager::Service {
    id: "calendar",
    schema: "calendar",
    page: "/calendar",
    html: include_str!("../../../../frontend/calendar/index.html"),
    uses_file_store: false,
    requires: None,
    worker: |_, _| Box::pin(async {}),
    routes,
};

fn routes() -> Router<App> {
    Router::new().nest(
        "/api/v1/calendar",
        Router::new()
            .route("/calendars", get(calendars).post(create_calendar))
            .route(
                "/calendars/{id}",
                put(rename_calendar).delete(delete_calendar),
            )
            .route("/calendars/{id}/members", get(members).put(grant))
            .route(
                "/calendars/{id}/members/{user}",
                axum::routing::delete(revoke),
            )
            .route("/events", get(events::list).post(events::create))
            .route(
                "/events/{id}/links/{service}",
                get(links::get).put(links::put),
            )
            .route(
                "/events/{id}",
                get(events::get).put(events::update).delete(events::remove),
            )
            .route(
                "/events/{id}/occurrences/{original}",
                get(events::occurrence).put(events::edit_occurrence),
            )
            .route("/events/{id}/reminder", put(events::set_reminder))
            .route("/reminders", get(events::reminders).post(events::dismiss))
            .layer(DefaultBodyLimit::max(32 * 1024)),
    )
}

// Serialize mutations and membership changes on the same calendar row.
async fn lock_access(db: &mut PgConnection, id: Uuid, user: Uuid, owner_only: bool) -> Result<()> {
    let owner: Uuid = sqlx::query_scalar("SELECT owner_id FROM calendars WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *db)
        .await?
        .ok_or_else(Error::denied)?;
    if owner == user {
        return Ok(());
    }
    if !owner_only && sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM members WHERE calendar_id=$1 AND user_id=$2 AND role='editor')"
    ).bind(id).bind(user).fetch_one(db).await? {
        return Ok(());
    }
    Err(Error::denied())
}

async fn owner_access(db: &PgPool, id: Uuid, user: Uuid) -> Result<()> {
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM calendars WHERE id=$1 AND owner_id=$2)",
    )
    .bind(id)
    .bind(user)
    .fetch_one(db)
    .await?
    {
        return Ok(());
    }
    Err(Error::denied())
}

async fn calendars(State(app): State<App>, user: User) -> Result<Json<Value>> {
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',c.id,'name',c.name,'is_default',c.is_default AND c.owner_id=$1,
                    'role',CASE WHEN c.owner_id=$1 THEN 'owner' ELSE m.role END)
         FROM calendars c LEFT JOIN members m ON m.calendar_id=c.id AND m.user_id=$1
         WHERE c.owner_id=$1 OR m.user_id IS NOT NULL
         ORDER BY (c.is_default AND c.owner_id=$1) DESC,c.name,c.id"
    ).bind(user.id).fetch_all(&app.db).await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
struct CalendarInput {
    name: String,
}

fn name(input: &CalendarInput) -> Result<&str> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.contains('\0') {
        return Err(Error::bad("invalid_calendar_name"));
    }
    Ok(name)
}

async fn create_calendar(
    State(app): State<App>,
    user: User,
    Json(input): Json<CalendarInput>,
) -> Result<(StatusCode, Json<Value>)> {
    let id = Uuid::new_v4();
    let name = name(&input)?;
    sqlx::query("INSERT INTO calendars (id,owner_id,name) VALUES ($1,$2,$3)")
        .bind(id)
        .bind(user.id)
        .bind(name)
        .execute(&app.db)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "name": name, "role": "owner", "is_default": false })),
    ))
}

async fn rename_calendar(
    State(app): State<App>,
    user: User,
    Path(id): Path<Uuid>,
    Json(input): Json<CalendarInput>,
) -> Result<StatusCode> {
    let result = sqlx::query("UPDATE calendars SET name=$3 WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .bind(name(&input)?)
        .execute(&app.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(Error::denied());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_calendar(
    State(app): State<App>,
    user: User,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let result = sqlx::query("DELETE FROM calendars WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&app.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(Error::denied());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn members(State(app): State<App>, user: User, Path(id): Path<Uuid>) -> Result<Json<Value>> {
    owner_access(&app.db, id, user.id).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('user_id',m.user_id,'username',u.username,'role',m.role)
         FROM members m JOIN public.users u ON u.id=m.user_id
         JOIN calendars c ON c.id=m.calendar_id AND c.owner_id=$2
         WHERE m.calendar_id=$1 ORDER BY u.username",
    )
    .bind(id)
    .bind(user.id)
    .fetch_all(&app.db)
    .await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
struct MemberInput {
    username: String,
    role: String,
}

async fn grant(
    State(app): State<App>,
    user: User,
    Path(id): Path<Uuid>,
    Json(input): Json<MemberInput>,
) -> Result<StatusCode> {
    if !matches!(input.role.as_str(), "editor" | "reader") {
        return Err(Error::bad("invalid_calendar_role"));
    }
    let mut tx = app.db.begin().await?;
    lock_access(&mut tx, id, user.id, true).await?;
    let target: Uuid = sqlx::query_scalar(
        "SELECT id FROM public.users WHERE username=$1 AND account_type='user' AND NOT disabled AND id<>$2"
    ).bind(input.username.trim().to_lowercase()).bind(user.id).fetch_optional(&mut *tx).await?.ok_or_else(Error::denied)?;
    sqlx::query(
        "INSERT INTO members VALUES ($1,$2,$3)
                 ON CONFLICT (calendar_id,user_id) DO UPDATE SET role=excluded.role",
    )
    .bind(id)
    .bind(target)
    .bind(input.role)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke(
    State(app): State<App>,
    user: User,
    Path((id, target)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode> {
    let mut tx = app.db.begin().await?;
    lock_access(&mut tx, id, user.id, true).await?;
    sqlx::query("DELETE FROM members WHERE calendar_id=$1 AND user_id=$2")
        .bind(id)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM reminders r USING events e WHERE e.calendar_id=$1 AND r.event_id=e.id AND r.user_id=$2")
        .bind(id).bind(target).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM dismissals d USING events e WHERE e.calendar_id=$1 AND d.event_id=e.id AND d.user_id=$2")
        .bind(id).bind(target).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
