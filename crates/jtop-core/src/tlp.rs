use std::{cmp::Ordering, time::Duration};

use crate::{
    ActionContext, PowerAction,
    error::{Error, Result},
    privilege::InvocationMode,
    runner::CommandRunner,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlpVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl TlpVersion {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn supports_named_profiles(&self) -> bool {
        self.cmp(&Self::new(1, 9, 0)) != Ordering::Less
    }
}

impl Ord for TlpVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }
}

impl PartialOrd for TlpVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlpStatusSummary {
    pub mode: Option<String>,
    pub power_source: Option<String>,
    pub service_state: Option<String>,
    pub raw: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlpConfigSummary {
    pub active_files: Vec<String>,
    pub raw: String,
}

pub fn parse_tlp_version(text: &str) -> Result<TlpVersion> {
    for token in text.split_whitespace() {
        let parts = token.split('.').collect::<Vec<_>>();
        if parts.len() >= 2
            && parts
                .iter()
                .take(3)
                .all(|part| part.chars().all(|c| c.is_ascii_digit()))
        {
            let major = parts[0].parse().map_err(|_| Error::ParseFailure {
                context: "TLP version".into(),
                details: format!("invalid major token: {}", parts[0]),
            })?;
            let minor = parts[1].parse().map_err(|_| Error::ParseFailure {
                context: "TLP version".into(),
                details: format!("invalid minor token: {}", parts[1]),
            })?;
            let patch = parts.get(2).and_then(|part| part.parse().ok()).unwrap_or(0);
            return Ok(TlpVersion::new(major, minor, patch));
        }
    }
    Err(Error::ParseFailure {
        context: "TLP version".into(),
        details: "no semantic version-like token found".into(),
    })
}

pub fn parse_tlp_status(text: &str) -> TlpStatusSummary {
    let mut mode = None;
    let mut power_source = None;
    let mut service_state = None;

    for line in text.lines() {
        let trimmed = line.trim();
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };

        match key.trim() {
            "Mode" => mode = Some(value.trim().to_string()),
            "Power source" => power_source = Some(value.trim().to_string()),
            "State" | "TLP power save" => service_state = Some(value.trim().to_string()),
            _ => {}
        }
    }

    TlpStatusSummary {
        mode,
        power_source,
        service_state,
        raw: text.to_string(),
    }
}

pub fn parse_tlp_config(text: &str) -> TlpConfigSummary {
    let active_files = text
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("/etc/") && trimmed.ends_with(".conf") {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
        .collect();

    TlpConfigSummary {
        active_files,
        raw: text.to_string(),
    }
}

pub fn read_tlp_version<R: CommandRunner>(runner: &R, timeout: Duration) -> Result<TlpVersion> {
    let spec = PowerAction::TlpVersion.command_spec(&ActionContext::default())?;
    let output = runner.run(&spec, InvocationMode::Direct, timeout)?;
    parse_tlp_version(&output.stdout)
}

pub fn read_tlp_status<R: CommandRunner>(
    runner: &R,
    timeout: Duration,
) -> Result<TlpStatusSummary> {
    let spec = PowerAction::TlpStatus.command_spec(&ActionContext::default())?;
    let output = runner.run(&spec, InvocationMode::Direct, timeout)?;
    Ok(parse_tlp_status(&output.stdout))
}

pub fn read_tlp_config<R: CommandRunner>(
    runner: &R,
    timeout: Duration,
) -> Result<TlpConfigSummary> {
    let spec = PowerAction::TlpConfig.command_spec(&ActionContext::default())?;
    let output = runner.run(&spec, InvocationMode::Direct, timeout)?;
    Ok(parse_tlp_config(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandOutput, FakeCommandRunner};

    const STATUS_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tlp-stat-s.txt"
    ));
    const CONFIG_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tlp-stat-c.txt"
    ));

    #[test]
    fn parses_version_and_named_profile_support() {
        let version = parse_tlp_version("TLP 1.9.2").unwrap();
        assert!(version.supports_named_profiles());
    }

    #[test]
    fn parses_status_fixture() {
        let status = parse_tlp_status(STATUS_FIXTURE);
        assert_eq!(status.mode.as_deref(), Some("battery"));
        assert_eq!(status.power_source.as_deref(), Some("battery"));
        assert_eq!(status.service_state.as_deref(), Some("enabled"));
    }

    #[test]
    fn parses_current_tlp_status_layout() {
        let status = parse_tlp_status(
            "+++ TLP Status\nState          = enabled\nMode           = AC\nPower source   = AC\n",
        );

        assert_eq!(status.mode.as_deref(), Some("AC"));
        assert_eq!(status.power_source.as_deref(), Some("AC"));
        assert_eq!(status.service_state.as_deref(), Some("enabled"));
    }

    #[test]
    fn parses_config_fixture() {
        let config = parse_tlp_config(CONFIG_FIXTURE);
        assert!(
            config
                .active_files
                .iter()
                .any(|file| file.ends_with("00-base.conf"))
        );
    }

    #[test]
    fn reads_tlp_status_from_runner() {
        let runner = FakeCommandRunner::default();
        runner.push_response(Ok(CommandOutput {
            stdout: STATUS_FIXTURE.into(),
            stderr: String::new(),
            exit_code: 0,
        }));
        let status = read_tlp_status(&runner, Duration::from_secs(1)).unwrap();
        assert_eq!(status.service_state.as_deref(), Some("enabled"));
    }
}
