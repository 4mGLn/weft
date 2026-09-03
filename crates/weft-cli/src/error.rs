use std::fmt::Display;

use weft_domain::{
    CompositionError, CoordinationError, IntegrationError, MaterializationError, RelationshipError,
};
use weft_provider_git::GitProviderError;
use weft_provider_gitbutler::GitButlerProviderError;
use weft_storage_sqlite::StoreError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ErrorKind {
    Local,
    Usage,
    NotFound,
    Conflict,
    Unsupported,
    Provider,
    Integrity,
}

impl From<GitProviderError> for CliError {
    fn from(error: GitProviderError) -> Self {
        let kind = match &error {
            GitProviderError::Unsupported { .. } => ErrorKind::Unsupported,
            GitProviderError::ChangedTarget { .. } | GitProviderError::Conflict { .. } => {
                ErrorKind::Conflict
            }
            GitProviderError::VerificationFailed(_) | GitProviderError::Artifact(_) => {
                ErrorKind::Integrity
            }
            _ => ErrorKind::Provider,
        };
        Self::new(kind, error.to_string(), false)
    }
}

impl From<GitButlerProviderError> for CliError {
    fn from(error: GitButlerProviderError) -> Self {
        let kind = match &error {
            GitButlerProviderError::Unsupported { .. } => ErrorKind::Unsupported,
            GitButlerProviderError::ChangedTarget { .. }
            | GitButlerProviderError::StaleProviderState(_) => ErrorKind::Conflict,
            GitButlerProviderError::VerificationFailed(_) | GitButlerProviderError::Artifact(_) => {
                ErrorKind::Integrity
            }
            _ => ErrorKind::Provider,
        };
        Self::new(kind, error.to_string(), false)
    }
}

#[derive(Debug)]
pub(crate) struct CliError {
    kind: ErrorKind,
    message: String,
    retryable: bool,
}

impl CliError {
    pub(crate) fn local(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Local, message.into(), false)
    }

    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Usage, message.into(), false)
    }

    pub(crate) fn integrity(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Integrity, message.into(), false)
    }

    pub(crate) fn from_input(error: impl Display) -> Self {
        Self::usage(error.to_string())
    }

    const fn new(kind: ErrorKind, message: String, retryable: bool) -> Self {
        Self {
            kind,
            message,
            retryable,
        }
    }

    pub(crate) const fn code(&self) -> &'static str {
        match self.kind {
            ErrorKind::Local => "local",
            ErrorKind::Usage => "usage",
            ErrorKind::NotFound => "not_found",
            ErrorKind::Conflict => "conflict",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::Provider => "provider",
            ErrorKind::Integrity => "integrity",
        }
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) const fn retryable(&self) -> bool {
        self.retryable
    }

    pub(crate) const fn exit_code(&self) -> i32 {
        match self.kind {
            ErrorKind::Local => 1,
            ErrorKind::Usage => 2,
            ErrorKind::NotFound => 3,
            ErrorKind::Conflict => 4,
            ErrorKind::Unsupported => 5,
            ErrorKind::Provider => 6,
            ErrorKind::Integrity => 7,
        }
    }
}

impl From<StoreError> for CliError {
    #[allow(clippy::too_many_lines)]
    fn from(error: StoreError) -> Self {
        let kind = match &error {
            StoreError::ChangeNotFound(_)
            | StoreError::RevisionNotFoundForChange { .. }
            | StoreError::AssignmentNotFound(_)
            | StoreError::LeaseNotFound(_)
            | StoreError::MaterializationNotFound(_)
            | StoreError::RelationshipNotFound(_)
            | StoreError::DependencyNotFound(_)
            | StoreError::StackNotFound(_)
            | StoreError::CandidateNotFound(_)
            | StoreError::ReviewRequestNotFound(_)
            | StoreError::ReviewSubmissionNotFound(_)
            | StoreError::ValidationResultNotFound(_)
            | StoreError::IntegrationNotFound(_) => ErrorKind::NotFound,
            StoreError::ChangeAlreadyExists(_)
            | StoreError::DuplicateRevision(_)
            | StoreError::DuplicateMaterialization(_)
            | StoreError::DuplicateRelationship(_)
            | StoreError::ActiveRelationshipExists
            | StoreError::DuplicateDependency(_)
            | StoreError::ActiveDependencyExists
            | StoreError::DependencyCycle
            | StoreError::DuplicateStack(_)
            | StoreError::StaleStackVersion { .. }
            | StoreError::DuplicateCandidate(_)
            | StoreError::DuplicateReviewRequest(_)
            | StoreError::DuplicateReviewSubmission(_)
            | StoreError::DuplicateValidationResult(_)
            | StoreError::DuplicateIntegration(_)
            | StoreError::IntegrationGateRejected(_)
            | StoreError::IntegrationTargetHeld
            | StoreError::DuplicateLease(_)
            | StoreError::LeaseNotCurrent(_)
            | StoreError::LeaseHeld { .. }
            | StoreError::StaleCoordinationVersion { .. }
            | StoreError::Coordination(
                CoordinationError::StaleVersion { .. }
                | CoordinationError::AssignmentAlreadyReleased
                | CoordinationError::LeaseExpired
                | CoordinationError::LeaseReleased
                | CoordinationError::LeaseRenewalDoesNotExtend,
            )
            | StoreError::Relationship(
                RelationshipError::StaleVersion { .. }
                | RelationshipError::AlreadyRemoved
                | RelationshipError::UnchangedPins,
            )
            | StoreError::Composition(
                CompositionError::StaleStackVersion { .. }
                | CompositionError::UnchangedStackDefinition,
            )
            | StoreError::Materialization(
                MaterializationError::StaleVersion { .. }
                | MaterializationError::TerminalState(_)
                | MaterializationError::NoChange,
            )
            | StoreError::Integration(
                IntegrationError::StaleVersion { .. }
                | IntegrationError::InvalidTransition(_)
                | IntegrationError::StaleTarget { .. }
                | IntegrationError::LeaseAuthorityMismatch
                | IntegrationError::ExecutionLeaseExpired
                | IntegrationError::LeaseMustExtend
                | IntegrationError::NoEffectNotVerified
                | IntegrationError::ResultNotReconciled
                | IntegrationError::DivergenceNotVerified,
            )
            | StoreError::OperationIdConflict(_)
            | StoreError::StaleHead { .. }
            | StoreError::ExactTargetMismatch
            | StoreError::EvidenceBeforeTarget => ErrorKind::Conflict,
            StoreError::UnsupportedJournalMode(_) | StoreError::UnsupportedSchemaVersion(_) => {
                ErrorKind::Unsupported
            }
            StoreError::Artifact(_)
            | StoreError::ArtifactBaseMismatch
            | StoreError::InvalidStoredData(_)
            | StoreError::InvariantViolation(_)
            | StoreError::Domain(_)
            | StoreError::Coordination(_)
            | StoreError::Materialization(_)
            | StoreError::Relationship(_)
            | StoreError::Composition(_)
            | StoreError::Review(_)
            | StoreError::Integration(_) => ErrorKind::Integrity,
            StoreError::Database(_)
            | StoreError::InvalidOperationId
            | StoreError::ChangeHasNoHead(_)
            | StoreError::CandidateRepositoryMismatch(_)
            | StoreError::CandidateMissingUpstream { .. }
            | StoreError::CandidateDependencyOrder(_)
            | StoreError::StaleCandidateDependency(_)
            | StoreError::CollectionTooLarge => ErrorKind::Local,
        };
        let message = match kind {
            ErrorKind::Integrity => "stored state failed integrity verification".to_owned(),
            ErrorKind::Local => "local metadata operation failed".to_owned(),
            _ => error.to_string(),
        };
        let retryable = matches!(
            error,
            StoreError::LeaseHeld { .. }
                | StoreError::StaleCoordinationVersion { .. }
                | StoreError::Coordination(
                    CoordinationError::StaleVersion { .. } | CoordinationError::LeaseExpired,
                )
                | StoreError::Relationship(RelationshipError::StaleVersion { .. })
                | StoreError::Composition(CompositionError::StaleStackVersion { .. })
                | StoreError::Materialization(MaterializationError::StaleVersion { .. })
                | StoreError::Integration(
                    IntegrationError::StaleVersion { .. }
                        | IntegrationError::StaleTarget { .. }
                        | IntegrationError::ExecutionLeaseExpired,
                )
                | StoreError::StaleStackVersion { .. }
                | StoreError::StaleHead { .. }
                | StoreError::IntegrationTargetHeld
        );
        Self::new(kind, message, retryable)
    }
}
