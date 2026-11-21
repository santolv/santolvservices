use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

pub fn temp(root: &Path, id: Uuid) -> PathBuf {
    root.join("uploads").join(id.to_string())
}
pub fn object(root: &Path, id: Uuid) -> PathBuf {
    root.join("objects").join(id.to_string())
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
