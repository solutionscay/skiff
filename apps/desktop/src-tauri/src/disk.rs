//! Settings › Disk use. The counts and flushes live in `skiff_client::disk`;
//! this reads the folder, the ledger and the daemon's sessions for them.
//! This computer only, also while the window shows a remote Host: a Host's
//! own disk use needs a Host op (v2 follow-up).

use serde::Serialize;
use skiff_client::{
    disk::{self, Removed},
    ledger, skipper,
};
use skiff_core::{config, session::SessionInfo};
use tauri::{AppHandle, Emitter, State};

use crate::connection::App;

/// The counts, with the auto flush setting for its row.
#[derive(Serialize)]
pub struct Usage {
    #[serde(flatten)]
    usage: disk::Usage,
    /// The folder with `~` for home.
    shown: String,
    cache_days: u32,
}

/// This computer's sessions, on the open connection. `None` when the daemon
/// does not answer, or while a remote Host is shown: its sessions are not
/// this computer's. Never starts a daemon.
async fn sessions(app: &App) -> Option<Vec<SessionInfo>> {
    if app.remote() {
        return None;
    }
    let c = app.live_session().await?;
    skiff_client::host::local::legacy_inventory(&c).await.ok()
}

/// The folder and its cache. Makes the folder when it is missing.
fn count(sessions: Option<Vec<SessionInfo>>) -> anyhow::Result<Usage> {
    let cfg = config::load()?;
    let folder = skipper::make_folder(&cfg)?;
    let cache_days = cfg.skiff_agent.cache_days();
    let usage = disk::usage(&folder, &folder.join("cache"), &ledger::path()?, sessions.as_deref(), &skiff_core::state::socket());
    let home = skipper::home().ok();
    let shown = match home.as_deref().and_then(|h| usage.folder.strip_prefix(h).ok()) {
        Some(rel) if rel.as_os_str().is_empty() => "~".to_string(),
        Some(rel) => format!("~/{}", rel.display()),
        None => usage.folder.display().to_string(),
    };
    Ok(Usage { usage, shown, cache_days })
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn disk_usage(app: State<'_, App>) -> Result<Usage, String> {
    let s = sessions(&app).await;
    blocking(move || count(s)).await
}

/// Auto flush's age in days, in this computer's projects.toml. `0`: off.
#[tauri::command]
pub async fn set_cache_days(days: u32) -> Result<(), String> {
    blocking(move || config::set_skiff_agent(config::SkiffAgentKey::CacheDays(days))).await
}

#[tauri::command]
pub async fn flush_cache(handle: AppHandle) -> Result<Removed, String> {
    let out = blocking(|| Ok(disk::flush_cache(&skipper::cache_dir(&config::load()?)?))).await;
    let _ = handle.emit(crate::drops::IMAGES_CHANGED, ());
    out
}

/// Counts again, then removes what is on both lists. Refused when the daemon
/// does not answer: the closed work needs the session list.
#[tauri::command]
pub async fn flush_done(handle: AppHandle, app: State<'_, App>, expected: Vec<String>) -> Result<Removed, String> {
    let Some(s) = sessions(&app).await else {
        return Err("Skiff cannot reach skiffd. Flush done work needs the session list.".into());
    };
    let out = blocking(move || {
        let fresh = count(Some(s))?.usage;
        if let Some(e) = fresh.ledger_error {
            anyhow::bail!("{e}");
        }
        Ok(disk::flush_done(&expected, &fresh))
    })
    .await;
    let _ = handle.emit(crate::drops::IMAGES_CHANGED, ());
    out
}

/// Runs at app start on its own thread: removes cache files older than
/// `[skiff_agent] cache_days`. One log line, no UI.
pub fn auto_flush() {
    let Ok(cfg) = config::load() else { return };
    let days = cfg.skiff_agent.cache_days();
    if days == 0 {
        return;
    }
    let Ok(cache) = skipper::cache_dir(&cfg) else { return };
    let out = disk::flush_old(&cache, days);
    eprintln!("auto flush: removed {} files, {} from {}", out.files, skiff_client::agent_folder::size(out.bytes), cache.display());
}
