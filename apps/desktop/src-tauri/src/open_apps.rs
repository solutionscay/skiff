//! What Settings › Open with can pick, read from this computer: the terminal
//! tools on the login shell's PATH, and the apps that say they open a kind of
//! file. Linux reads `.desktop` files. macOS reads each `.app`'s `Info.plist`.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

use serde::Serialize;
use skiff_core::shell::shell_quote;

/// One thing a row can pick. `command` is what `[open]` stores.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub label: String,
    pub command: String,
}

/// One row's list. `peek`: terminal tools. `apps`: a window of their own.
#[derive(Serialize, Debug, Default, PartialEq, Eq)]
pub(crate) struct Row {
    pub peek: Vec<Choice>,
    pub apps: Vec<Choice>,
}

pub(crate) type Choices = BTreeMap<&'static str, Row>;

const KEYS: [&str; 5] = ["diff", "text", "markdown", "html", "image"];

/// The terminal tools Skiff knows: row, label, program, command.
const TOOLS: &[(&str, &str, &str, &str)] = &[
    ("diff", "delta", "delta", "git -c core.pager=delta diff {target}"),
    ("diff", "difftastic", "difft", "git -c diff.external=difft diff {target}"),
    ("text", "nvim", "nvim", "nvim"),
    ("text", "vim", "vim", "vim"),
    ("text", "Helix", "hx", "hx"),
    ("text", "nano", "nano", "nano"),
    ("text", "micro", "micro", "micro"),
    ("text", "less", "less", "less"),
    ("markdown", "glow", "glow", "glow -p"),
    ("markdown", "nvim", "nvim", "nvim"),
    ("markdown", "vim", "vim", "vim"),
    ("markdown", "less", "less", "less"),
    ("html", "w3m", "w3m", "w3m"),
    ("html", "lynx", "lynx", "lynx"),
];

/// The diff apps with a window of their own, on PATH: label, program, command.
const DIFF_APPS: &[(&str, &str, &str)] = &[
    ("Meld", "meld", "git difftool --no-prompt --tool=meld {target}"),
    ("KDiff3", "kdiff3", "git difftool --no-prompt --tool=kdiff3 {target}"),
];

pub(crate) fn read() -> Choices {
    let dirs = path_dirs();
    let mut out = tools_on(&dirs);
    for (label, program, command) in DIFF_APPS {
        if on_path(&dirs, program) {
            out.entry("diff").or_default().apps.push(Choice { label: (*label).into(), command: (*command).into() });
        }
    }
    let home = dirs::home_dir().unwrap_or_default();
    if cfg!(target_os = "macos") {
        add_apps(&mut out, mac_apps(&home));
    } else {
        for found in linux_entries(&home) {
            add_entry(&mut out, found);
        }
    }
    for row in out.values_mut() {
        row.apps.sort_by_key(|c| c.label.to_lowercase());
        row.apps.dedup_by(|a, b| a.command == b.command);
    }
    out
}

/// What `[open]` stores for a program from the file picker. A macOS app is
/// a folder, so `open` starts it.
pub(crate) fn command_for_app(path: &str) -> String {
    if cfg!(target_os = "macos") && path.ends_with(".app") {
        format!("open -a {}", shell_quote(path))
    } else {
        shell_quote(path)
    }
}

/// This process's PATH, and the login shell's after it: an app started from
/// the desktop has a short PATH, and the peek runs with the login shell's.
fn path_dirs() -> Vec<PathBuf> {
    static LOGIN: OnceLock<Option<String>> = OnceLock::new();
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(login) = LOGIN.get_or_init(login_path) {
        for d in std::env::split_paths(login) {
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
    }
    dirs
}

/// The login shell's PATH, or nothing after 3 s: a slow shell must not hold Settings.
fn login_path() -> Option<String> {
    let shell = std::env::var("SHELL").ok()?;
    let mut child = Command::new(shell)
        .args(["-lc", "printf %s \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(3) {
        if child.try_wait().ok()?.is_some() {
            let out = child.wait_with_output().ok()?;
            return out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

fn on_path(dirs: &[PathBuf], program: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    dirs.iter().any(|d| std::fs::metadata(d.join(program)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

fn tools_on(dirs: &[PathBuf]) -> Choices {
    let mut out: Choices = KEYS.iter().map(|k| (*k, Row::default())).collect();
    for (key, label, program, command) in TOOLS {
        if on_path(dirs, program) {
            out.entry(key).or_default().peek.push(Choice { label: (*label).into(), command: (*command).into() });
        }
    }
    out
}

/// One app and the rows it opens.
struct Found {
    choice: Choice,
    keys: BTreeSet<&'static str>,
    /// `Terminal=true`: it needs the peek.
    terminal: bool,
}

fn add_apps(out: &mut Choices, found: Vec<Found>) {
    for f in found {
        add_entry(out, f);
    }
}

fn add_entry(out: &mut Choices, f: Found) {
    for key in &f.keys {
        let row = out.entry(key).or_default();
        if f.terminal {
            // A tool the table lists already shows once.
            let program = f.choice.command.split_whitespace().next().unwrap_or("");
            if !row.peek.iter().any(|c| c.command.split_whitespace().next() == Some(program)) {
                row.peek.push(f.choice.clone());
            }
        } else {
            row.apps.push(f.choice.clone());
        }
    }
}

/// The rows a `.desktop` file's MIME types open.
fn keys_for_mime(types: &str) -> BTreeSet<&'static str> {
    let mut keys = BTreeSet::new();
    for m in types.split(';').map(str::trim) {
        match m {
            "text/plain" => {
                keys.insert("text");
                keys.insert("markdown");
            }
            "text/markdown" | "text/x-markdown" => {
                keys.insert("markdown");
            }
            "text/html" => {
                keys.insert("html");
            }
            m if m.starts_with("image/") => {
                keys.insert("image");
            }
            _ => {}
        }
    }
    keys
}

/// A `.desktop` file's `[Desktop Entry]`: its app, when it shows in menus and
/// opens something. The first file code (`%f`, `%U`) becomes `{path}`, so a
/// wrapper such as flatpak gets the file where it asks for it. Other codes go.
fn parse_entry(text: &str) -> Option<Found> {
    let (mut name, mut exec, mut mime, mut terminal) = (None, None, "", false);
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match (k.trim(), v.trim()) {
            ("Type", t) if t != "Application" => return None,
            ("NoDisplay" | "Hidden", "true") => return None,
            ("Name", v) => name = Some(v),
            ("Exec", v) => exec = Some(v),
            ("MimeType", v) => mime = v,
            ("Terminal", "true") => terminal = true,
            _ => {}
        }
    }
    let keys = keys_for_mime(mime);
    if keys.is_empty() {
        return None;
    }
    let mut placed = false;
    let mut command = Vec::new();
    for word in exec?.split_whitespace() {
        match word {
            "%f" | "%F" | "%u" | "%U" if !placed => {
                placed = true;
                command.push("{path}");
            }
            w if w.len() == 2 && w.starts_with('%') => {}
            w => command.push(w),
        }
    }
    if command.is_empty() {
        return None;
    }
    Some(Found { choice: Choice { label: name?.into(), command: command.join(" ") }, keys, terminal })
}

/// The `applications` folders, the user's first, as the XDG spec orders them.
fn linux_entries(home: &Path) -> Vec<Found> {
    let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let data_dirs = std::env::var("XDG_DATA_DIRS").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let roots = std::iter::once(data_home).chain(data_dirs.split(':').map(PathBuf::from));
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for root in roots {
        let Ok(dir) = std::fs::read_dir(root.join("applications")) else { continue };
        for entry in dir.flatten() {
            let name = entry.file_name();
            // The first file with an id wins, so the user's copy can hide or replace a system one.
            if !name.to_string_lossy().ends_with(".desktop") || !seen.insert(name) {
                continue;
            }
            if let Some(found) = std::fs::read_to_string(entry.path()).ok().as_deref().and_then(parse_entry) {
                out.push(found);
            }
        }
    }
    out
}

/// The rows an `Info.plist` document type opens.
fn keys_for_doc_type(doc: &plist::Dictionary) -> BTreeSet<&'static str> {
    let mut keys = BTreeSet::new();
    if doc.get("LSHandlerRank").and_then(|r| r.as_string()) == Some("None") {
        return keys;
    }
    let words = |name: &str| -> Vec<String> {
        doc.get(name).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_string()).map(str::to_lowercase).collect()).unwrap_or_default()
    };
    for ext in words("CFBundleTypeExtensions") {
        match ext.as_str() {
            "txt" | "text" => {
                keys.insert("text");
                keys.insert("markdown");
            }
            "md" | "markdown" => {
                keys.insert("markdown");
            }
            "html" | "htm" => {
                keys.insert("html");
            }
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "tiff" | "svg" => {
                keys.insert("image");
            }
            _ => {}
        }
    }
    for uti in words("LSItemContentTypes") {
        match uti.as_str() {
            "public.plain-text" | "public.text" | "public.utf8-plain-text" | "public.source-code" => {
                keys.insert("text");
                keys.insert("markdown");
            }
            "net.daringfireball.markdown" => {
                keys.insert("markdown");
            }
            "public.html" => {
                keys.insert("html");
            }
            "public.image" | "public.png" | "public.jpeg" | "public.tiff" | "com.compuserve.gif" | "org.webmproject.webp" => {
                keys.insert("image");
            }
            _ => {}
        }
    }
    keys
}

/// One `.app`: its name and the rows its `Info.plist` says it opens.
fn parse_bundle(app: &Path) -> Option<Found> {
    let info: plist::Dictionary = plist::from_file(app.join("Contents/Info.plist")).ok()?;
    let keys: BTreeSet<&'static str> = info
        .get("CFBundleDocumentTypes")?
        .as_array()?
        .iter()
        .filter_map(|d| d.as_dictionary())
        .flat_map(keys_for_doc_type)
        .collect();
    if keys.is_empty() {
        return None;
    }
    let text = |k: &str| info.get(k).and_then(|v| v.as_string()).map(str::to_string).filter(|s| !s.is_empty());
    let label = text("CFBundleDisplayName").or_else(|| text("CFBundleName")).or_else(|| app.file_stem().map(|s| s.to_string_lossy().into_owned()))?;
    Some(Found { choice: Choice { label, command: format!("open -a {}", shell_quote(&app.to_string_lossy())) }, keys, terminal: false })
}

/// Every `.app` in the folders the Finder lists as Applications.
fn mac_apps(home: &Path) -> Vec<Found> {
    let roots = [home.join("Applications"), "/Applications".into(), "/Applications/Utilities".into(), "/System/Applications".into(), "/System/Applications/Utilities".into()];
    roots
        .iter()
        .filter_map(|r| std::fs::read_dir(r).ok())
        .flat_map(|d| d.flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .filter_map(|p| parse_bundle(&p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_file_gives_its_app_with_the_file_where_it_asks() {
        let f = parse_entry("[Desktop Entry]\nType=Application\nName=Zed\nExec=zed --wait %F\nMimeType=text/plain;text/html;\n[Desktop Action new]\nName=New\nExec=zed --new\n").unwrap();
        assert_eq!(f.choice, Choice { label: "Zed".into(), command: "zed --wait {path}".into() });
        assert_eq!(f.keys, BTreeSet::from(["text", "markdown", "html"]));
        assert!(!f.terminal);
    }

    #[test]
    fn a_hidden_desktop_file_or_one_that_opens_nothing_gives_no_app() {
        assert!(parse_entry("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\nMimeType=text/plain;\n").is_none());
        assert!(parse_entry("[Desktop Entry]\nType=Application\nName=X\nExec=x\nMimeType=audio/mpeg;\n").is_none());
    }

    #[test]
    fn a_terminal_desktop_file_goes_to_the_peek_list_once() {
        let mut out: Choices = BTreeMap::new();
        out.entry("text").or_default().peek.push(Choice { label: "vim".into(), command: "vim".into() });
        add_entry(&mut out, parse_entry("[Desktop Entry]\nType=Application\nName=Vim\nExec=vim %F\nTerminal=true\nMimeType=text/plain;\n").unwrap());
        assert_eq!(out["text"].peek.len(), 1);
        assert!(out["text"].apps.is_empty());
    }

    #[test]
    fn a_bundle_reads_binary_or_xml_plists_and_skips_rank_none() {
        let dir = std::env::temp_dir().join(format!("skiff-open-apps-{}", std::process::id()));
        let app = dir.join("My Editor.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        let doc = |exts: &[&str], rank: &str| {
            let mut d = plist::Dictionary::new();
            d.insert("CFBundleTypeExtensions".into(), plist::Value::Array(exts.iter().map(|e| plist::Value::String((*e).into())).collect()));
            d.insert("LSHandlerRank".into(), plist::Value::String(rank.into()));
            plist::Value::Dictionary(d)
        };
        let mut info = plist::Dictionary::new();
        info.insert("CFBundleName".into(), plist::Value::String("My Editor".into()));
        info.insert("CFBundleDocumentTypes".into(), plist::Value::Array(vec![doc(&["md"], "Owner"), doc(&["html"], "None")]));
        plist::to_file_binary(app.join("Contents/Info.plist"), &info).unwrap();
        let f = parse_bundle(&app).unwrap();
        assert_eq!(f.choice.label, "My Editor");
        assert!(f.choice.command.starts_with("open -a "));
        assert_eq!(f.keys, BTreeSet::from(["markdown"]));
        plist::to_file_xml(app.join("Contents/Info.plist"), &info).unwrap();
        assert_eq!(parse_bundle(&app).unwrap().keys, BTreeSet::from(["markdown"]));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
