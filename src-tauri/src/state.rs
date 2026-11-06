use notenest_core::services::Workspace;
use notenest_core::{Error, Result};
use tauri::async_runtime::RwLock;

/// Shared application state: the currently open workspace, if any.
#[derive(Default)]
pub struct AppState {
    workspace: RwLock<Option<Workspace>>,
}

impl AppState {
    /// A cheap clone of the open workspace.
    ///
    /// # Errors
    /// [`Error::VaultNotOpen`] before a vault has been opened.
    pub async fn workspace(&self) -> Result<Workspace> {
        self.workspace
            .read()
            .await
            .clone()
            .ok_or(Error::VaultNotOpen)
    }

    pub async fn set_workspace(&self, ws: Workspace) {
        *self.workspace.write().await = Some(ws);
    }
}
