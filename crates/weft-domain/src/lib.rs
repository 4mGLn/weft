//! Provider-neutral domain invariants for Weft.

mod artifact;
mod change;
mod composition;
mod coordination;
mod integration;
mod materialization;
mod relationship;
mod review;

pub use artifact::{ArtifactError, FileMode, PathOperation, TREE_DELTA_V1, TreeDelta};
pub use change::{
    ActorId, ArtifactRef, BaseState, Change, ChangeError, ChangeId, ChangeRevision, NewRevision,
    RepositoryId, RevisionId, UnixMillis,
};
pub use composition::{
    CandidateDigest, CandidateId, CandidateInput, CandidateStackRef, CompositionCandidate,
    CompositionError, ResolvedRequirement, ResolvedRequirementSource, Stack, StackDefinition,
    StackId, StackMember, StackPolicy, StackVersion,
};
pub use coordination::{
    Assignment, AssignmentId, AssignmentRole, CoordinationError, CoordinationVersion, Lease,
    LeaseId, LeaseOperation, LeaseScope, LeaseStatus, Subject, SubjectId, SubjectKind,
};
pub use integration::{
    ConflictResolution, ConflictResolutionId, EffectOperationId, ExecutionLease, ExecutionLeaseId,
    GatePolicyEvidence, IntegrationAttempt, IntegrationBinding, IntegrationCapabilityEvidence,
    IntegrationConflict, IntegrationConflictId, IntegrationError, IntegrationEvidence,
    IntegrationGate, IntegrationId, IntegrationIntent, IntegrationMethod, IntegrationReceipt,
    IntegrationReceiptId, IntegrationState, IntegrationStrategy, IntegrationTarget,
    IntegrationVersion, ReconciliationId, ReconciliationObservation, ReconciliationOutcome,
    TargetObservation, TargetRef, TargetRevision,
};
pub use materialization::{
    Materialization, MaterializationError, MaterializationId, MaterializationPlacement,
    MaterializationState, MaterializationVersion, ProviderEvidence, ProviderId,
    ProviderObservation, ProviderRef, WorkspaceId,
};
pub use relationship::{
    Dependency, DependencyFreshness, DependencyId, DependencyPins, Relationship,
    RelationshipEndpoints, RelationshipError, RelationshipId, RelationshipKind,
    RelationshipVersion,
};
pub use review::{
    ExactTarget, ReviewError, ReviewOutcome, ReviewRequest, ReviewRequestId, ReviewReusePolicy,
    ReviewSubmission, ReviewSubmissionId, ValidationEnvironment, ValidationExecutionId,
    ValidationObservation, ValidationOutcome, ValidationResult, ValidationResultId,
    ValidationScope, ValidationType,
};
