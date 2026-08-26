use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::artifact::{TREE_DELTA_V1, is_sha256_digest};

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates a non-empty domain identifier.
            ///
            /// # Errors
            ///
            /// Returns [`ChangeError::EmptyIdentifier`] for an empty or
            /// whitespace-only value.
            pub fn new(value: impl Into<String>) -> Result<Self, ChangeError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(ChangeError::EmptyIdentifier(stringify!($name)));
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseState {
    repository_id: RepositoryId,
    object_id: String,
}

impl BaseState {
    /// Creates an exact provider-addressable base state.
    ///
    /// # Errors
    ///
    /// Returns [`ChangeError::EmptyBaseObject`] when the object identity is empty.
    pub fn new(
        repository_id: RepositoryId,
        object_id: impl Into<String>,
    ) -> Result<Self, ChangeError> {
        let object_id = object_id.into();
        if object_id.trim().is_empty() {
            return Err(ChangeError::EmptyBaseObject);
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactRef {
    version: &'static str,
    manifest_digest: String,
}

impl ArtifactRef {
    /// Creates a reference to a validated `tree-delta-v1` manifest.
    ///
    /// # Errors
    ///
    /// Returns [`ChangeError::InvalidArtifactDigest`] unless the digest is a
    /// lowercase `sha256:` value containing exactly 64 hexadecimal digits.
    pub fn tree_delta_v1(manifest_digest: impl Into<String>) -> Result<Self, ChangeError> {
        let manifest_digest = manifest_digest.into();
        if !is_sha256_digest(&manifest_digest) {
            return Err(ChangeError::InvalidArtifactDigest);
        }
        Ok(Self {
            version: TREE_DELTA_V1,
            manifest_digest,
        })
    }

    #[must_use]
    pub const fn version(&self) -> &'static str {
        self.version
    }

    #[must_use]
    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRevision {
    revision_id: RevisionId,
    base: BaseState,
    artifact: ArtifactRef,
}

impl NewRevision {
    #[must_use]
    pub const fn new(revision_id: RevisionId, base: BaseState, artifact: ArtifactRef) -> Self {
        Self {
            revision_id,
            base,
            artifact,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeRevision {
    revision_id: RevisionId,
    change_id: ChangeId,
    parent_revision_id: Option<RevisionId>,
    base: BaseState,
    artifact: ArtifactRef,
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
    pub const fn base(&self) -> &BaseState {
        &self.base
    }

    #[must_use]
    pub const fn artifact(&self) -> &ArtifactRef {
        &self.artifact
    }
}

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

    /// Atomically appends to the logical revision sequence when the caller's
    /// expected head matches the current head.
    ///
    /// # Errors
    ///
    /// Returns [`ChangeError`] when the expected head is stale, the revision ID
    /// already exists, or the base/canonical artifact reference is invalid.
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
            base: new_revision.base,
            artifact: new_revision.artifact,
        };
        self.revisions.push(revision);
        self.head = Some(new_revision.revision_id);
        self.revisions.last().ok_or(ChangeError::InvariantViolation(
            "appended revision is missing",
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChangeError {
    EmptyIdentifier(&'static str),
    EmptyBaseObject,
    InvalidArtifactDigest,
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
            Self::EmptyIdentifier(kind) => write!(formatter, "{kind} cannot be empty"),
            Self::EmptyBaseObject => formatter.write_str("base object identity cannot be empty"),
            Self::InvalidArtifactDigest => formatter.write_str("invalid SHA-256 artifact digest"),
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

    fn revision(id: &str) -> NewRevision {
        NewRevision::new(
            RevisionId::new(id).unwrap(),
            BaseState::new(RepositoryId::new("repo-1").unwrap(), "base-object").unwrap(),
            ArtifactRef::tree_delta_v1(format!("sha256:{}", "a".repeat(64))).unwrap(),
        )
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
    fn rejects_duplicate_revision_identity() {
        let mut change = Change::new(ChangeId::new("change-1").unwrap());
        change.append_revision(None, revision("rev-1")).unwrap();
        let head = RevisionId::new("rev-1").unwrap();
        let error = change
            .append_revision(Some(&head), revision("rev-1"))
            .unwrap_err();
        assert!(matches!(error, ChangeError::DuplicateRevision(_)));
    }

    #[test]
    fn rejects_invalid_base_and_artifact_identity() {
        assert!(matches!(
            BaseState::new(RepositoryId::new("repo-1").unwrap(), " "),
            Err(ChangeError::EmptyBaseObject)
        ));
        assert!(matches!(
            ArtifactRef::tree_delta_v1("provider-object"),
            Err(ChangeError::InvalidArtifactDigest)
        ));
    }
}
