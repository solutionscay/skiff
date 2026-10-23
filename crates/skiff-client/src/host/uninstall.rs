//! Remove and uninstall: stop and disable Skiff's services on a server and
//! delete the managed install, after the person confirms it in Settings ›
//! Hosts. Only what Skiff put there goes: the managed folder, Skiff's unit
//! files and the `~/.local/bin/skiff` link when it is Skiff's. Projects,
//! skiffd's session data and `local-host-id` stay. A skiffd that Skiff did
//! not install is never stopped: `server.sh` stops a unit or a pid only
//! when it runs from the managed folder.

use serde::Serialize;

use super::{
    check::quote,
    install::{plain_version, run, value, SERVER},
    ssh::{SshOptions, SshTarget},
};

/// What an uninstall would end, for the Remove confirm.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Preview {
    /// Skiff installed Skiff there. False: there is nothing to uninstall.
    pub managed: bool,
    /// The shown names of the managed skiffd's sessions. They end.
    pub sessions: Vec<String>,
}

/// Reads whether the install is Skiff's and which sessions it runs.
pub async fn preview(target: &SshTarget, options: &SshOptions, socket_name: &str) -> Result<Preview, String> {
    if !plain_version(socket_name) {
        return Err("This socket name has a form Skiff does not send.".into());
    }
    let out = run(target, options, format!("sh -s -- sessions {}", quote(socket_name)), SERVER.as_bytes()).await?;
    if value(&out, "skiff-sessions") != "done" {
        return Err("Skiff cannot read the state of the server. Check again.".into());
    }
    Ok(Preview { managed: value(&out, "managed") == "yes", sessions: names(value(&out, "sessions")) })
}

/// The shown name of each session in `skiff ls --json`: its name, its
/// title, or its label, as the CLI shows it.
fn names(json: &str) -> Vec<String> {
    let all: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    all.iter()
        .map(|s| {
            ["name", "title", "label", "id"]
                .iter()
                .filter_map(|k| s.get(k).and_then(|v| v.as_str()))
                .find(|n| !n.is_empty())
                .unwrap_or("?")
                .to_string()
        })
        .collect()
}

/// Stops and disables Skiff's services on the server and deletes the
/// managed files. `name`: the Host's shown name, for the error copy.
pub async fn uninstall(target: &SshTarget, options: &SshOptions, name: &str) -> Result<(), String> {
    let out = run(target, options, "sh -s -- uninstall".into(), SERVER.as_bytes()).await?;
    match value(&out, "skiff-uninstall") {
        "done" => Ok(()),
        "unmanaged" => Err(format!("Skiff did not install the Skiff on {name}, so it does not uninstall it.")),
        _ => Err(format!("The uninstall on {name} did not finish. Try again.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_names_read_as_the_cli_shows_them() {
        let json = r#"[{"id":"a1","label":"shell","name":"build"},{"id":"b2","label":"claude","title":"fix tests"},{"id":"c3","label":"shell"}]"#;
        assert_eq!(names(json), ["build", "fix tests", "shell"]);
        assert!(names("").is_empty());
        assert!(names("not json").is_empty());
    }
}
