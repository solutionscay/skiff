use crate::Client;
use anyhow::{bail, Result};
use skiff_core::protocol::{Request, Response};

impl Client {
    pub async fn ping(&self) -> Result<String> {
        Ok(self.hello().await?.0)
    }

    /// Daemon version and protocol number.
    pub async fn hello(&self) -> Result<(String, u32)> {
        match self.request(Request::Ping).await? {
            Response::Pong { version, protocol } => Ok((version, protocol)),
            other => bail!("unexpected reply: {other:?}"),
        }
    }
}
