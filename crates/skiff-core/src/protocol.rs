//! Newline-delimited JSON over a Unix socket.
//!
//! Client → daemon: one [`Envelope`] per line.
//! Daemon → client: one [`ServerMessage`] per line, either a [`Reply`] that
//! carries the request `id`, or an [`Event`] with no `id`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::FolderInfo;
use crate::group::Group;
use crate::project::{AgentInfo, Project, Worktree};
use crate::session::{SessionId, SessionInfo, SessionSpec, SessionState};

/// Base64 for byte payloads so the wire stays valid UTF-8 JSON.
pub mod b64 {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        STANDARD.encode(v).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    ListSessions,
    CreateSession {
        spec: SessionSpec,
    },
    Write {
        session: SessionId,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
    Resize {
        session: SessionId,
        cols: u16,
        rows: u16,
    },
    /// Start receiving output for this session on this connection. The daemon
    /// sends one [`Event::Snapshot`] first, then [`Event::Output`] from there on.
    Subscribe {
        session: SessionId,
    },
    Unsubscribe {
        session: SessionId,
    },
    /// Sets the user's name for a session. Empty clears it.
    RenameSession {
        session: SessionId,
        name: String,
    },
    /// Sets one terminal's theme. `null` returns it to the project's.
    SetSessionTheme {
        session: SessionId,
        #[serde(default)]
        theme: Option<String>,
    },
    /// Terminate the process and drop the session.
    Kill {
        session: SessionId,
    },
    /// Rereads `projects.toml` and each project's worktrees.
    ListProjects,
    /// New branch in `<parent>/<dir>-worktrees/<branch>`, from `base` or HEAD.
    AddWorktree {
        project: String,
        branch: String,
        #[serde(default)]
        base: Option<String>,
    },
    /// Appends the git repository that contains `path` to `projects.toml`.
    /// `name` defaults to the directory name.
    AddProject {
        path: PathBuf,
        #[serde(default)]
        name: Option<String>,
        /// Rail label. Derived from `name` when absent.
        #[serde(default)]
        short: Option<String>,
        /// A free palette color when absent.
        #[serde(default)]
        color: Option<String>,
        /// Absent: detect. `""`: no icon. A path: relative to the repo or absolute.
        #[serde(default)]
        icon: Option<String>,
        #[serde(default)]
        agents: Vec<String>,
    },
    /// Key overrides from `[keys]`.
    GetKeys,
    /// The app theme from `[appearance]`.
    GetAppearance,
    /// Sets the app theme: a theme id, or `null` for Harbor.
    SetAppearance {
        #[serde(default)]
        theme: Option<String>,
    },
    /// Every terminal theme: built in, foot's, and theme files.
    ListThemes,
    /// A small image as a data URL, for previews.
    ReadIcon {
        path: PathBuf,
    },
    /// Sets the rail icon: a path, `""` for none, or `null` to detect again.
    SetProjectIcon {
        project: String,
        #[serde(default)]
        icon: Option<String>,
    },
    /// What `add_project` would do for `path`. Writes nothing.
    InspectFolder {
        path: PathBuf,
    },
    /// The known agent CLIs, installed and enabled.
    ListAgents,
    /// Saves which agents the + menu offers.
    SetAgents {
        enabled: Vec<String>,
    },
    /// Replaces an agent's command line. Empty restores the default.
    SetAgentCommand {
        agent: String,
        command: String,
    },
    /// Refuses the main worktree, uncommitted changes, and live sessions inside.
    RemoveWorktree {
        project: String,
        path: PathBuf,
    },
    ListGroups,
    /// Empty `group.id` creates a group. Otherwise replaces that group.
    SaveGroup {
        group: Group,
    },
    /// `group` is the group id.
    DeleteGroup {
        group: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Envelope {
    pub id: u64,
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Pong {
        version: String,
        /// 0 from daemons that predate the field.
        #[serde(default)]
        protocol: u32,
    },
    Sessions { sessions: Vec<SessionInfo> },
    Session { session: SessionInfo },
    Projects { projects: Vec<Project> },
    Worktree { worktree: Worktree },
    Project { project: Project },
    Folder { folder: FolderInfo },
    Agents { agents: Vec<AgentInfo> },
    Icon { icon: Option<String> },
    Themes { themes: Vec<crate::theme::TerminalTheme> },
    Appearance { theme: Option<String> },
    Keys { keys: std::collections::BTreeMap<String, String> },
    Groups { groups: Vec<Group> },
    Group { group: Group },
    Error { message: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Reply {
    pub id: u64,
    #[serde(flatten)]
    pub response: Response,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Output {
        session: SessionId,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
    /// Bytes that redraw the current screen and scrollback. Starts with a
    /// full reset. Sized for the session's current `cols` x `rows`.
    Snapshot {
        session: SessionId,
        #[serde(with = "b64")]
        data: Vec<u8>,
        cols: u16,
        rows: u16,
    },
    State {
        session: SessionId,
        state: SessionState,
    },
    /// The program set or cleared its terminal title.
    Title {
        session: SessionId,
        title: Option<String>,
    },
    Exit {
        session: SessionId,
        code: Option<i32>,
    },
    SessionCreated {
        session: SessionInfo,
    },
    /// A session's info changed outside the state and title events.
    SessionUpdated {
        session: SessionInfo,
    },
    SessionRemoved {
        session: SessionId,
    },
    /// A worktree was added or removed. Clients reload projects.
    ProjectsChanged {},
    /// A group was saved, deleted, or lost a killed session. Clients reload groups.
    GroupsChanged {},
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ServerMessage {
    Reply(Reply),
    Event(Event),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_and_event_round_trip() {
        let reply = ServerMessage::Reply(Reply {
            id: 7,
            response: Response::Pong {
                version: "0.1.0".into(),
                protocol: 2,
            },
        });
        let json = serde_json::to_string(&reply).unwrap();
        assert!(json.contains("\"id\":7") && json.contains("\"type\":\"pong\""));
        match serde_json::from_str::<ServerMessage>(&json).unwrap() {
            ServerMessage::Reply(r) => assert_eq!(r.id, 7),
            other => panic!("expected reply, got {other:?}"),
        }

        let event = ServerMessage::Event(Event::Output {
            session: "abc".into(),
            data: b"hi\x07".to_vec(),
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"output\""));
        match serde_json::from_str::<ServerMessage>(&json).unwrap() {
            ServerMessage::Event(Event::Output { session, data }) => {
                assert_eq!(session, "abc");
                assert_eq!(data, b"hi\x07");
            }
            other => panic!("expected output event, got {other:?}"),
        }
    }

    #[test]
    fn request_envelope_parses() {
        let line = r#"{"id":1,"cmd":"write","session":"s1","data":"aGk="}"#;
        let env: Envelope = serde_json::from_str(line).unwrap();
        match env.request {
            Request::Write { session, data } => {
                assert_eq!(session, "s1");
                assert_eq!(data, b"hi");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
