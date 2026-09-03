//! Provider-neutral domain invariants for Weft.

mod artifact;
mod change;
mod storage;

pub use artifact::{ArtifactError, FileMode, PathOperation, TreeDelta, sha256_digest};
pub use change::{
    AssignmentId, BaseState, CandidateId, CanonicalArtifact, Change, ChangeError, ChangeId,
    ChangeRevision, ConflictId, IntegrationId, IntegrationReceiptId, MaterializationId,
    NewRevision, OperationId, OverlapId, ReconciliationId, RepositoryId, ReuseDecisionId,
    ReviewRequestId, ReviewSubmissionId, RevisionId, StackId, ValidationResultId, WorkspaceId,
};
pub use storage::{
    Assignment, AuditContext, AuditEvent, CandidateInput, ChangeRelationKind, CompositionCandidate,
    ContentStore, Dependency, DomainEvent, IntegrationAttempt, IntegrationConflict,
    IntegrationReceipt, IntegrationState, Lease, Materialization, MaterializationState, Overlap,
    ReconciliationRecord, ReuseDecision, ReuseEvidenceKind, ReviewOutcome, ReviewRequest,
    ReviewSubmission, SqliteRepository, StackVersion, StorageError, StoredChange, Target,
    ValidationResult, ValidationStatus,
};
