use skiff_host_api::{
    AgentEdit, Error, FolderListing, FolderPlaces, HostBranches, HostCacheImage,
    HostCacheUse, HostAgent, HostChanges, HostDiff, HostFile, HostFolder,
    HostImage, HostLedgerRow, HostFileScope, HostListing, HostProject, HostCronPreview,
    HostSchedule, HostSkipper, HostSkipperFile, HostWorktree, ProjectEdit, ProjectFleet,
    ReadPurpose, Request, Response, SkipperEdit, SkipperFolderMoved, SkipperOpened, InventorySession, TakenSkipper, AgentUsage, UsageRefresh,
};

use crate::host::HostConnection;

/// Every project of the Host, open and closed, in its config order.
pub async fn project_list(c: &HostConnection) -> Result<Vec<HostProject>, Error> {
    Ok(project_rows(c).await?.0)
}

/// [`project_list`], and the revision a `project_config` change names.
/// `None` from a Host that predates it.
pub async fn project_rows(c: &HostConnection) -> Result<(Vec<HostProject>, Option<String>), Error> {
    match c.request(Request::ProjectList).await? {
        Response::Projects { projects, revision } => Ok((projects, revision)),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// One project read again.
pub async fn project_get(c: &HostConnection, name: &str) -> Result<HostProject, Error> {
    match c.request(Request::ProjectGet { name: name.into() }).await? {
        Response::Project { project } => Ok(*project),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The changed files of `worktree`, a path on the Host, and the branch they
/// count from.
pub async fn git_changes(c: &HostConnection, worktree: &str) -> Result<HostChanges, Error> {
    match c.request(Request::GitChanges { worktree: worktree.into() }).await? {
        Response::Changes { changes, base } => Ok(HostChanges { base, changes }),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// One changed file's diff text in `worktree`, or the whole worktree's.
/// Both are paths on the Host.
pub async fn git_diff(c: &HostConnection, worktree: &str, file: Option<&str>) -> Result<HostDiff, Error> {
    match c.request(Request::GitDiff { worktree: worktree.into(), file: file.map(Into::into) }).await? {
        Response::Diff { diff } => Ok(diff),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The folder `dir` (relative; empty for the worktree itself) of `worktree`.
/// `follow_links`: This Mac only; a remote Host never gets it.
pub async fn file_list(c: &HostConnection, worktree: &str, dir: &str, follow_links: bool) -> Result<HostListing, Error> {
    file_list_page(c, worktree, dir, follow_links, None).await
}

/// A page after `cursor`, which came from the preceding listing.
pub async fn file_list_page(c: &HostConnection, worktree: &str, dir: &str, follow_links: bool, cursor: Option<usize>) -> Result<HostListing, Error> {
    match c.request(Request::FileList { worktree: worktree.into(), dir: dir.into(), follow_links, cursor }).await? {
        Response::Files { listing } => Ok(listing),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Shows a known worktree folder or a known regular file in this Host's file
/// manager. No Host path is passed to this computer.
pub async fn file_reveal(c: &HostConnection, worktree: &str, file: Option<&str>) -> Result<(), Error> {
    match c.request(Request::FileReveal { worktree: worktree.into(), file: file.map(Into::into) }).await? {
        Response::Ok => Ok(()),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// `file` (relative) of `worktree`, read on the Host for `purpose`.
pub async fn file_read(c: &HostConnection, worktree: &str, file: &str, purpose: ReadPurpose) -> Result<HostFile, Error> {
    match c.request(Request::FileRead { worktree: worktree.into(), file: file.into(), purpose }).await? {
        Response::File { file } => Ok(*file),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The project's background image, as bytes the Host checked.
pub async fn project_background(c: &HostConnection, project: &str) -> Result<HostImage, Error> {
    match c.request(Request::ProjectBackground { project: project.into() }).await? {
        Response::Image { image } => Ok(image),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The Host's Images library, newest first.
pub async fn image_list(c: &HostConnection) -> Result<Vec<HostCacheImage>, Error> {
    match c.request(Request::ImageList).await? {
        Response::Images { images } => Ok(images),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// One Host cache image's bytes. `path` came from [`image_list`].
pub async fn image_read(c: &HostConnection, path: &str) -> Result<HostImage, Error> {
    image_get(c, path, None).await
}

/// One Host cache image scaled on the Host to at most `size` pixels on its
/// long side. An older Host answers the full image.
pub async fn image_thumb(c: &HostConnection, path: &str, size: u32) -> Result<HostImage, Error> {
    image_get(c, path, Some(size)).await
}

async fn image_get(c: &HostConnection, path: &str, thumb: Option<u32>) -> Result<HostImage, Error> {
    match c.request(Request::ImageRead { path: path.into(), thumb, offset: None, length: None }).await? {
        Response::Image { image } => Ok(image),
        _ => unreachable!("checked by Request::accepts"),
    }
}

pub async fn image_delete(c: &HostConnection, paths: Vec<String>) -> Result<(), Error> {
    match c.request(Request::ImageDelete { paths }).await? {
        Response::Ok => Ok(()),
        _ => unreachable!("checked by Request::accepts"),
    }
}

pub async fn image_pin(c: &HostConnection, paths: Vec<String>, on: bool) -> Result<(), Error> {
    match c.request(Request::ImagePin { paths, on }).await? {
        Response::Ok => Ok(()),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The total cache and image sizes of this Host.
pub async fn cache_use(c: &HostConnection) -> Result<HostCacheUse, Error> {
    match c.request(Request::CacheUse).await? {
        Response::CacheUse { usage } => Ok(usage),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The Host's Ledger panel rows, oldest first (`ledger@1`).
pub async fn ledger_rows(c: &HostConnection) -> Result<Vec<HostLedgerRow>, Error> {
    match c.request(Request::LedgerRows).await? {
        Response::Ledger { rows } => Ok(rows),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// A value that changes when the Host's ledger does.
pub async fn ledger_stamp(c: &HostConnection) -> Result<String, Error> {
    match c.request(Request::LedgerStamp).await? {
        Response::LedgerStamp { stamp } => Ok(stamp),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Every schedule with its newest runs (`schedules@2`).
pub async fn schedule_list(c: &HostConnection) -> Result<Vec<HostSchedule>, Error> {
    match c.request(Request::ScheduleList).await? {
        Response::Schedules { schedules } => Ok(schedules),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// A cron line checked on the Host: its next runs, or its error line.
pub async fn cron_preview(c: &HostConnection, cron: &str) -> Result<HostCronPreview, Error> {
    match c.request(Request::CronPreview { cron: cron.into() }).await? {
        Response::CronPreview { preview } => Ok(preview),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// `skiff schedule ARGS` on the Host. A refusal is `invalid` with its line.
pub async fn schedule_call(c: &HostConnection, args: Vec<String>) -> Result<(), Error> {
    c.request(Request::ScheduleCall { args }).await.map(|_| ())
}

/// A value that changes with the Host's `projects.toml` (`projects@2`).
pub async fn projects_stamp(c: &HostConnection) -> Result<String, Error> {
    match c.request(Request::ProjectsStamp).await? {
        Response::ProjectsStamp { stamp } => Ok(stamp),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The result of the row `uid`, read on the Host as a bounded preview. The
/// Host reads the path its row records; this side never names one.
pub async fn ledger_result(c: &HostConnection, uid: &str) -> Result<HostFile, Error> {
    match c.request(Request::LedgerResult { uid: uid.into() }).await? {
        Response::File { file } => Ok(*file),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The Host's `[skiff_agent]` settings and task types (`skipper@1`).
pub async fn skipper_settings(c: &HostConnection) -> Result<HostSkipper, Error> {
    match c.request(Request::SkipperSettings).await? {
        Response::Skipper { settings } => Ok(*settings),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Skipper's watcher log, chosen and read by the Host as a bounded preview.
/// A client supplies no path and never opens a Host path locally.
pub async fn skipper_watch_log(c: &HostConnection) -> Result<HostFile, Error> {
    match c.request(Request::SkipperWatchLog).await? {
        Response::File { file } => Ok(*file),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// An instruction file on the Host and its revision (`skipper_file@1`). A
/// Skipper's missing file is written from the template there first.
pub async fn skipper_file_read(c: &HostConnection, scope: HostFileScope) -> Result<HostSkipperFile, Error> {
    match c.request(Request::SkipperFileRead { scope }).await? {
        Response::SkipperFile { file } => Ok(file),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Sends a saved copy back: refused (`conflict`) when the Host's file
/// changed since `revision`. Answers the new revision.
pub async fn skipper_file_write(c: &HostConnection, scope: HostFileScope, text: String, revision: String) -> Result<HostSkipperFile, Error> {
    match c.request(Request::SkipperFileWrite { scope, text, revision }).await? {
        Response::SkipperFile { file } => Ok(file),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Reset… on the Host: the template again, the old file in `<name>.bak`.
pub async fn skipper_file_reset(c: &HostConnection, scope: HostFileScope) -> Result<(), Error> {
    c.request(Request::SkipperFileReset { scope }).await.map(drop)
}


/// The Host's agents and the `[agents]` revision they were read at (`agent_config@1`).
pub async fn agent_config(c: &HostConnection) -> Result<(Vec<HostAgent>, String), Error> {
    agents_of(c.request(Request::AgentConfig).await?)
}

fn agents_of(reply: Response) -> Result<(Vec<HostAgent>, String), Error> {
    match reply {
        Response::AgentConfig { agents, revision } => Ok((agents, revision)),
        _ => unreachable!("checked by Request::accepts"),
    }
}

// Changes. Each is sent once: a lost or late reply is reported, never
// retried, since the Host may have made the change. A project change names
// the revision it was made from, so a stale or repeated one is refused.

/// What the Host would add for `path`, a folder on the Host.
pub async fn project_inspect(c: &HostConnection, path: &str) -> Result<HostFolder, Error> {
    match c.request(Request::ProjectInspect { path: path.into() }).await? {
        Response::Folder { folder } => Ok(folder),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Runs `git init` in `path` on the Host, and inspects it again.
pub async fn project_init(c: &HostConnection, path: &str) -> Result<HostFolder, Error> {
    match c.request(Request::ProjectInit { path: path.into() }).await? {
        Response::Folder { folder } => Ok(folder),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The Host account's home folder (`folders@1`).
pub async fn folder_home(c: &HostConnection) -> Result<String, Error> {
    match c.request(Request::FolderHome).await? {
        Response::Path { path } => Ok(path),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The folders in `path` on the Host (`folders@1`).
pub async fn folder_list(c: &HostConnection, path: &str, hidden: bool, filter: Option<String>) -> Result<FolderListing, Error> {
    match c.request(Request::FolderList { path: path.into(), hidden, filter }).await? {
        Response::Folders { listing } => Ok(listing),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The programs in `path` on the Host (`agent_programs@1`).
pub async fn program_list(c: &HostConnection, path: &str) -> Result<Vec<String>, Error> {
    match c.request(Request::ProgramList { path: path.into() }).await? {
        Response::Programs { names } => Ok(names),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Each agent's last plan usage on the Host, after starting the reads
/// `refresh` asks for (`agent_usage@1`). It never waits for a read.
pub async fn usage_read(c: &HostConnection, refresh: UsageRefresh) -> Result<Vec<AgentUsage>, Error> {
    match c.request(Request::UsageRead { refresh }).await? {
        Response::Usage { agents } => Ok(agents),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Recent and the Host's git repositories under home (`folders@1`).
pub async fn folder_places(c: &HostConnection) -> Result<FolderPlaces, Error> {
    match c.request(Request::FolderPlaces).await? {
        Response::Places { places } => Ok(places),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Makes the folder `name` in `parent` on the Host. Answers its path.
pub async fn folder_create(c: &HostConnection, parent: &str, name: &str) -> Result<String, Error> {
    match c.request(Request::FolderCreate { parent: parent.into(), name: name.into() }).await? {
        Response::Path { path } => Ok(path),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Adds the repository at `path` on the Host. Answers the row and the new revision.
pub async fn project_add(
    c: &HostConnection,
    revision: &str,
    path: &str,
    name: Option<String>,
    short: Option<String>,
    color: Option<String>,
) -> Result<(HostProject, String), Error> {
    let request = Request::ProjectAdd { path: path.into(), revision: revision.into(), name, short, color };
    match c.request(request).await? {
        Response::ProjectAdded { project, revision } => Ok((*project, revision)),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// One key of one project. Answers the new revision.
pub async fn project_edit(c: &HostConnection, revision: &str, project: &str, edit: ProjectEdit) -> Result<String, Error> {
    revised(c.request(Request::ProjectEdit { project: project.into(), revision: revision.into(), edit }).await?)
}

/// The order and the fleets given, in one write. Answers the new revision.
pub async fn project_arrange(
    c: &HostConnection,
    revision: &str,
    order: Vec<String>,
    fleets: Vec<ProjectFleet>,
) -> Result<String, Error> {
    revised(c.request(Request::ProjectArrange { revision: revision.into(), order, fleets }).await?)
}

/// Drops one project's table. Answers the new revision.
pub async fn project_remove(c: &HostConnection, revision: &str, project: &str) -> Result<String, Error> {
    revised(c.request(Request::ProjectRemove { project: project.into(), revision: revision.into() }).await?)
}

fn revised(reply: Response) -> Result<String, Error> {
    match reply {
        Response::Revision { revision } => Ok(revision),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// A worktree on the new `branch` of a Host project, or, with `checkout`,
/// on an existing branch that has none.
pub async fn worktree_add(c: &HostConnection, project: &str, branch: &str, base: Option<&str>, checkout: bool) -> Result<HostWorktree, Error> {
    let request = Request::WorktreeAdd { project: project.into(), branch: branch.into(), base: base.map(Into::into), checkout };
    match c.request(request).await? {
        Response::Worktree { worktree } => Ok(worktree),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// A Host project's branches, and the one its folder has checked out.
pub async fn branch_list(c: &HostConnection, project: &str) -> Result<HostBranches, Error> {
    match c.request(Request::BranchList { project: project.into() }).await? {
        Response::Branches { branches } => Ok(branches),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Removes a linked worktree, `path` on the Host, as its Host allows.
pub async fn worktree_remove(c: &HostConnection, project: &str, path: &str, force: bool) -> Result<(), Error> {
    c.request(Request::WorktreeRemove { project: project.into(), path: path.into(), force }).await.map(drop)
}

/// One Skipper setting on the Host. Answers the settings, read again.
pub async fn skipper_edit(c: &HostConnection, edit: SkipperEdit) -> Result<HostSkipper, Error> {
    match c.request(Request::SkipperEdit { edit }).await? {
        Response::Skipper { settings } => Ok(*settings),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Moves the agent's folder on the Host to `path`.
pub async fn skipper_folder_move(c: &HostConnection, path: &str) -> Result<SkipperFolderMoved, Error> {
    match c.request(Request::SkipperFolderMove { path: path.into() }).await? {
        Response::SkipperFolderMoved { moved } => Ok(moved),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Finds or starts Skipper's session on the Host, and its watcher. `None`:
/// a restore found no session to continue.
pub async fn skipper_open(c: &HostConnection, restore: Option<u64>) -> Result<Option<SkipperOpened>, Error> {
    match c.request(Request::SkipperOpen { restore }).await? {
        Response::SkipperOpened { opened } => Ok(Some(*opened)),
        Response::Ok => Ok(None),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Starts the Host's watcher for Skipper's session `session`. True when this started one.
pub async fn skipper_watch(c: &HostConnection, session: &str) -> Result<bool, Error> {
    match c.request(Request::SkipperWatch { session: session.into() }).await? {
        Response::Watch { started } => Ok(started),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Finds or starts the Host's project Skipper of `project`. Answers its session.
pub async fn skipper_project(c: &HostConnection, project: &str) -> Result<InventorySession, Error> {
    match c.request(Request::SkipperProject { project: project.into() }).await? {
        Response::Session { session } => Ok(*session),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Finds or starts the Host's fleet Skipper of `fleet`, taking the fleet's
/// project Skippers over. Answers its session.
pub async fn skipper_fleet(c: &HostConnection, fleet: &str) -> Result<InventorySession, Error> {
    match c.request(Request::SkipperFleet { fleet: fleet.into() }).await? {
        Response::Session { session } => Ok(*session),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The project Skippers a takeover of `fleet` would stop on the Host.
pub async fn fleet_takeover_preview(c: &HostConnection, fleet: &str, join: Option<&str>) -> Result<Vec<TakenSkipper>, Error> {
    match c.request(Request::FleetTakeoverPreview { fleet: fleet.into(), join: join.map(Into::into) }).await? {
        Response::FleetTakeover { taken } => Ok(taken),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// The takeover on the Host: `join` joins `fleet`, whose Skipper takes its
/// project Skipper over. Answers what it took over.
pub async fn fleet_takeover(c: &HostConnection, fleet: &str, join: &str) -> Result<Vec<TakenSkipper>, Error> {
    match c.request(Request::FleetTakeover { fleet: fleet.into(), join: join.into() }).await? {
        Response::FleetTakeover { taken } => Ok(taken),
        _ => unreachable!("checked by Request::accepts"),
    }
}

/// Renames a fleet on the Host, with its Skipper.
pub async fn fleet_rename(c: &HostConnection, old: &str, new: &str) -> Result<(), Error> {
    c.request(Request::FleetRename { old: old.into(), new: new.into() }).await.map(|_| ())
}

/// One change to the Host's `[agents]`, from `revision`. Answers the agents
/// and the new revision.
pub async fn agent_edit(c: &HostConnection, revision: &str, edit: AgentEdit) -> Result<(Vec<HostAgent>, String), Error> {
    agents_of(c.request(Request::AgentEdit { revision: revision.into(), edit }).await?)
}
