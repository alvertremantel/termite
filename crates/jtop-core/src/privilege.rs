use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use nix::unistd::Uid;

use crate::error::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivilegeState {
    pub effective_root: bool,
    pub sudo_available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvocationMode {
    Direct,
    AlreadyRoot,
    UseSudo,
    ReadOnlyOnly,
}

pub fn invocation_mode(needs_root: bool, state: &PrivilegeState) -> InvocationMode {
    if !needs_root {
        InvocationMode::Direct
    } else if state.effective_root {
        InvocationMode::AlreadyRoot
    } else if state.sudo_available {
        InvocationMode::UseSudo
    } else {
        InvocationMode::ReadOnlyOnly
    }
}

pub trait SudoProbe {
    fn sudo_available(&self, timeout: Duration) -> Result<bool>;
}

#[derive(Clone, Debug, Default)]
pub struct StdSudoProbe;

impl SudoProbe for StdSudoProbe {
    fn sudo_available(&self, timeout: Duration) -> Result<bool> {
        let mut child = Command::new("sudo")
            .arg("-n")
            .arg("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => Error::CommandNotFound {
                    program: "sudo".into(),
                },
                _ => Error::io("start sudo probe", error),
            })?;

        let started = Instant::now();
        loop {
            match child
                .try_wait()
                .map_err(|error| Error::io("wait for sudo probe", error))?
            {
                Some(status) => return Ok(status.success()),
                None if started.elapsed() >= timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::CommandTimeout {
                        program: "sudo -n true".into(),
                        timeout,
                    });
                }
                None => thread::sleep(Duration::from_millis(20)),
            }
        }
    }
}

pub fn detect_privilege_state<P: SudoProbe>(
    probe: &P,
    sudo_in_path: bool,
    timeout: Duration,
) -> Result<PrivilegeState> {
    let effective_root = Uid::effective().is_root();
    let sudo_available = if effective_root {
        sudo_in_path
    } else if sudo_in_path {
        probe.sudo_available(timeout)?
    } else {
        false
    };

    Ok(PrivilegeState {
        effective_root,
        sudo_available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct FakeProbe(bool);

    impl SudoProbe for FakeProbe {
        fn sudo_available(&self, _timeout: Duration) -> Result<bool> {
            Ok(self.0)
        }
    }

    #[test]
    fn policy_decisions_are_pure() {
        let state = PrivilegeState {
            effective_root: false,
            sudo_available: true,
        };
        assert_eq!(invocation_mode(true, &state), InvocationMode::UseSudo);
        assert_eq!(invocation_mode(false, &state), InvocationMode::Direct);
    }

    #[test]
    fn detect_privilege_state_respects_probe() {
        let state =
            detect_privilege_state(&FakeProbe(true), true, Duration::from_millis(1)).unwrap();
        assert_eq!(state.effective_root, Uid::effective().is_root());
        if !state.effective_root {
            assert!(state.sudo_available);
        }
    }
}
