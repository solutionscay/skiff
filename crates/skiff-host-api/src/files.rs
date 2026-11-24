//! M5 data plane: the files of a Host's project worktrees (`files@1`) and
//! a project's configured images as bytes (`project_assets@1`). Served by
//! `skiff-host`. Every path is a Host path, opaque to the client: the client
//! never opens, resolves or hands one to an app on its own machine. What it
//! gets is bounded bytes or text, never a path to read.
use serde::{Deserialize, Serialize};

/// An entry as it is on the Host's disk. A link is not followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Dir,
    File,
    Link,
    /// A FIFO, device or socket: listed, never opened.
    Other,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostEntry {
    pub name: String,
    pub kind: EntryKind,
}

/// One folder of a worktree: folders first, then by name, without `.git`
/// and what the Host's git ignores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostListing {
    pub entries: Vec<HostEntry>,
    /// More entries than the Host lists (2000, after what git ignores), or
    /// more than it reads from disk (40000): the rest are left out.
    pub truncated: bool,
    /// Opaque cursor for the next page. It is never a Host path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<usize>,
}

/// Why a file is read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadPurpose {
    /// To show it: text up to 512 KiB, an image up to 5 MiB, else no bytes.
    #[default]
    Preview,
    /// To hand a copy to an app on the client: every byte of a regular file
    /// up to 5 MiB, else `invalid`. A link is `invalid`.
    Copy,
}

/// What a file holds, decided by the Host from its first bytes, never its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileContent {
    Text,
    /// PNG, JPEG, GIF, WebP, BMP or ICO. SVG is text.
    Image,
    Binary,
    /// A symlink; `target` says where it points. It was not followed.
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFile {
    pub content: FileContent,
    /// Bytes on the Host's disk; for a link, its target's length.
    pub size: u64,
    /// An image's type, from its bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    /// A previewed text file, as UTF-8 with invalid bytes replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A previewed image, or the whole file for a copy. Base64.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::wire::opt_bytes")]
    pub bytes: Option<Vec<u8>>,
    /// Text cut at 512 KiB (at a line end), or an image over 5 MiB not sent.
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// An image the Host validated: a regular file whose first bytes are an image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostImage {
    pub mime: String,
    #[serde(with = "crate::wire::bytes")]
    pub bytes: Vec<u8>,
    /// The whole file's length, on a ranged `image_read` only
    /// (`image_cache@2`). Absent: `bytes` is the whole file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// The raw bytes of one framed `drop_put` or ranged `image_read`. Base64
/// and JSON keep the frame below the 8 MiB frame limit.
pub const DATA_CHUNK: u64 = 4 * 1024 * 1024;

/// The frames of a `total`-byte file, in order: each frame's offset and
/// length, [`DATA_CHUNK`] bytes or fewer. An empty file is one empty frame,
/// so an upload always has a final frame.
pub fn chunks(total: u64) -> impl Iterator<Item = (u64, u64)> {
    let count = total.div_ceil(DATA_CHUNK).max(1);
    (0..count).map(move |i| {
        let offset = i * DATA_CHUNK;
        (offset, DATA_CHUNK.min(total - offset))
    })
}

/// The session that gets a file, as its pane shows it. The Host records it
/// in its library, so the Files tile shows the session line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostWho {
    pub session: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// One file in the Host's cache library: an image or any other dropped
/// file (the name stays from the Images library). `path` is an opaque Host cache path: a
/// client can give it back to this API, but must never open it itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostCacheImage {
    pub path: String,
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub at: u64,
    pub pinned: bool,
    pub size: u64,
}

/// The Host cache's total size and its library's file count and size.
/// `images` and `image_bytes` count every library file, images or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostCacheUse {
    pub bytes: u64,
    pub images: u64,
    pub image_bytes: u64,
}
