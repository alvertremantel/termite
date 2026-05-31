use std::env;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolStatus {
    pub name: &'static str,
    pub present: bool,
    pub hint: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub tlp: ToolStatus,
    pub tlp_stat: ToolStatus,
    pub powertop: ToolStatus,
    pub sudo: ToolStatus,
}

impl DiscoveryReport {
    pub fn missing_messages(&self) -> Vec<String> {
        [self.tlp.clone(), self.powertop.clone(), self.sudo.clone()]
            .into_iter()
            .filter(|tool| !tool.present)
            .map(|tool| format!("{} missing — {}", tool.name, tool.hint))
            .collect()
    }
}

pub trait ToolLocator {
    fn exists(&self, program: &str) -> bool;
}

#[derive(Clone, Debug, Default)]
pub struct StdToolLocator;

impl ToolLocator for StdToolLocator {
    fn exists(&self, program: &str) -> bool {
        let path = env::var_os("PATH").unwrap_or_default();
        env::split_paths(&path)
            .map(|entry| entry.join(program))
            .any(|candidate| is_executable(&candidate))
    }
}

fn is_executable(candidate: &std::path::Path) -> bool {
    let Ok(metadata) = candidate.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

pub fn discover_tools(locator: &impl ToolLocator) -> DiscoveryReport {
    DiscoveryReport {
        tlp: ToolStatus {
            name: "tlp",
            present: locator.exists("tlp"),
            hint: "install TLP to enable mode/profile actions",
        },
        tlp_stat: ToolStatus {
            name: "tlp-stat",
            present: locator.exists("tlp-stat"),
            hint: "optional legacy TLP status/config reader",
        },
        powertop: ToolStatus {
            name: "powertop",
            present: locator.exists("powertop"),
            hint: "install powertop to enable report generation",
        },
        sudo: ToolStatus {
            name: "sudo",
            present: locator.exists("sudo"),
            hint: "install sudo or launch jtop as root for privileged actions",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Clone, Debug)]
    struct FakeLocator {
        tools: HashSet<String>,
    }

    impl ToolLocator for FakeLocator {
        fn exists(&self, program: &str) -> bool {
            self.tools.contains(program)
        }
    }

    #[test]
    fn discovers_partial_toolset() {
        let report = discover_tools(&FakeLocator {
            tools: ["tlp", "sudo"].into_iter().map(String::from).collect(),
        });
        assert!(report.tlp.present);
        assert!(!report.tlp_stat.present);
        assert!(!report.powertop.present);
        assert_eq!(
            report.missing_messages(),
            vec!["powertop missing — install powertop to enable report generation"]
        );
    }

    #[test]
    fn missing_tlp_stat_is_not_reported_as_required() {
        let report = discover_tools(&FakeLocator {
            tools: ["tlp", "powertop", "sudo"]
                .into_iter()
                .map(String::from)
                .collect(),
        });

        assert!(!report.tlp_stat.present);
        assert!(report.missing_messages().is_empty());
    }
}
