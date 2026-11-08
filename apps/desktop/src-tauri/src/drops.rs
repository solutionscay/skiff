//! Files dropped on a pane, Skiff's Paste of a file list or an image, and
//! the Images library. Images go to the cache ([`skiff_client::cache`]),
//! which is the library. The page types the words with `term.paste()`, so an
//! agent that reads a pasted path attaches the image. Skiff never presses
//! Enter.
//!
//! On a remote Host's pane each file goes to that Host (`drops@1`), and the
//! pane gets the Host path. An image also stays in this computer's cache,
//! so Images lists it here. Insert from Images sends the file the same way.

use std::path::{Path, PathBuf};

use skiff_client::{
    cache::{self, Who},
    host::data,
};
use tauri::{AppHandle, Emitter, State};

use crate::connection::{data_now, ensure_client, err, App};

/// What the page types, and whether to say an image was too large.
#[derive(serde::Serialize)]
pub(crate) struct Typed {
    /// The words, one space apart, with one space after the last.
    text: String,
    /// The text for `showError`, when an image kept its own path.
    warning: Option<&'static str>,
    /// The first library image typed: the pane's insert signals show it.
    image: Option<String>,
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:#}"))
}

/// Every header button and open library redraws on this.
pub(crate) const IMAGES_CHANGED: &str = "skiff:images-changed";

/// The page's upload line for one drop or paste to a remote Host. Local to
/// this app: it is counted from the Host's replies.
const UPLOAD: &str = "skiff:upload";

/// How far one upload is. `id` is the page's name for the drop.
#[derive(Clone, serde::Serialize)]
struct Upload<'a> {
    id: &'a str,
    /// Files the Host took, of `count`.
    done: usize,
    count: usize,
    /// Bytes the Host took, of `size`.
    sent: u64,
    size: u64,
}

/// Where `to_host` reports: the app and the page's id for the drop.
type Report<'a> = Option<(&'a AppHandle, &'a str)>;

/// A picture selected for a project on a remote Host. The Host owns the
/// resulting cache path; the desktop never writes that Host's projects.toml.
#[tauri::command]
pub(crate) async fn project_image_upload(app: State<'_, App>, path: PathBuf, host: Option<String>) -> Result<String, String> {
    if !app.remote() {
        return Ok(path.to_string_lossy().into_owned());
    }
    let (name, bytes) = data::drop_file(&path, host.as_deref()).await?;
    let c = data_now(&app).await?;
    data::drop_send(&c, &name, bytes, host.as_deref()).await
}

/// Records the images for `who`, then builds the text. A failed record
/// costs only the library line: the text is typed all the same.
async fn finish(handle: &AppHandle, who: Who, t: cache::Typed) -> Result<Typed, String> {
    let image = t.images.first().map(|i| i.path.to_string_lossy().into_owned());
    if !t.images.is_empty() {
        let images = t.images;
        let done = blocking(move || {
            let dir = cache_dir()?;
            for i in &images {
                cache::record(&dir, &i.path, i.name.as_deref(), Some(&who))?;
            }
            Ok(())
        })
        .await;
        if let Err(e) = done {
            eprintln!("skiff: images: {e}");
        }
        let _ = handle.emit(IMAGES_CHANGED, ());
    }
    let text = t.words.iter().map(|w| format!("{w} ")).collect();
    Ok(Typed { text, warning: t.too_large.then_some(cache::TOO_LARGE), image })
}

/// The cache folder, made when an image needs it.
fn cache_dir() -> anyhow::Result<PathBuf> {
    let cfg = skiff_core::config::load().unwrap_or_default();
    cache::ensure(&cfg)
}

/// Files dropped on a session's pane, in drop order. `host` names a remote
/// Host for its messages.
#[tauri::command]
pub(crate) async fn drop_files(handle: AppHandle, app: State<'_, App>, who: Who, paths: Vec<PathBuf>, host: Option<String>, upload: Option<String>) -> Result<Typed, String> {
    let t = if app.remote() {
        let mut files = Vec::new();
        for p in &paths {
            files.push(data::drop_file(p, host.as_deref()).await?);
        }
        let names = paths.iter().map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
        to_host(&app, files, names, host.as_deref(), upload.as_deref().map(|id| (&handle, id))).await?
    } else {
        blocking(move || cache::files(&cache_dir()?, &paths)).await?
    };
    finish(&handle, who, t).await
}

/// Sends each file to the remote Host and types its Host path. An image
/// is kept here too, for Images. `names`: each file's own name for Images,
/// `None` for a paste. `report`: the page's upload line, told after each
/// frame the Host takes.
async fn to_host(app: &App, files: Vec<(String, Vec<u8>)>, names: Vec<Option<String>>, host: Option<&str>, report: Report<'_>) -> Result<cache::Typed, String> {
    let mut at = Upload { id: report.map_or("", |r| r.1), done: 0, count: files.len(), sent: 0, size: files.iter().map(|f| f.1.len() as u64).sum() };
    let tell = |at: &Upload| if let Some((handle, _)) = report { let _ = handle.emit(UPLOAD, at.clone()); };
    tell(&at);
    let c = data_now(app).await?;
    let mut out = cache::Typed::default();
    for ((name, bytes), own) in files.into_iter().zip(names) {
        let ext = Path::new(&name).extension().and_then(|e| e.to_str()).map(str::to_string);
        let local = cache::image_ext(&bytes, ext.as_deref()).map(|ext| (ext, bytes.clone()));
        let last = bytes.len() as u64 + at.sent;
        let path = data::drop_send_progress(&c, &name, bytes, host, |n| {
            at.sent += n as u64;
            if at.sent < last { tell(&at); }
        }).await?;
        at.done += 1;
        tell(&at);
        if let Some((ext, bytes)) = local {
            let stored = blocking(move || cache::store(&cache_dir()?, &bytes, ext)).await?;
            out.images.push(cache::Stored { path: stored, name: own });
        }
        out.words.push(skiff_core::shell::quote_word(&path));
    }
    Ok(out)
}

/// What Skiff's Paste found on the clipboard besides text.
enum Clip {
    Files(Vec<PathBuf>),
    Png(Vec<u8>),
    Nothing,
}

/// Skiff's Paste with no text on the clipboard: a file list does what a
/// drop does; an image is saved as PNG in the cache. `None`: neither.
#[tauri::command]
pub(crate) async fn clipboard_image(handle: AppHandle, app: State<'_, App>, who: Who, host: Option<String>, upload: Option<String>) -> Result<Option<Typed>, String> {
    let report = upload.as_deref().map(|id| (&handle, id));
    let t = match read_clip(&handle).await? {
        Clip::Nothing => return Ok(None),
        Clip::Files(paths) if app.remote() => {
            let mut files = Vec::new();
            for p in &paths {
                files.push(data::drop_file(p, host.as_deref()).await?);
            }
            let names = paths.iter().map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
            to_host(&app, files, names, host.as_deref(), report).await?
        }
        Clip::Png(bytes) if app.remote() => to_host(&app, vec![("clipboard.png".into(), bytes)], vec![None], host.as_deref(), report).await?,
        Clip::Files(paths) => blocking(move || cache::files(&cache_dir()?, &paths)).await?,
        Clip::Png(bytes) => {
            blocking(move || {
                let path = cache::store(&cache_dir()?, &bytes, "png")?;
                Ok(cache::Typed {
                    words: vec![skiff_core::shell::quote_word(&path.to_string_lossy())],
                    images: vec![cache::Stored { path, name: None }],
                    too_large: false,
                })
            })
            .await?
        }
    };
    finish(&handle, who, t).await.map(Some)
}

/// GTK on the main thread, for the reason `clipboard.rs` gives.
#[cfg(target_os = "linux")]
async fn read_clip(handle: &AppHandle) -> Result<Clip, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .run_on_main_thread(move || {
            let cb = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
            let files: Vec<PathBuf> = cb
                .wait_for_uris()
                .iter()
                .filter_map(|u| gtk::glib::filename_from_uri(u).ok().map(|(p, _)| p))
                .collect();
            let clip = if !files.is_empty() {
                Clip::Files(files)
            } else {
                match cb.wait_for_image().map(|p| p.save_to_bufferv("png", &[])) {
                    Some(Ok(png)) => Clip::Png(png),
                    _ => Clip::Nothing,
                }
            };
            let _ = tx.send(clip);
        })
        .map_err(|e| e.to_string())?;
    rx.await.map_err(|_| "the clipboard did not answer".to_string())
}

/// The clipboard plugin's image, encoded as PNG.
#[cfg(not(target_os = "linux"))]
async fn read_clip(handle: &AppHandle) -> Result<Clip, String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let Ok(img) = handle.clipboard().read_image() else { return Ok(Clip::Nothing) };
    let (w, h, rgba) = (img.width(), img.height(), img.rgba().to_vec());
    blocking(move || {
        let mut png = Vec::new();
        let mut enc = png::Encoder::new(&mut png, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&rgba)?;
        Ok(Clip::Png(png))
    })
    .await
}

/// The selected Host's Images library, newest first. A remote list is Host
/// metadata only; this desktop never opens its paths.
#[tauri::command]
pub(crate) async fn images(app: State<'_, App>) -> Result<Vec<cache::Image>, String> {
    if app.remote() {
        return crate::connection::data_read(&app, |c| async move { skiff_client::host::data::image_list(&c).await })
            .await
            .map(|images| images.into_iter().map(|image| cache::Image {
                path: image.path,
                entry: cache::Entry { file: image.file, name: image.name, session: image.session, title: image.title, project: image.project, at: image.at, pinned: image.pinned },
                size: image.size,
            }).collect());
    }
    blocking(|| {
        let cfg = skiff_core::config::load().unwrap_or_default();
        Ok(cache::list(&cache::ensure(&cfg)?))
    })
    .await
}

/// Insert: the library images go to the top with `who` as their session, and
/// the page types their paths as a drop of several files does. The first
/// path ends up newest. On a remote Host's pane each file goes to the Host
/// first, and the page types the Host paths.
#[tauri::command]
pub(crate) async fn image_insert(handle: AppHandle, app: State<'_, App>, paths: Vec<PathBuf>, who: Who, host: Option<String>) -> Result<Typed, String> {
    let mut text = String::new();
    if app.remote() {
        let c = data_now(&app).await?;
        for p in &paths {
            let image = data::image_read(&c, &p.to_string_lossy()).await.map_err(data::message)?;
            let name = p.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
            text += &format!("{} ", skiff_core::shell::quote_word(&data::drop_send(&c, &name, image.bytes, host.as_deref()).await?));
        }
    } else {
        text = paths.iter().map(|p| format!("{} ", skiff_core::shell::quote_word(&p.to_string_lossy()))).collect();
    }
    let image = paths.first().map(|p| p.to_string_lossy().into_owned());
    if !app.remote() {
        blocking(move || {
            let dir = cache_dir()?;
            paths.iter().rev().try_for_each(|p| cache::record(&dir, p, None, Some(&who)))
        })
        .await?;
    }
    let _ = handle.emit(IMAGES_CHANGED, ());
    Ok(Typed { text, warning: None, image })
}

#[tauri::command]
pub(crate) async fn image_pin(handle: AppHandle, app: State<'_, App>, paths: Vec<PathBuf>, on: bool) -> Result<(), String> {
    if app.remote() {
        let paths = paths.into_iter().map(|path| path.to_string_lossy().into_owned()).collect();
        crate::connection::data_read(&app, |c| async move { data::image_pin(&c, paths, on).await }).await?;
        let _ = handle.emit(IMAGES_CHANGED, ());
        return Ok(());
    }
    blocking(move || {
        let dir = cache_dir()?;
        paths.iter().try_for_each(|p| cache::pin(&dir, p, on))
    })
    .await?;
    let _ = handle.emit(IMAGES_CHANGED, ());
    Ok(())
}

/// Deletes the files. The page asks first only for two or more.
#[tauri::command]
pub(crate) async fn image_delete(handle: AppHandle, app: State<'_, App>, paths: Vec<PathBuf>) -> Result<(), String> {
    if app.remote() {
        let paths = paths.into_iter().map(|path| path.to_string_lossy().into_owned()).collect();
        crate::connection::data_read(&app, |c| async move { data::image_delete(&c, paths).await }).await?;
        let _ = handle.emit(IMAGES_CHANGED, ());
        return Ok(());
    }
    let done = blocking(move || {
        let dir = cache_dir()?;
        paths.iter().try_for_each(|p| cache::delete(&dir, p))
    })
    .await;
    // Some files can be gone before a later one fails.
    let _ = handle.emit(IMAGES_CHANGED, ());
    done
}

/// A library image's bytes, for its thumbnail. Only files in the cache. A
/// remote Host scales it to `size` pixels on its long side first, so the
/// link carries kilobytes, not the full image.
#[tauri::command]
pub(crate) async fn image_thumb(app: State<'_, App>, path: PathBuf, size: u32) -> Result<tauri::ipc::Response, String> {
    if app.remote() {
        let image = crate::connection::data_read(&app, |c| async move { data::image_thumb(&c, &path.to_string_lossy(), size).await }).await?;
        return Ok(tauri::ipc::Response::new(image.bytes));
    }
    let cfg = skiff_core::config::load().unwrap_or_default();
    let dir = cache::dir(&cfg).map_err(|e| format!("{e:#}"))?.canonicalize().map_err(|e| e.to_string())?;
    let file = path.canonicalize().map_err(|e| format!("read {}: {e}", path.display()))?;
    if file.parent() != Some(dir.as_path()) {
        return Err(format!("{} is not in the Images folder", path.display()));
    }
    let bytes = tokio::fs::read(&file).await.map_err(|e| format!("read {}: {e}", file.display()))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Opens a library image with `[open] image`, else the OS default app. It
/// never runs in the peek.
#[tauri::command]
pub(crate) async fn open_image(path: PathBuf) -> Result<(), String> {
    blocking(move || {
        if !path.is_file() {
            anyhow::bail!("The image is gone: {}", path.display());
        }
        let open = skiff_core::config::load().map(|c| c.open).unwrap_or_default();
        match open.image.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(cmd) => skiff_core::open::launch(cmd, &path),
            None => skiff_core::open::open_default(&path),
        }
    })
    .await
}

/// The image cache's read flag to type after `agent`'s resume line, with a
/// space in front. Empty for an agent that gets none.
#[tauri::command]
pub(crate) async fn agent_cache_flags(app: State<'_, App>, agent: String) -> Result<String, String> {
    if app.remote() {
        return crate::connection::data_read(&app, |c| async move {
            let (agents, _) = data::agent_config(&c).await?;
            Ok(agents.into_iter().find(|a| a.id == agent).map(|a| a.cache_flags).unwrap_or_default())
        })
        .await;
    }
    let (c, _) = ensure_client(&app).await?;
    let agents = c.list_agents().await.map_err(err)?;
    let cfg = skiff_core::config::load().unwrap_or_default();
    Ok(agents.iter().find(|a| a.id == agent).map(|a| cache::flags_typed(&cfg, a)).unwrap_or_default())
}
