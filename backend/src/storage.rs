use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

pub fn temp(root: &Path, _directory: &str, id: Uuid) -> PathBuf {
    root.join(".temporal").join("cargas").join(id.to_string())
}
pub fn object(root: &Path, directory: &str, id: Uuid, name: &str) -> PathBuf {
    let _ = directory;
    root.join(".temporal")
        .join("publicaciones")
        .join(id.to_string())
        .join(disk_filename(name))
}
pub fn disk_filename(name: &str) -> String {
    let mut value: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if value.ends_with(['.', ' ']) {
        value = format!("{}_", value.trim_end_matches(['.', ' ']));
    }
    if value.is_empty() {
        value = "archivo".into();
    }
    if !portable_username(&value) {
        value.insert(0, '_');
    }
    while value.len() > 255 {
        value.pop();
    }
    value
}
pub fn attachment(name: &str) -> String {
    let fallback: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}
pub fn portable_username(value: &str) -> bool {
    let base = value.split('.').next().unwrap_or("").to_ascii_lowercase();
    !(value.ends_with('.')
        || matches!(base.as_str(), "con" | "prn" | "aux" | "nul")
        || (base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && base.as_bytes()[3].is_ascii_digit()))
}
pub fn user_directory(username: &str, id: Uuid) -> String {
    if (3..=64).contains(&username.len())
        && portable_username(username)
        && username.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.')
        })
    {
        format!("usuarios/{username}")
    } else {
        format!("usuarios_compat/{id}")
    }
}
pub(crate) async fn private_dir(path: &Path) -> Result<()> {
    match tokio::fs::create_dir(path).await {
        Ok(()) => {
            crate::permissions::private_directory(path)?;
            if let Some(parent) = path.parent() {
                sync_dir(parent).await?;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let info = tokio::fs::symlink_metadata(path).await?;
            if !info.is_dir() || info.file_type().is_symlink() {
                return Err(Error::conflict("invalid_storage_directory"));
            }
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
pub async fn prepare_space(root: &Path, directory: &str) -> Result<()> {
    let (branch, component) = directory
        .split_once('/')
        .ok_or_else(|| Error::bad("invalid_storage_directory"))?;
    let valid = match branch {
        "usuarios" => user_directory(component, Uuid::nil()) == directory,
        "usuarios_compat" | "organizaciones" => component.parse::<Uuid>().is_ok(),
        _ => false,
    };
    if !valid || component.contains(['/', '\\']) {
        return Err(Error::bad("invalid_storage_directory"));
    }
    private_dir(&root.join(branch)).await?;
    let home = root.join(directory);
    private_dir(&home).await?;
    private_dir(&home.join("archivos")).await?;
    private_dir(&root.join(".temporal")).await?;
    for name in ["cargas", "publicaciones", "movimientos"] {
        private_dir(&root.join(".temporal").join(name)).await?;
    }
    cleanup_legacy_roots(&home).await?;
    Ok(())
}
pub async fn cleanup_legacy_roots(root: &Path) -> Result<()> {
    for kind in [
        "objects",
        "uploads",
        ".uploads",
        ".versions",
        ".moves",
        ".papelera",
    ] {
        let base = root.join(kind);
        if matches!(kind, ".versions" | ".moves" | ".papelera")
            && let Ok(mut entries) = tokio::fs::read_dir(&base).await
        {
            for _ in 0..100 {
                let Some(entry) = entries.next_entry().await? else {
                    break;
                };
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|n| n.parse::<Uuid>().is_ok())
                    && entry.file_type().await?.is_dir()
                {
                    match tokio::fs::remove_dir(entry.path()).await {
                        Ok(()) => sync_dir(&base).await?,
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::NotFound
                                    | std::io::ErrorKind::DirectoryNotEmpty
                            ) => {}
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
        match tokio::fs::remove_dir(root.join(kind)).await {
            Ok(()) => sync_dir(root).await?,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
// Same-volume migration is resumable; never copy large files into RAM or replace existing content.
pub async fn prepare_object(root: &Path, directory: &str, id: Uuid, name: &str) -> Result<PathBuf> {
    prepare_space(root, directory).await?;
    let path = object(root, directory, id, name);
    private_dir(path.parent().unwrap()).await?;
    Ok(path)
}
pub async fn remove_object(root: &Path, directory: &str, id: Uuid, name: &str) -> Result<()> {
    let path = object(root, directory, id, name);
    remove(&path).await?;
    match tokio::fs::remove_dir(path.parent().unwrap()).await {
        Ok(()) => sync_dir(&root.join(".temporal/publicaciones")).await?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
pub async fn relocate_legacy(root: &Path, directory: &str, id: Uuid, name: &str) -> Result<()> {
    for (old, new) in [
        (
            root.join(directory).join(".uploads").join(id.to_string()),
            temp(root, directory, id),
        ),
        (
            root.join(directory)
                .join(".versions")
                .join(id.to_string())
                .join(disk_filename(name)),
            object(root, directory, id, name),
        ),
    ] {
        if tokio::fs::try_exists(&old).await? {
            prepare_object(root, directory, id, name).await?;
            if tokio::fs::try_exists(&new).await? {
                return Err(Error::conflict("storage_layout_conflict"));
            }
            tokio::fs::rename(&old, &new).await?;
            sync_dir(new.parent().unwrap()).await?;
            sync_dir(old.parent().unwrap()).await?;
            // An archive directory is removed only when empty.
            if old
                .parent()
                .unwrap()
                .file_name()
                .is_some_and(|n| n == id.to_string().as_str())
            {
                let _ = tokio::fs::remove_dir(old.parent().unwrap()).await;
            }
        }
    }
    let previous = root
        .join(directory)
        .join("archivos")
        .join(id.to_string())
        .join(disk_filename(name));
    if tokio::fs::try_exists(&previous).await?
        && !tokio::fs::try_exists(previous.parent().unwrap().join(".drive-node")).await?
        && tokio::fs::symlink_metadata(&previous).await?.is_file()
    {
        let new = prepare_object(root, directory, id, name).await?;
        if tokio::fs::try_exists(&new).await? {
            return Err(Error::conflict("storage_layout_conflict"));
        }
        tokio::fs::rename(&previous, &new).await?;
        sync_dir(new.parent().unwrap()).await?;
        sync_dir(previous.parent().unwrap()).await?;
        match tokio::fs::remove_dir(previous.parent().unwrap()).await {
            Ok(()) => sync_dir(&root.join(directory).join("archivos")).await?,
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => (),
            Err(e) => return Err(e.into()),
        }
    }
    for (kind, base) in [
        ("uploads", root.to_owned()),
        ("uploads", root.join(directory)),
        ("objects", root.to_owned()),
        ("objects", root.join(directory)),
    ] {
        let old = base.join(kind).join(id.to_string());
        if !tokio::fs::try_exists(&old).await? {
            continue;
        }
        prepare_space(root, directory).await?;
        let new = if kind == "objects" {
            prepare_object(root, directory, id, name).await?
        } else {
            temp(root, directory, id)
        };
        if tokio::fs::try_exists(&new).await? {
            return Err(Error::conflict("storage_layout_conflict"));
        }
        match tokio::fs::rename(&old, &new).await {
            Ok(()) => {
                sync_dir(new.parent().unwrap()).await?;
                sync_dir(old.parent().unwrap()).await?;
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    && tokio::fs::try_exists(&new).await? => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
pub async fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let p = path.to_owned();
        tokio::task::spawn_blocking(move || std::fs::File::open(p)?.sync_all())
            .await
            .map_err(|_| Error::unavailable())??;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
pub async fn checksum(path: &Path) -> Result<(i64, String)> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut b = vec![0; 256 * 1024];
    let mut h = Sha256::new();
    let mut size = 0i64;
    loop {
        let n = f.read(&mut b).await?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
        size += n as i64;
    }
    Ok((size, hex::encode(h.finalize())))
}
pub async fn remove(path: &Path) -> Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => sync_dir(path.parent().unwrap_or_else(|| Path::new("."))).await,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub async fn capacity(root: &Path, required: u64) -> Result<()> {
    let p = root.to_owned();
    let free = tokio::task::spawn_blocking(move || fs2::available_space(p))
        .await
        .map_err(|_| Error::unavailable())??;
    if free < required.saturating_add(64 * 1024 * 1024) {
        return Err(Error(
            axum::http::StatusCode::INSUFFICIENT_STORAGE,
            "storage_full",
        ));
    }
    Ok(())
}
pub fn range(h: Option<&str>, size: u64) -> Result<(u64, u64, bool)> {
    let Some(s) = h else {
        return Ok((0, size, false));
    };
    let bad = || {
        Error(
            axum::http::StatusCode::RANGE_NOT_SATISFIABLE,
            "invalid_range",
        )
    };
    let (a, b) = s
        .strip_prefix("bytes=")
        .and_then(|s| s.split_once('-'))
        .ok_or_else(bad)?;
    if size == 0 || b.contains(',') {
        return Err(bad());
    }
    let (start, end) = if a.is_empty() {
        let suffix: u64 = b.parse().map_err(|_| bad())?;
        if suffix == 0 {
            return Err(bad());
        }
        (size.saturating_sub(suffix), size - 1)
    } else {
        let start: u64 = a.parse().map_err(|_| bad())?;
        let end = if b.is_empty() {
            size - 1
        } else {
            b.parse::<u64>().map_err(|_| bad())?.min(size - 1)
        };
        (start, end)
    };
    if start >= size || end < start {
        return Err(bad());
    }
    Ok((start, end - start + 1, true))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_names_preserve_extensions_and_download_headers_are_safe() {
        assert_eq!(disk_filename("Informe ñ.pdf"), "Informe ñ.pdf");
        for name in ["../escape.txt", "CON.txt", "bad:name?.pdf", ".."] {
            let safe = disk_filename(name);
            assert!(!safe.contains(['/', '\\', ':', '?']));
            assert!(!matches!(safe.as_str(), "." | ".."));
            assert!(portable_username(&safe));
        }
        let header = attachment("Informe ñ.pdf\r\n");
        assert!(!header.contains(['\r', '\n']));
        assert!(header.contains("filename*=UTF-8''Informe%20%C3%B1.pdf%0D%0A"));
        assert!(header.parse::<axum::http::HeaderValue>().is_ok());
    }
    #[tokio::test]
    async fn workspaces_are_private_safe_and_legacy_moves_are_resumable() {
        let root = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        assert_eq!(user_directory("alice", id), "usuarios/alice");
        for invalid in [
            "con",
            "aux.txt",
            "com1",
            "name.",
            "../escape",
            "legacy@example.com",
        ] {
            assert_eq!(user_directory(invalid, id), format!("usuarios_compat/{id}"));
        }
        for invalid in [
            "../escape",
            "usuarios/../escape",
            "usuarios/aux",
            "usuarios/alice/objects",
            "organizaciones/../escape",
        ] {
            assert!(prepare_space(root.path(), invalid).await.is_err());
        }
        prepare_space(root.path(), "usuarios/alice").await.unwrap();
        assert!(root.path().join("usuarios/alice/archivos").is_dir());
        tokio::fs::create_dir(root.path().join("objects"))
            .await
            .unwrap();
        let old = root.path().join("objects").join(id.to_string());
        tokio::fs::write(&old, b"custom\0content").await.unwrap();
        relocate_legacy(root.path(), "usuarios/alice", id, "original.bin")
            .await
            .unwrap();
        relocate_legacy(root.path(), "usuarios/alice", id, "original.bin")
            .await
            .unwrap();
        let new = object(root.path(), "usuarios/alice", id, "original.bin");
        assert_eq!(tokio::fs::read(&new).await.unwrap(), b"custom\0content");
        assert!(!old.exists());
        tokio::fs::write(&old, b"different").await.unwrap();
        assert!(
            relocate_legacy(root.path(), "usuarios/alice", id, "original.bin")
                .await
                .is_err()
        );
        assert_eq!(tokio::fs::read(&new).await.unwrap(), b"custom\0content");
        assert_eq!(tokio::fs::read(&old).await.unwrap(), b"different");
    }
    #[test]
    fn byte_ranges() {
        assert_eq!(range(Some("bytes=2-5"), 10).unwrap(), (2, 4, true));
        assert_eq!(range(Some("bytes=-3"), 10).unwrap(), (7, 3, true));
        assert_eq!(range(Some("bytes=9-99"), 10).unwrap(), (9, 1, true));
        for r in ["bytes=10-", "bytes=-0", "bytes=0-1,4-5", "bytes=5-2"] {
            assert!(range(Some(r), 10).is_err());
        }
        assert_eq!(range(None, 0).unwrap(), (0, 0, false));
    }
    #[tokio::test]
    async fn persistent_checksum() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("test");
        tokio::fs::write(&p, b"abc").await.unwrap();
        let (n, h) = checksum(&p).await.unwrap();
        assert_eq!(n, 3);
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        remove(&p).await.unwrap();
        remove(&p).await.unwrap();
    }
    #[tokio::test]
    async fn refuses_capacity_that_cannot_fit() {
        let d = tempfile::tempdir().unwrap();
        let e = capacity(d.path(), u64::MAX).await.unwrap_err();
        assert_eq!(e.0, axum::http::StatusCode::INSUFFICIENT_STORAGE);
    }
}
