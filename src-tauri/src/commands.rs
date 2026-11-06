use std::path::{Path, PathBuf};

use notenest_core::config::AppConfig;
use notenest_core::filesystem::trash::TrashEntry;
use notenest_core::models::NoteRecord;
use notenest_core::services::{Draft, NoteContent, SaveMode, Workspace};
use notenest_core::{Error, Result};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::state::AppState;

fn config_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_config_dir()
        .map_err(|e| Error::InvalidPath(format!("no config directory: {e}")))
}

fn default_vault(app: &AppHandle) -> Option<PathBuf> {
    app.path().home_dir().ok().map(|h| h.join("NoteNest"))
}

#[tauri::command]
pub fn app_info() -> String {
    notenest_core::app_info()
}

#[derive(Serialize)]
pub struct VaultStatus {
    /// Vault remembered from the last run, if any.
    pub configured_path: Option<String>,
    /// Suggested location for a new vault.
    pub default_path: Option<String>,
    /// Whether a vault is open right now.
    pub open: bool,
}

#[tauri::command]
pub async fn vault_status(app: AppHandle, state: State<'_, AppState>) -> Result<VaultStatus> {
    let cfg = AppConfig::load(&config_dir(&app)?)?;
    Ok(VaultStatus {
        configured_path: cfg.vault_path,
        default_path: default_vault(&app).map(|p| p.to_string_lossy().into_owned()),
        open: state.workspace().await.is_ok(),
    })
}

#[derive(Serialize)]
pub struct OpenedVault {
    pub path: String,
    pub index_rebuilt: bool,
    pub index_backup: Option<String>,
    pub notes: usize,
    pub unreadable: Vec<String>,
    pub skipped_invalid: Vec<String>,
    pub duplicate_ids_fixed: Vec<String>,
}

#[tauri::command]
pub async fn open_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    create: bool,
) -> Result<OpenedVault> {
    let (ws, report) = Workspace::open(Path::new(&path), create).await?;
    let root = ws.vault().root().to_string_lossy().into_owned();
    // Remember the vault only once it opened successfully.
    let mut cfg = AppConfig::load(&config_dir(&app)?)?;
    cfg.vault_path = Some(root.clone());
    cfg.save(&config_dir(&app)?)?;
    state.set_workspace(ws).await;
    Ok(OpenedVault {
        path: root,
        index_rebuilt: report.index_rebuilt,
        index_backup: report
            .index_backup
            .map(|p| p.to_string_lossy().into_owned()),
        notes: report.reindex.notes,
        unreadable: report.reindex.unreadable,
        skipped_invalid: report.reindex.skipped_invalid,
        duplicate_ids_fixed: report.reindex.duplicate_ids_fixed,
    })
}

#[tauri::command]
pub async fn reindex(state: State<'_, AppState>) -> Result<usize> {
    Ok(state.workspace().await?.reindex().await?.notes)
}

#[tauri::command]
pub async fn list_notes(state: State<'_, AppState>) -> Result<Vec<NoteRecord>> {
    state.workspace().await?.list_notes().await
}

#[tauri::command]
pub async fn list_folders(state: State<'_, AppState>) -> Result<Vec<String>> {
    state.workspace().await?.list_folders().await
}

#[tauri::command]
pub async fn read_note(state: State<'_, AppState>, id: Uuid) -> Result<NoteContent> {
    state.workspace().await?.read_note(id).await
}

/// Saves a note. With `overwrite`, external changes are replaced (after being
/// backed up); otherwise `base_hash` must match the file on disk.
#[tauri::command]
pub async fn save_note(
    state: State<'_, AppState>,
    id: Uuid,
    content: String,
    base_hash: Option<String>,
    overwrite: bool,
) -> Result<NoteContent> {
    let ws = state.workspace().await?;
    let mode = match (&base_hash, overwrite) {
        (_, true) => SaveMode::Overwrite,
        (Some(h), false) => SaveMode::Expect(h),
        (None, false) => {
            return Err(Error::InvalidPath(
                "save requires base_hash or overwrite".into(),
            ));
        }
    };
    ws.save_note(id, &content, mode).await
}

#[tauri::command]
pub async fn create_note(
    state: State<'_, AppState>,
    folder: Option<String>,
    title: Option<String>,
) -> Result<NoteRecord> {
    state
        .workspace()
        .await?
        .create_note(folder.as_deref(), title.as_deref())
        .await
}

#[tauri::command]
pub async fn rename_note(
    state: State<'_, AppState>,
    id: Uuid,
    new_title: String,
) -> Result<NoteRecord> {
    state.workspace().await?.rename_note(id, &new_title).await
}

#[tauri::command]
pub async fn move_note(
    state: State<'_, AppState>,
    id: Uuid,
    folder: Option<String>,
) -> Result<NoteRecord> {
    state
        .workspace()
        .await?
        .move_note(id, folder.as_deref())
        .await
}

#[tauri::command]
pub async fn duplicate_note(state: State<'_, AppState>, id: Uuid) -> Result<NoteRecord> {
    state.workspace().await?.duplicate_note(id).await
}

#[tauri::command]
pub async fn set_pinned(state: State<'_, AppState>, id: Uuid, pinned: bool) -> Result<NoteRecord> {
    state.workspace().await?.set_pinned(id, pinned).await
}

#[tauri::command]
pub async fn set_archived(
    state: State<'_, AppState>,
    id: Uuid,
    archived: bool,
) -> Result<NoteRecord> {
    state.workspace().await?.set_archived(id, archived).await
}

#[tauri::command]
pub async fn delete_note(state: State<'_, AppState>, id: Uuid) -> Result<TrashEntry> {
    state.workspace().await?.delete_note(id).await
}

#[tauri::command]
pub async fn mark_opened(state: State<'_, AppState>, id: Uuid) -> Result<()> {
    state.workspace().await?.mark_opened(id).await
}

#[tauri::command]
pub async fn list_trash(state: State<'_, AppState>) -> Result<Vec<TrashEntry>> {
    state.workspace().await?.list_trash()
}

#[tauri::command]
pub async fn restore(state: State<'_, AppState>, trash_id: Uuid) -> Result<String> {
    Ok(state
        .workspace()
        .await?
        .restore(trash_id)
        .await?
        .to_string())
}

#[tauri::command]
pub async fn create_folder(state: State<'_, AppState>, path: String) -> Result<()> {
    state.workspace().await?.create_folder(&path).await
}

#[tauri::command]
pub async fn rename_folder(
    state: State<'_, AppState>,
    path: String,
    new_name: String,
) -> Result<()> {
    state
        .workspace()
        .await?
        .rename_folder(&path, &new_name)
        .await
}

#[tauri::command]
pub async fn move_folder(
    state: State<'_, AppState>,
    path: String,
    new_parent: Option<String>,
) -> Result<()> {
    state
        .workspace()
        .await?
        .move_folder(&path, new_parent.as_deref())
        .await
}

#[tauri::command]
pub async fn delete_folder(state: State<'_, AppState>, path: String) -> Result<TrashEntry> {
    state.workspace().await?.delete_folder(&path).await
}

#[tauri::command]
pub async fn stash_draft(
    state: State<'_, AppState>,
    id: Uuid,
    content: String,
    base_hash: String,
) -> Result<()> {
    state
        .workspace()
        .await?
        .stash_draft(id, &content, &base_hash)
        .await
}

#[tauri::command]
pub async fn list_drafts(state: State<'_, AppState>) -> Result<Vec<Draft>> {
    state.workspace().await?.list_drafts().await
}

#[tauri::command]
pub async fn discard_draft(state: State<'_, AppState>, id: Uuid) -> Result<()> {
    state.workspace().await?.discard_draft(id)
}

#[tauri::command]
pub async fn get_setting(state: State<'_, AppState>, key: String) -> Result<Option<String>> {
    state.workspace().await?.get_setting(&key).await
}

#[tauri::command]
pub async fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<()> {
    state.workspace().await?.set_setting(&key, &value).await
}
