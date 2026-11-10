//! The scheduler's clock (#262). Each half minute skiffd runs `skiff schedule
//! tick` from beside its own binary: the CLI reads the ledger's schedules,
//! starts the due runs and sends the pending agent steps again. The CLI stays
//! the only writer of the ledger; skiffd only keeps the time, so schedules
//! run while the app is closed.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

/// How often the CLI looks for due schedules.
const EVERY: Duration = Duration::from_secs(30);
/// A tick that takes longer is stopped. The next one starts on time.
const LIMIT: Duration = Duration::from_secs(120);

/// The `skiff` CLI shipped beside this daemon. None: no CLI, no schedules.
fn cli() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let cli = exe.parent()?.join("skiff");
    cli.is_file().then_some(cli)
}

/// Starts the clock. `SKIFF_SCHEDULES=0` turns it off.
pub fn spawn() {
    if std::env::var("SKIFF_SCHEDULES").as_deref() == Ok("0") {
        tracing::info!("schedules: off (SKIFF_SCHEDULES=0)");
        return;
    }
    tokio::spawn(async {
        let mut tick = tokio::time::interval(EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            // Looked up each time: a reload or an update may bring the CLI.
            let Some(cli) = cli() else { continue };
            let mut cmd = tokio::process::Command::new(&cli);
            cmd.arg("--socket")
                .arg(crate::paths::socket())
                .args(["schedule", "tick"])
                .env_remove("SKIFF_SESSION")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            let out = match cmd.spawn() {
                Ok(child) => tokio::time::timeout(LIMIT, child.wait_with_output()).await,
                Err(e) => {
                    tracing::warn!("schedules: cannot start {}: {e}", cli.display());
                    continue;
                }
            };
            match out {
                Ok(Ok(o)) if !o.status.success() => {
                    tracing::warn!("schedules: tick failed: {}", String::from_utf8_lossy(&o.stderr).trim());
                }
                Ok(Err(e)) => tracing::warn!("schedules: tick did not end: {e}"),
                Err(_) => tracing::warn!("schedules: tick stopped after {} s", LIMIT.as_secs()),
                _ => {}
            }
        }
    });
}
