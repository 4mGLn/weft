use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::artifact::is_sha256_digest;
use crate::{ActorId, BaseState, CandidateId, ChangeId, RevisionId, UnixMillis};

macro_rules! review_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates a non-empty review or validation identifier.
            ///
            /// # Errors
            ///
            /// Returns [`ReviewError::EmptyField`] for an empty value.
            pub fn new(value: impl Into<String>) -> Result<Self, ReviewError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(ReviewError::EmptyField(stringify!($name)));
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

review_id!(ReviewRequestId);
review_id!(ReviewSubmissionId);
review_id!(ValidationResultId);
review_id!(ValidationType);
review_id!(ValidationEnvironment);
review_id!(ValidationExecutionId);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExactTarget {
    Revision {
        change_id: ChangeId,
        revision_id: RevisionId,
        base: BaseState,
        artifact_digest: String,
    },
    Candidate {
        candidate_id: CandidateId,
        target_base: BaseState,
        content_digest: String,
    },
}

impl ExactTarget {
    /// Snapshots one exact Change revision target.
    ///
    /// # Errors
    ///
    /// Rejects a malformed canonical artifact digest.
    pub fn revision(
        change_id: ChangeId,
        revision_id: RevisionId,
        base: BaseState,
        artifact_digest: impl Into<String>,
    ) -> Result<Self, ReviewError> {
        let artifact_digest = artifact_digest.into();
        ensure_digest(&artifact_digest)?;
        Ok(Self::Revision {
            change_id,
            revision_id,
            base,
            artifact_digest,
        })
    }

    /// Snapshots one exact immutable `CompositionCandidate` target.
    ///
    /// # Errors
    ///
    /// Rejects a malformed candidate content digest.
    pub fn candidate(
        candidate_id: CandidateId,
        target_base: BaseState,
        content_digest: impl Into<String>,
    ) -> Result<Self, ReviewError> {
        let content_digest = content_digest.into();
        ensure_digest(&content_digest)?;
        Ok(Self::Candidate {
            candidate_id,
            target_base,
            content_digest,
        })
    }

    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Revision { .. } => "revision",
            Self::Candidate { .. } => "candidate",
        }
    }

    #[must_use]
    pub const fn change_id(&self) -> Option<&ChangeId> {
        match self {
            Self::Revision { change_id, .. } => Some(change_id),
            Self::Candidate { .. } => None,
        }
    }

    #[must_use]
    pub const fn revision_id(&self) -> Option<&RevisionId> {
        match self {
            Self::Revision { revision_id, .. } => Some(revision_id),
            Self::Candidate { .. } => None,
        }
    }

    #[must_use]
    pub const fn candidate_id(&self) -> Option<&CandidateId> {
        match self {
            Self::Candidate { candidate_id, .. } => Some(candidate_id),
            Self::Revision { .. } => None,
        }
    }

    #[must_use]
    pub const fn context(&self) -> &BaseState {
        match self {
            Self::Revision { base, .. } => base,
            Self::Candidate { target_base, .. } => target_base,
        }
    }

    #[must_use]
    pub fn content_digest(&self) -> &str {
        match self {
            Self::Revision {
                artifact_digest, ..
            } => artifact_digest,
            Self::Candidate { content_digest, .. } => content_digest,
        }
    }
}

fn ensure_digest(value: &str) -> Result<(), ReviewError> {
    if is_sha256_digest(value) {
        Ok(())
    } else {
        Err(ReviewError::InvalidDigest)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewReusePolicy {
    NewSubmissionRequired,
}

impl ReviewReusePolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NewSubmissionRequired => "new_submission_required",
        }
    }

    /// Parses a stable storage/API value.
    ///
    /// # Errors
    ///
    /// Rejects policies unsupported by v1.
    pub fn parse(value: &str) -> Result<Self, ReviewError> {
        match value {
            "new_submission_required" => Ok(Self::NewSubmissionRequired),
            _ => Err(ReviewError::InvalidReviewReusePolicy(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRequest {
    id: ReviewRequestId,
    target: ExactTarget,
    requested_by: ActorId,
    reviewers: Vec<ActorId>,
    reuse_policy: ReviewReusePolicy,
    created_at: UnixMillis,
}

impl ReviewRequest {
    /// Creates an immutable exact-target review request.
    ///
    /// # Errors
    ///
    /// Requires at least one unique reviewer.
    pub fn new(
        id: ReviewRequestId,
        target: ExactTarget,
        requested_by: ActorId,
        mut reviewers: Vec<ActorId>,
        created_at: UnixMillis,
    ) -> Result<Self, ReviewError> {
        if reviewers.is_empty() {
            return Err(ReviewError::NoReviewers);
        }
        let mut seen = HashSet::with_capacity(reviewers.len());
        for reviewer in &reviewers {
            if !seen.insert(reviewer) {
                return Err(ReviewError::DuplicateReviewer(reviewer.clone()));
            }
        }
        reviewers.sort_unstable();
        Ok(Self {
            id,
            target,
            requested_by,
            reviewers,
            reuse_policy: ReviewReusePolicy::NewSubmissionRequired,
            created_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &ReviewRequestId {
        &self.id
    }

    #[must_use]
    pub const fn target(&self) -> &ExactTarget {
        &self.target
    }

    #[must_use]
    pub const fn requested_by(&self) -> &ActorId {
        &self.requested_by
    }

    #[must_use]
    pub fn reviewers(&self) -> &[ActorId] {
        &self.reviewers
    }

    #[must_use]
    pub const fn reuse_policy(&self) -> ReviewReusePolicy {
        self.reuse_policy
    }

    #[must_use]
    pub const fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    #[must_use]
    pub fn includes_reviewer(&self, reviewer: &ActorId) -> bool {
        self.reviewers.contains(reviewer)
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
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes_requested",
            Self::Rejected => "rejected",
            Self::Blocked => "blocked",
        }
    }

    /// Parses a stable storage/API value.
    ///
    /// # Errors
    ///
    /// Rejects unknown outcomes.
    pub fn parse(value: &str) -> Result<Self, ReviewError> {
        match value {
            "approved" => Ok(Self::Approved),
            "changes_requested" => Ok(Self::ChangesRequested),
            "rejected" => Ok(Self::Rejected),
            "blocked" => Ok(Self::Blocked),
            _ => Err(ReviewError::InvalidReviewOutcome(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewSubmission {
    id: ReviewSubmissionId,
    request_id: ReviewRequestId,
    target: ExactTarget,
    reviewer: ActorId,
    outcome: ReviewOutcome,
    comments: Option<String>,
    submitted_at: UnixMillis,
}

impl ReviewSubmission {
    /// Appends one requested reviewer's immutable submission.
    ///
    /// # Errors
    ///
    /// Rejects unrequested reviewers, empty present comments, and time reversal.
    pub fn new(
        id: ReviewSubmissionId,
        request: &ReviewRequest,
        reviewer: ActorId,
        outcome: ReviewOutcome,
        comments: Option<String>,
        submitted_at: UnixMillis,
    ) -> Result<Self, ReviewError> {
        if !request.includes_reviewer(&reviewer) {
            return Err(ReviewError::ReviewerNotRequested(reviewer));
        }
        if comments
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ReviewError::EmptyField("comments"));
        }
        if submitted_at < request.created_at() {
            return Err(ReviewError::TimestampBeforeTarget);
        }
        Ok(Self {
            id,
            request_id: request.id().clone(),
            target: request.target().clone(),
            reviewer,
            outcome,
            comments,
            submitted_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &ReviewSubmissionId {
        &self.id
    }

    #[must_use]
    pub const fn request_id(&self) -> &ReviewRequestId {
        &self.request_id
    }

    #[must_use]
    pub const fn target(&self) -> &ExactTarget {
        &self.target
    }

    #[must_use]
    pub const fn reviewer(&self) -> &ActorId {
        &self.reviewer
    }

    #[must_use]
    pub const fn outcome(&self) -> ReviewOutcome {
        self.outcome
    }

    #[must_use]
    pub fn comments(&self) -> Option<&str> {
        self.comments.as_deref()
    }

    #[must_use]
    pub const fn submitted_at(&self) -> UnixMillis {
        self.submitted_at
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationOutcome {
    Passed,
    Failed,
    Blocked,
    Error,
}

impl ValidationOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Error => "error",
        }
    }

    /// Parses a stable storage/API value.
    ///
    /// # Errors
    ///
    /// Rejects unknown outcomes.
    pub fn parse(value: &str) -> Result<Self, ReviewError> {
        match value {
            "passed" => Ok(Self::Passed),
            "failed" => Ok(Self::Failed),
            "blocked" => Ok(Self::Blocked),
            "error" => Ok(Self::Error),
            _ => Err(ReviewError::InvalidValidationOutcome(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationScope {
    ExactTarget,
    DeclaredReusable { scope: String, rationale: String },
}

impl ValidationScope {
    /// Records a validator-declared reusable scope without applying it.
    ///
    /// # Errors
    ///
    /// Requires non-empty scope and rationale values.
    pub fn declared_reusable(
        scope: impl Into<String>,
        rationale: impl Into<String>,
    ) -> Result<Self, ReviewError> {
        let scope = scope.into();
        let rationale = rationale.into();
        if scope.trim().is_empty() {
            return Err(ReviewError::EmptyField("validation scope"));
        }
        if rationale.trim().is_empty() {
            return Err(ReviewError::EmptyField("validation scope rationale"));
        }
        Ok(Self::DeclaredReusable { scope, rationale })
    }

    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ExactTarget => "exact_target",
            Self::DeclaredReusable { .. } => "declared_reusable",
        }
    }

    #[must_use]
    pub fn declaration(&self) -> Option<(&str, &str)> {
        match self {
            Self::ExactTarget => None,
            Self::DeclaredReusable { scope, rationale } => Some((scope, rationale)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationObservation {
    validation_type: ValidationType,
    environment: ValidationEnvironment,
    outcome: ValidationOutcome,
    execution_id: ValidationExecutionId,
    scope: ValidationScope,
}

impl ValidationObservation {
    #[must_use]
    pub const fn new(
        validation_type: ValidationType,
        environment: ValidationEnvironment,
        outcome: ValidationOutcome,
        execution_id: ValidationExecutionId,
        scope: ValidationScope,
    ) -> Self {
        Self {
            validation_type,
            environment,
            outcome,
            execution_id,
            scope,
        }
    }

    #[must_use]
    pub const fn validation_type(&self) -> &ValidationType {
        &self.validation_type
    }

    #[must_use]
    pub const fn environment(&self) -> &ValidationEnvironment {
        &self.environment
    }

    #[must_use]
    pub const fn outcome(&self) -> ValidationOutcome {
        self.outcome
    }

    #[must_use]
    pub const fn execution_id(&self) -> &ValidationExecutionId {
        &self.execution_id
    }

    #[must_use]
    pub const fn scope(&self) -> &ValidationScope {
        &self.scope
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationResult {
    id: ValidationResultId,
    target: ExactTarget,
    observation: ValidationObservation,
    validated_by: ActorId,
    validated_at: UnixMillis,
}

impl ValidationResult {
    #[must_use]
    pub const fn new(
        id: ValidationResultId,
        target: ExactTarget,
        observation: ValidationObservation,
        validated_by: ActorId,
        validated_at: UnixMillis,
    ) -> Self {
        Self {
            id,
            target,
            observation,
            validated_by,
            validated_at,
        }
    }

    #[must_use]
    pub const fn id(&self) -> &ValidationResultId {
        &self.id
    }

    #[must_use]
    pub const fn target(&self) -> &ExactTarget {
        &self.target
    }

    #[must_use]
    pub const fn validation_type(&self) -> &ValidationType {
        self.observation.validation_type()
    }

    #[must_use]
    pub const fn environment(&self) -> &ValidationEnvironment {
        self.observation.environment()
    }

    #[must_use]
    pub const fn outcome(&self) -> ValidationOutcome {
        self.observation.outcome()
    }

    #[must_use]
    pub const fn execution_id(&self) -> &ValidationExecutionId {
        self.observation.execution_id()
    }

    #[must_use]
    pub const fn scope(&self) -> &ValidationScope {
        self.observation.scope()
    }

    #[must_use]
    pub const fn validated_by(&self) -> &ActorId {
        &self.validated_by
    }

    #[must_use]
    pub const fn validated_at(&self) -> UnixMillis {
        self.validated_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewError {
    EmptyField(&'static str),
    InvalidDigest,
    NoReviewers,
    DuplicateReviewer(ActorId),
    ReviewerNotRequested(ActorId),
    TimestampBeforeTarget,
    InvalidReviewReusePolicy(String),
    InvalidReviewOutcome(String),
    InvalidValidationOutcome(String),
}

impl Display for ReviewError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "{field} cannot be empty"),
            Self::InvalidDigest => formatter.write_str("invalid exact-target SHA-256 digest"),
            Self::NoReviewers => formatter.write_str("a review request requires a reviewer"),
            Self::DuplicateReviewer(reviewer) => {
                write!(formatter, "duplicate reviewer: {}", reviewer.as_str())
            }
            Self::ReviewerNotRequested(reviewer) => {
                write!(
                    formatter,
                    "reviewer was not requested: {}",
                    reviewer.as_str()
                )
            }
            Self::TimestampBeforeTarget => {
                formatter.write_str("review submission precedes its request")
            }
            Self::InvalidReviewReusePolicy(value) => {
                write!(formatter, "invalid review reuse policy: {value}")
            }
            Self::InvalidReviewOutcome(value) => {
                write!(formatter, "invalid review outcome: {value}")
            }
            Self::InvalidValidationOutcome(value) => {
                write!(formatter, "invalid validation outcome: {value}")
            }
        }
    }
}

impl Error for ReviewError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RepositoryId;

    fn actor(value: &str) -> ActorId {
        ActorId::new(value).unwrap()
    }

    fn at(value: i64) -> UnixMillis {
        UnixMillis::new(value).unwrap()
    }

    fn target() -> ExactTarget {
        ExactTarget::revision(
            ChangeId::new("change-1").unwrap(),
            RevisionId::new("revision-1").unwrap(),
            BaseState::new(RepositoryId::new("repo-1").unwrap(), "base-1").unwrap(),
            format!("sha256:{}", "a".repeat(64)),
        )
        .unwrap()
    }

    fn request() -> ReviewRequest {
        ReviewRequest::new(
            ReviewRequestId::new("review-1").unwrap(),
            target(),
            actor("requester"),
            vec![actor("reviewer-1"), actor("reviewer-2")],
            at(2),
        )
        .unwrap()
    }

    #[test]
    fn exact_target_requires_canonical_digest_and_one_shape() {
        assert!(matches!(
            ExactTarget::revision(
                ChangeId::new("change-1").unwrap(),
                RevisionId::new("revision-1").unwrap(),
                BaseState::new(RepositoryId::new("repo-1").unwrap(), "base-1").unwrap(),
                "provider-ref"
            ),
            Err(ReviewError::InvalidDigest)
        ));
        assert_eq!(target().kind(), "revision");
    }

    #[test]
    fn review_request_requires_unique_reviewers_and_exact_only_reuse() {
        assert!(matches!(
            ReviewRequest::new(
                ReviewRequestId::new("review-1").unwrap(),
                target(),
                actor("requester"),
                Vec::new(),
                at(1)
            ),
            Err(ReviewError::NoReviewers)
        ));
        assert!(matches!(
            ReviewRequest::new(
                ReviewRequestId::new("review-1").unwrap(),
                target(),
                actor("requester"),
                vec![actor("same"), actor("same")],
                at(1)
            ),
            Err(ReviewError::DuplicateReviewer(_))
        ));
        assert_eq!(
            request().reuse_policy(),
            ReviewReusePolicy::NewSubmissionRequired
        );
        let forward = ReviewRequest::new(
            ReviewRequestId::new("review-order").unwrap(),
            target(),
            actor("requester"),
            vec![actor("a"), actor("b")],
            at(1),
        )
        .unwrap();
        let reversed = ReviewRequest::new(
            ReviewRequestId::new("review-order").unwrap(),
            target(),
            actor("requester"),
            vec![actor("b"), actor("a")],
            at(1),
        )
        .unwrap();
        assert_eq!(forward, reversed);
    }

    #[test]
    fn submission_copies_target_and_requires_requested_reviewer_and_time() {
        let request = request();
        let submission = ReviewSubmission::new(
            ReviewSubmissionId::new("submission-1").unwrap(),
            &request,
            actor("reviewer-1"),
            ReviewOutcome::Approved,
            Some("exact target reviewed".to_owned()),
            at(3),
        )
        .unwrap();
        assert_eq!(submission.target(), request.target());
        assert!(matches!(
            ReviewSubmission::new(
                ReviewSubmissionId::new("submission-2").unwrap(),
                &request,
                actor("outsider"),
                ReviewOutcome::Approved,
                None,
                at(3)
            ),
            Err(ReviewError::ReviewerNotRequested(_))
        ));
        assert!(matches!(
            ReviewSubmission::new(
                ReviewSubmissionId::new("submission-2").unwrap(),
                &request,
                actor("reviewer-1"),
                ReviewOutcome::Approved,
                None,
                at(1)
            ),
            Err(ReviewError::TimestampBeforeTarget)
        ));
    }

    #[test]
    fn validation_scope_records_declaration_without_changing_target() {
        let scope = ValidationScope::declared_reusable(
            "compiler-independent-unit-tests",
            "test inputs exclude platform behavior",
        )
        .unwrap();
        let result = ValidationResult::new(
            ValidationResultId::new("validation-1").unwrap(),
            target(),
            ValidationObservation::new(
                ValidationType::new("test").unwrap(),
                ValidationEnvironment::new("linux-x86_64").unwrap(),
                ValidationOutcome::Passed,
                ValidationExecutionId::new("execution-1").unwrap(),
                scope,
            ),
            actor("validator"),
            at(3),
        );
        assert_eq!(result.target(), &target());
        assert_eq!(
            result.scope().declaration().map(|value| value.0),
            Some("compiler-independent-unit-tests")
        );
    }

    #[test]
    fn stored_enums_reject_unknown_values() {
        assert!(ReviewOutcome::parse("accept").is_err());
        assert!(ValidationOutcome::parse("green").is_err());
        assert!(ReviewReusePolicy::parse("same-change").is_err());
    }
}
