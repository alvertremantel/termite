use jones_config::{
    ConfigError, data_dir as app_data_dir, load_app_config, load_toml_from_path, save_app_config,
    save_toml_to_path,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "termex";
pub const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub mouse: bool,
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
    }

    #[test]
    fn toml_roundtrip_preserves_mouse_setting() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let config = Config {
            ui: UiConfig { mouse: false },
        };

        config.save_to_path(&path).unwrap();
        let loaded = Config::load_from_path(&path).unwrap();

        assert_eq!(loaded, config);
    }
}
