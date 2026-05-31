use jones_config::{
    ConfigError, data_dir as app_data_dir, load_app_config, load_toml_from_path, save_app_config,
    save_toml_to_path,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "termite";
pub const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub mouse: bool,
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkspaceConfig {
    /// Best-effort sync of directory-browser cwd changes into Termite's process
    /// cwd and terminal-emulator OSC 7 cwd hint. This cannot change the parent
    /// shell's directory after Termite exits.
    #[serde(default)]
    pub sync_terminal_cwd: bool,
}

fn default_true() -> bool {
    true
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { mouse: true }
    }
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        load_app_config(APP_NAME, CONFIG_FILE_NAME)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        save_app_config(APP_NAME, CONFIG_FILE_NAME, self)
    }

    pub fn load_from_path(path: &Path) -> Result<Self, ConfigError> {
        load_toml_from_path(path)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), ConfigError> {
        save_toml_to_path(path, self)
    }

    pub fn data_dir() -> PathBuf {
        app_data_dir(APP_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn defaults_enable_mouse() {
        assert!(Config::default().ui.mouse);
        assert!(!Config::default().workspace.sync_terminal_cwd);
    }

    #[test]
    fn toml_roundtrip_preserves_mouse_setting() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let config = Config {
            ui: UiConfig { mouse: false },
            workspace: WorkspaceConfig {
                sync_terminal_cwd: true,
            },
        };

        config.save_to_path(&path).unwrap();
        let loaded = Config::load_from_path(&path).unwrap();

        assert_eq!(loaded, config);
    }
}
