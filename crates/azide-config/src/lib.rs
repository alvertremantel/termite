use jones_config::{
    ConfigError, data_dir as app_data_dir, load_app_config, load_toml_from_path, save_app_config,
    save_toml_to_path,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "azide";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const DEFAULT_THEME_ID: &str = "space";

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub rss: RssConfig,
    #[serde(default)]
    pub markdown: MarkdownConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ViewConfig {
    pub name: String,
    pub feeds: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RssConfig {
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval_minutes: u64,
    #[serde(default)]
    pub feeds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<ViewConfig>,
}

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct MarkdownConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub show_hidden: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapsed_dirs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_open_file: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub mouse: bool,
    #[serde(default = "default_mode_str")]
    pub default_mode: String,
    #[serde(default = "default_theme_str")]
    pub theme: String,
}

fn default_refresh_interval() -> u64 {
    30
}
fn default_true() -> bool {
    true
}
fn default_mode_str() -> String {
    "rss".to_string()
}
fn default_theme_str() -> String {
    DEFAULT_THEME_ID.to_string()
}

impl Default for RssConfig {
    fn default() -> Self {
        Self {
            refresh_interval_minutes: default_refresh_interval(),
            feeds: Vec::new(),
            views: Vec::new(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            mouse: true,
            default_mode: default_mode_str(),
            theme: default_theme_str(),
        }
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

    fn expand_path(p: &str) -> PathBuf {
        if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(rest)
        } else if p == "~" {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
        } else {
            PathBuf::from(p)
        }
    }

    pub fn markdown_roots(&self) -> Vec<PathBuf> {
        let mut result = Vec::new();
        if let Some(ref root) = self.markdown.root {
            result.push(Self::expand_path(root));
        }
        for root in &self.markdown.roots {
            let expanded = Self::expand_path(root);
            if !result.contains(&expanded) {
                result.push(expanded);
            }
        }
        if result.is_empty() {
            result.push(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        }
        result
    }

    pub fn remove_markdown_root(&mut self, path: &Path) {
        if let Some(ref root) = self.markdown.root
            && Self::expand_path(root) == path
        {
            self.markdown.root = None;
            return;
        }
        self.markdown
            .roots
            .retain(|root| Self::expand_path(root) != path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn defaults_include_theme_and_mouse() {
        let config = Config::default();
        assert_eq!(config.ui.theme, DEFAULT_THEME_ID);
        assert!(config.ui.mouse);
        assert_eq!(config.rss.views, Vec::<ViewConfig>::new());
    }

    #[test]
    fn toml_roundtrip_preserves_views_and_theme() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let config = Config {
            rss: RssConfig {
                refresh_interval_minutes: 15,
                feeds: vec!["https://example.test/feed.xml".into()],
                views: vec![ViewConfig {
                    name: "news".into(),
                    feeds: vec!["feed-a".into(), "feed-b".into()],
                }],
            },
            markdown: MarkdownConfig::default(),
            ui: UiConfig {
                mouse: true,
                default_mode: "rss".into(),
                theme: "clean-blue".into(),
            },
        };

        config.save_to_path(&path).unwrap();
        let loaded = Config::load_from_path(&path).unwrap();

        assert_eq!(loaded.rss.views[0].name, "news");
        assert_eq!(loaded.ui.theme, "clean-blue");
    }

    #[test]
    fn markdown_roots_expand_and_deduplicate() {
        let mut config = Config::default();
        config.markdown.root = Some("/tmp/docs".into());
        config.markdown.roots = vec!["/tmp/docs".into(), "/tmp/notes".into()];

        let roots = config.markdown_roots();

        assert_eq!(
            roots,
            vec![PathBuf::from("/tmp/docs"), PathBuf::from("/tmp/notes")]
        );
        config.remove_markdown_root(Path::new("/tmp/docs"));
        assert_eq!(config.markdown.root, None);
    }
}
