//! `schedules@1` (#262): the Host's schedules with their newest runs, read
//! from its own `ledger.db`, a cron check, and changes through `skiff
//! schedule` on the Host, the only writer of its ledger (M6).

use std::path::PathBuf;

use skiff_client::schedule::{self as sched, cron::Cron, When};
use skiff_core::session::SessionInfo;
use skiff_host_api::{Error, HostCronPreview, HostSchedule, HostScheduleRun};

use super::failed;
use super::ledger::{host_row, ledger_unread};

/// The runs each schedule carries for the series view.
const RUNS: i64 = 20;

/// The subcommands the app may call.
const CALLS: [&str; 6] = ["add", "edit", "rm", "pause", "resume", "run-now"];

pub(super) fn schedule_list(all: Option<&[SessionInfo]>) -> Result<Vec<HostSchedule>, Error> {
    let file = skiff_client::ledger::path().map_err(failed)?;
    if !file.exists() {
        return Ok(Vec::new());
    }
    let db = skiff_client::ledger::open_read(&file).map_err(ledger_unread)?;
    let list = sched::summaries(&db, all, RUNS).map_err(failed)?;
    Ok(list
        .into_iter()
        .map(|s| {
            let runs = s
                .runs
                .into_iter()
                .map(|r| HostScheduleRun {
                    row: host_row(&file, r.row),
                    status: r.status.into(),
                    at: r.at,
                    kids: r.kids.into_iter().map(|k| host_row(&file, k)).collect(),
                })
                .collect();
            let x = s.schedule;
            HostSchedule {
                uid: x.uid,
                name: x.name,
                cron: x.cron,
                when: s.when,
                custom: s.custom,
                command: x.command,
                timeout: x.timeout,
                gate: x.gate,
                target: x.target,
                project: x.project,
                task_type: x.task_type,
                pane: x.pane,
                owner: x.owner,
                text: x.text,
                paused_at: x.paused_at,
                next: s.next,
                last: s.last.map(Into::into),
                open: s.open,
                runs,
            }
        })
        .collect())
}

pub(super) fn cron_preview(cron: &str) -> HostCronPreview {
    let w = When::from_cron(cron);
    match Cron::parse(cron) {
        Ok(c) => {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default();
            HostCronPreview { next: c.next_runs(now, 3).into_iter().map(sched::cron::short_time).collect(), error: None, when: w.words(), custom: w.is_custom() }
        }
        Err(e) => HostCronPreview { next: Vec::new(), error: Some(e), when: w.words(), custom: true },
    }
}

/// The `skiff` CLI beside this service, else the one on PATH.
fn cli() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("skiff")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("skiff"))
}

pub(super) fn schedule_call(args: &[String]) -> Result<(), Error> {
    if !args.first().is_some_and(|a| CALLS.contains(&a.as_str())) {
        return Err(Error::Invalid { message: format!("schedule calls are {}", CALLS.join(", ")) });
    }
    let out = std::process::Command::new(cli())
        .arg("--socket")
        .arg(skiff_core::state::socket())
        .arg("schedule")
        .args(args)
        .arg("--person")
        .env_remove("SKIFF_SESSION")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| failed(format!("cannot run skiff schedule: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("skiff schedule failed");
    Err(Error::Invalid { message: line.trim().trim_start_matches("skiff: ").to_string() })
}
