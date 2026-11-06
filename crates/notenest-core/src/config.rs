//! Per-user application configuration (not per-vault): currently just which
//! vault was opened last. Stored as JSON in the OS app-config directory, whose
//! location is supplied by the caller so this crate stays independent of Tauri.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::filesystem::atomic::write_atomic;
use crate::{Error, Result};

const FILE: &str = "config.json";

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Absolute path of the vault to open at startup.
    #[serde(default)]
    pub vault_path: Option<String>,
}

impl AppConfig {
    /// Loads the config from `dir`. A missing file yields the default. An unreadable
    /// or malformed file is moved aside to `config.json.invalid` (never deleted) and
    /// the default is returned, so a bad config cannot stop the app from starting.
    ///
    /// # Errors
    /// [`Error::Io`] only if the file exists but cannot be read or moved aside.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(FILE);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(Error::io(path, e)),
        };
        match serde_json::from_slice(&bytes) {
            Ok(cfg) => Ok(cfg),
            Err(e) => {
                tracing::warn!(error = %e, "invalid app config; keeping a copy and using defaults");
                let aside = dir.join(format!("{FILE}.invalid"));
                fs::rename(&path, &aside).map_err(|e| Error::io(&path, e))?;
                Ok(Self::default())
            }
        }
    }

    /// Saves the config to `dir`, creating it if needed.
    ///
    /// # Errors
    /// [`Error::Io`] if the file cannot be written.
    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let json = serde_json::to_vec_pretty(self).map_err(|e| Error::Corrupt {
            context: "app config".into(),
            message: e.to_string(),
        })?;
        write_atomic(&dir.join(FILE), &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(AppConfig::load(dir.path()).unwrap(), AppConfig::default());
    }

    #[test]
    fn round_trips_and_creates_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b");
        let cfg = AppConfig {
            vault_path: Some("/home/me/NoteNest".into()),
        };
        cfg.save(&nested).unwrap();
        assert_eq!(AppConfig::load(&nested).unwrap(), cfg);
    }

    #[test]
    fn malformed_config_is_kept_aside_and_defaults_used() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), b"{ nope").unwrap();
        assert_eq!(AppConfig::load(dir.path()).unwrap(), AppConfig::default());
        assert_eq!(
            fs::read(dir.path().join("config.json.invalid")).unwrap(),
            b"{ nope"
        );
        assert!(!dir.path().join("config.json").exists());
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.json"),
            br#"{"vault_path":"/x","future":1}"#,
        )
        .unwrap();
        assert_eq!(
            AppConfig::load(dir.path()).unwrap().vault_path.as_deref(),
            Some("/x")
        );
    }
}
