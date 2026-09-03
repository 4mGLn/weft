use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::artifact::{PathOperation, is_sha256_digest, sha256_digest};
use crate::{
    AssignmentId, BaseState, CandidateId, CanonicalArtifact, ChangeError, ChangeId, ConflictId,
    IntegrationId, IntegrationReceiptId, MaterializationId, OperationId, OverlapId,
    ReconciliationId, RepositoryId, ReviewRequestId, ReviewSubmissionId, RevisionId, StackId,
    ValidationResultId, WorkspaceId,
};

const SCHEMA_VERSION: i64 = 13;
static NEXT_TEMPORARY_OBJECT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct ContentStore {
    root: PathBuf,
}

impl ContentStore {
    /// # Errors
    ///
    /// Returns an error when the content-store directories cannot be created.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        fs::create_dir_all(root.join("blobs").join("sha256"))?;
        fs::create_dir_all(root.join("artifacts").join("sha256"))?;
        Ok(Self { root })
    }

    /// # Errors
    ///
    /// Returns an error when the blob cannot be durably written or verified.
    pub fn put_blob(&self, content: &[u8]) -> Result<String, StorageError> {
        let digest = sha256_digest(content);
        self.put_addressed("blobs", &digest, content)?;
        Ok(digest)
    }

    /// # Errors
    ///
    /// Returns an error for an invalid, missing, or mismatched digest.
    pub fn read_blob(&self, digest: &str) -> Result<Vec<u8>, StorageError> {
        self.read_addressed("blobs", digest)
    }

    /// # Errors
    ///
    /// Returns an error if any referenced blob is absent or cannot be verified.
    pub fn put_artifact(&self, artifact: &CanonicalArtifact) -> Result<(), StorageError> {
        for operation in artifact.tree_delta().operations() {
            if let PathOperation::Upsert { blob_digest, .. } = operation {
                self.read_blob(blob_digest)?;
            }
        }
        let bytes = artifact.canonical_bytes();
        self.put_addressed("artifacts", artifact.digest(), &bytes)
    }

    /// # Errors
    ///
    /// Returns an error for a missing, tampered, or malformed artifact.
    pub fn read_artifact(&self, digest: &str) -> Result<CanonicalArtifact, StorageError> {
        let bytes = self.read_addressed("artifacts", digest)?;
        CanonicalArtifact::from_canonical_bytes_with_digest(&bytes, digest)
            .map_err(StorageError::InvalidArtifact)
    }

    fn put_addressed(&self, kind: &str, digest: &str, content: &[u8]) -> Result<(), StorageError> {
        validate_digest(digest)?;
        if sha256_digest(content) != digest {
            return Err(StorageError::DigestMismatch(digest.to_owned()));
        }
        let path = self.address_path(kind, digest)?;
        let temporary = path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY_OBJECT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(mut file) => {
                file.write_all(content)?;
                file.sync_all()?;
                drop(file);
                match fs::hard_link(&temporary, &path) {
                    Ok(()) => {
                        fs::remove_file(temporary)?;
                        Ok(())
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        fs::remove_file(temporary)?;
                        let existing = self.read_addressed(kind, digest)?;
                        if existing == content {
                            Ok(())
                        } else {
                            Err(StorageError::DigestMismatch(digest.to_owned()))
                        }
                    }
                    Err(error) => {
                        let _ = fs::remove_file(temporary);
                        Err(StorageError::Io(error))
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(temporary);
                let existing = self.read_addressed(kind, digest)?;
                if existing == content {
                    Ok(())
                } else {
                    Err(StorageError::DigestMismatch(digest.to_owned()))
                }
            }
            Err(error) => Err(StorageError::Io(error)),
        }
    }

    fn read_addressed(&self, kind: &str, digest: &str) -> Result<Vec<u8>, StorageError> {
        validate_digest(digest)?;
        let path = self.address_path(kind, digest)?;
        let mut file = File::open(path)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;
        if sha256_digest(&content) != digest {
            return Err(StorageError::DigestMismatch(digest.to_owned()));
        }
        Ok(content)
    }

    fn address_path(&self, kind: &str, digest: &str) -> Result<PathBuf, StorageError> {
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or_else(|| StorageError::InvalidDigest(digest.to_owned()))?;
        Ok(self.root.join(kind).join("sha256").join(hex))
    }
}

fn validate_digest(digest: &str) -> Result<(), StorageError> {
    if is_sha256_digest(digest) {
        Ok(())
    } else {
        Err(StorageError::InvalidDigest(digest.to_owned()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredChange {
    id: ChangeId,
    head: Option<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lease {
    change_id: ChangeId,
    operation: String,
    holder: String,
    expires_at_unix_ms: i64,
}

impl Lease {
    #[must_use]
    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    #[must_use]
    pub fn holder(&self) -> &str {
        &self.holder
    }

    #[must_use]
    pub const fn expires_at_unix_ms(&self) -> i64 {
        self.expires_at_unix_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEvent {
    event_id: i64,
    change_id: ChangeId,
    kind: String,
    detail: String,
}

/// Complete append-only evidence for a correctness-sensitive domain mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainEvent {
    event_id: i64,
    kind: String,
    actor: String,
    occurred_at_unix_ms: i64,
    expected_state: String,
    resulting_state: String,
    affected_ids: String,
    operation_id: Option<String>,
    provider_evidence: Option<String>,
}

/// Durable evidence of a provider operation that could not combine exact inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationConflict {
    id: ConflictId,
    integration_id: IntegrationId,
    candidate_id: CandidateId,
    provider_state: String,
    attempted_operation: String,
    resolver: Option<String>,
    resulting_target: Option<String>,
    validation_evidence: Option<String>,
}

/// Immutable evidence captured while reconciling an uncertain provider operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationRecord {
    id: ReconciliationId,
    integration_id: IntegrationId,
    observed_state: String,
    evidence: String,
    resolved: bool,
}

/// A non-conclusive risk signal that two exact revisions overlap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Overlap {
    id: OverlapId,
    left_revision: RevisionId,
    right_revision: RevisionId,
    detail: String,
}
impl Overlap {
    /// # Errors
    /// Returns an error for invalid detail metadata.
    pub fn new(
        id: OverlapId,
        left_revision: RevisionId,
        right_revision: RevisionId,
        detail: impl Into<String>,
    ) -> Result<Self, StorageError> {
        if left_revision == right_revision {
            return Err(StorageError::Invariant(
                "overlap requires distinct revisions",
            ));
        }
        Ok(Self {
            id,
            left_revision,
            right_revision,
            detail: valid_event_value(detail.into(), "overlap detail")?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeRelationKind {
    TaskDecomposition,
    RelatedTo,
}
impl ChangeRelationKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::TaskDecomposition => "task-decomposition",
            Self::RelatedTo => "related-to",
        }
    }
}

impl ReconciliationRecord {
    /// # Errors
    /// Returns an error for invalid reconciliation metadata.
    pub fn new(
        id: ReconciliationId,
        integration_id: IntegrationId,
        observed_state: impl Into<String>,
        evidence: impl Into<String>,
        resolved: bool,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            id,
            integration_id,
            observed_state: valid_event_value(observed_state.into(), "observed state")?,
            evidence: valid_event_value(evidence.into(), "reconciliation evidence")?,
            resolved,
        })
    }
}
impl IntegrationConflict {
    /// # Errors
    /// Returns an error for invalid conflict metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ConflictId,
        integration_id: IntegrationId,
        candidate_id: CandidateId,
        provider_state: impl Into<String>,
        attempted_operation: impl Into<String>,
        resolver: Option<String>,
        resulting_target: Option<String>,
        validation_evidence: Option<String>,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            id,
            integration_id,
            candidate_id,
            provider_state: valid_event_value(provider_state.into(), "provider state")?,
            attempted_operation: valid_event_value(
                attempted_operation.into(),
                "attempted operation",
            )?,
            resolver: resolver
                .map(|value| valid_event_value(value, "resolver"))
                .transpose()?,
            resulting_target: resulting_target
                .map(|value| valid_event_value(value, "resulting target"))
                .transpose()?,
            validation_evidence: validation_evidence
                .map(|value| valid_event_value(value, "validation evidence"))
                .transpose()?,
        })
    }
}
impl DomainEvent {
    /// # Errors
    /// Returns an error for invalid event metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: impl Into<String>,
        actor: impl Into<String>,
        occurred_at_unix_ms: i64,
        expected_state: impl Into<String>,
        resulting_state: impl Into<String>,
        affected_ids: impl Into<String>,
        operation_id: Option<String>,
        provider_evidence: Option<String>,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            event_id: 0,
            kind: valid_event_value(kind.into(), "event kind")?,
            actor: valid_event_value(actor.into(), "actor")?,
            occurred_at_unix_ms,
            expected_state: valid_event_value(expected_state.into(), "expected state")?,
            resulting_state: valid_event_value(resulting_state.into(), "resulting state")?,
            affected_ids: valid_event_value(affected_ids.into(), "affected ids")?,
            operation_id: operation_id
                .map(|value| valid_event_value(value, "operation id"))
                .transpose()?,
            provider_evidence: provider_evidence
                .map(|value| valid_event_value(value, "provider evidence"))
                .transpose()?,
        })
    }
    #[must_use]
    pub const fn event_id(&self) -> i64 {
        self.event_id
    }
}

/// A durable record that a subject holds a role on a Change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Assignment {
    id: AssignmentId,
    change_id: ChangeId,
    subject: String,
    role: String,
    actor: String,
    assigned_at_unix_ms: i64,
}

impl Assignment {
    /// Creates a validated assignment event.
    ///
    /// # Errors
    ///
    /// Returns an error for blank, padded, control-character, or oversized values.
    pub fn new(
        assignment_id: AssignmentId,
        change_id: ChangeId,
        subject: impl Into<String>,
        role: impl Into<String>,
        actor: impl Into<String>,
        assigned_at_unix_ms: i64,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            id: assignment_id,
            change_id,
            subject: valid_event_value(subject.into(), "subject")?,
            role: valid_event_value(role.into(), "role")?,
            actor: valid_event_value(actor.into(), "actor")?,
            assigned_at_unix_ms,
        })
    }

    #[must_use]
    pub fn assignment_id(&self) -> &AssignmentId {
        &self.id
    }
    #[must_use]
    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }
    #[must_use]
    pub fn actor(&self) -> &str {
        &self.actor
    }
    #[must_use]
    pub const fn assigned_at_unix_ms(&self) -> i64 {
        self.assigned_at_unix_ms
    }
}

/// The provider-facing lifecycle of a realization of one immutable revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaterializationState {
    Clean,
    Dirty,
    Diverged,
    Suspended,
    Released,
    Invalidated,
}

impl MaterializationState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::Diverged => "diverged",
            Self::Suspended => "suspended",
            Self::Released => "released",
            Self::Invalidated => "invalidated",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "clean" => Ok(Self::Clean),
            "dirty" => Ok(Self::Dirty),
            "diverged" => Ok(Self::Diverged),
            "suspended" => Ok(Self::Suspended),
            "released" => Ok(Self::Released),
            "invalidated" => Ok(Self::Invalidated),
            _ => Err(StorageError::Invariant("unknown materialization state")),
        }
    }

    const fn may_transition_to(self, next: Self) -> bool {
        match self {
            Self::Clean => matches!(
                next,
                Self::Dirty | Self::Diverged | Self::Suspended | Self::Released | Self::Invalidated
            ),
            Self::Dirty => matches!(
                next,
                Self::Diverged | Self::Suspended | Self::Released | Self::Invalidated
            ),
            Self::Diverged => matches!(next, Self::Suspended | Self::Released | Self::Invalidated),
            Self::Suspended => matches!(
                next,
                Self::Clean | Self::Dirty | Self::Diverged | Self::Released | Self::Invalidated
            ),
            Self::Released | Self::Invalidated => false,
        }
    }
}

/// Durable materialization metadata and its exact revision target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Materialization {
    id: MaterializationId,
    revision_id: RevisionId,
    workspace_id: WorkspaceId,
    provider: String,
    provider_ref: String,
    state: MaterializationState,
}

/// An immutable exact target; mutable Change IDs are deliberately not targets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    Revision(RevisionId),
    Candidate(CandidateId),
}

impl Target {
    fn kind(&self) -> &'static str {
        match self {
            Self::Revision(_) => "revision",
            Self::Candidate(_) => "candidate",
        }
    }
    fn id(&self) -> &str {
        match self {
            Self::Revision(id) => id.as_str(),
            Self::Candidate(id) => id.as_str(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewOutcome {
    Approved,
    ChangesRequested,
    Rejected,
    Blocked,
}
impl ReviewOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes-requested",
            Self::Rejected => "rejected",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRequest {
    id: ReviewRequestId,
    target: Target,
    requester: String,
    reviewers: String,
    created_at_unix_ms: i64,
}
impl ReviewRequest {
    /// # Errors
    /// Returns an error for invalid actor metadata.
    pub fn new(
        id: ReviewRequestId,
        target: Target,
        requester: impl Into<String>,
        reviewers: impl Into<String>,
        created_at_unix_ms: i64,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            id,
            target,
            requester: valid_event_value(requester.into(), "requester")?,
            reviewers: valid_event_value(reviewers.into(), "reviewers")?,
            created_at_unix_ms,
        })
    }
    #[must_use]
    pub fn id(&self) -> &ReviewRequestId {
        &self.id
    }
    #[must_use]
    pub fn target(&self) -> &Target {
        &self.target
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewSubmission {
    id: ReviewSubmissionId,
    request_id: ReviewRequestId,
    reviewer: String,
    outcome: ReviewOutcome,
    comments: String,
    submitted_at_unix_ms: i64,
}
impl ReviewSubmission {
    /// # Errors
    /// Returns an error for invalid reviewer or comment metadata.
    pub fn new(
        id: ReviewSubmissionId,
        request_id: ReviewRequestId,
        reviewer: impl Into<String>,
        outcome: ReviewOutcome,
        comments: impl Into<String>,
        submitted_at_unix_ms: i64,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            id,
            request_id,
            reviewer: valid_event_value(reviewer.into(), "reviewer")?,
            outcome,
            comments: valid_event_value(comments.into(), "comments")?,
            submitted_at_unix_ms,
        })
    }
    #[must_use]
    pub fn outcome(&self) -> ReviewOutcome {
        self.outcome
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationStatus {
    Passed,
    Failed,
    Blocked,
}
impl ValidationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }
    fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "passed" => Ok(Self::Passed),
            "failed" => Ok(Self::Failed),
            "blocked" => Ok(Self::Blocked),
            _ => Err(StorageError::Invariant("unknown validation status")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationResult {
    id: ValidationResultId,
    target: Target,
    kind: String,
    environment: String,
    status: ValidationStatus,
    execution_id: String,
    recorded_at_unix_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrationState {
    Planned,
    Running,
    Conflicted,
    Failed,
    Succeeded,
    Aborted,
}
impl IntegrationState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Running => "running",
            Self::Conflicted => "conflicted",
            Self::Failed => "failed",
            Self::Succeeded => "succeeded",
            Self::Aborted => "aborted",
        }
    }
    fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "planned" => Ok(Self::Planned),
            "running" => Ok(Self::Running),
            "conflicted" => Ok(Self::Conflicted),
            "failed" => Ok(Self::Failed),
            "succeeded" => Ok(Self::Succeeded),
            "aborted" => Ok(Self::Aborted),
            _ => Err(StorageError::Invariant("unknown integration state")),
        }
    }
    const fn may_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Planned, Self::Running | Self::Aborted)
                | (
                    Self::Running,
                    Self::Conflicted | Self::Failed | Self::Succeeded | Self::Aborted
                )
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationAttempt {
    id: IntegrationId,
    repository_id: RepositoryId,
    candidate_id: CandidateId,
    target_ref: String,
    expected_target_revision: String,
    provider: String,
    strategy: String,
    operation_id: OperationId,
    actor: String,
    state: IntegrationState,
}
impl IntegrationAttempt {
    /// # Errors
    /// Returns an error for invalid provider or operation metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: IntegrationId,
        repository_id: RepositoryId,
        candidate_id: CandidateId,
        target_ref: impl Into<String>,
        expected_target_revision: impl Into<String>,
        provider: impl Into<String>,
        strategy: impl Into<String>,
        operation_id: OperationId,
        actor: impl Into<String>,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            id,
            repository_id,
            candidate_id,
            target_ref: valid_event_value(target_ref.into(), "target ref")?,
            expected_target_revision: valid_event_value(
                expected_target_revision.into(),
                "expected target",
            )?,
            provider: valid_event_value(provider.into(), "provider")?,
            strategy: valid_event_value(strategy.into(), "strategy")?,
            operation_id,
            actor: valid_event_value(actor.into(), "actor")?,
            state: IntegrationState::Planned,
        })
    }
    #[must_use]
    pub fn id(&self) -> &IntegrationId {
        &self.id
    }
    #[must_use]
    pub fn state(&self) -> IntegrationState {
        self.state
    }
    #[must_use]
    pub fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationReceipt {
    id: IntegrationReceiptId,
    integration_id: IntegrationId,
    prior_target_revision: String,
    result_revision: String,
    provider_evidence: String,
}

/// An immutable ordered, duplicate-free Stack snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackVersion {
    stack_id: StackId,
    version: i64,
    changes: Vec<ChangeId>,
}
impl StackVersion {
    #[must_use]
    pub fn stack_id(&self) -> &StackId {
        &self.stack_id
    }
    #[must_use]
    pub const fn version(&self) -> i64 {
        self.version
    }
    #[must_use]
    pub fn changes(&self) -> &[ChangeId] {
        &self.changes
    }
}
impl IntegrationReceipt {
    /// # Errors
    /// Returns an error for invalid receipt metadata.
    pub fn new(
        id: IntegrationReceiptId,
        integration_id: IntegrationId,
        prior_target_revision: impl Into<String>,
        result_revision: impl Into<String>,
        provider_evidence: impl Into<String>,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            id,
            integration_id,
            prior_target_revision: valid_event_value(prior_target_revision.into(), "prior target")?,
            result_revision: valid_event_value(result_revision.into(), "result revision")?,
            provider_evidence: valid_event_value(provider_evidence.into(), "provider evidence")?,
        })
    }
}
impl ValidationResult {
    /// # Errors
    /// Returns an error for invalid validation metadata.
    pub fn new(
        id: ValidationResultId,
        target: Target,
        kind: impl Into<String>,
        environment: impl Into<String>,
        status: ValidationStatus,
        execution_id: impl Into<String>,
        recorded_at_unix_ms: i64,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            id,
            target,
            kind: valid_event_value(kind.into(), "validation kind")?,
            environment: valid_event_value(environment.into(), "environment")?,
            status,
            execution_id: valid_event_value(execution_id.into(), "execution id")?,
            recorded_at_unix_ms,
        })
    }
    #[must_use]
    pub fn target(&self) -> &Target {
        &self.target
    }
    #[must_use]
    pub fn status(&self) -> ValidationStatus {
        self.status
    }
}

impl Materialization {
    #[must_use]
    pub fn materialization_id(&self) -> &MaterializationId {
        &self.id
    }
    #[must_use]
    pub fn revision_id(&self) -> &RevisionId {
        &self.revision_id
    }
    #[must_use]
    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }
    #[must_use]
    pub fn provider_ref(&self) -> &str {
        &self.provider_ref
    }
    #[must_use]
    pub const fn state(&self) -> MaterializationState {
        self.state
    }
}

/// An exact upstream revision required by a downstream Change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dependency {
    upstream_change: ChangeId,
    upstream_revision: RevisionId,
    downstream_change: ChangeId,
}

impl Dependency {
    #[must_use]
    pub const fn new(
        upstream_change_id: ChangeId,
        upstream_revision_id: RevisionId,
        downstream_change_id: ChangeId,
    ) -> Self {
        Self {
            upstream_change: upstream_change_id,
            upstream_revision: upstream_revision_id,
            downstream_change: downstream_change_id,
        }
    }

    #[must_use]
    pub fn upstream_change_id(&self) -> &ChangeId {
        &self.upstream_change
    }

    #[must_use]
    pub fn upstream_revision_id(&self) -> &RevisionId {
        &self.upstream_revision
    }

    #[must_use]
    pub fn downstream_change_id(&self) -> &ChangeId {
        &self.downstream_change
    }
}

/// One ordered, exact Change revision in an immutable composition candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateInput {
    change_id: ChangeId,
    revision_id: RevisionId,
}

impl CandidateInput {
    #[must_use]
    pub const fn new(change_id: ChangeId, revision_id: RevisionId) -> Self {
        Self {
            change_id,
            revision_id,
        }
    }

    #[must_use]
    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub fn revision_id(&self) -> &RevisionId {
        &self.revision_id
    }
}

/// A durable snapshot of ordered revisions and their dependency pins.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionCandidate {
    candidate_id: CandidateId,
    target_base: BaseState,
    inputs: Vec<CandidateInput>,
    resolved_dependencies: Vec<Dependency>,
    content_digest: String,
}

impl CompositionCandidate {
    #[must_use]
    pub fn candidate_id(&self) -> &CandidateId {
        &self.candidate_id
    }

    #[must_use]
    pub fn target_base(&self) -> &BaseState {
        &self.target_base
    }

    #[must_use]
    pub fn inputs(&self) -> &[CandidateInput] {
        &self.inputs
    }

    #[must_use]
    pub fn resolved_dependencies(&self) -> &[Dependency] {
        &self.resolved_dependencies
    }

    #[must_use]
    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }
}

impl AuditEvent {
    #[must_use]
    pub const fn event_id(&self) -> i64 {
        self.event_id
    }

    #[must_use]
    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl StoredChange {
    #[must_use]
    pub fn id(&self) -> &ChangeId {
        &self.id
    }

    #[must_use]
    pub fn head(&self) -> Option<&RevisionId> {
        self.head.as_ref()
    }
}

pub struct SqliteRepository {
    connection: Connection,
    content_store: ContentStore,
}

impl SqliteRepository {
    /// # Errors
    ///
    /// Returns an error when `SQLite` cannot open, configure, or migrate the database.
    #[allow(clippy::too_many_lines)]
    pub fn open(
        database_path: impl AsRef<Path>,
        content_store: ContentStore,
    ) -> Result<Self, StorageError> {
        let connection = Connection::open(database_path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS schema_migrations (
                 version INTEGER PRIMARY KEY
             );
             CREATE TABLE IF NOT EXISTS changes (
                 change_id TEXT PRIMARY KEY NOT NULL,
                 head_revision_id TEXT NULL
             );
             CREATE TABLE IF NOT EXISTS revisions (
                 revision_id TEXT PRIMARY KEY NOT NULL,
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 parent_revision_id TEXT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 repository_id TEXT NOT NULL,
                 base_object_id TEXT NOT NULL,
                 artifact_digest TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS leases (
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 operation TEXT NOT NULL,
                 holder TEXT NOT NULL,
                 expires_at_unix_ms INTEGER NOT NULL,
                 PRIMARY KEY(change_id, operation)
             );
             CREATE TABLE IF NOT EXISTS audit_events (
                 event_id INTEGER PRIMARY KEY AUTOINCREMENT,
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 kind TEXT NOT NULL,
                 detail TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS domain_events (
                 event_id INTEGER PRIMARY KEY AUTOINCREMENT,
                 kind TEXT NOT NULL, actor TEXT NOT NULL, occurred_at_unix_ms INTEGER NOT NULL,
                 expected_state TEXT NOT NULL, resulting_state TEXT NOT NULL, affected_ids TEXT NOT NULL,
                 operation_id TEXT NULL, provider_evidence TEXT NULL
             );
             CREATE TABLE IF NOT EXISTS integration_conflicts (
                 conflict_id TEXT PRIMARY KEY NOT NULL,
                 integration_id TEXT NOT NULL REFERENCES integration_attempts(integration_id) ON DELETE RESTRICT,
                 candidate_id TEXT NOT NULL REFERENCES candidates(candidate_id) ON DELETE RESTRICT,
                 provider_state TEXT NOT NULL, attempted_operation TEXT NOT NULL,
                 resolver TEXT NULL, resulting_target TEXT NULL, validation_evidence TEXT NULL
             );
             CREATE TABLE IF NOT EXISTS reconciliation_records (
                 reconciliation_id TEXT PRIMARY KEY NOT NULL,
                 integration_id TEXT NOT NULL REFERENCES integration_attempts(integration_id) ON DELETE RESTRICT,
                 observed_state TEXT NOT NULL, evidence TEXT NOT NULL, resolved INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS change_relations (
                 source_change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 target_change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 kind TEXT NOT NULL CHECK(kind IN ('task-decomposition', 'related-to')),
                 PRIMARY KEY(source_change_id, target_change_id, kind),
                 CHECK(source_change_id <> target_change_id)
             );
             CREATE TABLE IF NOT EXISTS overlaps (
                 overlap_id TEXT PRIMARY KEY NOT NULL,
                 left_revision_id TEXT NOT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 right_revision_id TEXT NOT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 detail TEXT NOT NULL,
                 CHECK(left_revision_id <> right_revision_id)
             );
             CREATE TABLE IF NOT EXISTS dependencies (
                 upstream_change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 upstream_revision_id TEXT NOT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 downstream_change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 PRIMARY KEY(upstream_change_id, downstream_change_id),
                 CHECK(upstream_change_id <> downstream_change_id)
             );
             CREATE TABLE IF NOT EXISTS candidates (
                 candidate_id TEXT PRIMARY KEY NOT NULL,
                 repository_id TEXT NOT NULL,
                 target_base_object_id TEXT NOT NULL,
                 content_digest TEXT NOT NULL,
                 stack_id TEXT NULL,
                 stack_version INTEGER NULL
             );
             CREATE TABLE IF NOT EXISTS candidate_inputs (
                 candidate_id TEXT NOT NULL REFERENCES candidates(candidate_id) ON DELETE RESTRICT,
                 position INTEGER NOT NULL CHECK(position >= 0),
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 revision_id TEXT NOT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 PRIMARY KEY(candidate_id, position),
                 UNIQUE(candidate_id, change_id)
             );
             CREATE TABLE IF NOT EXISTS candidate_dependencies (
                 candidate_id TEXT NOT NULL REFERENCES candidates(candidate_id) ON DELETE RESTRICT,
                 upstream_change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 upstream_revision_id TEXT NOT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 downstream_change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 PRIMARY KEY(candidate_id, upstream_change_id, downstream_change_id)
             );
             CREATE TABLE IF NOT EXISTS assignments (
                 assignment_id TEXT PRIMARY KEY NOT NULL,
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 subject TEXT NOT NULL,
                 role TEXT NOT NULL,
                 actor TEXT NOT NULL,
                 assigned_at_unix_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS materializations (
                 materialization_id TEXT PRIMARY KEY NOT NULL,
                 revision_id TEXT NOT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 workspace_id TEXT NOT NULL,
                 provider TEXT NOT NULL,
                 provider_ref TEXT NOT NULL,
                 state TEXT NOT NULL,
                 version INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS review_requests (
                 review_request_id TEXT PRIMARY KEY NOT NULL,
                 target_kind TEXT NOT NULL,
                 target_id TEXT NOT NULL,
                 requester TEXT NOT NULL,
                 reviewers TEXT NOT NULL,
                 created_at_unix_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS review_submissions (
                 review_submission_id TEXT PRIMARY KEY NOT NULL,
                 review_request_id TEXT NOT NULL REFERENCES review_requests(review_request_id) ON DELETE RESTRICT,
                 reviewer TEXT NOT NULL,
                 outcome TEXT NOT NULL,
                 comments TEXT NOT NULL,
                 submitted_at_unix_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS validation_results (
                 validation_result_id TEXT PRIMARY KEY NOT NULL,
                 target_kind TEXT NOT NULL,
                 target_id TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 environment TEXT NOT NULL,
                 status TEXT NOT NULL,
                 execution_id TEXT NOT NULL,
                 recorded_at_unix_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS integration_attempts (
                 integration_id TEXT PRIMARY KEY NOT NULL,
                 repository_id TEXT NOT NULL,
                 candidate_id TEXT NOT NULL REFERENCES candidates(candidate_id) ON DELETE RESTRICT,
                 target_ref TEXT NOT NULL,
                 expected_target_revision TEXT NOT NULL,
                 provider TEXT NOT NULL,
                 strategy TEXT NOT NULL,
                 operation_id TEXT UNIQUE NOT NULL,
                 actor TEXT NOT NULL,
                 state TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS integration_receipts (
                 receipt_id TEXT PRIMARY KEY NOT NULL,
                 integration_id TEXT UNIQUE NOT NULL REFERENCES integration_attempts(integration_id) ON DELETE RESTRICT,
                 prior_target_revision TEXT NOT NULL,
                 result_revision TEXT NOT NULL,
                 provider_evidence TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS stacks (
                 stack_id TEXT PRIMARY KEY NOT NULL,
                 current_version INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS stack_entries (
                 stack_id TEXT NOT NULL REFERENCES stacks(stack_id) ON DELETE RESTRICT,
                 version INTEGER NOT NULL,
                 position INTEGER NOT NULL CHECK(position >= 0),
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 PRIMARY KEY(stack_id, version, position),
                 UNIQUE(stack_id, version, change_id)
             );",
        )?;
        let stored_version: Option<i64> =
            connection.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })?;
        if stored_version.is_some_and(|version| version > SCHEMA_VERSION) {
            return Err(StorageError::UnsupportedSchemaVersion(
                stored_version.unwrap_or_default(),
            ));
        }
        connection.execute(
            "INSERT OR IGNORE INTO schema_migrations(version) VALUES (?1)",
            [SCHEMA_VERSION],
        )?;
        Ok(Self {
            connection,
            content_store,
        })
    }

    /// # Errors
    ///
    /// Returns an error if the Change identity already exists or `SQLite` fails.
    pub fn create_change(&mut self, change_id: ChangeId) -> Result<(), StorageError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO changes(change_id, head_revision_id) VALUES (?1, NULL)",
            [change_id.as_str()],
        )?;
        if changed == 0 {
            return Err(StorageError::DuplicateChange(change_id));
        }
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![change_id.as_str(), "change-created", change_id.as_str()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error if the Change is missing, corrupted, or `SQLite` fails.
    pub fn load_change(&self, change_id: &ChangeId) -> Result<StoredChange, StorageError> {
        self.connection
            .query_row(
                "SELECT head_revision_id FROM changes WHERE change_id = ?1",
                [change_id.as_str()],
                |row| {
                    let head: Option<String> = row.get(0)?;
                    let head = head.map(RevisionId::new).transpose().map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok(StoredChange {
                        id: change_id.clone(),
                        head,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingChange(change_id.clone()))
    }

    /// Appends exactly one successor when the persisted head matches.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing Change, stale head, duplicate revision,
    /// missing/tampered canonical content, or `SQLite` failure.
    pub fn append_revision(
        &mut self,
        change_id: &ChangeId,
        expected_head: Option<&RevisionId>,
        revision_id: RevisionId,
        artifact: &CanonicalArtifact,
    ) -> Result<(), StorageError> {
        self.content_store.put_artifact(artifact)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual: Option<String> = transaction
            .query_row(
                "SELECT head_revision_id FROM changes WHERE change_id = ?1",
                [change_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingChange(change_id.clone()))?;
        let expected = expected_head.map(RevisionId::as_str);
        if actual.as_deref() != expected {
            return Err(StorageError::StaleHead {
                expected: expected_head.cloned(),
                actual: actual
                    .map(RevisionId::new)
                    .transpose()
                    .map_err(StorageError::Domain)?,
            });
        }
        let duplicate = transaction
            .query_row(
                "SELECT 1 FROM revisions WHERE revision_id = ?1",
                [revision_id.as_str()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if duplicate {
            return Err(StorageError::DuplicateRevision(revision_id));
        }
        transaction.execute(
            "INSERT INTO revisions(
                revision_id, change_id, parent_revision_id, repository_id, base_object_id, artifact_digest
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                revision_id.as_str(),
                change_id.as_str(),
                expected,
                artifact.base().repository_id().as_str(),
                artifact.base().object_id(),
                artifact.digest(),
            ],
        )?;
        let changed = transaction.execute(
            "UPDATE changes SET head_revision_id = ?1
             WHERE change_id = ?2 AND head_revision_id IS ?3",
            params![revision_id.as_str(), change_id.as_str(), expected],
        )?;
        if changed != 1 {
            return Err(StorageError::Invariant(
                "head changed during immediate transaction",
            ));
        }
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![
                change_id.as_str(),
                "revision-appended",
                revision_id.as_str()
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Acquires an expired-or-unheld exclusive operation lease.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid inputs, a missing Change, or an active
    /// lease held by another actor.
    pub fn acquire_lease(
        &mut self,
        change_id: &ChangeId,
        operation: impl Into<String>,
        holder: impl Into<String>,
        now_unix_ms: i64,
        expires_at_unix_ms: i64,
    ) -> Result<Lease, StorageError> {
        let operation = valid_lease_value(operation.into(), "operation")?;
        let holder = valid_lease_value(holder.into(), "holder")?;
        if expires_at_unix_ms <= now_unix_ms {
            return Err(StorageError::InvalidLeaseExpiry);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists = transaction
            .query_row(
                "SELECT 1 FROM changes WHERE change_id = ?1",
                [change_id.as_str()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(StorageError::MissingChange(change_id.clone()));
        }
        let existing: Option<(String, i64)> = transaction
            .query_row(
                "SELECT holder, expires_at_unix_ms FROM leases WHERE change_id = ?1 AND operation = ?2",
                params![change_id.as_str(), operation],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((existing_holder, expiry)) = existing
            && expiry > now_unix_ms
        {
            return Err(StorageError::LeaseHeld {
                holder: existing_holder,
                expires_at_unix_ms: expiry,
            });
        }
        transaction.execute(
            "INSERT INTO leases(change_id, operation, holder, expires_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(change_id, operation) DO UPDATE SET
                 holder = excluded.holder,
                 expires_at_unix_ms = excluded.expires_at_unix_ms",
            params![change_id.as_str(), operation, holder, expires_at_unix_ms],
        )?;
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![change_id.as_str(), "lease-acquired", operation],
        )?;
        transaction.commit()?;
        Ok(Lease {
            change_id: change_id.clone(),
            operation,
            holder,
            expires_at_unix_ms,
        })
    }

    /// Renews an active lease only for its current holder.
    /// # Errors
    /// Returns an error for an expired/missing lease, mismatched holder, or invalid expiry.
    pub fn renew_lease(
        &mut self,
        lease: &Lease,
        now_unix_ms: i64,
        expires_at_unix_ms: i64,
    ) -> Result<Lease, StorageError> {
        if expires_at_unix_ms <= now_unix_ms {
            return Err(StorageError::InvalidLeaseExpiry);
        }
        let updated = self.connection.execute(
            "UPDATE leases SET expires_at_unix_ms = ?1 WHERE change_id = ?2 AND operation = ?3 AND holder = ?4 AND expires_at_unix_ms > ?5",
            params![expires_at_unix_ms, lease.change_id.as_str(), lease.operation, lease.holder, now_unix_ms],
        )?;
        if updated != 1 {
            return Err(StorageError::LeaseLost);
        }
        Ok(Lease {
            change_id: lease.change_id.clone(),
            operation: lease.operation.clone(),
            holder: lease.holder.clone(),
            expires_at_unix_ms,
        })
    }

    /// Releases an active lease only for its current holder.
    /// # Errors
    /// Returns an error when the lease was lost or already released.
    pub fn release_lease(&mut self, lease: &Lease) -> Result<(), StorageError> {
        let deleted = self.connection.execute(
            "DELETE FROM leases WHERE change_id = ?1 AND operation = ?2 AND holder = ?3",
            params![lease.change_id.as_str(), lease.operation, lease.holder],
        )?;
        if deleted != 1 {
            return Err(StorageError::LeaseLost);
        }
        Ok(())
    }

    /// Returns durable events in creation order for a Change.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing Change or `SQLite` failure.
    pub fn audit_events(&self, change_id: &ChangeId) -> Result<Vec<AuditEvent>, StorageError> {
        self.load_change(change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT event_id, kind, detail FROM audit_events
             WHERE change_id = ?1 ORDER BY event_id",
        )?;
        let rows = statement.query_map([change_id.as_str()], |row| {
            Ok(AuditEvent {
                event_id: row.get(0)?,
                change_id: change_id.clone(),
                kind: row.get(1)?,
                detail: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Sqlite)
    }

    /// # Errors
    ///
    /// Returns an error if the revision is missing or its artifact cannot be verified.
    pub fn load_artifact_for_revision(
        &self,
        revision_id: &RevisionId,
    ) -> Result<CanonicalArtifact, StorageError> {
        let digest: String = self
            .connection
            .query_row(
                "SELECT artifact_digest FROM revisions WHERE revision_id = ?1",
                [revision_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingRevision(revision_id.clone()))?;
        self.content_store.read_artifact(&digest)
    }

    /// Adds a durable exact-revision dependency after atomically rejecting a cycle.
    ///
    /// # Errors
    ///
    /// Returns an error if an endpoint or pinned revision is missing, the pin does
    /// not belong to its declared upstream Change, the edge already exists, or it
    /// would create a dependency cycle.
    pub fn add_dependency(&mut self, dependency: &Dependency) -> Result<(), StorageError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_change_exists(&transaction, dependency.upstream_change_id())?;
        ensure_change_exists(&transaction, dependency.downstream_change_id())?;
        let revision_change: String = transaction
            .query_row(
                "SELECT change_id FROM revisions WHERE revision_id = ?1",
                [dependency.upstream_revision_id().as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                StorageError::MissingRevision(dependency.upstream_revision_id().clone())
            })?;
        if revision_change != dependency.upstream_change_id().as_str() {
            return Err(StorageError::RevisionDoesNotBelongToChange {
                revision_id: dependency.upstream_revision_id().clone(),
                change_id: dependency.upstream_change_id().clone(),
            });
        }
        let creates_cycle: bool = transaction.query_row(
            "WITH RECURSIVE reachable(change_id) AS (
                 SELECT downstream_change_id FROM dependencies WHERE upstream_change_id = ?1
                 UNION
                 SELECT dependencies.downstream_change_id
                 FROM dependencies JOIN reachable ON dependencies.upstream_change_id = reachable.change_id
             )
             SELECT EXISTS(SELECT 1 FROM reachable WHERE change_id = ?2)",
            params![dependency.downstream_change_id().as_str(), dependency.upstream_change_id().as_str()],
            |row| row.get(0),
        )?;
        if creates_cycle {
            return Err(StorageError::DependencyCycle);
        }
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO dependencies(
                upstream_change_id, upstream_revision_id, downstream_change_id
             ) VALUES (?1, ?2, ?3)",
            params![
                dependency.upstream_change_id().as_str(),
                dependency.upstream_revision_id().as_str(),
                dependency.downstream_change_id().as_str(),
            ],
        )?;
        if changed == 0 {
            return Err(StorageError::DuplicateDependency);
        }
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![
                dependency.downstream_change_id().as_str(),
                "dependency-added",
                format!(
                    "{}@{}",
                    dependency.upstream_change_id().as_str(),
                    dependency.upstream_revision_id().as_str()
                ),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Returns the current exact dependency contracts for one downstream Change.
    ///
    /// # Errors
    ///
    /// Returns an error if the Change is missing or persisted identifiers are invalid.
    pub fn dependencies_for(
        &self,
        downstream_change_id: &ChangeId,
    ) -> Result<Vec<Dependency>, StorageError> {
        self.load_change(downstream_change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT upstream_change_id, upstream_revision_id FROM dependencies
             WHERE downstream_change_id = ?1 ORDER BY upstream_change_id",
        )?;
        let rows = statement.query_map([downstream_change_id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (change, revision) = row?;
            Ok(Dependency::new(
                ChangeId::new(change).map_err(StorageError::Domain)?,
                RevisionId::new(revision).map_err(StorageError::Domain)?,
                downstream_change_id.clone(),
            ))
        })
        .collect()
    }

    /// Creates an immutable candidate from ordered exact revision inputs.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate/empty inputs, mismatched repository or
    /// revision ownership, unresolved dependency pins, or duplicate candidate ID.
    pub fn create_candidate(
        &mut self,
        candidate_id: CandidateId,
        target_base: BaseState,
        inputs: Vec<CandidateInput>,
    ) -> Result<CompositionCandidate, StorageError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let resolved_dependencies = validate_candidate_inputs(&transaction, &target_base, &inputs)?;
        let content_digest = candidate_digest(&target_base, &inputs, &resolved_dependencies);
        let inserted_candidate = transaction.execute(
            "INSERT OR IGNORE INTO candidates(
                candidate_id, repository_id, target_base_object_id, content_digest
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                candidate_id.as_str(),
                target_base.repository_id().as_str(),
                target_base.object_id(),
                content_digest,
            ],
        )?;
        if inserted_candidate == 0 {
            return Err(StorageError::DuplicateCandidate(candidate_id));
        }
        for (position, input) in inputs.iter().enumerate() {
            transaction.execute(
                "INSERT INTO candidate_inputs(candidate_id, position, change_id, revision_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    candidate_id.as_str(),
                    position,
                    input.change_id().as_str(),
                    input.revision_id().as_str()
                ],
            )?;
            transaction.execute(
                "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
                params![
                    input.change_id().as_str(),
                    "candidate-created",
                    candidate_id.as_str()
                ],
            )?;
        }
        for dependency in &resolved_dependencies {
            transaction.execute(
                "INSERT INTO candidate_dependencies(
                    candidate_id, upstream_change_id, upstream_revision_id, downstream_change_id
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    candidate_id.as_str(),
                    dependency.upstream_change_id().as_str(),
                    dependency.upstream_revision_id().as_str(),
                    dependency.downstream_change_id().as_str(),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(CompositionCandidate {
            candidate_id,
            target_base,
            inputs,
            resolved_dependencies,
            content_digest,
        })
    }

    /// Creates a candidate and records the exact immutable Stack version it consumed.
    /// # Errors
    /// Returns an error if inputs differ from the specified Stack version.
    pub fn create_candidate_from_stack(
        &mut self,
        candidate_id: &CandidateId,
        target_base: BaseState,
        inputs: Vec<CandidateInput>,
        stack_id: &StackId,
        stack_version: i64,
    ) -> Result<CompositionCandidate, StorageError> {
        let stack = self.load_stack(stack_id, stack_version)?;
        let input_changes: Vec<_> = inputs.iter().map(CandidateInput::change_id).collect();
        let stack_changes: Vec<_> = stack.changes().iter().collect();
        if input_changes != stack_changes {
            return Err(StorageError::CandidateStackMismatch);
        }
        let candidate = self.create_candidate(candidate_id.clone(), target_base, inputs)?;
        self.connection.execute(
            "UPDATE candidates SET stack_id = ?1, stack_version = ?2 WHERE candidate_id = ?3",
            params![stack_id.as_str(), stack_version, candidate_id.as_str()],
        )?;
        Ok(candidate)
    }

    /// Loads a candidate's immutable snapshot. It never resolves current heads.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing or malformed persisted candidate.
    pub fn load_candidate(
        &self,
        candidate_id: &CandidateId,
    ) -> Result<CompositionCandidate, StorageError> {
        let (repository_id, object_id, content_digest): (String, String, String) = self
            .connection
            .query_row(
                "SELECT repository_id, target_base_object_id, content_digest
                 FROM candidates WHERE candidate_id = ?1",
                [candidate_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingCandidate(candidate_id.clone()))?;
        let target_base = BaseState::new(RepositoryId::new(repository_id)?, object_id)?;
        let inputs = candidate_inputs(&self.connection, candidate_id)?;
        let dependencies = candidate_dependencies(&self.connection, candidate_id)?;
        if candidate_digest(&target_base, &inputs, &dependencies) != content_digest {
            return Err(StorageError::Invariant("candidate content digest mismatch"));
        }
        Ok(CompositionCandidate {
            candidate_id: candidate_id.clone(),
            target_base,
            inputs,
            resolved_dependencies: dependencies,
            content_digest,
        })
    }

    /// Reports whether any candidate input or dependency pin has advanced.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing or corrupt candidate.
    pub fn candidate_is_stale(&self, candidate_id: &CandidateId) -> Result<bool, StorageError> {
        let candidate = self.load_candidate(candidate_id)?;
        for input in candidate.inputs() {
            let head = self.load_change(input.change_id())?.head().cloned();
            if head.as_ref() != Some(input.revision_id()) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Reports whether an exact review/validation target has been superseded.
    /// # Errors
    /// Returns an error for a missing target or corrupt persisted identity.
    pub fn target_is_stale(&self, target: &Target) -> Result<bool, StorageError> {
        match target {
            Target::Candidate(id) => self.candidate_is_stale(id),
            Target::Revision(id) => {
                let change: String = self
                    .connection
                    .query_row(
                        "SELECT change_id FROM revisions WHERE revision_id = ?1",
                        [id.as_str()],
                        |row| row.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| StorageError::MissingRevision(id.clone()))?;
                Ok(self.load_change(&ChangeId::new(change)?)?.head() != Some(id))
            }
        }
    }

    /// Records an immutable assignment event for an existing Change.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing Change, duplicate assignment identity, or storage failure.
    pub fn record_assignment(&mut self, assignment: &Assignment) -> Result<(), StorageError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_change_exists(&transaction, assignment.change_id())?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO assignments(
                assignment_id, change_id, subject, role, actor, assigned_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                assignment.assignment_id().as_str(),
                assignment.change_id().as_str(),
                assignment.subject(),
                assignment.role(),
                assignment.actor(),
                assignment.assigned_at_unix_ms(),
            ],
        )?;
        if inserted == 0 {
            return Err(StorageError::DuplicateAssignment(
                assignment.assignment_id().clone(),
            ));
        }
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![
                assignment.change_id().as_str(),
                "assignment-recorded",
                assignment.assignment_id().as_str()
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Returns assignment history in its durable event order.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing Change or malformed persisted assignment.
    pub fn assignments_for(&self, change_id: &ChangeId) -> Result<Vec<Assignment>, StorageError> {
        self.load_change(change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT assignment_id, subject, role, actor, assigned_at_unix_ms FROM assignments
             WHERE change_id = ?1 ORDER BY assigned_at_unix_ms, assignment_id",
        )?;
        let rows = statement.query_map([change_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (id, subject, role, actor, timestamp) = row?;
            Assignment::new(
                AssignmentId::new(id)?,
                change_id.clone(),
                subject,
                role,
                actor,
                timestamp,
            )
        })
        .collect()
    }

    /// Creates a clean materialization for one exact revision.
    ///
    /// # Errors
    ///
    /// Returns an error if the revision is absent, identity is duplicated, or provider metadata is invalid.
    pub fn create_materialization(
        &mut self,
        materialization_id: MaterializationId,
        revision_id: RevisionId,
        workspace_id: WorkspaceId,
        provider: impl Into<String>,
        provider_ref: impl Into<String>,
    ) -> Result<Materialization, StorageError> {
        let provider = valid_event_value(provider.into(), "provider")?;
        let provider_ref = valid_event_value(provider_ref.into(), "provider reference")?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let change_id: String = transaction
            .query_row(
                "SELECT change_id FROM revisions WHERE revision_id = ?1",
                [revision_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingRevision(revision_id.clone()))?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO materializations(
                materialization_id, revision_id, workspace_id, provider, provider_ref, state, version
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'clean', 0)",
            params![materialization_id.as_str(), revision_id.as_str(), workspace_id.as_str(), provider, provider_ref],
        )?;
        if inserted == 0 {
            return Err(StorageError::DuplicateMaterialization(materialization_id));
        }
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![
                change_id,
                "materialization-created",
                materialization_id.as_str()
            ],
        )?;
        transaction.commit()?;
        Ok(Materialization {
            id: materialization_id,
            revision_id,
            workspace_id,
            provider,
            provider_ref,
            state: MaterializationState::Clean,
        })
    }

    /// Loads a materialization without consulting provider state.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing or malformed persisted materialization.
    pub fn load_materialization(
        &self,
        materialization_id: &MaterializationId,
    ) -> Result<Materialization, StorageError> {
        let row: Option<(String, String, String, String, String)> = self.connection.query_row(
            "SELECT revision_id, workspace_id, provider, provider_ref, state FROM materializations WHERE materialization_id = ?1",
            [materialization_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).optional()?;
        let (revision_id, workspace_id, provider, provider_ref, state) =
            row.ok_or_else(|| StorageError::MissingMaterialization(materialization_id.clone()))?;
        Ok(Materialization {
            id: materialization_id.clone(),
            revision_id: RevisionId::new(revision_id)?,
            workspace_id: WorkspaceId::new(workspace_id)?,
            provider,
            provider_ref,
            state: MaterializationState::parse(&state)?,
        })
    }

    /// Transitions a materialization only from the caller's expected state.
    ///
    /// # Errors
    ///
    /// Returns a stale-state error rather than overwriting concurrent provider observations.
    pub fn transition_materialization(
        &mut self,
        materialization_id: &MaterializationId,
        expected: MaterializationState,
        next: MaterializationState,
    ) -> Result<Materialization, StorageError> {
        if !expected.may_transition_to(next) {
            return Err(StorageError::InvalidMaterializationTransition { expected, next });
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, String, String, String, String)> = transaction.query_row(
            "SELECT revision_id, workspace_id, provider, provider_ref, state FROM materializations WHERE materialization_id = ?1",
            [materialization_id.as_str()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).optional()?;
        let (revision, workspace, provider, provider_ref, actual) =
            row.ok_or_else(|| StorageError::MissingMaterialization(materialization_id.clone()))?;
        let actual = MaterializationState::parse(&actual)?;
        if actual != expected {
            return Err(StorageError::StaleMaterializationState { expected, actual });
        }
        let updated = transaction.execute(
            "UPDATE materializations SET state = ?1, version = version + 1 WHERE materialization_id = ?2 AND state = ?3",
            params![next.as_str(), materialization_id.as_str(), expected.as_str()],
        )?;
        if updated != 1 {
            return Err(StorageError::Invariant(
                "materialization changed during immediate transaction",
            ));
        }
        let change_id: String = transaction.query_row(
            "SELECT change_id FROM revisions WHERE revision_id = ?1",
            [revision.as_str()],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![
                change_id,
                "materialization-transitioned",
                format!("{}:{}", materialization_id.as_str(), next.as_str())
            ],
        )?;
        transaction.commit()?;
        Ok(Materialization {
            id: materialization_id.clone(),
            revision_id: RevisionId::new(revision)?,
            workspace_id: WorkspaceId::new(workspace)?,
            provider,
            provider_ref,
            state: next,
        })
    }

    /// Persists a request targeting one immutable revision or candidate.
    /// # Errors
    /// Returns an error for a missing target, duplicate ID, or storage failure.
    pub fn create_review_request(&mut self, request: &ReviewRequest) -> Result<(), StorageError> {
        ensure_target_exists(&self.connection, request.target())?;
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO review_requests(review_request_id, target_kind, target_id, requester, reviewers, created_at_unix_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![request.id.as_str(), request.target.kind(), request.target.id(), request.requester, request.reviewers, request.created_at_unix_ms],
        )?;
        if inserted == 0 {
            return Err(StorageError::DuplicateReviewRequest(request.id.clone()));
        }
        Ok(())
    }

    /// Persists an immutable outcome against an existing exact-target request.
    /// # Errors
    /// Returns an error for a missing request, duplicate ID, or storage failure.
    pub fn submit_review(&mut self, submission: &ReviewSubmission) -> Result<(), StorageError> {
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO review_submissions(review_submission_id, review_request_id, reviewer, outcome, comments, submitted_at_unix_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![submission.id.as_str(), submission.request_id.as_str(), submission.reviewer, submission.outcome.as_str(), submission.comments, submission.submitted_at_unix_ms],
        )?;
        if inserted == 0 {
            return Err(StorageError::DuplicateReviewSubmission(
                submission.id.clone(),
            ));
        }
        Ok(())
    }

    /// Persists a validation result against one immutable revision or candidate.
    /// # Errors
    /// Returns an error for a missing target, duplicate ID, or storage failure.
    pub fn record_validation(&mut self, result: &ValidationResult) -> Result<(), StorageError> {
        ensure_target_exists(&self.connection, result.target())?;
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO validation_results(validation_result_id, target_kind, target_id, kind, environment, status, execution_id, recorded_at_unix_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![result.id.as_str(), result.target.kind(), result.target.id(), result.kind, result.environment, result.status.as_str(), result.execution_id, result.recorded_at_unix_ms],
        )?;
        if inserted == 0 {
            return Err(StorageError::DuplicateValidationResult(result.id.clone()));
        }
        Ok(())
    }

    /// Returns validation statuses recorded against one exact immutable target.
    /// # Errors
    /// Returns an error for a missing target or malformed persisted status.
    pub fn validation_statuses_for(
        &self,
        target: &Target,
    ) -> Result<Vec<ValidationStatus>, StorageError> {
        ensure_target_exists(&self.connection, target)?;
        let mut statement = self.connection.prepare("SELECT status FROM validation_results WHERE target_kind = ?1 AND target_id = ?2 ORDER BY recorded_at_unix_ms")?;
        let rows = statement.query_map(params![target.kind(), target.id()], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| ValidationStatus::parse(&row?)).collect()
    }

    /// Plans a provider-neutral integration from a fresh immutable candidate.
    /// # Errors
    /// Returns an error for a stale/missing candidate or reused operation ID.
    pub fn plan_integration(&mut self, attempt: &IntegrationAttempt) -> Result<(), StorageError> {
        if self.candidate_is_stale(&attempt.candidate_id)? {
            return Err(StorageError::StaleCandidate(attempt.candidate_id.clone()));
        }
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO integration_attempts(integration_id, repository_id, candidate_id, target_ref, expected_target_revision, provider, strategy, operation_id, actor, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'planned')",
            params![attempt.id.as_str(), attempt.repository_id.as_str(), attempt.candidate_id.as_str(), attempt.target_ref, attempt.expected_target_revision, attempt.provider, attempt.strategy, attempt.operation_id.as_str(), attempt.actor],
        )?;
        if inserted == 0 {
            return Err(StorageError::DuplicateOperation(
                attempt.operation_id.clone(),
            ));
        }
        Ok(())
    }

    /// Starts only a planned attempt when the provider-observed target is still exact.
    /// # Errors
    /// Returns a stale-target error rather than implicitly replanning.
    pub fn start_integration(
        &mut self,
        integration_id: &IntegrationId,
        observed_target: &str,
        now_unix_ms: i64,
    ) -> Result<(), StorageError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, String, String, String)> = transaction.query_row(
            "SELECT expected_target_revision, state, candidate_id, actor FROM integration_attempts WHERE integration_id = ?1", [integration_id.as_str()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        let (expected, state, candidate, actor) =
            row.ok_or_else(|| StorageError::MissingIntegration(integration_id.clone()))?;
        if expected != observed_target {
            return Err(StorageError::StaleTarget {
                expected,
                actual: observed_target.to_owned(),
            });
        }
        if IntegrationState::parse(&state)? != IntegrationState::Planned {
            return Err(StorageError::InvalidIntegrationTransition);
        }
        for input in candidate_inputs(&transaction, &CandidateId::new(candidate)?)? {
            let lease: Option<(String, i64)> = transaction.query_row(
                "SELECT holder, expires_at_unix_ms FROM leases WHERE change_id = ?1 AND operation = 'integrate'",
                [input.change_id().as_str()], |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional()?;
            if !matches!(lease, Some((ref holder, expiry)) if holder == &actor && expiry > now_unix_ms)
            {
                return Err(StorageError::IntegrationLeaseRequired(
                    input.change_id().clone(),
                ));
            }
        }
        let updated = transaction.execute(
            "UPDATE integration_attempts SET state = 'running' WHERE integration_id = ?1 AND state = 'planned'",
            [integration_id.as_str()],
        )?;
        if updated != 1 {
            return Err(StorageError::Invariant(
                "integration changed during immediate transaction",
            ));
        }
        transaction.commit()?;
        Ok(())
    }

    /// Records a terminal provider outcome; only verified success can create a receipt.
    /// # Errors
    /// Returns an error for invalid transitions or success without a receipt.
    pub fn finish_integration(
        &mut self,
        integration_id: &IntegrationId,
        next: IntegrationState,
        receipt: Option<&IntegrationReceipt>,
    ) -> Result<(), StorageError> {
        let state: String = self
            .connection
            .query_row(
                "SELECT state FROM integration_attempts WHERE integration_id = ?1",
                [integration_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingIntegration(integration_id.clone()))?;
        if !IntegrationState::parse(&state)?.may_transition_to(next) {
            return Err(StorageError::InvalidIntegrationTransition);
        }
        if next == IntegrationState::Succeeded && receipt.is_none() {
            return Err(StorageError::SuccessRequiresReceipt);
        }
        if let Some(receipt) = receipt {
            if receipt.integration_id != *integration_id {
                return Err(StorageError::ReceiptIntegrationMismatch);
            }
            if next != IntegrationState::Succeeded {
                return Err(StorageError::ReceiptRequiresSuccess);
            }
            self.connection.execute("INSERT INTO integration_receipts(receipt_id, integration_id, prior_target_revision, result_revision, provider_evidence) VALUES (?1, ?2, ?3, ?4, ?5)", params![receipt.id.as_str(), integration_id.as_str(), receipt.prior_target_revision, receipt.result_revision, receipt.provider_evidence])?;
        }
        self.connection.execute(
            "UPDATE integration_attempts SET state = ?1 WHERE integration_id = ?2",
            params![next.as_str(), integration_id.as_str()],
        )?;
        Ok(())
    }

    /// Creates the first immutable stack version from an ordered, duplicate-free Change list.
    /// # Errors
    /// Returns an error for missing Changes, empty/duplicate entries, or duplicate Stack ID.
    pub fn create_stack(
        &mut self,
        stack_id: StackId,
        changes: Vec<ChangeId>,
    ) -> Result<StackVersion, StorageError> {
        self.write_stack_version(stack_id, None, changes)
    }

    /// Appends a new immutable stack version only if the expected version is current.
    /// # Errors
    /// Returns an error for a stale/missing Stack or invalid entries.
    pub fn revise_stack(
        &mut self,
        stack_id: StackId,
        expected_version: i64,
        changes: Vec<ChangeId>,
    ) -> Result<StackVersion, StorageError> {
        self.write_stack_version(stack_id, Some(expected_version), changes)
    }

    fn write_stack_version(
        &mut self,
        stack_id: StackId,
        expected_version: Option<i64>,
        changes: Vec<ChangeId>,
    ) -> Result<StackVersion, StorageError> {
        if changes.is_empty() {
            return Err(StorageError::EmptyStack);
        }
        let mut seen = HashSet::new();
        if changes.iter().any(|change| !seen.insert(change.as_str())) {
            return Err(StorageError::DuplicateStackEntry);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version = match expected_version {
            None => {
                let inserted = transaction.execute(
                    "INSERT OR IGNORE INTO stacks(stack_id, current_version) VALUES (?1, 1)",
                    [stack_id.as_str()],
                )?;
                if inserted == 0 {
                    return Err(StorageError::DuplicateStack(stack_id));
                }
                1
            }
            Some(expected) => {
                let current: Option<i64> = transaction
                    .query_row(
                        "SELECT current_version FROM stacks WHERE stack_id = ?1",
                        [stack_id.as_str()],
                        |row| row.get(0),
                    )
                    .optional()?;
                let current =
                    current.ok_or_else(|| StorageError::MissingStack(stack_id.clone()))?;
                if current != expected {
                    return Err(StorageError::StaleStackVersion {
                        expected,
                        actual: current,
                    });
                }
                transaction.execute("UPDATE stacks SET current_version = ?1 WHERE stack_id = ?2 AND current_version = ?3", params![current + 1, stack_id.as_str(), expected])?;
                current + 1
            }
        };
        for (position, change) in changes.iter().enumerate() {
            ensure_change_exists(&transaction, change)?;
            transaction.execute("INSERT INTO stack_entries(stack_id, version, position, change_id) VALUES (?1, ?2, ?3, ?4)", params![stack_id.as_str(), version, position, change.as_str()])?;
        }
        transaction.commit()?;
        Ok(StackVersion {
            stack_id,
            version,
            changes,
        })
    }

    /// Loads one immutable stack version; no current ordering is inferred.
    /// # Errors
    /// Returns an error for a missing Stack/version or invalid persisted IDs.
    pub fn load_stack(
        &self,
        stack_id: &StackId,
        version: i64,
    ) -> Result<StackVersion, StorageError> {
        let mut statement = self.connection.prepare("SELECT change_id FROM stack_entries WHERE stack_id = ?1 AND version = ?2 ORDER BY position")?;
        let rows = statement.query_map(params![stack_id.as_str(), version], |row| {
            row.get::<_, String>(0)
        })?;
        let changes: Vec<ChangeId> = rows
            .map(|row| ChangeId::new(row?).map_err(StorageError::Domain))
            .collect::<Result<_, _>>()?;
        if changes.is_empty() {
            return Err(StorageError::MissingStackVersion {
                stack_id: stack_id.clone(),
                version,
            });
        }
        Ok(StackVersion {
            stack_id: stack_id.clone(),
            version,
            changes,
        })
    }

    /// Appends complete domain evidence without mutating historical events.
    /// # Errors
    /// Returns an error if storage cannot persist the event.
    pub fn record_domain_event(&mut self, event: &mut DomainEvent) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO domain_events(kind, actor, occurred_at_unix_ms, expected_state, resulting_state, affected_ids, operation_id, provider_evidence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![event.kind, event.actor, event.occurred_at_unix_ms, event.expected_state, event.resulting_state, event.affected_ids, event.operation_id, event.provider_evidence],
        )?;
        event.event_id = self.connection.last_insert_rowid();
        Ok(())
    }

    /// Records immutable provider conflict evidence and moves a running attempt to conflicted.
    /// # Errors
    /// Returns an error for missing/mismatched attempts or non-running state.
    pub fn record_integration_conflict(
        &mut self,
        conflict: &IntegrationConflict,
    ) -> Result<(), StorageError> {
        let row: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT candidate_id, state FROM integration_attempts WHERE integration_id = ?1",
                [conflict.integration_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (candidate, state) =
            row.ok_or_else(|| StorageError::MissingIntegration(conflict.integration_id.clone()))?;
        if candidate != conflict.candidate_id.as_str() {
            return Err(StorageError::ConflictCandidateMismatch);
        }
        if IntegrationState::parse(&state)? != IntegrationState::Running {
            return Err(StorageError::InvalidIntegrationTransition);
        }
        self.connection.execute("INSERT INTO integration_conflicts(conflict_id, integration_id, candidate_id, provider_state, attempted_operation, resolver, resulting_target, validation_evidence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![conflict.id.as_str(), conflict.integration_id.as_str(), conflict.candidate_id.as_str(), conflict.provider_state, conflict.attempted_operation, conflict.resolver, conflict.resulting_target, conflict.validation_evidence])?;
        self.connection.execute("UPDATE integration_attempts SET state = 'conflicted' WHERE integration_id = ?1 AND state = 'running'", [conflict.integration_id.as_str()])?;
        Ok(())
    }

    /// Records reconciliation evidence for a running or terminal integration attempt.
    /// # Errors
    /// Returns an error if the referenced attempt is absent or the record ID is duplicated.
    pub fn record_reconciliation(
        &mut self,
        record: &ReconciliationRecord,
    ) -> Result<(), StorageError> {
        let exists = self
            .connection
            .query_row(
                "SELECT 1 FROM integration_attempts WHERE integration_id = ?1",
                [record.integration_id.as_str()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(StorageError::MissingIntegration(
                record.integration_id.clone(),
            ));
        }
        self.connection.execute(
            "INSERT INTO reconciliation_records(reconciliation_id, integration_id, observed_state, evidence, resolved) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![record.id.as_str(), record.integration_id.as_str(), record.observed_state, record.evidence, record.resolved],
        )?;
        Ok(())
    }

    /// Adds a non-dependency relationship between two existing Changes.
    /// # Errors
    /// Returns an error for missing endpoints, self-links, or duplicate relations.
    pub fn add_change_relation(
        &mut self,
        source: &ChangeId,
        target: &ChangeId,
        kind: ChangeRelationKind,
    ) -> Result<(), StorageError> {
        if source == target {
            return Err(StorageError::Invariant(
                "change relation cannot be self-referential",
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_change_exists(&transaction, source)?;
        ensure_change_exists(&transaction, target)?;
        let inserted = transaction.execute("INSERT OR IGNORE INTO change_relations(source_change_id, target_change_id, kind) VALUES (?1, ?2, ?3)", params![source.as_str(), target.as_str(), kind.as_str()])?;
        if inserted == 0 {
            return Err(StorageError::DuplicateChangeRelation);
        }
        transaction.commit()?;
        Ok(())
    }

    /// Persists a non-conclusive overlap signal for two exact existing revisions.
    /// # Errors
    /// Returns an error for missing revisions or duplicate overlap identity.
    pub fn record_overlap(&mut self, overlap: &Overlap) -> Result<(), StorageError> {
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO overlaps(overlap_id, left_revision_id, right_revision_id, detail) VALUES (?1, ?2, ?3, ?4)",
            params![overlap.id.as_str(), overlap.left_revision.as_str(), overlap.right_revision.as_str(), overlap.detail],
        )?;
        if inserted == 0 {
            return Err(StorageError::DuplicateOverlap(overlap.id.clone()));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Domain(ChangeError),
    InvalidArtifact(ChangeError),
    InvalidDigest(String),
    DigestMismatch(String),
    DuplicateChange(ChangeId),
    DuplicateRevision(RevisionId),
    DuplicateCandidate(CandidateId),
    DuplicateAssignment(AssignmentId),
    DuplicateMaterialization(MaterializationId),
    DuplicateReviewRequest(ReviewRequestId),
    DuplicateReviewSubmission(ReviewSubmissionId),
    DuplicateValidationResult(ValidationResultId),
    DuplicateOperation(OperationId),
    DuplicateCandidateInput(ChangeId),
    DuplicateDependency,
    MissingChange(ChangeId),
    MissingRevision(RevisionId),
    MissingCandidate(CandidateId),
    MissingMaterialization(MaterializationId),
    MissingIntegration(IntegrationId),
    StaleCandidate(CandidateId),
    StaleTarget {
        expected: String,
        actual: String,
    },
    InvalidIntegrationTransition,
    SuccessRequiresReceipt,
    ReceiptRequiresSuccess,
    ReceiptIntegrationMismatch,
    IntegrationLeaseRequired(ChangeId),
    DuplicateStack(StackId),
    MissingStack(StackId),
    MissingStackVersion {
        stack_id: StackId,
        version: i64,
    },
    EmptyStack,
    DuplicateStackEntry,
    ConflictCandidateMismatch,
    DuplicateChangeRelation,
    CandidateStackMismatch,
    DuplicateOverlap(OverlapId),
    StaleStackVersion {
        expected: i64,
        actual: i64,
    },
    EmptyCandidate,
    RevisionDoesNotBelongToChange {
        revision_id: RevisionId,
        change_id: ChangeId,
    },
    CandidateRepositoryMismatch {
        revision_id: RevisionId,
        expected: RepositoryId,
    },
    UnresolvedDependency(Dependency),
    CandidateDependencyOrder(Dependency),
    InvalidMaterializationTransition {
        expected: MaterializationState,
        next: MaterializationState,
    },
    StaleMaterializationState {
        expected: MaterializationState,
        actual: MaterializationState,
    },
    DependencyCycle,
    UnsupportedSchemaVersion(i64),
    InvalidLeaseValue(&'static str),
    InvalidLeaseExpiry,
    LeaseHeld {
        holder: String,
        expires_at_unix_ms: i64,
    },
    LeaseLost,
    StaleHead {
        expected: Option<RevisionId>,
        actual: Option<RevisionId>,
    },
    Invariant(&'static str),
}

#[allow(clippy::too_many_lines)]
impl Display for StorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "storage I/O failure: {error}"),
            Self::Sqlite(error) => write!(formatter, "SQLite failure: {error}"),
            Self::Domain(error) | Self::InvalidArtifact(error) => {
                write!(formatter, "invalid domain state: {error}")
            }
            Self::InvalidDigest(digest) => write!(formatter, "invalid content digest: {digest}"),
            Self::DigestMismatch(digest) => write!(formatter, "content digest mismatch: {digest}"),
            Self::DuplicateChange(id) => write!(formatter, "duplicate change: {}", id.as_str()),
            Self::DuplicateRevision(id) => write!(formatter, "duplicate revision: {}", id.as_str()),
            Self::DuplicateCandidate(id) => {
                write!(formatter, "duplicate candidate: {}", id.as_str())
            }
            Self::DuplicateAssignment(id) => {
                write!(formatter, "duplicate assignment: {}", id.as_str())
            }
            Self::DuplicateMaterialization(id) => {
                write!(formatter, "duplicate materialization: {}", id.as_str())
            }
            Self::DuplicateReviewRequest(id) => {
                write!(formatter, "duplicate review request: {}", id.as_str())
            }
            Self::DuplicateReviewSubmission(id) => {
                write!(formatter, "duplicate review submission: {}", id.as_str())
            }
            Self::DuplicateValidationResult(id) => {
                write!(formatter, "duplicate validation result: {}", id.as_str())
            }
            Self::DuplicateOperation(id) => {
                write!(formatter, "duplicate operation: {}", id.as_str())
            }
            Self::DuplicateCandidateInput(id) => {
                write!(
                    formatter,
                    "duplicate candidate input change: {}",
                    id.as_str()
                )
            }
            Self::DuplicateDependency => formatter.write_str("duplicate dependency"),
            Self::MissingChange(id) => write!(formatter, "missing change: {}", id.as_str()),
            Self::MissingRevision(id) => write!(formatter, "missing revision: {}", id.as_str()),
            Self::MissingCandidate(id) => write!(formatter, "missing candidate: {}", id.as_str()),
            Self::MissingMaterialization(id) => {
                write!(formatter, "missing materialization: {}", id.as_str())
            }
            Self::MissingIntegration(id) => {
                write!(formatter, "missing integration: {}", id.as_str())
            }
            Self::StaleCandidate(id) => write!(formatter, "stale candidate: {}", id.as_str()),
            Self::StaleTarget { expected, actual } => write!(
                formatter,
                "stale target: expected {expected}, actual {actual}"
            ),
            Self::InvalidIntegrationTransition => {
                formatter.write_str("invalid integration transition")
            }
            Self::SuccessRequiresReceipt => {
                formatter.write_str("successful integration requires a verified receipt")
            }
            Self::ReceiptRequiresSuccess => {
                formatter.write_str("receipt requires successful integration")
            }
            Self::ReceiptIntegrationMismatch => {
                formatter.write_str("receipt does not match integration")
            }
            Self::IntegrationLeaseRequired(id) => write!(
                formatter,
                "integration lease required for change: {}",
                id.as_str()
            ),
            Self::DuplicateStack(id) => write!(formatter, "duplicate stack: {}", id.as_str()),
            Self::MissingStack(id) => write!(formatter, "missing stack: {}", id.as_str()),
            Self::MissingStackVersion { stack_id, version } => write!(
                formatter,
                "missing stack version: {}@{version}",
                stack_id.as_str()
            ),
            Self::EmptyStack => formatter.write_str("stack requires at least one change"),
            Self::DuplicateStackEntry => formatter.write_str("stack contains a duplicate change"),
            Self::ConflictCandidateMismatch => {
                formatter.write_str("conflict candidate does not match integration")
            }
            Self::DuplicateChangeRelation => formatter.write_str("duplicate change relation"),
            Self::CandidateStackMismatch => {
                formatter.write_str("candidate inputs do not match stack version")
            }
            Self::DuplicateOverlap(id) => write!(formatter, "duplicate overlap: {}", id.as_str()),
            Self::StaleStackVersion { expected, actual } => write!(
                formatter,
                "stale stack version: expected {expected}, actual {actual}"
            ),
            Self::EmptyCandidate => formatter.write_str("candidate requires at least one input"),
            Self::RevisionDoesNotBelongToChange {
                revision_id,
                change_id,
            } => write!(
                formatter,
                "revision {} does not belong to change {}",
                revision_id.as_str(),
                change_id.as_str()
            ),
            Self::CandidateRepositoryMismatch {
                revision_id,
                expected,
            } => write!(
                formatter,
                "revision {} is not in candidate repository {}",
                revision_id.as_str(),
                expected.as_str()
            ),
            Self::UnresolvedDependency(dependency) => write!(
                formatter,
                "candidate lacks required dependency {}@{} for {}",
                dependency.upstream_change_id().as_str(),
                dependency.upstream_revision_id().as_str(),
                dependency.downstream_change_id().as_str()
            ),
            Self::CandidateDependencyOrder(dependency) => write!(
                formatter,
                "candidate orders dependent {} before required upstream {}",
                dependency.downstream_change_id().as_str(),
                dependency.upstream_change_id().as_str()
            ),
            Self::InvalidMaterializationTransition { expected, next } => write!(
                formatter,
                "invalid materialization transition from {} to {}",
                expected.as_str(),
                next.as_str()
            ),
            Self::StaleMaterializationState { expected, actual } => write!(
                formatter,
                "stale materialization state: expected {}, actual {}",
                expected.as_str(),
                actual.as_str()
            ),
            Self::DependencyCycle => formatter.write_str("dependency would create a cycle"),
            Self::UnsupportedSchemaVersion(version) => write!(
                formatter,
                "database schema version {version} is newer than this Weft build"
            ),
            Self::InvalidLeaseValue(kind) => write!(formatter, "invalid lease {kind}"),
            Self::InvalidLeaseExpiry => formatter.write_str("lease expiry must be in the future"),
            Self::LeaseHeld {
                holder,
                expires_at_unix_ms,
            } => write!(
                formatter,
                "lease held by {holder} until {expires_at_unix_ms}"
            ),
            Self::LeaseLost => formatter.write_str("lease is no longer held by this actor"),
            Self::StaleHead { expected, actual } => write!(
                formatter,
                "stale revision head: expected {:?}, actual {:?}",
                expected.as_ref().map(RevisionId::as_str),
                actual.as_ref().map(RevisionId::as_str)
            ),
            Self::Invariant(message) => write!(formatter, "storage invariant failed: {message}"),
        }
    }
}

impl Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<ChangeError> for StorageError {
    fn from(error: ChangeError) -> Self {
        Self::Domain(error)
    }
}

fn ensure_change_exists(
    transaction: &rusqlite::Transaction<'_>,
    change_id: &ChangeId,
) -> Result<(), StorageError> {
    let exists = transaction
        .query_row(
            "SELECT 1 FROM changes WHERE change_id = ?1",
            [change_id.as_str()],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(StorageError::MissingChange(change_id.clone()))
    }
}

fn ensure_target_exists(connection: &Connection, target: &Target) -> Result<(), StorageError> {
    match target {
        Target::Revision(id) => {
            let exists = connection
                .query_row(
                    "SELECT 1 FROM revisions WHERE revision_id = ?1",
                    [id.as_str()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if exists {
                Ok(())
            } else {
                Err(StorageError::MissingRevision(id.clone()))
            }
        }
        Target::Candidate(id) => {
            let exists = connection
                .query_row(
                    "SELECT 1 FROM candidates WHERE candidate_id = ?1",
                    [id.as_str()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if exists {
                Ok(())
            } else {
                Err(StorageError::MissingCandidate(id.clone()))
            }
        }
    }
}

fn dependencies_for_inputs(
    transaction: &rusqlite::Transaction<'_>,
    inputs: &[CandidateInput],
) -> Result<Vec<Dependency>, StorageError> {
    let mut dependencies = Vec::new();
    for input in inputs {
        let mut statement = transaction.prepare(
            "SELECT upstream_change_id, upstream_revision_id FROM dependencies
             WHERE downstream_change_id = ?1 ORDER BY upstream_change_id",
        )?;
        let rows = statement.query_map([input.change_id().as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (upstream_change, upstream_revision) = row?;
            dependencies.push(Dependency::new(
                ChangeId::new(upstream_change)?,
                RevisionId::new(upstream_revision)?,
                input.change_id().clone(),
            ));
        }
    }
    Ok(dependencies)
}

fn validate_candidate_inputs(
    transaction: &rusqlite::Transaction<'_>,
    target_base: &BaseState,
    inputs: &[CandidateInput],
) -> Result<Vec<Dependency>, StorageError> {
    if inputs.is_empty() {
        return Err(StorageError::EmptyCandidate);
    }
    let mut candidate_changes = HashSet::new();
    for input in inputs {
        if !candidate_changes.insert(input.change_id().as_str()) {
            return Err(StorageError::DuplicateCandidateInput(
                input.change_id().clone(),
            ));
        }
        let row: Option<(String, String)> = transaction
            .query_row(
                "SELECT change_id, repository_id FROM revisions WHERE revision_id = ?1",
                [input.revision_id().as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (revision_change, repository_id) =
            row.ok_or_else(|| StorageError::MissingRevision(input.revision_id().clone()))?;
        if revision_change != input.change_id().as_str() {
            return Err(StorageError::RevisionDoesNotBelongToChange {
                revision_id: input.revision_id().clone(),
                change_id: input.change_id().clone(),
            });
        }
        if repository_id != target_base.repository_id().as_str() {
            return Err(StorageError::CandidateRepositoryMismatch {
                revision_id: input.revision_id().clone(),
                expected: target_base.repository_id().clone(),
            });
        }
    }
    let dependencies = dependencies_for_inputs(transaction, inputs)?;
    let positions: HashMap<_, _> = inputs
        .iter()
        .enumerate()
        .map(|(position, input)| (input.change_id(), position))
        .collect();
    for dependency in &dependencies {
        let Some(&upstream) = positions.get(dependency.upstream_change_id()) else {
            return Err(StorageError::UnresolvedDependency(dependency.clone()));
        };
        let Some(&downstream) = positions.get(dependency.downstream_change_id()) else {
            return Err(StorageError::UnresolvedDependency(dependency.clone()));
        };
        if inputs[upstream].revision_id() != dependency.upstream_revision_id() {
            return Err(StorageError::UnresolvedDependency(dependency.clone()));
        }
        if upstream >= downstream {
            return Err(StorageError::CandidateDependencyOrder(dependency.clone()));
        }
    }
    Ok(dependencies)
}

fn candidate_inputs(
    connection: &Connection,
    candidate_id: &CandidateId,
) -> Result<Vec<CandidateInput>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT change_id, revision_id FROM candidate_inputs
         WHERE candidate_id = ?1 ORDER BY position",
    )?;
    let rows = statement.query_map([candidate_id.as_str()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (change, revision) = row?;
        Ok(CandidateInput::new(
            ChangeId::new(change)?,
            RevisionId::new(revision)?,
        ))
    })
    .collect()
}

fn candidate_dependencies(
    connection: &Connection,
    candidate_id: &CandidateId,
) -> Result<Vec<Dependency>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT upstream_change_id, upstream_revision_id, downstream_change_id
         FROM candidate_dependencies WHERE candidate_id = ?1
         ORDER BY downstream_change_id, upstream_change_id",
    )?;
    let rows = statement.query_map([candidate_id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (upstream_change, upstream_revision, downstream_change) = row?;
        Ok(Dependency::new(
            ChangeId::new(upstream_change)?,
            RevisionId::new(upstream_revision)?,
            ChangeId::new(downstream_change)?,
        ))
    })
    .collect()
}

fn candidate_digest(
    target_base: &BaseState,
    inputs: &[CandidateInput],
    dependencies: &[Dependency],
) -> String {
    let mut bytes = b"weft/composition-candidate-v1\0".to_vec();
    write_candidate_string(&mut bytes, target_base.repository_id().as_str());
    write_candidate_string(&mut bytes, target_base.object_id());
    write_candidate_count(&mut bytes, inputs.len());
    for input in inputs {
        write_candidate_string(&mut bytes, input.change_id().as_str());
        write_candidate_string(&mut bytes, input.revision_id().as_str());
    }
    let mut ordered_dependencies: Vec<_> = dependencies.iter().collect();
    ordered_dependencies.sort_by(|left, right| {
        (
            left.downstream_change_id().as_str(),
            left.upstream_change_id().as_str(),
            left.upstream_revision_id().as_str(),
        )
            .cmp(&(
                right.downstream_change_id().as_str(),
                right.upstream_change_id().as_str(),
                right.upstream_revision_id().as_str(),
            ))
    });
    write_candidate_count(&mut bytes, ordered_dependencies.len());
    for dependency in ordered_dependencies {
        write_candidate_string(&mut bytes, dependency.upstream_change_id().as_str());
        write_candidate_string(&mut bytes, dependency.upstream_revision_id().as_str());
        write_candidate_string(&mut bytes, dependency.downstream_change_id().as_str());
    }
    sha256_digest(&bytes)
}

fn write_candidate_string(bytes: &mut Vec<u8>, value: &str) {
    write_candidate_count(bytes, value.len());
    bytes.extend_from_slice(value.as_bytes());
}

fn write_candidate_count(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend_from_slice(&(value as u64).to_be_bytes());
}

fn valid_lease_value(value: String, kind: &'static str) -> Result<String, StorageError> {
    valid_event_value(value, kind)
}

fn valid_event_value(value: String, kind: &'static str) -> Result<String, StorageError> {
    if value.trim().is_empty() || value != value.trim() || value.chars().any(char::is_control) {
        return Err(StorageError::InvalidLeaseValue(kind));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;
    use crate::{BaseState, FileMode, PathOperation, RepositoryId, TreeDelta};

    static NEXT_TEMPORARY_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory() -> PathBuf {
        let sequence = NEXT_TEMPORARY_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weft-domain-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn artifact(store: &ContentStore) -> CanonicalArtifact {
        let binary = store.put_blob(b"\0binary\xff").unwrap();
        let symlink = store.put_blob(b"target").unwrap();
        CanonicalArtifact::new(
            BaseState::new(RepositoryId::new("repo-1").unwrap(), "git:deadbeef").unwrap(),
            TreeDelta::new(vec![
                PathOperation::Upsert {
                    path: "bin/tool".to_owned(),
                    mode: FileMode::Executable,
                    blob_digest: binary,
                },
                PathOperation::Delete {
                    path: "old.txt".to_owned(),
                },
                PathOperation::Upsert {
                    path: "src/link".to_owned(),
                    mode: FileMode::SymbolicLink,
                    blob_digest: symlink,
                },
            ])
            .unwrap(),
        )
    }

    #[test]
    fn persists_and_reopens_verified_canonical_artifacts() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let database = root.join("weft.sqlite");
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, revision.clone(), &artifact)
                .unwrap();
        }
        let repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(
            repository.load_change(&change).unwrap().head(),
            Some(&revision)
        );
        assert_eq!(
            repository.load_artifact_for_revision(&revision).unwrap(),
            artifact
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_tampered_content_addressed_artifacts() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        store.put_artifact(&artifact).unwrap();
        let digest = artifact.digest().strip_prefix("sha256:").unwrap();
        let artifact_path = store.root.join("artifacts").join("sha256").join(digest);
        fs::write(artifact_path, b"tampered").unwrap();
        assert!(matches!(
            store.read_artifact(artifact.digest()),
            Err(StorageError::DigestMismatch(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_to_persist_an_artifact_with_a_missing_blob() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let missing = sha256_digest(b"missing");
        let artifact = CanonicalArtifact::new(
            BaseState::new(RepositoryId::new("repo-1").unwrap(), "git:deadbeef").unwrap(),
            TreeDelta::new(vec![PathOperation::Upsert {
                path: "missing".to_owned(),
                mode: FileMode::Regular,
                blob_digest: missing,
            }])
            .unwrap(),
        );
        assert!(matches!(
            store.put_artifact(&artifact),
            Err(StorageError::Io(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_a_database_created_by_a_newer_build() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        {
            let repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository
                .connection
                .execute(
                    "INSERT INTO schema_migrations(version) VALUES (?1)",
                    [SCHEMA_VERSION + 1],
                )
                .unwrap();
        }
        assert!(matches!(
            SqliteRepository::open(database, store),
            Err(StorageError::UnsupportedSchemaVersion(version)) if version == SCHEMA_VERSION + 1
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn records_atomic_history_and_recovers_expired_leases() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let database = root.join("weft.sqlite");
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        let mut repository = SqliteRepository::open(&database, store).unwrap();
        repository.create_change(change.clone()).unwrap();
        repository
            .append_revision(&change, None, revision, &artifact)
            .unwrap();
        let lease = repository
            .acquire_lease(&change, "integrate", "agent-a", 100, 200)
            .unwrap();
        assert_eq!(lease.holder(), "agent-a");
        assert!(matches!(
            repository.acquire_lease(&change, "integrate", "agent-b", 150, 300),
            Err(StorageError::LeaseHeld { .. })
        ));
        let recovered = repository
            .acquire_lease(&change, "integrate", "agent-b", 200, 300)
            .unwrap();
        assert_eq!(recovered.holder(), "agent-b");
        let kinds: Vec<_> = repository
            .audit_events(&change)
            .unwrap()
            .into_iter()
            .map(|event| event.kind().to_owned())
            .collect();
        assert_eq!(
            kinds,
            vec![
                "change-created",
                "revision-appended",
                "lease-acquired",
                "lease-acquired"
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn renews_and_releases_only_active_holder_leases() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let mut repository = SqliteRepository::open(root.join("weft.sqlite"), store).unwrap();
        let change = ChangeId::new("change-1").unwrap();
        repository.create_change(change.clone()).unwrap();
        let lease = repository
            .acquire_lease(&change, "integrate", "agent-a", 100, 200)
            .unwrap();
        let renewed = repository.renew_lease(&lease, 150, 300).unwrap();
        assert_eq!(renewed.expires_at_unix_ms(), 300);
        assert!(matches!(
            repository.renew_lease(&renewed, 300, 400),
            Err(StorageError::LeaseLost)
        ));
        repository.release_lease(&renewed).unwrap();
        assert!(matches!(
            repository.release_lease(&renewed),
            Err(StorageError::LeaseLost)
        ));
        assert_eq!(
            repository
                .acquire_lease(&change, "integrate", "agent-b", 151, 250)
                .unwrap()
                .holder(),
            "agent-b"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_competing_successors_from_independent_connections() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let change = ChangeId::new("change-1").unwrap();
        let root_revision = RevisionId::new("revision-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, root_revision.clone(), &artifact)
                .unwrap();
        }
        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for revision in ["revision-2a", "revision-2b"] {
            let database = database.clone();
            let store = store.clone();
            let change = change.clone();
            let expected = root_revision.clone();
            let artifact = artifact.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                let mut repository = SqliteRepository::open(database, store).unwrap();
                barrier.wait();
                repository.append_revision(
                    &change,
                    Some(&expected),
                    RevisionId::new(revision).unwrap(),
                    &artifact,
                )
            }));
        }
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(StorageError::StaleHead { .. })))
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_exact_dependency_candidate_and_marks_it_stale_after_an_advance() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let upstream = ChangeId::new("upstream").unwrap();
        let downstream = ChangeId::new("downstream").unwrap();
        let upstream_revision = RevisionId::new("upstream-r1").unwrap();
        let downstream_revision = RevisionId::new("downstream-r1").unwrap();
        let candidate_id = CandidateId::new("candidate-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(upstream.clone()).unwrap();
            repository.create_change(downstream.clone()).unwrap();
            repository
                .append_revision(&upstream, None, upstream_revision.clone(), &artifact)
                .unwrap();
            repository
                .append_revision(&downstream, None, downstream_revision.clone(), &artifact)
                .unwrap();
            repository
                .add_dependency(&Dependency::new(
                    upstream.clone(),
                    upstream_revision.clone(),
                    downstream.clone(),
                ))
                .unwrap();
            let candidate = repository
                .create_candidate(
                    candidate_id.clone(),
                    artifact.base().clone(),
                    vec![
                        CandidateInput::new(upstream.clone(), upstream_revision.clone()),
                        CandidateInput::new(downstream.clone(), downstream_revision.clone()),
                    ],
                )
                .unwrap();
            assert_eq!(candidate.resolved_dependencies().len(), 1);
            assert!(!repository.candidate_is_stale(&candidate_id).unwrap());
        }
        let mut repository = SqliteRepository::open(&database, store).unwrap();
        let persisted = repository.load_candidate(&candidate_id).unwrap();
        assert_eq!(persisted.inputs()[0].revision_id(), &upstream_revision);
        assert_eq!(persisted.inputs()[1].revision_id(), &downstream_revision);
        repository
            .append_revision(
                &upstream,
                Some(&upstream_revision),
                RevisionId::new("upstream-r2").unwrap(),
                &artifact,
            )
            .unwrap();
        assert!(repository.candidate_is_stale(&candidate_id).unwrap());
        assert_eq!(
            repository.load_candidate(&candidate_id).unwrap().inputs()[0].revision_id(),
            &upstream_revision
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_dependency_cycles_and_candidates_without_exact_pins() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let mut repository = SqliteRepository::open(root.join("weft.sqlite"), store).unwrap();
        let changes: Vec<_> = ["a", "b", "c"]
            .into_iter()
            .map(|id| ChangeId::new(id).unwrap())
            .collect();
        let revisions: Vec<_> = ["a-r1", "b-r1", "c-r1"]
            .into_iter()
            .map(|id| RevisionId::new(id).unwrap())
            .collect();
        for (change, revision) in changes.iter().zip(&revisions) {
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(change, None, revision.clone(), &artifact)
                .unwrap();
        }
        repository
            .add_dependency(&Dependency::new(
                changes[0].clone(),
                revisions[0].clone(),
                changes[1].clone(),
            ))
            .unwrap();
        repository
            .add_dependency(&Dependency::new(
                changes[1].clone(),
                revisions[1].clone(),
                changes[2].clone(),
            ))
            .unwrap();
        assert!(matches!(
            repository.add_dependency(&Dependency::new(
                changes[2].clone(),
                revisions[2].clone(),
                changes[0].clone(),
            )),
            Err(StorageError::DependencyCycle)
        ));
        assert!(matches!(
            repository.create_candidate(
                CandidateId::new("candidate-missing-pin").unwrap(),
                artifact.base().clone(),
                vec![CandidateInput::new(
                    changes[1].clone(),
                    revisions[1].clone()
                )],
            ),
            Err(StorageError::UnresolvedDependency(_))
        ));
        assert!(matches!(
            repository.create_candidate(
                CandidateId::new("candidate-wrong-order").unwrap(),
                artifact.base().clone(),
                vec![
                    CandidateInput::new(changes[1].clone(), revisions[1].clone()),
                    CandidateInput::new(changes[0].clone(), revisions[0].clone()),
                ],
            ),
            Err(StorageError::CandidateDependencyOrder(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn candidate_order_is_immutable_and_changes_its_digest() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let mut repository = SqliteRepository::open(root.join("weft.sqlite"), store).unwrap();
        let first_change = ChangeId::new("first").unwrap();
        let second_change = ChangeId::new("second").unwrap();
        let first_revision = RevisionId::new("first-r1").unwrap();
        let second_revision = RevisionId::new("second-r1").unwrap();
        for (change, revision) in [
            (&first_change, &first_revision),
            (&second_change, &second_revision),
        ] {
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(change, None, revision.clone(), &artifact)
                .unwrap();
        }
        let forward = repository
            .create_candidate(
                CandidateId::new("candidate-forward").unwrap(),
                artifact.base().clone(),
                vec![
                    CandidateInput::new(first_change.clone(), first_revision.clone()),
                    CandidateInput::new(second_change.clone(), second_revision.clone()),
                ],
            )
            .unwrap();
        let reverse = repository
            .create_candidate(
                CandidateId::new("candidate-reverse").unwrap(),
                artifact.base().clone(),
                vec![
                    CandidateInput::new(second_change, second_revision),
                    CandidateInput::new(first_change, first_revision),
                ],
            )
            .unwrap();
        assert_ne!(forward.content_digest(), reverse.content_digest());
        assert_eq!(
            forward.inputs(),
            repository
                .load_candidate(forward.candidate_id())
                .unwrap()
                .inputs()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn records_exact_stack_version_for_candidate_inputs() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let mut repository = SqliteRepository::open(root.join("weft.sqlite"), store).unwrap();
        let first = ChangeId::new("first").unwrap();
        let second = ChangeId::new("second").unwrap();
        let first_revision = RevisionId::new("first-r1").unwrap();
        let second_revision = RevisionId::new("second-r1").unwrap();
        for (change, revision) in [(&first, &first_revision), (&second, &second_revision)] {
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(change, None, revision.clone(), &artifact)
                .unwrap();
        }
        let stack = StackId::new("stack-1").unwrap();
        repository
            .create_stack(stack.clone(), vec![first.clone(), second.clone()])
            .unwrap();
        repository
            .create_candidate_from_stack(
                &CandidateId::new("candidate-1").unwrap(),
                artifact.base().clone(),
                vec![
                    CandidateInput::new(first.clone(), first_revision.clone()),
                    CandidateInput::new(second.clone(), second_revision.clone()),
                ],
                &stack,
                1,
            )
            .unwrap();
        assert_eq!(
            repository
                .connection
                .query_row(
                    "SELECT stack_version FROM candidates WHERE candidate_id = 'candidate-1'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert!(matches!(
            repository.create_candidate_from_stack(
                &CandidateId::new("candidate-bad").unwrap(),
                artifact.base().clone(),
                vec![
                    CandidateInput::new(second, second_revision),
                    CandidateInput::new(first, first_revision)
                ],
                &stack,
                1
            ),
            Err(StorageError::CandidateStackMismatch)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_assignment_history_and_guards_materialization_transitions() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        let materialization = MaterializationId::new("materialization-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, revision.clone(), &artifact)
                .unwrap();
            let assignment = Assignment::new(
                AssignmentId::new("assignment-1").unwrap(),
                change.clone(),
                "agent-1",
                "implementer",
                "operator",
                100,
            )
            .unwrap();
            repository.record_assignment(&assignment).unwrap();
            assert_eq!(
                repository.assignments_for(&change).unwrap(),
                vec![assignment]
            );
            assert_eq!(
                repository
                    .create_materialization(
                        materialization.clone(),
                        revision.clone(),
                        WorkspaceId::new("workspace-1").unwrap(),
                        "native-git",
                        "worktree:one",
                    )
                    .unwrap()
                    .state(),
                MaterializationState::Clean
            );
            assert!(matches!(
                repository.transition_materialization(
                    &materialization,
                    MaterializationState::Clean,
                    MaterializationState::Clean,
                ),
                Err(StorageError::InvalidMaterializationTransition { .. })
            ));
            assert_eq!(
                repository
                    .transition_materialization(
                        &materialization,
                        MaterializationState::Clean,
                        MaterializationState::Dirty,
                    )
                    .unwrap()
                    .state(),
                MaterializationState::Dirty
            );
            assert!(matches!(
                repository.transition_materialization(
                    &materialization,
                    MaterializationState::Clean,
                    MaterializationState::Released,
                ),
                Err(StorageError::StaleMaterializationState { .. })
            ));
        }
        let repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(
            repository.assignments_for(&change).unwrap()[0].role(),
            "implementer"
        );
        assert_eq!(
            repository
                .load_materialization(&materialization)
                .unwrap()
                .state(),
            MaterializationState::Dirty
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_exact_review_and_validation_targets() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, revision.clone(), &artifact)
                .unwrap();
            let request = ReviewRequest::new(
                ReviewRequestId::new("review-1").unwrap(),
                Target::Revision(revision.clone()),
                "author",
                "reviewer",
                100,
            )
            .unwrap();
            repository.create_review_request(&request).unwrap();
            repository
                .submit_review(
                    &ReviewSubmission::new(
                        ReviewSubmissionId::new("submission-1").unwrap(),
                        request.id().clone(),
                        "reviewer",
                        ReviewOutcome::Approved,
                        "looks good",
                        101,
                    )
                    .unwrap(),
                )
                .unwrap();
            repository
                .record_validation(
                    &ValidationResult::new(
                        ValidationResultId::new("validation-1").unwrap(),
                        Target::Revision(revision.clone()),
                        "test",
                        "local",
                        ValidationStatus::Passed,
                        "run-1",
                        102,
                    )
                    .unwrap(),
                )
                .unwrap();
            assert!(matches!(
                repository.create_review_request(
                    &ReviewRequest::new(
                        ReviewRequestId::new("review-missing").unwrap(),
                        Target::Revision(RevisionId::new("missing").unwrap()),
                        "author",
                        "reviewer",
                        100,
                    )
                    .unwrap()
                ),
                Err(StorageError::MissingRevision(_))
            ));
        }
        let repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(
            repository
                .connection
                .query_row("SELECT COUNT(*) FROM review_requests", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            repository
                .connection
                .query_row("SELECT COUNT(*) FROM review_submissions", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            repository
                .connection
                .query_row("SELECT COUNT(*) FROM validation_results", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marks_revision_targets_stale_after_head_advances() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let mut repository = SqliteRepository::open(root.join("weft.sqlite"), store).unwrap();
        let change = ChangeId::new("change-1").unwrap();
        let first = RevisionId::new("revision-1").unwrap();
        repository.create_change(change.clone()).unwrap();
        repository
            .append_revision(&change, None, first.clone(), &artifact)
            .unwrap();
        assert!(
            !repository
                .target_is_stale(&Target::Revision(first.clone()))
                .unwrap()
        );
        repository
            .append_revision(
                &change,
                Some(&first),
                RevisionId::new("revision-2").unwrap(),
                &artifact,
            )
            .unwrap();
        assert!(
            repository
                .target_is_stale(&Target::Revision(first))
                .unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn returns_validation_history_for_exact_target() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let mut repository = SqliteRepository::open(root.join("weft.sqlite"), store).unwrap();
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        repository.create_change(change.clone()).unwrap();
        repository
            .append_revision(&change, None, revision.clone(), &artifact)
            .unwrap();
        let target = Target::Revision(revision);
        repository
            .record_validation(
                &ValidationResult::new(
                    ValidationResultId::new("v1").unwrap(),
                    target.clone(),
                    "test",
                    "local",
                    ValidationStatus::Failed,
                    "run-1",
                    1,
                )
                .unwrap(),
            )
            .unwrap();
        repository
            .record_validation(
                &ValidationResult::new(
                    ValidationResultId::new("v2").unwrap(),
                    target.clone(),
                    "test",
                    "local",
                    ValidationStatus::Passed,
                    "run-2",
                    2,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            repository.validation_statuses_for(&target).unwrap(),
            vec![ValidationStatus::Failed, ValidationStatus::Passed]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn guards_integration_target_operation_and_success_receipt() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        let candidate_id = CandidateId::new("candidate-1").unwrap();
        let integration_id = IntegrationId::new("integration-1").unwrap();
        let operation_id = OperationId::new("operation-1").unwrap();
        let attempt = IntegrationAttempt::new(
            integration_id.clone(),
            RepositoryId::new("repo-1").unwrap(),
            candidate_id.clone(),
            "refs/heads/main",
            "target-r1",
            "native-git",
            "merge",
            operation_id.clone(),
            "agent-1",
        )
        .unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, revision.clone(), &artifact)
                .unwrap();
            repository
                .create_candidate(
                    candidate_id.clone(),
                    artifact.base().clone(),
                    vec![CandidateInput::new(change, revision)],
                )
                .unwrap();
            repository.plan_integration(&attempt).unwrap();
            assert!(matches!(
                repository.plan_integration(&attempt),
                Err(StorageError::DuplicateOperation(_))
            ));
            assert!(matches!(
                repository.start_integration(&integration_id, "target-r2", 100),
                Err(StorageError::StaleTarget { .. })
            ));
            assert!(matches!(
                repository.start_integration(&integration_id, "target-r1", 100),
                Err(StorageError::IntegrationLeaseRequired(_))
            ));
            repository
                .acquire_lease(
                    &ChangeId::new("change-1").unwrap(),
                    "integrate",
                    "agent-1",
                    100,
                    200,
                )
                .unwrap();
            repository
                .start_integration(&integration_id, "target-r1", 100)
                .unwrap();
            assert!(matches!(
                repository.finish_integration(&integration_id, IntegrationState::Succeeded, None),
                Err(StorageError::SuccessRequiresReceipt)
            ));
            let receipt = IntegrationReceipt::new(
                IntegrationReceiptId::new("receipt-1").unwrap(),
                integration_id.clone(),
                "target-r1",
                "target-r2",
                "verified by provider",
            )
            .unwrap();
            repository
                .finish_integration(&integration_id, IntegrationState::Succeeded, Some(&receipt))
                .unwrap();
        }
        let repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(
            repository
                .connection
                .query_row(
                    "SELECT state FROM integration_attempts WHERE integration_id = ?1",
                    [integration_id.as_str()],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "succeeded"
        );
        assert_eq!(
            repository
                .connection
                .query_row("SELECT COUNT(*) FROM integration_receipts", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn starts_an_integration_once_across_independent_connections() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        let candidate = CandidateId::new("candidate-1").unwrap();
        let integration = IntegrationId::new("integration-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, revision.clone(), &artifact)
                .unwrap();
            repository
                .create_candidate(
                    candidate.clone(),
                    artifact.base().clone(),
                    vec![CandidateInput::new(change.clone(), revision)],
                )
                .unwrap();
            let attempt = IntegrationAttempt::new(
                integration.clone(),
                RepositoryId::new("repo-1").unwrap(),
                candidate,
                "main",
                "target-r1",
                "native-git",
                "merge",
                OperationId::new("operation-1").unwrap(),
                "agent-1",
            )
            .unwrap();
            repository.plan_integration(&attempt).unwrap();
            repository
                .acquire_lease(&change, "integrate", "agent-1", 100, 200)
                .unwrap();
        }

        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let database = database.clone();
            let store = store.clone();
            let integration = integration.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                let mut repository = SqliteRepository::open(database, store).unwrap();
                barrier.wait();
                repository.start_integration(&integration, "target-r1", 100)
            }));
        }
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(StorageError::InvalidIntegrationTransition)))
                .count(),
            1
        );
        let repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(
            repository
                .connection
                .query_row(
                    "SELECT state FROM integration_attempts WHERE integration_id = ?1",
                    [integration.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "running"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_exact_integration_conflict_evidence() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        let candidate = CandidateId::new("candidate-1").unwrap();
        let integration = IntegrationId::new("integration-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, revision.clone(), &artifact)
                .unwrap();
            repository
                .create_candidate(
                    candidate.clone(),
                    artifact.base().clone(),
                    vec![CandidateInput::new(change.clone(), revision)],
                )
                .unwrap();
            let attempt = IntegrationAttempt::new(
                integration.clone(),
                RepositoryId::new("repo-1").unwrap(),
                candidate.clone(),
                "main",
                "target-r1",
                "native-git",
                "merge",
                OperationId::new("operation-1").unwrap(),
                "agent-1",
            )
            .unwrap();
            repository.plan_integration(&attempt).unwrap();
            repository
                .acquire_lease(&change, "integrate", "agent-1", 100, 200)
                .unwrap();
            repository
                .start_integration(&integration, "target-r1", 100)
                .unwrap();
            let conflict = IntegrationConflict::new(
                ConflictId::new("conflict-1").unwrap(),
                integration.clone(),
                candidate.clone(),
                "merge conflict",
                "merge",
                Some("resolver-1".to_owned()),
                None,
                Some("tests pending".to_owned()),
            )
            .unwrap();
            repository.record_integration_conflict(&conflict).unwrap();
            assert!(matches!(
                repository.record_integration_conflict(&conflict),
                Err(StorageError::InvalidIntegrationTransition)
            ));
        }
        let repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(
            repository
                .connection
                .query_row(
                    "SELECT state FROM integration_attempts WHERE integration_id = ?1",
                    [integration.as_str()],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "conflicted"
        );
        assert_eq!(
            repository
                .connection
                .query_row("SELECT COUNT(*) FROM integration_conflicts", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_reconciliation_evidence_for_an_exact_attempt() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        let candidate = CandidateId::new("candidate-1").unwrap();
        let integration = IntegrationId::new("integration-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, revision.clone(), &artifact)
                .unwrap();
            repository
                .create_candidate(
                    candidate.clone(),
                    artifact.base().clone(),
                    vec![CandidateInput::new(change, revision)],
                )
                .unwrap();
            repository
                .plan_integration(
                    &IntegrationAttempt::new(
                        integration.clone(),
                        RepositoryId::new("repo-1").unwrap(),
                        candidate,
                        "main",
                        "target-r1",
                        "native-git",
                        "merge",
                        OperationId::new("operation-1").unwrap(),
                        "agent-1",
                    )
                    .unwrap(),
                )
                .unwrap();
            repository
                .record_reconciliation(
                    &ReconciliationRecord::new(
                        ReconciliationId::new("reconciliation-1").unwrap(),
                        integration.clone(),
                        "provider response uncertain",
                        "target requires inspection",
                        false,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(
            repository
                .connection
                .query_row(
                    "SELECT observed_state FROM reconciliation_records WHERE integration_id = ?1",
                    [integration.as_str()],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "provider response uncertain"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_non_dependency_change_relations() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let first = ChangeId::new("first").unwrap();
        let second = ChangeId::new("second").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(first.clone()).unwrap();
            repository.create_change(second.clone()).unwrap();
            repository
                .add_change_relation(&first, &second, ChangeRelationKind::TaskDecomposition)
                .unwrap();
            repository
                .add_change_relation(&first, &second, ChangeRelationKind::RelatedTo)
                .unwrap();
            assert!(matches!(
                repository.add_change_relation(&first, &second, ChangeRelationKind::RelatedTo),
                Err(StorageError::DuplicateChangeRelation)
            ));
            assert!(matches!(
                repository.add_change_relation(&first, &first, ChangeRelationKind::RelatedTo),
                Err(StorageError::Invariant(_))
            ));
        }
        let repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(
            repository
                .connection
                .query_row("SELECT COUNT(*) FROM change_relations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_exact_revision_overlap_signal() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let mut repository = SqliteRepository::open(root.join("weft.sqlite"), store).unwrap();
        let left = ChangeId::new("left").unwrap();
        let right = ChangeId::new("right").unwrap();
        let left_revision = RevisionId::new("left-r1").unwrap();
        let right_revision = RevisionId::new("right-r1").unwrap();
        repository.create_change(left.clone()).unwrap();
        repository.create_change(right.clone()).unwrap();
        repository
            .append_revision(&left, None, left_revision.clone(), &artifact)
            .unwrap();
        repository
            .append_revision(&right, None, right_revision.clone(), &artifact)
            .unwrap();
        let overlap = Overlap::new(
            OverlapId::new("overlap-1").unwrap(),
            left_revision,
            right_revision,
            "src/lib.rs",
        )
        .unwrap();
        repository.record_overlap(&overlap).unwrap();
        assert!(matches!(
            repository.record_overlap(&overlap),
            Err(StorageError::DuplicateOverlap(_))
        ));
        assert_eq!(
            repository
                .connection
                .query_row("SELECT detail FROM overlaps", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "src/lib.rs"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn versions_ordered_stacks_without_mutating_history() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
        let first = ChangeId::new("first").unwrap();
        let second = ChangeId::new("second").unwrap();
        repository.create_change(first.clone()).unwrap();
        repository.create_change(second.clone()).unwrap();
        let stack = StackId::new("stack-1").unwrap();
        let original = repository
            .create_stack(stack.clone(), vec![first.clone(), second.clone()])
            .unwrap();
        let revised = repository
            .revise_stack(stack.clone(), 1, vec![second.clone(), first.clone()])
            .unwrap();
        assert_eq!(original.version(), 1);
        assert_eq!(revised.version(), 2);
        assert_eq!(repository.load_stack(&stack, 1).unwrap(), original);
        assert!(matches!(
            repository.revise_stack(stack.clone(), 1, vec![first.clone()]),
            Err(StorageError::StaleStackVersion { .. })
        ));
        drop(repository);
        let mut repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(repository.load_stack(&stack, 2).unwrap(), revised);
        assert!(matches!(
            repository.create_stack(
                StackId::new("stack-duplicate").unwrap(),
                vec![first.clone(), first]
            ),
            Err(StorageError::DuplicateStackEntry)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_complete_domain_event_evidence() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let mut event = DomainEvent::new(
            "integration-started",
            "agent-1",
            100,
            "planned",
            "running",
            "integration-1,candidate-1",
            Some("operation-1".to_owned()),
            Some("provider inspected target-r1".to_owned()),
        )
        .unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.record_domain_event(&mut event).unwrap();
            assert!(event.event_id() > 0);
        }
        let repository = SqliteRepository::open(&database, store).unwrap();
        let row: (String, String, i64, String, String, String, Option<String>, Option<String>) = repository.connection.query_row(
            "SELECT kind, actor, occurred_at_unix_ms, expected_state, resulting_state, affected_ids, operation_id, provider_evidence FROM domain_events WHERE event_id = ?1", [event.event_id()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
        ).unwrap();
        assert_eq!(
            row,
            (
                "integration-started".to_owned(),
                "agent-1".to_owned(),
                100,
                "planned".to_owned(),
                "running".to_owned(),
                "integration-1,candidate-1".to_owned(),
                Some("operation-1".to_owned()),
                Some("provider inspected target-r1".to_owned())
            )
        );
        fs::remove_dir_all(root).unwrap();
    }
}
