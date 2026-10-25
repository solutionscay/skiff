//! Files dropped on a pane, and Skiff's Paste of a file list or an image.
//! Images go to the cache ([`skiff_client::cache`]) and into the session's
//! image strip. The page types the words with `term.paste()`, so an agent
//! that reads a pasted path attaches the image. Skiff never presses Enter.

use std::path::PathBuf;

use skiff_client::cache;
use tauri::{AppHandle, State};

use crate::connection::{ensure_client, err, App};

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

/// Files dropped on a session's pane, in drop order.
#[tauri::command]
pub(crate) async fn drop_files(app: State<'_, App>, session: String, paths: Vec<PathBuf>) -> Result<Typed, String> {
    let t = blocking(move || cache::files(&cache_dir()?, &paths)).await?;
    finish(&app, &session, t).await
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
pub(crate) async fn clipboard_image(handle: AppHandle, app: State<'_, App>, session: String) -> Result<Option<Typed>, String> {
    let t = match read_clip(&handle).await? {
        Clip::Nothing => return Ok(None),
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

/// Opens an image from the strip with `[open] image`, else the OS default
/// app. It never runs in the peek.
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
    let (c, _) = ensure_client(&app).await?;
    let agents = c.list_agents().await.map_err(err)?;
    let cfg = skiff_core::config::load().unwrap_or_default();
    Ok(agents.iter().find(|a| a.id == agent).map(|a| cache::flags_typed(&cfg, a)).unwrap_or_default())
}
