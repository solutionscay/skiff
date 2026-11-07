//! `drops@1` and the image cache: files the app drops into a pane, and the
//! list, pins and use of the cache that holds them.
use std::{
    collections::HashMap,
    path::Path,
    sync::{LazyLock, Mutex},
};

use skiff_core::config;
use skiff_host_api::{Error, HostCacheImage, HostCacheUse, HostImage, Response};

use super::{failed, refused};

/// In-progress framed drops. A service restart discards them; no partial file
/// reaches the cache before the final frame.
static UPLOADS: LazyLock<Mutex<HashMap<String, Upload>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
const MAX_DROP: usize = 50 * 1024 * 1024;

struct Upload { name: String, total: usize, bytes: Vec<u8> }

pub(super) fn drop_put(name: String, bytes: Vec<u8>, upload: Option<String>, total: Option<u64>, offset: u64, finish: bool) -> Result<Response, Error> {
    let cfg = config::load().map_err(failed)?;
    let bytes = if let Some(upload) = upload {
        let total = total.ok_or_else(|| Error::Invalid { message: "an upload needs its total size".into() })? as usize;
        if total > MAX_DROP || bytes.len() > MAX_DROP || offset as usize > total || bytes.len() > total.saturating_sub(offset as usize) {
            return Err(Error::Invalid { message: "the upload is too large or out of range".into() });
        }
        let mut uploads = UPLOADS.lock().map_err(|_| Error::HostBusy)?;
        let entry = uploads.entry(upload.clone()).or_insert_with(|| Upload { name: name.clone(), total, bytes: Vec::new() });
        if entry.name != name || entry.total != total || entry.bytes.len() != offset as usize {
            uploads.remove(&upload);
            return Err(Error::Invalid { message: "the upload chunks are not in order".into() });
        }
        entry.bytes.extend_from_slice(&bytes);
        if !finish {
            return Ok(Response::Path { path: String::new() });
        }
        if entry.bytes.len() != total {
            uploads.remove(&upload);
            return Err(Error::Invalid { message: "the upload ended before all bytes arrived".into() });
        }
        uploads.remove(&upload).expect("present").bytes
    } else {
        bytes
    };
    if bytes.len() > MAX_DROP { return Err(Error::Invalid { message: "the upload is larger than 50 MB".into() }); }
    let path = skiff_client::cache::put(&cfg, &name, &bytes).map_err(refused)?;
    Ok(Response::Path { path: path.to_string_lossy().into_owned() })
}

pub(super) fn image_list() -> Result<Response, Error> {
    let cfg = config::load().map_err(failed)?;
    let dir = skiff_client::cache::ensure(&cfg).map_err(failed)?;
    Ok(Response::Images { images: skiff_client::cache::list(&dir).into_iter().map(host_cache_image).collect() })
}

pub(super) fn image_read(path: String, thumb: Option<u32>) -> Result<Response, Error> {
    let cfg = config::load().map_err(failed)?;
    let dir = skiff_client::cache::ensure(&cfg).map_err(failed)?;
    let (mime, bytes) = cache_image(&dir, &path)?;
    let bytes = match thumb {
        Some(size) => scaled(&mime, bytes, size.clamp(16, 1024)),
        None => bytes,
    };
    Ok(Response::Image { image: HostImage { mime, bytes } })
}

/// A PNG or JPEG at most `size` pixels on its long side, in its own format.
/// Any other image, a smaller one, or one that does not decode: as it is.
fn scaled(mime: &str, bytes: Vec<u8>, size: u32) -> Vec<u8> {
    let format = match mime {
        "image/png" => image::ImageFormat::Png,
        "image/jpeg" => image::ImageFormat::Jpeg,
        _ => return bytes,
    };
    let Ok(img) = image::load_from_memory_with_format(&bytes, format) else { return bytes };
    if img.width().max(img.height()) <= size {
        return bytes;
    }
    let small = img.thumbnail(size, size);
    // JPEG has no alpha.
    let small = if format == image::ImageFormat::Jpeg { image::DynamicImage::ImageRgb8(small.to_rgb8()) } else { small };
    let mut out = std::io::Cursor::new(Vec::new());
    match small.write_to(&mut out, format) {
        Ok(()) if out.get_ref().len() < bytes.len() => out.into_inner(),
        _ => bytes,
    }
}

pub(super) fn image_delete(paths: Vec<String>) -> Result<Response, Error> {
    let cfg = config::load().map_err(failed)?;
    let dir = skiff_client::cache::ensure(&cfg).map_err(failed)?;
    for path in paths { skiff_client::cache::delete(&dir, Path::new(&path)).map_err(refused)?; }
    Ok(Response::Ok)
}

pub(super) fn image_pin(paths: Vec<String>, on: bool) -> Result<Response, Error> {
    let cfg = config::load().map_err(failed)?;
    let dir = skiff_client::cache::ensure(&cfg).map_err(failed)?;
    for path in paths { skiff_client::cache::pin(&dir, Path::new(&path), on).map_err(refused)?; }
    Ok(Response::Ok)
}

pub(super) fn cache_use() -> Result<Response, Error> {
    let cfg = config::load().map_err(failed)?;
    let dir = skiff_client::cache::ensure(&cfg).map_err(failed)?;
    let images = skiff_client::cache::list(&dir);
    let image_bytes = images.iter().map(|image| image.size).sum();
    let bytes = cache_bytes(&dir).map_err(failed)?;
    Ok(Response::CacheUse { usage: HostCacheUse { bytes, images: images.len() as u64, image_bytes } })
}

fn host_cache_image(image: skiff_client::cache::Image) -> HostCacheImage {
    HostCacheImage {
        path: image.path,
        file: image.entry.file,
        name: image.entry.name,
        session: image.entry.session,
        title: image.entry.title,
        project: image.entry.project,
        at: image.entry.at,
        pinned: image.entry.pinned,
        size: image.size,
    }
}

/// Reads only a direct image cache entry and gives the webview its MIME type.
fn cache_image(dir: &Path, path: &str) -> Result<(String, Vec<u8>), Error> {
    let bytes = skiff_client::cache::read(dir, Path::new(path)).map_err(refused)?;
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default();
    let mime = match ext.to_ascii_lowercase().as_str() {
        "svg" => "image/svg+xml", "png" => "image/png", "jpg" => "image/jpeg", "gif" => "image/gif",
        "webp" => "image/webp", "bmp" => "image/bmp", "tif" => "image/tiff", "heic" => "image/heic", "avif" => "image/avif",
        _ => return Err(Error::NotFound { message: "the image is not in this Host's cache".into() }),
    };
    Ok((mime.into(), bytes))
}

fn cache_bytes(dir: &Path) -> anyhow::Result<u64> {
    let mut bytes = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() { bytes += meta.len(); }
        if meta.is_dir() { bytes += cache_bytes(&entry.path())?; }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(w, h).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_thumb_is_at_most_its_size_and_a_small_image_stays_as_it_is() {
        let small = scaled("image/png", png(1600, 800), 160);
        let img = image::load_from_memory_with_format(&small, image::ImageFormat::Png).unwrap();
        assert_eq!((img.width(), img.height()), (160, 80));
        let tiny = png(100, 50);
        assert_eq!(scaled("image/png", tiny.clone(), 160), tiny);
        assert_eq!(scaled("image/svg+xml", b"<svg/>".to_vec(), 160), b"<svg/>");
    }
}
