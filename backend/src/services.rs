use crate::{
    App,
    error::{Error, Result},
    storage,
};
use sqlx::Row;

// Run before accepting requests or starting workers; SQL records the source across restarts.
pub async fn migrate_storage(s: &App) -> Result<()> {
    let mut tx = s.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('service-storage-migration',0))")
        .execute(&mut *tx)
        .await?;
    let rows = sqlx::query("SELECT id,legacy_storage_directory,storage_directory FROM spaces WHERE legacy_storage_directory IS NOT NULL FOR UPDATE").fetch_all(&mut *tx).await?;
    for row in rows {
        let id: uuid::Uuid = row.get("id");
        let old: String = row.get("legacy_storage_directory");
        let new: String = row.get("storage_directory");
        let source = s.root.join(&old);
        let target = s.root.join(&new);
        // Validate the legacy path before using any database value as a filesystem path.
        let components: Vec<_> = old.split('/').collect();
        if components.len() != 2
            || !matches!(components[0], "usuarios" | "usuarios_compat")
            || components[1].is_empty()
            || components[1].contains(['\\', ':'])
            || matches!(components[1], "." | "..")
        {
            return Err(Error::conflict("invalid_storage_directory"));
        }
        let exists = tokio::fs::try_exists(&source).await?;
        if exists {
            storage::private_dir(&s.root.join(components[0])).await?;
            if tokio::fs::try_exists(&target).await? {
                return Err(Error::conflict("storage_layout_conflict"));
            }
            let info = tokio::fs::symlink_metadata(&source).await?;
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
                "SELECT j.node_id,j.source AS path FROM workspace_moves j JOIN nodes n ON n.id=j.node_id WHERE n.space_id=$1",
                "UPDATE workspace_moves SET source=$2 WHERE node_id=$1",
            ),
            (
                "SELECT j.node_id,j.target AS path FROM workspace_moves j JOIN nodes n ON n.id=j.node_id WHERE n.space_id=$1",
                "UPDATE workspace_moves SET target=$2 WHERE node_id=$1",
            ),
            (
                "SELECT j.node_id,j.target AS path FROM workspace_publications j JOIN nodes n ON n.id=j.node_id WHERE n.space_id=$1",
                "UPDATE workspace_publications SET target=$2 WHERE node_id=$1",
            ),
        ] {
            let entries = sqlx::query(select).bind(id).fetch_all(&mut *tx).await?;
            for entry in entries {
                let path: String = entry.get("path");
                if let Ok(suffix) = std::path::Path::new(&path).strip_prefix(&source) {
                    sqlx::query(update)
                        .bind(entry.get::<uuid::Uuid, _>("node_id"))
                        .bind(target.join(suffix).to_string_lossy().as_ref())
                        .execute(&mut *tx)
                        .await?;
                }
            }
        }
        sqlx::query("UPDATE spaces SET legacy_storage_directory=NULL WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
