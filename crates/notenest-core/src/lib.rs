//! NoteNest core library.
//!
//! This crate must stay independent of Tauri so that it can be tested and
//! reused without a GUI. The notes engine is not implemented yet.

/// Application name.
pub const APP_NAME: &str = "NoteNest";

/// Human-readable application identity, e.g. `NoteNest 0.1.0`.
#[must_use]
pub fn app_info() -> String {
    format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_contains_name_and_version() {
        let info = app_info();
        assert!(info.starts_with(APP_NAME));
        assert!(info.ends_with(env!("CARGO_PKG_VERSION")));
    }
}
