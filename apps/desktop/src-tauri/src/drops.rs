//! Files dropped on a pane, Skiff's Paste of a file list or an image, and
//! the Files library. Each file up to the size limit goes to the cache
//! ([`skiff_client::cache`]), which is the library. The page types the words
//! with `term.paste()`, so an agent that reads a pasted path attaches the
//! file. Skiff never presses Enter.
//!
//! On a remote Host's pane each file goes to that Host (`drops@1`), and the
//! pane gets the Host path. That Host's cache records it with its session,
//! so Files lists it there. A `drops@2` Host takes a file up to its own size
//! limit; an older one, up to 50 MB. An image also stays in this computer's
//! cache.

use std::path::{Path, PathBuf};

use skiff_client::{
    cache::{self, Who},
    host::data::{self, DropFile},
};
use skiff_host_api::HostWho;
use tauri::{AppHandle, Emitter, State};

use crate::connection::{data_now, ensure_client, err, App};

/// What the page types, and whether to say a file was too large.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Typed {
    /// The words, one space apart, with one space after the last.
    text: String,
    /// The size limit in MB, when a file over it kept its own path.
    too_large: Option<u32>,
    /// The first library file typed: the pane's insert signals show it.
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
    let c = data_now(&app).await?;
    let f = data::drop_file(&c, &path, host.as_deref())?;
    data::drop_send_file(&c, &f, None, host.as_deref(), |_| {}).await
}

fn host_who(who: &Who) -> HostWho {
    HostWho { session: who.session.clone(), title: who.title.clone(), project: who.project.clone() }
}

/// One file for a remote Host: a local file, read as it is sent, or bytes
/// (a pasted picture) with its name.
enum Item {
    File(DropFile),
    Bytes(String, Vec<u8>),
}

/// Records the files for `who`, then builds the text. A failed record
/// costs only the library line: the text is typed all the same. `remote`:
/// the words are Host paths; only a local copy is recorded here.
async fn finish(handle: &AppHandle, who: Who, t: cache::Typed, remote: bool) -> Result<Typed, String> {
    let image = if remote { None } else { t.stored.first().map(|i| i.path.to_string_lossy().into_owned()) };
    if !t.stored.is_empty() {
        let stored = t.stored;
        let done = blocking(move || {
            let dir = cache_dir()?;
            for i in &stored {
                cache::record(&dir, &i.path, i.name.as_deref(), Some(&who))?;
            }
            Ok(())
        })
        .await;
        if let Err(e) = done {
            eprintln!("skiff: files: {e}");
        }
    }
    // A remote Host's cache recorded its files too.
    let _ = handle.emit(IMAGES_CHANGED, ());
    let text = t.words.iter().map(|w| format!("{w} ")).collect();
    let limit = t.too_large.then(|| skiff_core::config::load().unwrap_or_default().skiff_agent.cache_max_mb());
    Ok(Typed { text, too_large: limit, image: image.or_else(|| remote.then(|| t.words.first().cloned()).flatten()) })
}

/// The cache folder, made when a file needs it.
fn cache_dir() -> anyhow::Result<PathBuf> {
    let cfg = skiff_core::config::load().unwrap_or_default();
    cache::ensure(&cfg)
}

/// The local drop: each file up to `[skiff_agent] cache_max_mb` goes to the cache.
fn local_files(paths: &[PathBuf]) -> anyhow::Result<cache::Typed> {
    let cfg = skiff_core::config::load().unwrap_or_default();
    cache::files(&cache::ensure(&cfg)?, paths, cache::max_bytes(&cfg))
}

/// Files dropped on a session's pane, in drop order. `host` names a remote
/// Host for its messages.
#[tauri::command]
pub(crate) async fn drop_files(handle: AppHandle, app: State<'_, App>, who: Who, paths: Vec<PathBuf>, host: Option<String>, upload: Option<String>) -> Result<Typed, String> {
    let t = if app.remote() {
        to_host(&app, Picked::Paths(&paths), &who, host.as_deref(), upload.as_deref().map(|id| (&handle, id))).await?
    } else {
        blocking(move || local_files(&paths)).await?
    };
    finish(&handle, who, t, app.remote()).await
}

/// What a remote drop or paste sends: local files, or a pasted picture.
enum Picked<'a> {
    Paths(&'a [PathBuf]),
    Png(Vec<u8>),
}

/// Sends each file to the remote Host and types its Host path. The Host
/// records it in its library with `who`. An image is kept here too, with
/// its own name for Files; a paste has none. `report`: the page's upload
/// line, told after each frame the Host takes.
async fn to_host(app: &App, picked: Picked<'_>, who: &Who, host: Option<&str>, report: Report<'_>) -> Result<cache::Typed, String> {
    let c = data_now(app).await?;
    let items: Vec<(Item, Option<String>)> = match picked {
        Picked::Paths(paths) => {
            let mut items = Vec::new();
            for p in paths {
                let f = data::drop_file(&c, p, host)?;
                let own = Some(f.name.clone());
                items.push((Item::File(f), own));
            }
            items
        }
        Picked::Png(bytes) => vec![(Item::Bytes("clipboard.png".into(), bytes), None)],
    };
    let size = items.iter().map(|(i, _)| match i { Item::File(f) => f.size, Item::Bytes(_, b) => b.len() as u64 }).sum();
    let mut at = Upload { id: report.map_or("", |r| r.1), done: 0, count: items.len(), sent: 0, size };
    let tell = |at: &Upload| if let Some((handle, _)) = report { let _ = handle.emit(UPLOAD, at.clone()); };
    tell(&at);
    let mut out = cache::Typed::default();
    for (item, own) in items {
        let local = local_image(&item).await;
        let path = match &item {
            Item::File(f) => {
                let last = f.size + at.sent;
                data::drop_send_file(&c, f, Some(host_who(who)), host, |n| {
                    at.sent += n;
                    if at.sent < last { tell(&at); }
                })
                .await?
            }
            Item::Bytes(name, bytes) => {
                let n = bytes.len() as u64;
                let path = data::drop_send(&c, name, bytes.clone(), Some(host_who(who)), host).await?;
                at.sent += n;
                path
            }
        };
        at.done += 1;
        tell(&at);
        if let Some((ext, bytes)) = local {
            let stored = blocking(move || cache::store(&cache_dir()?, &bytes, ext)).await?;
            out.stored.push(cache::Stored { path: stored, name: own });
        }
        out.words.push(skiff_core::shell::quote_word(&path));
    }
    Ok(out)
}

/// An image's cache extension and bytes, for the copy this computer keeps.
/// Any other file: `None`, and only its first bytes are read.
async fn local_image(item: &Item) -> Option<(&'static str, Vec<u8>)> {
    match item {
        Item::Bytes(name, bytes) => {
            let ext = Path::new(name).extension().and_then(|e| e.to_str());
            cache::image_ext(bytes, ext).map(|e| (e, bytes.clone()))
        }
        Item::File(f) => {
            let path = f.path.clone();
            blocking(move || {
                use std::io::Read;
                let mut head = Vec::new();
                std::fs::File::open(&path)?.take(64 * 1024).read_to_end(&mut head)?;
                let ext = path.extension().and_then(|e| e.to_str());
                Ok(match cache::image_ext(&head, ext) {
                    Some(e) => Some((e, std::fs::read(&path)?)),
                    None => None,
                })
            })
            .await
            .ok()
            .flatten()
        }
    }
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
        Clip::Files(paths) if app.remote() => to_host(&app, Picked::Paths(&paths), &who, host.as_deref(), report).await?,
        Clip::Png(bytes) if app.remote() => to_host(&app, Picked::Png(bytes), &who, host.as_deref(), report).await?,
        Clip::Files(paths) => blocking(move || local_files(&paths)).await?,
        Clip::Png(bytes) => {
            blocking(move || {
                let path = cache::store(&cache_dir()?, &bytes, "png")?;
                Ok(cache::Typed {
                    words: vec![skiff_core::shell::quote_word(&path.to_string_lossy())],
                    stored: vec![cache::Stored { path, name: None }],
                    too_large: false,
                })
            })
            .await?
        }
    };
    finish(&handle, who, t, app.remote()).await.map(Some)
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

/// The selected Host's Files library, newest first. A remote list is Host
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

/// Insert: the library files go to the top with `who` as their session, and
/// the page types their paths as a drop of several files does. The first
/// path ends up newest. On a remote Host the library is that Host's cache,
/// so its paths are typed as they are, and that Host records the Insert
/// (`image_cache@2`). A failed record there costs only the library line.
#[tauri::command]
pub(crate) async fn image_insert(handle: AppHandle, app: State<'_, App>, paths: Vec<PathBuf>, who: Who, host: Option<String>) -> Result<Typed, String> {
    let _ = host;
    let text = paths.iter().map(|p| format!("{} ", skiff_core::shell::quote_word(&p.to_string_lossy()))).collect();
    let image = paths.first().map(|p| p.to_string_lossy().into_owned());
    if app.remote() {
        let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let who = host_who(&who);
        if let Err(e) = crate::connection::data_read(&app, |c| async move { data::image_touch(&c, paths, Some(who)).await }).await {
            eprintln!("skiff: files: {e}");
        }
    } else {
        blocking(move || {
            let dir = cache_dir()?;
            paths.iter().rev().try_for_each(|p| cache::record(&dir, p, None, Some(&who)))
        })
        .await?;
    }
    let _ = handle.emit(IMAGES_CHANGED, ());
    Ok(Typed { text, too_large: None, image })
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

/// A library image's bytes, for its thumbnail. Other files show a type icon. Only files in the cache. A
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
        return Err(format!("{} is not in the Files folder", path.display()));
    }
    let bytes = tokio::fs::read(&file).await.map_err(|e| format!("read {}: {e}", file.display()))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Opens a library image with `[open] image`, else the OS default app. It
/// never runs in the peek. The page opens another local file as a file row
/// opens. On a remote Host the file comes to a local copy first, and opens
/// in an app: the peek on that Host cannot see the copy.
#[tauri::command]
pub(crate) async fn open_image(app: State<'_, App>, path: PathBuf) -> Result<(), String> {
    let path = if app.remote() {
        let c = data_now(&app).await?;
        // Refused before anything is fetched, as Open with refuses.
        let copy = host_copy(c.hello().host.host_id.as_str(), &path).map_err(err)?;
        if let Some(dir) = copy.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        data::image_fetch(&c, &path.to_string_lossy(), &copy).await?;
        crate::copies::seal(&copy).map_err(err)?;
        copy
    } else {
        path
    };
    blocking(move || {
        if !path.is_file() {
            anyhow::bail!("The file is gone: {}", path.display());
        }
        let open = skiff_core::config::load().map(|c| c.open).unwrap_or_default();
        if cache::is_image(&path) {
            if let Some(cmd) = open.image.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
                return skiff_core::open::launch(cmd, &path);
            }
            return skiff_core::open::open_default(&path);
        }
        match skiff_core::open::plan_file(&path, &open) {
            skiff_core::open::Plan::Run(script) if !skiff_core::open::previews_file(&path, &open) => {
                let cwd = path.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
                skiff_core::open::start(&script, &cwd).map(|_| ())
            }
            _ => skiff_core::open::open_default(&path),
        }
    })
    .await
}

/// `hosts/<host>/cache/<hash>/<name>` beside `projects.toml`: this desktop's
/// copy of a remote Host's library file, for Open. Refused for a type that
/// `copies::copy_name` refuses, and for a part that is empty, `.` or `..`.
fn host_copy(host: &str, path: &Path) -> anyhow::Result<PathBuf> {
    let safe = |s: &str| -> anyhow::Result<String> {
        let part: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
        if part.is_empty() || part == "." || part == ".." {
            anyhow::bail!("The Host sent a file path that Skiff does not open: {}", path.display());
        }
        Ok(part)
    };
    crate::copies::copy_name(&path.to_string_lossy())?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let name = safe(safe(&name)?.trim_start_matches('.'))?;
    crate::copies::copy_name(&name)?;
    let sub = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = skiff_core::config::config_path().with_file_name("hosts").join(safe(host)?).join("cache");
    if sub.len() == 16 {
        out = out.join(safe(&sub)?);
    }
    Ok(out.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_copy_refuses_what_runs_and_dot_parts() {
        let lib = "/home/u/.skiff/images/0123456789abcdef";
        let ok = host_copy("h1", &Path::new(lib).join("shot.png")).unwrap();
        assert!(ok.ends_with("hosts/h1/cache/0123456789abcdef/shot.png"), "{}", ok.display());
        for name in ["x.terminal", "x.JAR", "x.command", "..jar", "a b.sh"] {
            let e = host_copy("h1", &Path::new(lib).join(name)).unwrap_err().to_string();
            assert!(e.starts_with("Skiff does not open a copy of a ."), "{name}: {e}");
        }
        for host in [".", "..", ""] {
            assert!(host_copy(host, &Path::new(lib).join("shot.png")).is_err(), "{host:?}");
        }
        assert!(host_copy("h1", Path::new("/lib/....../..")).is_err());
        assert!(host_copy("h1", Path::new("/lib/0123456789abcdef/...")).is_err());
    }
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
