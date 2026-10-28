//! daemon reload and stop.

use std::{collections::HashMap, path::{Path, PathBuf}, time::{Duration, Instant}};

use anyhow::{bail, Context, Result};
use skiff_client::{update::{self, Handover}, Client, DaemonInfo};
use skiff_core::session::SessionState;

/// How long the new image may take to adopt the sessions and answer.
const RECONNECT: Duration = Duration::from_secs(15);
/// How long a stopping daemon may take to save and let go of its socket.
const STOP: Duration = Duration::from_secs(10);

/// How a shell restarts skiffd, for [`update::unfinished`].
const RESTART: &str = "Restarting skiffd (`skiff daemon stop`, then open Skiff)";

/// The skiffd installed with this skiff: `SKIFF_DAEMON`, else the one
/// beside this executable, else the first on `PATH`. The app picks its own
/// the same way.
fn installed() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SKIFF_DAEMON") {
        return Some(PathBuf::from(p));
    }
    let beside = std::env::current_exe().ok()?.with_file_name("skiffd");
    if beside.is_file() {
        return Some(beside);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join("skiffd")).find(|p| p.is_file())
}

/// What to do about a daemon that is not the skiffd this skiff was built
/// for: a reload that keeps every session, else a restart and why. The app's
/// badge says the same, from the same [`update`] words.
pub async fn advice(c: &Client, info: &DaemonInfo) -> String {
    let fallback = format!("skiffd is {}, and this skiff was built for skiffd {}. Update Skiff", info.version, skiff_core::VERSION);
    let Some(binary) = installed() else { return fallback };
    let target = match update::reload_info(&binary).await {
        Ok(t) => t,
        Err(e) => return format!("{fallback}: {e}"),
    };
    match update::handover(info, &target) {
        Handover::Ready => update::ready(&info.version, &target.version, &format!("`skiff daemon reload --binary {}`", binary.display())),
        Handover::Blocked(why) => {
            let live = c.list_sessions().await.ok().map(|l| l.iter().filter(|s| s.state != SessionState::Done).count());
            update::unfinished(&info.version, &target.version, &why, live, RESTART)
        }
        Handover::NotNewer => fallback,
    }
}

/// Reloads the daemon onto `binary`, then proves the sessions survived:
/// the same daemon pid, and each live session with the same id and pid.
pub async fn reload(socket: &Path, binary: Option<PathBuf>) -> Result<()> {
    let binary = match binary {
        Some(b) => Some(std::path::absolute(&b).with_context(|| format!("resolve {}", b.display()))?),
        None => None,
    };
    let c = crate::connect(socket).await?;
    let before = c.daemon_info().await?;
    let (Some(_), Some(pid)) = (before.reload_state, before.pid) else {
        bail!("skiffd {} cannot reload; restarting it ends every session", before.version);
    };
    // Say why before the daemon refuses, with the advice `skiff status` gives.
    if let Some(b) = &binary {
        if let Ok(target) = update::reload_info(b).await {
            if let Handover::Blocked(why) = update::handover(&before, &target) {
                let live = c.list_sessions().await.ok().map(|l| l.iter().filter(|s| s.state != SessionState::Done).count());
                bail!("{}", update::unfinished(&before.version, &target.version, &why, live, RESTART));
            }
        }
    }
    let live: HashMap<String, Option<u32>> = c
        .list_sessions()
        .await?
        .into_iter()
        .filter(|s| s.state != SessionState::Done)
        .map(|s| (s.id, s.pid))
        .collect();
    c.reload(binary).await.map_err(|e| match e.downcast_ref::<skiff_client::Refused>() {
        Some(_) => e.context(format!("skiffd {} did not reload. Its sessions keep running", before.version)),
        None => e,
    })?;
    drop(c);

    let start = Instant::now();
    let (c, after) = loop {
        if let Ok(c) = Client::connect(socket).await {
            if let Ok(info) = c.daemon_info().await {
                break (c, info);
            }
        }
        if start.elapsed() > RECONNECT {
            bail!("skiffd did not answer after the reload; check its log");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    if after.pid != Some(pid) {
        bail!(
            "skiffd restarted instead of reloading (pid {pid}, now {}); its sessions did not survive",
            after.pid.map_or("unknown".into(), |p| p.to_string())
        );
    }
    let now: HashMap<String, Option<u32>> = c.list_sessions().await?.into_iter().map(|s| (s.id, s.pid)).collect();
    let lost: Vec<&String> = live.iter().filter(|(id, p)| now.get(*id) != Some(p)).map(|(id, _)| id).collect();
    println!("skiffd {} -> {}, pid {pid}, {} of {} live sessions kept", before.version, after.version, live.len() - lost.len(), live.len());
    if !lost.is_empty() {
        let ids: Vec<&str> = lost.iter().map(|s| s.as_str()).collect();
        bail!("lost in the reload: {}", ids.join(", "));
    }
    Ok(())
}

/// Asks the daemon to stop, then waits until its socket no longer answers.
pub async fn stop(socket: &Path) -> Result<()> {
    let c = crate::connect(socket).await?;
    let info = c.daemon_info().await?;
    c.shutdown().await.with_context(|| format!("skiffd {} did not take the request", info.version))?;
    drop(c);
    let start = Instant::now();
    while Client::connect(socket).await.is_ok() {
        if start.elapsed() > STOP {
            bail!("skiffd still answers at {}", socket.display());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let pid = info.pid.map_or(String::new(), |p| format!(", pid {p}"));
    println!("skiffd {} stopped{pid}", info.version);
    Ok(())
}
