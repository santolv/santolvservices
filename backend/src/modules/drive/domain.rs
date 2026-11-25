use crate::{
    App,
    auth::User,
    error::{Error, Result},
    files::*,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

pub async fn spaces(
    State(s): State<App>,
    u: User,
    Query(q): Query<VersionQuery>,
) -> Result<Json<Value>> {
    let v:Vec<Value>=sqlx::query_scalar("SELECT (to_jsonb(s)-'storage_directory'-'legacy_storage_directory'-'storage_removed'-'deleted_at')||jsonb_build_object('storage_pending',EXISTS(SELECT 1 FROM workspace_jobs j WHERE j.space_id=s.id AND NOT j.audit)) FROM spaces s JOIN members m ON m.space_id=s.id WHERE m.user_id=$1 AND s.service='drive' AND s.deleted_at IS NULL AND ($2::uuid IS NULL OR s.id>$2) ORDER BY s.id LIMIT 101").bind(u.id).bind(q.after).fetch_all(&s.db).await?;
    Ok(page(v, "id"))
}
pub async fn shared(
    State(s): State<App>,
    u: User,
    Query(q): Query<Listing>,
) -> Result<Json<Value>> {
    let ids:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT g.node_id FROM grants g WHERE (g.user_id=$1 OR EXISTS(SELECT 1 FROM members m WHERE m.space_id=g.subject_space_id AND m.user_id=$1)) AND ($2::uuid IS NULL OR g.node_id>$2) ORDER BY g.node_id LIMIT 101").bind(u.id).bind(q.after).fetch_all(&s.db).await?;
    let next = if ids.len() > 100 { Some(ids[99]) } else { None };
    let mut tx = s.db.begin().await?;
    let mut items = Vec::new();
    for id in ids.into_iter().take(100) {
        match access(&mut tx, id, u.id, 1, false).await {
            Ok(n) => items.push(n),
            Err(e) if e.0 == StatusCode::NOT_FOUND || e.0 == StatusCode::CONFLICT => (),
            Err(e) => return Err(e),
        }
    }
    Ok(Json(json!({"items":items,"next_cursor":next})))
}
#[derive(Deserialize, Serialize)]
pub struct SpaceInput {
    name: String,
}
pub async fn delete_space(
    State(s): State<App>,
    u: User,
    Path(space): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = s.db.begin().await?;
    lock(&mut tx, space).await?;
    let row = sqlx::query(
        "SELECT kind,owner_id,deleted_at IS NOT NULL AS deleting FROM spaces WHERE id=$1",
    )
    .bind(space)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Error::denied)?;
    if row.get::<Uuid, _>("owner_id") != u.id {
        return Err(Error::denied());
    }
    if row.get::<String, _>("kind") != "organization" {
        return Err(Error::conflict("personal_space_protected"));
    }
    if !row.get::<bool, _>("deleting") {
        workspace_ready(&mut tx, space).await?;
        sqlx::query("UPDATE spaces SET deleted_at=now() WHERE id=$1")
            .bind(space)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM members WHERE space_id=$1")
            .bind(space)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM grants WHERE subject_space_id=$1")
            .bind(space)
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, u.id, space, "space_deleted", space).await?;
    }
    tx.commit().await?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn recent(
    State(s): State<App>,
    u: User,
    Query(q): Query<RecentQuery>,
) -> Result<Json<Value>> {
    let page = recent_items(&s, u.id, q).await?;
    Ok(Json(
        json!({"items":page.items,"next_cursor":page.boundary.map(|(stamp,id)|format!("{stamp}:{id}"))}),
    ))
}
pub async fn create_space(
    State(s): State<App>,
    u: User,
    h: HeaderMap,
    Json(c): Json<SpaceInput>,
) -> Result<Json<Value>> {
    let name = name(&c.name)?;
    let fp = format!("space:{name}");
    let mut tx = s.db.begin().await?;
    let (key, old) = replay(&mut tx, u.id, &h, &fp).await?;
    if let Some(v) = old {
        return Ok(Json(v));
    }
    let id = Uuid::new_v4();
    let directory = format!("organizaciones/{id}");
    sqlx::query(
        "INSERT INTO spaces(id,name,kind,owner_id,quota,storage_directory) VALUES($1,$2,'organization',$3,$4,$5)",
    )
    .bind(id)
    .bind(name)
    .bind(u.id)
    .bind(s.quota)
    .bind(&directory)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO members VALUES($1,$2,3)")
        .bind(id)
        .bind(u.id)
        .execute(&mut *tx)
        .await?;
    crate::storage::prepare_space(&s.root, &directory).await?;
    let result = json!({"id":id});
    remember(&mut tx, u.id, &key, &fp, &result).await?;
    tx.commit().await?;
    Ok(Json(result))
}
pub async fn members(
    State(s): State<App>,
    u: User,
    Path(space): Path<Uuid>,
    Query(q): Query<VersionQuery>,
) -> Result<Json<Value>> {
    let mut tx = s.db.begin().await?;
    if member_role(&mut tx, space, u.id).await? < 3 {
        return Err(Error::denied());
    }
    let v:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('user_id',m.user_id,'username',u.username,'role',m.role) FROM members m JOIN users u ON u.id=m.user_id WHERE m.space_id=$1 AND ($2::uuid IS NULL OR m.user_id>$2) ORDER BY m.user_id LIMIT 101").bind(space).bind(q.after).fetch_all(&mut *tx).await?;
    Ok(page(v, "user_id"))
}
#[derive(Deserialize)]
pub struct Membership {
    username: Option<String>,
    user_id: Option<Uuid>,
    role: i16,
}
async fn resolve_user(
    tx: &mut Transaction<'_, Postgres>,
    username: Option<&str>,
    id: Option<Uuid>,
    allow_disabled: bool,
) -> Result<Uuid> {
    if username.is_some() == id.is_some() {
        return Err(Error::bad("choose_username_or_user_id"));
    }
    let username = username.map(|v| v.trim().to_ascii_lowercase());
    if username
        .as_ref()
        .is_some_and(|v| v.is_empty() || v.len() > 254 || v.chars().any(char::is_control))
    {
        return Err(Error::bad("invalid_username"));
    }
    sqlx::query_scalar("SELECT id FROM users WHERE account_type='user' AND (NOT disabled OR $3) AND (username=$1 OR id=$2)")
        .bind(username).bind(id).bind(allow_disabled).fetch_optional(&mut **tx).await?.ok_or_else(Error::denied)
}
pub async fn set_member(
    State(s): State<App>,
    u: User,
    Path(space): Path<Uuid>,
    Json(c): Json<Membership>,
) -> Result<StatusCode> {
    if !(1..=3).contains(&c.role) {
        return Err(Error::bad("invalid_role"));
    }
    let mut tx = s.db.begin().await?;
    lock(&mut tx, space).await?;
    let owner: Option<Uuid> =
        sqlx::query_scalar("SELECT owner_id FROM spaces WHERE id=$1 AND kind='organization'")
            .bind(space)
            .fetch_optional(&mut *tx)
            .await?;
    if member_role(&mut tx, space, u.id).await? < 3 || owner.is_none() {
        return Err(Error::denied());
    }
    let id = resolve_user(&mut tx, c.username.as_deref(), c.user_id, false).await?;
    if Some(id) == owner {
        return Err(Error::conflict("owner_membership_protected"));
    }
    sqlx::query("INSERT INTO members VALUES($1,$2,$3) ON CONFLICT(space_id,user_id) DO UPDATE SET role=excluded.role").bind(space).bind(id).bind(c.role).execute(&mut *tx).await?;
    audit(&mut tx, u.id, space, "membership_set", id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn remove_member(
    State(s): State<App>,
    u: User,
    Path((space, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode> {
    let mut tx = s.db.begin().await?;
    lock(&mut tx, space).await?;
    let owner: Option<Uuid> =
        sqlx::query_scalar("SELECT owner_id FROM spaces WHERE id=$1 AND kind='organization'")
            .bind(space)
            .fetch_optional(&mut *tx)
            .await?;
    if member_role(&mut tx, space, u.id).await? < 3 || owner.is_none() {
        return Err(Error::denied());
    }
    if Some(id) == owner {
        return Err(Error::conflict("owner_membership_protected"));
    }
    sqlx::query("DELETE FROM members WHERE space_id=$1 AND user_id=$2")
        .bind(space)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM grants g USING nodes n WHERE g.node_id=n.id AND n.space_id=$1 AND g.user_id=$2").bind(space).bind(id).execute(&mut *tx).await?;
    audit(&mut tx, u.id, space, "membership_removed", id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
pub struct Grant {
    username: Option<String>,
    user_id: Option<Uuid>,
    subject_space_id: Option<Uuid>,
    role: Option<i16>,
}
async fn change_grant(s: App, u: User, id: Uuid, c: Grant, revoke: bool) -> Result<StatusCode> {
    if usize::from(c.user_id.is_some())
        + usize::from(c.username.is_some())
        + usize::from(c.subject_space_id.is_some())
        != 1
        || (!revoke && !c.role.is_some_and(|r| (1..=3).contains(&r)))
    {
        return Err(Error::bad("invalid_grant"));
    }
    let mut tx = s.db.begin().await?;
    let n = node(&mut tx, id).await?;
    lock(&mut tx, n.space_id).await?;
    access(&mut tx, id, u.id, 3, false).await?;
    let user_id = if c.subject_space_id.is_none() {
        Some(resolve_user(&mut tx, c.username.as_deref(), c.user_id, revoke).await?)
    } else {
        None
    };
    // Sharing to a space requires membership, so callers cannot enumerate arbitrary organizations.
    if let Some(subject) = c.subject_space_id
        && member_role(&mut tx, subject, u.id).await? < 1
    {
        return Err(Error::denied());
    }
    sqlx::query("DELETE FROM grants WHERE node_id=$1 AND user_id IS NOT DISTINCT FROM $2 AND subject_space_id IS NOT DISTINCT FROM $3").bind(id).bind(user_id).bind(c.subject_space_id).execute(&mut *tx).await?;
    if !revoke {
        sqlx::query(
            "INSERT INTO grants(node_id,user_id,subject_space_id,role) VALUES($1,$2,$3,$4)",
        )
        .bind(id)
        .bind(user_id)
        .bind(c.subject_space_id)
        .bind(c.role)
        .execute(&mut *tx)
        .await?;
    }
    audit(&mut tx, u.id, n.space_id, "grant_changed", id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn set_grant(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
    Json(c): Json<Grant>,
) -> Result<StatusCode> {
    change_grant(s, u, id, c, false).await
}
pub async fn revoke_grant(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
    Json(c): Json<Grant>,
) -> Result<StatusCode> {
    change_grant(s, u, id, c, true).await
}
pub async fn grants(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
    Query(q): Query<VersionQuery>,
) -> Result<Json<Value>> {
    let mut tx = s.db.begin().await?;
    access(&mut tx, id, u.id, 3, false).await?;
    let v: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(g)||jsonb_build_object('username',u.username,'space_name',s.name) FROM grants g LEFT JOIN users u ON u.id=g.user_id LEFT JOIN spaces s ON s.id=g.subject_space_id WHERE g.node_id=$1 AND ($2::uuid IS NULL OR g.id>$2) ORDER BY g.id LIMIT 101")
            .bind(id)
            .bind(q.after)
            .fetch_all(&mut *tx)
            .await?;
    Ok(page(v, "id"))
}
#[derive(Deserialize)]
pub struct VersionQuery {
    after: Option<Uuid>,
}
