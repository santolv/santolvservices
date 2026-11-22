use crate::{App, error::Result, files, storage, uploads};
use axum::{extract::State, http::StatusCode};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

pub async fn health(State(s): State<App>) -> Result<StatusCode> {
    sqlx::query("SELECT 1").execute(&s.db).await?;
    storage::capacity(&s.root, 0).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn run(s: App) {
    tokio::join!(
        maintenance(s.clone()),
        finalization(s.clone()),
        crate::workspace::run(s)
    );
}
async fn finalization(s: App) {
    loop {
        if let Err(e) = finalize_requested(&s).await {
            tracing::error!(code = e.1, "finalization pass failed");
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
pub async fn finalize_requested(s: &App) -> Result<()> {
    let r=sqlx::query("SELECT id,user_id FROM uploads WHERE state='pending' AND complete_requested AND expires_at>now() ORDER BY expires_at LIMIT 1").fetch_optional(&s.db).await?;
    if let Some(r) = r {
        let id: Uuid = r.get("id");
        if let Err(e) = uploads::finish(s, id, r.get("user_id")).await {
            let retry = matches!(
                e.0,
                StatusCode::SERVICE_UNAVAILABLE
                    | StatusCode::TOO_MANY_REQUESTS
                    | StatusCode::INSUFFICIENT_STORAGE
            );
            sqlx::query("UPDATE uploads SET completion_error=$2,complete_requested=$3 WHERE id=$1 AND state='pending'").bind(id).bind(e.1).bind(retry).execute(&s.db).await?;
            tracing::warn!(upload=%id,code=e.1,"finalization pending or rejected");
        }
    }
    Ok(())
}
async fn maintenance(s: App) {
    let mut cursor = Uuid::nil();
    let mut cancelled_cursor = Uuid::nil();
    let mut workspace_cursor = Uuid::nil();
    loop {
        if let Err(e) = prepare_workspaces(&s, &mut workspace_cursor).await {
            tracing::error!(code = e.1, "workspace preparation failed");
        }
        if let Err(e) = tick(&s, &mut cursor, &mut cancelled_cursor).await {
            tracing::error!(code = e.1, "maintenance pass failed");
        }
        if let Err(e) = storage::cleanup_legacy_roots(&s.root).await {
            tracing::warn!(code = e.1, "legacy directory cleanup deferred");
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
async fn prepare_workspaces(s: &App, cursor: &mut Uuid) -> Result<()> {
    let rows =
        sqlx::query("SELECT id,storage_directory FROM spaces WHERE id>$1 ORDER BY id LIMIT 100")
            .bind(*cursor)
            .fetch_all(&s.db)
            .await?;
    for row in &rows {
        storage::prepare_space(&s.root, row.get("storage_directory")).await?;
        *cursor = row.get("id");
    }
    if rows.len() < 100 {
        *cursor = Uuid::nil();
    }
    Ok(())
}
pub async fn tick(s: &App, cursor: &mut Uuid, cancelled_cursor: &mut Uuid) -> Result<()> {
    let expired=sqlx::query("SELECT id,user_id FROM uploads WHERE state='pending' AND expires_at<now() ORDER BY expires_at LIMIT 20").fetch_all(&s.db).await?;
    for r in expired {
        uploads::cancel_upload(s, r.get("id"), r.get("user_id")).await?;
    }
    let pending=sqlx::query("SELECT u.id,u.user_id,u.name AS original_name,s.storage_directory FROM uploads u JOIN spaces s ON s.id=u.space_id WHERE u.state='pending' AND u.received=u.size AND u.expires_at>now() ORDER BY u.expires_at LIMIT 20").fetch_all(&s.db).await?;
    for r in pending {
        let id: Uuid = r.get("id");
        let directory: &str = r.get("storage_directory");
        let name: &str = r.get("original_name");
        storage::relocate_legacy(&s.root, directory, id, name).await?;
        if tokio::fs::try_exists(storage::object(&s.root, directory, id, name)).await?
            && let Err(e) = uploads::finish(s, id, r.get("user_id")).await
        {
            tracing::warn!(upload=%id,code=e.1,"pending publication needs attention");
        }
    }
    // Cancellation commits before removing bytes; retry deletion if the process stopped between them.
    let cancelled = sqlx::query(
        "SELECT u.id,u.name AS original_name,s.storage_directory FROM uploads u JOIN spaces s ON s.id=u.space_id WHERE u.state='cancelled' AND u.id>$1 ORDER BY u.id LIMIT 100",
    )
    .bind(*cancelled_cursor)
    .fetch_all(&s.db)
    .await?;
    for row in &cancelled {
        let id: Uuid = row.get("id");
        let directory: &str = row.get("storage_directory");
        let name: &str = row.get("original_name");
        storage::relocate_legacy(&s.root, directory, id, name).await?;
        storage::remove(&storage::temp(&s.root, directory, id)).await?;
        storage::remove_object(&s.root, directory, id, name).await?;
        sqlx::query("DELETE FROM uploads WHERE id=$1 AND state='cancelled' AND expires_at<now()-interval '7 days'").bind(id).execute(&s.db).await?;
        *cancelled_cursor = id;
    }
    if cancelled.len() < 100 {
        *cancelled_cursor = Uuid::nil();
    }
    for _ in 0..100 {
        if !purge_leaf(s).await? {
            break;
        }
    }
    garbage_collect(s).await?;
    let objects = sqlx::query(
        "SELECT o.id,o.size,o.sha256,o.state,o.original_name,s.storage_directory FROM objects o JOIN spaces s ON s.id=o.space_id WHERE o.state IN ('ready','missing') AND o.id>$1 ORDER BY o.id LIMIT 100",
    )
    .bind(*cursor)
    .fetch_all(&s.db)
    .await?;
    for r in &objects {
        let id: Uuid = r.get("id");
        let size: i64 = r.get("size");
        let directory: &str = r.get("storage_directory");
        let name: &str = r.get("original_name");
        storage::relocate_legacy(&s.root, directory, id, name).await?;
        let valid = match tokio::fs::metadata(storage::object(&s.root, directory, id, name)).await {
            Ok(m) => m.len() == size as u64,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        if !valid {
            sqlx::query("UPDATE objects SET state='missing' WHERE id=$1 AND state='ready'")
                .bind(id)
                .execute(&s.db)
                .await?;
            tracing::error!(object=%id,"content missing or truncated; restore backup required");
        } else if r.get::<String, _>("state") == "missing" {
            let (_, hash) =
                storage::checksum(&storage::object(&s.root, directory, id, name)).await?;
            if hash == r.get::<String, _>("sha256") {
                sqlx::query("UPDATE objects SET state='ready' WHERE id=$1 AND state='missing'")
                    .bind(id)
                    .execute(&s.db)
                    .await?;
            }
        }
        *cursor = id;
    }
    if objects.len() < 100 {
        *cursor = Uuid::nil();
    }
    sqlx::query("DELETE FROM sessions WHERE expires_at<now()")
        .execute(&s.db)
        .await?;
    sqlx::query("DELETE FROM rate_limits WHERE window_start<now()-interval '1 day'")
        .execute(&s.db)
        .await?;
    sqlx::query("DELETE FROM idempotency WHERE created_at<now()-interval '7 days'")
        .execute(&s.db)
        .await?;
    sqlx::query("DELETE FROM uploads WHERE id IN(SELECT id FROM uploads WHERE state='ready' AND expires_at<now()-interval '7 days' LIMIT 100)").execute(&s.db).await?;
    crate::workspace::process(s, 100).await?;
    Ok(())
}
async fn purge_leaf(s: &App) -> Result<bool> {
    let mut tx = s.db.begin().await?;
    let root: Option<Uuid> = sqlx::query_scalar(
        "SELECT node_id FROM purge_jobs ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(root) = root else { return Ok(false) };
    let n = files::node(&mut tx, root).await?;
    files::lock(&mut tx, n.space_id).await?;
    if n.trashed_at.is_none() {
        sqlx::query("DELETE FROM purge_jobs WHERE node_id=$1")
            .bind(root)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(true);
    }
    let leaf:Option<Uuid>=sqlx::query_scalar("WITH RECURSIVE tree AS (SELECT id FROM nodes WHERE id=$1 UNION ALL SELECT n.id FROM nodes n JOIN tree t ON n.parent_id=t.id) SELECT t.id FROM tree t WHERE NOT EXISTS(SELECT 1 FROM nodes c WHERE c.parent_id=t.id) AND NOT EXISTS(SELECT 1 FROM uploads u WHERE (u.node_id=t.id OR u.parent_id=t.id) AND u.state='pending') ORDER BY t.id LIMIT 1").bind(root).fetch_optional(&mut *tx).await?;
    if let Some(id) = leaf {
        crate::workspace::schedule(&mut tx, id, n.space_id).await?;
        sqlx::query("DELETE FROM uploads WHERE node_id=$1 OR parent_id=$1 OR result_node=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE nodes SET current_version=NULL WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE objects o SET state='deleting' WHERE o.id IN(SELECT object_id FROM versions WHERE node_id=$1) AND NOT EXISTS(SELECT 1 FROM versions v WHERE v.object_id=o.id AND v.node_id<>$1)")
            .bind(id).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM versions WHERE node_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM grants WHERE node_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM purge_jobs WHERE node_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM nodes WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(leaf.is_some())
}
async fn garbage_collect(s: &App) -> Result<()> {
    let objects=sqlx::query("SELECT o.id,o.space_id,o.original_name,s.storage_directory FROM objects o JOIN spaces s ON s.id=o.space_id WHERE o.state='deleting' OR (o.created_at<now()-interval '1 day' AND NOT EXISTS(SELECT 1 FROM versions v WHERE v.object_id=o.id)) ORDER BY o.id LIMIT 20").fetch_all(&s.db).await?;
    for row in objects {
        let id: Uuid = row.get("id");
        let space: Uuid = row.get("space_id");
        let mut tx = s.db.begin().await?;
        files::lock(&mut tx, space).await?;
        let changed=sqlx::query("UPDATE objects o SET state='deleting' WHERE id=$1 AND NOT EXISTS(SELECT 1 FROM versions v WHERE v.object_id=o.id)").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        if changed.rows_affected() == 0 {
            continue;
        }
        let directory: &str = row.get("storage_directory");
        let name: &str = row.get("original_name");
        storage::relocate_legacy(&s.root, directory, id, name).await?;
        storage::remove_object(&s.root, directory, id, name).await?;
        let mut tx = s.db.begin().await?;
        files::lock(&mut tx, space).await?;
        let size: Option<i64> = sqlx::query_scalar(
            "DELETE FROM objects WHERE id=$1 AND state='deleting' RETURNING size",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(size) = size {
            sqlx::query("UPDATE spaces SET used_bytes=used_bytes-$1 WHERE id=$2")
                .bind(size)
                .bind(space)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
    }
    Ok(())
}
