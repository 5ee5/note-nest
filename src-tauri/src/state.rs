use std::sync::{Mutex, PoisonError};

use notenest_core::services::{VaultWatcher, Workspace};
use notenest_core::{Error, Result};
use tauri::async_runtime::RwLock;

/// Shared application state: the currently open workspace, if any, and the
/// watcher that keeps its index in line with changes made outside NoteNest.
#[derive(Default)]
pub struct AppState {
    workspace: RwLock<Option<Workspace>>,
    watcher: Mutex<Option<VaultWatcher>>,
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

    /// Replaces (or with `None`, stops) the vault watcher.
    pub fn set_watcher(&self, watcher: Option<VaultWatcher>) {
        *self.watcher.lock().unwrap_or_else(PoisonError::into_inner) = watcher;
    }
}
