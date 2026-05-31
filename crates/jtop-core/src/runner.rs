use std::{
    collections::VecDeque,
    io::Read,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    action::CommandSpec,
    error::{Error, Result},
    privilege::InvocationMode,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub trait CommandRunner: Send + Sync {
    fn run(
        &self,
        spec: &CommandSpec,
        mode: InvocationMode,
        timeout: Duration,
    ) -> Result<CommandOutput>;
    fn request_shutdown(&self);
}

#[derive(Clone, Debug)]
pub struct StdCommandRunner {
    shutdown_requested: Arc<AtomicBool>,
}

impl Default for StdCommandRunner {
    fn default() -> Self {
        Self {
            shutdown_requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl StdCommandRunner {
    fn build_command(
        &self,
        spec: &CommandSpec,
        mode: InvocationMode,
    ) -> Result<(String, Vec<String>, Command)> {
        if spec.needs_root && matches!(mode, InvocationMode::ReadOnlyOnly) {
            return Err(Error::SudoUnavailable);
        }

        let (program, args) = match mode {
            InvocationMode::UseSudo if spec.needs_root => {
                let mut args = vec!["-n".to_string(), spec.program.clone()];
                args.extend(spec.args.clone());
                ("sudo".to_string(), args)
            }
            _ => (spec.program.clone(), spec.args.clone()),
        };

        let mut command = Command::new(&program);
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Ok((program, args, command))
    }
}

impl CommandRunner for StdCommandRunner {
    fn run(
        &self,
        spec: &CommandSpec,
        mode: InvocationMode,
        timeout: Duration,
    ) -> Result<CommandOutput> {
        let (program, _args, mut command) = self.build_command(spec, mode)?;
        let mut child = command.spawn().map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => Error::CommandNotFound {
                program: program.clone(),
            },
            _ => Error::io(format!("spawn {program}"), error),
        })?;

        let stdout_handle = child
            .stdout
            .take()
            .ok_or_else(|| Error::io("capture stdout", std::io::Error::other("missing stdout")))?;
        let stderr_handle = child
            .stderr
            .take()
            .ok_or_else(|| Error::io("capture stderr", std::io::Error::other("missing stderr")))?;

        let stdout_thread = thread::spawn(move || -> std::io::Result<String> {
            let mut reader = stdout_handle;
            let mut buffer = String::new();
            reader.read_to_string(&mut buffer)?;
            Ok(buffer)
        });
        let stderr_thread = thread::spawn(move || -> std::io::Result<String> {
            let mut reader = stderr_handle;
            let mut buffer = String::new();
            reader.read_to_string(&mut buffer)?;
            Ok(buffer)
        });

        let started = Instant::now();
        loop {
            if self.shutdown_requested.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::CommandTimeout {
                    program: spec.program.clone(),
                    timeout: started.elapsed(),
                });
            }

            match child
                .try_wait()
                .map_err(|error| Error::io(format!("wait for {program}"), error))?
            {
                Some(status) => {
                    let stdout = stdout_thread
                        .join()
                        .map_err(|_| Error::Io {
                            context: format!("collect stdout from {program}"),
                            details: "reader thread panicked".into(),
                        })?
                        .map_err(|error| Error::io(format!("read stdout from {program}"), error))?;
                    let stderr = stderr_thread
                        .join()
                        .map_err(|_| Error::Io {
                            context: format!("collect stderr from {program}"),
                            details: "reader thread panicked".into(),
                        })?
                        .map_err(|error| Error::io(format!("read stderr from {program}"), error))?;
                    let exit_code = status.code().unwrap_or(-1);
                    if status.success() {
                        return Ok(CommandOutput {
                            stdout,
                            stderr,
                            exit_code,
                        });
                    }
                    return Err(Error::NonZeroExit {
                        program: program.clone(),
                        code: status.code(),
                        stderr,
                    });
                }
                None if started.elapsed() >= timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(Error::CommandTimeout {
                        program: program.clone(),
                        timeout,
                    });
                }
                None => thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    fn request_shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedCall {
    pub program: String,
    pub args: Vec<String>,
    pub mode: InvocationMode,
    pub timeout: Duration,
}

#[derive(Clone, Debug, Default)]
pub struct FakeCommandRunner {
    responses: Arc<Mutex<VecDeque<Result<CommandOutput>>>>,
    calls: Arc<Mutex<Vec<RecordedCall>>>,
    shutdown_requested: Arc<AtomicBool>,
}

impl FakeCommandRunner {
    pub fn push_response(&self, response: Result<CommandOutput>) {
        self.responses.lock().unwrap().push_back(response);
    }

    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl CommandRunner for FakeCommandRunner {
    fn run(
        &self,
        spec: &CommandSpec,
        mode: InvocationMode,
        timeout: Duration,
    ) -> Result<CommandOutput> {
        self.calls.lock().unwrap().push(RecordedCall {
            program: spec.program.clone(),
            args: spec.args.clone(),
            mode,
            timeout,
        });
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Ok(CommandOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 0,
                })
            })
    }

    fn request_shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ActionRisk;

    fn sample_spec(needs_root: bool) -> CommandSpec {
        CommandSpec {
            program: "tlp".into(),
            args: vec!["start".into()],
            needs_root,
            risk: ActionRisk::Caution,
            description: "Apply TLP settings".into(),
        }
    }

    #[test]
    fn fake_runner_records_calls() {
        let runner = FakeCommandRunner::default();
        runner
            .run(
                &sample_spec(true),
                InvocationMode::UseSudo,
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(runner.calls()[0].mode, InvocationMode::UseSudo);
    }

    #[test]
    fn read_only_mode_rejects_privileged_action() {
        let runner = StdCommandRunner::default();
        let error = runner
            .run(
                &sample_spec(true),
                InvocationMode::ReadOnlyOnly,
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(matches!(error, Error::SudoUnavailable));
    }
}
