use std::time::Duration;

use jtop_core::{ActionRisk, CommandRunner, CommandSpec, Error, InvocationMode, StdCommandRunner};

fn spec(program: &str, args: &[&str]) -> CommandSpec {
    CommandSpec {
        program: program.into(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        needs_root: false,
        risk: ActionRisk::Safe,
        description: format!("run {program}"),
    }
}

#[test]
fn runner_captures_stdout() {
    let runner = StdCommandRunner::default();
    let output = runner
        .run(
            &spec("printf", &["hello"]),
            InvocationMode::Direct,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(output.stdout, "hello");
}

#[test]
fn runner_reports_non_zero_exit() {
    let runner = StdCommandRunner::default();
    let error = runner
        .run(
            &spec("false", &[]),
            InvocationMode::Direct,
            Duration::from_secs(2),
        )
        .unwrap_err();
    assert!(matches!(error, Error::NonZeroExit { .. }));
}

#[test]
fn runner_times_out() {
    let runner = StdCommandRunner::default();
    let error = runner
        .run(
            &spec("sleep", &["2"]),
            InvocationMode::Direct,
            Duration::from_millis(50),
        )
        .unwrap_err();
    assert!(matches!(error, Error::CommandTimeout { .. }));
}
