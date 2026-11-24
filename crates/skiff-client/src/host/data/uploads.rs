use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use skiff_host_api::{chunks, Capability, Error, HostWho, Request, Response, DATA_CHUNK};

use crate::host::HostConnection;
use super::message;

/// Puts a dropped or pasted file into the Host's cache in one frame.
/// Answers its Host path.
pub async fn drop_put(c: &HostConnection, name: &str, bytes: Vec<u8>, who: Option<HostWho>) -> Result<String, Error> {
    match c.request(Request::DropPut { name: name.into(), bytes, upload: None, total: None, offset: 0, finish: true, who }).await? {
        Response::Path { path } => Ok(path),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The largest file a Host before `drops@2` takes. A `drops@2` Host takes
/// a file up to its own `[skiff_agent] cache_max_mb`, and says so itself.
pub const DROP_MAX: u64 = 50 * 1024 * 1024;

/// A local file to drop on a pane on a remote Host. It is read frame by
/// frame as it is sent.
pub struct DropFile {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

/// A dropped or pasted file for the Host of `c`: no folder, and for a Host
/// before `drops@2` no larger than [`DROP_MAX`]. Answers the file, or the
/// message to show. `on` names the Host.
pub fn drop_file(c: &HostConnection, p: &Path, on: Option<&str>) -> Result<DropFile, String> {
    let on = on.unwrap_or("the Host");
    let m = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if m.is_dir() {
        return Err(format!("Drop files, not folders, on a pane on {on}."));
    }
    if m.len() > DROP_MAX && !c.hello().capabilities.contains(&Capability::drops_limit()) {
        return Err(format!(
            "{} is larger than 50 MB, the limit of the older Skiff on {on}. Use Update Skiff… on that Host in Settings › Hosts.",
            p.display()
        ));
    }
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(DropFile { name, path: p.to_path_buf(), size: m.len() })
}

/// Sends `bytes` (a paste, a small picture) to the Host's cache. Answers
/// its Host path, or the message to show. `on` names the Host.
pub async fn drop_send(c: &HostConnection, name: &str, bytes: Vec<u8>, who: Option<HostWho>, on: Option<&str>) -> Result<String, String> {
    let total = bytes.len() as u64;
    let read = |offset: u64, len: u64| Ok(bytes[offset as usize..(offset + len) as usize].to_vec());
    send(c, name, total, who, on, read, |_| {}).await
}

/// Sends the file `f` to the Host's cache frame by frame, and calls `sent`
/// with the bytes of each frame the Host took. Nothing more crosses the
/// link: the count comes from the replies.
pub async fn drop_send_file(c: &HostConnection, f: &DropFile, who: Option<HostWho>, on: Option<&str>, sent: impl FnMut(u64)) -> Result<String, String> {
    let mut file = std::fs::File::open(&f.path).map_err(|e| format!("{}: {e}", f.path.display()))?;
    let path = f.path.clone();
    let read = move |offset: u64, len: u64| -> Result<Vec<u8>, String> {
        let mut buf = vec![0; len as usize];
        file.seek(SeekFrom::Start(offset)).and_then(|_| file.read_exact(&mut buf)).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(buf)
    };
    send(c, &f.name, f.size, who, on, read, sent).await
}

/// One upload, in the frames [`chunks`] gives. A file of one frame is the
/// original one-frame request, so any Host with `drops@1` takes it.
async fn send(
    c: &HostConnection,
    name: &str,
    total: u64,
    who: Option<HostWho>,
    on: Option<&str>,
    mut read: impl FnMut(u64, u64) -> Result<Vec<u8>, String>,
    mut sent: impl FnMut(u64),
) -> Result<String, String> {
    let failed = |e: Error| match e {
        Error::UnsupportedCapability { .. } => format!(
            "{} cannot take dropped files. Its Skiff is older than this app: use Update Skiff… on that Host in Settings › Hosts.",
            on.unwrap_or("This Host")
        ),
        e => message(e),
    };
    if total <= DATA_CHUNK {
        let path = drop_put(c, name, read(0, total)?, who).await.map_err(failed)?;
        sent(total);
        return Ok(path);
    }
    let upload = uuid::Uuid::new_v4().to_string();
    for (offset, len) in chunks(total) {
        let finish = offset + len == total;
        let reply = c
            .request(Request::DropPut {
                name: name.into(),
                bytes: read(offset, len)?,
                upload: Some(upload.clone()),
                total: Some(total),
                offset,
                finish,
                who: if finish { who.clone() } else { None },
            })
            .await
            .map_err(failed)?;
        sent(len);
        if finish {
            return match reply { Response::Path { path } => Ok(path), _ => unreachable!("checked by Request::accepts") };
        }
    }
    unreachable!("an upload has a final frame")
}

/// Copies one Host library file to `dest`, frame by frame on a Host with
/// `image_cache@2`; an older Host answers it in one frame, which holds a
/// file up to about 6 MB. Writes beside `dest` and renames into place.
pub async fn image_fetch(c: &HostConnection, path: &str, dest: &Path) -> Result<(), String> {
    let tmp = dest.with_file_name(format!(".{}.part", dest.file_name().unwrap_or_default().to_string_lossy()));
    let fetched = fetch_to(c, path, &tmp).await;
    match fetched.and_then(|()| std::fs::rename(&tmp, dest).map_err(|e| format!("{}: {e}", dest.display()))) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

async fn fetch_to(c: &HostConnection, path: &str, tmp: &Path) -> Result<(), String> {
    let mut out = std::fs::File::create(tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let mut write = |bytes: &[u8]| out.write_all(bytes).map_err(|e| format!("{}: {e}", tmp.display()));
    if !c.hello().capabilities.contains(&Capability::image_cache_ranges()) {
        let image = super::image_read(c, path).await.map_err(message)?;
        return write(&image.bytes);
    }
    let range = |offset: u64| Request::ImageRead { path: path.into(), thumb: None, offset: Some(offset), length: Some(DATA_CHUNK) };
    let part = |reply: Response| match reply { Response::Image { image } => image, _ => unreachable!("checked by Request::accepts") };
    let first = part(c.request(range(0)).await.map_err(message)?);
    let total = first.total.unwrap_or(first.bytes.len() as u64);
    write(&first.bytes)?;
    for (offset, _) in chunks(total).skip(1) {
        let image = part(c.request(range(offset)).await.map_err(message)?);
        if image.bytes.is_empty() || image.total != Some(total) {
            return Err(format!("{path} changed on the Host while Skiff read it."));
        }
        write(&image.bytes)?;
    }
    Ok(())
}

/// An Insert of Host library files into `who`'s pane: they go to the top
/// of that Host's library with a new mtime. A Host before `image_cache@2`
/// is left as it is: Ok(false).
pub async fn image_touch(c: &HostConnection, paths: Vec<String>, who: Option<HostWho>) -> Result<bool, Error> {
    if !c.hello().capabilities.contains(&Capability::image_cache_ranges()) {
        return Ok(false);
    }
    c.request(Request::ImageTouch { paths, who }).await.map(|_| true)
}
