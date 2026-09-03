use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum GitButlerProviderError {
    Io(io::Error),
    Artifact(weft_artifact::ArtifactStoreError),
    NativeGit(weft_provider_git::GitProviderError),
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
    RepositoryMismatch,
    ChangedTarget {
        expected: String,
        observed: String,
    },
    StaleProviderState(String),
    VerificationFailed(String),
}

impl Display for GitButlerProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "GitButler adapter I/O error: {error}"),
            Self::Artifact(error) => write!(formatter, "canonical artifact error: {error}"),
            Self::NativeGit(error) => write!(formatter, "GitButler Git evidence error: {error}"),
            Self::Domain(reason) => write!(formatter, "domain value error: {reason}"),
            Self::CommandFailed {
                operation,
                code,
                redacted_stderr_bytes,
            } => write!(
                formatter,
                "GitButler operation {operation} failed with status {code:?} ({redacted_stderr_bytes} redacted stderr bytes)"
            ),
            Self::CommandTimedOut { operation } => {
                write!(
                    formatter,
                    "GitButler operation {operation} exceeded its deadline"
                )
            }
            Self::OutputLimit { operation } => write!(
                formatter,
                "GitButler operation {operation} exceeded its output limit"
            ),
            Self::InvalidOutput { operation, reason } => write!(
                formatter,
                "GitButler operation {operation} returned invalid output: {reason}"
            ),
            Self::Unsupported { capability, reason } => write!(
                formatter,
                "unsupported GitButler capability {capability}: {reason}"
            ),
            Self::RepositoryNotFound(path) => write!(
                formatter,
                "no GitButler project was discovered at {}",
                path.display()
            ),
            Self::RepositoryMismatch => {
                write!(
                    formatter,
                    "GitButler repository identity or locator changed"
                )
            }
            Self::ChangedTarget { expected, observed } => write!(
                formatter,
                "GitButler target changed from expected {expected} to {observed}"
            ),
            Self::StaleProviderState(reason) => {
                write!(formatter, "GitButler provider state changed: {reason}")
            }
            Self::VerificationFailed(reason) => {
                write!(formatter, "GitButler verification failed: {reason}")
            }
        }
    }
}

impl Error for GitButlerProviderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Artifact(error) => Some(error),
            Self::NativeGit(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for GitButlerProviderError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<weft_artifact::ArtifactStoreError> for GitButlerProviderError {
    fn from(value: weft_artifact::ArtifactStoreError) -> Self {
        Self::Artifact(value)
    }
}

impl From<weft_provider_git::GitProviderError> for GitButlerProviderError {
    fn from(value: weft_provider_git::GitProviderError) -> Self {
        Self::NativeGit(value)
    }
}
