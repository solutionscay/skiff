//! Terminal color themes: built in, or dropped as files into
//! `<config dir>/skiff/themes/`. A project picks one by id.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TerminalTheme {
    /// `builtin:<slug>` or `file:<file name>`. Old `foot:<slug>` ids mean `builtin:<slug>`.
    pub id: String,
    pub name: String,
    /// "built-in" or "file".
    pub source: String,
    pub foreground: String,
    pub background: String,
    pub cursor: Option<String>,
    pub selection: Option<String>,
    /// ANSI 0–15: regular 0–7, then bright 0–7. `#rrggbb`.
    pub palette: Vec<String>,
    /// App colors a theme file brings (Superset's `ui` block: background,
    /// card, sidebar, border, mutedForeground, primary, warning, destructive,
    /// ...). Without it the app derives its colors from the palette.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<std::collections::BTreeMap<String, String>>,
}

/// The default when a project names no theme.
pub const DEFAULT_THEME: &str = "builtin:harbor";

const BUILTIN: &[(&str, &str, &str, &str, [&str; 16])] = &[
    ("harbor", "Harbor", "#e6e8eb", "#0b0e12", [
        "#0f1216", "#ff8a80", "#7ee0cb", "#ffb454", "#6b9cff", "#b69cff", "#7ee0cb", "#c3c9d1",
        "#7d8794", "#ffb3b3", "#a4f0e0", "#ffcf8a", "#9ec1ff", "#d0c2ff", "#a4f0e0", "#ffffff",
    ]),
    ("gruvbox-dark", "Gruvbox Dark", "#ebdbb2", "#282828", [
        "#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
        "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2",
    ]),
    ("nord", "Nord", "#d8dee9", "#2e3440", [
        "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
        "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4",
    ]),
    ("dracula", "Dracula", "#f8f8f2", "#282a36", [
        "#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
        "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff",
    ]),
    ("catppuccin-mocha", "Catppuccin Mocha", "#cdd6f4", "#1e1e2e", [
        "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
        "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
    ]),
    ("tokyo-night", "Tokyo Night", "#c0caf5", "#1a1b26", [
        "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
        "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
    ]),
    ("one-dark", "One Dark", "#abb2bf", "#282c34", [
        "#282c34", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#abb2bf",
        "#5c6370", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#ffffff",
    ]),
    ("rose-pine", "Rosé Pine", "#e0def4", "#191724", [
        "#26233a", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
        "#6e6a86", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
    ]),
    ("kanagawa", "Kanagawa", "#dcd7ba", "#1f1f28", [
        "#090618", "#c34043", "#76946a", "#c0a36e", "#7e9cd8", "#957fb8", "#6a9589", "#c8c093",
        "#727169", "#e82424", "#98bb6c", "#e6c384", "#7fb4ca", "#938aa9", "#7aa89f", "#dcd7ba",
    ]),
    ("everforest-dark", "Everforest Dark", "#d3c6aa", "#2d353b", [
        "#475258", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6", "#83c092", "#d3c6aa",
        "#475258", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6", "#83c092", "#d3c6aa",
    ]),
    ("solarized-dark", "Solarized Dark", "#839496", "#002b36", [
        "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
        "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3",
    ]),
    ("solarized-light", "Solarized Light", "#657b83", "#fdf6e3", [
        "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
        "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3",
    ]),
];

/// The palette of the Foot themes: 16 colors shared, each theme its own background and text.
const FOOT_PALETTE: [&str; 16] = [
    "#08001a", "#ff1177", "#aaff00", "#ffdd00", "#3366ff", "#cc00ff", "#00ffcc", "#d8c8ff",
    "#440066", "#ff4499", "#ccff33", "#ffff00", "#33aaff", "#ff00ff", "#00ffff", "#ffffff",
];

/// Foot themes: (slug, name, foreground, background) over FOOT_PALETTE.
const FOOT: &[(&str, &str, &str, &str)] = &[
    ("foot", "Ultraviolet", "#e0ccff", "#08001a"),
    ("default", "Blackout", "#eee8d5", "#000000"),
    ("neon-tropic", "Neon Tropic", "#7dffc3", "#060807"),
    ("moonlight", "Moonlight", "#c4d2e8", "#07080e"),
    ("ember", "Ember", "#ffb86a", "#0a0705"),
    ("lagoon", "Lagoon", "#7ef0d8", "#050c0c"),
    ("violet-pulse", "Violet Pulse", "#dcc4ff", "#08050e"),
    ("manila-brief", "Manila Brief", "#e4d2ac", "#0e0c08"),
    ("navy-cut", "Navy Cut", "#eccc94", "#060a07"),
    ("pad-39", "Pad 39", "#ced6d0", "#08090d"),
    ("galley-7", "Galley 7", "#d2d0b0", "#0a0a08"),
    ("dradis", "DRADIS", "#e8a838", "#070503"),
    ("candy", "Candy", "#ffc6e8", "#1a0512"),
    ("hail-mary", "Hail Mary", "#ff6455", "#05040a"),
    ("pirate", "Pirate", "#e8bf5c", "#03100e"),
    ("jungle-canopy", "Jungle Canopy", "#8fdc6a", "#040c05"),
    ("macaw", "Macaw", "#ffcf3f", "#050a04"),
    ("sonar", "Sonar", "#6ec8e8", "#04121c"),
    ("ice-station", "Ice Station", "#b8d4f0", "#080c16"),
    ("prussian", "Prussian", "#5a9fd4", "#07101c"),
    ("adriatic", "Adriatic", "#4fd4ff", "#04161e"),
    ("blue-note", "Blue Note", "#8eb4ff", "#0c1424"),
];

/// Every theme: Harbor, the Foot themes, the classics, then theme files by name.
pub fn list() -> Vec<TerminalTheme> {
    let theme = |slug: &str, name: &str, fg: &str, bg: &str, pal: &[&str; 16]| TerminalTheme {
        id: format!("builtin:{slug}"),
        name: name.to_string(),
        source: "built-in".into(),
        foreground: fg.to_string(),
        background: bg.to_string(),
        cursor: None,
        selection: None,
        palette: pal.iter().map(|c| c.to_string()).collect(),
        ui: None,
    };
    let mut out = Vec::new();
    let mut files: Vec<(TerminalTheme, Option<String>)> = theme_dir()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| rd.flatten().filter_map(|e| from_file(&e.path())).collect())
        .unwrap_or_default();
    files.sort_by(|a, b| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()));
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let (harbor, classics) = BUILTIN.split_first().expect("Harbor is first");
    out.push(theme(harbor.0, harbor.1, harbor.2, harbor.3, &harbor.4));
    out.extend(FOOT.iter().map(|(slug, name, fg, bg)| theme(slug, name, fg, bg, &FOOT_PALETTE)));
    // A theme file replaces a classic of the same name: it may carry app colors.
    out.extend(
        classics
            .iter()
            .filter(|c| !files.iter().any(|(f, _)| same(&f.name, c.1)))
            .map(|(slug, name, fg, bg, pal)| theme(slug, name, fg, bg, pal)),
    );
    for (mut f, author) in files {
        // Never shadow one of the Foot themes: keep both, the file with its author.
        if FOOT.iter().any(|x| same(x.1, &f.name)) {
            f.name = format!("{} ({})", f.name, author.unwrap_or_else(|| "file".into()));
        }
        out.push(f);
    }
    out
}

fn theme_dir() -> Option<PathBuf> {
    crate::config::config_path().parent().map(|d| d.join("themes"))
}

/// `#rrggbb` from `rrggbb`, `#rrggbb`, or `0xrrggbb`.
fn hex(v: &str) -> Option<String> {
    let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
    let v = v.strip_prefix('#').or_else(|| v.strip_prefix("0x")).unwrap_or(v);
    let v = v.get(..6)?;
    v.chars().all(|c| c.is_ascii_hexdigit()).then(|| format!("#{}", v.to_ascii_lowercase()))
}

/// Flat `key = value` or `key: value` lines, with the section they sit in.
fn pairs(text: &str) -> Vec<(String, String, String)> {
    let mut section = String::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split(" #").next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').trim().to_string();
            continue;
        }
        let Some(i) = line.find(['=', ':']) else { continue };
        let (k, v) = line.split_at(i);
        out.push((section.clone(), k.trim().to_string(), v[1..].trim().to_string()));
    }
    out
}

fn finish(
    id: String,
    name: String,
    source: &str,
    fg: Option<String>,
    bg: Option<String>,
    pal: Vec<Option<String>>,
    cursor: Option<String>,
    selection: Option<String>,
) -> Option<TerminalTheme> {
    let palette: Option<Vec<String>> = pal.into_iter().collect();
    Some(TerminalTheme {
        id,
        name,
        source: source.into(),
        foreground: fg?,
        background: bg?,
        cursor,
        selection,
        palette: palette?,
        ui: None,
    })
}

fn parse_foot(text: &str, id: String, name: String, source: &str) -> Option<TerminalTheme> {
    let kv: Vec<(String, String)> = pairs(text)
        .into_iter()
        .filter(|(s, _, _)| s == "colors" || s == "colors-dark")
        .map(|(_, k, v)| (k, v))
        .collect();
    let get = |k: &str| kv.iter().find(|(kk, _)| kk == k).and_then(|(_, v)| hex(v));
    let mut pal = Vec::new();
    for p in ["regular", "bright"] {
        for i in 0..8 {
            pal.push(get(&format!("{p}{i}")));
        }
    }
    let cursor = kv
        .iter()
        .find(|(k, _)| k == "cursor")
        .and_then(|(_, v)| v.split_whitespace().nth(1).and_then(hex));
    finish(id, name, source, get("foreground"), get("background"), pal, cursor, get("selection-background"))
}

/// A theme file, with its author when the file names one.
fn from_file(path: &Path) -> Option<(TerminalTheme, Option<String>)> {
    let text = std::fs::read_to_string(path).ok()?;
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
        return from_json(path, &text);
    }
    parse_file(path, &text).map(|t| (t, None))
}

/// Superset's theme JSON: `name`, `author`, `terminal` (xterm names), `ui`.
fn from_json(path: &Path, text: &str) -> Option<(TerminalTheme, Option<String>)> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let term = v.get("terminal")?.as_object()?;
    let c = |k: &str| term.get(k).and_then(|x| x.as_str()).and_then(hex);
    let names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
    let mut pal: Vec<Option<String>> = names.iter().map(|n| c(n)).collect();
    pal.extend(names.iter().map(|n| c(&format!("bright{}{}", n[..1].to_uppercase(), &n[1..]))));
    let ui = v.get("ui").and_then(|u| u.as_object()).map(|u| {
        u.iter()
            .filter_map(|(k, x)| x.as_str().and_then(hex).map(|h| (k.clone(), h)))
            .collect()
    });
    let file = path.file_name()?.to_string_lossy().into_owned();
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let name = v.get("name").and_then(|x| x.as_str()).unwrap_or(&stem).to_string();
    let mut t = finish(format!("file:{file}"), name, "file", c("foreground"), c("background"), pal, c("cursor"), c("selectionBackground"))?;
    t.ui = ui;
    let author = v.get("author").and_then(|x| x.as_str()).map(|a| {
        // "Pavel Pertsev (port: Baris Can Sayin)" -> "Pavel Pertsev"
        a.split(" (").next().unwrap_or(a).to_string()
    });
    Some((t, author))
}

fn parse_file(path: &Path, text: &str) -> Option<TerminalTheme> {
    let file = path.file_name()?.to_string_lossy().into_owned();
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let id = format!("file:{file}");
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let kv = pairs(text);
    let named = |k: &str| kv.iter().find(|(_, kk, _)| kk == k).map(|(_, _, v)| v.trim_matches('"').to_string());
    let name = named("name").or_else(|| named("scheme")).unwrap_or_else(|| stem.clone());

    // base16 / base24 YAML: base00..base0F (and base10..base17).
    if ext == "yaml" || ext == "yml" {
        let b = |k: &str| kv.iter().find(|(_, kk, _)| kk == k).and_then(|(_, _, v)| hex(v));
        let pick = |k: &str, alt: &str| b(k).or_else(|| b(alt));
        let pal = vec![
            b("base00"), b("base08"), b("base0B"), b("base0A"), b("base0D"), b("base0E"), b("base0C"), b("base05"),
            b("base03"), pick("base12", "base08"), pick("base14", "base0B"), pick("base13", "base0A"),
            pick("base16", "base0D"), pick("base17", "base0E"), pick("base15", "base0C"), b("base07"),
        ];
        return finish(id, name, "file", b("base05"), b("base00"), pal, None, b("base02"));
    }
    // foot .ini
    if kv.iter().any(|(s, _, _)| s == "colors" || s == "colors-dark") && ext != "toml" {
        return parse_foot(&text, id, name, "file");
    }
    // Skiff TOML: [terminal] foreground/background/regular/bright arrays.
    if ext == "toml" {
        let doc: toml::Table = text.parse().ok()?;
        if let Some(t) = doc.get("terminal").and_then(|v| v.as_table()) {
            let s = |k: &str| t.get(k).and_then(|v| v.as_str()).and_then(hex);
            let arr = |k: &str| -> Vec<Option<String>> {
                t.get(k)
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().map(|x| x.as_str().and_then(hex)).collect())
                    .unwrap_or_default()
            };
            let mut pal = arr("regular");
            pal.extend(arr("bright"));
            if pal.len() != 16 {
                return None;
            }
            let name = doc.get("name").and_then(|v| v.as_str()).map(String::from).unwrap_or(name);
            return finish(id, name, "file", s("foreground"), s("background"), pal, s("cursor"), s("selection"));
        }
        // Alacritty: [colors.primary], [colors.normal], [colors.bright].
        let colors = doc.get("colors").and_then(|v| v.as_table())?;
        let sec = |s: &str, k: &str| {
            colors.get(s).and_then(|v| v.as_table()).and_then(|t| t.get(k)).and_then(|v| v.as_str()).and_then(hex)
        };
        let names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
        let mut pal: Vec<Option<String>> = names.iter().map(|n| sec("normal", n)).collect();
        pal.extend(names.iter().map(|n| sec("bright", n)));
        return finish(id, name, "file", sec("primary", "foreground"), sec("primary", "background"), pal, sec("cursor", "cursor"), sec("selection", "background"));
    }
    // Ghostty: `palette = N=#hex`, background, foreground.
    if kv.iter().any(|(_, k, _)| k == "palette") {
        let mut pal = vec![None; 16];
        for (_, k, v) in &kv {
            if k == "palette" {
                if let Some((n, c)) = v.split_once('=') {
                    if let Ok(i) = n.trim().parse::<usize>() {
                        if i < 16 {
                            pal[i] = hex(c);
                        }
                    }
                }
            }
        }
        let g = |k: &str| kv.iter().find(|(_, kk, _)| kk == k).and_then(|(_, _, v)| hex(v));
        return finish(id, name, "file", g("foreground"), g("background"), pal, g("cursor-color"), g("selection-background"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_foot_and_base16() {
        let foot = "[colors]\nbackground=08001a\nforeground=e0ccff\nregular0=000000\nregular1=ff0000\nregular2=00ff00\nregular3=ffff00\nregular4=0000ff\nregular5=ff00ff\nregular6=00ffff\nregular7=ffffff\nbright0=111111\nbright1=ff1111\nbright2=11ff11\nbright3=ffff11\nbright4=1111ff\nbright5=ff11ff\nbright6=11ffff\nbright7=eeeeee\n";
        let t = parse_foot(foot, "x".into(), "X".into(), "foot").unwrap();
    assert!(t.ui.is_none());
        assert_eq!(t.background, "#08001a");
        assert_eq!(t.palette.len(), 16);
        assert_eq!(t.palette[9], "#ff1111");
        // Theme files on this machine may replace some classics.
        let n = builtin_count();
        assert!(n >= 1 + FOOT.len() && n <= BUILTIN.len() + FOOT.len());
    }

    fn builtin_count() -> usize {
        list().iter().filter(|t| t.source == "built-in").count()
    }
}
