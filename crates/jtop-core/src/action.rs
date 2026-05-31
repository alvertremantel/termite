use std::path::PathBuf;

use crate::{error::Error, tlp::TlpVersion};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionRisk {
    Safe,
    Caution,
    Dangerous,
}

impl ActionRisk {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Caution => "caution",
            Self::Dangerous => "danger",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub needs_root: bool,
    pub risk: ActionRisk,
    pub description: String,
}

impl CommandSpec {
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.program.clone())
            .chain(self.args.clone())
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActionContext {
    pub tlp_version: Option<TlpVersion>,
    pub powertop_csv_path: Option<PathBuf>,
}

impl ActionContext {
    pub fn tlp_named_profiles_supported(&self) -> bool {
        self.tlp_version
            .as_ref()
            .is_some_and(TlpVersion::supports_named_profiles)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlpMode {
    Ac,
    Bat,
}

impl TlpMode {
    pub fn as_arg(&self) -> &'static str {
        match self {
            Self::Ac => "ac",
            Self::Bat => "bat",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlpProfile {
    Performance,
    Balanced,
    PowerSaver,
}

impl TlpProfile {
    pub fn as_arg(&self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Balanced => "balanced",
            Self::PowerSaver => "power-saver",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PowerAction {
    TlpStatus,
    TlpConfig,
    TlpVersion,
    PowertopVersion,
    TlpStart,
    TlpMode(TlpMode),
    TlpNamedProfile(TlpProfile),
    PowertopReport { seconds: u64 },
    ListTlpConfig,
}

impl PowerAction {
    pub fn label(&self) -> String {
        match self {
            Self::TlpStatus => "Refresh TLP status".into(),
            Self::TlpConfig => "Refresh TLP config".into(),
            Self::TlpVersion => "Read TLP version".into(),
            Self::PowertopVersion => "Read powertop version".into(),
            Self::TlpStart => "Apply TLP defaults".into(),
            Self::TlpMode(TlpMode::Ac) => "Switch TLP mode to AC".into(),
            Self::TlpMode(TlpMode::Bat) => "Switch TLP mode to battery".into(),
            Self::TlpNamedProfile(profile) => {
                format!("Switch TLP profile to {}", profile.as_arg())
            }
            Self::PowertopReport { seconds } => format!("Generate powertop report ({seconds}s)"),
            Self::ListTlpConfig => "List /etc/tlp.d snippets".into(),
        }
    }

    pub fn command_spec(&self, context: &ActionContext) -> Result<CommandSpec, Error> {
        let spec = match self {
            Self::TlpStatus => CommandSpec {
                program: "tlp-stat".into(),
                args: vec!["-s".into()],
                needs_root: false,
                risk: ActionRisk::Safe,
                description: "Read TLP status".into(),
            },
            Self::TlpConfig => CommandSpec {
                program: "tlp-stat".into(),
                args: vec!["-c".into()],
                needs_root: false,
                risk: ActionRisk::Safe,
                description: "Read TLP config".into(),
            },
            Self::TlpVersion => CommandSpec {
                program: "tlp".into(),
                args: vec!["--version".into()],
                needs_root: false,
                risk: ActionRisk::Safe,
                description: "Read TLP version".into(),
            },
            Self::PowertopVersion => CommandSpec {
                program: "powertop".into(),
                args: vec!["--version".into()],
                needs_root: false,
                risk: ActionRisk::Safe,
                description: "Read powertop version".into(),
            },
            Self::TlpStart => CommandSpec {
                program: "tlp".into(),
                args: vec!["start".into()],
                needs_root: true,
                risk: ActionRisk::Caution,
                description: "Apply TLP settings".into(),
            },
            Self::TlpMode(mode) => CommandSpec {
                program: "tlp".into(),
                args: vec![mode.as_arg().into()],
                needs_root: true,
                risk: ActionRisk::Caution,
                description: format!("Switch TLP mode to {}", mode.as_arg()),
            },
            Self::TlpNamedProfile(profile) => {
                if !context.tlp_named_profiles_supported() {
                    return Err(Error::ParseFailure {
                        context: "TLP named profiles".into(),
                        details: "installed TLP version does not support named profiles".into(),
                    });
                }
                CommandSpec {
                    program: "tlp".into(),
                    args: vec![profile.as_arg().into()],
                    needs_root: true,
                    risk: ActionRisk::Caution,
                    description: format!("Switch TLP profile to {}", profile.as_arg()),
                }
            }
            Self::PowertopReport { seconds } => {
                let csv_path = context.powertop_csv_path.as_ref().ok_or_else(|| {
                    Error::RejectedPathOperation {
                        path: PathBuf::from("<powertop-tempfile>"),
                        reason: "powertop report needs a prepared temp csv path".into(),
                    }
                })?;
                CommandSpec {
                    program: "powertop".into(),
                    args: vec![
                        format!("--csv={}", csv_path.display()),
                        format!("--time={seconds}"),
                    ],
                    needs_root: true,
                    risk: ActionRisk::Caution,
                    description: format!("Generate powertop CSV report over {seconds}s"),
                }
            }
            Self::ListTlpConfig => CommandSpec {
                program: "true".into(),
                args: vec![],
                needs_root: false,
                risk: ActionRisk::Safe,
                description: "List TLP config snippets from disk".into(),
            },
        };
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tlp_mode_builds_exact_args() {
        let spec = PowerAction::TlpMode(TlpMode::Bat)
            .command_spec(&ActionContext::default())
            .unwrap();
        assert_eq!(spec.program, "tlp");
        assert_eq!(spec.args, vec!["bat"]);
        assert!(spec.needs_root);
    }

    #[test]
    fn named_profiles_are_version_gated() {
        let unsupported = ActionContext {
            tlp_version: Some(TlpVersion::new(1, 8, 0)),
            powertop_csv_path: None,
        };
        assert!(
            PowerAction::TlpNamedProfile(TlpProfile::Balanced)
                .command_spec(&unsupported)
                .is_err()
        );

        let supported = ActionContext {
            tlp_version: Some(TlpVersion::new(1, 9, 0)),
            powertop_csv_path: None,
        };
        let spec = PowerAction::TlpNamedProfile(TlpProfile::Balanced)
            .command_spec(&supported)
            .unwrap();
        assert_eq!(spec.args, vec!["balanced"]);
    }

    #[test]
    fn powertop_report_uses_prepared_temp_path() {
        let spec = PowerAction::PowertopReport { seconds: 7 }
            .command_spec(&ActionContext {
                tlp_version: None,
                powertop_csv_path: Some(PathBuf::from("/tmp/example.csv")),
            })
            .unwrap();
        assert_eq!(spec.args, vec!["--csv=/tmp/example.csv", "--time=7"],);
    }

    #[test]
    fn commands_are_argument_arrays_not_shell_soup() {
        let actions = [
            PowerAction::TlpStart,
            PowerAction::TlpMode(TlpMode::Ac),
            PowerAction::TlpStatus,
        ];
        for action in actions {
            let spec = action.command_spec(&ActionContext::default()).unwrap();
            for part in spec.argv() {
                assert!(!part.contains(';'));
                assert!(!part.contains('|'));
                assert!(!part.contains("&&"));
            }
        }
    }
}
