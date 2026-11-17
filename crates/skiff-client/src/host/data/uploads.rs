use skiff_host_api::{
    Error, Request, Response,
};

use crate::host::HostConnection;
use super::message;

/// Puts a dropped or pasted file into the Host's cache. Answers its Host path.
pub async fn drop_put(c: &HostConnection, name: &str, bytes: Vec<u8>) -> Result<String, Error> {
    match c.request(Request::DropPut { name: name.into(), bytes, upload: None, total: None, offset: 0, finish: true }).await? {
        Response::Path { path } => Ok(path),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The largest file a drop sends to a remote Host: one frame holds it.
pub const DROP_MAX: u64 = 50 * 1024 * 1024;
/// The raw payload of one upload frame. JSON/base64 stays below 8 MiB.
const DROP_CHUNK: usize = 4 * 1024 * 1024;

/// A dropped or pasted file for the Host `on`: no folder, and no larger than
/// [`DROP_MAX`]. Answers its name and bytes, or the message to show. It
/// reads at once: the file is small.
pub async fn drop_file(p: &std::path::Path, on: Option<&str>) -> Result<(String, Vec<u8>), String> {
    let on = on.unwrap_or("the Host");
    let m = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if m.is_dir() {
        return Err(format!("Drop files, not folders, on a pane on {on}."));
    }
    if m.len() > DROP_MAX {
        return Err(format!("{} is larger than 50 MB. Skiff cannot send it to {on}.", p.display()));
    }
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((name, bytes))
}

/// [`drop_put`] with the message to show when it fails. `on` names the Host.
pub async fn drop_send(c: &HostConnection, name: &str, bytes: Vec<u8>, on: Option<&str>) -> Result<String, String> {
    drop_send_progress(c, name, bytes, on, |_| {}).await
}

/// [`drop_send`] that calls `sent` with the bytes of each frame the Host
/// took. A file up to [`DROP_CHUNK`] is one frame. Nothing more crosses the
/// link: the count comes from the replies.
pub async fn drop_send_progress(c: &HostConnection, name: &str, bytes: Vec<u8>, on: Option<&str>, mut sent: impl FnMut(usize)) -> Result<String, String> {
    let put = async {
        if bytes.len() <= DROP_CHUNK {
            let n = bytes.len();
            let path = drop_put(c, name, bytes).await?;
            sent(n);
            return Ok(path);
        }
        let upload = uuid::Uuid::new_v4().to_string();
        let total = bytes.len() as u64;
        let mut offset = 0usize;
        while offset < bytes.len() {
            let end = (offset + DROP_CHUNK).min(bytes.len());
            let finish = end == bytes.len();
            let reply = c.request(Request::DropPut {
                name: name.into(), bytes: bytes[offset..end].to_vec(), upload: Some(upload.clone()), total: Some(total), offset: offset as u64, finish,
            }).await?;
            sent(end - offset);
            if finish {
                return match reply { Response::Path { path } => Ok(path), _ => unreachable!("checked by Request::accepts") };
            }
            offset = end;
        }
        unreachable!("a non-empty upload has a final frame")
    };
    put.await.map_err(|e| match e {
        Error::UnsupportedCapability { .. } => format!(
            "{} cannot take dropped files. Its Skiff is older than this app: use Update Skiff… on that Host in Settings › Hosts.",
            on.unwrap_or("This Host")
        ),
        e => message(e),
    })
}
