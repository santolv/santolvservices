use crate::{craft, json_error, server};
use image::{
    codecs::jpeg::JpegEncoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits,
};
use std::{
    collections::VecDeque,
    fs::File,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};
use tiny_http::{Header, Request, Response};

const MAX_SOURCE: u64 = 12 * 1024 * 1024;
type Source = (PathBuf, u64, SystemTime, Option<SystemTime>);
static CACHE: Mutex<VecDeque<(Source, Option<Vec<u8>>)>> = Mutex::new(VecDeque::new());

pub(super) fn respond(req: Request, path: &str) {
    let Some(logical) = super::safe_path(path) else {
        return json_error(req, 404, "Vista previa no disponible");
    };
    let full = super::document_path(&logical);
    let Some(bytes) = thumbnail(&logical, full) else {
        return json_error(req, 404, "Vista previa no disponible");
    };
    let tag = craft::revision(&bytes);
    let response = if server::header(&req, "If-None-Match") == Some(tag.as_str()) {
        Response::from_data(Vec::new()).with_status_code(304)
    } else {
        Response::from_data(bytes)
    };
    let _ = req.respond(
        response
            .with_header(Header::from_bytes("Content-Type", "image/jpeg").unwrap())
            .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap())
            .with_header(Header::from_bytes("Cache-Control", "private, no-cache").unwrap())
            .with_header(Header::from_bytes("ETag", tag).unwrap()),
    );
}

fn thumbnail(logical: &Path, full: PathBuf) -> Option<Vec<u8>> {
    let extension = logical.extension()?.to_str()?.to_ascii_lowercase();
    let office = matches!(extension.as_str(), "docx" | "pptx" | "xlsx");
    if !office && !matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif") {
        return None;
    }
    let file = File::open(&full).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_SOURCE {
        return None;
    }
    let source = (
        full,
        metadata.len(),
        metadata.modified().ok()?,
        metadata.created().ok(),
    );
    let mut cache = CACHE.lock().unwrap();
    if let Some(index) = cache.iter().position(|(key, _)| *key == source) {
        let item = cache.remove(index)?;
        let bytes = item.1.clone();
        cache.push_back(item);
        return bytes;
    }
    let bytes = load(file, office);
    // A single decoder bounds peak memory. The cache holds only small JPEGs or misses.
    if cache.len() >= 96 {
        cache.pop_front();
    }
    cache.push_back((source, bytes.clone()));
    bytes
}

fn load(file: File, office: bool) -> Option<Vec<u8>> {
    let bytes = if office {
        let mut archive = zip::ZipArchive::new(file).ok()?;
        let name = [
            "docProps/thumbnail.jpeg",
            "docProps/thumbnail.jpg",
            "docProps/thumbnail.png",
        ]
        .into_iter()
        .find(|name| archive.index_for_name(name).is_some())?;
        let embedded = archive.by_name(name).ok()?;
        server::read_limited(embedded, 512 * 1024).ok()?
    } else {
        server::read_limited(file, MAX_SOURCE).ok()?
    };
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    if !matches!(
        reader.format()?,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP | ImageFormat::Gif
    ) {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > 10_000_000 {
        return None;
    }
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    if width > 320 || height > 224 {
        image = image.thumbnail(320, 224);
    }
    let resized = image.into_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 70)
        .encode_image(&resized)
        .ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_shrink_images_and_skip_office_files_without_thumbnails() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.png");
        image::RgbImage::new(800, 400).save(&photo).unwrap();
        let preview = thumbnail(&photo, photo.clone()).unwrap();
        assert_eq!(image::load_from_memory(&preview).unwrap().width(), 320);
        assert_eq!(thumbnail(&photo, photo.clone()).unwrap(), preview);
        let office = dir.path().join("blank.docx");
        std::fs::write(&office, include_bytes!("../blank.docx")).unwrap();
        assert!(thumbnail(&office, office.clone()).is_none());
    }
}
