//! What the session family tests share: the fixture Host's hello, and the
//! requests outside the session family.
#![allow(dead_code)]

use std::collections::BTreeMap;

use skiff_host_api::*;

/// A fixture Host `host`, instance `instance`, offering `capabilities`.
pub fn hello(host: &str, instance: &str, capabilities: Vec<Capability>) -> Hello {
    Hello {
        host: Host { host_id: HostId::try_from(host.to_owned()).unwrap(), os: "fixture".into(), account: "fixture".into() },
        instance_id: InstanceId::try_from(instance.to_owned()).unwrap(),
        service_versions: BTreeMap::new(),
        api_major: API_MAJOR,
        capabilities,
        limits: Limits { max_frame_bytes: 8 * 1024 * 1024, max_in_flight: 64 },
    }
}

/// The agent and terminal requests. A Host that offers only one session
/// capability refuses each of them.
pub fn agent_and_terminal_requests() -> [Request; 5] {
    [
        Request::AgentList,
        Request::TerminalInput { session: "s".into(), data: vec![], paste: false },
        Request::TerminalResize { session: "s".into(), cols: 1, rows: 1 },
        Request::TerminalAttach { session: "s".into() },
        Request::TerminalDetach { session: "s".into() },
    ]
}
