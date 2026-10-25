//! The image cache: images dropped or pasted on a pane are copied here, and
//! the pane gets the copy's path. `<agent folder>/cache/<hash>.<ext>`, or
//! `/tmp/skiff-drops/` when the Skiff agent has no usable folder. The cache
//! does not need the Skiff agent to be on. Other files keep their own path.
//!
//! The first bytes decide what is an image, not the extension. The name is
//! the first 16 hex characters of the SHA-256 of the bytes, so the same
//! image is one file. A repeat sets the mtime to now and writes nothing.

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use skiff_core::{
    config::Config,
    project::AgentInfo,
    session::{Role, SessionSpec},
    shell::quote_word,
    tasks,
};

/// An image larger than this keeps its own path.
pub const MAX_BYTES: u64 = 50 * 1024 * 1024;

/// What `showError` says for an image over [`MAX_BYTES`].
pub const TOO_LARGE: &str = "The image is larger than 50 MB. Skiff typed its path and did not copy it.";

/// The cache folder. It may not exist yet: [`ensure`] makes it.
pub fn dir(cfg: &Config) -> PathBuf {
    crate::skipper::cache_dir(cfg)
}

/// Makes the cache folder, mode 0700, and returns it.
pub fn ensure(cfg: &Config) -> Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let d = dir(cfg);
    if !d.is_dir() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&d)
            .with_context(|| format!("create {}", d.display()))?;
    }
    Ok(d)
}

/// The cache extension of an image, from its first bytes. SVG also needs the
/// `.svg` extension. `None`: not an image.
pub fn image_ext(head: &[u8], file_ext: Option<&str>) -> Option<&'static str> {
    let at = |i: usize, sig: &[u8]| head.get(i..i + sig.len()) == Some(sig);
    if at(0, b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if at(0, b"\xff\xd8\xff") {
        return Some("jpg");
    }
    if at(0, b"GIF87a") || at(0, b"GIF89a") {
        return Some("gif");
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return Some("webp");
    }
    if at(0, b"BM") && head.len() >= 26 {
        return Some("bmp");
    }
    if at(0, b"II*\0") || at(0, b"MM\0*") {
        return Some("tif");
    }
    if at(4, b"ftyp") {
        return match head.get(8..12) {
            Some(b"avif" | b"avis") => Some("avif"),
            Some(b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"hevm" | b"hevs" | b"mif1" | b"msf1") => Some("heic"),
            _ => None,
        };
    }
    if file_ext.is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
        let text = String::from_utf8_lossy(head);
        let t = text.trim_start_matches('\u{feff}').trim_start();
        if (t.starts_with("<svg") || t.starts_with("<?xml")) && t.contains("<svg") {
            return Some("svg");
        }
    }
    None
}

/// Puts `bytes` in the cache as `<hash>.<ext>`. An existing copy gets a new
/// mtime, so an image still in use stays. A new one is written beside its
/// name and renamed into place, so a crash leaves no half file.
pub fn store(dir: &Path, bytes: &[u8], ext: &str) -> Result<PathBuf> {
    write_once(&dir.join(format!("{}.{ext}", hash16(bytes))), bytes)
}

/// The first 16 hex characters of the bytes' SHA-256.
fn hash16(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A file a client sent for a pane on this Host (a drop or a paste on a
/// remote pane): an image as [`store`] keeps it, any other file as
/// `cache/files/<hash>/<name>`, so it keeps its name. `name` is one file
/// name; anything else is refused.
pub fn put(cfg: &Config, name: &str, bytes: &[u8]) -> Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
        return Err(skiff_core::config::Refused::Invalid(format!("{name:?} is not a file name")).into());
    }
    let dir = ensure(cfg)?;
    let ext = Path::new(name).extension().and_then(|e| e.to_str());
    if let Some(ext) = image_ext(bytes, ext) {
        return store(&dir, bytes, ext);
    }
    let sub = dir.join("files").join(hash16(bytes));
    std::fs::create_dir_all(&sub).with_context(|| format!("create {}", sub.display()))?;
    write_once(&sub.join(name), bytes)
}

/// Writes `path` beside its name and renames it into place. An existing
/// file gets a new mtime instead.
fn write_once(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let path = path.to_path_buf();
    let dir = path.parent().unwrap_or(Path::new("."));
    if path.is_file() {
        let f = std::fs::File::options().write(true).open(&path).with_context(|| format!("open {}", path.display()))?;
        f.set_modified(std::time::SystemTime::now()).with_context(|| format!("touch {}", path.display()))?;
        return Ok(path);
    }
    let tmp = dir.join(format!("{}.tmp-{}", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("write {}", path.display()));
    }
    Ok(path)
}

/// What Skiff types for dropped or pasted files.
#[derive(Debug, Default, PartialEq)]
pub struct Typed {
    /// Each file's word, quoted, in order.
    pub words: Vec<String>,
    /// The cache paths of the images, in order.
    pub images: Vec<String>,
    /// An image was over [`MAX_BYTES`]: it kept its own path.
    pub too_large: bool,
}

/// The words for `paths`, in order: an image's cache copy, else the path as
/// it is. A failed copy fails the whole drop, so nothing is typed.
pub fn files(dir: &Path, paths: &[PathBuf]) -> Result<Typed> {
    let mut out = Typed::default();
    for p in paths {
        let word = |p: &Path| quote_word(&p.to_string_lossy());
        let Some((len, ext)) = image_of(p)? else {
            out.words.push(word(p));
            continue;
        };
        if len > MAX_BYTES {
            out.too_large = true;
            out.words.push(word(p));
            continue;
        }
        let bytes = std::fs::read(p).with_context(|| format!("read {}", p.display()))?;
        let stored = store(dir, &bytes, ext)?;
        out.words.push(word(&stored));
        out.images.push(stored.to_string_lossy().into_owned());
    }
    Ok(out)
}

/// A file's length and cache extension when it is an image. A folder, or a
/// file Skiff cannot read, is not.
fn image_of(p: &Path) -> Result<Option<(u64, &'static str)>> {
    let Ok(m) = std::fs::metadata(p) else { return Ok(None) };
    if !m.is_file() {
        return Ok(None);
    }
    let Ok(f) = std::fs::File::open(p) else { return Ok(None) };
    let mut head = Vec::with_capacity(4096);
    if f.take(4096).read_to_end(&mut head).is_err() {
        return Ok(None);
    }
    let file_ext = p.extension().and_then(|e| e.to_str());
    Ok(image_ext(&head, file_ext).map(|e| (m.len(), e)))
}

/// The flags for the agent's start line, as argv words. See [`tasks::cache_flags`].
pub fn flags(cfg: &Config, agent: &AgentInfo) -> Vec<String> {
    tasks::cache_flags(agent, &dir(cfg)).unwrap_or_default()
}

/// [`flags`] as text to type after a start line in a shell.
pub fn flags_typed(cfg: &Config, agent: &AgentInfo) -> String {
    tasks::cache_flags_typed(agent, &dir(cfg))
}

/// Adds the cache flags to an agent session that runs one of `agents` as
/// its command, as the + menu, `skiff new` and Skipper start one.
pub fn add_flags(cfg: &Config, agents: &[AgentInfo], spec: &mut SessionSpec) {
    if spec.role != Role::Agent || spec.peek {
        return;
    }
    let Some(program) = spec.command.as_deref() else { return };
    let started = |a: &&AgentInfo| {
        let mut words = a.command.split_whitespace();
        words.next() == Some(program) && words.zip(spec.args.iter()).all(|(w, arg)| w == arg)
    };
    if let Some(a) = agents.iter().find(started) {
        let more = flags(cfg, a);
        spec.args.extend(more);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_put_keeps_images_by_hash_and_files_by_name() {
        let root = std::env::temp_dir().join(format!("skiff-put-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let cfg = Config { skiff_agent: skiff_core::config::SkiffAgent { folder: Some(root.clone()), ..Default::default() }, ..Default::default() };
        // A folder outside home is refused: the put falls back to the /tmp cache, which is fine here.
        let png = put(&cfg, "shot.png", b"\x89PNG\r\n\x1a\nbody").unwrap();
        let note = put(&cfg, "note.txt", b"pelican").unwrap();
        let again = put(&cfg, "note.txt", b"pelican").unwrap();
        let bad = put(&cfg, "../x", b"x").is_err() && put(&cfg, "", b"x").is_err();
        let note_text = std::fs::read(&note).unwrap();
        let _ = std::fs::remove_file(&png);
        let _ = std::fs::remove_dir_all(note.parent().unwrap());
        std::fs::remove_dir_all(&root).unwrap();
        assert!(png.extension().unwrap() == "png" && png.file_name().unwrap().len() == 20, "{png:?}");
        assert_eq!(note.file_name().unwrap(), "note.txt");
        assert_eq!(note.parent().unwrap().parent().unwrap().file_name().unwrap(), "files");
        assert_eq!(note, again);
        assert_eq!(note_text, b"pelican");
        assert!(bad);
    }

    #[test]
    fn the_first_bytes_decide() {
        assert_eq!(image_ext(b"\x89PNG\r\n\x1a\n....", Some("txt")), Some("png"));
        assert_eq!(image_ext(b"\xff\xd8\xff\xe0", None), Some("jpg"));
        assert_eq!(image_ext(b"GIF89a..", None), Some("gif"));
        assert_eq!(image_ext(b"RIFF\0\0\0\0WEBPVP8 ", None), Some("webp"));
        assert_eq!(image_ext(b"\0\0\0\x1cftypavif", None), Some("avif"));
        assert_eq!(image_ext(b"\0\0\0\x18ftypheic", None), Some("heic"));
        assert_eq!(image_ext(b"\0\0\0\x18ftypisom", None), None);
        assert_eq!(image_ext(b"II*\0", None), Some("tif"));
        assert_eq!(image_ext(b"<?xml version=\"1.0\"?><svg>", Some("SVG")), Some("svg"));
        assert_eq!(image_ext(b"<svg xmlns=", Some("txt")), None);
        assert_eq!(image_ext(b"<?xml version=\"1.0\"?><html>", Some("svg")), None);
        assert_eq!(image_ext(b"hello", Some("png")), None);
    }

    #[test]
    fn drops_copy_images_once_and_keep_other_paths() {
        let root = std::env::temp_dir().join(format!("skiff-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cache = root.join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let png = root.join("shot one.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\nbody").unwrap();
        let note = root.join("note.txt");
        std::fs::write(&note, b"pelican").unwrap();
        let t = files(&cache, &[png.clone(), note.clone(), root.clone(), png.clone()]).unwrap();
        let copy = std::fs::read_dir(&cache).unwrap().map(|e| e.unwrap().path()).collect::<Vec<_>>();
        let named = copy.len() == 1 && copy[0].file_name().unwrap().to_string_lossy().len() == 16 + 4;
        std::fs::remove_dir_all(&root).unwrap();
        assert!(named, "{copy:?}");
        assert_eq!(t.images.len(), 2);
        assert_eq!(t.images[0], t.images[1]);
        assert_eq!(t.words[0], t.images[0]);
        assert_eq!(t.words[1], note.to_string_lossy());
        assert_eq!(t.words[2], root.to_string_lossy());
        assert!(!t.too_large);
        assert_eq!(quote_word("/a/shot one.png"), "'/a/shot one.png'");
        assert_eq!(quote_word("/a/it's"), r"'/a/it'\''s'");
    }
}
