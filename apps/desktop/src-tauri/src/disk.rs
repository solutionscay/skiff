//! Settings › Disk use. The counts and flushes live in `skiff_client::disk`;
//! this reads the folder, the ledger and the daemon's sessions for them.

use std::path::PathBuf;

use serde::Serialize;
use skiff_client::{
    disk::{self, Removed},
    ledger, skipper,
};
use skiff_core::{config, session::SessionInfo};
use tauri::State;

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

/// The sessions on the open connection. `None` when the daemon does not
/// answer. Never starts a daemon.
async fn sessions(app: &App) -> Option<Vec<SessionInfo>> {
    let c = app.client.lock().await.clone().filter(|c| !c.is_closed())?;
    c.list_sessions().await.ok()
}

/// The folder and its cache, or the `/tmp` cache alone when no agent folder is usable.
fn count(sessions: Option<Vec<SessionInfo>>) -> anyhow::Result<Usage> {
    let cfg = config::load()?;
    let cache = skipper::cache_dir(&cfg);
    let cache_days = cfg.skiff_agent.cache_days();
    let usage = match skipper::folder(&cfg) {
        Ok(folder) => {
            let file = ledger::path()?;
            disk::usage(&folder, &cache, Some(&file), sessions.as_deref(), &skiff_core::state::socket())
        }
        Err(_) => disk::usage(&cache, &cache, None, None, &skiff_core::state::socket()),
    };
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

#[tauri::command]
pub async fn flush_cache() -> Result<Removed, String> {
    blocking(|| Ok(disk::flush_cache(&skipper::cache_dir(&config::load()?)))).await
}

/// Counts again, then removes what is on both lists. Refused when the daemon
/// does not answer: the closed work needs the session list.
#[tauri::command]
pub async fn flush_done(app: State<'_, App>, expected: Vec<String>) -> Result<Removed, String> {
    let Some(s) = sessions(&app).await else {
        return Err("Skiff cannot reach skiffd. Flush done work needs the session list.".into());
    };
    blocking(move || {
        let fresh = count(Some(s))?.usage;
        if let Some(e) = fresh.ledger_error {
            anyhow::bail!("{e}");
        }
        Ok(disk::flush_done(&expected, &fresh))
    })
    .await
}

/// Runs at app start on its own thread: removes cache files older than
/// `[skiff_agent] cache_days`. One log line, no UI.
pub fn auto_flush() {
    let Ok(cfg) = config::load() else { return };
    let days = cfg.skiff_agent.cache_days();
    if days == 0 {
        return;
    }
    let cache: PathBuf = skipper::cache_dir(&cfg);
    let out = disk::flush_old(&cache, days);
    eprintln!("auto flush: removed {} files, {} from {}", out.files, skiff_client::agent_folder::size(out.bytes), cache.display());
}
