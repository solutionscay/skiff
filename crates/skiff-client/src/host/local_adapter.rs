//! The in-process local adapter: `LocalSources` and the constructors that
//! build a local `HostConnection`.
use std::sync::Arc;

use skiff_host_api::{Error, Hello};
use tokio::sync::watch;

use super::{
    engine::MAX_PENDING, AgentSource, Backend, GroupSource, HostConnection, InventorySource,
    LifecycleSource, ScreenSource, StatusSource, TerminalSource, UpdateSource,
};

/// The in-process sources one local adapter offers. Each optional source
/// lets the adapter advertise exactly its own capability.
pub struct LocalSources {
    pub status: Arc<dyn StatusSource>,
    pub screen: Option<Arc<dyn ScreenSource>>,
    pub inventory: Option<Arc<dyn InventorySource>>,
    pub updates: Option<Arc<dyn UpdateSource>>,
    pub terminal: Option<Arc<dyn TerminalSource>>,
    pub lifecycle: Option<Arc<dyn LifecycleSource>>,
    pub groups: Option<Arc<dyn GroupSource>>,
    pub agents: Option<Arc<dyn AgentSource>>,
}

impl HostConnection {
    /// Bootstrap an in-process local status source. It is never a wire Hello
    /// from legacy skiffd, and cannot claim the session-plane capability.
    pub fn local_status(
        source: Arc<dyn StatusSource>,
        hello: Hello,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        Self::local(
            LocalSources {
                status: source,
                screen: None,
                inventory: None,
                updates: None,
                terminal: None,
                lifecycle: None,
                groups: None,
                agents: None,
            },
            hello,
            previous,
        )
    }

    /// M1b adds only session_screen; broad sessions@1 stays unavailable locally.
    pub fn local_screen(
        source: Arc<dyn StatusSource>,
        screen: Arc<dyn ScreenSource>,
        hello: Hello,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        Self::local(
            LocalSources {
                status: source,
                screen: Some(screen),
                inventory: None,
                updates: None,
                terminal: None,
                lifecycle: None,
                groups: None,
                agents: None,
            },
            hello,
            previous,
        )
    }

    /// M1c adds only the lossless read-only inventory, not sessions@1.
    pub fn local_inventory(
        source: Arc<dyn StatusSource>,
        screen: Arc<dyn ScreenSource>,
        inventory: Arc<dyn InventorySource>,
        hello: Hello,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        Self::local(
            LocalSources {
                status: source,
                screen: Some(screen),
                inventory: Some(inventory),
                updates: None,
                terminal: None,
                lifecycle: None,
                groups: None,
                agents: None,
            },
            hello,
            previous,
        )
    }

    /// M1d adds only the read-only session-update stream; no output, input,
    /// subscription request or lifecycle operation.
    pub fn local_updates(
        source: Arc<dyn StatusSource>,
        screen: Arc<dyn ScreenSource>,
        inventory: Arc<dyn InventorySource>,
        updates: Arc<dyn UpdateSource>,
        hello: Hello,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        Self::local(
            LocalSources {
                status: source,
                screen: Some(screen),
                inventory: Some(inventory),
                updates: Some(updates),
                terminal: None,
                lifecycle: None,
                groups: None,
                agents: None,
            },
            hello,
            previous,
        )
    }

    /// An in-process local adapter. It may advertise only the capabilities
    /// its sources serve; M1e adds `session_terminal@1` with a TerminalSource,
    /// M1g `session_lifecycle@1` and `session_groups@1`, M3 `session_agents@1`,
    /// and `program_status@1` with the inventory, when skiffd reads OSC 7501.
    /// Never `sessions@1`.
    pub fn local(
        sources: LocalSources,
        hello: Hello,
        previous: Option<&Hello>,
    ) -> Result<Self, Error> {
        let LocalSources {
            status: source,
            screen,
            inventory,
            updates,
            terminal,
            lifecycle,
            groups,
            agents,
        } = sources;
        hello.validate()?;
        if hello.capabilities.iter().any(|c| {
            c.name != skiff_host_api::CONNECTION_STATUS
                && !(screen.is_some() && c.name == skiff_host_api::SESSION_SCREEN)
                && !(inventory.is_some() && c.name == skiff_host_api::SESSION_INVENTORY)
                && !(updates.is_some() && c.name == skiff_host_api::SESSION_UPDATES)
                && !(terminal.is_some() && c.name == skiff_host_api::SESSION_TERMINAL)
                && !(lifecycle.is_some() && c.name == skiff_host_api::SESSION_LIFECYCLE)
                && !(groups.is_some() && c.name == skiff_host_api::SESSION_GROUPS)
                && !(agents.is_some() && c.name == skiff_host_api::SESSION_AGENTS)
                // A field of the inventory and its updates, from the same source.
                && !(inventory.is_some() && c.name == skiff_host_api::PROGRAM_STATUS)
        }) {
            return Err(Error::MalformedContract {
                message: "local adapter cannot advertise capabilities without a source".into(),
            });
        }
        let limit = hello.limits.max_in_flight.min(MAX_PENDING);
        let slots = tokio::sync::Semaphore::new(limit);
        // Reserve one slot for status while Mission control fans out reads.
        let read_slots = tokio::sync::Semaphore::new(limit.saturating_sub(1).max(1));
        Self::negotiated(
            Backend::Local {
                source,
                screen,
                inventory,
                updates,
                terminal,
                lifecycle,
                groups,
                agents,
                read_slots,
                terminal_slots: tokio::sync::Semaphore::new(limit),
                closed: watch::channel(false).0,
                slots,
            },
            hello,
            previous,
        )
    }
}
