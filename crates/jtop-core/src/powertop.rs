use std::{fs, path::PathBuf, time::Duration};

use tempfile::NamedTempFile;

use crate::{
    ActionContext, PowerAction,
    error::{Error, Result},
    privilege::InvocationMode,
    runner::CommandRunner,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowertopVersion {
    pub version: Option<String>,
    pub raw: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowertopReportSummary {
    pub seconds: u64,
    pub rows: usize,
    pub headers: Vec<String>,
    pub raw_csv: String,
}

#[derive(Debug)]
pub struct PreparedPowertopReport {
    temp_file: NamedTempFile,
    spec: crate::CommandSpec,
    seconds: u64,
}

impl PreparedPowertopReport {
    pub fn new(seconds: u64) -> Result<Self> {
        let temp_file =
            NamedTempFile::new().map_err(|error| Error::io("create powertop tempfile", error))?;
        let context = ActionContext {
            tlp_version: None,
            powertop_csv_path: Some(temp_file.path().to_path_buf()),
        };
        let spec = PowerAction::PowertopReport { seconds }.command_spec(&context)?;
        Ok(Self {
            temp_file,
            spec,
            seconds,
        })
    }

    pub fn spec(&self) -> &crate::CommandSpec {
        &self.spec
    }

    pub fn csv_path(&self) -> PathBuf {
        self.temp_file.path().to_path_buf()
    }

    pub fn finish(self) -> Result<PowertopReportSummary> {
        let csv = fs::read_to_string(self.temp_file.path())
            .map_err(|error| Error::io("read powertop csv report", error))?;
        parse_powertop_report(&csv, self.seconds)
    }
}

pub fn parse_powertop_version(text: &str) -> PowertopVersion {
    let version = text
        .split_whitespace()
        .find(|token| token.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
        .map(str::to_string);
    PowertopVersion {
        version,
        raw: text.to_string(),
    }
}

pub fn parse_powertop_report(text: &str, seconds: u64) -> Result<PowertopReportSummary> {
    let mut lines = text.lines();
    let headers = lines
        .next()
        .ok_or_else(|| Error::ParseFailure {
            context: "powertop report".into(),
            details: "missing header row".into(),
        })?
        .split(';')
        .map(|cell| cell.trim().to_string())
        .collect::<Vec<_>>();
    let rows = lines.filter(|line| !line.trim().is_empty()).count();
    Ok(PowertopReportSummary {
        seconds,
        rows,
        headers,
        raw_csv: text.to_string(),
    })
}

pub fn read_powertop_version<R: CommandRunner>(
    runner: &R,
    timeout: Duration,
) -> Result<PowertopVersion> {
    let spec = PowerAction::PowertopVersion.command_spec(&ActionContext::default())?;
    let output = runner.run(&spec, InvocationMode::Direct, timeout)?;
    Ok(parse_powertop_version(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandOutput, FakeCommandRunner};

    const CSV_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/powertop-report.csv"
    ));

    #[test]
    fn parses_report_fixture() {
        let report = parse_powertop_report(CSV_FIXTURE, 5).unwrap();
        assert_eq!(report.headers[0], "Category");
        assert_eq!(report.rows, 2);
    }

    #[test]
    fn prepared_report_builds_command_spec() {
        let prepared = PreparedPowertopReport::new(5).unwrap();
        assert_eq!(prepared.spec().program, "powertop");
        assert!(
            prepared
                .spec()
                .args
                .iter()
                .any(|arg| arg.starts_with("--csv="))
        );
        assert!(prepared.spec().args.contains(&"--time=5".to_string()));
    }

    #[test]
    fn reads_powertop_version_from_runner() {
        let runner = FakeCommandRunner::default();
        runner.push_response(Ok(CommandOutput {
            stdout: "PowerTOP version 2.15".into(),
            stderr: String::new(),
            exit_code: 0,
        }));
        let version = read_powertop_version(&runner, Duration::from_secs(1)).unwrap();
        assert_eq!(version.version.as_deref(), Some("2.15"));
    }
}
