use serde::{Deserialize, Serialize};

macro_rules! identity {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if value.trim().is_empty() {
                    return Err(concat!(stringify!($name), " must not be empty").into());
                }
                Ok(Self(value))
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
identity!(HostId);
identity!(InstanceId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub host_id: HostId,
    pub os: String,
    pub account: String,
}

/// A locator, never identity or an instruction to launch a process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Endpoint {
    Local {
        socket: String,
    },
    /// Descriptor only; M0 has no SSH implementation.
    Ssh {
        hostname: String,
        account: String,
        port: u16,
        socket: String,
    },
}
impl Endpoint {
    /// Only a local endpoint may let the app start, signal, reload or link
    /// processes on its machine. Every other endpoint is reached, never managed.
    pub fn is_local(&self) -> bool {
        match self {
            Self::Local { .. } => true,
            Self::Ssh { .. } => false,
        }
    }
}
