//! Files dropped on a pane, and Skiff's Paste of a file list or an image.
//! Images go to the cache ([`skiff_client::cache`]) and into the session's
//! image strip. The page types the words with `term.paste()`, so an agent
//! that reads a pasted path attaches the image. Skiff never presses Enter.
//!
//! On a remote Host's pane each file goes to that Host (`drops@1`), and the
//! pane gets the Host path. An image also stays in this computer's cache
//! under the same name, so the strip shows and opens it here.

use std::path::{Path, PathBuf};

use skiff_client::{cache, host::data};
use tauri::{AppHandle, State};

use crate::connection::{data_err, data_now, ensure_client, err, App};

/// The largest file a drop sends to a remote Host: one frame holds it.
const REMOTE_MAX: u64 = 5 * 1024 * 1024;

/// What the page types, and whether to say an image was too large.
#[derive(serde::Serialize)]
pub(crate) struct Typed {
    /// The words, one space apart, with one space after the last.
    text: String,
    /// The text for `showError`, when an image kept its own path.
    warning: Option<&'static str>,
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:#}"))
}

/// Adds the images to the session's strip, then builds the text. An older
/// skiffd has no strip: the text is typed all the same.
async fn finish(app: &App, session: &str, t: cache::Typed) -> Result<Typed, String> {
    if !t.images.is_empty() {
        let (c, _) = ensure_client(app).await?;
        if let Err(e) = c.add_session_images(session, t.images).await {
            eprintln!("skiff: image strip: {e:#}");
        }
    }
    let text = t.words.iter().map(|w| format!("{w} ")).collect();
    Ok(Typed { text, warning: t.too_large.then_some(cache::TOO_LARGE) })
}

/// The cache folder, made when an image needs it.
fn cache_dir() -> anyhow::Result<PathBuf> {
    let cfg = skiff_core::config::load().unwrap_or_default();
    cache::ensure(&cfg)
}

/// Files dropped on a session's pane, in drop order. `host` names a remote
/// Host for its messages.
#[tauri::command]
pub(crate) async fn drop_files(app: State<'_, App>, session: String, paths: Vec<PathBuf>, host: Option<String>) -> Result<Typed, String> {
    let t = if app.remote() {
        let mut files = Vec::new();
        for p in paths {
            let name = file_name(&p);
            files.push((name, read_for_host(p, host.as_deref()).await?));
        }
        to_host(&app, files, host.as_deref()).await?
    } else {
        blocking(move || cache::files(&cache_dir()?, &paths)).await?
    };
    finish(&app, &session, t).await
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A file to send to a remote Host: no folder, and small enough for one frame.
async fn read_for_host(p: PathBuf, host: Option<&str>) -> Result<Vec<u8>, String> {
    let on = host.unwrap_or("the Host");
    let m = tokio::fs::metadata(&p).await.map_err(|e| format!("{}: {e}", p.display()))?;
    if m.is_dir() {
        return Err(format!("Drop files, not folders, on a pane on {on}."));
    }
    if m.len() > REMOTE_MAX {
        return Err(format!("{} is larger than 5 MB. Skiff cannot send it to {on} yet.", p.display()));
    }
    tokio::fs::read(&p).await.map_err(|e| format!("{}: {e}", p.display()))
}

/// Sends each file to the remote Host and types its Host path. An image
/// is kept here too, under the name the Host gives it, for the strip.
async fn to_host(app: &App, files: Vec<(String, Vec<u8>)>, host: Option<&str>) -> Result<cache::Typed, String> {
    let c = data_now(app).await?;
    let mut out = cache::Typed::default();
    for (name, bytes) in files {
        let ext = Path::new(&name).extension().and_then(|e| e.to_str()).map(str::to_string);
        let image = cache::image_ext(&bytes, ext.as_deref());
        let local = image.map(|ext| (ext, bytes.clone()));
        let path = data::drop_put(&c, &name, bytes).await.map_err(|e| match e {
            skiff_host_api::Error::UnsupportedCapability { .. } => format!(
                "{} cannot take dropped files. Its Skiff is older than this app: use Update Skiff… on that Host in Settings › Hosts.",
                host.unwrap_or("This Host")
            ),
            e => data_err(e),
        })?;
        if let Some((ext, bytes)) = local {
            blocking(move || cache::store(&cache_dir()?, &bytes, ext)).await?;
            out.images.push(path.clone());
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
pub(crate) async fn clipboard_image(handle: AppHandle, app: State<'_, App>, session: String, host: Option<String>) -> Result<Option<Typed>, String> {
    let t = match read_clip(&handle).await? {
        Clip::Nothing => return Ok(None),
        Clip::Files(paths) if app.remote() => {
            let mut files = Vec::new();
            for p in paths {
                let name = file_name(&p);
                files.push((name, read_for_host(p, host.as_deref()).await?));
            }
            to_host(&app, files, host.as_deref()).await?
        }
        Clip::Png(bytes) if app.remote() => to_host(&app, vec![("clipboard.png".into(), bytes)], host.as_deref()).await?,
        Clip::Files(paths) => blocking(move || cache::files(&cache_dir()?, &paths)).await?,
        Clip::Png(bytes) => {
            blocking(move || {
                let path = cache::store(&cache_dir()?, &bytes, "png")?;
                let path = path.to_string_lossy().into_owned();
                Ok(cache::Typed {
                    words: vec![skiff_core::shell::quote_word(&path)],
                    images: vec![path],
                    too_large: false,
                })
            })
            .await?
        }
    };
    finish(&app, &session, t).await.map(Some)
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

/// Empties the session's image strip. The files stay.
#[tauri::command]
pub(crate) async fn clear_session_images(app: State<'_, App>, session: String) -> Result<(), String> {
    let (c, _) = ensure_client(&app).await?;
    c.clear_session_images(&session).await.map(|_| ()).map_err(err)
}

/// A strip image's file on this computer: the path itself, or for a remote
/// Host's path, the copy in this computer's cache under the same name.
fn here(app: &App, path: PathBuf) -> PathBuf {
    if !app.remote() {
        return path;
    }
    let cfg = skiff_core::config::load().unwrap_or_default();
    cache::dir(&cfg).join(path.file_name().unwrap_or_default())
}

/// A strip image's bytes, for its thumbnail.
#[tauri::command]
pub(crate) async fn strip_image(app: State<'_, App>, path: PathBuf) -> Result<tauri::ipc::Response, String> {
    let file = here(&app, path);
    let bytes = tokio::fs::read(&file).await.map_err(|e| format!("read {}: {e}", file.display()))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Opens an image from the strip with `[open] image`, else the OS default
/// app. It never runs in the peek.
#[tauri::command]
pub(crate) async fn open_image(app: State<'_, App>, path: PathBuf) -> Result<(), String> {
    let path = here(&app, path);
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
    // A remote Host's cache path is its own: this computer's means nothing there.
    if app.remote() {
        return Ok(String::new());
    }
    let (c, _) = ensure_client(&app).await?;
    let agents = c.list_agents().await.map_err(err)?;
    let cfg = skiff_core::config::load().unwrap_or_default();
    Ok(agents.iter().find(|a| a.id == agent).map(|a| cache::flags_typed(&cfg, a)).unwrap_or_default())
}
