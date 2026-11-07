//! NoteNest core library.
//!
//! This crate must stay independent of Tauri so that it can be tested and
//! reused without a GUI. Markdown files in the vault are the source of truth;
//! everything else (the SQLite index) is derived and rebuildable.

pub mod config;
pub mod database;
pub mod errors;
pub mod extract;
pub mod filesystem;
pub mod frontmatter;
pub mod hash;
pub mod markdown;
pub mod models;
pub mod services;
pub mod tag;

pub use errors::{Error, Result};

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
