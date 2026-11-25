use super::lock_access;
use crate::{
    App,
    auth::User,
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use time::{
    Date, Duration, OffsetDateTime, PrimitiveDateTime,
    format_description::{self, well_known::Rfc3339},
};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
pub(super) struct EventInput {
    calendar_id: Option<Uuid>,
    #[serde(flatten)]
    fields: EventFields,
    repeat_kind: Option<String>,
    repeat_until: Option<String>,
    repeat_count: Option<i32>,
    reminder_minutes: Option<i32>,
    revision: Option<i64>,
    #[serde(default)]
    clear_exceptions: bool,
}

#[derive(Deserialize, Serialize)]
struct EventFields {
    title: String,
    #[serde(default)]
    description: String,
    local_start: String,
    local_end: String,
    zone: String,
    #[serde(default)]
    all_day: bool,
    #[serde(default = "busy_default")]
    busy: bool,
}

fn busy_default() -> bool {
    true
}

fn local(value: &str) -> Result<PrimitiveDateTime> {
    let format = if value.len() == 16 {
        "[year]-[month]-[day]T[hour]:[minute]"
    } else {
        "[year]-[month]-[day]T[hour]:[minute]:[second]"
    };
    let format = format_description::parse_borrowed::<2>(format).expect("local date format");
    let value =
        PrimitiveDateTime::parse(value, &format).map_err(|_| Error::bad("invalid_local_time"))?;
    if !(1900..=2200).contains(&value.year()) {
        return Err(Error::bad("invalid_local_time"));
    }
    Ok(value)
}

fn instant(value: &str) -> Result<OffsetDateTime> {
    let value =
        OffsetDateTime::parse(value, &Rfc3339).map_err(|_| Error::bad("invalid_instant"))?;
    if !(1899..=2201).contains(&value.year()) {
        return Err(Error::bad("invalid_instant"));
    }
    Ok(value)
}

async fn validate_fields(db: &PgPool, fields: &mut EventFields) -> Result<Date> {
    let start = local(&fields.local_start)?;
    let end = local(&fields.local_end)?;
    fields.title = fields.title.trim().to_owned();
    if fields.title.is_empty()
        || fields.title.chars().count() > 200
        || fields.title.contains('\0')
        || fields.description.chars().count() > 4000
        || fields.description.contains('\0')
    {
        return Err(Error::bad("invalid_event_text"));
    }
    if end <= start
        || end - start > Duration::days(31)
        || (fields.all_day
            && (start.time() != time::Time::MIDNIGHT || end.time() != time::Time::MIDNIGHT))
    {
        return Err(Error::bad("invalid_event_duration"));
    }
    let valid_zone: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name=$1)")
            .bind(&fields.zone)
            .fetch_one(db)
            .await?;
    if !valid_zone {
        return Err(Error::bad("invalid_time_zone"));
    }
    if !fields.all_day {
        let valid: bool = sqlx::query_scalar(
            "SELECT $1::timestamp=($1::timestamp AT TIME ZONE $3) AT TIME ZONE $3
                AND $2::timestamp=($2::timestamp AT TIME ZONE $3) AT TIME ZONE $3
                AND ($2::timestamp AT TIME ZONE $3)>($1::timestamp AT TIME ZONE $3)",
        )
        .bind(start)
        .bind(end)
        .bind(&fields.zone)
        .fetch_one(db)
        .await?;
        if !valid {
            return Err(Error::bad("invalid_zoned_time"));
        }
    }
    Ok(start.date())
}

async fn validate_input(db: &PgPool, input: &mut EventInput) -> Result<()> {
    let start = validate_fields(db, &mut input.fields).await?;
    let has_end = input.repeat_until.is_some() || input.repeat_count.is_some();
    if input
        .repeat_count
        .is_some_and(|value| !(1..=10000).contains(&value))
        || input.repeat_count.is_some() && input.repeat_until.is_some()
        || has_end && input.repeat_kind.is_none()
        || input
            .repeat_kind
            .as_deref()
            .is_some_and(|kind| !matches!(kind, "daily" | "weekly" | "monthly" | "yearly"))
    {
        return Err(Error::bad("invalid_repeat_rule"));
    }
    if let Some(until) = &input.repeat_until {
        let until = local(&format!("{until}T00:00"))?;
        if until.date() < start {
            return Err(Error::bad("invalid_repeat_rule"));
        }
    }
    validate_reminder(input.reminder_minutes)
}

fn validate_reminder(minutes: Option<i32>) -> Result<()> {
    if minutes.is_some_and(|value| !(0..=10080).contains(&value)) {
        return Err(Error::bad("invalid_reminder"));
    }
    Ok(())
}

async fn write_reminder(
    db: &mut PgConnection,
    event: Uuid,
    user: Uuid,
    minutes: Option<i32>,
) -> Result<()> {
    if let Some(minutes) = minutes {
        sqlx::query("INSERT INTO reminders VALUES ($1,$2,$3)
                     ON CONFLICT(event_id,user_id) DO UPDATE SET minutes_before=excluded.minutes_before")
            .bind(event).bind(user).bind(minutes).execute(db).await?;
    } else {
        sqlx::query("DELETE FROM reminders WHERE event_id=$1 AND user_id=$2")
            .bind(event)
            .bind(user)
            .execute(db)
            .await?;
    }
    Ok(())
}

async fn event_calendar(db: &mut PgConnection, id: Uuid) -> Result<Uuid> {
    sqlx::query_scalar("SELECT calendar_id FROM events WHERE id=$1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(Error::denied)
}

async fn check_revision(db: &mut PgConnection, id: Uuid, revision: i64) -> Result<()> {
    let current: i64 = sqlx::query_scalar("SELECT revision FROM events WHERE id=$1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(Error::denied)?;
    if current != revision {
        return Err(Error::conflict("event_changed"));
    }
    Ok(())
}

#[derive(Deserialize)]
pub(super) struct Range {
    from: String,
    to: String,
    calendar_id: Option<Uuid>,
    limit: Option<i64>,
    offset: Option<i64>,
}

pub(super) async fn list(
    State(app): State<App>,
    user: User,
    Query(q): Query<Range>,
) -> Result<Json<Value>> {
    let (from, to) = (instant(&q.from)?, instant(&q.to)?);
    let limit = q.limit.unwrap_or(100);
    let offset = q.offset.unwrap_or(0);
    if from >= to || to - from > Duration::days(62) || !(1..=200).contains(&limit) || offset < 0 {
        return Err(Error::bad("invalid_calendar_range"));
    }
    let mut items: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(o) FROM calendar.occurrences($1,$2,$3,$4,NULL,false) o
         ORDER BY starts_at,event_id,original_start LIMIT $5 OFFSET $6",
    )
    .bind(user.id)
    .bind(from)
    .bind(to)
    .bind(q.calendar_id)
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&app.db)
    .await?;
    let next = (items.len() as i64 > limit).then(|| offset.saturating_add(limit));
    items.truncate(limit as usize);
    Ok(Json(json!({ "items": items, "next_offset": next })))
}

pub(super) async fn get(
    State(app): State<App>,
    user: User,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let event: Value = sqlx::query_scalar(
        "SELECT to_jsonb(e) || jsonb_build_object('role',CASE WHEN c.owner_id=$1 THEN 'owner' ELSE m.role END,
                 'calendar_name',c.name,'reminder_minutes',r.minutes_before)
         FROM events e JOIN calendars c ON c.id=e.calendar_id
         LEFT JOIN members m ON m.calendar_id=c.id AND m.user_id=$1
         LEFT JOIN reminders r ON r.event_id=e.id AND r.user_id=$1
         WHERE e.id=$2 AND (c.owner_id=$1 OR m.user_id IS NOT NULL)"
    ).bind(user.id).bind(id).fetch_optional(&app.db).await?.ok_or_else(Error::denied)?;
    Ok(Json(event))
}

pub(super) async fn create(
    State(app): State<App>,
    user: User,
    Json(mut input): Json<EventInput>,
) -> Result<(StatusCode, Json<Value>)> {
    validate_input(&app.db, &mut input).await?;
    let mut tx = app.db.begin().await?;
    let calendar_id = if let Some(id) = input.calendar_id {
        lock_access(&mut tx, id, user.id, false).await?;
        id
    } else {
        sqlx::query_scalar(
            "INSERT INTO calendars (id,owner_id,name,is_default) VALUES ($1,$2,'Personal',true)
             ON CONFLICT (owner_id) WHERE is_default DO UPDATE SET is_default=true RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(user.id)
        .fetch_one(&mut *tx)
        .await?
    };
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO events (id,calendar_id,title,description,local_start,local_end,zone,all_day,busy,
                             repeat_kind,repeat_until,repeat_count)
         SELECT $1,$2,p.title,p.description,p.local_start,p.local_end,p.zone,p.all_day,p.busy,
                p.repeat_kind,p.repeat_until,p.repeat_count FROM jsonb_populate_record(NULL::events,$3) p"
    ).bind(id).bind(calendar_id).bind(sqlx::types::Json(&input)).execute(&mut *tx).await?;
    write_reminder(&mut tx, id, user.id, input.reminder_minutes).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "calendar_id": calendar_id, "revision": 1 })),
    ))
}

pub(super) async fn update(
    State(app): State<App>,
    user: User,
    Path(id): Path<Uuid>,
    Json(mut input): Json<EventInput>,
) -> Result<StatusCode> {
    validate_input(&app.db, &mut input).await?;
    let mut tx = app.db.begin().await?;
    let calendar_id = event_calendar(&mut tx, id).await?;
    lock_access(&mut tx, calendar_id, user.id, false).await?;
    check_revision(
        &mut tx,
        id,
        input
            .revision
            .ok_or_else(|| Error::bad("missing_revision"))?,
    )
    .await?;
    if input.calendar_id.is_some_and(|value| value != calendar_id) {
        return Err(Error::bad("calendar_change_unsupported"));
    }
    let payload = sqlx::types::Json(&input);
    let schedule_changed: bool = sqlx::query_scalar(
        "SELECT (e.local_start,e.local_end,e.zone,e.all_day,e.repeat_kind,e.repeat_until,e.repeat_count)
         IS DISTINCT FROM (p.local_start,p.local_end,p.zone,p.all_day,p.repeat_kind,p.repeat_until,p.repeat_count)
         FROM events e, jsonb_populate_record(NULL::events,$2) p WHERE e.id=$1"
    ).bind(id).bind(sqlx::types::Json(&input)).fetch_one(&mut *tx).await?;
    if schedule_changed {
        let has_exceptions: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM exceptions WHERE event_id=$1)")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if has_exceptions && !input.clear_exceptions {
            return Err(Error::conflict("confirm_clear_exceptions"));
        }
        sqlx::query("DELETE FROM exceptions WHERE event_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "UPDATE events e SET title=p.title,description=p.description,local_start=p.local_start,local_end=p.local_end,
             zone=p.zone,all_day=p.all_day,busy=p.busy,repeat_kind=p.repeat_kind,repeat_until=p.repeat_until,
             repeat_count=p.repeat_count,revision=e.revision+1
         FROM jsonb_populate_record(NULL::events,$2) p WHERE e.id=$1"
    ).bind(id).bind(payload).execute(&mut *tx).await?;
    write_reminder(&mut tx, id, user.id, input.reminder_minutes).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(super) struct Revision {
    revision: i64,
}

pub(super) async fn remove(
    State(app): State<App>,
    user: User,
    Path(id): Path<Uuid>,
    Query(q): Query<Revision>,
) -> Result<StatusCode> {
    let mut tx = app.db.begin().await?;
    let calendar_id = event_calendar(&mut tx, id).await?;
    lock_access(&mut tx, calendar_id, user.id, false).await?;
    check_revision(&mut tx, id, q.revision).await?;
    sqlx::query("DELETE FROM events WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn occurrence(
    State(app): State<App>,
    user: User,
    Path((id, original)): Path<(Uuid, String)>,
) -> Result<Json<Value>> {
    let original = local(&original)?;
    let item: Value = sqlx::query_scalar(
        "SELECT to_jsonb(o) || jsonb_build_object('reminder_minutes',r.minutes_before,'repeat_kind',e.repeat_kind)
         FROM events e LEFT JOIN exceptions x ON x.event_id=e.id AND x.original_start=$3
         LEFT JOIN reminders r ON r.event_id=e.id AND r.user_id=$1
         CROSS JOIN LATERAL (SELECT coalesce(x.local_start,$3) AT TIME ZONE coalesce(x.zone,e.zone) AS at) t
         CROSS JOIN LATERAL calendar.occurrences($1,t.at-interval '1 hour',t.at+interval '1 hour',e.calendar_id,e.id,false) o
         WHERE e.id=$2 AND o.original_start=$3"
    ).bind(user.id).bind(id).bind(original).fetch_optional(&app.db).await?.ok_or_else(Error::denied)?;
    Ok(Json(item))
}

#[derive(Deserialize)]
pub(super) struct OccurrenceInput {
    revision: i64,
    #[serde(default)]
    cancelled: bool,
    event: Option<EventFields>,
}

pub(super) async fn edit_occurrence(
    State(app): State<App>,
    user: User,
    Path((id, original)): Path<(Uuid, String)>,
    Json(mut input): Json<OccurrenceInput>,
) -> Result<StatusCode> {
    let original = local(&original)?;
    if input.cancelled == input.event.is_some() {
        return Err(Error::bad("invalid_occurrence_change"));
    }
    if let Some(fields) = &mut input.event {
        validate_fields(&app.db, fields).await?;
    }
    let mut tx = app.db.begin().await?;
    let calendar_id = event_calendar(&mut tx, id).await?;
    lock_access(&mut tx, calendar_id, user.id, false).await?;
    check_revision(&mut tx, id, input.revision).await?;
    let valid: bool = sqlx::query_scalar(
        "SELECT repeat_kind IS NOT NULL AND calendar.is_slot(e,$2) FROM events e WHERE id=$1",
    )
    .bind(id)
    .bind(original)
    .fetch_one(&mut *tx)
    .await?;
    if !valid {
        return Err(Error::denied());
    }
    sqlx::query(
        "INSERT INTO exceptions (event_id,original_start,cancelled,title,description,local_start,local_end,zone,all_day,busy)
         SELECT $1,$2,$3,p.title,p.description,p.local_start,p.local_end,p.zone,p.all_day,p.busy
         FROM jsonb_populate_record(NULL::exceptions,coalesce($4,'{}'::jsonb)) p
         ON CONFLICT (event_id,original_start) DO UPDATE SET cancelled=excluded.cancelled,title=excluded.title,
             description=excluded.description,local_start=excluded.local_start,local_end=excluded.local_end,
             zone=excluded.zone,all_day=excluded.all_day,busy=excluded.busy"
    ).bind(id).bind(original).bind(input.cancelled).bind(input.event.map(sqlx::types::Json)).execute(&mut *tx).await?;
    sqlx::query("UPDATE events SET revision=revision+1 WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(super) struct ReminderInput {
    minutes_before: Option<i32>,
}

pub(super) async fn set_reminder(
    State(app): State<App>,
    user: User,
    Path(id): Path<Uuid>,
    Json(input): Json<ReminderInput>,
) -> Result<StatusCode> {
    validate_reminder(input.minutes_before)?;
    let mut tx = app.db.begin().await?;
    let calendar_id = event_calendar(&mut tx, id).await?;
    let owner: Uuid = sqlx::query_scalar("SELECT owner_id FROM calendars WHERE id=$1 FOR UPDATE")
        .bind(calendar_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::denied)?;
    if owner != user.id
        && !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM members WHERE calendar_id=$1 AND user_id=$2)",
        )
        .bind(calendar_id)
        .bind(user.id)
        .fetch_one(&mut *tx)
        .await?
    {
        return Err(Error::denied());
    }
    write_reminder(&mut tx, id, user.id, input.minutes_before).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn reminders(State(app): State<App>, user: User) -> Result<Json<Value>> {
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(r) FROM calendar.pending_reminders($1) r
        ORDER BY due_at DESC,event_id,original_start LIMIT 100",
    )
    .bind(user.id)
    .fetch_all(&app.db)
    .await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
pub(super) struct DismissInput {
    event_id: Uuid,
    original_start: String,
    due_at: String,
}

pub(super) async fn dismiss(
    State(app): State<App>,
    user: User,
    Json(input): Json<DismissInput>,
) -> Result<StatusCode> {
    sqlx::query(
        "INSERT INTO dismissals (event_id,user_id,original_start,due_at)
         SELECT r.event_id,$1,r.original_start,r.due_at FROM calendar.pending_reminders($1) r
         WHERE r.event_id=$2 AND r.original_start=$3 AND r.due_at=$4 ON CONFLICT DO NOTHING",
    )
    .bind(user.id)
    .bind(input.event_id)
    .bind(local(&input.original_start)?)
    .bind(instant(&input.due_at)?)
    .execute(&app.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
