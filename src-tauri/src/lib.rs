//! Thin Tauri adapter. Application logic lives in `notenest-core`; every command
//! here only unwraps arguments, calls the workspace, and returns its result.

mod commands;
mod state;

/// Starts the Tauri application.
///
/// # Panics
/// Panics if the Tauri runtime fails to start.
#[allow(clippy::missing_panics_doc)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::vault_status,
            commands::open_vault,
            commands::reindex,
            commands::reconcile,
            commands::list_notes,
            commands::list_folders,
            commands::read_note,
            commands::save_note,
            commands::create_note,
            commands::rename_note,
            commands::move_note,
            commands::duplicate_note,
            commands::set_pinned,
            commands::set_archived,
            commands::delete_note,
            commands::mark_opened,
            commands::list_trash,
            commands::restore,
            commands::create_folder,
            commands::rename_folder,
            commands::move_folder,
            commands::delete_folder,
            commands::stash_draft,
            commands::list_drafts,
            commands::discard_draft,
            commands::get_setting,
            commands::set_setting,
            commands::render_markdown,
            commands::add_attachment,
            commands::search_notes,
            commands::list_tags,
            commands::note_links,
            commands::graph,
            commands::orphans,
            commands::broken_links,
            commands::resolve_link,
            commands::set_tags,
            commands::preview_rename_note,
            commands::preview_move_note,
            commands::preview_rename_folder,
            commands::preview_move_folder,
            commands::rename_note_with_links,
            commands::move_note_with_links,
            commands::rename_folder_with_links,
            commands::move_folder_with_links,
            commands::import_files,
            commands::import_folder,
            commands::export_note,
            commands::export_vault,
            commands::backup_vault,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NoteNest");
}
