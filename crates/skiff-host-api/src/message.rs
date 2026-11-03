use serde::{Deserialize, Serialize};

use crate::wire::bytes;
use crate::{Capability, EchoTimes, Error, Request, Response, SessionUpdate, TerminalFrame};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub id: u64,
    pub request: Request,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Boxed: an inventory row would otherwise size every event. Same JSON.
    SessionUpdate {
        update: Box<SessionUpdate>,
    },
    TerminalSnapshot {
        session: String,
        #[serde(with = "bytes")]
        data: Vec<u8>,
        cols: u16,
        rows: u16,
    },
    TerminalOutput {
        session: String,
        #[serde(with = "bytes")]
        data: Vec<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        echo: Option<EchoTimes>,
    },
}
impl Event {
    /// A terminal event's session and frame; None for any other family.
    pub fn into_terminal(self) -> Option<(String, TerminalFrame)> {
        match self {
            Self::TerminalSnapshot {
                session,
                data,
                cols,
                rows,
            } => Some((session, TerminalFrame::Snapshot { data, cols, rows })),
            Self::TerminalOutput {
                session,
                data,
                echo,
            } => Some((session, TerminalFrame::Output { data, echo })),
            Self::SessionUpdate { .. } => None,
        }
    }
    /// The capability whose negotiation makes this event family meaningful.
    pub fn capability(&self) -> Capability {
        match self {
            Self::SessionUpdate { .. } => Capability::session_updates(),
            Self::TerminalSnapshot { .. } | Self::TerminalOutput { .. } => {
                Capability::session_terminal()
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Reply {
        id: u64,
        result: Result<Response, Error>,
    },
    Event {
        event: Event,
    },
}
