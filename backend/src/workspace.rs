use crate::{
    App,
    error::{Error, Result},
    files, storage,
};
use sqlx::{FromRow, Postgres, Row, Transaction};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(FromRow)]
struct Entry {
    node_id: Uuid,
    space_id: Uuid,
    parent_id: Option<Uuid>,
    component: String,
    source_name: String,
    kind: String,
    object_id: Option<Uuid>,
    content_name: Option<String>,
    trashed: bool,
}

pub async fn schedule(tx: &mut Transaction<'_, Postgres>, node: Uuid, space: Uuid) -> Result<()> {
    sqlx::query("INSERT INTO workspace_jobs(node_id,space_id) VALUES($1,$2) ON CONFLICT(node_id) DO UPDATE SET attempted_at='-infinity',last_error=NULL,audit=false")
        .bind(node).bind(space).execute(&mut **tx).await?;
    Ok(())
}

// SQL is authoritative. A durable job survives interruption between the directory rename and commit.
pub async fn publish(s: &App, node: Uuid, mut value: serde_json::Value) -> serde_json::Value {
    let pending = match sync_node(s, node).await {
        Ok(()) => false,
        Err(e) => {
            tracing::warn!(node=%node,code=e.1,"named workspace pending reconciliation");
            true
        }
    };
    value["storage_pending"] = pending.into();
    value
}

async fn move_directory(s: &App, old: &Path, target: &Path, node: Uuid) -> Result<()> {
    // A bounded wait lets a cold pool open a connection without exhausting it in a lock cycle.
    let mut journal = tokio::time::timeout(std::time::Duration::from_millis(250), s.db.acquire())
        .await
        .map_err(|_| Error::unavailable())??;
    let intent: Option<(String, String)> =
        sqlx::query_as("SELECT source,target FROM workspace_moves WHERE node_id=$1")
            .bind(node)
            .fetch_optional(&mut *journal)
            .await?;
    let source = old.to_string_lossy().to_string();
    let destination = target.to_string_lossy().to_string();
    let stage = s.root.join(".temporal/movimientos").join(node.to_string());
    if let Some((a, b)) = &intent {
        if Path::new(a) != old || Path::new(b) != target {
            return Err(Error::conflict("workspace_path_occupied"));
        }
    } else {
        if !tokio::fs::try_exists(old).await? && !tokio::fs::try_exists(&stage).await? {
            return Err(Error::unavailable());
        }
        sqlx::query("INSERT INTO workspace_moves(node_id,source,target) VALUES($1,$2,$3)")
            .bind(node)
            .bind(&source)
            .bind(&destination)
            .execute(&mut *journal)
            .await?;
    }
    drop(journal);
    if !tokio::fs::try_exists(&stage).await? {
        if !tokio::fs::try_exists(old).await?
            && intent.is_some()
            && tokio::fs::try_exists(target).await?
        {
            return Ok(());
        }
        tokio::fs::rename(old, &stage).await?;
        storage::sync_dir(old.parent().unwrap()).await?;
        storage::sync_dir(stage.parent().unwrap()).await?;
    }
    if tokio::fs::try_exists(target).await? {
        if !tokio::fs::try_exists(old).await? {
            tokio::fs::rename(&stage, old).await?;
        }
        return Err(Error::conflict("workspace_path_occupied"));
    }
    tokio::fs::rename(&stage, target).await?;
    storage::sync_dir(stage.parent().unwrap()).await?;
    storage::sync_dir(target.parent().unwrap()).await
}

async fn owned_directory(path: &Path, known: bool) -> Result<()> {
    let existed = tokio::fs::try_exists(path).await?;
    storage::private_dir(path).await?;
    if existed
        && !known
        && tokio::fs::read_dir(path)
            .await?
            .next_entry()
            .await?
            .is_some()
    {
        return Err(Error::conflict("workspace_path_occupied"));
    }
    // Compatibility: remove the previous identity sidecar only on an already registered node.
    if known {
        storage::remove(&path.join(".drive-node")).await?;
    }
    Ok(())
}
async fn publication_intent(s: &App, path: &Path, node: Uuid) -> Result<()> {
    let mut connection =
        tokio::time::timeout(std::time::Duration::from_millis(250), s.db.acquire())
            .await
            .map_err(|_| Error::unavailable())??;
    let existing: Option<String> =
        sqlx::query_scalar("SELECT target FROM workspace_publications WHERE node_id=$1")
            .bind(node)
            .fetch_optional(&mut *connection)
            .await?;
    if let Some(existing) = existing {
        if Path::new(&existing) != path {
            return Err(Error::conflict("workspace_path_occupied"));
        }
    } else {
        if tokio::fs::try_exists(path).await? {
            return Err(Error::conflict("workspace_path_occupied"));
        }
        sqlx::query("INSERT INTO workspace_publications(node_id,target) VALUES($1,$2)")
            .bind(node)
            .bind(path.to_string_lossy().as_ref())
            .execute(&mut *connection)
            .await?;
    }
    Ok(())
}

pub async fn content_path(s: &App, directory: &str, object: Uuid, name: &str) -> Result<PathBuf> {
    let mut tx = s.db.begin().await?;
    content_path_tx(&mut tx, &s.root, directory, object, name).await
}

pub async fn content_path_tx(
    tx: &mut Transaction<'_, Postgres>,
    root: &Path,
    directory: &str,
    object: Uuid,
    name: &str,
) -> Result<PathBuf> {
    let entry: Option<Entry> =
        sqlx::query_as("SELECT * FROM workspace_entries WHERE object_id=$1 LIMIT 1")
            .bind(object)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(e) = entry {
        let path = entry_path(tx, &root.join(directory), e.node_id).await?;
        if let Some(content_name) = e.content_name {
            let path = path.join(content_name);
            if let Ok(info) = tokio::fs::symlink_metadata(&path).await {
                if !info.is_file() || storage::unsafe_link(&info) {
                    return Err(Error::unavailable());
                }
                return Ok(path);
            }
            if !tokio::fs::try_exists(storage::object(root, directory, object, name)).await? {
                return Ok(path);
            }
        }
    }
    Ok(storage::object(root, directory, object, name))
}

pub(crate) async fn entry_path(
    tx: &mut Transaction<'_, Postgres>,
    home: &Path,
    id: Uuid,
) -> Result<PathBuf> {
    let entries:Vec<Entry>=sqlx::query_as("WITH RECURSIVE c AS (SELECT e.*,0 depth,ARRAY[e.node_id] seen FROM workspace_entries e WHERE node_id=$1 UNION ALL SELECT e.*,c.depth+1,c.seen||e.node_id FROM workspace_entries e JOIN c ON e.node_id=c.parent_id WHERE NOT c.trashed AND c.depth<128 AND NOT e.node_id=ANY(c.seen)) SELECT node_id,space_id,parent_id,component,source_name,kind,object_id,content_name,trashed FROM c ORDER BY depth DESC")
        .bind(id).fetch_all(&mut **tx).await?;
    let first = entries
        .first()
        .ok_or_else(|| Error::conflict("workspace_parent_pending"))?;
    if !first.trashed && first.parent_id.is_some() {
        return Err(Error::conflict("workspace_parent_pending"));
    }
    let space = first.space_id;
    let mut path = if first.trashed {
        let legacy = home.join(".papelera").join(first.node_id.to_string());
        if tokio::fs::try_exists(&legacy).await? {
            legacy
        } else {
            home.join("papelera")
        }
    } else {
        home.join("archivos")
    };
    for entry in entries {
        if entry.space_id != space || (entry.node_id != id && entry.kind != "folder") {
            return Err(Error::conflict("invalid_workspace_parent"));
        }
        if entry.component != storage::disk_filename(&entry.component)
            || entry.component.to_ascii_lowercase().starts_with(".drive-")
        {
            return Err(Error::conflict("invalid_workspace_component"));
        }
        path.push(entry.component);
        if let Ok(info) = tokio::fs::symlink_metadata(&path).await
            && (!info.is_dir() || storage::unsafe_link(&info))
        {
            return Err(Error::conflict("workspace_path_occupied"));
        }
    }
    Ok(path)
}

pub async fn sync_node(s: &App, id: Uuid) -> Result<()> {
    let mut tx = s.db.begin().await?;
    let space:Option<Uuid>=sqlx::query_scalar("SELECT space_id FROM workspace_jobs WHERE node_id=$1 UNION ALL SELECT space_id FROM nodes WHERE id=$1 LIMIT 1")
        .bind(id).fetch_optional(&mut *tx).await?;
    let Some(space) = space else { return Ok(()) };
    files::lock(&mut tx, space).await?;
    let directory: String = sqlx::query_scalar("SELECT storage_directory FROM spaces WHERE id=$1")
        .bind(space)
        .fetch_one(&mut *tx)
        .await?;
    storage::prepare_space(&s.root, &directory).await?;
    let home = s.root.join(&directory);
    let nodes:Vec<files::Node>=sqlx::query_as("WITH RECURSIVE c AS (SELECT n.*,0 depth,ARRAY[n.id] seen FROM nodes n WHERE id=$1 UNION ALL SELECT n.*,c.depth+1,c.seen||n.id FROM nodes n JOIN c ON n.id=c.parent_id WHERE c.depth<128 AND NOT n.id=ANY(c.seen)) SELECT id,space_id,parent_id,name,kind,revision,trashed_at,current_version FROM c ORDER BY depth DESC")
        .bind(id).fetch_all(&mut *tx).await?;
    if nodes.is_empty() {
        let old: Option<Entry> = sqlx::query_as("SELECT * FROM workspace_entries WHERE node_id=$1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        if let Some(entry) = old {
            let path = entry_path(&mut tx, &home, id).await?;
            if tokio::fs::try_exists(&path).await? {
                if let (Some(object), Some(name)) = (entry.object_id, entry.content_name.as_ref()) {
                    let retained:Option<String>=sqlx::query_scalar("SELECT original_name FROM objects o WHERE id=$1 AND EXISTS(SELECT 1 FROM versions v WHERE v.object_id=o.id)")
                        .bind(object).fetch_optional(&mut *tx).await?;
                    if let Some(original) = retained {
                        let source = path.join(name);
                        let target =
                            storage::prepare_object(&s.root, &directory, object, &original).await?;
                        if !tokio::fs::try_exists(&target).await? {
                            tokio::fs::rename(source, &target).await?;
                            storage::sync_dir(target.parent().unwrap()).await?;
                        }
                    }
                }
                // The path is resolved from the durable registry, with every ancestor checked.
                tokio::fs::remove_dir_all(&path).await?;
                storage::sync_dir(path.parent().unwrap()).await?;
            }
            sqlx::query("WITH RECURSIVE c AS(SELECT node_id FROM workspace_entries WHERE node_id=$1 UNION ALL SELECT e.node_id FROM workspace_entries e JOIN c ON e.parent_id=c.node_id) DELETE FROM workspace_entries WHERE node_id IN(SELECT node_id FROM c)")
                .bind(entry.node_id).execute(&mut *tx).await?;
        }
    } else {
        if nodes[0].parent_id.is_some() {
            return Err(Error::conflict("workspace_tree_too_deep"));
        }
        for node in nodes {
            sync_entry(s, &mut tx, &home, &directory, &node).await?;
            sqlx::query("DELETE FROM workspace_moves WHERE node_id=$1")
                .bind(node.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM workspace_publications WHERE node_id=$1")
                .bind(node.id)
                .execute(&mut *tx)
                .await?;
        }
    }
    sqlx::query("DELETE FROM workspace_jobs WHERE node_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn sync_entry(
    s: &App,
    tx: &mut Transaction<'_, Postgres>,
    home: &Path,
    directory: &str,
    node: &files::Node,
) -> Result<()> {
    let old: Option<Entry> = sqlx::query_as("SELECT * FROM workspace_entries WHERE node_id=$1")
        .bind(node.id)
        .fetch_optional(&mut **tx)
        .await?;
    let trashed = node.trashed_at.is_some();
    let parent = if trashed {
        let base = home.join("papelera");
        storage::private_dir(&base).await?;
        base
    } else if let Some(id) = node.parent_id {
        entry_path(tx, home, id).await?
    } else {
        home.join("archivos")
    };
    let mut name = storage::disk_filename(&node.name);
    if name.to_ascii_lowercase().starts_with(".drive-") {
        name = format!("archivo-{}", node.id)
    }
    let mut component = old
        .as_ref()
        .filter(|e| e.source_name == node.name && e.trashed == trashed)
        .map(|e| e.component.clone())
        .unwrap_or_else(|| name.clone());
    if component.to_ascii_lowercase().starts_with(".drive-") {
        component = format!("archivo-{}", node.id)
    }
    let occupied:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_entries WHERE space_id=$1 AND COALESCE(parent_id,'00000000-0000-0000-0000-000000000000'::uuid)=COALESCE($2,'00000000-0000-0000-0000-000000000000'::uuid) AND lower(component)=lower($3) AND node_id<>$4 AND NOT trashed)")
        .bind(node.space_id).bind(node.parent_id).bind(&component).bind(node.id).fetch_one(&mut **tx).await?;
    // Preserve pre-existing duplicate names without merging their directories.
    if !trashed && occupied {
        let suffix = format!(" ({})", node.id);
        let mut prefix = name.clone();
        while prefix.len() + suffix.len() > 255 {
            prefix.pop();
        }
        component = prefix + &suffix;
    }
    if trashed {
        let mut number = 1;
        loop {
            let occupied: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_entries WHERE space_id=$1 AND trashed AND lower(component)=lower($2) AND node_id<>$3)")
                .bind(node.space_id).bind(&component).bind(node.id).fetch_one(&mut **tx).await?;
            if !occupied {
                break;
            }
            number += 1;
            let suffix = format!(" ({number})");
            let mut prefix = name.clone();
            while prefix.len() + suffix.len() > 255 {
                prefix.pop();
            }
            component = prefix + &suffix;
        }
    }
    let path = parent.join(&component);
    if let Some(entry) = &old {
        let old_path = entry_path(tx, home, entry.node_id).await?;
        if old_path != path {
            let legacy = home.join(".moves").join(node.id.to_string());
            if tokio::fs::try_exists(&legacy).await? {
                let info = tokio::fs::symlink_metadata(&legacy).await?;
                if !info.is_dir()
                    || storage::unsafe_link(&info)
                    || tokio::fs::read_to_string(legacy.join(".drive-node"))
                        .await?
                        .trim()
                        != node.id.to_string()
                {
                    return Err(Error::conflict("workspace_path_occupied"));
                }
                let stage = s
                    .root
                    .join(".temporal/movimientos")
                    .join(node.id.to_string());
                if tokio::fs::try_exists(&stage).await? {
                    return Err(Error::conflict("workspace_path_occupied"));
                }
                tokio::fs::rename(&legacy, &stage).await?;
                storage::sync_dir(stage.parent().unwrap()).await?;
                storage::sync_dir(legacy.parent().unwrap()).await?;
            }
            move_directory(s, &old_path, &path, node.id).await?;
            // Remove only empty directories left by the previous trash layout.
            if old_path.starts_with(home.join(".papelera")) {
                let _ = tokio::fs::remove_dir(old_path.parent().unwrap()).await;
                let _ = tokio::fs::remove_dir(home.join(".papelera")).await;
            }
        }
    }
    if old.is_none() {
        publication_intent(s, &path, node.id).await?;
    }
    owned_directory(&path, true).await?;
    let mut content = None;
    if let Some(version) = node.current_version {
        let row=sqlx::query("SELECT o.id,o.original_name,o.size,o.sha256 FROM versions v JOIN objects o ON o.id=v.object_id WHERE v.id=$1 AND o.state='ready'")
            .bind(version).fetch_optional(&mut **tx).await?.ok_or_else(Error::unavailable)?;
        let object: Uuid = row.get("id");
        storage::relocate_legacy(&s.root, directory, object, row.get("original_name")).await?;
        let target = path.join(&name);
        let known = old.as_ref().is_some_and(|e| e.object_id == Some(object));
        let previous_name = old.as_ref().and_then(|e| e.content_name.as_ref());
        if previous_name.is_some_and(|n| {
            storage::disk_filename(n) != *n || n.to_ascii_lowercase().starts_with(".drive-")
        }) {
            return Err(Error::conflict("invalid_workspace_component"));
        }
        let previous = previous_name.map(|n| path.join(n));
        if known && previous.as_ref() != Some(&target) {
            let stage = s
                .root
                .join(".temporal/movimientos")
                .join(format!("{}.contenido", node.id));
            let legacy = path.join(".drive-rename");
            if tokio::fs::try_exists(&legacy).await? {
                let info = tokio::fs::symlink_metadata(&legacy).await?;
                if !info.is_file()
                    || storage::unsafe_link(&info)
                    || stage.exists()
                    || storage::checksum(&legacy).await?.1 != row.get::<String, _>("sha256")
                {
                    return Err(Error::conflict("workspace_path_occupied"));
                }
                tokio::fs::rename(&legacy, &stage).await?;
                storage::sync_dir(&path).await?;
                storage::sync_dir(stage.parent().unwrap()).await?;
            }
            if stage.exists() {
                let info = tokio::fs::symlink_metadata(&stage).await?;
                if !info.is_file()
                    || storage::unsafe_link(&info)
                    || previous.as_ref().is_some_and(|p| p.exists())
                    || target.exists()
                {
                    return Err(Error::conflict("workspace_path_occupied"));
                }
                let (_, hash) = storage::checksum(&stage).await?;
                if hash != row.get::<String, _>("sha256") {
                    return Err(Error::conflict("workspace_path_occupied"));
                }
            } else if previous.as_ref().is_some_and(|p| p.exists()) {
                tokio::fs::rename(previous.as_ref().unwrap(), &stage).await?;
                storage::sync_dir(&path).await?;
                storage::sync_dir(stage.parent().unwrap()).await?;
                if target.exists() {
                    tokio::fs::rename(&stage, previous.as_ref().unwrap()).await?;
                    return Err(Error::conflict("workspace_path_occupied"));
                }
            }
            if stage.exists() {
                tokio::fs::rename(&stage, &target).await?;
                storage::sync_dir(stage.parent().unwrap()).await?;
            }
        }
        let valid = match tokio::fs::symlink_metadata(&target).await {
            Ok(info) if !info.is_file() || storage::unsafe_link(&info) => {
                return Err(Error::conflict("workspace_path_occupied"));
            }
            Ok(info) => info.len() == row.get::<i64, _>("size") as u64,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        if !known || !valid {
            let source = storage::object(&s.root, directory, object, row.get("original_name"));
            if tokio::fs::try_exists(&source).await? {
                if let Ok(info) = tokio::fs::symlink_metadata(&target).await
                    && (!info.is_file() || storage::unsafe_link(&info))
                {
                    return Err(Error::conflict("workspace_path_occupied"));
                }
                tokio::fs::rename(&source, &target).await?;
                storage::sync_dir(source.parent().unwrap()).await?;
                tokio::fs::remove_dir(source.parent().unwrap()).await?;
                if let Some(previous) = previous.filter(|p| p != &target) {
                    storage::remove(&previous).await?;
                }
            } else {
                let (size, hash) = storage::checksum(&target).await?;
                if size != row.get::<i64, _>("size") || hash != row.get::<String, _>("sha256") {
                    return Err(Error::unavailable());
                }
            }
        } else {
            // Remove a redundant copy left by the old layout only after verifying the live bytes.
            let source = storage::object(&s.root, directory, object, row.get("original_name"));
            if tokio::fs::try_exists(&source).await? {
                let (_, hash) = storage::checksum(&target).await?;
                if hash != row.get::<String, _>("sha256") {
                    return Err(Error::conflict("file_integrity_mismatch"));
                }
                storage::remove_object(&s.root, directory, object, row.get("original_name"))
                    .await?;
            }
        }
        storage::sync_dir(&path).await?;
        content = Some(object);
    }
    sqlx::query("INSERT INTO workspace_entries(node_id,space_id,parent_id,component,source_name,kind,object_id,content_name,trashed) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(node_id) DO UPDATE SET parent_id=excluded.parent_id,component=excluded.component,source_name=excluded.source_name,kind=excluded.kind,object_id=excluded.object_id,content_name=excluded.content_name,trashed=excluded.trashed")
        .bind(node.id).bind(node.space_id).bind(node.parent_id).bind(component).bind(&node.name).bind(&node.kind).bind(content).bind(content.map(|_|name)).bind(trashed).execute(&mut **tx).await?;
    Ok(())
}

pub async fn process(s: &App, limit: i64) -> Result<()> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT node_id FROM workspace_jobs ORDER BY attempted_at,node_id LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&s.db)
    .await?;
    for id in ids {
        if let Err(e) = sync_node(s, id).await {
            sqlx::query(
                "UPDATE workspace_jobs SET attempted_at=now(),last_error=$2,audit=false WHERE node_id=$1",
            )
            .bind(id)
            .bind(e.1)
            .execute(&s.db)
            .await?;
            tracing::warn!(node=%id,code=e.1,"workspace synchronization deferred");
        }
    }
    Ok(())
}

pub async fn run(s: App) {
    let mut cursor = Uuid::nil();
    let mut round = 0u8;
    loop {
        if let Err(e) = process(&s, 20).await {
            tracing::error!(code = e.1, "workspace synchronization failed")
        }
        round = (round + 1) % 10;
        if round == 0 {
            let result=async {
                let rows=sqlx::query("SELECT id,space_id FROM nodes WHERE id>$1 ORDER BY id LIMIT 50").bind(cursor).fetch_all(&s.db).await?;
                for row in &rows {
                    let node:Uuid=row.get("id");
                    sqlx::query("INSERT INTO workspace_jobs(node_id,space_id,audit) VALUES($1,$2,true) ON CONFLICT DO NOTHING").bind(node).bind(row.get::<Uuid,_>("space_id")).execute(&s.db).await?;
                    cursor=node;
                }
                if rows.len()<50 {cursor=Uuid::nil()}
                Ok::<_,sqlx::Error>(())
            }.await;
            if let Err(e) = result {
                tracing::error!(error=%e,"workspace audit failed")
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
