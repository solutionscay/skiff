use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Capability, Error, Host, InstanceId};

pub const API_MAJOR: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub max_frame_bytes: usize,
    pub max_in_flight: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    #[serde(flatten)]
    pub host: Host,
    pub instance_id: InstanceId,
    pub service_versions: BTreeMap<String, String>,
    pub api_major: u32,
    pub capabilities: Vec<Capability>,
    pub limits: Limits,
}
impl Hello {
    pub fn validate(&self) -> Result<(), Error> {
        if self.api_major != API_MAJOR {
            return Err(Error::IncompatibleVersion {
                supported: API_MAJOR,
                received: self.api_major,
            });
        }
        if self.host.os.is_empty()
            || self.host.account.is_empty()
            || self.limits.max_frame_bytes == 0
            || self.limits.max_in_flight == 0
            || self
                .capabilities
                .iter()
                .any(|c| c.name.is_empty() || c.version == 0)
            || self
                .capabilities
                .iter()
                .enumerate()
                .any(|(i, c)| self.capabilities[..i].contains(c))
        {
            return Err(Error::MalformedContract {
                message: "invalid Hello metadata or limits".into(),
            });
        }
        Ok(())
    }

    pub fn require(&self, capability: &Capability) -> Result<(), Error> {
        if self.capabilities.contains(capability) {
            Ok(())
        } else {
            Err(Error::UnsupportedCapability {
                capability: capability.clone(),
            })
        }
    }
}
