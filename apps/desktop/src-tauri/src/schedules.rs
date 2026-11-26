//! Schedules (#262) for the Ledger's Schedules tab and Settings › Schedules,
//! from the selected Host (`schedules@2`). The app writes nothing itself:
//! each change runs `skiff schedule` on the Host, its ledger's only writer.

use skiff_client::host::data;
use skiff_host_api::{HostCronPreview, HostSchedule};
use crate::routes::Routed;

use crate::connection::{data_read, data_write};

/// Every schedule by name, each with its newest runs.
#[tauri::command]
pub async fn schedule_list(app: Routed) -> Result<Vec<HostSchedule>, String> {
    data_read(&app, |c| async move { data::schedule_list(&c).await }).await
}

/// The next runs of a cron line in the Host's local time, or its error line.
#[tauri::command]
pub async fn cron_preview(app: Routed, cron: String) -> Result<HostCronPreview, String> {
    data_read(&app, |c| async move { data::cron_preview(&c, &cron).await }).await
}

/// `skiff schedule ARGS` on the Host: add, edit, rm, pause, resume, run-now.
#[tauri::command]
pub async fn schedule_call(app: Routed, args: Vec<String>) -> Result<(), String> {
    data_write(&app, |c| async move { data::schedule_call(&c, args).await }).await
}
