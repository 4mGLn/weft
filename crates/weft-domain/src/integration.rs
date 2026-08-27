use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::artifact::is_sha256_digest;
use crate::{
    ActorId, CandidateId, CandidateInput, ExactTarget, ProviderId, ReviewSubmissionId, Subject,
    UnixMillis, ValidationResultId,
};

macro_rules! integration_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates a non-empty integration identifier or evidence value.
            ///
            /// # Errors
            ///
            /// Returns [`IntegrationError::EmptyField`] for an empty value.
            pub fn new(value: impl Into<String>) -> Result<Self, IntegrationError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(IntegrationError::EmptyField(stringify!($name)));
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

integration_id!(IntegrationId);
integration_id!(IntegrationConflictId);
integration_id!(ConflictResolutionId);
integration_id!(ReconciliationId);
integration_id!(IntegrationReceiptId);
integration_id!(EffectOperationId);
integration_id!(ExecutionLeaseId);
integration_id!(TargetRef);
integration_id!(TargetRevision);
integration_id!(IntegrationStrategy);
integration_id!(GatePolicyEvidence);
integration_id!(IntegrationCapabilityEvidence);
integration_id!(IntegrationEvidence);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct IntegrationVersion(i64);

impl IntegrationVersion {
    pub const EMPTY: Self = Self(0);
    pub const INITIAL: Self = Self(1);

    /// Creates a non-negative attempt version.
    ///
    /// # Errors
    ///
    /// Rejects negative versions.
    pub const fn new(value: i64) -> Result<Self, IntegrationError> {
        if value < 0 {
            return Err(IntegrationError::InvalidVersion);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> i64 {
        self.0
    }

    fn next(self) -> Result<Self, IntegrationError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(IntegrationError::VersionExhausted)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationBinding {
    candidate_id: CandidateId,
    candidate_digest: String,
    ordered_inputs: Vec<CandidateInput>,
}

impl IntegrationBinding {
    /// Copies one immutable candidate's correctness identity.
    ///
    /// # Errors
    ///
    /// Requires a canonical digest and non-empty duplicate-free ordered inputs.
    pub fn new(
        candidate_id: CandidateId,
        candidate_digest: impl Into<String>,
        ordered_inputs: Vec<CandidateInput>,
    ) -> Result<Self, IntegrationError> {
        let candidate_digest = candidate_digest.into();
        if !is_sha256_digest(&candidate_digest) {
            return Err(IntegrationError::InvalidDigest);
        }
        if ordered_inputs.is_empty() {
            return Err(IntegrationError::NoInputs);
        }
        for (position, input) in ordered_inputs.iter().enumerate() {
            if ordered_inputs[..position]
                .iter()
                .any(|prior| prior.change_id() == input.change_id())
            {
                return Err(IntegrationError::DuplicateInput);
            }
        }
        Ok(Self {
            candidate_id,
            candidate_digest,
            ordered_inputs,
        })
    }

    #[must_use]
    pub const fn candidate_id(&self) -> &CandidateId {
        &self.candidate_id
    }

    #[must_use]
    pub fn candidate_digest(&self) -> &str {
        &self.candidate_digest
    }

    #[must_use]
    pub fn ordered_inputs(&self) -> &[CandidateInput] {
        &self.ordered_inputs
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationTarget {
    repository_id: crate::RepositoryId,
    target_ref: TargetRef,
    expected_revision: TargetRevision,
}

impl IntegrationTarget {
    #[must_use]
    pub const fn new(
        repository_id: crate::RepositoryId,
        target_ref: TargetRef,
        expected_revision: TargetRevision,
    ) -> Self {
        Self {
            repository_id,
            target_ref,
            expected_revision,
        }
    }

    #[must_use]
    pub const fn repository_id(&self) -> &crate::RepositoryId {
        &self.repository_id
    }

    #[must_use]
    pub const fn target_ref(&self) -> &TargetRef {
        &self.target_ref
    }

    #[must_use]
    pub const fn expected_revision(&self) -> &TargetRevision {
        &self.expected_revision
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationMethod {
    provider_id: ProviderId,
    strategy: IntegrationStrategy,
    effect_operation_id: EffectOperationId,
}

impl IntegrationMethod {
    #[must_use]
    pub const fn new(
        provider_id: ProviderId,
        strategy: IntegrationStrategy,
        effect_operation_id: EffectOperationId,
    ) -> Self {
        Self {
            provider_id,
            strategy,
            effect_operation_id,
        }
    }

    #[must_use]
    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    #[must_use]
    pub const fn strategy(&self) -> &IntegrationStrategy {
        &self.strategy
    }

    #[must_use]
    pub const fn effect_operation_id(&self) -> &EffectOperationId {
        &self.effect_operation_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationIntent {
    binding: IntegrationBinding,
    target: IntegrationTarget,
    method: IntegrationMethod,
}

impl IntegrationIntent {
    #[must_use]
    pub const fn new(
        binding: IntegrationBinding,
        target: IntegrationTarget,
        method: IntegrationMethod,
    ) -> Self {
        Self {
            binding,
            target,
            method,
        }
    }

    #[must_use]
    pub const fn binding(&self) -> &IntegrationBinding {
        &self.binding
    }

    #[must_use]
    pub const fn target(&self) -> &IntegrationTarget {
        &self.target
    }

    #[must_use]
    pub const fn method(&self) -> &IntegrationMethod {
        &self.method
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetObservation {
    target_ref: TargetRef,
    observed_revision: TargetRevision,
    evidence: IntegrationEvidence,
}

impl TargetObservation {
    #[must_use]
    pub const fn new(
        target_ref: TargetRef,
        observed_revision: TargetRevision,
        evidence: IntegrationEvidence,
    ) -> Self {
        Self {
            target_ref,
            observed_revision,
            evidence,
        }
    }

    #[must_use]
    pub const fn target_ref(&self) -> &TargetRef {
        &self.target_ref
    }

    #[must_use]
    pub const fn observed_revision(&self) -> &TargetRevision {
        &self.observed_revision
    }

    #[must_use]
    pub const fn evidence(&self) -> &IntegrationEvidence {
        &self.evidence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationGate {
    policy_evidence: GatePolicyEvidence,
    capability_evidence: IntegrationCapabilityEvidence,
    review_refs: Vec<ReviewSubmissionId>,
    validation_refs: Vec<ValidationResultId>,
    target_observation: TargetObservation,
}

impl IntegrationGate {
    #[must_use]
    pub fn new(
        policy_evidence: GatePolicyEvidence,
        capability_evidence: IntegrationCapabilityEvidence,
        mut review_refs: Vec<ReviewSubmissionId>,
        mut validation_refs: Vec<ValidationResultId>,
        target_observation: TargetObservation,
    ) -> Self {
        review_refs.sort_unstable();
        review_refs.dedup();
        validation_refs.sort_unstable();
        validation_refs.dedup();
        Self {
            policy_evidence,
            capability_evidence,
            review_refs,
            validation_refs,
            target_observation,
        }
    }

    #[must_use]
    pub const fn policy_evidence(&self) -> &GatePolicyEvidence {
        &self.policy_evidence
    }

    #[must_use]
    pub const fn capability_evidence(&self) -> &IntegrationCapabilityEvidence {
        &self.capability_evidence
    }

    #[must_use]
    pub fn review_refs(&self) -> &[ReviewSubmissionId] {
        &self.review_refs
    }

    #[must_use]
    pub fn validation_refs(&self) -> &[ValidationResultId] {
        &self.validation_refs
    }

    #[must_use]
    pub const fn target_observation(&self) -> &TargetObservation {
        &self.target_observation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrationState {
    Planned,
    Running,
    Reconciling,
    Conflicted,
    Failed,
    Succeeded,
    Aborted,
    Superseded,
}

impl IntegrationState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Running => "running",
            Self::Reconciling => "reconciling",
            Self::Conflicted => "conflicted",
            Self::Failed => "failed",
            Self::Succeeded => "succeeded",
            Self::Aborted => "aborted",
            Self::Superseded => "superseded",
        }
    }

    /// Parses a stable storage/API state.
    ///
    /// # Errors
    ///
    /// Rejects unknown states.
    pub fn parse(value: &str) -> Result<Self, IntegrationError> {
        match value {
            "planned" => Ok(Self::Planned),
            "running" => Ok(Self::Running),
            "reconciling" => Ok(Self::Reconciling),
            "conflicted" => Ok(Self::Conflicted),
            "failed" => Ok(Self::Failed),
            "succeeded" => Ok(Self::Succeeded),
            "aborted" => Ok(Self::Aborted),
            "superseded" => Ok(Self::Superseded),
            _ => Err(IntegrationError::InvalidState(value.to_owned())),
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Conflicted | Self::Failed | Self::Succeeded | Self::Aborted | Self::Superseded
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionLease {
    id: ExecutionLeaseId,
    holder: Subject,
    acquired_at: UnixMillis,
    expires_at: UnixMillis,
    version: IntegrationVersion,
}

impl ExecutionLease {
    /// Creates target-scoped execution authority.
    ///
    /// # Errors
    ///
    /// Expiry must be strictly later than acquisition.
    pub fn new(
        id: ExecutionLeaseId,
        holder: Subject,
        acquired_at: UnixMillis,
        expires_at: UnixMillis,
    ) -> Result<Self, IntegrationError> {
        if expires_at <= acquired_at {
            return Err(IntegrationError::InvalidLeaseExpiry);
        }
        Ok(Self {
            id,
            holder,
            acquired_at,
            expires_at,
            version: IntegrationVersion::INITIAL,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &ExecutionLeaseId {
        &self.id
    }

    #[must_use]
    pub const fn holder(&self) -> &Subject {
        &self.holder
    }

    #[must_use]
    pub const fn acquired_at(&self) -> UnixMillis {
        self.acquired_at
    }

    #[must_use]
    pub const fn expires_at(&self) -> UnixMillis {
        self.expires_at
    }

    #[must_use]
    pub const fn version(&self) -> IntegrationVersion {
        self.version
    }

    fn authorize(
        &self,
        lease_id: &ExecutionLeaseId,
        holder: &Subject,
        at: UnixMillis,
    ) -> Result<(), IntegrationError> {
        if &self.id != lease_id || &self.holder != holder {
            return Err(IntegrationError::LeaseAuthorityMismatch);
        }
        if at >= self.expires_at {
            return Err(IntegrationError::ExecutionLeaseExpired);
        }
        Ok(())
    }

    fn renew(
        &mut self,
        lease_id: &ExecutionLeaseId,
        holder: &Subject,
        at: UnixMillis,
        expires_at: UnixMillis,
    ) -> Result<(), IntegrationError> {
        self.authorize(lease_id, holder, at)?;
        if expires_at <= self.expires_at || expires_at <= at {
            return Err(IntegrationError::LeaseMustExtend);
        }
        self.version = self.version.next()?;
        self.expires_at = expires_at;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationObservation {
    id: ReconciliationId,
    attempt_id: IntegrationId,
    outcome: ReconciliationOutcome,
    target: TargetObservation,
    actor: ActorId,
    observed_at: UnixMillis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationOutcome {
    StillUncertain,
    NoEffectVerified,
    ResultVerified,
    Diverged,
}

impl ReconciliationOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StillUncertain => "still_uncertain",
            Self::NoEffectVerified => "no_effect_verified",
            Self::ResultVerified => "result_verified",
            Self::Diverged => "diverged",
        }
    }

    /// Parses a stable storage/API outcome.
    ///
    /// # Errors
    ///
    /// Rejects unknown outcomes.
    pub fn parse(value: &str) -> Result<Self, IntegrationError> {
        match value {
            "still_uncertain" => Ok(Self::StillUncertain),
            "no_effect_verified" => Ok(Self::NoEffectVerified),
            "result_verified" => Ok(Self::ResultVerified),
            "diverged" => Ok(Self::Diverged),
            _ => Err(IntegrationError::InvalidReconciliationOutcome(
                value.to_owned(),
            )),
        }
    }
}

impl ReconciliationObservation {
    #[must_use]
    pub const fn id(&self) -> &ReconciliationId {
        &self.id
    }

    #[must_use]
    pub const fn attempt_id(&self) -> &IntegrationId {
        &self.attempt_id
    }

    #[must_use]
    pub const fn outcome(&self) -> ReconciliationOutcome {
        self.outcome
    }

    #[must_use]
    pub const fn target(&self) -> &TargetObservation {
        &self.target
    }

    #[must_use]
    pub const fn actor(&self) -> &ActorId {
        &self.actor
    }

    #[must_use]
    pub const fn observed_at(&self) -> UnixMillis {
        self.observed_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationAttempt {
    id: IntegrationId,
    intent: IntegrationIntent,
    gate: IntegrationGate,
    state: IntegrationState,
    version: IntegrationVersion,
    created_at: UnixMillis,
    created_by: ActorId,
    updated_at: UnixMillis,
    updated_by: ActorId,
    started_at: Option<UnixMillis>,
    finished_at: Option<UnixMillis>,
    result_revision: Option<TargetRevision>,
    lease: Option<ExecutionLease>,
    latest_reconciliation: Option<ReconciliationObservation>,
}

impl IntegrationAttempt {
    /// Plans one exact immutable integration intent.
    ///
    /// # Errors
    ///
    /// The planning target observation must equal the expected target.
    pub fn plan(
        id: IntegrationId,
        intent: IntegrationIntent,
        gate: IntegrationGate,
        created_at: UnixMillis,
        created_by: ActorId,
    ) -> Result<Self, IntegrationError> {
        ensure_expected_observation(&intent, gate.target_observation())?;
        Ok(Self {
            id,
            intent,
            gate,
            state: IntegrationState::Planned,
            version: IntegrationVersion::INITIAL,
            created_at,
            created_by: created_by.clone(),
            updated_at: created_at,
            updated_by: created_by,
            started_at: None,
            finished_at: None,
            result_revision: None,
            lease: None,
            latest_reconciliation: None,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &IntegrationId {
        &self.id
    }

    #[must_use]
    pub const fn intent(&self) -> &IntegrationIntent {
        &self.intent
    }

    #[must_use]
    pub const fn gate(&self) -> &IntegrationGate {
        &self.gate
    }

    #[must_use]
    pub const fn state(&self) -> IntegrationState {
        self.state
    }

    #[must_use]
    pub const fn version(&self) -> IntegrationVersion {
        self.version
    }

    #[must_use]
    pub const fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    #[must_use]
    pub const fn created_by(&self) -> &ActorId {
        &self.created_by
    }

    #[must_use]
    pub const fn updated_at(&self) -> UnixMillis {
        self.updated_at
    }

    #[must_use]
    pub const fn updated_by(&self) -> &ActorId {
        &self.updated_by
    }

    #[must_use]
    pub const fn started_at(&self) -> Option<UnixMillis> {
        self.started_at
    }

    #[must_use]
    pub const fn finished_at(&self) -> Option<UnixMillis> {
        self.finished_at
    }

    #[must_use]
    pub const fn result_revision(&self) -> Option<&TargetRevision> {
        self.result_revision.as_ref()
    }

    #[must_use]
    pub const fn lease(&self) -> Option<&ExecutionLease> {
        self.lease.as_ref()
    }

    #[must_use]
    pub const fn latest_reconciliation(&self) -> Option<&ReconciliationObservation> {
        self.latest_reconciliation.as_ref()
    }

    /// Starts execution with target-scoped authority and target CAS evidence.
    ///
    /// # Errors
    ///
    /// Rejects stale version/state, changed target, invalid lease, and time reversal.
    pub fn start(
        &mut self,
        expected_version: IntegrationVersion,
        lease: ExecutionLease,
        observation: &TargetObservation,
        at: UnixMillis,
        actor: ActorId,
    ) -> Result<(), IntegrationError> {
        self.ensure_transition(expected_version, &[IntegrationState::Planned], at)?;
        ensure_expected_observation(&self.intent, observation)?;
        if lease.acquired_at() != at {
            return Err(IntegrationError::LeaseAcquisitionTimeMismatch);
        }
        self.advance(IntegrationState::Running, at, actor)?;
        self.started_at = Some(at);
        self.lease = Some(lease);
        Ok(())
    }

    /// Renews current execution authority without changing intent or state.
    ///
    /// # Errors
    ///
    /// Only the current unexpired holder may extend the exact lease.
    pub fn renew_lease(
        &mut self,
        expected_version: IntegrationVersion,
        lease_id: &ExecutionLeaseId,
        holder: &Subject,
        at: UnixMillis,
        expires_at: UnixMillis,
        actor: ActorId,
    ) -> Result<(), IntegrationError> {
        self.ensure_transition(expected_version, &[IntegrationState::Running], at)?;
        self.lease
            .as_mut()
            .ok_or(IntegrationError::ExecutionLeaseMissing)?
            .renew(lease_id, holder, at, expires_at)?;
        self.advance_same_state(at, actor)
    }

    /// Records uncertainty and enters reconciliation.
    ///
    /// # Errors
    ///
    /// Before lease expiry only the current holder may report uncertainty.
    pub fn enter_reconciliation(
        &mut self,
        expected_version: IntegrationVersion,
        id: ReconciliationId,
        authority: Option<(&ExecutionLeaseId, &Subject)>,
        target: TargetObservation,
        at: UnixMillis,
        actor: ActorId,
    ) -> Result<ReconciliationObservation, IntegrationError> {
        self.ensure_transition(expected_version, &[IntegrationState::Running], at)?;
        self.authorize_or_expired(authority, at)?;
        let observation = ReconciliationObservation {
            id,
            attempt_id: self.id.clone(),
            outcome: ReconciliationOutcome::StillUncertain,
            target,
            actor: actor.clone(),
            observed_at: at,
        };
        self.advance(IntegrationState::Reconciling, at, actor)?;
        self.latest_reconciliation = Some(observation.clone());
        Ok(observation)
    }

    /// Appends a provider reconciliation observation.
    ///
    /// # Errors
    ///
    /// `NoEffect` must observe the expected target; all observations retain intent.
    pub fn reconcile(
        &mut self,
        expected_version: IntegrationVersion,
        id: ReconciliationId,
        outcome: ReconciliationOutcome,
        target: TargetObservation,
        at: UnixMillis,
        actor: ActorId,
    ) -> Result<ReconciliationObservation, IntegrationError> {
        self.ensure_transition(expected_version, &[IntegrationState::Reconciling], at)?;
        ensure_target_ref(&self.intent, &target)?;
        if outcome == ReconciliationOutcome::NoEffectVerified
            && target.observed_revision() != self.intent.target().expected_revision()
        {
            return Err(IntegrationError::NoEffectTargetMismatch);
        }
        let observation = ReconciliationObservation {
            id,
            attempt_id: self.id.clone(),
            outcome,
            target,
            actor: actor.clone(),
            observed_at: at,
        };
        self.advance_same_state(at, actor)?;
        self.latest_reconciliation = Some(observation.clone());
        Ok(observation)
    }

    /// Aborts a plan before execution begins.
    ///
    /// # Errors
    ///
    /// Running or uncertain attempts cannot be aborted without reconciliation.
    pub fn abort_planned(
        &mut self,
        expected_version: IntegrationVersion,
        at: UnixMillis,
        actor: ActorId,
    ) -> Result<(), IntegrationError> {
        self.ensure_transition(expected_version, &[IntegrationState::Planned], at)?;
        self.finish(IntegrationState::Aborted, at, actor, None)
    }

    /// Finishes a reconciled attempt only after no effect was verified.
    ///
    /// # Errors
    ///
    /// Requires the latest exact reconciliation outcome to be `NoEffectVerified`.
    pub fn finish_no_effect(
        &mut self,
        expected_version: IntegrationVersion,
        state: IntegrationState,
        at: UnixMillis,
        actor: ActorId,
    ) -> Result<(), IntegrationError> {
        self.ensure_transition(expected_version, &[IntegrationState::Reconciling], at)?;
        if !matches!(state, IntegrationState::Failed | IntegrationState::Aborted) {
            return Err(IntegrationError::InvalidNoEffectTerminalState);
        }
        if self
            .latest_reconciliation
            .as_ref()
            .map(ReconciliationObservation::outcome)
            != Some(ReconciliationOutcome::NoEffectVerified)
        {
            return Err(IntegrationError::NoEffectNotVerified);
        }
        self.finish(state, at, actor, None)
    }

    /// Closes a diverged attempt so a new exact-target attempt can be planned.
    ///
    /// # Errors
    ///
    /// Requires the latest exact reconciliation outcome to be `Diverged`.
    pub fn supersede_diverged(
        &mut self,
        expected_version: IntegrationVersion,
        at: UnixMillis,
        actor: ActorId,
    ) -> Result<(), IntegrationError> {
        self.ensure_transition(expected_version, &[IntegrationState::Reconciling], at)?;
        if self
            .latest_reconciliation
            .as_ref()
            .map(ReconciliationObservation::outcome)
            != Some(ReconciliationOutcome::Diverged)
        {
            return Err(IntegrationError::DivergenceNotVerified);
        }
        self.finish(IntegrationState::Superseded, at, actor, None)
    }

    /// Records a conflict and terminally preserves the attempt.
    ///
    /// # Errors
    ///
    /// Running callers need current authority; Reconciling callers need exact evidence.
    pub fn conflict(
        &mut self,
        expected_version: IntegrationVersion,
        conflict_id: IntegrationConflictId,
        authority: Option<(&ExecutionLeaseId, &Subject)>,
        provider_state: IntegrationEvidence,
        at: UnixMillis,
        actor: ActorId,
    ) -> Result<IntegrationConflict, IntegrationError> {
        self.ensure_transition(
            expected_version,
            &[IntegrationState::Running, IntegrationState::Reconciling],
            at,
        )?;
        if self.state == IntegrationState::Running {
            self.authorize_current(authority, at)?;
        }
        let conflict = IntegrationConflict {
            id: conflict_id,
            attempt_id: self.id.clone(),
            candidate_id: self.intent.binding().candidate_id().clone(),
            candidate_digest: self.intent.binding().candidate_digest().to_owned(),
            ordered_inputs: self.intent.binding().ordered_inputs().to_vec(),
            provider_id: self.intent.method().provider_id().clone(),
            provider_state,
            created_at: at,
            created_by: actor.clone(),
        };
        self.finish(IntegrationState::Conflicted, at, actor, None)?;
        Ok(conflict)
    }

    /// Completes verified success and creates the only valid receipt shape.
    ///
    /// # Errors
    ///
    /// Running success needs live authority; reconciled success needs `ResultVerified`.
    pub fn succeed(
        &mut self,
        expected_version: IntegrationVersion,
        receipt_id: IntegrationReceiptId,
        authority: Option<(&ExecutionLeaseId, &Subject)>,
        target: &TargetObservation,
        at: UnixMillis,
        actor: ActorId,
    ) -> Result<IntegrationReceipt, IntegrationError> {
        self.ensure_transition(
            expected_version,
            &[IntegrationState::Running, IntegrationState::Reconciling],
            at,
        )?;
        ensure_target_ref(&self.intent, target)?;
        if self.state == IntegrationState::Running {
            self.authorize_current(authority, at)?;
        } else if self
            .latest_reconciliation
            .as_ref()
            .is_none_or(|reconciliation| {
                reconciliation.outcome() != ReconciliationOutcome::ResultVerified
                    || reconciliation.target() != target
            })
        {
            return Err(IntegrationError::ResultNotReconciled);
        }
        let result = target.observed_revision().clone();
        let receipt = IntegrationReceipt {
            id: receipt_id,
            attempt_id: self.id.clone(),
            candidate_id: self.intent.binding().candidate_id().clone(),
            candidate_digest: self.intent.binding().candidate_digest().to_owned(),
            repository_id: self.intent.target().repository_id().clone(),
            target_ref: self.intent.target().target_ref().clone(),
            prior_revision: self.intent.target().expected_revision().clone(),
            result_revision: result.clone(),
            provider_id: self.intent.method().provider_id().clone(),
            effect_operation_id: self.intent.method().effect_operation_id().clone(),
            verification_evidence: target.evidence().clone(),
            verified_at: at,
            verified_by: actor.clone(),
        };
        self.finish(IntegrationState::Succeeded, at, actor, Some(result))?;
        Ok(receipt)
    }

    fn ensure_transition(
        &self,
        expected_version: IntegrationVersion,
        allowed: &[IntegrationState],
        at: UnixMillis,
    ) -> Result<(), IntegrationError> {
        if self.version != expected_version {
            return Err(IntegrationError::StaleVersion {
                expected: expected_version,
                actual: self.version,
            });
        }
        if !allowed.contains(&self.state) {
            return Err(IntegrationError::InvalidTransition(self.state));
        }
        if at < self.updated_at {
            return Err(IntegrationError::TimestampBeforePriorEvent);
        }
        Ok(())
    }

    fn authorize_current(
        &self,
        authority: Option<(&ExecutionLeaseId, &Subject)>,
        at: UnixMillis,
    ) -> Result<(), IntegrationError> {
        let (lease_id, holder) = authority.ok_or(IntegrationError::LeaseAuthorityMismatch)?;
        self.lease
            .as_ref()
            .ok_or(IntegrationError::ExecutionLeaseMissing)?
            .authorize(lease_id, holder, at)
    }

    fn authorize_or_expired(
        &self,
        authority: Option<(&ExecutionLeaseId, &Subject)>,
        at: UnixMillis,
    ) -> Result<(), IntegrationError> {
        let lease = self
            .lease
            .as_ref()
            .ok_or(IntegrationError::ExecutionLeaseMissing)?;
        if at >= lease.expires_at() {
            return Ok(());
        }
        self.authorize_current(authority, at)
    }

    fn advance(
        &mut self,
        state: IntegrationState,
        at: UnixMillis,
        actor: ActorId,
    ) -> Result<(), IntegrationError> {
        self.version = self.version.next()?;
        self.state = state;
        self.updated_at = at;
        self.updated_by = actor;
        Ok(())
    }

    fn advance_same_state(
        &mut self,
        at: UnixMillis,
        actor: ActorId,
    ) -> Result<(), IntegrationError> {
        self.advance(self.state, at, actor)
    }

    fn finish(
        &mut self,
        state: IntegrationState,
        at: UnixMillis,
        actor: ActorId,
        result: Option<TargetRevision>,
    ) -> Result<(), IntegrationError> {
        self.advance(state, at, actor)?;
        self.finished_at = Some(at);
        self.result_revision = result;
        Ok(())
    }
}

fn ensure_target_ref(
    intent: &IntegrationIntent,
    observation: &TargetObservation,
) -> Result<(), IntegrationError> {
    if observation.target_ref() != intent.target().target_ref() {
        return Err(IntegrationError::TargetRefMismatch);
    }
    Ok(())
}

fn ensure_expected_observation(
    intent: &IntegrationIntent,
    observation: &TargetObservation,
) -> Result<(), IntegrationError> {
    ensure_target_ref(intent, observation)?;
    if observation.observed_revision() != intent.target().expected_revision() {
        return Err(IntegrationError::StaleTarget {
            expected: intent.target().expected_revision().clone(),
            actual: observation.observed_revision().clone(),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationConflict {
    id: IntegrationConflictId,
    attempt_id: IntegrationId,
    candidate_id: CandidateId,
    candidate_digest: String,
    ordered_inputs: Vec<CandidateInput>,
    provider_id: ProviderId,
    provider_state: IntegrationEvidence,
    created_at: UnixMillis,
    created_by: ActorId,
}

impl IntegrationConflict {
    #[must_use]
    pub const fn id(&self) -> &IntegrationConflictId {
        &self.id
    }

    #[must_use]
    pub const fn attempt_id(&self) -> &IntegrationId {
        &self.attempt_id
    }

    #[must_use]
    pub const fn candidate_id(&self) -> &CandidateId {
        &self.candidate_id
    }

    #[must_use]
    pub fn candidate_digest(&self) -> &str {
        &self.candidate_digest
    }

    #[must_use]
    pub fn ordered_inputs(&self) -> &[CandidateInput] {
        &self.ordered_inputs
    }

    #[must_use]
    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    #[must_use]
    pub const fn provider_state(&self) -> &IntegrationEvidence {
        &self.provider_state
    }

    #[must_use]
    pub const fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    #[must_use]
    pub const fn created_by(&self) -> &ActorId {
        &self.created_by
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictResolution {
    id: ConflictResolutionId,
    conflict_id: IntegrationConflictId,
    resulting_target: ExactTarget,
    validation_refs: Vec<ValidationResultId>,
    provider_evidence: IntegrationEvidence,
    resolved_at: UnixMillis,
    resolved_by: ActorId,
}

impl ConflictResolution {
    /// Creates a separate immutable resolution record.
    ///
    /// # Errors
    ///
    /// Resolution must follow the conflict and cite at least one validation.
    pub fn new(
        id: ConflictResolutionId,
        conflict: &IntegrationConflict,
        resulting_target: ExactTarget,
        mut validation_refs: Vec<ValidationResultId>,
        provider_evidence: IntegrationEvidence,
        resolved_at: UnixMillis,
        resolved_by: ActorId,
    ) -> Result<Self, IntegrationError> {
        if resolved_at < conflict.created_at() {
            return Err(IntegrationError::TimestampBeforePriorEvent);
        }
        validation_refs.sort_unstable();
        validation_refs.dedup();
        if validation_refs.is_empty() {
            return Err(IntegrationError::ResolutionNeedsValidation);
        }
        Ok(Self {
            id,
            conflict_id: conflict.id().clone(),
            resulting_target,
            validation_refs,
            provider_evidence,
            resolved_at,
            resolved_by,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &ConflictResolutionId {
        &self.id
    }

    #[must_use]
    pub const fn conflict_id(&self) -> &IntegrationConflictId {
        &self.conflict_id
    }

    #[must_use]
    pub const fn resulting_target(&self) -> &ExactTarget {
        &self.resulting_target
    }

    #[must_use]
    pub fn validation_refs(&self) -> &[ValidationResultId] {
        &self.validation_refs
    }

    #[must_use]
    pub const fn provider_evidence(&self) -> &IntegrationEvidence {
        &self.provider_evidence
    }

    #[must_use]
    pub const fn resolved_at(&self) -> UnixMillis {
        self.resolved_at
    }

    #[must_use]
    pub const fn resolved_by(&self) -> &ActorId {
        &self.resolved_by
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationReceipt {
    id: IntegrationReceiptId,
    attempt_id: IntegrationId,
    candidate_id: CandidateId,
    candidate_digest: String,
    repository_id: crate::RepositoryId,
    target_ref: TargetRef,
    prior_revision: TargetRevision,
    result_revision: TargetRevision,
    provider_id: ProviderId,
    effect_operation_id: EffectOperationId,
    verification_evidence: IntegrationEvidence,
    verified_at: UnixMillis,
    verified_by: ActorId,
}

impl IntegrationReceipt {
    #[must_use]
    pub const fn id(&self) -> &IntegrationReceiptId {
        &self.id
    }

    #[must_use]
    pub const fn attempt_id(&self) -> &IntegrationId {
        &self.attempt_id
    }

    #[must_use]
    pub const fn candidate_id(&self) -> &CandidateId {
        &self.candidate_id
    }

    #[must_use]
    pub fn candidate_digest(&self) -> &str {
        &self.candidate_digest
    }

    #[must_use]
    pub const fn repository_id(&self) -> &crate::RepositoryId {
        &self.repository_id
    }

    #[must_use]
    pub const fn target_ref(&self) -> &TargetRef {
        &self.target_ref
    }

    #[must_use]
    pub const fn prior_revision(&self) -> &TargetRevision {
        &self.prior_revision
    }

    #[must_use]
    pub const fn result_revision(&self) -> &TargetRevision {
        &self.result_revision
    }

    #[must_use]
    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    #[must_use]
    pub const fn effect_operation_id(&self) -> &EffectOperationId {
        &self.effect_operation_id
    }

    #[must_use]
    pub const fn verification_evidence(&self) -> &IntegrationEvidence {
        &self.verification_evidence
    }

    #[must_use]
    pub const fn verified_at(&self) -> UnixMillis {
        self.verified_at
    }

    #[must_use]
    pub const fn verified_by(&self) -> &ActorId {
        &self.verified_by
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IntegrationError {
    EmptyField(&'static str),
    InvalidDigest,
    NoInputs,
    DuplicateInput,
    InvalidVersion,
    VersionExhausted,
    InvalidState(String),
    InvalidReconciliationOutcome(String),
    StaleVersion {
        expected: IntegrationVersion,
        actual: IntegrationVersion,
    },
    InvalidTransition(IntegrationState),
    TimestampBeforePriorEvent,
    TargetRefMismatch,
    StaleTarget {
        expected: TargetRevision,
        actual: TargetRevision,
    },
    InvalidLeaseExpiry,
    LeaseAcquisitionTimeMismatch,
    ExecutionLeaseMissing,
    LeaseAuthorityMismatch,
    ExecutionLeaseExpired,
    LeaseMustExtend,
    NoEffectTargetMismatch,
    InvalidNoEffectTerminalState,
    NoEffectNotVerified,
    ResultNotReconciled,
    DivergenceNotVerified,
    ResolutionNeedsValidation,
}

impl Display for IntegrationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "{field} cannot be empty"),
            Self::InvalidDigest => formatter.write_str("invalid candidate SHA-256 digest"),
            Self::NoInputs => formatter.write_str("integration requires candidate inputs"),
            Self::DuplicateInput => {
                formatter.write_str("integration inputs contain a duplicate Change")
            }
            Self::InvalidVersion => formatter.write_str("integration version cannot be negative"),
            Self::VersionExhausted => formatter.write_str("integration version is exhausted"),
            Self::InvalidState(state) => write!(formatter, "invalid integration state: {state}"),
            Self::InvalidReconciliationOutcome(outcome) => {
                write!(formatter, "invalid reconciliation outcome: {outcome}")
            }
            Self::StaleVersion { expected, actual } => write!(
                formatter,
                "stale integration version: expected {}, actual {}",
                expected.value(),
                actual.value()
            ),
            Self::InvalidTransition(state) => write!(
                formatter,
                "invalid transition from integration state {}",
                state.as_str()
            ),
            Self::TimestampBeforePriorEvent => {
                formatter.write_str("integration event precedes prior history")
            }
            Self::TargetRefMismatch => {
                formatter.write_str("provider observation targets a different ref")
            }
            Self::StaleTarget { expected, actual } => write!(
                formatter,
                "stale integration target: expected {}, actual {}",
                expected.as_str(),
                actual.as_str()
            ),
            Self::InvalidLeaseExpiry => {
                formatter.write_str("execution lease expiry must follow acquisition")
            }
            Self::LeaseAcquisitionTimeMismatch => {
                formatter.write_str("execution lease acquisition must match start event")
            }
            Self::ExecutionLeaseMissing => {
                formatter.write_str("integration has no execution lease")
            }
            Self::LeaseAuthorityMismatch => {
                formatter.write_str("caller does not hold execution authority")
            }
            Self::ExecutionLeaseExpired => formatter.write_str("execution lease is expired"),
            Self::LeaseMustExtend => {
                formatter.write_str("lease renewal must extend current expiry")
            }
            Self::NoEffectTargetMismatch => {
                formatter.write_str("no-effect verification must observe the expected target")
            }
            Self::InvalidNoEffectTerminalState => {
                formatter.write_str("no-effect completion must be Failed or Aborted")
            }
            Self::NoEffectNotVerified => {
                formatter.write_str("no-effect completion requires matching reconciliation")
            }
            Self::ResultNotReconciled => {
                formatter.write_str("success result does not match verified reconciliation")
            }
            Self::DivergenceNotVerified => {
                formatter.write_str("supersession requires a diverged reconciliation")
            }
            Self::ResolutionNeedsValidation => {
                formatter.write_str("conflict resolution requires validation evidence")
            }
        }
    }
}

impl Error for IntegrationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChangeId, RepositoryId, RevisionId, SubjectId, SubjectKind};

    fn at(value: i64) -> UnixMillis {
        UnixMillis::new(value).unwrap()
    }
    fn actor() -> ActorId {
        ActorId::new("operator-1").unwrap()
    }
    fn holder() -> Subject {
        Subject::new(SubjectKind::Agent, SubjectId::new("agent-1").unwrap())
    }
    fn observation(revision: &str) -> TargetObservation {
        TargetObservation::new(
            TargetRef::new("refs/heads/main").unwrap(),
            TargetRevision::new(revision).unwrap(),
            IntegrationEvidence::new(format!("observed:{revision}")).unwrap(),
        )
    }
    fn intent() -> IntegrationIntent {
        IntegrationIntent::new(
            IntegrationBinding::new(
                CandidateId::new("candidate-1").unwrap(),
                format!("sha256:{}", "a".repeat(64)),
                vec![CandidateInput::new(
                    ChangeId::new("change-1").unwrap(),
                    RevisionId::new("revision-1").unwrap(),
                )],
            )
            .unwrap(),
            IntegrationTarget::new(
                RepositoryId::new("repo-1").unwrap(),
                TargetRef::new("refs/heads/main").unwrap(),
                TargetRevision::new("base-1").unwrap(),
            ),
            IntegrationMethod::new(
                ProviderId::new("native-git").unwrap(),
                IntegrationStrategy::new("merge").unwrap(),
                EffectOperationId::new("effect-1").unwrap(),
            ),
        )
    }
    fn gate() -> IntegrationGate {
        IntegrationGate::new(
            GatePolicyEvidence::new("policy:allowed").unwrap(),
            IntegrationCapabilityEvidence::new("capability:merge").unwrap(),
            Vec::new(),
            Vec::new(),
            observation("base-1"),
        )
    }
    fn attempt() -> IntegrationAttempt {
        IntegrationAttempt::plan(
            IntegrationId::new("integration-1").unwrap(),
            intent(),
            gate(),
            at(1),
            actor(),
        )
        .unwrap()
    }
    fn lease(expires: i64) -> ExecutionLease {
        ExecutionLease::new(
            ExecutionLeaseId::new("lease-1").unwrap(),
            holder(),
            at(2),
            at(expires),
        )
        .unwrap()
    }

    #[test]
    fn plan_and_start_require_exact_target_and_live_authority() {
        assert!(matches!(
            IntegrationAttempt::plan(
                IntegrationId::new("bad").unwrap(),
                intent(),
                IntegrationGate::new(
                    GatePolicyEvidence::new("policy").unwrap(),
                    IntegrationCapabilityEvidence::new("capability").unwrap(),
                    Vec::new(),
                    Vec::new(),
                    observation("other")
                ),
                at(1),
                actor()
            ),
            Err(IntegrationError::StaleTarget { .. })
        ));
        let mut value = attempt();
        value
            .start(
                IntegrationVersion::INITIAL,
                lease(5),
                &observation("base-1"),
                at(2),
                actor(),
            )
            .unwrap();
        assert_eq!(value.state(), IntegrationState::Running);
        assert!(matches!(
            value.succeed(
                value.version(),
                IntegrationReceiptId::new("receipt").unwrap(),
                Some((&ExecutionLeaseId::new("wrong").unwrap(), &holder())),
                &observation("result-1"),
                at(3),
                actor()
            ),
            Err(IntegrationError::LeaseAuthorityMismatch)
        ));
    }

    #[test]
    fn expired_execution_enters_reconciliation_and_never_implies_failure() {
        let mut value = attempt();
        value
            .start(
                value.version(),
                lease(3),
                &observation("base-1"),
                at(2),
                actor(),
            )
            .unwrap();
        let uncertain = value
            .enter_reconciliation(
                value.version(),
                ReconciliationId::new("reconcile-1").unwrap(),
                None,
                observation("unknown"),
                at(3),
                actor(),
            )
            .unwrap();
        assert_eq!(uncertain.outcome(), ReconciliationOutcome::StillUncertain);
        assert_eq!(value.state(), IntegrationState::Reconciling);
        assert!(matches!(
            value.finish_no_effect(value.version(), IntegrationState::Failed, at(4), actor()),
            Err(IntegrationError::NoEffectNotVerified)
        ));
    }

    #[test]
    fn reconciled_success_requires_matching_verified_result_and_emits_receipt() {
        let mut value = attempt();
        value
            .start(
                value.version(),
                lease(3),
                &observation("base-1"),
                at(2),
                actor(),
            )
            .unwrap();
        value
            .enter_reconciliation(
                value.version(),
                ReconciliationId::new("reconcile-1").unwrap(),
                None,
                observation("unknown"),
                at(3),
                actor(),
            )
            .unwrap();
        value
            .reconcile(
                value.version(),
                ReconciliationId::new("reconcile-2").unwrap(),
                ReconciliationOutcome::ResultVerified,
                observation("result-1"),
                at(4),
                actor(),
            )
            .unwrap();
        let receipt = value
            .succeed(
                value.version(),
                IntegrationReceiptId::new("receipt-1").unwrap(),
                None,
                &observation("result-1"),
                at(5),
                actor(),
            )
            .unwrap();
        assert_eq!(value.state(), IntegrationState::Succeeded);
        assert_eq!(receipt.result_revision().as_str(), "result-1");
        assert_eq!(receipt.effect_operation_id().as_str(), "effect-1");
    }

    #[test]
    fn conflict_is_terminal_and_resolution_is_separate_validated_history() {
        let mut value = attempt();
        value
            .start(
                value.version(),
                lease(5),
                &observation("base-1"),
                at(2),
                actor(),
            )
            .unwrap();
        let conflict = value
            .conflict(
                value.version(),
                IntegrationConflictId::new("conflict-1").unwrap(),
                Some((&ExecutionLeaseId::new("lease-1").unwrap(), &holder())),
                IntegrationEvidence::new("unmerged:path").unwrap(),
                at(3),
                actor(),
            )
            .unwrap();
        assert_eq!(value.state(), IntegrationState::Conflicted);
        let target = ExactTarget::candidate(
            CandidateId::new("candidate-2").unwrap(),
            crate::BaseState::new(RepositoryId::new("repo-1").unwrap(), "base-1").unwrap(),
            format!("sha256:{}", "b".repeat(64)),
        )
        .unwrap();
        assert!(matches!(
            ConflictResolution::new(
                ConflictResolutionId::new("resolution-1").unwrap(),
                &conflict,
                target.clone(),
                Vec::new(),
                IntegrationEvidence::new("resolved").unwrap(),
                at(4),
                actor()
            ),
            Err(IntegrationError::ResolutionNeedsValidation)
        ));
        ConflictResolution::new(
            ConflictResolutionId::new("resolution-1").unwrap(),
            &conflict,
            target,
            vec![ValidationResultId::new("validation-1").unwrap()],
            IntegrationEvidence::new("resolved").unwrap(),
            at(4),
            actor(),
        )
        .unwrap();
    }

    #[test]
    fn no_effect_verification_allows_terminal_failure_without_receipt() {
        let mut value = attempt();
        value
            .start(
                value.version(),
                lease(3),
                &observation("base-1"),
                at(2),
                actor(),
            )
            .unwrap();
        value
            .enter_reconciliation(
                value.version(),
                ReconciliationId::new("reconcile-1").unwrap(),
                None,
                observation("unknown"),
                at(3),
                actor(),
            )
            .unwrap();
        value
            .reconcile(
                value.version(),
                ReconciliationId::new("reconcile-2").unwrap(),
                ReconciliationOutcome::NoEffectVerified,
                observation("base-1"),
                at(4),
                actor(),
            )
            .unwrap();
        value
            .finish_no_effect(value.version(), IntegrationState::Failed, at(5), actor())
            .unwrap();
        assert_eq!(value.state(), IntegrationState::Failed);
        assert_eq!(value.result_revision(), None);
    }

    #[test]
    fn diverged_reconciliation_requires_explicit_receipt_free_supersession() {
        let mut value = attempt();
        value
            .start(
                value.version(),
                lease(3),
                &observation("base-1"),
                at(2),
                actor(),
            )
            .unwrap();
        value
            .enter_reconciliation(
                value.version(),
                ReconciliationId::new("reconcile-1").unwrap(),
                None,
                observation("unknown"),
                at(3),
                actor(),
            )
            .unwrap();
        assert!(matches!(
            value.supersede_diverged(value.version(), at(4), actor()),
            Err(IntegrationError::DivergenceNotVerified)
        ));
        value
            .reconcile(
                value.version(),
                ReconciliationId::new("reconcile-2").unwrap(),
                ReconciliationOutcome::Diverged,
                observation("external-target"),
                at(4),
                actor(),
            )
            .unwrap();
        value
            .supersede_diverged(value.version(), at(5), actor())
            .unwrap();
        assert_eq!(value.state(), IntegrationState::Superseded);
        assert_eq!(value.result_revision(), None);
    }
}
