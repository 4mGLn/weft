//! Provider-neutral domain invariants for Weft.

mod artifact;
mod change;
mod storage;

pub use artifact::{ArtifactError, FileMode, PathOperation, TreeDelta, sha256_digest};
pub use change::{
    AssignmentId, BaseState, CandidateId, CanonicalArtifact, Change, ChangeError, ChangeId,
    ChangeRevision, MaterializationId, NewRevision, RepositoryId, RevisionId, WorkspaceId,
};
pub use storage::{
    Assignment, AuditEvent, CandidateInput, CompositionCandidate, ContentStore, Dependency, Lease,
    Materialization, MaterializationState, SqliteRepository, StorageError, StoredChange,
};
