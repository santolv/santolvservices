use crate::{
    App,
    auth::User,
    error::{Error, Result},
    files, storage,
};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use image::ImageDecoder;
use sqlx::Row;
use std::{io::BufReader, time::Duration};
use uuid::Uuid;

// One decoder in the process, including when more than one app instance is constructed.
static PROCESSOR: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

fn encode(source: impl std::io::BufRead + std::io::Seek) -> std::result::Result<Vec<u8>, String> {
    let mut reader = image::ImageReader::new(source)
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(1024 * 1024 * 1024);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 200_000_000 {
        return Err("image_limits".into());
    }
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    image.apply_orientation(orientation);
    let variant = |size: u32| -> std::result::Result<Vec<u8>, String> {
        let resized = if image.width().max(image.height()) > size {
            image.thumbnail(size, size)
        } else {
            image.clone()
        };
        let pixels = resized.to_rgba8();
        let mut output = Vec::new();
        // Encoding pixels alone deliberately excludes EXIF, XMP and GPS.
        image_webp::WebPEncoder::new(&mut output)
            .encode(
                &pixels,
                pixels.width(),
                pixels.height(),
                image_webp::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
        Ok(output)
    };
    variant(1280)
}

pub async fn run(app: App) {
    loop {
        if let Err(error) = process(&app).await {
            tracing::warn!(code = error.1, "photo derivatives pass failed");
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn process(app: &App) -> Result<()> {
    let Some(_lease) = crate::core::manager::lease(&app.db, "photos").await? else {
        return Ok(());
    };
    let _permit = PROCESSOR
        .acquire()
        .await
        .map_err(|_| Error::unavailable())?;
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('photos-derivatives',0))")
        .execute(&mut *tx)
        .await?;
    let row=sqlx::query("SELECT n.id,n.space_id,n.current_version,v.object_id,o.original_name,s.storage_directory,o.size FROM nodes n JOIN derivative_jobs j ON j.node_id=n.id AND j.version_id=n.current_version JOIN versions v ON v.id=n.current_version JOIN objects o ON o.id=v.object_id JOIN spaces s ON s.id=n.space_id WHERE n.kind='file' AND n.trashed_at IS NULL AND s.deleted_at IS NULL AND o.state='ready' AND n.name ~* '[.](jpe?g|png|webp|gif|avif)$' AND NOT EXISTS(SELECT 1 FROM derivatives d WHERE d.node_id=n.id AND d.version_id=n.current_version) AND NOT EXISTS(SELECT 1 FROM workspace_jobs j WHERE j.node_id=n.id AND NOT j.audit) ORDER BY n.created_at LIMIT 1")
        .fetch_optional(&mut *tx).await?;
    if let Some(row) = row {
        let id: Uuid = row.get("id");
        files::lock(&mut tx, row.get("space_id")).await?;
        if files::node(&mut tx, id).await?.current_version != Some(row.get("current_version")) {
            return Ok(());
        }
        let path = crate::workspace::content_path_tx(
            &mut tx,
            &app.root,
            row.get("storage_directory"),
            row.get("object_id"),
            row.get("original_name"),
        )
        .await?;
        let result = async {
            let file = storage::open_verified(&path).await?;
            if file.metadata().await?.len() > 256 * 1024 * 1024 {
                return Err(Error::bad("image_limits"));
            }
            let source = BufReader::new(file.into_std().await);
            tokio::task::spawn_blocking(move || encode(source))
                .await
                .map_err(|_| Error::unavailable())?
                .map_err(|_| Error::bad("invalid_image"))
        }
        .await;
        let (preview, error) = match result {
            Ok(preview) => (Some(preview), false),
            Err(error) if error.0 == StatusCode::BAD_REQUEST => (None, true),
            Err(error) => return Err(error),
        };
        sqlx::query("INSERT INTO derivatives(node_id,version_id,preview,error) VALUES($1,$2,$3,$4) ON CONFLICT(node_id) DO UPDATE SET version_id=excluded.version_id,preview=excluded.preview,error=excluded.error")
            .bind(id).bind(row.get::<Uuid,_>("current_version")).bind(preview).bind(error).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM derivative_jobs WHERE node_id IN (SELECT node_id FROM derivatives)")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn get(
    State(app): State<App>,
    user: User,
    Path((id, size)): Path<(Uuid, u32)>,
    headers: HeaderMap,
) -> Result<Response> {
    if size != 1280 {
        return Err(Error::bad("invalid_image_size"));
    }
    let mut tx = app.db.begin().await?;
    let node = files::access(&mut tx, id, user.id, 1, false).await?;
    let row = sqlx::query(
        "SELECT preview AS content,error FROM derivatives WHERE node_id=$1 AND version_id=$2",
    )
    .bind(id)
    .bind(node.current_version)
    .fetch_optional(&mut *tx)
    .await?;
    let mut response = if let Some(row) = row {
        if row.get::<bool, _>("error") {
            return Err(Error::bad("invalid_image"));
        }
        let etag = format!(
            "\"{}-{size}\"",
            node.current_version.ok_or_else(Error::denied)?
        );
        let mut response =
            if headers.get("if-none-match").and_then(|v| v.to_str().ok()) == Some(&etag) {
                StatusCode::NOT_MODIFIED.into_response()
            } else {
                let bytes: Vec<u8> = row.get("content");
                Body::from(bytes).into_response()
            };
        response.headers_mut().insert("etag", etag.parse().unwrap());
        response
            .headers_mut()
            .insert("content-type", "image/webp".parse().unwrap());
        response
    } else {
        let mut response = StatusCode::ACCEPTED.into_response();
        response
            .headers_mut()
            .insert("retry-after", "2".parse().unwrap());
        response
    };
    response
        .headers_mut()
        .insert("cache-control", "private, no-cache".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn variants_preserve_original_ratio_and_never_upscale() {
        for (width, height) in [(2000, 1000), (40, 80), (6000, 3000)] {
            let original = image::DynamicImage::new_rgb8(width, height);
            let mut source = Cursor::new(Vec::new());
            original
                .write_to(&mut source, image::ImageFormat::Png)
                .unwrap();
            let bytes = source.into_inner();
            let retained = bytes.clone();
            let preview = encode(Cursor::new(bytes)).unwrap();
            assert_eq!(&preview[0..4], b"RIFF");
            assert!(!preview.windows(4).any(|w| matches!(w, b"EXIF" | b"XMP ")));
            let result = image::load_from_memory(&preview).unwrap();
            assert!(result.width() <= width && result.height() <= height);
            assert!(result.width().max(result.height()) <= 1280);
            assert_eq!(result.width() * height, result.height() * width);
            assert_eq!(image::load_from_memory(&retained).unwrap().width(), width);
        }
        assert!(encode(Cursor::new(b"not an image")).is_err());
    }
}
