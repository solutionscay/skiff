use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Capability, HostId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Error {
    UnsupportedCapability {
        capability: Capability,
    },
    IncompatibleVersion {
        supported: u32,
        received: u32,
    },
    MalformedContract {
        message: String,
    },
    /// Legacy service rejection without a structured cause; never guess a
    /// not-found/permission code from message text.
    OperationFailed {
        message: String,
    },
    NotFound {
        message: String,
    },
    /// A value the request carries is not one the Host takes.
    Invalid {
        message: String,
    },
    Conflict {
        message: String,
    },
    Permission {
        message: String,
    },
    HostBusy,
    TransportLost,
    TimedOut,
    IdentityChanged {
        expected: HostId,
        received: HostId,
    },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
