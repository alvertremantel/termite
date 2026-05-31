use serde::Serialize;
use serde::de::DeserializeOwned;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    TomlDe(toml::de::Error),
    TomlSer(toml::ser::Error),
}

impl Display for ConfigError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::TomlDe(err) => write!(f, "TOML decode error: {err}"),
            Self::TomlSer(err) => write!(f, "TOML encode error: {err}"),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::TomlDe(err) => Some(err),
            Self::TomlSer(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(value: toml::de::Error) -> Self {
        Self::TomlDe(value)
    }
}

impl From<toml::ser::Error> for ConfigError {
    fn from(value: toml::ser::Error) -> Self {
        Self::TomlSer(value)
    }
}

pub fn config_dir(app_name: &str) -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(app_name)
}

pub fn data_dir(app_name: &str) -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(app_name)
}

pub fn load_toml_from_path<T: DeserializeOwned + Default>(path: &Path) -> Result<T, ConfigError> {
    if path.exists() {
        Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
    } else {
        Ok(T::default())
    }
}

pub fn save_toml_to_path<T: Serialize>(path: &Path, value: &T) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(value)?)?;
    Ok(())
}

pub fn load_app_config<T: DeserializeOwned + Default>(
    app_name: &str,
    file_name: &str,
) -> Result<T, ConfigError> {
    let dir = config_dir(app_name);
    let path = dir.join(file_name);
    if !path.exists() {
        let _ = std::fs::create_dir_all(&dir);
        return Ok(T::default());
    }
    load_toml_from_path(&path)
}

pub fn save_app_config<T: Serialize>(
    app_name: &str,
    file_name: &str,
    value: &T,
) -> Result<(), ConfigError> {
    save_toml_to_path(&config_dir(app_name).join(file_name), value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    #[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
    struct SampleConfig {
        enabled: bool,
        name: String,
    }

    #[test]
    fn load_missing_path_returns_default() {
        let dir = TempDir::new().unwrap();
        let value = load_toml_from_path::<SampleConfig>(&dir.path().join("missing.toml")).unwrap();

        assert_eq!(value, SampleConfig::default());
    }

    #[test]
    fn save_and_load_roundtrip_uses_explicit_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested/config.toml");
        let expected = SampleConfig {
            enabled: true,
            name: "reader".into(),
        };

        save_toml_to_path(&path, &expected).unwrap();
        let loaded = load_toml_from_path::<SampleConfig>(&path).unwrap();

        assert_eq!(loaded, expected);
    }

    #[test]
    fn load_app_config_returns_default_when_file_absent() {
        // Use a uniquely-named app that certainly has no config directory.
        let value =
            load_app_config::<SampleConfig>("__jones_config_test_no_such_app__", "config.toml")
                .unwrap();
        assert_eq!(value, SampleConfig::default());
    }
}
