// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::OsString;

use clap::Parser;

/// The command line. Parsed before Tauri or GTK starts: `--version`,
/// `--help` and an unknown flag print and exit without a window.
#[derive(Parser)]
#[command(name = "skiff-desktop", version, about = "Skiff: a minimalist desktop workspace for coding agents")]
struct Args {
    /// A workspace file to open, as a path or a file:// URI.
    /// `workspace_file` reads it from the arguments itself.
    #[arg(value_name = "FILE")]
    files: Vec<OsString>,
}

/// The arguments to check. Older macOS adds `-psn_…` when Finder starts an
/// app; it is not ours to reject.
fn args() -> impl Iterator<Item = OsString> {
    std::env::args_os().filter(|a| !a.to_string_lossy().starts_with("-psn_"))
}

fn main() {
    Args::parse_from(args());
    skiff_desktop_lib::run()
}

#[cfg(test)]
mod tests {
    use super::Args;
    use clap::{error::ErrorKind, Parser};

    fn parse(args: &[&str]) -> Result<Args, ErrorKind> {
        Args::try_parse_from(std::iter::once("skiff-desktop").chain(args.iter().copied())).map_err(|e| e.kind())
    }

    #[test]
    fn version_and_help_stop_before_the_app() {
        for a in ["--version", "-V"] {
            assert_eq!(parse(&[a]).err(), Some(ErrorKind::DisplayVersion), "{a}");
        }
        for a in ["--help", "-h"] {
            assert_eq!(parse(&[a]).err(), Some(ErrorKind::DisplayHelp), "{a}");
        }
    }

    #[test]
    fn an_unknown_flag_is_an_error() {
        assert_eq!(parse(&["--bogus"]).err(), Some(ErrorKind::UnknownArgument));
    }

    #[test]
    fn workspace_files_still_open() {
        assert!(parse(&[]).is_ok());
        assert!(parse(&["/home/me/work.skiffw"]).is_ok());
        assert!(parse(&["file:///home/me/my%20work.skiffw", "b.skiffw"]).is_ok());
    }
}
