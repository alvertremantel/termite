use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::error::{Error, Result};

pub const DEFAULT_TLP_DIR: &str = "/etc/tlp.d";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlpConfigState {
    ActiveConf,
    DisabledBak,
    Ignored,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlpConfigFile {
    pub path: PathBuf,
    pub basename: String,
    pub state: TlpConfigState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlpConfigScan {
    pub root: PathBuf,
    pub files: Vec<TlpConfigFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameOp {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlpConfigSwitchPlan {
    pub target_set: String,
    pub enable: Vec<RenameOp>,
    pub disable: Vec<RenameOp>,
    pub commands_or_ops: Vec<String>,
    pub warnings: Vec<String>,
    pub journal_design: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConfigIdentity {
    logical_name: String,
    set_name: String,
}

pub fn scan_tlp_config_dir(root: impl AsRef<Path>) -> Result<TlpConfigScan> {
    let root = root.as_ref().to_path_buf();
    let mut files = Vec::new();

    let entries = fs::read_dir(&root)
        .map_err(|error| Error::io(format!("read {}", root.display()), error))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| Error::io(format!("read {} entry", root.display()), error))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| Error::io(format!("read metadata for {}", path.display()), error))?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        let (basename, state) = if metadata.file_type().is_symlink() || !metadata.is_file() {
            (file_name, TlpConfigState::Ignored)
        } else if let Some(stem) = file_name.strip_suffix(".conf") {
            (stem.to_string(), TlpConfigState::ActiveConf)
        } else if let Some(stem) = file_name.strip_suffix(".conf.bak") {
            (stem.to_string(), TlpConfigState::DisabledBak)
        } else {
            (file_name, TlpConfigState::Ignored)
        };

        files.push(TlpConfigFile {
            path,
            basename,
            state,
        });
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(TlpConfigScan { root, files })
}

pub fn plan_tlp_config_switch(
    scan: &TlpConfigScan,
    target_set: &str,
) -> Result<TlpConfigSwitchPlan> {
    let canonical_root = fs::canonicalize(&scan.root)
        .map_err(|error| Error::io(format!("canonicalize {}", scan.root.display()), error))?;
    let mut target_candidates = Vec::new();
    let mut active_by_logical = BTreeMap::<String, &TlpConfigFile>::new();
    let mut seen_basenames = BTreeMap::<String, PathBuf>::new();
    let mut warnings = Vec::new();

    for file in &scan.files {
        let canonical_parent = file
            .path
            .parent()
            .ok_or_else(|| Error::RejectedPathOperation {
                path: file.path.clone(),
                reason: "missing parent directory".into(),
            })?
            .to_path_buf();
        let canonical_parent = fs::canonicalize(&canonical_parent).map_err(|error| {
            Error::io(
                format!("canonicalize {}", canonical_parent.display()),
                error,
            )
        })?;
        if canonical_parent != canonical_root {
            return Err(Error::RejectedPathOperation {
                path: file.path.clone(),
                reason: "file is outside configured tlp directory".into(),
            });
        }

        if !matches!(file.state, TlpConfigState::Ignored)
            && let Some(previous_path) =
                seen_basenames.insert(file.basename.clone(), file.path.clone())
        {
            return Err(Error::RejectedPathOperation {
                path: file.path.clone(),
                reason: format!(
                    "duplicate basename `{}` conflicts with {}",
                    file.basename,
                    previous_path.display()
                ),
            });
        }

        let Some(identity) = parse_identity(&file.basename) else {
            if !matches!(file.state, TlpConfigState::Ignored) {
                warnings.push(format!(
                    "{} does not match the <logical>@<set>.conf(.bak) preview convention",
                    file.path.display()
                ));
            }
            continue;
        };

        if identity.set_name == target_set {
            target_candidates.push((identity.clone(), file));
        }

        if matches!(file.state, TlpConfigState::ActiveConf)
            && active_by_logical
                .insert(identity.logical_name.clone(), file)
                .is_some()
        {
            return Err(Error::RejectedPathOperation {
                path: file.path.clone(),
                reason: "duplicate active logical name would make switch preview ambiguous".into(),
            });
        }
    }

    if target_candidates.is_empty() {
        warnings.push(format!(
            "no disabled snippets found for target set `{target_set}`; preview may be a no-op"
        ));
    }

    let mut enable = Vec::new();
    let mut disable = Vec::new();
    let mut destinations = BTreeSet::new();
    let existing_paths = scan
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();

    for (identity, file) in target_candidates {
        match file.state {
            TlpConfigState::DisabledBak => {
                let target = file.path.with_file_name(format!("{}.conf", file.basename));
                if !destinations.insert(target.clone()) {
                    return Err(Error::RejectedPathOperation {
                        path: target,
                        reason: "duplicate destination basename in switch plan".into(),
                    });
                }
                enable.push(RenameOp {
                    from: file.path.clone(),
                    to: target,
                });
            }
            TlpConfigState::ActiveConf => {
                warnings.push(format!("{} is already active", file.basename));
            }
            TlpConfigState::Ignored => {}
        }

        if let Some(current_active) = active_by_logical.get(&identity.logical_name)
            && current_active.basename != file.basename
        {
            let target = current_active
                .path
                .with_file_name(format!("{}.conf.bak", current_active.basename));
            if !destinations.insert(target.clone()) {
                return Err(Error::RejectedPathOperation {
                    path: target,
                    reason: "duplicate destination basename in switch plan".into(),
                });
            }
            disable.push(RenameOp {
                from: current_active.path.clone(),
                to: target,
            });
        }
    }

    let planned_sources = disable
        .iter()
        .chain(enable.iter())
        .map(|op| op.from.clone())
        .collect::<BTreeSet<_>>();

    for op in disable.iter().chain(enable.iter()) {
        if existing_paths.contains(&op.to) && op.to != op.from && !planned_sources.contains(&op.to)
        {
            return Err(Error::RejectedPathOperation {
                path: op.to.clone(),
                reason: "switch plan would overwrite an existing file".into(),
            });
        }
    }

    let mut commands_or_ops = Vec::new();
    commands_or_ops.extend(
        disable
            .iter()
            .map(|op| format!("rename {} -> {}", op.from.display(), op.to.display())),
    );
    commands_or_ops.extend(
        enable
            .iter()
            .map(|op| format!("rename {} -> {}", op.from.display(), op.to.display())),
    );

    Ok(TlpConfigSwitchPlan {
        target_set: target_set.to_string(),
        enable,
        disable,
        commands_or_ops,
        warnings,
        journal_design: "Later execution should journal rename pairs under /etc/tlp.d/.jtop-journal/, fsync the journal, then apply reversible same-directory renames.".into(),
    })
}

fn parse_identity(basename: &str) -> Option<ConfigIdentity> {
    let (logical_name, set_name) = basename.rsplit_once('@')?;
    if logical_name.is_empty() || set_name.is_empty() {
        return None;
    }
    Some(ConfigIdentity {
        logical_name: logical_name.to_string(),
        set_name: set_name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};

    #[cfg(unix)]
    use std::os::unix::fs::{PermissionsExt, symlink};

    use tempfile::tempdir;

    #[test]
    fn scans_conf_and_bak_files() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("cpu@balanced.conf")).unwrap();
        File::create(dir.path().join("cpu@travel.conf.bak")).unwrap();
        File::create(dir.path().join("README")).unwrap();
        fs::create_dir(dir.path().join("nested@travel.conf")).unwrap();

        let scan = scan_tlp_config_dir(dir.path()).unwrap();
        assert_eq!(scan.files.len(), 4);
        assert!(
            scan.files
                .iter()
                .any(|file| matches!(file.state, TlpConfigState::ActiveConf))
        );
        assert!(
            scan.files
                .iter()
                .any(|file| matches!(file.state, TlpConfigState::DisabledBak))
        );
        assert!(scan.files.iter().any(|file| {
            file.basename == "nested@travel.conf" && matches!(file.state, TlpConfigState::Ignored)
        }));
    }

    #[test]
    #[cfg(unix)]
    fn symlinks_are_ignored() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("cpu@balanced.conf")).unwrap();
        symlink(
            dir.path().join("cpu@balanced.conf"),
            dir.path().join("alias.conf"),
        )
        .unwrap();

        let scan = scan_tlp_config_dir(dir.path()).unwrap();
        assert!(
            scan.files.iter().any(|file| file.basename == "alias.conf"
                && matches!(file.state, TlpConfigState::Ignored))
        );
    }

    #[test]
    fn plans_reversible_switch() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("cpu@balanced.conf")).unwrap();
        File::create(dir.path().join("cpu@travel.conf.bak")).unwrap();
        File::create(dir.path().join("wifi@travel.conf.bak")).unwrap();

        let scan = scan_tlp_config_dir(dir.path()).unwrap();
        let plan = plan_tlp_config_switch(&scan, "travel").unwrap();
        assert_eq!(plan.enable.len(), 2);
        assert_eq!(plan.disable.len(), 1);
        assert!(plan.commands_or_ops.iter().all(|op| op.contains("rename ")));
    }

    #[test]
    fn duplicate_destinations_are_refused() {
        let scan = TlpConfigScan {
            root: PathBuf::from(DEFAULT_TLP_DIR),
            files: vec![
                TlpConfigFile {
                    path: PathBuf::from("/etc/tlp.d/cpu@balanced.conf"),
                    basename: "cpu@balanced".into(),
                    state: TlpConfigState::ActiveConf,
                },
                TlpConfigFile {
                    path: PathBuf::from("/etc/tlp.d/cpu@travel.conf"),
                    basename: "cpu@travel".into(),
                    state: TlpConfigState::ActiveConf,
                },
            ],
        };
        assert!(plan_tlp_config_switch(&scan, "travel").is_err());
    }

    #[test]
    fn duplicate_basename_states_are_refused() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("cpu@travel.conf")).unwrap();
        File::create(dir.path().join("cpu@travel.conf.bak")).unwrap();

        let scan = scan_tlp_config_dir(dir.path()).unwrap();
        assert!(plan_tlp_config_switch(&scan, "travel").is_err());
    }

    #[test]
    fn existing_destination_files_are_refused() {
        let dir = tempdir().unwrap();
        File::create(dir.path().join("cpu@balanced.conf")).unwrap();
        File::create(dir.path().join("cpu@travel.conf.bak")).unwrap();
        File::create(dir.path().join("cpu@balanced.conf.bak")).unwrap();

        let scan = scan_tlp_config_dir(dir.path()).unwrap();
        assert!(plan_tlp_config_switch(&scan, "travel").is_err());
    }

    #[test]
    #[cfg(unix)]
    fn permission_errors_are_reported() {
        let dir = tempdir().unwrap();
        let blocked = dir.path().join("blocked");
        fs::create_dir(&blocked).unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
        if nix::unistd::Uid::effective().is_root() {
            return;
        }
        let result = scan_tlp_config_dir(&blocked);
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            result.unwrap_err(),
            Error::PermissionDenied { .. }
        ));
    }
}
