use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use weft_artifact::ArtifactStore;
use weft_domain::{
    ActorId, CandidateId, CandidateInput, ConflictResolution, ConflictResolutionId,
    EffectOperationId, ExecutionLease, ExecutionLeaseId, GatePolicyEvidence, IntegrationAttempt,
    IntegrationBinding, IntegrationCapabilityEvidence, IntegrationConflict, IntegrationConflictId,
    IntegrationEvidence, IntegrationGate, IntegrationId, IntegrationIntent, IntegrationMethod,
    IntegrationReceipt, IntegrationReceiptId, IntegrationState, IntegrationStrategy,
    IntegrationTarget, IntegrationVersion, ProviderId, ReconciliationId, ReconciliationObservation,
    ReconciliationOutcome, RepositoryId, ReviewOutcome, ReviewSubmissionId, Subject, SubjectId,
    SubjectKind, TargetObservation, TargetRef, TargetRevision, UnixMillis, ValidationOutcome,
    ValidationResultId,
};

use super::{
    MutationContext, SqliteStore, StoreError, insert_operation_record, recorded_operation,
};

#[derive(Debug)]
struct PlanRow {
    candidate_id: String,
    candidate_digest: String,
    repository_id: String,
    target_ref: String,
    expected_revision: String,
    provider_id: String,
    strategy: String,
    effect_operation_id: String,
    policy_evidence: String,
    capability_evidence: String,
    observed_revision: String,
    observation_evidence: String,
    created_at: i64,
    created_by: String,
    operation_id: String,
    input_count: i64,
    review_count: i64,
    validation_count: i64,
}

#[derive(Debug)]
struct StoredEvent {
    kind: String,
    expected_version: i64,
    resulting_version: i64,
    resulting_state: String,
    observed_revision: Option<String>,
    observation_evidence: Option<String>,
    lease_id: Option<String>,
    holder_kind: Option<String>,
    holder_id: Option<String>,
    lease_acquired_at: Option<i64>,
    lease_expires_at: Option<i64>,
    lease_version: Option<i64>,
    reconciliation_id: Option<String>,
    reconciliation_outcome: Option<String>,
    conflict_id: Option<String>,
    provider_state: Option<String>,
    receipt_id: Option<String>,
    result_revision: Option<String>,
    operation_id: String,
    actor_id: String,
    occurred_at: i64,
}

#[derive(Clone, Copy, Default)]
struct EventDetails<'a> {
    observation: Option<&'a TargetObservation>,
    reconciliation: Option<&'a ReconciliationObservation>,
    conflict: Option<&'a IntegrationConflict>,
    receipt: Option<&'a IntegrationReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseRenewal {
    pub expected_version: IntegrationVersion,
    pub lease_id: ExecutionLeaseId,
    pub holder: Subject,
    pub expires_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationStart {
    pub expected_version: IntegrationVersion,
    pub reconciliation_id: ReconciliationId,
    pub authority: Option<(ExecutionLeaseId, Subject)>,
    pub observation: TargetObservation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationRecord {
    pub expected_version: IntegrationVersion,
    pub reconciliation_id: ReconciliationId,
    pub outcome: ReconciliationOutcome,
    pub observation: TargetObservation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictReport {
    pub expected_version: IntegrationVersion,
    pub conflict_id: IntegrationConflictId,
    pub authority: Option<(ExecutionLeaseId, Subject)>,
    pub provider_state: IntegrationEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuccessVerification {
    pub expected_version: IntegrationVersion,
    pub receipt_id: IntegrationReceiptId,
    pub authority: Option<(ExecutionLeaseId, Subject)>,
    pub observation: TargetObservation,
}

impl SqliteStore {
    /// Persists immutable integration intent and its initial event.
    ///
    /// # Errors
    ///
    /// The candidate, target base, gate evidence, provenance, and operation replay
    /// must all remain exact and current.
    pub fn create_integration_attempt(
        &mut self,
        artifact_store: &ArtifactStore,
        attempt: &IntegrationAttempt,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        if attempt.state() != IntegrationState::Planned
            || attempt.version() != IntegrationVersion::INITIAL
            || attempt.created_by() != &context.actor
            || attempt.created_at() != context.occurred_at
        {
            return Err(StoreError::InvariantViolation(
                "integration plan provenance or initial state differs",
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(operation) = recorded_operation(&transaction, context.operation_id())? {
            if operation.event_kind != "integration.planned"
                || operation.actor_id != context.actor.as_str()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            if event_id_for_operation(&transaction, context.operation_id())?.as_deref()
                != Some(attempt.id().as_str())
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            let current = load_attempt(&transaction, artifact_store, attempt.id())?;
            let stored = load_attempt_before_event(&transaction, &current, 1)?;
            if stored != *attempt {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok(());
        }
        validate_plan_current(&transaction, artifact_store, attempt)?;
        if attempt_exists(&transaction, attempt.id())? {
            return Err(StoreError::DuplicateIntegration(attempt.id().clone()));
        }
        insert_operation_record(&transaction, "integration.planned", context)?;
        insert_plan(&transaction, attempt, context)?;
        transaction.commit()?;
        Ok(())
    }

    /// Reconstructs an attempt from immutable intent and ordered events.
    ///
    /// # Errors
    ///
    /// Missing source content, event drift, or invalid transitions fail closed.
    pub fn integration_attempt(
        &self,
        artifact_store: &ArtifactStore,
        id: &IntegrationId,
    ) -> Result<IntegrationAttempt, StoreError> {
        load_attempt(&self.connection, artifact_store, id)
    }

    /// Loads one immutable conflict through its authoritative attempt event.
    ///
    /// # Errors
    ///
    /// Missing or drifted attempt, event, conflict, or input history fails closed.
    pub fn integration_conflict(
        &self,
        artifact_store: &ArtifactStore,
        id: &IntegrationConflictId,
    ) -> Result<IntegrationConflict, StoreError> {
        load_conflict(&self.connection, artifact_store, id)
    }

    /// Persists a separate immutable, exactly validated conflict resolution.
    ///
    /// # Errors
    ///
    /// Resolution provenance, target, validations, and operation replay must match.
    pub fn create_conflict_resolution(
        &mut self,
        artifact_store: &ArtifactStore,
        resolution: &ConflictResolution,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        if resolution.resolved_by() != &context.actor
            || resolution.resolved_at() != context.occurred_at
        {
            return Err(StoreError::InvariantViolation(
                "conflict resolution provenance differs",
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(operation) = recorded_operation(&transaction, context.operation_id())? {
            let recorded_id = transaction
                .query_row(
                    "SELECT resolution_id FROM conflict_resolutions WHERE operation_id = ?1",
                    [context.operation_id()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if operation.event_kind != "integration.conflict_resolved"
                || operation.actor_id != context.actor.as_str()
                || recorded_id.as_deref() != Some(resolution.id().as_str())
                || load_resolution(&transaction, artifact_store, resolution.id())? != *resolution
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok(());
        }
        validate_resolution(&transaction, artifact_store, resolution)?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS (SELECT 1 FROM conflict_resolutions WHERE resolution_id = ?1)",
            [resolution.id().as_str()],
            |row| row.get(0),
        )?;
        if exists {
            return Err(StoreError::InvariantViolation(
                "conflict resolution identity already exists",
            ));
        }
        insert_operation_record(&transaction, "integration.conflict_resolved", context)?;
        insert_resolution(&transaction, resolution, context)?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads and revalidates one immutable conflict resolution.
    ///
    /// # Errors
    ///
    /// Missing target, conflict, validation, or provenance history fails closed.
    pub fn conflict_resolution(
        &self,
        artifact_store: &ArtifactStore,
        id: &ConflictResolutionId,
    ) -> Result<ConflictResolution, StoreError> {
        load_resolution(&self.connection, artifact_store, id)
    }

    /// Starts execution after target compare-and-swap and target-scoped lease checks.
    ///
    /// # Errors
    ///
    /// Stale target/version, a live competing lease, or conflicting replay fails atomically.
    pub fn start_integration(
        &mut self,
        artifact_store: &ArtifactStore,
        id: &IntegrationId,
        expected_version: IntegrationVersion,
        lease: ExecutionLease,
        observation: &TargetObservation,
        context: &MutationContext,
    ) -> Result<IntegrationAttempt, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(outcome) = replay_attempt(
            &transaction,
            artifact_store,
            id,
            "integration.started",
            context,
        )? {
            let event = event_for_operation(&transaction, context.operation_id())?;
            if event.expected_version != expected_version.value()
                || event.observed_revision.as_deref()
                    != Some(observation.observed_revision().as_str())
                || event.observation_evidence.as_deref() != Some(observation.evidence().as_str())
                || event.lease_id.as_deref() != Some(lease.id().as_str())
                || event.lease_expires_at != Some(lease.expires_at().value())
                || event.holder_kind.as_deref() != Some(lease.holder().kind().as_str())
                || event.holder_id.as_deref() != Some(lease.holder().id().as_str())
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok(outcome);
        }
        let mut attempt = load_attempt(&transaction, artifact_store, id)?;
        validate_plan_current(&transaction, artifact_store, &attempt)?;
        ensure_no_unresolved_target_attempt(&transaction, &attempt)?;
        attempt.start(
            expected_version,
            lease,
            observation,
            context.occurred_at,
            context.actor.clone(),
        )?;
        insert_operation_record(&transaction, "integration.started", context)?;
        insert_event(
            &transaction,
            &attempt,
            expected_version,
            "integration.started",
            EventDetails {
                observation: Some(observation),
                ..EventDetails::default()
            },
            context,
        )?;
        transaction.commit()?;
        Ok(attempt)
    }

    /// Renews the exact current execution lease.
    ///
    /// # Errors
    ///
    /// Only the current unexpired holder may extend it.
    pub fn renew_integration_lease(
        &mut self,
        artifact_store: &ArtifactStore,
        id: &IntegrationId,
        renewal: &LeaseRenewal,
        context: &MutationContext,
    ) -> Result<IntegrationAttempt, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(outcome) = replay_attempt(
            &transaction,
            artifact_store,
            id,
            "integration.lease_renewed",
            context,
        )? {
            let event = event_for_operation(&transaction, context.operation_id())?;
            if event.expected_version != renewal.expected_version.value()
                || event.lease_id.as_deref() != Some(renewal.lease_id.as_str())
                || event.holder_kind.as_deref() != Some(renewal.holder.kind().as_str())
                || event.holder_id.as_deref() != Some(renewal.holder.id().as_str())
                || event.lease_expires_at != Some(renewal.expires_at.value())
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok(outcome);
        }
        let mut attempt = load_attempt(&transaction, artifact_store, id)?;
        attempt.renew_lease(
            renewal.expected_version,
            &renewal.lease_id,
            &renewal.holder,
            context.occurred_at,
            renewal.expires_at,
            context.actor.clone(),
        )?;
        insert_operation_record(&transaction, "integration.lease_renewed", context)?;
        insert_event(
            &transaction,
            &attempt,
            renewal.expected_version,
            "integration.lease_renewed",
            EventDetails::default(),
            context,
        )?;
        transaction.commit()?;
        Ok(attempt)
    }

    /// Moves an uncertain provider mutation into mandatory reconciliation.
    ///
    /// # Errors
    ///
    /// Before expiry exact authority is required; after expiry no blind retry is allowed.
    pub fn enter_integration_reconciliation(
        &mut self,
        artifact_store: &ArtifactStore,
        id: &IntegrationId,
        start: &ReconciliationStart,
        context: &MutationContext,
    ) -> Result<(IntegrationAttempt, ReconciliationObservation), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(outcome) = replay_attempt(
            &transaction,
            artifact_store,
            id,
            "integration.reconciliation_entered",
            context,
        )? {
            let stored =
                outcome
                    .latest_reconciliation()
                    .cloned()
                    .ok_or(StoreError::InvariantViolation(
                        "reconciliation event lacks observation",
                    ))?;
            if stored.id() != &start.reconciliation_id
                || stored.target() != &start.observation
                || event_for_operation(&transaction, context.operation_id())?.expected_version
                    != start.expected_version.value()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok((outcome, stored));
        }
        let mut attempt = load_attempt(&transaction, artifact_store, id)?;
        let reconciliation = attempt.enter_reconciliation(
            start.expected_version,
            start.reconciliation_id.clone(),
            start
                .authority
                .as_ref()
                .map(|(lease_id, holder)| (lease_id, holder)),
            start.observation.clone(),
            context.occurred_at,
            context.actor.clone(),
        )?;
        insert_operation_record(&transaction, "integration.reconciliation_entered", context)?;
        insert_event(
            &transaction,
            &attempt,
            start.expected_version,
            "integration.reconciliation_entered",
            EventDetails {
                observation: Some(reconciliation.target()),
                reconciliation: Some(&reconciliation),
                ..EventDetails::default()
            },
            context,
        )?;
        transaction.commit()?;
        Ok((attempt, reconciliation))
    }

    /// Appends one authoritative reconciliation observation.
    ///
    /// # Errors
    ///
    /// Outcome, target evidence, version, and operation replay must be exact.
    pub fn reconcile_integration(
        &mut self,
        artifact_store: &ArtifactStore,
        id: &IntegrationId,
        record: &ReconciliationRecord,
        context: &MutationContext,
    ) -> Result<(IntegrationAttempt, ReconciliationObservation), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(attempt) = replay_attempt(
            &transaction,
            artifact_store,
            id,
            "integration.reconciled",
            context,
        )? {
            let stored =
                attempt
                    .latest_reconciliation()
                    .cloned()
                    .ok_or(StoreError::InvariantViolation(
                        "reconciliation event lacks observation",
                    ))?;
            if stored.id() != &record.reconciliation_id
                || stored.outcome() != record.outcome
                || stored.target() != &record.observation
                || event_for_operation(&transaction, context.operation_id())?.expected_version
                    != record.expected_version.value()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok((attempt, stored));
        }
        let mut attempt = load_attempt(&transaction, artifact_store, id)?;
        let reconciliation = attempt.reconcile(
            record.expected_version,
            record.reconciliation_id.clone(),
            record.outcome,
            record.observation.clone(),
            context.occurred_at,
            context.actor.clone(),
        )?;
        insert_operation_record(&transaction, "integration.reconciled", context)?;
        insert_event(
            &transaction,
            &attempt,
            record.expected_version,
            "integration.reconciled",
            EventDetails {
                observation: Some(reconciliation.target()),
                reconciliation: Some(&reconciliation),
                ..EventDetails::default()
            },
            context,
        )?;
        transaction.commit()?;
        Ok((attempt, reconciliation))
    }

    /// Records a terminal conflict atomically with its attempt event.
    ///
    /// # Errors
    ///
    /// Authority, identity, evidence, provenance, and replay are checked exactly.
    pub fn conflict_integration(
        &mut self,
        artifact_store: &ArtifactStore,
        id: &IntegrationId,
        report: &ConflictReport,
        context: &MutationContext,
    ) -> Result<(IntegrationAttempt, IntegrationConflict), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(attempt) = replay_attempt(
            &transaction,
            artifact_store,
            id,
            "integration.conflicted",
            context,
        )? {
            let conflict = replay_conflict(&transaction, &attempt, context.operation_id())?;
            if conflict.id() != &report.conflict_id
                || conflict.provider_state() != &report.provider_state
                || event_for_operation(&transaction, context.operation_id())?.expected_version
                    != report.expected_version.value()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok((attempt, conflict));
        }
        let mut attempt = load_attempt(&transaction, artifact_store, id)?;
        let conflict = attempt.conflict(
            report.expected_version,
            report.conflict_id.clone(),
            report
                .authority
                .as_ref()
                .map(|(lease_id, holder)| (lease_id, holder)),
            report.provider_state.clone(),
            context.occurred_at,
            context.actor.clone(),
        )?;
        insert_operation_record(&transaction, "integration.conflicted", context)?;
        insert_event(
            &transaction,
            &attempt,
            report.expected_version,
            "integration.conflicted",
            EventDetails {
                conflict: Some(&conflict),
                ..EventDetails::default()
            },
            context,
        )?;
        insert_conflict(&transaction, &conflict, context)?;
        transaction.commit()?;
        Ok((attempt, conflict))
    }

    /// Records verified success and its immutable receipt atomically.
    ///
    /// # Errors
    ///
    /// Live authority or exact reconciliation, target evidence, and replay must match.
    pub fn succeed_integration(
        &mut self,
        artifact_store: &ArtifactStore,
        id: &IntegrationId,
        verification: &SuccessVerification,
        context: &MutationContext,
    ) -> Result<(IntegrationAttempt, IntegrationReceipt), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(attempt) = replay_attempt(
            &transaction,
            artifact_store,
            id,
            "integration.succeeded",
            context,
        )? {
            let receipt = replay_receipt(&transaction, &attempt, context.operation_id())?;
            if receipt.id() != &verification.receipt_id
                || receipt.result_revision() != verification.observation.observed_revision()
                || receipt.verification_evidence() != verification.observation.evidence()
                || event_for_operation(&transaction, context.operation_id())?.expected_version
                    != verification.expected_version.value()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok((attempt, receipt));
        }
        let mut attempt = load_attempt(&transaction, artifact_store, id)?;
        let receipt = attempt.succeed(
            verification.expected_version,
            verification.receipt_id.clone(),
            verification
                .authority
                .as_ref()
                .map(|(lease_id, holder)| (lease_id, holder)),
            &verification.observation,
            context.occurred_at,
            context.actor.clone(),
        )?;
        insert_operation_record(&transaction, "integration.succeeded", context)?;
        insert_event(
            &transaction,
            &attempt,
            verification.expected_version,
            "integration.succeeded",
            EventDetails {
                observation: Some(&verification.observation),
                receipt: Some(&receipt),
                ..EventDetails::default()
            },
            context,
        )?;
        insert_receipt(&transaction, &receipt, context)?;
        transaction.commit()?;
        Ok((attempt, receipt))
    }

    /// Terminates a reconciled no-effect attempt as failed or aborted.
    ///
    /// # Errors
    ///
    /// Exact no-effect evidence and version are required.
    pub fn finish_integration_no_effect(
        &mut self,
        artifact_store: &ArtifactStore,
        id: &IntegrationId,
        expected_version: IntegrationVersion,
        state: IntegrationState,
        context: &MutationContext,
    ) -> Result<IntegrationAttempt, StoreError> {
        let kind = match state {
            IntegrationState::Failed => "integration.failed",
            IntegrationState::Aborted => "integration.aborted",
            _ => {
                return Err(StoreError::InvariantViolation(
                    "no-effect state must be failed or aborted",
                ));
            }
        };
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(attempt) = replay_attempt(&transaction, artifact_store, id, kind, context)? {
            if event_for_operation(&transaction, context.operation_id())?.expected_version
                != expected_version.value()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok(attempt);
        }
        let mut attempt = load_attempt(&transaction, artifact_store, id)?;
        attempt.finish_no_effect(
            expected_version,
            state,
            context.occurred_at,
            context.actor.clone(),
        )?;
        insert_operation_record(&transaction, kind, context)?;
        insert_event(
            &transaction,
            &attempt,
            expected_version,
            kind,
            EventDetails::default(),
            context,
        )?;
        transaction.commit()?;
        Ok(attempt)
    }

    /// Closes an authoritatively diverged attempt before exact-target replanning.
    ///
    /// # Errors
    ///
    /// The latest reconciliation must be `Diverged`; replay and version are exact.
    pub fn supersede_diverged_integration(
        &mut self,
        artifact_store: &ArtifactStore,
        id: &IntegrationId,
        expected_version: IntegrationVersion,
        context: &MutationContext,
    ) -> Result<IntegrationAttempt, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(attempt) = replay_attempt(
            &transaction,
            artifact_store,
            id,
            "integration.superseded",
            context,
        )? {
            if event_for_operation(&transaction, context.operation_id())?.expected_version
                != expected_version.value()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok(attempt);
        }
        let mut attempt = load_attempt(&transaction, artifact_store, id)?;
        attempt.supersede_diverged(expected_version, context.occurred_at, context.actor.clone())?;
        insert_operation_record(&transaction, "integration.superseded", context)?;
        insert_event(
            &transaction,
            &attempt,
            expected_version,
            "integration.superseded",
            EventDetails::default(),
            context,
        )?;
        transaction.commit()?;
        Ok(attempt)
    }

    /// Aborts an unstarted integration plan.
    ///
    /// # Errors
    ///
    /// Running or uncertain attempts cannot use this transition.
    pub fn abort_planned_integration(
        &mut self,
        artifact_store: &ArtifactStore,
        id: &IntegrationId,
        expected_version: IntegrationVersion,
        context: &MutationContext,
    ) -> Result<IntegrationAttempt, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(attempt) = replay_attempt(
            &transaction,
            artifact_store,
            id,
            "integration.aborted",
            context,
        )? {
            if event_for_operation(&transaction, context.operation_id())?.expected_version
                != expected_version.value()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok(attempt);
        }
        let mut attempt = load_attempt(&transaction, artifact_store, id)?;
        attempt.abort_planned(expected_version, context.occurred_at, context.actor.clone())?;
        insert_operation_record(&transaction, "integration.aborted", context)?;
        insert_event(
            &transaction,
            &attempt,
            expected_version,
            "integration.aborted",
            EventDetails::default(),
            context,
        )?;
        transaction.commit()?;
        Ok(attempt)
    }
}

fn validate_plan_current(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    attempt: &IntegrationAttempt,
) -> Result<(), StoreError> {
    validate_plan_source(connection, artifact_store, attempt)?;
    let candidate = super::load_candidate_internal(
        connection,
        artifact_store,
        attempt.intent().binding().candidate_id(),
    )?;
    let freshness = candidate_freshness(connection, artifact_store, &candidate)?;
    if !freshness.is_current() {
        return Err(StoreError::IntegrationGateRejected("candidate is stale"));
    }
    for id in attempt.gate().review_refs() {
        let submission = super::load_review_submission_internal(connection, artifact_store, id)?;
        if !exact_target_is_current(connection, artifact_store, submission.target())? {
            return Err(StoreError::IntegrationGateRejected(
                "review target is stale",
            ));
        }
    }
    for id in attempt.gate().validation_refs() {
        let result = super::load_validation_result_internal(connection, artifact_store, id)?;
        if !exact_target_is_current(connection, artifact_store, result.target())? {
            return Err(StoreError::IntegrationGateRejected(
                "validation target is stale",
            ));
        }
    }
    Ok(())
}

fn validate_plan_source(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    attempt: &IntegrationAttempt,
) -> Result<(), StoreError> {
    let candidate = super::load_candidate_internal(
        connection,
        artifact_store,
        attempt.intent().binding().candidate_id(),
    )?;
    let binding = attempt.intent().binding();
    if candidate.content_digest().as_str() != binding.candidate_digest()
        || candidate.inputs() != binding.ordered_inputs()
        || candidate.target_base().repository_id() != attempt.intent().target().repository_id()
        || candidate.target_base().object_id()
            != attempt.intent().target().expected_revision().as_str()
    {
        return Err(StoreError::IntegrationGateRejected(
            "candidate binding or target base differs",
        ));
    }
    for id in attempt.gate().review_refs() {
        let submission = super::load_review_submission_internal(connection, artifact_store, id)?;
        if submission.outcome() != ReviewOutcome::Approved
            || !target_is_within_candidate(submission.target(), &candidate)
        {
            return Err(StoreError::IntegrationGateRejected(
                "review is not an approval for this candidate",
            ));
        }
    }
    for id in attempt.gate().validation_refs() {
        let result = super::load_validation_result_internal(connection, artifact_store, id)?;
        if result.outcome() != ValidationOutcome::Passed
            || !target_is_within_candidate(result.target(), &candidate)
        {
            return Err(StoreError::IntegrationGateRejected(
                "validation did not pass for this candidate",
            ));
        }
    }
    Ok(())
}

fn candidate_freshness(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    candidate: &weft_domain::CompositionCandidate,
) -> Result<super::CandidateFreshness, StoreError> {
    let mut advanced_inputs = Vec::new();
    for input in candidate.inputs() {
        let change =
            super::load_change_from_connection(connection, artifact_store, input.change_id())?;
        if change.head() != Some(input.revision_id()) {
            advanced_inputs.push(input.change_id().clone());
        }
    }
    let mut changed_dependencies = Vec::new();
    for requirement in candidate.requirements() {
        if let weft_domain::ResolvedRequirementSource::Dependency {
            dependency_id,
            version,
        } = requirement.source()
        {
            let changed =
                match super::load_dependency_internal(connection, dependency_id, None, true) {
                    Ok(dependency) => {
                        !dependency.is_active()
                            || dependency.version() != *version
                            || dependency.pins().downstream_revision_id()
                                != requirement.downstream().revision_id()
                            || dependency.pins().upstream_revision_id()
                                != requirement.upstream().revision_id()
                    }
                    Err(StoreError::DependencyNotFound(_)) => true,
                    Err(error) => return Err(error),
                };
            if changed {
                changed_dependencies.push(dependency_id.clone());
            }
        }
    }
    let stack_changed = if let Some(stack_ref) = candidate.stack() {
        super::load_stack_internal(connection, stack_ref.stack_id(), None, true)?.version()
            != stack_ref.version()
    } else {
        false
    };
    Ok(super::CandidateFreshness {
        advanced_inputs,
        changed_dependencies,
        stack_changed,
    })
}

fn exact_target_is_current(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    target: &weft_domain::ExactTarget,
) -> Result<bool, StoreError> {
    super::verify_exact_target(connection, artifact_store, target)?;
    match target {
        weft_domain::ExactTarget::Revision {
            change_id,
            revision_id,
            ..
        } => Ok(
            super::load_change_from_connection(connection, artifact_store, change_id)?.head()
                == Some(revision_id),
        ),
        weft_domain::ExactTarget::Candidate { candidate_id, .. } => {
            let candidate =
                super::load_candidate_internal(connection, artifact_store, candidate_id)?;
            Ok(candidate_freshness(connection, artifact_store, &candidate)?.is_current())
        }
    }
}

fn target_is_within_candidate(
    target: &weft_domain::ExactTarget,
    candidate: &weft_domain::CompositionCandidate,
) -> bool {
    match target {
        weft_domain::ExactTarget::Candidate {
            candidate_id,
            content_digest,
            ..
        } => {
            candidate_id == candidate.id() && content_digest == candidate.content_digest().as_str()
        }
        weft_domain::ExactTarget::Revision {
            change_id,
            revision_id,
            base,
            ..
        } => {
            base.repository_id() == candidate.target_base().repository_id()
                && candidate.inputs().iter().any(|input| {
                    input.change_id() == change_id && input.revision_id() == revision_id
                })
        }
    }
}

fn attempt_exists(connection: &Connection, id: &IntegrationId) -> Result<bool, StoreError> {
    Ok(connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM integration_attempts WHERE integration_id = ?1)",
        [id.as_str()],
        |row| row.get(0),
    )?)
}

fn replay_attempt(
    connection: &Transaction<'_>,
    artifact_store: &ArtifactStore,
    id: &IntegrationId,
    kind: &str,
    context: &MutationContext,
) -> Result<Option<IntegrationAttempt>, StoreError> {
    let Some(operation) = recorded_operation(connection, context.operation_id())? else {
        return Ok(None);
    };
    if operation.event_kind != kind || operation.actor_id != context.actor.as_str() {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    if event_id_for_operation(connection, context.operation_id())?.as_deref() != Some(id.as_str()) {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    let event = event_for_operation(connection, context.operation_id())?;
    let current = load_attempt(connection, artifact_store, id)?;
    Ok(Some(load_attempt_before_event(
        connection,
        &current,
        event.resulting_version,
    )?))
}

fn event_id_for_operation(
    connection: &Connection,
    operation_id: &str,
) -> Result<Option<String>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT integration_id FROM integration_events WHERE operation_id = ?1",
            [operation_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn insert_plan(
    transaction: &Transaction<'_>,
    attempt: &IntegrationAttempt,
    context: &MutationContext,
) -> Result<(), StoreError> {
    let binding = attempt.intent().binding();
    let target = attempt.intent().target();
    let method = attempt.intent().method();
    let gate = attempt.gate();
    transaction.execute(
        "INSERT INTO integration_attempts (
            integration_id, candidate_id, candidate_digest, input_count,
            repository_id, target_ref, expected_target_revision, provider_id,
            strategy, effect_operation_id, policy_evidence, capability_evidence,
            review_ref_count, validation_ref_count, planned_observed_revision,
            planned_observation_evidence, created_at_unix_ms, created_by, operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![attempt.id().as_str(), binding.candidate_id().as_str(), binding.candidate_digest(), i64::try_from(binding.ordered_inputs().len()).map_err(|_| StoreError::CollectionTooLarge)?, target.repository_id().as_str(), target.target_ref().as_str(), target.expected_revision().as_str(), method.provider_id().as_str(), method.strategy().as_str(), method.effect_operation_id().as_str(), gate.policy_evidence().as_str(), gate.capability_evidence().as_str(), i64::try_from(gate.review_refs().len()).map_err(|_| StoreError::CollectionTooLarge)?, i64::try_from(gate.validation_refs().len()).map_err(|_| StoreError::CollectionTooLarge)?, gate.target_observation().observed_revision().as_str(), gate.target_observation().evidence().as_str(), attempt.created_at().value(), attempt.created_by().as_str(), context.operation_id()],
    )?;
    for (position, input) in binding.ordered_inputs().iter().enumerate() {
        transaction.execute("INSERT INTO integration_attempt_inputs (integration_id, input_position, change_id, revision_id) VALUES (?1, ?2, ?3, ?4)", params![attempt.id().as_str(), i64::try_from(position).map_err(|_| StoreError::CollectionTooLarge)?, input.change_id().as_str(), input.revision_id().as_str()])?;
    }
    for (position, id) in gate.review_refs().iter().enumerate() {
        transaction.execute("INSERT INTO integration_attempt_review_refs (integration_id, ref_position, review_submission_id) VALUES (?1, ?2, ?3)", params![attempt.id().as_str(), i64::try_from(position).map_err(|_| StoreError::CollectionTooLarge)?, id.as_str()])?;
    }
    for (position, id) in gate.validation_refs().iter().enumerate() {
        transaction.execute("INSERT INTO integration_attempt_validation_refs (integration_id, ref_position, validation_result_id) VALUES (?1, ?2, ?3)", params![attempt.id().as_str(), i64::try_from(position).map_err(|_| StoreError::CollectionTooLarge)?, id.as_str()])?;
    }
    transaction.execute(
        "INSERT INTO integration_events (integration_id, event_kind, expected_version, resulting_version, resulting_state, operation_id, actor_id, occurred_at_unix_ms) VALUES (?1, 'integration.planned', 0, 1, 'planned', ?2, ?3, ?4)",
        params![attempt.id().as_str(), context.operation_id(), context.actor.as_str(), context.occurred_at.value()],
    )?;
    Ok(())
}

fn insert_event(
    transaction: &Transaction<'_>,
    attempt: &IntegrationAttempt,
    expected_version: IntegrationVersion,
    kind: &str,
    details: EventDetails<'_>,
    context: &MutationContext,
) -> Result<(), StoreError> {
    let lease = attempt.lease();
    transaction.execute(
        "INSERT INTO integration_events (
            integration_id, event_kind, expected_version, resulting_version,
            resulting_state, observed_revision, observation_evidence, lease_id,
            lease_holder_kind, lease_holder_id, lease_acquired_at_unix_ms,
            lease_expires_at_unix_ms, lease_version, reconciliation_id,
            reconciliation_outcome, conflict_id, provider_state, receipt_id,
            result_revision, operation_id, actor_id, occurred_at_unix_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
        params![attempt.id().as_str(), kind, expected_version.value(), attempt.version().value(), attempt.state().as_str(), details.observation.map(|value| value.observed_revision().as_str()), details.observation.map(|value| value.evidence().as_str()), lease.map(|value| value.id().as_str()), lease.map(|value| value.holder().kind().as_str()), lease.map(|value| value.holder().id().as_str()), lease.map(|value| value.acquired_at().value()), lease.map(|value| value.expires_at().value()), lease.map(|value| value.version().value()), details.reconciliation.map(|value| value.id().as_str()), details.reconciliation.map(|value| value.outcome().as_str()), details.conflict.map(|value| value.id().as_str()), details.conflict.map(|value| value.provider_state().as_str()), details.receipt.map(|value| value.id().as_str()), details.receipt.map(|value| value.result_revision().as_str()).or_else(|| attempt.result_revision().map(TargetRevision::as_str)), context.operation_id(), context.actor.as_str(), context.occurred_at.value()],
    )?;
    Ok(())
}

fn load_plan_row(connection: &Connection, id: &IntegrationId) -> Result<PlanRow, StoreError> {
    connection
        .query_row(
            "SELECT candidate_id, candidate_digest, repository_id, target_ref,
                expected_target_revision, provider_id, strategy, effect_operation_id,
                policy_evidence, capability_evidence, planned_observed_revision,
                planned_observation_evidence, created_at_unix_ms, created_by,
                operation_id, input_count, review_ref_count, validation_ref_count
         FROM integration_attempts WHERE integration_id = ?1",
            [id.as_str()],
            |row| {
                Ok(PlanRow {
                    candidate_id: row.get(0)?,
                    candidate_digest: row.get(1)?,
                    repository_id: row.get(2)?,
                    target_ref: row.get(3)?,
                    expected_revision: row.get(4)?,
                    provider_id: row.get(5)?,
                    strategy: row.get(6)?,
                    effect_operation_id: row.get(7)?,
                    policy_evidence: row.get(8)?,
                    capability_evidence: row.get(9)?,
                    observed_revision: row.get(10)?,
                    observation_evidence: row.get(11)?,
                    created_at: row.get(12)?,
                    created_by: row.get(13)?,
                    operation_id: row.get(14)?,
                    input_count: row.get(15)?,
                    review_count: row.get(16)?,
                    validation_count: row.get(17)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::IntegrationNotFound(id.clone()))
}

fn reconstruct_plan(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    id: &IntegrationId,
    row: &PlanRow,
) -> Result<IntegrationAttempt, StoreError> {
    let inputs = load_inputs(connection, id)?;
    let review_refs = load_review_refs(connection, id)?;
    let validation_refs = load_validation_refs(connection, id)?;
    if usize_to_i64(inputs.len())? != row.input_count
        || usize_to_i64(review_refs.len())? != row.review_count
        || usize_to_i64(validation_refs.len())? != row.validation_count
    {
        return Err(StoreError::InvariantViolation(
            "integration finalized collection count differs",
        ));
    }
    let intent = IntegrationIntent::new(
        IntegrationBinding::new(
            CandidateId::new(row.candidate_id.clone())?,
            row.candidate_digest.clone(),
            inputs,
        )?,
        IntegrationTarget::new(
            RepositoryId::new(row.repository_id.clone())?,
            TargetRef::new(row.target_ref.clone())?,
            TargetRevision::new(row.expected_revision.clone())?,
        ),
        IntegrationMethod::new(
            ProviderId::new(row.provider_id.clone())?,
            IntegrationStrategy::new(row.strategy.clone())?,
            EffectOperationId::new(row.effect_operation_id.clone())?,
        ),
    );
    let gate = IntegrationGate::new(
        GatePolicyEvidence::new(row.policy_evidence.clone())?,
        IntegrationCapabilityEvidence::new(row.capability_evidence.clone())?,
        review_refs,
        validation_refs,
        TargetObservation::new(
            intent.target().target_ref().clone(),
            TargetRevision::new(row.observed_revision.clone())?,
            IntegrationEvidence::new(row.observation_evidence.clone())?,
        ),
    );
    let attempt = IntegrationAttempt::plan(
        id.clone(),
        intent,
        gate,
        UnixMillis::new(row.created_at)?,
        ActorId::new(row.created_by.clone())?,
    )?;
    let operation = connection.query_row("SELECT event_kind, actor_id, occurred_at_unix_ms FROM operation_records WHERE operation_id = ?1", [&row.operation_id], |value| Ok((value.get::<_, String>(0)?, value.get::<_, String>(1)?, value.get::<_, i64>(2)?))).optional()?.ok_or(StoreError::InvariantViolation("integration plan operation is missing"))?;
    if operation
        != (
            "integration.planned".to_owned(),
            row.created_by.clone(),
            row.created_at,
        )
    {
        return Err(StoreError::InvariantViolation(
            "integration plan operation drift",
        ));
    }
    validate_plan_source(connection, artifact_store, &attempt)?;
    Ok(attempt)
}

fn load_attempt(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    id: &IntegrationId,
) -> Result<IntegrationAttempt, StoreError> {
    let row = load_plan_row(connection, id)?;
    let mut attempt = reconstruct_plan(connection, artifact_store, id, &row)?;
    let events = load_events(connection, id)?;
    if events.first().is_none_or(|event| {
        event.kind != "integration.planned"
            || event.expected_version != 0
            || event.resulting_version != 1
            || event.resulting_state != "planned"
            || event.operation_id != row.operation_id
    }) {
        return Err(StoreError::InvariantViolation(
            "integration initial event drift",
        ));
    }
    for event in events.iter().skip(1) {
        replay_event(connection, &mut attempt, event)?;
    }
    verify_terminal_row_shape(connection, &attempt)?;
    if attempt.version().value() != events.last().map_or(0, |event| event.resulting_version)
        || attempt.state().as_str()
            != events
                .last()
                .map_or("", |event| event.resulting_state.as_str())
    {
        return Err(StoreError::InvariantViolation(
            "integration event head drift",
        ));
    }
    Ok(attempt)
}

fn verify_terminal_row_shape(
    connection: &Connection,
    attempt: &IntegrationAttempt,
) -> Result<(), StoreError> {
    let (conflicts, receipts) = connection.query_row(
        "SELECT
            (SELECT count(*) FROM integration_conflicts WHERE integration_id = ?1),
            (SELECT count(*) FROM integration_receipts WHERE integration_id = ?1)",
        [attempt.id().as_str()],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let expected_conflicts = i64::from(attempt.state() == IntegrationState::Conflicted);
    let expected_receipts = i64::from(attempt.state() == IntegrationState::Succeeded);
    if conflicts != expected_conflicts || receipts != expected_receipts {
        return Err(StoreError::InvariantViolation(
            "integration terminal evidence cardinality drift",
        ));
    }
    Ok(())
}

fn load_inputs(
    connection: &Connection,
    id: &IntegrationId,
) -> Result<Vec<CandidateInput>, StoreError> {
    let mut statement = connection.prepare("SELECT change_id, revision_id FROM integration_attempt_inputs WHERE integration_id = ?1 ORDER BY input_position")?;
    statement
        .query_map([id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .map(|row| {
            let (change, revision) = row?;
            Ok(CandidateInput::new(
                weft_domain::ChangeId::new(change)?,
                weft_domain::RevisionId::new(revision)?,
            ))
        })
        .collect()
}

fn load_review_refs(
    connection: &Connection,
    id: &IntegrationId,
) -> Result<Vec<ReviewSubmissionId>, StoreError> {
    load_strings(connection, "SELECT review_submission_id FROM integration_attempt_review_refs WHERE integration_id = ?1 ORDER BY ref_position", id)?.into_iter().map(|value| Ok(ReviewSubmissionId::new(value)?)).collect()
}

fn load_validation_refs(
    connection: &Connection,
    id: &IntegrationId,
) -> Result<Vec<ValidationResultId>, StoreError> {
    load_strings(connection, "SELECT validation_result_id FROM integration_attempt_validation_refs WHERE integration_id = ?1 ORDER BY ref_position", id)?.into_iter().map(|value| Ok(ValidationResultId::new(value)?)).collect()
}

fn load_strings(
    connection: &Connection,
    sql: &str,
    id: &IntegrationId,
) -> Result<Vec<String>, StoreError> {
    let mut statement = connection.prepare(sql)?;
    Ok(statement
        .query_map([id.as_str()], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn load_events(
    connection: &Connection,
    id: &IntegrationId,
) -> Result<Vec<StoredEvent>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT event.event_kind, event.expected_version, event.resulting_version,
                event.resulting_state, event.observed_revision, event.observation_evidence,
                event.lease_id, event.lease_holder_kind, event.lease_holder_id,
                event.lease_acquired_at_unix_ms, event.lease_expires_at_unix_ms,
                event.lease_version, event.reconciliation_id, event.reconciliation_outcome,
                event.conflict_id, event.provider_state, event.receipt_id,
                event.result_revision, event.operation_id, event.actor_id,
                event.occurred_at_unix_ms
         FROM integration_events AS event
         JOIN operation_records AS operation USING (operation_id)
         WHERE event.integration_id = ?1
           AND operation.event_kind = event.event_kind
           AND operation.actor_id = event.actor_id
           AND operation.occurred_at_unix_ms = event.occurred_at_unix_ms
         ORDER BY event.resulting_version",
    )?;
    Ok(statement
        .query_map([id.as_str()], event_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn event_from_row(row: &rusqlite::Row<'_>) -> Result<StoredEvent, rusqlite::Error> {
    Ok(StoredEvent {
        kind: row.get(0)?,
        expected_version: row.get(1)?,
        resulting_version: row.get(2)?,
        resulting_state: row.get(3)?,
        observed_revision: row.get(4)?,
        observation_evidence: row.get(5)?,
        lease_id: row.get(6)?,
        holder_kind: row.get(7)?,
        holder_id: row.get(8)?,
        lease_acquired_at: row.get(9)?,
        lease_expires_at: row.get(10)?,
        lease_version: row.get(11)?,
        reconciliation_id: row.get(12)?,
        reconciliation_outcome: row.get(13)?,
        conflict_id: row.get(14)?,
        provider_state: row.get(15)?,
        receipt_id: row.get(16)?,
        result_revision: row.get(17)?,
        operation_id: row.get(18)?,
        actor_id: row.get(19)?,
        occurred_at: row.get(20)?,
    })
}

fn event_for_operation(
    connection: &Connection,
    operation_id: &str,
) -> Result<StoredEvent, StoreError> {
    connection.query_row("SELECT event_kind, expected_version, resulting_version, resulting_state, observed_revision, observation_evidence, lease_id, lease_holder_kind, lease_holder_id, lease_acquired_at_unix_ms, lease_expires_at_unix_ms, lease_version, reconciliation_id, reconciliation_outcome, conflict_id, provider_state, receipt_id, result_revision, operation_id, actor_id, occurred_at_unix_ms FROM integration_events WHERE operation_id = ?1", [operation_id], event_from_row).optional()?.ok_or(StoreError::InvariantViolation("integration operation lacks event"))
}

fn replay_event(
    connection: &Connection,
    attempt: &mut IntegrationAttempt,
    event: &StoredEvent,
) -> Result<(), StoreError> {
    let expected = IntegrationVersion::new(event.expected_version)?;
    let at = UnixMillis::new(event.occurred_at)?;
    let actor = ActorId::new(event.actor_id.clone())?;
    match event.kind.as_str() {
        "integration.started" | "integration.lease_renewed" => {
            replay_execution_event(attempt, event, expected, at, actor)?;
        }
        "integration.reconciliation_entered" | "integration.reconciled" => {
            replay_reconciliation_event(attempt, event, expected, at, actor)?;
        }
        "integration.conflicted"
        | "integration.succeeded"
        | "integration.failed"
        | "integration.aborted"
        | "integration.superseded" => {
            replay_terminal_event(connection, attempt, event, expected, at, actor)?;
        }
        _ => {
            return Err(StoreError::InvariantViolation(
                "unknown integration event kind",
            ));
        }
    }
    if attempt.version().value() != event.resulting_version
        || attempt.state().as_str() != event.resulting_state
        || attempt.result_revision().map(TargetRevision::as_str) != event.result_revision.as_deref()
    {
        return Err(StoreError::InvariantViolation(
            "integration event result drift",
        ));
    }
    Ok(())
}

fn replay_execution_event(
    attempt: &mut IntegrationAttempt,
    event: &StoredEvent,
    expected: IntegrationVersion,
    at: UnixMillis,
    actor: ActorId,
) -> Result<(), StoreError> {
    if event.kind == "integration.started" {
        let observation = observation_from_event(attempt, event)?;
        attempt.start(expected, lease_from_event(event)?, &observation, at, actor)?;
        return Ok(());
    }
    let current = attempt.lease().ok_or(StoreError::InvariantViolation(
        "lease renewal lacks prior lease",
    ))?;
    let lease_id = current.id().clone();
    let holder = current.holder().clone();
    let expires_at = UnixMillis::new(
        event
            .lease_expires_at
            .ok_or(StoreError::InvariantViolation("lease expiry missing"))?,
    )?;
    attempt.renew_lease(expected, &lease_id, &holder, at, expires_at, actor)?;
    let renewed = attempt
        .lease()
        .ok_or(StoreError::InvariantViolation("renewed lease missing"))?;
    if renewed.id() != &lease_id
        || renewed.holder() != &holder
        || renewed.expires_at() != expires_at
        || renewed.version().value()
            != event
                .lease_version
                .ok_or(StoreError::InvariantViolation("lease version missing"))?
    {
        return Err(StoreError::InvariantViolation(
            "renewed lease snapshot drift",
        ));
    }
    Ok(())
}

fn replay_reconciliation_event(
    attempt: &mut IntegrationAttempt,
    event: &StoredEvent,
    expected: IntegrationVersion,
    at: UnixMillis,
    actor: ActorId,
) -> Result<(), StoreError> {
    let observation = observation_from_event(attempt, event)?;
    let id = ReconciliationId::new(
        required(event.reconciliation_id.as_deref(), "reconciliation ID")?.to_owned(),
    )?;
    if event.kind == "integration.reconciled" {
        attempt.reconcile(
            expected,
            id,
            ReconciliationOutcome::parse(required(
                event.reconciliation_outcome.as_deref(),
                "reconciliation outcome",
            )?)?,
            observation,
            at,
            actor,
        )?;
        return Ok(());
    }
    let authority = attempt
        .lease()
        .filter(|lease| at < lease.expires_at())
        .map(|lease| (lease.id().clone(), lease.holder().clone()));
    attempt.enter_reconciliation(
        expected,
        id,
        authority
            .as_ref()
            .map(|(lease_id, holder)| (lease_id, holder)),
        observation,
        at,
        actor,
    )?;
    Ok(())
}

fn replay_terminal_event(
    connection: &Connection,
    attempt: &mut IntegrationAttempt,
    event: &StoredEvent,
    expected: IntegrationVersion,
    at: UnixMillis,
    actor: ActorId,
) -> Result<(), StoreError> {
    if event.kind == "integration.failed" {
        attempt.finish_no_effect(expected, IntegrationState::Failed, at, actor)?;
        return Ok(());
    }
    if event.kind == "integration.aborted" {
        if attempt.state() == IntegrationState::Planned {
            attempt.abort_planned(expected, at, actor)?;
        } else {
            attempt.finish_no_effect(expected, IntegrationState::Aborted, at, actor)?;
        }
        return Ok(());
    }
    if event.kind == "integration.superseded" {
        attempt.supersede_diverged(expected, at, actor)?;
        return Ok(());
    }
    let authority = if attempt.state() == IntegrationState::Running {
        attempt
            .lease()
            .map(|lease| (lease.id().clone(), lease.holder().clone()))
    } else {
        None
    };
    if event.kind == "integration.conflicted" {
        let conflict = attempt.conflict(
            expected,
            IntegrationConflictId::new(
                required(event.conflict_id.as_deref(), "conflict ID")?.to_owned(),
            )?,
            authority
                .as_ref()
                .map(|(lease_id, holder)| (lease_id, holder)),
            IntegrationEvidence::new(
                required(event.provider_state.as_deref(), "provider state")?.to_owned(),
            )?,
            at,
            actor,
        )?;
        verify_conflict_row(connection, &conflict, &event.operation_id)?;
    } else {
        let observation = observation_from_event(attempt, event)?;
        let receipt = attempt.succeed(
            expected,
            IntegrationReceiptId::new(
                required(event.receipt_id.as_deref(), "receipt ID")?.to_owned(),
            )?,
            authority
                .as_ref()
                .map(|(lease_id, holder)| (lease_id, holder)),
            &observation,
            at,
            actor,
        )?;
        verify_receipt_row(connection, &receipt, &event.operation_id)?;
    }
    Ok(())
}

fn lease_from_event(event: &StoredEvent) -> Result<ExecutionLease, StoreError> {
    let lease = ExecutionLease::new(
        ExecutionLeaseId::new(required(event.lease_id.as_deref(), "lease ID")?.to_owned())?,
        Subject::new(
            SubjectKind::parse(required(event.holder_kind.as_deref(), "lease holder kind")?)?,
            SubjectId::new(required(event.holder_id.as_deref(), "lease holder ID")?.to_owned())?,
        ),
        UnixMillis::new(
            event
                .lease_acquired_at
                .ok_or(StoreError::InvariantViolation("lease acquisition missing"))?,
        )?,
        UnixMillis::new(
            event
                .lease_expires_at
                .ok_or(StoreError::InvariantViolation("lease expiry missing"))?,
        )?,
    )?;
    let target_version = IntegrationVersion::new(
        event
            .lease_version
            .ok_or(StoreError::InvariantViolation("lease version missing"))?,
    )?;
    if lease.version() != target_version {
        return Err(StoreError::InvariantViolation(
            "initial lease version drift",
        ));
    }
    Ok(lease)
}

fn observation_from_event(
    attempt: &IntegrationAttempt,
    event: &StoredEvent,
) -> Result<TargetObservation, StoreError> {
    Ok(TargetObservation::new(
        attempt.intent().target().target_ref().clone(),
        TargetRevision::new(
            required(event.observed_revision.as_deref(), "observed revision")?.to_owned(),
        )?,
        IntegrationEvidence::new(
            required(
                event.observation_evidence.as_deref(),
                "observation evidence",
            )?
            .to_owned(),
        )?,
    ))
}

fn required<'a>(value: Option<&'a str>, field: &'static str) -> Result<&'a str, StoreError> {
    value.ok_or(StoreError::InvariantViolation(field))
}

fn usize_to_i64(value: usize) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::CollectionTooLarge)
}

fn ensure_no_unresolved_target_attempt(
    connection: &Connection,
    attempt: &IntegrationAttempt,
) -> Result<(), StoreError> {
    let held: bool = connection.query_row(
        "SELECT EXISTS (
            SELECT 1 FROM integration_attempts AS planned
            JOIN integration_events AS head ON head.integration_id = planned.integration_id
            WHERE planned.repository_id = ?1 AND planned.target_ref = ?2
              AND planned.integration_id <> ?3
              AND head.resulting_state IN ('running', 'reconciling')
              AND head.resulting_version = (SELECT MAX(latest.resulting_version) FROM integration_events AS latest WHERE latest.integration_id = planned.integration_id)
         )",
        params![attempt.intent().target().repository_id().as_str(), attempt.intent().target().target_ref().as_str(), attempt.id().as_str()],
        |row| row.get(0),
    )?;
    if held {
        return Err(StoreError::IntegrationTargetHeld);
    }
    Ok(())
}

fn insert_conflict(
    transaction: &Transaction<'_>,
    conflict: &IntegrationConflict,
    context: &MutationContext,
) -> Result<(), StoreError> {
    transaction.execute("INSERT INTO integration_conflicts (conflict_id, integration_id, candidate_id, candidate_digest, provider_id, provider_state, created_at_unix_ms, created_by, operation_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", params![conflict.id().as_str(), conflict.attempt_id().as_str(), conflict.candidate_id().as_str(), conflict.candidate_digest(), conflict.provider_id().as_str(), conflict.provider_state().as_str(), conflict.created_at().value(), conflict.created_by().as_str(), context.operation_id()])?;
    for (position, input) in conflict.ordered_inputs().iter().enumerate() {
        transaction.execute("INSERT INTO integration_conflict_inputs (conflict_id, input_position, change_id, revision_id) VALUES (?1, ?2, ?3, ?4)", params![conflict.id().as_str(), usize_to_i64(position)?, input.change_id().as_str(), input.revision_id().as_str()])?;
    }
    Ok(())
}

fn verify_conflict_row(
    connection: &Connection,
    conflict: &IntegrationConflict,
    operation_id: &str,
) -> Result<(), StoreError> {
    let row = connection.query_row("SELECT integration_id, candidate_id, candidate_digest, provider_id, provider_state, created_at_unix_ms, created_by, operation_id FROM integration_conflicts WHERE conflict_id = ?1", [conflict.id().as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, i64>(5)?, row.get::<_, String>(6)?, row.get::<_, String>(7)?))).optional()?.ok_or(StoreError::InvariantViolation("integration conflict row is missing"))?;
    if row
        != (
            conflict.attempt_id().as_str().to_owned(),
            conflict.candidate_id().as_str().to_owned(),
            conflict.candidate_digest().to_owned(),
            conflict.provider_id().as_str().to_owned(),
            conflict.provider_state().as_str().to_owned(),
            conflict.created_at().value(),
            conflict.created_by().as_str().to_owned(),
            operation_id.to_owned(),
        )
    {
        return Err(StoreError::InvariantViolation(
            "integration conflict row drift",
        ));
    }
    let mut statement = connection.prepare("SELECT change_id, revision_id FROM integration_conflict_inputs WHERE conflict_id = ?1 ORDER BY input_position")?;
    let inputs = statement
        .query_map([conflict.id().as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let expected = conflict
        .ordered_inputs()
        .iter()
        .map(|input| {
            (
                input.change_id().as_str().to_owned(),
                input.revision_id().as_str().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    if inputs != expected {
        return Err(StoreError::InvariantViolation(
            "integration conflict input drift",
        ));
    }
    Ok(())
}

fn replay_conflict(
    connection: &Connection,
    attempt: &IntegrationAttempt,
    operation_id: &str,
) -> Result<IntegrationConflict, StoreError> {
    let event = event_for_operation(connection, operation_id)?;
    let mut replay = load_attempt_before_event(connection, attempt, event.expected_version)?;
    let authority = if replay.state() == IntegrationState::Running {
        replay
            .lease()
            .map(|lease| (lease.id().clone(), lease.holder().clone()))
    } else {
        None
    };
    Ok(replay.conflict(
        IntegrationVersion::new(event.expected_version)?,
        IntegrationConflictId::new(
            required(event.conflict_id.as_deref(), "conflict ID")?.to_owned(),
        )?,
        authority
            .as_ref()
            .map(|(lease_id, holder)| (lease_id, holder)),
        IntegrationEvidence::new(
            required(event.provider_state.as_deref(), "provider state")?.to_owned(),
        )?,
        UnixMillis::new(event.occurred_at)?,
        ActorId::new(event.actor_id)?,
    )?)
}

fn insert_receipt(
    transaction: &Transaction<'_>,
    receipt: &IntegrationReceipt,
    context: &MutationContext,
) -> Result<(), StoreError> {
    transaction.execute("INSERT INTO integration_receipts (receipt_id, integration_id, candidate_id, candidate_digest, repository_id, target_ref, prior_revision, result_revision, provider_id, effect_operation_id, verification_evidence, verified_at_unix_ms, verified_by, operation_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)", params![receipt.id().as_str(), receipt.attempt_id().as_str(), receipt.candidate_id().as_str(), receipt.candidate_digest(), receipt.repository_id().as_str(), receipt.target_ref().as_str(), receipt.prior_revision().as_str(), receipt.result_revision().as_str(), receipt.provider_id().as_str(), receipt.effect_operation_id().as_str(), receipt.verification_evidence().as_str(), receipt.verified_at().value(), receipt.verified_by().as_str(), context.operation_id()])?;
    Ok(())
}

fn verify_receipt_row(
    connection: &Connection,
    receipt: &IntegrationReceipt,
    operation_id: &str,
) -> Result<(), StoreError> {
    let matches: bool = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM integration_receipts WHERE receipt_id = ?1 AND integration_id = ?2 AND candidate_id = ?3 AND candidate_digest = ?4 AND repository_id = ?5 AND target_ref = ?6 AND prior_revision = ?7 AND result_revision = ?8 AND provider_id = ?9 AND effect_operation_id = ?10 AND verification_evidence = ?11 AND verified_at_unix_ms = ?12 AND verified_by = ?13 AND operation_id = ?14)",
        params![receipt.id().as_str(), receipt.attempt_id().as_str(), receipt.candidate_id().as_str(), receipt.candidate_digest(), receipt.repository_id().as_str(), receipt.target_ref().as_str(), receipt.prior_revision().as_str(), receipt.result_revision().as_str(), receipt.provider_id().as_str(), receipt.effect_operation_id().as_str(), receipt.verification_evidence().as_str(), receipt.verified_at().value(), receipt.verified_by().as_str(), operation_id],
        |row| row.get(0),
    )?;
    if !matches {
        return Err(StoreError::InvariantViolation(
            "integration receipt row drift",
        ));
    }
    Ok(())
}

fn replay_receipt(
    connection: &Connection,
    attempt: &IntegrationAttempt,
    operation_id: &str,
) -> Result<IntegrationReceipt, StoreError> {
    let event = event_for_operation(connection, operation_id)?;
    let mut replay = load_attempt_before_event(connection, attempt, event.expected_version)?;
    let observation = observation_from_event(&replay, &event)?;
    let authority = if replay.state() == IntegrationState::Running {
        replay
            .lease()
            .map(|lease| (lease.id().clone(), lease.holder().clone()))
    } else {
        None
    };
    Ok(replay.succeed(
        IntegrationVersion::new(event.expected_version)?,
        IntegrationReceiptId::new(required(event.receipt_id.as_deref(), "receipt ID")?.to_owned())?,
        authority
            .as_ref()
            .map(|(lease_id, holder)| (lease_id, holder)),
        &observation,
        UnixMillis::new(event.occurred_at)?,
        ActorId::new(event.actor_id)?,
    )?)
}

fn load_attempt_before_event(
    connection: &Connection,
    final_attempt: &IntegrationAttempt,
    version: i64,
) -> Result<IntegrationAttempt, StoreError> {
    let id = final_attempt.id();
    let row = connection.query_row("SELECT candidate_id, candidate_digest, repository_id, target_ref, expected_target_revision, provider_id, strategy, effect_operation_id, policy_evidence, capability_evidence, planned_observed_revision, planned_observation_evidence, created_at_unix_ms, created_by FROM integration_attempts WHERE integration_id = ?1", [id.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?, row.get::<_, String>(7)?, row.get::<_, String>(8)?, row.get::<_, String>(9)?, row.get::<_, String>(10)?, row.get::<_, String>(11)?, row.get::<_, i64>(12)?, row.get::<_, String>(13)?)))?;
    let intent = IntegrationIntent::new(
        IntegrationBinding::new(
            CandidateId::new(row.0)?,
            row.1,
            load_inputs(connection, id)?,
        )?,
        IntegrationTarget::new(
            RepositoryId::new(row.2)?,
            TargetRef::new(row.3)?,
            TargetRevision::new(row.4)?,
        ),
        IntegrationMethod::new(
            ProviderId::new(row.5)?,
            IntegrationStrategy::new(row.6)?,
            EffectOperationId::new(row.7)?,
        ),
    );
    let gate = IntegrationGate::new(
        GatePolicyEvidence::new(row.8)?,
        IntegrationCapabilityEvidence::new(row.9)?,
        load_review_refs(connection, id)?,
        load_validation_refs(connection, id)?,
        TargetObservation::new(
            intent.target().target_ref().clone(),
            TargetRevision::new(row.10)?,
            IntegrationEvidence::new(row.11)?,
        ),
    );
    let mut attempt = IntegrationAttempt::plan(
        id.clone(),
        intent,
        gate,
        UnixMillis::new(row.12)?,
        ActorId::new(row.13)?,
    )?;
    for event in load_events(connection, id)?
        .into_iter()
        .skip(1)
        .take_while(|event| event.resulting_version <= version)
    {
        replay_event(connection, &mut attempt, &event)?;
    }
    if attempt.version().value() != version {
        return Err(StoreError::InvariantViolation(
            "operation predecessor version is missing",
        ));
    }
    Ok(attempt)
}

fn load_conflict(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    id: &IntegrationConflictId,
) -> Result<IntegrationConflict, StoreError> {
    let (integration_id, operation_id) = connection
        .query_row(
            "SELECT integration_id, operation_id FROM integration_conflicts WHERE conflict_id = ?1",
            [id.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or(StoreError::InvariantViolation(
            "integration conflict is missing",
        ))?;
    let attempt = load_attempt(
        connection,
        artifact_store,
        &IntegrationId::new(integration_id)?,
    )?;
    let conflict = replay_conflict(connection, &attempt, &operation_id)?;
    verify_conflict_row(connection, &conflict, &operation_id)?;
    Ok(conflict)
}

fn validate_resolution(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    resolution: &ConflictResolution,
) -> Result<(), StoreError> {
    let conflict = load_conflict(connection, artifact_store, resolution.conflict_id())?;
    if resolution.resolved_at() < conflict.created_at() {
        return Err(StoreError::InvariantViolation(
            "conflict resolution precedes conflict",
        ));
    }
    super::verify_exact_target(connection, artifact_store, resolution.resulting_target())?;
    if !exact_target_is_current(connection, artifact_store, resolution.resulting_target())? {
        return Err(StoreError::IntegrationGateRejected(
            "conflict resolution target is stale",
        ));
    }
    for id in resolution.validation_refs() {
        let result = super::load_validation_result_internal(connection, artifact_store, id)?;
        if result.outcome() != ValidationOutcome::Passed
            || result.target() != resolution.resulting_target()
            || !exact_target_is_current(connection, artifact_store, result.target())?
        {
            return Err(StoreError::IntegrationGateRejected(
                "conflict resolution validation is not exact, passed, and current",
            ));
        }
    }
    Ok(())
}

fn insert_resolution(
    transaction: &Transaction<'_>,
    resolution: &ConflictResolution,
    context: &MutationContext,
) -> Result<(), StoreError> {
    let target = super::target_columns(resolution.resulting_target());
    transaction.execute("INSERT INTO conflict_resolutions (resolution_id, conflict_id, target_kind, change_id, revision_id, candidate_id, repository_id, context_object_id, content_digest, validation_ref_count, provider_evidence, resolved_at_unix_ms, resolved_by, operation_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)", params![resolution.id().as_str(), resolution.conflict_id().as_str(), target.kind, target.change_id, target.revision_id, target.candidate_id, target.repository_id, target.context_object_id, target.digest, usize_to_i64(resolution.validation_refs().len())?, resolution.provider_evidence().as_str(), resolution.resolved_at().value(), resolution.resolved_by().as_str(), context.operation_id()])?;
    for (position, id) in resolution.validation_refs().iter().enumerate() {
        transaction.execute("INSERT INTO conflict_resolution_validation_refs (resolution_id, ref_position, validation_result_id) VALUES (?1, ?2, ?3)", params![resolution.id().as_str(), usize_to_i64(position)?, id.as_str()])?;
    }
    Ok(())
}

fn load_resolution(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    id: &ConflictResolutionId,
) -> Result<ConflictResolution, StoreError> {
    let row = connection.query_row("SELECT conflict_id, target_kind, change_id, revision_id, candidate_id, repository_id, context_object_id, content_digest, validation_ref_count, provider_evidence, resolved_at_unix_ms, resolved_by, operation_id FROM conflict_resolutions WHERE resolution_id = ?1", [id.as_str()], |row| Ok((row.get::<_, String>(0)?, super::StoredTarget { kind: row.get(1)?, change_id: row.get(2)?, revision_id: row.get(3)?, candidate_id: row.get(4)?, repository_id: row.get(5)?, context_object_id: row.get(6)?, digest: row.get(7)? }, row.get::<_, i64>(8)?, row.get::<_, String>(9)?, row.get::<_, i64>(10)?, row.get::<_, String>(11)?, row.get::<_, String>(12)?))).optional()?.ok_or(StoreError::InvariantViolation("conflict resolution is missing"))?;
    let conflict_id = IntegrationConflictId::new(row.0)?;
    let conflict = load_conflict(connection, artifact_store, &conflict_id)?;
    let target = row.1.into_domain()?;
    super::verify_exact_target(connection, artifact_store, &target)?;
    let mut statement = connection.prepare("SELECT validation_result_id FROM conflict_resolution_validation_refs WHERE resolution_id = ?1 ORDER BY ref_position")?;
    let validations = statement
        .query_map([id.as_str()], |value| value.get::<_, String>(0))?
        .map(|value| Ok(ValidationResultId::new(value?)?))
        .collect::<Result<Vec<_>, StoreError>>()?;
    if usize_to_i64(validations.len())? != row.2 {
        return Err(StoreError::InvariantViolation(
            "conflict resolution validation count drift",
        ));
    }
    for validation_id in &validations {
        let result =
            super::load_validation_result_internal(connection, artifact_store, validation_id)?;
        if result.outcome() != ValidationOutcome::Passed || result.target() != &target {
            return Err(StoreError::InvariantViolation(
                "conflict resolution validation drift",
            ));
        }
    }
    let operation = connection.query_row("SELECT event_kind, actor_id, occurred_at_unix_ms FROM operation_records WHERE operation_id = ?1", [&row.6], |value| Ok((value.get::<_, String>(0)?, value.get::<_, String>(1)?, value.get::<_, i64>(2)?))).optional()?.ok_or(StoreError::InvariantViolation("conflict resolution operation is missing"))?;
    if operation
        != (
            "integration.conflict_resolved".to_owned(),
            row.5.clone(),
            row.4,
        )
    {
        return Err(StoreError::InvariantViolation(
            "conflict resolution operation drift",
        ));
    }
    Ok(ConflictResolution::new(
        id.clone(),
        &conflict,
        target,
        validations,
        IntegrationEvidence::new(row.3)?,
        UnixMillis::new(row.4)?,
        ActorId::new(row.5)?,
    )?)
}
