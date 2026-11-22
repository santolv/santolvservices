use crate::{
    App,
    auth::User,
    error::{Error, Result},
    files, storage,
};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, Postgres, Row, Transaction};
use std::{io::SeekFrom, time::Duration};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
pub struct Create {
    pub space_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub node_id: Option<Uuid>,
    pub name: String,
    pub size: i64,
    pub sha256: String,
}
#[derive(Serialize, FromRow, Clone)]
pub struct Upload {
    pub storage_pending: bool,
    #[serde(skip)]
    pub storage_directory: String,
    pub id: Uuid,
    pub user_id: Uuid,
    pub space_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub node_id: Option<Uuid>,
    pub name: String,
    pub expected_revision: Option<i64>,
    pub size: i64,
    pub received: i64,
    pub sha256: String,
    pub state: String,
    pub expires_at: time::OffsetDateTime,
    pub result_node: Option<Uuid>,
    pub complete_requested: bool,
    pub completion_error: Option<String>,
}
pub async fn upload(tx: &mut Transaction<'_, Postgres>, id: Uuid, user: Uuid) -> Result<Upload> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("upload:{id}"))
        .execute(&mut **tx)
        .await?;
    sqlx::query_as("SELECT u.*,s.storage_directory,EXISTS(SELECT 1 FROM workspace_jobs j WHERE j.node_id=u.result_node AND NOT j.audit) AS storage_pending FROM uploads u JOIN spaces s ON s.id=u.space_id WHERE u.id=$1 AND u.user_id=$2")
        .bind(id)
        .bind(user)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(Error::denied)
}
fn pending(u: &Upload) -> Result<()> {
    if u.state != "pending" || u.expires_at < time::OffsetDateTime::now_utc() {
        return Err(Error::conflict("upload_not_pending"));
    }
    Ok(())
}
pub async fn allowed(tx: &mut Transaction<'_, Postgres>, v: &Upload, user: Uuid) -> Result<()> {
    if let Some(id) = v.node_id {
        files::access(tx, id, user, 2, false).await?;
    } else {
        files::folder(tx, v.space_id, v.parent_id, user, 2).await?;
    }
    Ok(())
}
pub async fn create(
    State(s): State<App>,
    u: User,
    h: HeaderMap,
    Json(c): Json<Create>,
) -> Result<Json<Value>> {
    let name = files::name(&c.name)?;
    if c.size < 0
        || c.size > 1024_i64.pow(4)
        || c.sha256.len() != 64
        || !c.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::bad("invalid_size_or_sha256"));
    }
    let fp = format!(
        "upload:{}:{}",
        serde_json::to_string(&c).unwrap(),
        h.get("if-match")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    );
    let mut tx = s.db.begin().await?;
    let (key, old) = files::replay(&mut tx, u.id, &h, &fp).await?;
    if let Some(v) = old {
        return Ok(Json(v));
    }
    files::lock(&mut tx, c.space_id).await?;
    let expected = if let Some(id) = c.node_id {
        let n = files::access(&mut tx, id, u.id, 2, false).await?;
        if n.space_id != c.space_id || n.kind != "file" {
            return Err(Error::bad("invalid_target"));
        }
        files::check_revision(&n, &h)?;
        Some(n.revision)
    } else {
        files::folder(&mut tx, c.space_id, c.parent_id, u.id, 2).await?;
        files::name_available(&mut tx, c.space_id, c.parent_id, &name, None, None).await?;
        None
    };
    files::workspace_ready(&mut tx, c.space_id).await?;
    storage::capacity(&s.root, c.size as u64).await?;
    let reserved=sqlx::query("UPDATE spaces SET reserved_bytes=reserved_bytes+$1 WHERE id=$2 AND $1<=quota-used_bytes-reserved_bytes").bind(c.size).bind(c.space_id).execute(&mut *tx).await?;
    if reserved.rows_affected() == 0 {
        return Err(Error::conflict("quota_exceeded"));
    }
    let id = Uuid::new_v4();
    let directory: String = sqlx::query_scalar("SELECT storage_directory FROM spaces WHERE id=$1")
        .bind(c.space_id)
        .fetch_one(&mut *tx)
        .await?;
    storage::prepare_space(&s.root, &directory).await?;
    sqlx::query("INSERT INTO uploads(id,user_id,space_id,parent_id,node_id,name,expected_revision,size,sha256) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(id).bind(u.id).bind(c.space_id).bind(c.parent_id).bind(c.node_id).bind(name).bind(expected).bind(c.size).bind(c.sha256.to_lowercase()).execute(&mut *tx).await?;
    let v = json!({"id":id,"offset":0,"max_chunk_bytes":8388608,"expires_in":86400});
    files::remember(&mut tx, u.id, &key, &fp, &v).await?;
    tx.commit().await?;
    Ok(Json(v))
}
pub async fn status(State(s): State<App>, u: User, Path(id): Path<Uuid>) -> Result<Json<Upload>> {
    let mut tx = s.db.begin().await?;
    let v: Upload = sqlx::query_as("SELECT u.*,s.storage_directory,EXISTS(SELECT 1 FROM workspace_jobs j WHERE j.node_id=u.result_node AND NOT j.audit) AS storage_pending FROM uploads u JOIN spaces s ON s.id=u.space_id WHERE u.id=$1 AND u.user_id=$2")
        .bind(id)
        .bind(u.id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::denied)?;
    allowed(&mut tx, &v, u.id).await?;
    Ok(Json(v))
}
pub async fn append(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    body: Body,
) -> Result<Json<Value>> {
    let _permit = s
        .transfers
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "transfer_capacity"))?;
    let offset: i64 = h
        .get("upload-offset")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| Error::bad("upload_offset_required"))?;
    let len: i64 = h
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0 && *v <= 8 * 1024 * 1024)
        .ok_or_else(|| Error::bad("chunk_length_1_to_8388608"))?;
    let expected = h
        .get("x-chunk-sha256")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| Error::bad("chunk_sha256_required"))?
        .to_lowercase();
    let mut tx = s.db.begin().await?;
    let v = upload(&mut tx, id, u.id).await?;
    pending(&v)?;
    allowed(&mut tx, &v, u.id).await?;
    if offset != v.received {
        return Err(Error::conflict("offset_mismatch_query_upload_status"));
    }
    if len > v.size - v.received {
        return Err(Error::bad("chunk_exceeds_expected_size"));
    }
    storage::capacity(&s.root, len as u64).await?;
    storage::relocate_legacy(&s.root, &v.storage_directory, id, &v.name).await?;
    let p = storage::temp(&s.root, &v.storage_directory, id);
    if !tokio::fs::try_exists(p.parent().unwrap()).await? {
        storage::prepare_space(&s.root, &v.storage_directory).await?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&p)
        .await?;
    if file.metadata().await?.len() < v.received as u64 {
        return Err(Error::unavailable());
    }
    file.set_len(v.received as u64).await?;
    file.seek(SeekFrom::Start(v.received as u64)).await?;
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    let mut stream = body.into_data_stream();
    let mut written = 0i64;
    tokio::time::timeout(Duration::from_secs(120), async {
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|_| Error::bad("interrupted_chunk"))?;
            written += bytes.len() as i64;
            if written > len {
                return Err(Error::bad("chunk_length_mismatch"));
            }
            digest.update(&bytes);
            file.write_all(&bytes).await?;
        }
        if written != len || hex::encode(digest.finalize()) != expected {
            return Err(Error::bad("chunk_integrity_mismatch"));
        }
        file.flush().await?;
        file.sync_all().await?;
        storage::sync_dir(p.parent().unwrap()).await?;
        Ok::<_, Error>(())
    })
    .await
    .map_err(|_| Error(StatusCode::REQUEST_TIMEOUT, "chunk_timeout"))??;
    sqlx::query("UPDATE uploads SET received=received+$1 WHERE id=$2")
        .bind(len)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"offset":v.received+len})))
}
pub async fn complete(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<Value>)> {
    let mut tx = s.db.begin().await?;
    let v: Upload = sqlx::query_as("SELECT u.*,s.storage_directory,EXISTS(SELECT 1 FROM workspace_jobs j WHERE j.node_id=u.result_node AND NOT j.audit) AS storage_pending FROM uploads u JOIN spaces s ON s.id=u.space_id WHERE u.id=$1 AND u.user_id=$2")
        .bind(id)
        .bind(u.id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::denied)?;
    allowed(&mut tx, &v, u.id).await?;
    if v.state == "ready" {
        return Ok((
            StatusCode::OK,
            Json(json!({"id":v.result_node,"upload_id":id})),
        ));
    }
    pending(&v)?;
    if v.received != v.size {
        return Err(Error::conflict("upload_incomplete"));
    }
    let changed=sqlx::query("UPDATE uploads SET complete_requested=true,completion_error=NULL WHERE id=$1 AND state='pending'")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if changed.rows_affected() == 0 {
        return Err(Error::conflict("upload_state_changed_query_status"));
    }
    tx.commit().await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({"upload_id":id,"state":"pending"})),
    ))
}
pub async fn finish(s: &App, id: Uuid, user: Uuid) -> Result<Value> {
    let _permit = s
        .transfers
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "transfer_capacity"))?;
    let mut tx = s.db.begin().await?;
    let v = upload(&mut tx, id, user).await?;
    if v.state == "ready" {
        allowed(&mut tx, &v, user).await?;
        return Ok(json!({"id":v.result_node,"upload_id":id}));
    }
    pending(&v)?;
    if v.received != v.size {
        return Err(Error::conflict("upload_incomplete"));
    }
    storage::relocate_legacy(&s.root, &v.storage_directory, id, &v.name).await?;
    storage::prepare_space(&s.root, &v.storage_directory).await?;
    let src = storage::temp(&s.root, &v.storage_directory, id);
    let dst = storage::prepare_object(&s.root, &v.storage_directory, id, &v.name).await?;
    if v.size == 0 && !tokio::fs::try_exists(&src).await? && !tokio::fs::try_exists(&dst).await? {
        let f = tokio::fs::File::create(&src).await?;
        f.sync_all().await?;
        storage::sync_dir(src.parent().unwrap()).await?;
    }
    let published = tokio::fs::try_exists(&dst).await?;
    let (size, hash) = storage::checksum(if published { &dst } else { &src }).await?;
    if size != v.size || hash != v.sha256 {
        return Err(Error::conflict("file_integrity_mismatch"));
    }
    // Finish before a concurrent disable/reset commits, or reject its now-invalid actor.
    let active:Option<Uuid>=sqlx::query_scalar("SELECT id FROM users WHERE id=$1 AND NOT disabled AND NOT must_change_password AND account_type='user' FOR SHARE").bind(user).fetch_optional(&mut *tx).await?;
    if active.is_none() {
        return Err(Error::denied());
    }
    files::lock(&mut tx, v.space_id).await?;
    allowed(&mut tx, &v, user).await?;
    files::workspace_ready(&mut tx, v.space_id).await?;
    let node = if let Some(n) = v.node_id {
        let current = files::access(&mut tx, n, user, 2, false).await?;
        if Some(current.revision) != v.expected_revision {
            return Err(Error(StatusCode::PRECONDITION_FAILED, "revision_changed"));
        }
        n
    } else {
        files::name_available(&mut tx, v.space_id, v.parent_id, &v.name, None, Some(id)).await?;
        Uuid::new_v4()
    };
    // Both directories are under one persistent volume; sync the rename before committing ready.
    if !published {
        tokio::fs::rename(&src, &dst).await?;
        storage::sync_dir(dst.parent().unwrap()).await?;
        storage::sync_dir(src.parent().unwrap()).await?;
    }
    sqlx::query(
        "INSERT INTO objects(id,space_id,size,sha256,original_name) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(v.space_id)
    .bind(v.size)
    .bind(&v.sha256)
    .bind(&v.name)
    .execute(&mut *tx)
    .await?;
    if v.node_id.is_none() {
        sqlx::query(
            "INSERT INTO nodes(id,space_id,parent_id,name,kind) VALUES($1,$2,$3,$4,'file')",
        )
        .bind(node)
        .bind(v.space_id)
        .bind(v.parent_id)
        .bind(&v.name)
        .execute(&mut *tx)
        .await?;
    }
    let version = Uuid::new_v4();
    sqlx::query("INSERT INTO versions(id,node_id,object_id,created_by) VALUES($1,$2,$3,$4)")
        .bind(version)
        .bind(node)
        .bind(id)
        .bind(user)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE nodes SET current_version=$1,revision=revision+CASE WHEN $3 THEN 1 ELSE 0 END WHERE id=$2").bind(version).bind(node).bind(v.node_id.is_some()).execute(&mut *tx).await?;
    sqlx::query("UPDATE objects o SET state='deleting' WHERE id IN(SELECT object_id FROM versions WHERE node_id=$1 AND id<>$2) AND NOT EXISTS(SELECT 1 FROM versions v WHERE v.object_id=o.id AND (v.node_id<>$1 OR v.id=$2))")
        .bind(node).bind(version).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM versions WHERE node_id=$1 AND id<>$2")
        .bind(node)
        .bind(version)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE uploads SET state='ready',result_node=$1,complete_requested=false,completion_error=NULL WHERE id=$2")
        .bind(node)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE spaces SET reserved_bytes=reserved_bytes-$1,used_bytes=used_bytes+$1 WHERE id=$2",
    )
    .bind(v.size)
    .bind(v.space_id)
    .execute(&mut *tx)
    .await?;
    files::audit(&mut tx, user, v.space_id, "upload_completed", node).await?;
    crate::workspace::schedule(&mut tx, node, v.space_id).await?;
    tx.commit().await?;
    Ok(crate::workspace::publish(s, node, json!({"id":node,"upload_id":id})).await)
}
pub async fn cancel(State(s): State<App>, u: User, Path(id): Path<Uuid>) -> Result<StatusCode> {
    cancel_upload(&s, id, u.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn cancel_upload(s: &App, id: Uuid, user: Uuid) -> Result<()> {
    let mut tx = s.db.begin().await?;
    let v = upload(&mut tx, id, user).await?;
    if v.state == "ready" {
        return Err(Error::conflict("upload_already_completed"));
    }
    if v.state == "pending" {
        files::lock(&mut tx, v.space_id).await?;
        sqlx::query("UPDATE uploads SET state='cancelled' WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE spaces SET reserved_bytes=reserved_bytes-$1 WHERE id=$2")
            .bind(v.size)
            .bind(v.space_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    storage::relocate_legacy(&s.root, &v.storage_directory, id, &v.name).await?;
    storage::remove(&storage::temp(&s.root, &v.storage_directory, id)).await?;
    storage::remove_object(&s.root, &v.storage_directory, id, &v.name).await?;
    Ok(())
}
#[derive(Deserialize)]
pub struct Download {
    version: Option<Uuid>,
}
pub async fn download(
    State(s): State<App>,
    u: User,
    Path(id): Path<Uuid>,
    Query(q): Query<Download>,
    h: HeaderMap,
) -> Result<Response> {
    let permit = s
        .transfers
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "transfer_capacity"))?;
    let mut tx = s.db.begin().await?;
    let n = files::access(&mut tx, id, u.id, 1, false).await?;
    if q.version.is_some() {
        return Err(Error::bad("history_not_supported"));
    }
    files::lock(&mut tx, n.space_id).await?;
    let version = n.current_version.ok_or_else(Error::denied)?;
    let r=sqlx::query("SELECT o.id,o.size,o.sha256,o.state,o.original_name,s.storage_directory FROM versions v JOIN objects o ON o.id=v.object_id JOIN spaces s ON s.id=o.space_id WHERE v.id=$1 AND v.node_id=$2").bind(version).bind(id).fetch_optional(&mut *tx).await?.ok_or_else(Error::denied)?;
    if r.get::<String, _>("state") != "ready" {
        return Err(Error::unavailable());
    }
    let object: Uuid = r.get("id");
    let size = r.get::<i64, _>("size") as u64;
    let etag = format!("\"{}\"", r.get::<String, _>("sha256"));
    let range = h.get("range").and_then(|v| v.to_str().ok()).filter(|_| {
        h.get("if-range")
            .is_none_or(|v| v.to_str().ok() == Some(&etag))
    });
    let (start, len, partial) = storage::range(range, size)?;
    // Open before releasing the snapshot: a concurrent purge cannot serve another object's bytes.
    let directory: String = r.get("storage_directory");
    let original_name: &str = r.get("original_name");
    storage::relocate_legacy(&s.root, &directory, object, original_name).await?;
    let mut file = tokio::fs::File::open(
        crate::workspace::content_path_tx(&mut tx, &s.root, &directory, object, original_name)
            .await?,
    )
    .await?;
    if file.metadata().await?.len() != size {
        return Err(Error::unavailable());
    }
    file.seek(SeekFrom::Start(start)).await?;
    tx.commit().await?;
    let stream = tokio_util::io::ReaderStream::with_capacity(file.take(len), 64 * 1024);
    let stream =
        futures_util::stream::unfold((stream, permit), |(mut stream, permit)| async move {
            stream.next().await.map(|chunk| (chunk, (stream, permit)))
        });
    let mut response = Body::from_stream(stream).into_response();
    *response.status_mut() = if partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let hh = response.headers_mut();
    hh.insert("content-type", "application/octet-stream".parse().unwrap());
    hh.insert(
        "content-disposition",
        storage::attachment(&n.name)
            .parse()
            .expect("ASCII attachment header"),
    );
    hh.insert("accept-ranges", "bytes".parse().unwrap());
    hh.insert("content-length", len.to_string().parse().unwrap());
    hh.insert("etag", etag.parse().unwrap());
    if partial {
        hh.insert(
            "content-range",
            format!("bytes {start}-{}/{size}", start + len - 1)
                .parse()
                .unwrap(),
        );
    }
    Ok(response)
}
