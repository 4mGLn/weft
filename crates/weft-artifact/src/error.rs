use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::PathBuf;

use weft_domain::{ArtifactError, ChangeError};

use crate::CasDigest;

#[derive(Debug)]
pub enum ArtifactStoreError {
    Io(io::Error),
    DomainArtifact(ArtifactError),
    DomainChange(ChangeError),
    InvalidDigest(String),
    ObjectTooLarge {
        size: u64,
        limit: u64,
    },
    ObjectMissing(CasDigest),
    DigestMismatch {
        expected: CasDigest,
        actual: CasDigest,
    },
    InvalidObjectType(PathBuf),
    InvalidManifest(String),
    MissingReferencedBlob(CasDigest),
    BaseMismatch,
    DestinationExists(PathBuf),
    NonUtf8Path(PathBuf),
    UnsupportedFileType(PathBuf),
    StructuralConflict(String),
}

impl Display for ArtifactStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "artifact I/O error: {error}"),
            Self::DomainArtifact(error) => write!(formatter, "invalid tree delta: {error}"),
            Self::DomainChange(error) => write!(formatter, "invalid revision data: {error}"),
            Self::InvalidDigest(value) => write!(formatter, "invalid SHA-256 digest: {value}"),
            Self::ObjectTooLarge { size, limit } => {
                write!(
                    formatter,
                    "CAS object is {size} bytes; limit is {limit} bytes"
                )
            }
            Self::ObjectMissing(digest) => write!(formatter, "CAS object is missing: {digest}"),
            Self::DigestMismatch { expected, actual } => write!(
                formatter,
                "CAS object digest mismatch: expected {expected}, actual {actual}"
            ),
            Self::InvalidObjectType(path) => {
                write!(
                    formatter,
                    "CAS object is not a regular file: {}",
                    path.display()
                )
            }
            Self::InvalidManifest(message) => write!(formatter, "invalid manifest: {message}"),
            Self::MissingReferencedBlob(digest) => {
                write!(formatter, "manifest references missing blob: {digest}")
            }
            Self::BaseMismatch => {
                formatter.write_str("artifact base does not match requested base")
            }
            Self::DestinationExists(path) => {
                write!(
                    formatter,
                    "reconstruction destination exists: {}",
                    path.display()
                )
            }
            Self::NonUtf8Path(path) => {
                write!(
                    formatter,
                    "repository path is not UTF-8: {}",
                    path.display()
                )
            }
            Self::UnsupportedFileType(path) => {
                write!(
                    formatter,
                    "unsupported repository file type: {}",
                    path.display()
                )
            }
            Self::StructuralConflict(message) => {
                write!(formatter, "tree reconstruction conflict: {message}")
            }
        }
    }
}

impl Error for ArtifactStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::DomainArtifact(error) => Some(error),
            Self::DomainChange(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ArtifactStoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ArtifactError> for ArtifactStoreError {
    fn from(value: ArtifactError) -> Self {
        Self::DomainArtifact(value)
    }
}

impl From<ChangeError> for ArtifactStoreError {
    fn from(value: ChangeError) -> Self {
        Self::DomainChange(value)
    }
}
