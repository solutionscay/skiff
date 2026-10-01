use crate::Client;
use anyhow::{bail, Result};
use skiff_core::protocol::{Request, Response};

impl Client {
    pub async fn keys(&self) -> Result<std::collections::BTreeMap<String, String>> {
        match self.request(Request::GetKeys).await? {
            Response::Keys { keys } => Ok(keys),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn appearance(&self, set: Option<Option<String>>) -> Result<skiff_core::config::Appearance> {
        let req = match set {
            Some(theme) => Request::SetAppearance { theme },
            None => Request::GetAppearance,
        };
        self.appearance_reply(req).await
    }

    /// Sets the text size: points, or `None` for the default.
    pub async fn set_font_size(&self, size: Option<u8>) -> Result<skiff_core::config::Appearance> {
        self.appearance_reply(Request::SetFontSize { size }).await
    }

    async fn appearance_reply(&self, req: Request) -> Result<skiff_core::config::Appearance> {
        match self.request(req).await? {
            Response::Appearance { theme, font_size } => Ok(skiff_core::config::Appearance { theme, font_size, ..Default::default() }),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }

    pub async fn list_themes(&self) -> Result<Vec<skiff_core::theme::TerminalTheme>> {
        match self.request(Request::ListThemes).await? {
            Response::Themes { themes } => Ok(themes),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected reply: {other:?}"),
        }
    }
}
