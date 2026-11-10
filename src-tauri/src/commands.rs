use std::path::{Path, PathBuf};

use notenest_core::config::AppConfig;
use notenest_core::filesystem::trash::TrashEntry;
use notenest_core::markdown::Rendered;
use notenest_core::models::{
    BrokenLink, Graph, NoteEntry, NoteLinks, NoteRecord, NoteRef, SearchHit, TagCount,
};
use notenest_core::services::{
    watch, AffectedNote, Attachment, Draft, LinkUpdateReport, NoteContent, RenameOutcome, SaveMode,
    VaultChange, Workspace,
};
use notenest_core::{Error, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
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
    /// Whether changes made outside NoteNest are picked up as they happen. If not
    /// (e.g. the OS watch limit is reached), they are picked up on window focus.
    pub watching: bool,
}

/// Event sent to the window when files in the vault changed outside NoteNest.
pub const VAULT_CHANGED: &str = "vault-changed";

#[tauri::command]
pub async fn open_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    create: bool,
) -> Result<OpenedVault> {
    // Stop watching the previous vault before anything else touches the state.
    state.set_watcher(None);
    let (ws, report) = Workspace::open(Path::new(&path), create).await?;
    let root = ws.vault().root().to_string_lossy().into_owned();
    // Remember the vault only once it opened successfully.
    let mut cfg = AppConfig::load(&config_dir(&app)?)?;
    cfg.vault_path = Some(root.clone());
    cfg.save(&config_dir(&app)?)?;
    state.set_workspace(ws.clone()).await;
    let emitter = app.clone();
    let watcher = watch(ws, move |change| {
        // Fails only while the window is closing.
        let _ = emitter.emit(VAULT_CHANGED, change);
    });
    let watching = watcher.is_ok();
    if let Err(e) = &watcher {
        eprintln!("NoteNest: not watching the vault for outside changes: {e}");
    }
    state.set_watcher(watcher.ok());
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
        watching,
    })
}

#[tauri::command]
pub async fn reindex(state: State<'_, AppState>) -> Result<usize> {
    Ok(state.workspace().await?.reindex().await?.notes)
}

/// Brings the index in line with the vault now and says what changed outside
/// NoteNest (the watcher does the same on its own; this covers missed events).
#[tauri::command]
pub async fn reconcile(state: State<'_, AppState>) -> Result<VaultChange> {
    let report = state.workspace().await?.reindex().await?;
    Ok(VaultChange::from_report(&report, false))
}

#[tauri::command]
pub async fn list_notes(state: State<'_, AppState>) -> Result<Vec<NoteEntry>> {
    state.workspace().await?.list_entries().await
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

/// Renders note text for the preview (sanitized HTML).
#[tauri::command]
pub async fn render_markdown(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    content: String,
) -> Result<Rendered> {
    state.workspace().await?.render_markdown(id, content).await
}

/// Stores an image for a note. The image bytes are the raw request body (not a JSON
/// array, which would be huge); the note id and suggested file name travel in the
/// `x-note-id` and `x-file-name` (URL-encoded, optional) headers.
#[tauri::command]
pub async fn add_attachment(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<Attachment> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(Error::InvalidPath(
            "attachment bytes must be sent as a raw body".into(),
        ));
    };
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok());
    let note_id = header("x-note-id")
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or_else(|| Error::InvalidPath("missing or invalid x-note-id header".into()))?;
    let name = header("x-file-name").map(percent_decode);
    state
        .workspace()
        .await?
        .add_attachment(note_id, bytes, name.as_deref())
        .await
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        if let Some(b) = hex {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ------------------------------------------------------------ knowledge

#[tauri::command]
pub async fn search_notes(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>> {
    state
        .workspace()
        .await?
        .search(&query, limit.unwrap_or(50))
        .await
}

#[tauri::command]
pub async fn list_tags(state: State<'_, AppState>) -> Result<Vec<TagCount>> {
    state.workspace().await?.list_tags().await
}

#[tauri::command]
pub async fn note_links(state: State<'_, AppState>, id: Uuid) -> Result<NoteLinks> {
    state.workspace().await?.note_links(id).await
}

#[tauri::command]
pub async fn graph(
    state: State<'_, AppState>,
    center: Option<Uuid>,
    depth: Option<usize>,
) -> Result<Graph> {
    state
        .workspace()
        .await?
        .graph(center, depth.unwrap_or(1))
        .await
}

#[tauri::command]
pub async fn orphans(state: State<'_, AppState>) -> Result<Vec<NoteRef>> {
    state.workspace().await?.orphans().await
}

#[tauri::command]
pub async fn broken_links(state: State<'_, AppState>) -> Result<Vec<BrokenLink>> {
    state.workspace().await?.broken_links().await
}

#[tauri::command]
pub async fn resolve_link(
    state: State<'_, AppState>,
    source: Uuid,
    target: String,
) -> Result<Option<NoteRef>> {
    state.workspace().await?.resolve_link(source, &target).await
}

#[tauri::command]
pub async fn set_tags(
    state: State<'_, AppState>,
    id: Uuid,
    tags: Vec<String>,
) -> Result<NoteRecord> {
    state.workspace().await?.set_tags(id, &tags).await
}

#[tauri::command]
pub async fn preview_rename_note(
    state: State<'_, AppState>,
    id: Uuid,
    new_title: String,
) -> Result<Vec<AffectedNote>> {
    state
        .workspace()
        .await?
        .preview_rename_note(id, &new_title)
        .await
}

#[tauri::command]
pub async fn preview_move_note(
    state: State<'_, AppState>,
    id: Uuid,
    folder: Option<String>,
) -> Result<Vec<AffectedNote>> {
    state
        .workspace()
        .await?
        .preview_move_note(id, folder.as_deref())
        .await
}

#[tauri::command]
pub async fn preview_rename_folder(
    state: State<'_, AppState>,
    path: String,
    new_name: String,
) -> Result<Vec<AffectedNote>> {
    state
        .workspace()
        .await?
        .preview_rename_folder(&path, &new_name)
        .await
}

#[tauri::command]
pub async fn preview_move_folder(
    state: State<'_, AppState>,
    path: String,
    new_parent: Option<String>,
) -> Result<Vec<AffectedNote>> {
    state
        .workspace()
        .await?
        .preview_move_folder(&path, new_parent.as_deref())
        .await
}

#[tauri::command]
pub async fn rename_note_with_links(
    state: State<'_, AppState>,
    id: Uuid,
    new_title: String,
) -> Result<RenameOutcome> {
    state
        .workspace()
        .await?
        .rename_note_with_links(id, &new_title)
        .await
}

#[tauri::command]
pub async fn move_note_with_links(
    state: State<'_, AppState>,
    id: Uuid,
    folder: Option<String>,
) -> Result<RenameOutcome> {
    state
        .workspace()
        .await?
        .move_note_with_links(id, folder.as_deref())
        .await
}

#[tauri::command]
pub async fn rename_folder_with_links(
    state: State<'_, AppState>,
    path: String,
    new_name: String,
) -> Result<LinkUpdateReport> {
    state
        .workspace()
        .await?
        .rename_folder_with_links(&path, &new_name)
        .await
}

#[tauri::command]
pub async fn move_folder_with_links(
    state: State<'_, AppState>,
    path: String,
    new_parent: Option<String>,
) -> Result<LinkUpdateReport> {
    state
        .workspace()
        .await?
        .move_folder_with_links(&path, new_parent.as_deref())
        .await
}
