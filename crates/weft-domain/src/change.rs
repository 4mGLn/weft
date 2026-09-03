use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::artifact::{ArtifactError, TreeDelta, sha256_digest};

const ARTIFACT_MAGIC: &[u8] = b"weft/canonical-artifact-v1\0";
const MAX_IDENTIFIER_BYTES: usize = 512;
const MAX_BASE_OBJECT_BYTES: usize = 4_096;

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates a safe, non-empty domain identifier.
            ///
            /// # Errors
            ///
            /// Returns an error for blank, padded, or control-character input.
            pub fn new(value: impl Into<String>) -> Result<Self, ChangeError> {
                let value = value.into();
                if value.trim().is_empty()
                    || value != value.trim()
                    || value.chars().any(char::is_control)
                    || value.len() > MAX_IDENTIFIER_BYTES
                {
                    return Err(ChangeError::InvalidIdentifier(stringify!($name)));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

domain_id!(RepositoryId);
domain_id!(ChangeId);
domain_id!(RevisionId);
domain_id!(CandidateId);
domain_id!(AssignmentId);
domain_id!(MaterializationId);
domain_id!(WorkspaceId);
domain_id!(ReviewRequestId);
domain_id!(ReviewSubmissionId);
domain_id!(ValidationResultId);
domain_id!(IntegrationId);
domain_id!(OperationId);
domain_id!(IntegrationReceiptId);
domain_id!(StackId);
domain_id!(ConflictId);
domain_id!(ReconciliationId);
domain_id!(OverlapId);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseState {
    repository_id: RepositoryId,
    object_id: String,
}

impl BaseState {
    /// Creates an exact, provider-addressable base state.
    ///
    /// # Errors
    ///
    /// Returns an error for blank, padded, or control-character object IDs.
    pub fn new(
        repository_id: RepositoryId,
        object_id: impl Into<String>,
    ) -> Result<Self, ChangeError> {
        let object_id = object_id.into();
        if object_id.trim().is_empty()
            || object_id != object_id.trim()
            || object_id.chars().any(char::is_control)
            || object_id.len() > MAX_BASE_OBJECT_BYTES
        {
            return Err(ChangeError::InvalidBaseObject);
        }
        Ok(Self {
            repository_id,
            object_id,
        })
    }

    #[must_use]
    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    #[must_use]
    pub fn object_id(&self) -> &str {
        &self.object_id
    }
}

/// Canonical revision content that binds an exact base to a validated tree
/// delta. Its digest is computed from its deterministic encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalArtifact {
    base: BaseState,
    tree_delta: TreeDelta,
    digest: String,
}

impl CanonicalArtifact {
    #[must_use]
    pub fn new(base: BaseState, tree_delta: TreeDelta) -> Self {
        let mut artifact = Self {
            base,
            tree_delta,
            digest: String::new(),
        };
        artifact.digest = sha256_digest(&artifact.canonical_bytes());
        artifact
    }

    #[must_use]
    pub fn base(&self) -> &BaseState {
        &self.base
    }

    #[must_use]
    pub fn tree_delta(&self) -> &TreeDelta {
        &self.tree_delta
    }

    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let manifest = self.tree_delta.canonical_bytes();
        let mut bytes = Vec::with_capacity(
            ARTIFACT_MAGIC.len()
                + self.base.repository_id.as_str().len()
                + self.base.object_id.len()
                + manifest.len()
                + 24,
        );
        bytes.extend_from_slice(ARTIFACT_MAGIC);
        write_string(&mut bytes, self.base.repository_id.as_str());
        write_string(&mut bytes, &self.base.object_id);
        write_bytes(&mut bytes, &manifest);
        bytes
    }

    /// Reopens canonical artifact bytes after validating their structure.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed bytes or invalid embedded content.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ChangeError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.take(ARTIFACT_MAGIC.len())? != ARTIFACT_MAGIC {
            return Err(ChangeError::MalformedArtifact("incorrect artifact magic"));
        }
        let repository_id = RepositoryId::new(cursor.string()?)?;
        let base = BaseState::new(repository_id, cursor.string()?)?;
        let tree_delta = TreeDelta::from_canonical_bytes(cursor.bytes()?)
            .map_err(ChangeError::InvalidManifest)?;
        if !cursor.is_at_end() {
            return Err(ChangeError::MalformedArtifact("trailing artifact bytes"));
        }
        Ok(Self::new(base, tree_delta))
    }

    /// Reopens canonical artifact bytes only if their address matches.
    ///
    /// # Errors
    ///
    /// Returns an error for a digest mismatch, malformed bytes, or invalid
    /// embedded content.
    pub fn from_canonical_bytes_with_digest(
        bytes: &[u8],
        expected_digest: &str,
    ) -> Result<Self, ChangeError> {
        if sha256_digest(bytes) != expected_digest {
            return Err(ChangeError::ArtifactDigestMismatch);
        }
        Self::from_canonical_bytes(bytes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRevision {
    revision_id: RevisionId,
    artifact: CanonicalArtifact,
}

impl NewRevision {
    #[must_use]
    pub const fn new(revision_id: RevisionId, artifact: CanonicalArtifact) -> Self {
        Self {
            revision_id,
            artifact,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeRevision {
    revision_id: RevisionId,
    change_id: ChangeId,
    parent_revision_id: Option<RevisionId>,
    artifact: CanonicalArtifact,
}

impl ChangeRevision {
    #[must_use]
    pub fn revision_id(&self) -> &RevisionId {
        &self.revision_id
    }

    #[must_use]
    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub fn parent_revision_id(&self) -> Option<&RevisionId> {
        self.parent_revision_id.as_ref()
    }

    #[must_use]
    pub fn artifact(&self) -> &CanonicalArtifact {
        &self.artifact
    }
}

/// An in-memory projection useful for callers that already serialize mutation.
/// Durable multi-process compare-and-swap is provided by `SqliteRepository`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
    id: ChangeId,
    head: Option<RevisionId>,
    revisions: Vec<ChangeRevision>,
}

impl Change {
    #[must_use]
    pub const fn new(id: ChangeId) -> Self {
        Self {
            id,
            head: None,
            revisions: Vec::new(),
        }
    }

    #[must_use]
    pub fn id(&self) -> &ChangeId {
        &self.id
    }

    #[must_use]
    pub fn head(&self) -> Option<&RevisionId> {
        self.head.as_ref()
    }

    #[must_use]
    pub fn revisions(&self) -> &[ChangeRevision] {
        &self.revisions
    }

    /// # Errors
    ///
    /// Returns an error for a stale head or duplicate revision ID.
    pub fn append_revision(
        &mut self,
        expected_head: Option<&RevisionId>,
        new_revision: NewRevision,
    ) -> Result<&ChangeRevision, ChangeError> {
        if expected_head != self.head.as_ref() {
            return Err(ChangeError::StaleHead {
                expected: expected_head.cloned(),
                actual: self.head.clone(),
            });
        }
        if self
            .revisions
            .iter()
            .any(|revision| revision.revision_id == new_revision.revision_id)
        {
            return Err(ChangeError::DuplicateRevision(new_revision.revision_id));
        }
        let revision = ChangeRevision {
            revision_id: new_revision.revision_id.clone(),
            change_id: self.id.clone(),
            parent_revision_id: self.head.clone(),
            artifact: new_revision.artifact,
        };
        self.revisions.push(revision);
        self.head = Some(new_revision.revision_id);
        self.revisions.last().ok_or(ChangeError::InvariantViolation(
            "appended revision is missing",
        ))
    }
}

fn write_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn write_string(bytes: &mut Vec<u8>, value: &str) {
    write_bytes(bytes, value.as_bytes());
}

fn write_bytes(output: &mut Vec<u8>, value: &[u8]) {
    write_u64(output, value.len() as u64);
    output.extend_from_slice(value);
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ChangeError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(ChangeError::MalformedArtifact("length overflow"))?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(ChangeError::MalformedArtifact("truncated artifact"))?;
        self.position = end;
        Ok(value)
    }

    fn u64(&mut self) -> Result<u64, ChangeError> {
        let value: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| ChangeError::MalformedArtifact("invalid length"))?;
        Ok(u64::from_be_bytes(value))
    }

    fn bytes(&mut self) -> Result<&'a [u8], ChangeError> {
        let length = usize::try_from(self.u64()?)
            .map_err(|_| ChangeError::MalformedArtifact("length overflows usize"))?;
        self.take(length)
    }

    fn string(&mut self) -> Result<String, ChangeError> {
        String::from_utf8(self.bytes()?.to_owned())
            .map_err(|_| ChangeError::MalformedArtifact("string is not valid UTF-8"))
    }

    const fn is_at_end(&self) -> bool {
        self.position == self.bytes.len()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChangeError {
    InvalidIdentifier(&'static str),
    InvalidBaseObject,
    InvalidManifest(ArtifactError),
    ArtifactDigestMismatch,
    MalformedArtifact(&'static str),
    DuplicateRevision(RevisionId),
    StaleHead {
        expected: Option<RevisionId>,
        actual: Option<RevisionId>,
    },
    InvariantViolation(&'static str),
}

impl Display for ChangeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier(kind) => write!(formatter, "invalid {kind}"),
            Self::InvalidBaseObject => formatter.write_str("invalid base object identity"),
            Self::InvalidManifest(error) => {
                write!(formatter, "invalid canonical manifest: {error}")
            }
            Self::ArtifactDigestMismatch => {
                formatter.write_str("canonical artifact digest mismatch")
            }
            Self::MalformedArtifact(message) => {
                write!(formatter, "malformed canonical artifact: {message}")
            }
            Self::DuplicateRevision(id) => write!(formatter, "duplicate revision: {}", id.as_str()),
            Self::StaleHead { expected, actual } => write!(
                formatter,
                "stale revision head: expected {}, actual {}",
                display_optional_id(expected.as_ref()),
                display_optional_id(actual.as_ref())
            ),
            Self::InvariantViolation(message) => {
                write!(formatter, "domain invariant failed: {message}")
            }
        }
    }
}

fn display_optional_id(id: Option<&RevisionId>) -> &str {
    id.map_or("<none>", RevisionId::as_str)
}

impl Error for ChangeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileMode, PathOperation};

    fn artifact() -> CanonicalArtifact {
        let blob = sha256_digest(b"binary\0content");
        CanonicalArtifact::new(
            BaseState::new(RepositoryId::new("repo-1").unwrap(), "git:012345").unwrap(),
            TreeDelta::new(vec![PathOperation::Upsert {
                path: "bin/data".to_owned(),
                mode: FileMode::Executable,
                blob_digest: blob,
            }])
            .unwrap(),
        )
    }

    fn revision(id: &str) -> NewRevision {
        NewRevision::new(RevisionId::new(id).unwrap(), artifact())
    }

    #[test]
    fn canonical_artifact_binds_base_and_round_trips() {
        let artifact = artifact();
        let bytes = artifact.canonical_bytes();
        assert_eq!(
            CanonicalArtifact::from_canonical_bytes(&bytes).unwrap(),
            artifact
        );
        assert_eq!(artifact.digest(), sha256_digest(&bytes));
    }

    #[test]
    fn rejects_tampered_artifact() {
        let artifact = artifact();
        let mut bytes = artifact.canonical_bytes();
        *bytes.last_mut().unwrap() ^= 1;
        assert!(matches!(
            CanonicalArtifact::from_canonical_bytes_with_digest(&bytes, artifact.digest()),
            Err(ChangeError::ArtifactDigestMismatch)
        ));
    }

    #[test]
    fn creates_a_linear_revision_sequence() {
        let change_id = ChangeId::new("change-1").unwrap();
        let mut change = Change::new(change_id.clone());

        let first = change.append_revision(None, revision("rev-1")).unwrap();
        assert_eq!(first.change_id(), &change_id);
        assert_eq!(first.parent_revision_id(), None);

        let expected = RevisionId::new("rev-1").unwrap();
        let second = change
            .append_revision(Some(&expected), revision("rev-2"))
            .unwrap();
        assert_eq!(second.parent_revision_id(), Some(&expected));
        assert_eq!(change.head().map(RevisionId::as_str), Some("rev-2"));
    }

    #[test]
    fn rejects_a_stale_writer_without_mutation() {
        let mut change = Change::new(ChangeId::new("change-1").unwrap());
        change.append_revision(None, revision("rev-1")).unwrap();
        let stale = RevisionId::new("missing").unwrap();

        let error = change
            .append_revision(Some(&stale), revision("rev-2"))
            .unwrap_err();
        assert!(matches!(error, ChangeError::StaleHead { .. }));
        assert_eq!(change.revisions().len(), 1);
        assert_eq!(change.head().map(RevisionId::as_str), Some("rev-1"));
    }

    #[test]
    fn rejects_unsafe_identifier_and_base_values() {
        assert!(RepositoryId::new(" repo").is_err());
        assert!(ChangeId::new("change\n1").is_err());
        assert!(BaseState::new(RepositoryId::new("repo").unwrap(), "base\n1").is_err());
        assert!(ChangeId::new("x".repeat(MAX_IDENTIFIER_BYTES + 1)).is_err());
        assert!(
            BaseState::new(
                RepositoryId::new("repo").unwrap(),
                "x".repeat(MAX_BASE_OBJECT_BYTES + 1)
            )
            .is_err()
        );
    }
}
