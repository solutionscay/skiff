use crate::{Capability, Request, Response};

impl Request {
    pub fn capability(&self) -> Option<Capability> {
        match self {
            Self::Hello => None,
            Self::ConnectionStatus => Some(Capability::connection_status()),
            Self::SessionInventory => Some(Capability::session_inventory()),
            Self::SessionScreen { .. } => Some(Capability::session_screen()),
            Self::TerminalAttach { .. }
            | Self::TerminalDetach { .. }
            | Self::TerminalResize { .. }
            | Self::TerminalInput { .. } => Some(Capability::session_terminal()),
            Self::SessionCreate { .. }
            | Self::SessionKill { .. }
            | Self::SessionRename { .. }
            | Self::SessionSetTheme { .. }
            | Self::SessionSeen { .. } => Some(Capability::session_lifecycle()),
            Self::GroupList | Self::GroupSave { .. } | Self::GroupDelete { .. } | Self::GroupOrder { .. } => {
                Some(Capability::session_groups())
            }
            Self::AgentList => Some(Capability::session_agents()),
            Self::ProjectList | Self::ProjectGet { .. } => Some(Capability::projects()),
            Self::ProjectsStamp => Some(Capability::projects_watch()),
            Self::GitChanges { .. } => Some(Capability::git_status()),
            Self::GitDiff { .. } => Some(Capability::git_diff()),
            Self::FileList { .. } | Self::FileRead { .. } => Some(Capability::files()),
            Self::ProjectBackground { .. } => Some(Capability::project_assets()),
            Self::FolderHome | Self::FolderList { .. } | Self::FolderPlaces | Self::FolderCreate { .. } => Some(Capability::folders()),
            Self::DropPut { .. } => Some(Capability::drops()),
            Self::LedgerRows | Self::LedgerStamp | Self::LedgerResult { .. } => Some(Capability::ledger_read()),
            Self::SkipperSettings => Some(Capability::skipper_settings()),
            Self::SkipperEdit { .. } | Self::SkipperFolderPreview { .. } | Self::SkipperFolderChange { .. } => {
                Some(Capability::skipper_config())
            }
            Self::SkipperFileRead { .. } | Self::SkipperFileWrite { .. } | Self::SkipperFileReset { .. } => Some(Capability::skipper_file()),
            Self::SkipperOpen { .. } | Self::SkipperWatch { .. } | Self::SkipperProject { .. } => Some(Capability::skipper_run()),
            Self::SkipperWatchLog => Some(Capability::skipper_watch_log()),
            Self::SkipperFleet { .. } | Self::FleetTakeoverPreview { .. } | Self::FleetTakeover { .. } | Self::FleetRename { .. } => {
                Some(Capability::skipper_run_fleets())
            }
            Self::AgentConfig | Self::AgentEdit { .. } => Some(Capability::agent_config()),
            Self::ProjectInspect { .. }
            | Self::ProjectInit { .. }
            | Self::ProjectAdd { .. }
            | Self::ProjectEdit { .. }
            | Self::ProjectArrange { .. }
            | Self::ProjectRemove { .. } => Some(Capability::project_config()),
            Self::WorktreeAdd { .. } | Self::WorktreeRemove { .. } | Self::BranchList { .. } => {
                Some(Capability::worktrees())
            }
        }
    }
    pub fn accepts(&self, reply: &Response) -> bool {
        matches!(
            (self, reply),
            (Self::Hello, Response::Hello { .. })
                | (Self::ConnectionStatus, Response::ConnectionStatus { .. })
                | (Self::SessionScreen { .. }, Response::SessionScreen { .. })
                | (Self::SessionInventory, Response::SessionInventory { .. })
                | (
                    Self::SessionCreate { .. }
                        | Self::SessionRename { .. }
                        | Self::SessionSetTheme { .. },
                    Response::Session { .. }
                )
                | (Self::GroupList, Response::Groups { .. })
                | (Self::GroupSave { .. }, Response::Group { .. })
                | (Self::AgentList, Response::Agents { .. })
                | (Self::ProjectList, Response::Projects { .. })
                | (Self::ProjectGet { .. }, Response::Project { .. })
                | (Self::GitChanges { .. }, Response::Changes { .. })
                | (Self::GitDiff { .. }, Response::Diff { .. })
                | (Self::ProjectInspect { .. } | Self::ProjectInit { .. }, Response::Folder { .. })
                | (Self::ProjectAdd { .. }, Response::ProjectAdded { .. })
                | (
                    Self::ProjectEdit { .. }
                        | Self::ProjectArrange { .. }
                        | Self::ProjectRemove { .. },
                    Response::Revision { .. }
                )
                | (Self::WorktreeAdd { .. }, Response::Worktree { .. })
                | (Self::BranchList { .. }, Response::Branches { .. })
                | (Self::FileList { .. }, Response::Files { .. })
                | (Self::FileRead { .. }, Response::File { .. })
                | (Self::ProjectBackground { .. }, Response::Image { .. })
                | (Self::FolderHome | Self::FolderCreate { .. } | Self::DropPut { .. }, Response::Path { .. })
                | (Self::FolderList { .. }, Response::Folders { .. })
                | (Self::FolderPlaces, Response::Places { .. })
                | (Self::LedgerRows, Response::Ledger { .. })
                | (Self::LedgerStamp, Response::LedgerStamp { .. })
                | (Self::ProjectsStamp, Response::ProjectsStamp { .. })
                | (Self::LedgerResult { .. }, Response::File { .. })
                | (Self::SkipperSettings | Self::SkipperEdit { .. } | Self::SkipperFileReset { .. }, Response::Skipper { .. })
                | (Self::SkipperFileRead { .. } | Self::SkipperFileWrite { .. }, Response::SkipperFile { .. })
                | (Self::SkipperFolderPreview { .. }, Response::SkipperFolder { .. })
                | (Self::SkipperFolderChange { .. }, Response::SkipperFolderDone { .. })
                | (Self::SkipperOpen { .. }, Response::SkipperOpened { .. })
                | (Self::SkipperWatch { .. }, Response::Watch { .. })
                | (Self::SkipperWatchLog, Response::File { .. })
                | (Self::SkipperProject { .. } | Self::SkipperFleet { .. }, Response::Session { .. })
                | (Self::FleetTakeoverPreview { .. } | Self::FleetTakeover { .. }, Response::FleetTakeover { .. })
                | (Self::AgentConfig | Self::AgentEdit { .. }, Response::AgentConfig { .. })
        ) || (!matches!(
            self,
            Self::Hello
                | Self::ConnectionStatus
                | Self::SessionScreen { .. }
                | Self::SessionInventory
                | Self::SessionCreate { .. }
                | Self::SessionRename { .. }
                | Self::SessionSetTheme { .. }
                | Self::GroupList
                | Self::GroupSave { .. }
                | Self::AgentList
                | Self::ProjectList
                | Self::ProjectGet { .. }
                | Self::GitChanges { .. }
                | Self::GitDiff { .. }
                | Self::ProjectInspect { .. }
                | Self::ProjectInit { .. }
                | Self::ProjectAdd { .. }
                | Self::ProjectEdit { .. }
                | Self::ProjectArrange { .. }
                | Self::ProjectRemove { .. }
                | Self::WorktreeAdd { .. }
                | Self::BranchList { .. }
                | Self::FileList { .. }
                | Self::FileRead { .. }
                | Self::ProjectBackground { .. }
                | Self::FolderHome
                | Self::FolderList { .. }
                | Self::FolderPlaces
                | Self::FolderCreate { .. }
                | Self::DropPut { .. }
                | Self::LedgerRows
                | Self::LedgerStamp
                | Self::ProjectsStamp
                | Self::LedgerResult { .. }
                | Self::SkipperSettings
                | Self::SkipperEdit { .. }
                | Self::SkipperFileRead { .. }
                | Self::SkipperFileWrite { .. }
                | Self::SkipperFileReset { .. }
                | Self::SkipperFolderPreview { .. }
                | Self::SkipperFolderChange { .. }
                | Self::SkipperWatch { .. }
                | Self::SkipperWatchLog
                | Self::SkipperProject { .. }
                | Self::SkipperFleet { .. }
                | Self::FleetTakeoverPreview { .. }
                | Self::FleetTakeover { .. }
                | Self::AgentConfig
                | Self::AgentEdit { .. }
        ) && matches!(reply, Response::Ok))
    }
}
