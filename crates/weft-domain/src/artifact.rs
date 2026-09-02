use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Component, Path};

use sha2::{Digest, Sha256};

/// The only canonical artifact version accepted by the initial domain kernel.
pub const TREE_DELTA_V1: &str = "tree-delta-v1";

const MANIFEST_MAGIC: &[u8] = b"weft/tree-delta-v1\0";
const MAX_OPERATIONS: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileMode {
    Regular,
    Executable,
    SymbolicLink,
}

impl FileMode {
    fn encoded(self) -> u8 {
        match self {
            Self::Regular => 1,
            Self::Executable => 2,
            Self::SymbolicLink => 3,
        }
    }

    fn decode(value: u8) -> Result<Self, ArtifactError> {
        match value {
            1 => Ok(Self::Regular),
            2 => Ok(Self::Executable),
            3 => Ok(Self::SymbolicLink),
            _ => Err(ArtifactError::MalformedEncoding("unknown file mode")),
        }
    }
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
    operations: Vec<PathOperation>,
}

impl TreeDelta {
    /// Creates a validated canonical manifest.
    ///
    /// Operations are strictly ordered by their UTF-8 repository paths. The
    /// binary encoding below is the sole digest input; provider traversal never
    /// influences canonical artifact identity.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty, unsafe, duplicate, unsorted, or
    /// content-less operation.
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

        Ok(Self { operations })
    }

    #[must_use]
    pub const fn version(&self) -> &'static str {
        TREE_DELTA_V1
    }

    #[must_use]
    pub fn operations(&self) -> &[PathOperation] {
        &self.operations
    }

    /// Returns the deterministic bytes used by canonical artifact storage.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(MANIFEST_MAGIC.len() + self.operations.len() * 96);
        bytes.extend_from_slice(MANIFEST_MAGIC);
        write_u64(&mut bytes, self.operations.len() as u64);
        for operation in &self.operations {
            match operation {
                PathOperation::Upsert {
                    path,
                    mode,
                    blob_digest,
                } => {
                    bytes.push(1);
                    write_string(&mut bytes, path);
                    bytes.push(mode.encoded());
                    write_string(&mut bytes, blob_digest);
                }
                PathOperation::Delete { path } => {
                    bytes.push(2);
                    write_string(&mut bytes, path);
                }
            }
        }
        bytes
    }

    /// Reopens a manifest only after validating its canonical bytes.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed bytes or an invalid manifest.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ArtifactError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.take(MANIFEST_MAGIC.len())? != MANIFEST_MAGIC {
            return Err(ArtifactError::MalformedEncoding("incorrect manifest magic"));
        }
        let count = cursor.u64()?;
        let count = usize::try_from(count)
            .map_err(|_| ArtifactError::MalformedEncoding("operation count overflows usize"))?;
        if count > MAX_OPERATIONS {
            return Err(ArtifactError::MalformedEncoding(
                "operation count exceeds the safety limit",
            ));
        }
        let mut operations = Vec::with_capacity(count);
        for _ in 0..count {
            match cursor.byte()? {
                1 => operations.push(PathOperation::Upsert {
                    path: cursor.string()?,
                    mode: FileMode::decode(cursor.byte()?)?,
                    blob_digest: cursor.string()?,
                }),
                2 => operations.push(PathOperation::Delete {
                    path: cursor.string()?,
                }),
                _ => return Err(ArtifactError::MalformedEncoding("unknown path operation")),
            }
        }
        if !cursor.is_at_end() {
            return Err(ArtifactError::MalformedEncoding("trailing manifest bytes"));
        }
        Self::new(operations)
    }
}

fn validate_path(value: &str) -> Result<(), ArtifactError> {
    if value.is_empty()
        || value.contains(['\\', '\0'])
        || value.chars().any(char::is_control)
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
    if path
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .is_some_and(|first| first.eq_ignore_ascii_case(".git"))
    {
        return Err(ArtifactError::ReservedPath(value.to_owned()));
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

#[must_use]
pub fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn write_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn write_string(bytes: &mut Vec<u8>, value: &str) {
    write_u64(bytes, value.len() as u64);
    bytes.extend_from_slice(value.as_bytes());
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ArtifactError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(ArtifactError::MalformedEncoding("length overflow"))?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or(ArtifactError::MalformedEncoding("truncated manifest"))?;
        self.position = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8, ArtifactError> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64, ArtifactError> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| ArtifactError::MalformedEncoding("invalid integer"))?;
        Ok(u64::from_be_bytes(bytes))
    }

    fn string(&mut self) -> Result<String, ArtifactError> {
        let length = usize::try_from(self.u64()?)
            .map_err(|_| ArtifactError::MalformedEncoding("string length overflows usize"))?;
        let bytes = self.take(length)?;
        String::from_utf8(bytes.to_owned())
            .map_err(|_| ArtifactError::MalformedEncoding("path is not valid UTF-8"))
    }

    const fn is_at_end(&self) -> bool {
        self.position == self.bytes.len()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactError {
    EmptyManifest,
    InvalidPath(String),
    ReservedPath(String),
    DuplicatePath(String),
    NonCanonicalOrder { previous: String, current: String },
    InvalidBlobDigest(String),
    MalformedEncoding(&'static str),
}

impl Display for ArtifactError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyManifest => formatter.write_str("tree delta has no operations"),
            Self::InvalidPath(path) => {
                write!(formatter, "invalid repository-relative path: {path}")
            }
            Self::ReservedPath(path) => write!(formatter, "reserved repository path: {path}"),
            Self::DuplicatePath(path) => write!(formatter, "duplicate tree delta path: {path}"),
            Self::NonCanonicalOrder { previous, current } => write!(
                formatter,
                "tree delta paths are not strictly sorted: {previous} before {current}"
            ),
            Self::InvalidBlobDigest(path) => {
                write!(formatter, "invalid SHA-256 blob digest for path: {path}")
            }
            Self::MalformedEncoding(message) => {
                write!(formatter, "malformed tree delta encoding: {message}")
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
    fn canonical_bytes_round_trip_and_are_stable() {
        let delta = TreeDelta::new(vec![
            upsert("a.txt"),
            PathOperation::Delete {
                path: "old.txt".to_owned(),
            },
            PathOperation::Upsert {
                path: "src/link".to_owned(),
                mode: FileMode::SymbolicLink,
                blob_digest: format!("sha256:{}", "b".repeat(64)),
            },
        ])
        .unwrap();
        let bytes = delta.canonical_bytes();
        assert_eq!(TreeDelta::from_canonical_bytes(&bytes).unwrap(), delta);
        assert_eq!(delta.canonical_bytes(), bytes);
    }

    #[test]
    fn rejects_non_canonical_or_reserved_paths() {
        assert!(matches!(
            TreeDelta::new(vec![upsert("z.txt"), upsert("a.txt")]),
            Err(ArtifactError::NonCanonicalOrder { .. })
        ));
        assert!(matches!(
            TreeDelta::new(vec![upsert(".git/config")]),
            Err(ArtifactError::ReservedPath(_))
        ));
        assert!(matches!(
            TreeDelta::new(vec![upsert("../secret")]),
            Err(ArtifactError::InvalidPath(_))
        ));
    }

    #[test]
    fn rejects_duplicate_or_contentless_operations() {
        assert!(matches!(
            TreeDelta::new(vec![upsert("a.txt"), upsert("a.txt")]),
            Err(ArtifactError::DuplicatePath(_))
        ));
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

    #[test]
    fn rejects_an_unbounded_operation_count_before_allocation() {
        let mut bytes = MANIFEST_MAGIC.to_vec();
        bytes.extend_from_slice(&u64::MAX.to_be_bytes());
        assert!(matches!(
            TreeDelta::from_canonical_bytes(&bytes),
            Err(ArtifactError::MalformedEncoding(
                "operation count exceeds the safety limit"
            ))
        ));
    }
}
