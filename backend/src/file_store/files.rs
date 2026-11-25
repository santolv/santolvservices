use crate::{
    App,
    auth::User,
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, Postgres, Row, Transaction};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

#[derive(Serialize, FromRow, Clone)]
pub struct Node {
    pub id: Uuid,
    pub space_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    pub revision: i64,
    pub trashed_at: Option<time::OffsetDateTime>,
    pub current_version: Option<Uuid>,
}
pub fn name(n: &str) -> Result<String> {
    let n: String = n.nfc().collect();
    if n.is_empty()
        || n.len() > 255
        || n == "."
        || n == ".."
        || n.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
        || crate::storage::disk_filename(&n) != n
        || n.to_ascii_lowercase().starts_with(".drive-")
    {
        return Err(Error::bad("invalid_name"));
    }
    Ok(n)
}
pub async fn name_available(
    tx: &mut Transaction<'_, Postgres>,
    space: Uuid,
    parent: Option<Uuid>,
    name: &str,
    node: Option<Uuid>,
    upload: Option<Uuid>,
) -> Result<()> {
    let occupied:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM nodes WHERE space_id=$1 AND COALESCE(parent_id,'00000000-0000-0000-0000-000000000000'::uuid)=COALESCE($2,'00000000-0000-0000-0000-000000000000'::uuid) AND lower(name)=lower($3) AND trashed_at IS NULL AND ($4::uuid IS NULL OR id<>$4)) OR EXISTS(SELECT 1 FROM uploads WHERE space_id=$1 AND parent_id IS NOT DISTINCT FROM $2 AND lower(name)=lower($3) AND node_id IS NULL AND state='pending' AND expires_at>now() AND ($5::uuid IS NULL OR id<>$5))")
        .bind(space).bind(parent).bind(name).bind(node).bind(upload).fetch_one(&mut **tx).await?;
    if occupied {
        return Err(Error::conflict("name_already_exists"));
    }
    Ok(())
}
pub async fn workspace_ready(tx: &mut Transaction<'_, Postgres>, space: Uuid) -> Result<()> {
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workspace_jobs WHERE space_id=$1 AND NOT audit)",
    )
    .bind(space)
    .fetch_one(&mut **tx)
    .await?;
    if pending {
        return Err(Error::conflict("workspace_sync_pending"));
    }
    Ok(())
}
pub fn revision(h: &HeaderMap) -> Result<i64> {
    h.get("if-match")
        .ok_or(Error(
            StatusCode::PRECONDITION_REQUIRED,
            "if_match_required",
        ))?
        .to_str()
        .ok()
        .and_then(|s| s.strip_prefix('"')?.strip_suffix('"')?.parse::<i64>().ok())
        .filter(|r| *r > 0)
        .ok_or_else(|| Error::bad("invalid_etag"))
}
pub fn check_revision(n: &Node, h: &HeaderMap) -> Result<()> {
    if n.revision != revision(h)? {
        return Err(Error(StatusCode::PRECONDITION_FAILED, "revision_changed"));
    }
    Ok(())
}
pub async fn lock(tx: &mut Transaction<'_, Postgres>, s: Uuid) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("space:{s}"))
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub async fn node(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Node> {
    sqlx::query_as("SELECT id,space_id,parent_id,name,kind,revision,trashed_at,current_version FROM nodes WHERE id=$1").bind(id).fetch_optional(&mut **tx).await?.ok_or_else(Error::denied)
}
pub async fn member_role(tx: &mut Transaction<'_, Postgres>, s: Uuid, u: Uuid) -> Result<i16> {
    Ok(
        sqlx::query_scalar("SELECT m.role FROM members m JOIN users u ON u.id=m.user_id JOIN spaces s ON s.id=m.space_id WHERE m.space_id=$1 AND m.user_id=$2 AND s.deleted_at IS NULL AND NOT u.disabled AND NOT u.must_change_password AND u.account_type='user'")
            .bind(s)
            .bind(u)
            .fetch_optional(&mut **tx)
            .await?
            .unwrap_or(0),
    )
}
pub async fn access(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    u: Uuid,
    required: i16,
    allow_trash: bool,
) -> Result<Node> {
    let n = node(tx, id).await?;
    let r=sqlx::query("WITH RECURSIVE ancestors AS (SELECT id,parent_id,trashed_at FROM nodes WHERE id=$1 UNION ALL SELECT n.id,n.parent_id,n.trashed_at FROM nodes n JOIN ancestors a ON n.id=a.parent_id) SELECT EXISTS(SELECT 1 FROM users WHERE id=$3 AND NOT disabled AND NOT must_change_password AND account_type='user') AND EXISTS(SELECT 1 FROM spaces WHERE id=$2 AND deleted_at IS NULL) AS active, EXISTS(SELECT 1 FROM ancestors WHERE trashed_at IS NOT NULL) AS trash, EXISTS(SELECT 1 FROM ancestors a JOIN purge_jobs p ON p.node_id=a.id) AS deleting, GREATEST(COALESCE((SELECT role FROM members WHERE space_id=$2 AND user_id=$3),0),COALESCE((SELECT max(g.role) FROM grants g JOIN ancestors a ON a.id=g.node_id WHERE g.user_id=$3 OR EXISTS(SELECT 1 FROM members m WHERE m.space_id=g.subject_space_id AND m.user_id=$3)),0))::smallint AS role")
        .bind(id).bind(n.space_id).bind(u).fetch_one(&mut **tx).await?;
    if !r.get::<bool, _>("active")
        || r.get::<i16, _>("role") < required
        || (!allow_trash && r.get::<bool, _>("trash"))
    {
        return Err(Error::denied());
    }
    if r.get::<bool, _>("deleting") {
        return Err(Error::conflict("purge_in_progress"));
    }
    Ok(n)
}
pub async fn folder(
    tx: &mut Transaction<'_, Postgres>,
    space: Uuid,
    parent: Option<Uuid>,
    u: Uuid,
    role: i16,
) -> Result<()> {
    if let Some(p) = parent {
        let n = access(tx, p, u, role, false).await?;
        if n.space_id != space || n.kind != "folder" {
            return Err(Error::bad("invalid_parent"));
        }
    } else if member_role(tx, space, u).await? < role {
        return Err(Error::denied());
    }
    Ok(())
}
pub async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    u: Uuid,
    s: Uuid,
    action: &str,
    id: Uuid,
) -> Result<()> {
    sqlx::query("INSERT INTO audit(actor,space_id,action,target) VALUES($1,$2,$3,$4)")
        .bind(u)
        .bind(s)
        .bind(action)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    u: Uuid,
    h: &HeaderMap,
    fingerprint: &str,
) -> Result<(String, Option<Value>)> {
    let key = h
        .get("idempotency-key")
        .and_then(|h| h.to_str().ok())
        .filter(|k| !k.is_empty() && k.len() <= 128)
        .ok_or_else(|| Error::bad("idempotency_key_required"))?
        .to_owned();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("idem:{u}:{key}"))
        .execute(&mut **tx)
        .await?;
    let r = sqlx::query("SELECT fingerprint,result FROM idempotency WHERE user_id=$1 AND key=$2")
        .bind(u)
        .bind(&key)
        .fetch_optional(&mut **tx)
        .await?;
    if let Some(r) = r {
        if r.get::<String, _>("fingerprint") != fingerprint {
            return Err(Error::conflict("idempotency_key_reused"));
        }
        return Ok((key, Some(r.get("result"))));
    }
    Ok((key, None))
}
pub async fn remember(
    tx: &mut Transaction<'_, Postgres>,
    u: Uuid,
    key: &str,
    fp: &str,
    result: &Value,
) -> Result<()> {
    sqlx::query("INSERT INTO idempotency(user_id,key,fingerprint,result) VALUES($1,$2,$3,$4)")
        .bind(u)
        .bind(key)
        .bind(fp)
        .bind(result)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub(crate) fn page(mut values: Vec<Value>, key: &str) -> Json<Value> {
    let next = if values.len() > 100 {
        values[99][key].clone()
    } else {
        Value::Null
    };
    values.truncate(100);
    Json(json!({"items":values,"next_cursor":next}))
}
pub async fn touch_recent(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    node: Uuid,
) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("recent:{user}"))
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO recent_nodes(user_id,node_id,used_at) VALUES($1,$2,(extract(epoch FROM clock_timestamp())*1000000)::bigint) ON CONFLICT(user_id,node_id) DO UPDATE SET used_at=excluded.used_at").bind(user).bind(node).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM recent_nodes WHERE user_id=$1 AND node_id IN(SELECT node_id FROM recent_nodes WHERE user_id=$1 ORDER BY used_at DESC,node_id DESC OFFSET 200)").bind(user).execute(&mut **tx).await?;
    Ok(())
}
pub async fn touch(State(s): State<App>, u: User, Path(id): Path<Uuid>) -> Result<StatusCode> {
    let mut tx = s.db.begin().await?;
    access(&mut tx, id, u.id, 1, false).await?;
    touch_recent(&mut tx, u.id, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Clone, Deserialize)]
pub struct RecentQuery {
    after: Option<String>,
}
pub(crate) struct RecentPage {
    pub items: Vec<Value>,
    pub boundary: Option<(i64, Uuid)>,
}
pub(crate) fn recent_key(item: &Value) -> (i64, Uuid) {
    (
        item["last_used_at"].as_i64().expect("recent timestamp"),
        serde_json::from_value(item["id"].clone()).expect("node UUID"),
    )
}
pub(crate) async fn recent_items(s: &App, user: Uuid, q: RecentQuery) -> Result<RecentPage> {
    let (stamp, id) = match q.after {
        Some(v) => {
            let (a, b) = v
                .split_once(':')
                .ok_or_else(|| Error::bad("invalid_cursor"))?;
            (
                a.parse::<i64>().map_err(|_| Error::bad("invalid_cursor"))?,
                b.parse::<Uuid>()
                    .map_err(|_| Error::bad("invalid_cursor"))?,
            )
        }
        None => (i64::MAX, Uuid::max()),
    };
    let mut tx = s.db.begin().await?;
    let rows=sqlx::query("SELECT r.node_id,r.used_at,s.service FROM recent_nodes r JOIN nodes n ON n.id=r.node_id JOIN spaces s ON s.id=n.space_id WHERE r.user_id=$1 AND (used_at,node_id)<($2,$3) ORDER BY used_at DESC,node_id DESC LIMIT 101").bind(user).bind(stamp).bind(id).fetch_all(&mut *tx).await?;
    let next = if rows.len() > 100 {
        Some((rows[99].get("used_at"), rows[99].get("node_id")))
    } else {
        None
    };
    let mut items = Vec::new();
    for row in rows.iter().take(100) {
        match access(&mut tx, row.get("node_id"), user, 1, false).await {
            Ok(node) => {
                let mut value = serde_json::to_value(node).map_err(|_| Error::unavailable())?;
                value["last_used_at"] = row.get::<i64, _>("used_at").into();
                value["service"] = row.get::<String, _>("service").into();
                items.push(value);
            }
            Err(e) if matches!(e.0, StatusCode::NOT_FOUND | StatusCode::CONFLICT) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(RecentPage {
        items,
        boundary: next,
    })
}
#[derive(Deserialize)]
pub struct Listing {
    pub parent: Option<Uuid>,
    pub after: Option<Uuid>,
    pub limit: Option<i64>,
    #[serde(default)]
    pub trash: bool,
}
pub async fn list(
    State(s): State<App>,
    u: User,
    Path(space): Path<Uuid>,
    Query(q): Query<Listing>,
) -> Result<Json<Value>> {
    let is_root = q.parent.is_none() && q.after.is_none() && !q.trash;
    let mut page = list_children(&s, &u, space, q, None).await?;
    if is_root
        && crate::core::manager::accessible(&s.db, u.id)
            .await?
            .iter()
            .any(|id| id == "photos")
    {
        let personal_drive: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM spaces WHERE id=$1 AND owner_id=$2
             AND service='drive' AND kind='personal' AND deleted_at IS NULL)",
        )
        .bind(space)
        .bind(u.id)
        .fetch_one(&s.db)
        .await?;
        if personal_drive {
            page["photos_folder"] =
                json!({"id":"photos","name":"Fotos","kind":"folder","protected":true});
        }
    }
    Ok(Json(page))
}

pub(crate) async fn list_children(
    s: &App,
    u: &User,
    space: Uuid,
    q: Listing,
    kind: Option<&str>,
) -> Result<Value> {
    let mut tx = s.db.begin().await?;
    if q.trash {
        if member_role(&mut tx, space, u.id).await? < 1 {
            return Err(Error::denied());
        }
    } else {
        folder(&mut tx, space, q.parent, u.id, 1).await?;
    }
    let limit = q.limit.unwrap_or(100).clamp(1, 200);
    let mut query = sqlx::QueryBuilder::<Postgres>::new(
        "SELECT id,space_id,parent_id,name,kind,revision,trashed_at,current_version FROM nodes WHERE space_id=",
    );
    query.push_bind(space);
    query.push(" AND NOT EXISTS(SELECT 1 FROM purge_jobs p WHERE p.node_id=nodes.id)");
    if let Some(kind) = kind {
        query.push(" AND kind=").push_bind(kind);
    }
    if q.trash {
        query.push(" AND trashed_at IS NOT NULL");
    } else {
        query.push(" AND trashed_at IS NULL AND parent_id");
        if let Some(parent) = q.parent {
            query.push("=").push_bind(parent);
        } else {
            query.push(" IS NULL");
        }
    }
    query
        .push(" AND id>")
        .push_bind(q.after.unwrap_or(Uuid::nil()))
        .push(" ORDER BY id LIMIT ")
        .push_bind(limit + 1);
    let nodes: Vec<Node> = query.build_query_as().fetch_all(&mut *tx).await?;
    let next = if nodes.len() > limit as usize {
        Some(nodes[limit as usize - 1].id)
    } else {
        None
    };
    Ok(
        json!({"items":nodes.into_iter().take(limit as usize).collect::<Vec<_>>(),"next_cursor":next}),
    )
}
#[derive(Serialize, Deserialize)]
pub struct FolderInput {
    pub name: String,
    pub parent_id: Option<Uuid>,
}
pub async fn create_folder(
    State(s): State<App>,
    u: User,
    Path(space): Path<Uuid>,
    h: HeaderMap,
    Json(c): Json<FolderInput>,
) -> Result<Json<Value>> {
    let name = name(&c.name)?;
    let fp = format!("folder:{space}:{}", serde_json::to_string(&c).unwrap());
    let mut tx = s.db.begin().await?;
    let (key, old) = replay(&mut tx, u.id, &h, &fp).await?;
    if let Some(v) = old {
        return Ok(Json(v));
    }
    lock(&mut tx, space).await?;
    folder(&mut tx, space, c.parent_id, u.id, 2).await?;
    workspace_ready(&mut tx, space).await?;
    name_available(&mut tx, space, c.parent_id, &name, None, None).await?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO nodes(id,space_id,parent_id,name,kind) VALUES($1,$2,$3,$4,'folder')")
        .bind(id)
        .bind(space)
        .bind(c.parent_id)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    let v = json!({"id":id,"revision":1});
    audit(&mut tx, u.id, space, "folder_created", id).await?;
    touch_recent(&mut tx, u.id, id).await?;
    crate::workspace::schedule(&mut tx, id, space).await?;
    remember(&mut tx, u.id, &key, &fp, &v).await?;
    tx.commit().await?;
    Ok(Json(crate::workspace::publish(&s, id, v).await))
}
pub async fn get_node(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
) -> Result<(HeaderMap, Json<Node>)> {
    let mut tx = s.db.begin().await?;
    let n = access(&mut tx, id, u.id, 1, false).await?;
    let mut h = HeaderMap::new();
    h.insert("etag", format!("\"{}\"", n.revision).parse().unwrap());
    Ok((h, Json(n)))
}
#[derive(Deserialize, Serialize)]
pub struct Edit {
    name: Option<String>,
    parent_id: Option<Uuid>,
    #[serde(default)]
    move_to_root: bool,
}
pub async fn edit_node(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(c): Json<Edit>,
) -> Result<Json<Value>> {
    let fp = format!(
        "edit:{id}:{}:{}",
        revision(&h)?,
        serde_json::to_string(&c).unwrap()
    );
    let mut tx = s.db.begin().await?;
    let (key, old) = replay(&mut tx, u.id, &h, &fp).await?;
    if let Some(v) = old {
        return Ok(Json(v));
    }
    let n = node(&mut tx, id).await?;
    lock(&mut tx, n.space_id).await?;
    let n = access(&mut tx, id, u.id, 2, false).await?;
    check_revision(&n, &h)?;
    workspace_ready(&mut tx, n.space_id).await?;
    let target = if c.move_to_root {
        None
    } else {
        c.parent_id.or(n.parent_id)
    };
    if target != n.parent_id {
        folder(&mut tx, n.space_id, target, u.id, 2).await?;
        let cycle:bool=sqlx::query_scalar("WITH RECURSIVE a AS (SELECT id,parent_id FROM nodes WHERE id=$1 UNION ALL SELECT n.id,n.parent_id FROM nodes n JOIN a ON n.id=a.parent_id) SELECT EXISTS(SELECT 1 FROM a WHERE id=$2)").bind(target).bind(id).fetch_one(&mut *tx).await?;
        if cycle {
            return Err(Error::conflict("folder_cycle"));
        }
    }
    let name = match c.name {
        Some(n) => name(&n)?,
        None => n.name,
    };
    name_available(&mut tx, n.space_id, target, &name, Some(id), None).await?;
    sqlx::query("UPDATE nodes SET name=$1,parent_id=$2,revision=revision+1 WHERE id=$3")
        .bind(name)
        .bind(target)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let v = json!({"id":id,"revision":n.revision+1});
    audit(&mut tx, u.id, n.space_id, "node_updated", id).await?;
    touch_recent(&mut tx, u.id, id).await?;
    crate::workspace::schedule(&mut tx, id, n.space_id).await?;
    remember(&mut tx, u.id, &key, &fp, &v).await?;
    tx.commit().await?;
    Ok(Json(crate::workspace::publish(&s, id, v).await))
}
async fn trash_action(
    s: App,
    u: User,
    id: Uuid,
    h: HeaderMap,
    restore: bool,
) -> Result<Json<Value>> {
    let fp = format!("trash:{id}:{restore}:{}", revision(&h)?);
    let mut tx = s.db.begin().await?;
    let (key, old) = replay(&mut tx, u.id, &h, &fp).await?;
    if let Some(v) = old {
        return Ok(Json(v));
    }
    let n = node(&mut tx, id).await?;
    lock(&mut tx, n.space_id).await?;
    let n = access(&mut tx, id, u.id, 2, true).await?;
    workspace_ready(&mut tx, n.space_id).await?;
    check_revision(&n, &h)?;
    if restore {
        folder(&mut tx, n.space_id, n.parent_id, u.id, 2).await?;
        name_available(&mut tx, n.space_id, n.parent_id, &n.name, Some(id), None).await?;
    }
    sqlx::query("UPDATE nodes SET trashed_at=CASE WHEN $2 THEN NULL ELSE now() END,revision=revision+1 WHERE id=$1").bind(id).bind(restore).execute(&mut *tx).await?;
    let v = json!({"id":id,"revision":n.revision+1});
    audit(
        &mut tx,
        u.id,
        n.space_id,
        if restore { "restored" } else { "trashed" },
        id,
    )
    .await?;
    remember(&mut tx, u.id, &key, &fp, &v).await?;
    crate::workspace::schedule(&mut tx, id, n.space_id).await?;
    tx.commit().await?;
    Ok(Json(crate::workspace::publish(&s, id, v).await))
}
pub async fn trash(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    trash_action(s, u, id, h, false).await
}
pub async fn restore(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    trash_action(s, u, id, h, true).await
}
pub async fn purge(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<StatusCode> {
    let mut tx = s.db.begin().await?;
    let n = node(&mut tx, id).await?;
    lock(&mut tx, n.space_id).await?;
    let n = access(&mut tx, id, u.id, 3, true).await?;
    workspace_ready(&mut tx, n.space_id).await?;
    check_revision(&n, &h)?;
    if n.trashed_at.is_none() {
        return Err(Error::conflict("trash_required_before_purge"));
    }
    // A durable task is requested; the worker deletes one leaf at a time.
    sqlx::query("INSERT INTO purge_jobs(node_id) VALUES($1) ON CONFLICT DO NOTHING")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, u.id, n.space_id, "purge_requested", id).await?;
    tx.commit().await?;
    Ok(StatusCode::ACCEPTED)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_preconditions() {
        assert_eq!(name("cafe\u{301}").unwrap(), "café");
        for n in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
            assert!(name(n).is_err());
        }
        let mut h = HeaderMap::new();
        assert_eq!(
            revision(&h).unwrap_err().0,
            StatusCode::PRECONDITION_REQUIRED
        );
        h.insert("if-match", "\"4\"".parse().unwrap());
        assert_eq!(revision(&h).unwrap(), 4);
    }
}
