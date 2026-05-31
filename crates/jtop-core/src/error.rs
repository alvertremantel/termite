use std::{fmt, io, path::PathBuf, time::Duration};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    CommandNotFound {
        program: String,
    },
    CommandTimeout {
        program: String,
        timeout: Duration,
    },
    NonZeroExit {
        program: String,
        code: Option<i32>,
        stderr: String,
    },
    ParseFailure {
        context: String,
        details: String,
    },
    PermissionDenied {
        context: String,
    },
    SudoUnavailable,
    RejectedPathOperation {
        path: PathBuf,
        reason: String,
    },
    Io {
        context: String,
        details: String,
    },
}

impl Error {
    pub fn io(context: impl Into<String>, error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::PermissionDenied => Self::PermissionDenied {
                context: context.into(),
            },
            _ => Self::Io {
                context: context.into(),
                details: error.to_string(),
            },
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandNotFound { program } => write!(f, "command not found: {program}"),
            Self::CommandTimeout { program, timeout } => {
                write!(f, "command timed out after {:?}: {program}", timeout)
            }
            Self::NonZeroExit {
                program,
                code,
                stderr,
            } => {
                if stderr.trim().is_empty() {
                    write!(f, "command failed with exit code {:?}: {program}", code)
                } else {
                    write!(
                        f,
                        "command failed with exit code {:?}: {program} — {}",
                        code,
                        stderr.trim()
                    )
                }
            }
            Self::ParseFailure { context, details } => {
                write!(f, "could not parse {context}: {details}")
            }
            Self::PermissionDenied { context } => write!(f, "permission denied: {context}"),
            Self::SudoUnavailable => write!(
                f,
                "sudo is not ready; run `sudo -v` before launching jtop or run it via sudo"
            ),
            Self::RejectedPathOperation { path, reason } => {
                write!(
                    f,
                    "rejected path operation for {}: {reason}",
                    path.display()
                )
            }
            Self::Io { context, details } => write!(f, "i/o error during {context}: {details}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::io("i/o operation", value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_messages_are_friendly() {
        let error = Error::NonZeroExit {
            program: "tlp".into(),
            code: Some(2),
            stderr: "bad option".into(),
        };
        assert_eq!(
            error.to_string(),
            "command failed with exit code Some(2): tlp — bad option"
        );
    }

    #[test]
    fn io_permission_denied_becomes_permission_error() {
        let error = Error::io(
            "scan /etc/tlp.d",
            io::Error::from(io::ErrorKind::PermissionDenied),
        );
        assert_eq!(error.to_string(), "permission denied: scan /etc/tlp.d");
    }

    #[test]
    fn generic_io_conversion_keeps_context() {
        let error: Error = io::Error::from(io::ErrorKind::TimedOut).into();
        assert!(error.to_string().contains("i/o error during i/o operation"));
    }
}
