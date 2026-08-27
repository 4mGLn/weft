use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum GitProviderError {
    Io(io::Error),
    Artifact(weft_artifact::ArtifactStoreError),
    Domain(String),
    CommandFailed {
        operation: &'static str,
        code: Option<i32>,
        redacted_stderr_bytes: usize,
    },
    CommandTimedOut {
        operation: &'static str,
    },
    OutputLimit {
        operation: &'static str,
    },
    InvalidOutput {
        operation: &'static str,
        reason: String,
    },
    Unsupported {
        capability: &'static str,
        reason: String,
    },
    RepositoryNotFound(PathBuf),
    UnsafeTargetRef(String),
    ChangedTarget {
        expected: String,
        observed: String,
    },
    Conflict {
        paths: Vec<String>,
        evidence: String,
    },
    DestinationExists(PathBuf),
    VerificationFailed(String),
}

impl Display for GitProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "native Git I/O error: {error}"),
            Self::Artifact(error) => write!(formatter, "canonical artifact error: {error}"),
            Self::Domain(reason) => write!(formatter, "domain value error: {reason}"),
            Self::CommandFailed {
                operation,
                code,
                redacted_stderr_bytes,
            } => {
                write!(
                    formatter,
                    "Git operation {operation} failed with status {code:?} ({redacted_stderr_bytes} redacted stderr bytes)"
                )
            }
            Self::CommandTimedOut { operation } => {
                write!(formatter, "Git operation {operation} exceeded its deadline")
            }
            Self::OutputLimit { operation } => {
                write!(
                    formatter,
                    "Git operation {operation} exceeded its output limit"
                )
            }
            Self::InvalidOutput { operation, reason } => {
                write!(
                    formatter,
                    "Git operation {operation} returned invalid output: {reason}"
                )
            }
            Self::Unsupported { capability, reason } => {
                write!(
                    formatter,
                    "unsupported Native Git capability {capability}: {reason}"
                )
            }
            Self::RepositoryNotFound(path) => {
                write!(
                    formatter,
                    "no Git repository was discovered at {}",
                    path.display()
                )
            }
            Self::UnsafeTargetRef(value) => {
                write!(formatter, "unsafe integration target ref: {value}")
            }
            Self::ChangedTarget { expected, observed } => write!(
                formatter,
                "integration target changed from expected {expected} to {observed}"
            ),
            Self::Conflict { paths, .. } => {
                write!(
                    formatter,
                    "Git operation conflicted on {} path(s)",
                    paths.len()
                )
            }
            Self::DestinationExists(path) => {
                write!(
                    formatter,
                    "materialization destination already exists: {}",
                    path.display()
                )
            }
            Self::VerificationFailed(reason) => {
                write!(formatter, "Native Git verification failed: {reason}")
            }
        }
    }
}

impl Error for GitProviderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Artifact(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for GitProviderError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<weft_artifact::ArtifactStoreError> for GitProviderError {
    fn from(value: weft_artifact::ArtifactStoreError) -> Self {
        Self::Artifact(value)
    }
}
