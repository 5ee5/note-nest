//! Thin Tauri adapter. Application logic lives in `notenest-core`.

#[tauri::command]
fn app_info() -> String {
    notenest_core::app_info()
}

/// Starts the Tauri application.
///
/// # Panics
/// Panics if the Tauri runtime fails to start.
#[allow(clippy::missing_panics_doc)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("error while running NoteNest");
}
