use crate::{
    App,
    error::{Error, Result},
    storage,
};
use sqlx::{Postgres, Row, Transaction};

// Run before accepting requests or starting workers; SQL records the source across restarts.
pub async fn migrate_storage(s: &App) -> Result<()> {
    let mut tx = s.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('service-storage-migration',0))")
        .execute(&mut *tx)
        .await?;
    migrate_spaces(s, &mut tx, false).await?;
    sqlx::query("SET LOCAL search_path TO photos, public")
        .execute(&mut *tx)
        .await?;
    migrate_spaces(s, &mut tx, true).await?;
    tx.commit().await?;
    Ok(())
}

async fn migrate_spaces(s: &App, tx: &mut Transaction<'_, Postgres>, photos: bool) -> Result<()> {
    let rows = sqlx::query("SELECT id,owner_id,legacy_storage_directory,storage_directory FROM spaces WHERE legacy_storage_directory IS NOT NULL FOR UPDATE").fetch_all(&mut **tx).await?;
    for row in rows {
        let id: uuid::Uuid = row.get("id");
        let old: String = row.get("legacy_storage_directory");
        let new: String = row.get("storage_directory");
        let source = s.root.join(&old);
        let target = s.root.join(&new);
        // Validate the legacy path before using any database value as a filesystem path.
        let components: Vec<_> = old.split('/').collect();
        let owner: uuid::Uuid = row.get("owner_id");
        let parent = if photos {
            if old != format!("usuarios/{owner}/photos") || new != storage::photos_directory(owner)
            {
                return Err(Error::conflict("invalid_storage_directory"));
            }
            // Validates every ancestor and creates Drive before moving the old Photos tree.
            storage::prepare_space(&s.root, &storage::user_directory("", owner)).await?;
            s.root.join("usuarios").join(owner.to_string())
        } else if let [branch @ ("usuarios" | "usuarios_compat"), name] = components.as_slice()
            && !name.is_empty()
            && !name.contains(['\\', ':'])
            && !matches!(*name, "." | "..")
        {
            s.root.join(branch)
        } else {
            return Err(Error::conflict("invalid_storage_directory"));
        };
        storage::private_dir(&parent).await?;
        if let Some(info) = dir_metadata(&source).await? {
            if dir_metadata(&target).await?.is_some() {
                return Err(Error::conflict("storage_layout_conflict"));
            }
            if !info.is_dir() || storage::unsafe_link(&info) {
                return Err(Error::conflict("invalid_storage_directory"));
            }
            storage::private_dir(&s.root.join("usuarios")).await?;
            storage::private_dir(target.parent().ok_or_else(Error::unavailable)?).await?;
            storage::rename(&source, &target).await?;
            storage::sync_dir(source.parent().unwrap()).await?;
            storage::sync_dir(target.parent().unwrap()).await?;
        }
        storage::prepare_space(&s.root, &new).await?;
        // Outstanding publication/move journals contain absolute paths as well as node IDs.
        for (select, update) in [
            (
                "SELECT j.node_id,j.source AS path FROM workspace_moves j WHERE j.node_id IN (SELECT id FROM nodes WHERE space_id=$1 UNION SELECT node_id FROM workspace_jobs WHERE space_id=$1 UNION SELECT node_id FROM workspace_entries WHERE space_id=$1)",
                "UPDATE workspace_moves SET source=$2 WHERE node_id=$1",
            ),
            (
                "SELECT j.node_id,j.target AS path FROM workspace_moves j WHERE j.node_id IN (SELECT id FROM nodes WHERE space_id=$1 UNION SELECT node_id FROM workspace_jobs WHERE space_id=$1 UNION SELECT node_id FROM workspace_entries WHERE space_id=$1)",
                "UPDATE workspace_moves SET target=$2 WHERE node_id=$1",
            ),
            (
                "SELECT j.node_id,j.target AS path FROM workspace_publications j WHERE j.node_id IN (SELECT id FROM nodes WHERE space_id=$1 UNION SELECT node_id FROM workspace_jobs WHERE space_id=$1 UNION SELECT node_id FROM workspace_entries WHERE space_id=$1)",
                "UPDATE workspace_publications SET target=$2 WHERE node_id=$1",
            ),
        ] {
            let entries = sqlx::query(select).bind(id).fetch_all(&mut **tx).await?;
            for entry in entries {
                let path: String = entry.get("path");
                if let Ok(suffix) = std::path::Path::new(&path).strip_prefix(&source) {
                    sqlx::query(update)
                        .bind(entry.get::<uuid::Uuid, _>("node_id"))
                        .bind(target.join(suffix).to_string_lossy().as_ref())
                        .execute(&mut **tx)
                        .await?;
                }
            }
        }
        sqlx::query("UPDATE spaces SET legacy_storage_directory=NULL WHERE id=$1")
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn dir_metadata(path: &std::path::Path) -> Result<Option<std::fs::Metadata>> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
