use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Component, Path};

/// The only canonical artifact version accepted by the initial domain kernel.
pub const TREE_DELTA_V1: &str = "tree-delta-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileMode {
    Regular,
    Executable,
    SymbolicLink,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathOperation {
    Upsert {
        path: String,
        mode: FileMode,
        blob_digest: String,
    },
    Delete {
        path: String,
    },
}

impl PathOperation {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Upsert { path, .. } | Self::Delete { path } => path,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeDelta {
    version: &'static str,
    operations: Vec<PathOperation>,
}

impl TreeDelta {
    /// Creates a validated canonical manifest.
    ///
    /// Operations must be sorted by path and paths must be unique. Enforcing
    /// canonical order makes manifest digests independent of provider traversal.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactError`] when the manifest is empty, paths are unsafe,
    /// duplicated or unsorted, or an upsert lacks a blob digest.
    pub fn new(operations: Vec<PathOperation>) -> Result<Self, ArtifactError> {
        if operations.is_empty() {
            return Err(ArtifactError::EmptyManifest);
        }

        let mut previous: Option<&str> = None;
        let mut paths = HashSet::with_capacity(operations.len());
        for operation in &operations {
            let path = operation.path();
            validate_path(path)?;
            if !paths.insert(path) {
                return Err(ArtifactError::DuplicatePath(path.to_owned()));
            }
            if previous.is_some_and(|prior| prior >= path) {
                return Err(ArtifactError::NonCanonicalOrder {
                    previous: previous.unwrap_or_default().to_owned(),
                    current: path.to_owned(),
                });
            }
            if let PathOperation::Upsert { blob_digest, .. } = operation
                && !is_sha256_digest(blob_digest)
            {
                return Err(ArtifactError::InvalidBlobDigest(path.to_owned()));
            }
            previous = Some(path);
        }

        Ok(Self {
            version: TREE_DELTA_V1,
            operations,
        })
    }

    #[must_use]
    pub const fn version(&self) -> &'static str {
        self.version
    }

    #[must_use]
    pub fn operations(&self) -> &[PathOperation] {
        &self.operations
    }
}

fn validate_path(value: &str) -> Result<(), ArtifactError> {
    if value.is_empty()
        || value.contains(['\\', '\0'])
        || value.contains("//")
        || value.ends_with('/')
    {
        return Err(ArtifactError::InvalidPath(value.to_owned()));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ArtifactError::InvalidPath(value.to_owned()));
    }
    Ok(())
}

pub(crate) fn is_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactError {
    EmptyManifest,
    InvalidPath(String),
    DuplicatePath(String),
    NonCanonicalOrder { previous: String, current: String },
    InvalidBlobDigest(String),
}

impl Display for ArtifactError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyManifest => formatter.write_str("tree delta has no operations"),
            Self::InvalidPath(path) => {
                write!(formatter, "invalid repository-relative path: {path}")
            }
            Self::DuplicatePath(path) => write!(formatter, "duplicate tree delta path: {path}"),
            Self::NonCanonicalOrder { previous, current } => write!(
                formatter,
                "tree delta paths are not strictly sorted: {previous} before {current}"
            ),
            Self::InvalidBlobDigest(path) => {
                write!(formatter, "invalid SHA-256 blob digest for path: {path}")
            }
        }
    }
}

impl Error for ArtifactError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn upsert(path: &str) -> PathOperation {
        PathOperation::Upsert {
            path: path.to_owned(),
            mode: FileMode::Regular,
            blob_digest: format!("sha256:{}", "a".repeat(64)),
        }
    }

    #[test]
    fn accepts_sorted_unique_repository_paths() {
        let delta = TreeDelta::new(vec![upsert("a.txt"), upsert("src/lib.rs")]).unwrap();
        assert_eq!(delta.version(), TREE_DELTA_V1);
        assert_eq!(delta.operations().len(), 2);
    }

    #[test]
    fn rejects_non_canonical_order() {
        let error = TreeDelta::new(vec![upsert("z.txt"), upsert("a.txt")]).unwrap_err();
        assert!(matches!(error, ArtifactError::NonCanonicalOrder { .. }));
    }

    #[test]
    fn rejects_duplicate_and_traversal_paths() {
        assert!(matches!(
            TreeDelta::new(vec![upsert("a.txt"), upsert("a.txt")]),
            Err(ArtifactError::DuplicatePath(_))
        ));
        assert!(matches!(
            TreeDelta::new(vec![upsert("../secret")]),
            Err(ArtifactError::InvalidPath(_))
        ));
        assert!(matches!(
            TreeDelta::new(vec![upsert("src//lib.rs")]),
            Err(ArtifactError::InvalidPath(_))
        ));
    }

    #[test]
    fn rejects_missing_blob_content_identity() {
        let operation = PathOperation::Upsert {
            path: "a.txt".to_owned(),
            mode: FileMode::Executable,
            blob_digest: String::new(),
        };
        assert!(matches!(
            TreeDelta::new(vec![operation]),
            Err(ArtifactError::InvalidBlobDigest(_))
        ));
    }
}
