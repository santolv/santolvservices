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

async fn move_directory(old: &Path, target: &Path, stage: &Path, node: Uuid) -> Result<()> {
    if tokio::fs::try_exists(stage).await? {
        if marker(stage).await? != Some(node) || old.exists() || target.exists() {
            return Err(Error::conflict("workspace_path_occupied"));
        }
    } else if tokio::fs::try_exists(old).await? {
        if marker(old).await? != Some(node) {
            return Err(Error::conflict("workspace_path_occupied"));
        }
        tokio::fs::rename(old, stage).await?;
        storage::sync_dir(old.parent().unwrap()).await?;
        storage::sync_dir(stage.parent().unwrap()).await?;
        if tokio::fs::try_exists(target).await? {
            tokio::fs::rename(stage, old).await?;
            storage::sync_dir(old.parent().unwrap()).await?;
            storage::sync_dir(stage.parent().unwrap()).await?;
            return Err(Error::conflict("workspace_path_occupied"));
        }
    } else {
        return Ok(());
    }
    tokio::fs::rename(stage, target).await?;
    storage::sync_dir(stage.parent().unwrap()).await?;
    storage::sync_dir(target.parent().unwrap()).await?;
    Ok(())
}

async fn marker(path: &Path) -> Result<Option<Uuid>> {
    let file = path.join(".drive-node");
    match tokio::fs::symlink_metadata(&file).await {
        Ok(info) if info.is_file() && !info.file_type().is_symlink() && info.len() <= 64 => {
            Ok(tokio::fs::read_to_string(file).await?.trim().parse().ok())
        }
        Ok(_) => Err(Error::conflict("workspace_path_occupied")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

async fn owned_directory(path: &Path, node: Uuid) -> Result<()> {
    storage::private_dir(path).await?;
    if let Some(owner) = marker(path).await? {
        return if owner == node {
            Ok(())
        } else {
            Err(Error::conflict("workspace_path_occupied"))
        };
    }
    if tokio::fs::read_dir(path)
        .await?
        .next_entry()
        .await?
        .is_some()
    {
        return Err(Error::conflict("workspace_path_occupied"));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path.join(".drive-node"))
        .await?;
    use tokio::io::AsyncWriteExt;
    file.write_all(node.to_string().as_bytes()).await?;
    file.sync_all().await?;
    storage::sync_dir(path).await
}

async fn entry_path(tx: &mut Transaction<'_, Postgres>, home: &Path, id: Uuid) -> Result<PathBuf> {
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
        home.join(".papelera").join(first.node_id.to_string())
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
            && (!info.is_dir() || info.file_type().is_symlink())
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
                if marker(&path).await? != Some(id) {
                    return Err(Error::conflict("workspace_path_occupied"));
                }
                // Only a server-managed directory with the matching node marker may be removed.
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
        let base = home.join(".papelera");
        storage::private_dir(&base).await?;
        let base = base.join(node.id.to_string());
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
        .filter(|e| e.source_name == node.name)
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
    let path = parent.join(&component);
    if let Some(entry) = &old {
        let old_path = entry_path(tx, home, entry.node_id).await?;
        if old_path != path {
            let moves = home.join(".moves");
            storage::private_dir(&moves).await?;
            move_directory(&old_path, &path, &moves.join(node.id.to_string()), node.id).await?;
        }
    }
    owned_directory(&path, node.id).await?;
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
            let stage = path.join(".drive-rename");
            if stage.exists() {
                let info = tokio::fs::symlink_metadata(&stage).await?;
                if !info.is_file()
                    || info.file_type().is_symlink()
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
                if target.exists() {
                    tokio::fs::rename(&stage, previous.as_ref().unwrap()).await?;
                    return Err(Error::conflict("workspace_path_occupied"));
                }
            }
            if stage.exists() {
                tokio::fs::rename(stage, &target).await?;
            }
        }
        let valid = match tokio::fs::symlink_metadata(&target).await {
            Ok(info) if !info.is_file() || info.file_type().is_symlink() => {
                return Err(Error::conflict("workspace_path_occupied"));
            }
            Ok(info) => info.len() == row.get::<i64, _>("size") as u64,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        if !known || !valid {
            storage::capacity(&s.root, row.get::<i64, _>("size") as u64).await?;
            let temporary = path.join(".drive-copy");
            if tokio::fs::try_exists(&temporary).await? {
                let info = tokio::fs::symlink_metadata(&temporary).await?;
                if !info.is_file() || info.file_type().is_symlink() {
                    return Err(Error::conflict("workspace_path_occupied"));
                }
                storage::remove(&temporary).await?;
            }
            let source = storage::object(&s.root, directory, object, row.get("original_name"));
            tokio::fs::copy(&source, &temporary).await?;
            let f = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&temporary)
                .await?;
            f.sync_all().await?;
            drop(f);
            if !known && target.exists() && previous.as_ref() != Some(&target) {
                let (_, hash) = storage::checksum(&target).await?;
                if hash != row.get::<String, _>("sha256") {
                    return Err(Error::conflict("workspace_path_occupied"));
                }
            }
            if let Ok(info) = tokio::fs::symlink_metadata(&target).await
                && (!info.is_file() || info.file_type().is_symlink())
            {
                return Err(Error::conflict("workspace_path_occupied"));
            }
            tokio::fs::rename(&temporary, &target).await?;
            if let Some(previous) = previous.filter(|p| p != &target) {
                storage::remove(&previous).await?;
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
