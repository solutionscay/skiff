//! `drops@1`/`@2` and `image_cache@1`/`@2`: files the app drops into a pane, and the
//! list, pins and use of the cache that holds them. The requests keep their
//! image names (`ImageList`, `ImageRead`, ...): the library now holds every
//! kind of file.
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};

use skiff_client::cache::Who;
use skiff_core::config;
use skiff_host_api::{Error, HostCacheImage, HostCacheUse, HostImage, HostWho, Response, DATA_CHUNK};

use super::{failed, refused};

/// In-progress framed drops, each written to a temp file in the cache's
/// `.uploads` folder. A service restart forgets them; the next upload
/// removes a temp file older than a day. No partial file reaches the
/// library before the final frame.
static UPLOADS: LazyLock<Mutex<HashMap<String, Upload>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// The cap of a `drops@1` client's file, and of a one-frame drop.
const MAX_DROP: u64 = 50 * 1024 * 1024;
const UPLOADS_DIR: &str = ".uploads";

struct Upload { name: String, total: u64, received: u64, tmp: PathBuf, file: std::fs::File }

/// One frame of a drop. Framed uploads take a file up to this Host's
/// `[skiff_agent] cache_max_mb` (`drops@2`).
pub(super) fn drop_put(name: String, bytes: Vec<u8>, upload: Option<String>, total: Option<u64>, offset: u64, finish: bool, who: Option<HostWho>) -> Result<Response, Error> {
    let cfg = config::load().map_err(failed)?;
    let who = who.map(|w| Who { session: w.session, title: w.title, project: w.project });
    let Some(upload) = upload else {
        if bytes.len() as u64 > MAX_DROP { return Err(Error::Invalid { message: "the upload is larger than 50 MB".into() }); }
        let path = skiff_client::cache::put(&cfg, &name, &bytes, who.as_ref()).map_err(refused)?;
        return Ok(Response::Path { path: path.to_string_lossy().into_owned() });
    };
    let total = total.ok_or_else(|| Error::Invalid { message: "an upload needs its total size".into() })?;
    let max = skiff_client::cache::max_bytes(&cfg);
    if total > max {
        return Err(Error::Invalid { message: format!("{name} is larger than the {} MB size limit of Files on this Host.", max / (1024 * 1024)) });
    }
    let len = bytes.len() as u64;
    if len > DATA_CHUNK || offset > total || len > total - offset {
        return Err(Error::Invalid { message: "the upload is too large or out of range".into() });
    }
    let mut uploads = UPLOADS.lock().map_err(|_| Error::HostBusy)?;
    if !uploads.contains_key(&upload) {
        if offset != 0 {
            return Err(Error::Invalid { message: "the upload chunks are not in order".into() });
        }
        let dir = skiff_client::cache::ensure(&cfg).map_err(failed)?.join(UPLOADS_DIR);
        std::fs::create_dir_all(&dir).map_err(failed)?;
        clear_stale(&dir);
        let tmp = dir.join(uuid::Uuid::new_v4().to_string());
        let file = std::fs::File::create(&tmp).map_err(failed)?;
        uploads.insert(upload.clone(), Upload { name: name.clone(), total, received: 0, tmp, file });
    }
    let entry = uploads.get_mut(&upload).expect("present");
    if entry.name != name || entry.total != total || entry.received != offset {
        drop_upload(uploads.remove(&upload));
        return Err(Error::Invalid { message: "the upload chunks are not in order".into() });
    }
    if let Err(e) = entry.file.write_all(&bytes) {
        drop_upload(uploads.remove(&upload));
        return Err(failed(e));
    }
    entry.received += len;
    if !finish {
        return Ok(Response::Path { path: String::new() });
    }
    let done = uploads.remove(&upload).expect("present");
    drop(uploads);
    if done.received != total {
        drop_upload(Some(done));
        return Err(Error::Invalid { message: "the upload ended before all bytes arrived".into() });
    }
    let Upload { tmp, file, .. } = done;
    drop(file);
    let path = skiff_client::cache::put_file(&cfg, &name, &tmp, who.as_ref());
    let _ = std::fs::remove_file(&tmp);
    let path = path.map_err(refused)?;
    Ok(Response::Path { path: path.to_string_lossy().into_owned() })
}

fn drop_upload(upload: Option<Upload>) {
    if let Some(u) = upload {
        let _ = std::fs::remove_file(&u.tmp);
    }
}

/// Removes upload temp files a restart or a lost client left, after a day.
fn clear_stale(dir: &Path) {
    let day = std::time::Duration::from_secs(24 * 60 * 60);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > day);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

pub(super) fn image_list() -> Result<Response, Error> {
    let cfg = config::load().map_err(failed)?;
    let dir = skiff_client::cache::ensure(&cfg).map_err(failed)?;
    Ok(Response::Images { images: skiff_client::cache::list(&dir).into_iter().map(host_cache_image).collect() })
}

/// A library file's bytes. With `offset`, one range of it and its total
/// length (`image_cache@2`), at most [`DATA_CHUNK`] bytes.
pub(super) fn image_read(path: String, thumb: Option<u32>, offset: Option<u64>, length: Option<u64>) -> Result<Response, Error> {
    let cfg = config::load().map_err(failed)?;
    let dir = skiff_client::cache::ensure(&cfg).map_err(failed)?;
    if let Some(offset) = offset {
        let len = length.unwrap_or(DATA_CHUNK).min(DATA_CHUNK);
        let (bytes, total) = skiff_client::cache::read_range(&dir, Path::new(&path), offset, len).map_err(refused)?;
        return Ok(Response::Image { image: HostImage { mime: mime_of(&path).into(), bytes, total: Some(total) } });
    }
    let (mime, bytes) = cache_image(&dir, &path)?;
    let bytes = match thumb {
        Some(size) => scaled(&mime, bytes, size.clamp(16, 1024)),
        None => bytes,
    };
    Ok(Response::Image { image: HostImage { mime, bytes, total: None } })
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

/// An Insert of library files into `who`'s pane: each goes to the top with
/// a new mtime. The first path ends up newest.
pub(super) fn image_touch(paths: Vec<String>, who: Option<HostWho>) -> Result<Response, Error> {
    let cfg = config::load().map_err(failed)?;
    let dir = skiff_client::cache::ensure(&cfg).map_err(failed)?;
    let who = who.map(|w| Who { session: w.session, title: w.title, project: w.project });
    for path in paths.iter().rev() { skiff_client::cache::record(&dir, Path::new(path), None, who.as_ref()).map_err(refused)?; }
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

/// Reads only a library file and gives the webview its MIME type. A file
/// that is not an image is `application/octet-stream`.
fn cache_image(dir: &Path, path: &str) -> Result<(String, Vec<u8>), Error> {
    let bytes = skiff_client::cache::read(dir, Path::new(path)).map_err(refused)?;
    Ok((mime_of(path).into(), bytes))
}

fn mime_of(path: &str) -> &'static str {
    if !skiff_client::cache::is_image(Path::new(path)) {
        return "application/octet-stream";
    }
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default();
    match ext.to_ascii_lowercase().as_str() {
        "svg" => "image/svg+xml", "png" => "image/png", "jpg" => "image/jpeg", "gif" => "image/gif",
        "webp" => "image/webp", "bmp" => "image/bmp", "tif" => "image/tiff", "heic" => "image/heic", "avif" => "image/avif",
        _ => "application/octet-stream",
    }
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
