//! Provider-neutral domain invariants for Weft.

mod artifact;
mod change;
mod storage;

pub use artifact::{ArtifactError, FileMode, PathOperation, TreeDelta, sha256_digest};
pub use change::{
    AssignmentId, BaseState, CandidateId, CanonicalArtifact, Change, ChangeError, ChangeId,
    ChangeRevision, IntegrationId, IntegrationReceiptId, MaterializationId, NewRevision,
    OperationId, RepositoryId, ReviewRequestId, ReviewSubmissionId, RevisionId, ValidationResultId,
    WorkspaceId,
};
pub use storage::{
    Assignment, AuditEvent, CandidateInput, CompositionCandidate, ContentStore, Dependency,
    IntegrationAttempt, IntegrationReceipt, IntegrationState, Lease, Materialization,
    MaterializationState, ReviewOutcome, ReviewRequest, ReviewSubmission, SqliteRepository,
    StorageError, StoredChange, Target, ValidationResult, ValidationStatus,
};
