//! Transactional `SQLite` persistence for Weft domain state.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};
use weft_artifact::{ArtifactStore, ArtifactStoreError};
use weft_domain::{
    ActorId, ArtifactRef, Assignment, AssignmentId, AssignmentRole, BaseState, CandidateId,
    CandidateInput, CandidateStackRef, Change, ChangeError, ChangeId, CompositionCandidate,
    CompositionError, CoordinationError, CoordinationVersion, Dependency, DependencyFreshness,
    DependencyId, DependencyPins, ExactTarget, IntegrationError, IntegrationId, Lease, LeaseId,
    LeaseOperation, LeaseScope, Materialization, MaterializationError, MaterializationId,
    MaterializationPlacement, MaterializationState, MaterializationVersion, NewRevision,
    ProviderEvidence, ProviderId, ProviderObservation, ProviderRef, Relationship,
    RelationshipEndpoints, RelationshipError, RelationshipId, RelationshipKind,
    RelationshipVersion, RepositoryId, ResolvedRequirement, ResolvedRequirementSource, ReviewError,
    ReviewOutcome, ReviewRequest, ReviewRequestId, ReviewReusePolicy, ReviewSubmission,
    ReviewSubmissionId, RevisionId, Stack, StackDefinition, StackId, StackMember, StackPolicy,
    StackVersion, Subject, SubjectId, SubjectKind, UnixMillis, ValidationEnvironment,
    ValidationExecutionId, ValidationObservation, ValidationOutcome, ValidationResult,
    ValidationResultId, ValidationScope, ValidationType, WorkspaceId,
};

const SCHEMA_VERSION: i64 = 7;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const BUSY_RETRY_INTERVAL: Duration = Duration::from_millis(10);

const MIGRATION_1: &str = include_str!("../migrations/0001_initial.sql");
const MIGRATION_2: &str = include_str!("../migrations/0002_coordination.sql");
const MIGRATION_3: &str = include_str!("../migrations/0003_materializations.sql");
const MIGRATION_4: &str = include_str!("../migrations/0004_relationships.sql");
const MIGRATION_5: &str = include_str!("../migrations/0005_composition.sql");
const MIGRATION_6: &str = include_str!("../migrations/0006_reviews_validations.sql");
const MIGRATION_7: &str = include_str!("../migrations/0007_integrations.sql");

mod integration;
pub use integration::{
    ConflictReport, LeaseRenewal, ReconciliationRecord, ReconciliationStart, SuccessVerification,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationContext {
    operation_id: String,
    actor: ActorId,
    occurred_at: UnixMillis,
}

impl MutationContext {
    /// Creates the durable context attached to one mutation and its audit event.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidOperationId`] when the operation ID is empty.
    pub fn new(
        operation_id: impl Into<String>,
        actor: ActorId,
        occurred_at: UnixMillis,
    ) -> Result<Self, StoreError> {
        let operation_id = operation_id.into();
        if operation_id.trim().is_empty() {
            return Err(StoreError::InvalidOperationId);
        }
        Ok(Self {
            operation_id,
            actor,
            occurred_at,
        })
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEvent {
    pub event_id: i64,
    pub event_kind: String,
    pub change_id: ChangeId,
    pub revision_id: Option<RevisionId>,
    pub expected_head_revision_id: Option<RevisionId>,
    pub resulting_head_revision_id: Option<RevisionId>,
    pub operation_id: String,
    pub actor: ActorId,
    pub occurred_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignmentEvent {
    pub event_id: i64,
    pub event_kind: String,
    pub assignment_id: AssignmentId,
    pub change_id: ChangeId,
    pub expected_version: CoordinationVersion,
    pub resulting_version: CoordinationVersion,
    pub operation_id: String,
    pub actor: ActorId,
    pub occurred_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseEvent {
    pub event_id: i64,
    pub event_kind: String,
    pub lease_id: LeaseId,
    pub scope: LeaseScope,
    pub expected_version: CoordinationVersion,
    pub resulting_version: CoordinationVersion,
    pub resulting_expires_at: Option<UnixMillis>,
    pub operation_id: String,
    pub actor: ActorId,
    pub occurred_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializationEvent {
    pub event_id: i64,
    pub event_kind: String,
    pub materialization_id: MaterializationId,
    pub change_id: ChangeId,
    pub revision_id: RevisionId,
    pub expected_version: MaterializationVersion,
    pub resulting_version: MaterializationVersion,
    pub resulting_state: MaterializationState,
    pub resulting_provider_ref: ProviderRef,
    pub provider_evidence: ProviderEvidence,
    pub operation_id: String,
    pub actor: ActorId,
    pub occurred_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationshipEvent {
    pub event_id: i64,
    pub event_kind: String,
    pub relationship_id: RelationshipId,
    pub relationship_kind: RelationshipKind,
    pub endpoints: RelationshipEndpoints,
    pub expected_version: RelationshipVersion,
    pub resulting_version: RelationshipVersion,
    pub operation_id: String,
    pub actor: ActorId,
    pub occurred_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyEvent {
    pub event_id: i64,
    pub event_kind: String,
    pub dependency_id: DependencyId,
    pub downstream_change_id: ChangeId,
    pub upstream_change_id: ChangeId,
    pub expected_version: RelationshipVersion,
    pub resulting_version: RelationshipVersion,
    pub resulting_pins: DependencyPins,
    pub operation_id: String,
    pub actor: ActorId,
    pub occurred_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateSelection {
    Changes(Vec<ChangeId>),
    Stack {
        stack_id: StackId,
        expected_version: StackVersion,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateFreshness {
    pub advanced_inputs: Vec<ChangeId>,
    pub changed_dependencies: Vec<DependencyId>,
    pub stack_changed: bool,
}

impl CandidateFreshness {
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.advanced_inputs.is_empty()
            && self.changed_dependencies.is_empty()
            && !self.stack_changed
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExactTargetFreshness {
    Current,
    RevisionAdvanced,
    CandidateStale(CandidateFreshness),
}

impl ExactTargetFreshness {
    #[must_use]
    pub const fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }
}

pub struct SqliteStore {
    connection: Connection,
}

impl SqliteStore {
    /// Opens and migrates a Weft metadata database.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `SQLite` cannot open/configure the database or
    /// when its schema is newer than this binary understands.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        let journal_mode = enable_wal(&connection)?;
        if journal_mode != "wal" {
            return Err(StoreError::UnsupportedJournalMode(journal_mode));
        }

        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        match version {
            0..=6 => {
                begin_immediate_with_retry(&connection)?;
                let locked_version: i64 =
                    connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
                let migration = match locked_version {
                    0 => connection
                        .execute_batch(MIGRATION_1)
                        .and_then(|()| connection.execute_batch(MIGRATION_2))
                        .and_then(|()| connection.execute_batch(MIGRATION_3))
                        .and_then(|()| connection.execute_batch(MIGRATION_4))
                        .and_then(|()| connection.execute_batch(MIGRATION_5))
                        .and_then(|()| connection.execute_batch(MIGRATION_6))
                        .and_then(|()| connection.execute_batch(MIGRATION_7)),
                    1 => connection
                        .execute_batch(MIGRATION_2)
                        .and_then(|()| connection.execute_batch(MIGRATION_3))
                        .and_then(|()| connection.execute_batch(MIGRATION_4))
                        .and_then(|()| connection.execute_batch(MIGRATION_5))
                        .and_then(|()| connection.execute_batch(MIGRATION_6))
                        .and_then(|()| connection.execute_batch(MIGRATION_7)),
                    2 => connection
                        .execute_batch(MIGRATION_3)
                        .and_then(|()| connection.execute_batch(MIGRATION_4))
                        .and_then(|()| connection.execute_batch(MIGRATION_5))
                        .and_then(|()| connection.execute_batch(MIGRATION_6))
                        .and_then(|()| connection.execute_batch(MIGRATION_7)),
                    3 => connection
                        .execute_batch(MIGRATION_4)
                        .and_then(|()| connection.execute_batch(MIGRATION_5))
                        .and_then(|()| connection.execute_batch(MIGRATION_6))
                        .and_then(|()| connection.execute_batch(MIGRATION_7)),
                    4 => connection
                        .execute_batch(MIGRATION_5)
                        .and_then(|()| connection.execute_batch(MIGRATION_6))
                        .and_then(|()| connection.execute_batch(MIGRATION_7)),
                    5 => connection
                        .execute_batch(MIGRATION_6)
                        .and_then(|()| connection.execute_batch(MIGRATION_7)),
                    6 => connection.execute_batch(MIGRATION_7),
                    SCHEMA_VERSION => Ok(()),
                    other => {
                        connection.execute_batch("ROLLBACK")?;
                        return Err(StoreError::UnsupportedSchemaVersion(other));
                    }
                };
                if let Err(error) = migration {
                    let _ = connection.execute_batch("ROLLBACK");
                    return Err(error.into());
                }
                connection.execute_batch("COMMIT")?;
            }
            SCHEMA_VERSION => {}
            other => return Err(StoreError::UnsupportedSchemaVersion(other)),
        }

        Ok(Self { connection })
    }

    #[must_use]
    pub const fn schema_version() -> i64 {
        SCHEMA_VERSION
    }

    /// Creates a durable Change and its audit event atomically.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for duplicate identities or operation IDs and for
    /// database failures. No partial row is committed on failure.
    pub fn create_change(
        &mut self,
        change_id: &ChangeId,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if operation_is_replay(
            &transaction,
            "change.created",
            change_id,
            None,
            None,
            None,
            context,
        )? {
            return Ok(());
        }
        if change_exists(&transaction, change_id)? {
            return Err(StoreError::ChangeAlreadyExists(change_id.clone()));
        }
        transaction.execute(
            "INSERT INTO changes (change_id) VALUES (?1)",
            [change_id.as_str()],
        )?;
        insert_audit_event(
            &transaction,
            "change.created",
            change_id,
            None,
            None,
            None,
            context,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Appends an immutable revision when `expected_head` still matches.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::StaleHead`] without mutation when another writer
    /// advanced the Change. Identity conflicts and database errors also roll back
    /// the entire transaction, including its audit event.
    pub fn append_revision(
        &mut self,
        artifact_store: &ArtifactStore,
        change_id: &ChangeId,
        expected_head: Option<&RevisionId>,
        revision: &NewRevision,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        let artifact = artifact_store.load_manifest(revision.artifact())?;
        if artifact.base() != revision.base() {
            return Err(StoreError::ArtifactBaseMismatch);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if operation_is_replay(
            &transaction,
            "revision.appended",
            change_id,
            Some(revision),
            expected_head,
            Some(revision.revision_id()),
            context,
        )? {
            return Ok(());
        }
        let StoredHead::Found(actual_head) = read_head(&transaction, change_id)? else {
            return Err(StoreError::ChangeNotFound(change_id.clone()));
        };
        if actual_head.as_ref() != expected_head {
            return Err(StoreError::StaleHead {
                expected: expected_head.cloned(),
                actual: actual_head,
            });
        }
        if revision_exists(&transaction, revision.revision_id())? {
            return Err(StoreError::DuplicateRevision(
                revision.revision_id().clone(),
            ));
        }

        let sequence: i64 = transaction.query_row(
            "SELECT count(*) FROM change_revisions WHERE change_id = ?1",
            [change_id.as_str()],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO change_revisions (
                revision_id, change_id, sequence, parent_revision_id,
                repository_id, base_object_id, artifact_version, artifact_digest,
                created_at_unix_ms, created_by
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                revision.revision_id().as_str(),
                change_id.as_str(),
                sequence,
                expected_head.map(RevisionId::as_str),
                revision.base().repository_id().as_str(),
                revision.base().object_id(),
                revision.artifact().version(),
                revision.artifact().manifest_digest(),
                revision.created_at().value(),
                revision.created_by().as_str(),
            ],
        )?;
        let updated = transaction.execute(
            "UPDATE changes SET head_revision_id = ?1
             WHERE change_id = ?2
               AND ((head_revision_id IS NULL AND ?3 IS NULL) OR head_revision_id = ?3)",
            params![
                revision.revision_id().as_str(),
                change_id.as_str(),
                expected_head.map(RevisionId::as_str),
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::InvariantViolation(
                "head compare-and-swap updated an unexpected number of rows",
            ));
        }
        insert_audit_event(
            &transaction,
            "revision.appended",
            change_id,
            Some(revision),
            expected_head,
            Some(revision.revision_id()),
            context,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Rehydrates a Change through domain constructors, revalidating persisted
    /// identity, provenance, ancestry and head invariants, then verifying every
    /// canonical manifest/blob and its exact base binding.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the Change is absent or durable rows no longer
    /// satisfy the domain model.
    pub fn load_change(
        &self,
        artifact_store: &ArtifactStore,
        change_id: &ChangeId,
    ) -> Result<Change, StoreError> {
        load_change_from_connection(&self.connection, artifact_store, change_id)
    }

    /// Lists immutable audit events for one Change in commit order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when rows cannot be read or validated.
    pub fn audit_events(&self, change_id: &ChangeId) -> Result<Vec<AuditEvent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT event_id, event_kind, revision_id, expected_head_revision_id,
                    resulting_head_revision_id, operation_id, actor_id, occurred_at_unix_ms
             FROM audit_events WHERE change_id = ?1 ORDER BY event_id",
        )?;
        let rows = statement.query_map([change_id.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (event_id, event_kind, revision, expected, resulting, operation_id, actor, at) =
                row?;
            Ok(AuditEvent {
                event_id,
                event_kind,
                change_id: change_id.clone(),
                revision_id: optional_revision_id(revision)?,
                expected_head_revision_id: optional_revision_id(expected)?,
                resulting_head_revision_id: optional_revision_id(resulting)?,
                operation_id,
                actor: ActorId::new(actor)?,
                occurred_at: UnixMillis::new(at)?,
            })
        })
        .collect()
    }

    /// Records a new overlapping assignment and its immutable event.
    ///
    /// # Errors
    ///
    /// The Change and assignment identity must be new, the assignment must be
    /// active at version one, and its provenance must match the mutation context.
    pub fn create_assignment(
        &mut self,
        assignment: &Assignment,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        if assignment.version() != CoordinationVersion::INITIAL
            || !assignment.is_active()
            || assignment.assigned_at() != context.occurred_at
            || assignment.assigned_by() != &context.actor
        {
            return Err(StoreError::InvariantViolation(
                "new assignment must be active at version one with matching provenance",
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if assignment_operation_is_replay(
            &transaction,
            "assignment.assigned",
            assignment.id(),
            CoordinationVersion::EMPTY,
            Some(assignment),
            context,
        )? {
            return Ok(());
        }
        if !change_exists(&transaction, assignment.change_id())? {
            return Err(StoreError::ChangeNotFound(assignment.change_id().clone()));
        }
        transaction.execute(
            "INSERT INTO assignments (
                assignment_id, change_id, subject_kind, subject_id, role,
                assigned_at_unix_ms, assigned_by, version
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)",
            params![
                assignment.id().as_str(),
                assignment.change_id().as_str(),
                assignment.subject().kind().as_str(),
                assignment.subject().id().as_str(),
                assignment.role().as_str(),
                assignment.assigned_at().value(),
                assignment.assigned_by().as_str(),
            ],
        )?;
        insert_operation_record(&transaction, "assignment.assigned", context)?;
        insert_assignment_event(
            &transaction,
            "assignment.assigned",
            assignment,
            CoordinationVersion::EMPTY,
            context,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Releases one assignment with optimistic concurrency while retaining its history.
    ///
    /// # Errors
    ///
    /// Returns a not-found, stale-version, already-released, timestamp, operation,
    /// or database error without committing a partial transition.
    pub fn release_assignment(
        &mut self,
        assignment_id: &AssignmentId,
        expected_version: CoordinationVersion,
        context: &MutationContext,
    ) -> Result<Assignment, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if assignment_operation_is_replay(
            &transaction,
            "assignment.released",
            assignment_id,
            expected_version,
            None,
            context,
        )? {
            return load_assignment(&transaction, assignment_id);
        }
        let mut assignment = load_assignment(&transaction, assignment_id)?;
        assignment.release(expected_version, context.occurred_at, context.actor.clone())?;
        let updated = transaction.execute(
            "UPDATE assignments
             SET version = ?1, released_at_unix_ms = ?2, released_by = ?3
             WHERE assignment_id = ?4 AND version = ?5 AND released_at_unix_ms IS NULL",
            params![
                assignment.version().value(),
                assignment.released_at().map(UnixMillis::value),
                assignment.released_by().map(ActorId::as_str),
                assignment_id.as_str(),
                expected_version.value(),
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::InvariantViolation(
                "assignment release compare-and-swap updated an unexpected number of rows",
            ));
        }
        insert_operation_record(&transaction, "assignment.released", context)?;
        insert_assignment_event(
            &transaction,
            "assignment.released",
            &assignment,
            expected_version,
            context,
        )?;
        transaction.commit()?;
        Ok(assignment)
    }

    /// Lists every current and released assignment for a Change.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when durable rows fail database or domain validation.
    pub fn assignments(&self, change_id: &ChangeId) -> Result<Vec<Assignment>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT assignment_id FROM assignments WHERE change_id = ?1 ORDER BY assigned_at_unix_ms, assignment_id",
        )?;
        let ids = statement
            .query_map([change_id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| load_assignment(&self.connection, &AssignmentId::new(id)?))
            .collect()
    }

    /// Lists immutable assignment events for one Change.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when durable event values are invalid.
    pub fn assignment_events(
        &self,
        change_id: &ChangeId,
    ) -> Result<Vec<AssignmentEvent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT event.event_id, event.event_kind, event.assignment_id,
                    event.expected_version, event.resulting_version, event.operation_id,
                    operation.actor_id, operation.occurred_at_unix_ms
             FROM assignment_events AS event
             JOIN operation_records AS operation USING (operation_id)
             WHERE event.change_id = ?1 ORDER BY event.event_id",
        )?;
        let rows = statement.query_map([change_id.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (event_id, kind, assignment, expected, resulting, operation, actor, at) = row?;
            Ok(AssignmentEvent {
                event_id,
                event_kind: kind,
                assignment_id: AssignmentId::new(assignment)?,
                change_id: change_id.clone(),
                expected_version: CoordinationVersion::new(expected)?,
                resulting_version: CoordinationVersion::new(resulting)?,
                operation_id: operation,
                actor: ActorId::new(actor)?,
                occurred_at: UnixMillis::new(at)?,
            })
        })
        .collect()
    }

    /// Acquires an unheld or expired operation scope using a version compare-and-swap.
    ///
    /// # Errors
    ///
    /// Active scopes reject competing holders. Stale versions, invalid expiry,
    /// duplicate lease identity, operation conflicts, and database errors fail atomically.
    pub fn acquire_lease(
        &mut self,
        lease_id: &LeaseId,
        scope: &LeaseScope,
        holder: &Subject,
        expected_version: CoordinationVersion,
        expires_at: UnixMillis,
        context: &MutationContext,
    ) -> Result<Lease, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if lease_operation_is_replay(
            &transaction,
            &["lease.acquired", "lease.reclaimed"],
            lease_id,
            scope,
            expected_version,
            Some(holder),
            Some(expires_at),
            context,
        )? {
            return load_lease_operation_outcome(&transaction, context.operation_id());
        }
        if !change_exists(&transaction, scope.change_id())? {
            return Err(StoreError::ChangeNotFound(scope.change_id().clone()));
        }
        let state = read_lease_scope(&transaction, scope)?.unwrap_or(LeaseScopeState {
            version: CoordinationVersion::EMPTY,
            current_lease_id: None,
            current_expires_at: None,
        });
        if state.version != expected_version {
            return Err(StoreError::StaleCoordinationVersion {
                expected: expected_version,
                actual: state.version,
            });
        }
        let predecessor = match (&state.current_lease_id, state.current_expires_at) {
            (Some(current), Some(expiry)) if context.occurred_at < expiry => {
                return Err(StoreError::LeaseHeld {
                    lease_id: current.clone(),
                    expires_at: expiry,
                });
            }
            (Some(current), Some(_)) => Some(current.clone()),
            (None, None) => None,
            _ => {
                return Err(StoreError::InvalidStoredData(
                    "lease scope has inconsistent current identity and expiry".to_owned(),
                ));
            }
        };
        let resulting_version = next_version(expected_version)?;
        let lease = Lease::new(
            lease_id.clone(),
            scope.clone(),
            holder.clone(),
            predecessor.clone(),
            context.occurred_at,
            expires_at,
            resulting_version,
        )?;
        if read_lease_identity(&transaction, lease_id)? {
            return Err(StoreError::DuplicateLease(lease_id.clone()));
        }
        persist_lease_acquisition(&transaction, &lease, expected_version, context)?;
        transaction.commit()?;
        Ok(lease)
    }

    /// Renews the exact current lease and scope version.
    ///
    /// # Errors
    ///
    /// Lost, stale, expired, non-extending, conflicting, and database mutations fail atomically.
    pub fn renew_lease(
        &mut self,
        lease_id: &LeaseId,
        expected_version: CoordinationVersion,
        expires_at: UnixMillis,
        context: &MutationContext,
    ) -> Result<Lease, StoreError> {
        self.update_lease(
            "lease.renewed",
            lease_id,
            expected_version,
            Some(expires_at),
            context,
        )
    }

    /// Releases the exact current lease and scope version.
    ///
    /// # Errors
    ///
    /// Lost, stale, expired, conflicting, and database mutations fail atomically.
    pub fn release_lease(
        &mut self,
        lease_id: &LeaseId,
        expected_version: CoordinationVersion,
        context: &MutationContext,
    ) -> Result<Lease, StoreError> {
        self.update_lease("lease.released", lease_id, expected_version, None, context)
    }

    fn update_lease(
        &mut self,
        event_kind: &str,
        lease_id: &LeaseId,
        expected_version: CoordinationVersion,
        expires_at: Option<UnixMillis>,
        context: &MutationContext,
    ) -> Result<Lease, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored = load_lease(&transaction, lease_id)?;
        if lease_operation_is_replay(
            &transaction,
            &[event_kind],
            lease_id,
            stored.scope(),
            expected_version,
            None,
            expires_at,
            context,
        )? {
            return load_lease_operation_outcome(&transaction, context.operation_id());
        }
        let state = read_lease_scope(&transaction, stored.scope())?.ok_or_else(|| {
            StoreError::InvalidStoredData("lease has no scope projection".to_owned())
        })?;
        if state.version != expected_version {
            return Err(StoreError::StaleCoordinationVersion {
                expected: expected_version,
                actual: state.version,
            });
        }
        if state.current_lease_id.as_ref() != Some(lease_id) {
            return Err(StoreError::LeaseNotCurrent(lease_id.clone()));
        }
        let mut lease = stored;
        match expires_at {
            Some(value) => lease.renew(expected_version, context.occurred_at, value)?,
            None => lease.release(expected_version, context.occurred_at)?,
        }
        let updated = transaction.execute(
            "UPDATE lease_scopes
             SET version = ?1, current_lease_id = ?2, current_expires_at_unix_ms = ?3
             WHERE change_id = ?4 AND operation_key = ?5 AND version = ?6
               AND current_lease_id = ?7",
            params![
                lease.version().value(),
                if expires_at.is_some() {
                    Some(lease_id.as_str())
                } else {
                    None
                },
                expires_at.map(UnixMillis::value),
                lease.scope().change_id().as_str(),
                lease.scope().operation().as_str(),
                expected_version.value(),
                lease_id.as_str(),
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::InvariantViolation(
                "lease update compare-and-swap updated an unexpected number of rows",
            ));
        }
        insert_operation_record(&transaction, event_kind, context)?;
        insert_lease_event(
            &transaction,
            event_kind,
            &lease,
            expected_version,
            expires_at,
            context,
        )?;
        transaction.commit()?;
        Ok(lease)
    }

    /// Returns the current lease projection for a scope, including expired leases.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when durable lease state is inconsistent.
    pub fn current_lease(&self, scope: &LeaseScope) -> Result<Option<Lease>, StoreError> {
        let Some(state) = read_lease_scope(&self.connection, scope)? else {
            return Ok(None);
        };
        let final_event_lease = validate_lease_scope_events(&self.connection, scope, &state)?;
        let Some(current_id) = state.current_lease_id.as_ref() else {
            if state.current_expires_at.is_some() {
                return Err(StoreError::InvalidStoredData(
                    "unheld lease scope retains an expiry".to_owned(),
                ));
            }
            if let Some(final_id) = final_event_lease {
                let released = load_lease(&self.connection, &final_id)?;
                if released.scope() != scope
                    || released.version() != state.version
                    || released.released_at().is_none()
                {
                    return Err(StoreError::InvalidStoredData(
                        "empty lease scope does not match its final release event".to_owned(),
                    ));
                }
            }
            return Ok(None);
        };
        if final_event_lease.as_ref() != Some(current_id) {
            return Err(StoreError::InvalidStoredData(
                "current lease identity does not match the final scope event".to_owned(),
            ));
        }
        let lease = load_lease(&self.connection, current_id)?;
        if lease.scope() != scope
            || lease.version() != state.version
            || state.current_expires_at != Some(lease.expires_at())
            || lease.released_at().is_some()
        {
            return Err(StoreError::InvalidStoredData(
                "lease scope projection does not match its immutable event history".to_owned(),
            ));
        }
        Ok(Some(lease))
    }

    /// Lists immutable lease events for one Change.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when durable event values are invalid.
    pub fn lease_events(&self, change_id: &ChangeId) -> Result<Vec<LeaseEvent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT event.event_id, event.event_kind, event.lease_id,
                    event.operation_key, event.expected_version, event.resulting_version,
                    event.resulting_expires_at_unix_ms, event.operation_id,
                    operation.actor_id, operation.occurred_at_unix_ms
             FROM lease_events AS event
             JOIN operation_records AS operation USING (operation_id)
             WHERE event.change_id = ?1 ORDER BY event.event_id",
        )?;
        let rows = statement.query_map([change_id.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i64>(9)?,
            ))
        })?;
        rows.map(|row| {
            let (
                event_id,
                kind,
                lease,
                operation_key,
                expected,
                resulting,
                expiry,
                operation_id,
                actor,
                at,
            ) = row?;
            Ok(LeaseEvent {
                event_id,
                event_kind: kind,
                lease_id: LeaseId::new(lease)?,
                scope: LeaseScope::new(change_id.clone(), LeaseOperation::new(operation_key)?),
                expected_version: CoordinationVersion::new(expected)?,
                resulting_version: CoordinationVersion::new(resulting)?,
                resulting_expires_at: expiry.map(UnixMillis::new).transpose()?,
                operation_id,
                actor: ActorId::new(actor)?,
                occurred_at: UnixMillis::new(at)?,
            })
        })
        .collect()
    }

    /// Records one verified clean realization of an exact durable revision.
    ///
    /// # Errors
    ///
    /// The revision and canonical content must exist, creation provenance must
    /// match the operation, and identity/active-placement conflicts fail atomically.
    pub fn create_materialization(
        &mut self,
        artifact_store: &ArtifactStore,
        materialization: &Materialization,
        provider_evidence: &ProviderEvidence,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        if materialization.version() != MaterializationVersion::INITIAL
            || materialization.state() != MaterializationState::Clean
            || materialization.created_at() != context.occurred_at
            || materialization.created_by() != &context.actor
        {
            return Err(StoreError::InvariantViolation(
                "new materialization must be clean at version one with matching provenance",
            ));
        }
        verify_exact_revision(
            self,
            artifact_store,
            materialization.change_id(),
            materialization.revision_id(),
        )?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let request = MaterializationMutationRequest {
            event_kind: "materialization.created",
            materialization_id: materialization.id(),
            expected_version: MaterializationVersion::EMPTY,
            state: materialization.state(),
            provider_ref: materialization.provider_ref(),
            provider_evidence,
            creation: Some(materialization),
        };
        if materialization_operation_is_replay(&transaction, &request, context)? {
            return Ok(());
        }
        if materialization_identity_exists(&transaction, materialization.id())? {
            return Err(StoreError::DuplicateMaterialization(
                materialization.id().clone(),
            ));
        }
        transaction.execute(
            "INSERT INTO materializations (
                materialization_id, change_id, revision_id, workspace_id, provider_id,
                current_provider_ref, state, version, created_at_unix_ms, created_by,
                state_changed_at_unix_ms, released_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?8, NULL)",
            params![
                materialization.id().as_str(),
                materialization.change_id().as_str(),
                materialization.revision_id().as_str(),
                materialization.workspace_id().as_str(),
                materialization.provider_id().as_str(),
                materialization.provider_ref().as_str(),
                materialization.state().as_str(),
                materialization.created_at().value(),
                materialization.created_by().as_str(),
            ],
        )?;
        insert_operation_record(&transaction, "materialization.created", context)?;
        insert_materialization_event(
            &transaction,
            "materialization.created",
            materialization,
            MaterializationVersion::EMPTY,
            provider_evidence,
            context,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Advances only the observed state/provider reference of one exact Materialization.
    ///
    /// # Errors
    ///
    /// Missing canonical content, stale versions, terminal/no-op transitions,
    /// operation conflicts, and database failures commit no partial transition.
    pub fn transition_materialization(
        &mut self,
        artifact_store: &ArtifactStore,
        materialization_id: &MaterializationId,
        expected_version: MaterializationVersion,
        observation: ProviderObservation,
        context: &MutationContext,
    ) -> Result<Materialization, StoreError> {
        verify_materialization_content(self, artifact_store, materialization_id)?;
        let (state, provider_ref, provider_evidence) = observation.into_parts();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let request = MaterializationMutationRequest {
            event_kind: "materialization.transitioned",
            materialization_id,
            expected_version,
            state,
            provider_ref: &provider_ref,
            provider_evidence: &provider_evidence,
            creation: None,
        };
        if materialization_operation_is_replay(&transaction, &request, context)? {
            return load_materialization_operation_outcome(&transaction, context.operation_id());
        }
        let mut materialization =
            load_materialization_internal(&transaction, materialization_id, None, true)?;
        materialization.transition(expected_version, state, provider_ref, context.occurred_at)?;
        let updated = transaction.execute(
            "UPDATE materializations
             SET current_provider_ref = ?1, state = ?2, version = ?3,
                 state_changed_at_unix_ms = ?4, released_at_unix_ms = ?5
             WHERE materialization_id = ?6 AND version = ?7",
            params![
                materialization.provider_ref().as_str(),
                materialization.state().as_str(),
                materialization.version().value(),
                materialization.state_changed_at().value(),
                materialization.released_at().map(UnixMillis::value),
                materialization_id.as_str(),
                expected_version.value(),
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::InvariantViolation(
                "materialization compare-and-swap updated an unexpected number of rows",
            ));
        }
        insert_operation_record(&transaction, "materialization.transitioned", context)?;
        insert_materialization_event(
            &transaction,
            "materialization.transitioned",
            &materialization,
            expected_version,
            &provider_evidence,
            context,
        )?;
        transaction.commit()?;
        Ok(materialization)
    }

    /// Loads and fail-closed reconstructs one Materialization and exact revision.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for missing canonical content or projection/event drift.
    pub fn materialization(
        &self,
        artifact_store: &ArtifactStore,
        materialization_id: &MaterializationId,
    ) -> Result<Materialization, StoreError> {
        verify_materialization_content(self, artifact_store, materialization_id)?;
        load_materialization_internal(&self.connection, materialization_id, None, true)
    }

    /// Lists all historical and current Materializations owned by one Change.
    ///
    /// # Errors
    ///
    /// Every result is reconstructed from immutable events and canonical revision content.
    pub fn materializations(
        &self,
        artifact_store: &ArtifactStore,
        change_id: &ChangeId,
    ) -> Result<Vec<Materialization>, StoreError> {
        let change = self.load_change(artifact_store, change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT materialization_id, revision_id FROM materializations
             WHERE change_id = ?1 ORDER BY created_at_unix_ms, materialization_id",
        )?;
        let rows = statement
            .query_map([change_id.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, revision)| {
                let revision = RevisionId::new(revision)?;
                if !change
                    .revisions()
                    .iter()
                    .any(|value| value.revision_id() == &revision)
                {
                    return Err(StoreError::RevisionNotFoundForChange {
                        change_id: change_id.clone(),
                        revision_id: revision,
                    });
                }
                load_materialization_internal(
                    &self.connection,
                    &MaterializationId::new(id)?,
                    None,
                    true,
                )
            })
            .collect()
    }

    /// Lists immutable Materialization events for one Change.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when durable event values are invalid.
    pub fn materialization_events(
        &self,
        change_id: &ChangeId,
    ) -> Result<Vec<MaterializationEvent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT event.event_id, event.event_kind, event.materialization_id,
                    event.revision_id, event.expected_version, event.resulting_version,
                    event.resulting_state, event.resulting_provider_ref,
                    event.provider_evidence, event.operation_id, operation.actor_id,
                    operation.occurred_at_unix_ms
             FROM materialization_events AS event
             JOIN operation_records AS operation USING (operation_id)
             WHERE event.change_id = ?1 ORDER BY event.event_id",
        )?;
        let rows = statement.query_map([change_id.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, i64>(11)?,
            ))
        })?;
        rows.map(|row| {
            let (
                event_id,
                kind,
                materialization,
                revision,
                expected,
                resulting,
                state,
                provider_ref,
                provider_evidence,
                operation,
                actor,
                at,
            ) = row?;
            Ok(MaterializationEvent {
                event_id,
                event_kind: kind,
                materialization_id: MaterializationId::new(materialization)?,
                change_id: change_id.clone(),
                revision_id: RevisionId::new(revision)?,
                expected_version: MaterializationVersion::new(expected)?,
                resulting_version: MaterializationVersion::new(resulting)?,
                resulting_state: MaterializationState::parse(&state)?,
                resulting_provider_ref: ProviderRef::new(provider_ref)?,
                provider_evidence: ProviderEvidence::new(provider_evidence)?,
                operation_id: operation,
                actor: ActorId::new(actor)?,
                occurred_at: UnixMillis::new(at)?,
            })
        })
        .collect()
    }

    /// Creates a symmetric contextual relationship with canonical endpoints.
    ///
    /// # Errors
    ///
    /// Both Changes must exist; identity, active-pair, operation, and provenance
    /// conflicts fail without committing a partial event.
    pub fn create_relationship(
        &mut self,
        relationship: &Relationship,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        validate_new_relationship(relationship, context)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if relationship_operation_is_replay(
            &transaction,
            "relationship.created",
            relationship.id(),
            RelationshipVersion::EMPTY,
            Some(relationship),
            context,
        )? {
            return Ok(());
        }
        if relationship_identity_exists(&transaction, relationship.id())? {
            return Err(StoreError::DuplicateRelationship(relationship.id().clone()));
        }
        ensure_change_exists(&transaction, relationship.endpoints().first())?;
        ensure_change_exists(&transaction, relationship.endpoints().second())?;
        if active_relationship_exists(&transaction, relationship.kind(), relationship.endpoints())?
        {
            return Err(StoreError::ActiveRelationshipExists);
        }
        transaction.execute(
            "INSERT INTO relationships (
                relationship_id, relationship_kind, first_change_id, second_change_id,
                created_at_unix_ms, created_by, version
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
            params![
                relationship.id().as_str(),
                relationship.kind().as_str(),
                relationship.endpoints().first().as_str(),
                relationship.endpoints().second().as_str(),
                relationship.created_at().value(),
                relationship.created_by().as_str(),
            ],
        )?;
        insert_operation_record(&transaction, "relationship.created", context)?;
        insert_relationship_event(
            &transaction,
            "relationship.created",
            relationship,
            RelationshipVersion::EMPTY,
            context,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Removes a relationship through exact-version compare-and-swap.
    ///
    /// # Errors
    ///
    /// Missing, stale, terminal, time-reversing, and operation-conflicting requests fail.
    pub fn remove_relationship(
        &mut self,
        relationship_id: &RelationshipId,
        expected_version: RelationshipVersion,
        context: &MutationContext,
    ) -> Result<Relationship, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if relationship_operation_is_replay(
            &transaction,
            "relationship.removed",
            relationship_id,
            expected_version,
            None,
            context,
        )? {
            return load_relationship(&transaction, relationship_id);
        }
        let mut relationship = load_relationship(&transaction, relationship_id)?;
        relationship.remove(expected_version, context.occurred_at, context.actor.clone())?;
        let updated = transaction.execute(
            "UPDATE relationships
             SET version = ?1, removed_at_unix_ms = ?2, removed_by = ?3
             WHERE relationship_id = ?4 AND version = ?5
               AND removed_at_unix_ms IS NULL",
            params![
                relationship.version().value(),
                relationship.removed_at().map(UnixMillis::value),
                relationship.removed_by().map(ActorId::as_str),
                relationship_id.as_str(),
                expected_version.value(),
            ],
        )?;
        if updated != 1 {
            return Err(StoreError::InvariantViolation(
                "relationship compare-and-swap updated an unexpected number of rows",
            ));
        }
        insert_operation_record(&transaction, "relationship.removed", context)?;
        insert_relationship_event(
            &transaction,
            "relationship.removed",
            &relationship,
            expected_version,
            context,
        )?;
        transaction.commit()?;
        Ok(relationship)
    }

    /// Loads one relationship by reconstructing its immutable event history.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] for absence, invalid values, or projection/event drift.
    pub fn relationship(
        &self,
        relationship_id: &RelationshipId,
    ) -> Result<Relationship, StoreError> {
        load_relationship(&self.connection, relationship_id)
    }

    /// Lists all current and removed contextual relationships touching one Change.
    ///
    /// # Errors
    ///
    /// Every record is reconstructed from its immutable history.
    pub fn relationships(&self, change_id: &ChangeId) -> Result<Vec<Relationship>, StoreError> {
        ensure_change_exists(&self.connection, change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT relationship_id FROM relationships
             WHERE first_change_id = ?1 OR second_change_id = ?1
             ORDER BY created_at_unix_ms, relationship_id",
        )?;
        let ids = statement
            .query_map([change_id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| load_relationship(&self.connection, &RelationshipId::new(id)?))
            .collect()
    }

    /// Lists immutable contextual relationship events touching one Change.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when durable event or projection values are invalid.
    pub fn relationship_events(
        &self,
        change_id: &ChangeId,
    ) -> Result<Vec<RelationshipEvent>, StoreError> {
        self.relationships(change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT event.event_id, event.event_kind, event.relationship_id,
                    relationship.relationship_kind, event.first_change_id,
                    event.second_change_id, event.expected_version,
                    event.resulting_version, event.operation_id,
                    operation.actor_id, operation.occurred_at_unix_ms
             FROM relationship_events AS event
             JOIN relationships AS relationship USING (relationship_id)
             JOIN operation_records AS operation USING (operation_id)
             WHERE event.first_change_id = ?1 OR event.second_change_id = ?1
             ORDER BY event.event_id",
        )?;
        let rows = statement.query_map([change_id.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, i64>(10)?,
            ))
        })?;
        rows.map(|row| {
            let (
                event_id,
                kind,
                id,
                relation_kind,
                first,
                second,
                expected,
                resulting,
                operation,
                actor,
                at,
            ) = row?;
            Ok(RelationshipEvent {
                event_id,
                event_kind: kind,
                relationship_id: RelationshipId::new(id)?,
                relationship_kind: RelationshipKind::parse(&relation_kind)?,
                endpoints: RelationshipEndpoints::new(
                    ChangeId::new(first)?,
                    ChangeId::new(second)?,
                )?,
                expected_version: RelationshipVersion::new(expected)?,
                resulting_version: RelationshipVersion::new(resulting)?,
                operation_id: operation,
                actor: ActorId::new(actor)?,
                occurred_at: UnixMillis::new(at)?,
            })
        })
        .collect()
    }

    /// Creates one active, directed dependency between exact durable revisions.
    ///
    /// # Errors
    ///
    /// Exact pin ownership/content, active uniqueness, cycle, identity, operation,
    /// and provenance checks are atomic at the metadata boundary.
    pub fn create_dependency(
        &mut self,
        artifact_store: &ArtifactStore,
        dependency: &Dependency,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        validate_new_dependency(dependency, context)?;
        verify_dependency_pins(self, artifact_store, dependency)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if dependency_operation_is_replay(
            &transaction,
            "dependency.created",
            dependency.id(),
            RelationshipVersion::EMPTY,
            dependency.pins(),
            Some(dependency),
            context,
        )? {
            return Ok(());
        }
        if dependency_identity_exists(&transaction, dependency.id())? {
            return Err(StoreError::DuplicateDependency(dependency.id().clone()));
        }
        if active_dependency_exists(
            &transaction,
            dependency.downstream_change_id(),
            dependency.upstream_change_id(),
        )? {
            return Err(StoreError::ActiveDependencyExists);
        }
        if dependency_would_cycle(
            &transaction,
            dependency.downstream_change_id(),
            dependency.upstream_change_id(),
        )? {
            return Err(StoreError::DependencyCycle);
        }
        transaction.execute(
            "INSERT INTO dependencies (
                dependency_id, downstream_change_id, upstream_change_id,
                downstream_revision_id, upstream_revision_id,
                created_at_unix_ms, created_by, version,
                updated_at_unix_ms, updated_by
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?6, ?7)",
            params![
                dependency.id().as_str(),
                dependency.downstream_change_id().as_str(),
                dependency.upstream_change_id().as_str(),
                dependency.pins().downstream_revision_id().as_str(),
                dependency.pins().upstream_revision_id().as_str(),
                dependency.created_at().value(),
                dependency.created_by().as_str(),
            ],
        )?;
        insert_operation_record(&transaction, "dependency.created", context)?;
        insert_dependency_event(
            &transaction,
            "dependency.created",
            dependency,
            RelationshipVersion::EMPTY,
            context,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Atomically replaces both exact dependency pins using the expected version.
    ///
    /// # Errors
    ///
    /// Missing content, stale/terminal/no-op requests, and operation conflicts fail.
    pub fn repin_dependency(
        &mut self,
        artifact_store: &ArtifactStore,
        dependency_id: &DependencyId,
        expected_version: RelationshipVersion,
        pins: DependencyPins,
        context: &MutationContext,
    ) -> Result<Dependency, StoreError> {
        let identity = read_dependency_identity(&self.connection, dependency_id)?;
        verify_exact_revision(
            self,
            artifact_store,
            &identity.0,
            pins.downstream_revision_id(),
        )?;
        verify_exact_revision(
            self,
            artifact_store,
            &identity.1,
            pins.upstream_revision_id(),
        )?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if dependency_operation_is_replay(
            &transaction,
            "dependency.repinned",
            dependency_id,
            expected_version,
            &pins,
            None,
            context,
        )? {
            return load_dependency_operation_outcome(&transaction, context.operation_id());
        }
        let mut dependency = load_dependency_internal(&transaction, dependency_id, None, true)?;
        dependency.repin(
            expected_version,
            pins,
            context.occurred_at,
            context.actor.clone(),
        )?;
        persist_dependency_transition(
            &transaction,
            "dependency.repinned",
            &dependency,
            expected_version,
            context,
        )?;
        transaction.commit()?;
        Ok(dependency)
    }

    /// Removes one dependency through exact-version compare-and-swap.
    ///
    /// # Errors
    ///
    /// Missing canonical content, stale/terminal state, or conflicts fail atomically.
    pub fn remove_dependency(
        &mut self,
        artifact_store: &ArtifactStore,
        dependency_id: &DependencyId,
        expected_version: RelationshipVersion,
        context: &MutationContext,
    ) -> Result<Dependency, StoreError> {
        verify_dependency_content(self, artifact_store, dependency_id)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let pins = read_dependency_pins(&transaction, dependency_id)?;
        if dependency_operation_is_replay(
            &transaction,
            "dependency.removed",
            dependency_id,
            expected_version,
            &pins,
            None,
            context,
        )? {
            return load_dependency_operation_outcome(&transaction, context.operation_id());
        }
        let mut dependency = load_dependency_internal(&transaction, dependency_id, None, true)?;
        dependency.remove(expected_version, context.occurred_at, context.actor.clone())?;
        persist_dependency_transition(
            &transaction,
            "dependency.removed",
            &dependency,
            expected_version,
            context,
        )?;
        transaction.commit()?;
        Ok(dependency)
    }

    /// Loads one dependency and verifies both exact canonical revision pins.
    ///
    /// # Errors
    ///
    /// Missing content or projection/event drift fails closed.
    pub fn dependency(
        &self,
        artifact_store: &ArtifactStore,
        dependency_id: &DependencyId,
    ) -> Result<Dependency, StoreError> {
        verify_dependency_content(self, artifact_store, dependency_id)?;
        load_dependency_internal(&self.connection, dependency_id, None, true)
    }

    /// Lists all active and removed dependencies touching one Change.
    ///
    /// # Errors
    ///
    /// Every exact pin and immutable history is revalidated.
    pub fn dependencies(
        &self,
        artifact_store: &ArtifactStore,
        change_id: &ChangeId,
    ) -> Result<Vec<Dependency>, StoreError> {
        self.load_change(artifact_store, change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT dependency_id FROM dependencies
             WHERE downstream_change_id = ?1 OR upstream_change_id = ?1
             ORDER BY created_at_unix_ms, dependency_id",
        )?;
        let ids = statement
            .query_map([change_id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| self.dependency(artifact_store, &DependencyId::new(id)?))
            .collect()
    }

    /// Derives dependency freshness from both current Change heads.
    ///
    /// # Errors
    ///
    /// Exact pin content and Change histories must remain valid.
    pub fn dependency_freshness(
        &self,
        artifact_store: &ArtifactStore,
        dependency_id: &DependencyId,
    ) -> Result<DependencyFreshness, StoreError> {
        let dependency = self.dependency(artifact_store, dependency_id)?;
        if !dependency.is_active() {
            return Ok(DependencyFreshness::Removed);
        }
        let downstream = self.load_change(artifact_store, dependency.downstream_change_id())?;
        let upstream = self.load_change(artifact_store, dependency.upstream_change_id())?;
        let downstream_head = downstream.head().ok_or(StoreError::InvariantViolation(
            "dependency downstream Change has no head",
        ))?;
        let upstream_head = upstream.head().ok_or(StoreError::InvariantViolation(
            "dependency upstream Change has no head",
        ))?;
        Ok(dependency.freshness(downstream_head, upstream_head))
    }

    /// Lists immutable dependency events touching one Change.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when event values or exact ownership are invalid.
    pub fn dependency_events(
        &self,
        artifact_store: &ArtifactStore,
        change_id: &ChangeId,
    ) -> Result<Vec<DependencyEvent>, StoreError> {
        self.dependencies(artifact_store, change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT event.event_id, event.event_kind, event.dependency_id,
                    event.downstream_change_id, event.upstream_change_id,
                    event.expected_version, event.resulting_version,
                    event.resulting_downstream_revision_id,
                    event.resulting_upstream_revision_id, event.operation_id,
                    operation.actor_id, operation.occurred_at_unix_ms
             FROM dependency_events AS event
             JOIN operation_records AS operation USING (operation_id)
             WHERE event.downstream_change_id = ?1 OR event.upstream_change_id = ?1
             ORDER BY event.event_id",
        )?;
        let rows = statement.query_map([change_id.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, i64>(11)?,
            ))
        })?;
        rows.map(|row| {
            let (
                event_id,
                kind,
                id,
                downstream,
                upstream,
                expected,
                resulting,
                downstream_revision,
                upstream_revision,
                operation,
                actor,
                at,
            ) = row?;
            Ok(DependencyEvent {
                event_id,
                event_kind: kind,
                dependency_id: DependencyId::new(id)?,
                downstream_change_id: ChangeId::new(downstream)?,
                upstream_change_id: ChangeId::new(upstream)?,
                expected_version: RelationshipVersion::new(expected)?,
                resulting_version: RelationshipVersion::new(resulting)?,
                resulting_pins: DependencyPins::new(
                    RevisionId::new(downstream_revision)?,
                    RevisionId::new(upstream_revision)?,
                ),
                operation_id: operation,
                actor: ActorId::new(actor)?,
                occurred_at: UnixMillis::new(at)?,
            })
        })
        .collect()
    }

    /// Creates a versioned Stack and its complete initial snapshot atomically.
    ///
    /// # Errors
    ///
    /// Missing members, duplicate identities, invalid provenance, and operation
    /// conflicts fail without a partial Stack.
    pub fn create_stack(
        &mut self,
        stack: &Stack,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        validate_new_stack(stack, context)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if stack_operation_is_replay(
            &transaction,
            "stack.created",
            stack.id(),
            StackVersion::EMPTY,
            stack.definition(),
            context,
        )? {
            return Ok(());
        }
        if stack_exists(&transaction, stack.id())? {
            return Err(StoreError::DuplicateStack(stack.id().clone()));
        }
        ensure_stack_members_exist(&transaction, stack.definition())?;
        transaction.execute(
            "INSERT INTO stacks (
                stack_id, policy, version, created_at_unix_ms, created_by,
                updated_at_unix_ms, updated_by
             ) VALUES (?1, ?2, 1, ?3, ?4, ?3, ?4)",
            params![
                stack.id().as_str(),
                stack.definition().policy().as_str(),
                stack.created_at().value(),
                stack.created_by().as_str(),
            ],
        )?;
        replace_stack_projection_members(&transaction, stack.id(), stack.definition())?;
        insert_operation_record(&transaction, "stack.created", context)?;
        insert_stack_event(
            &transaction,
            "stack.created",
            stack,
            StackVersion::EMPTY,
            context,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Replaces a Stack definition using exact-version compare-and-swap.
    ///
    /// # Errors
    ///
    /// Stale/no-op definitions, missing members, and operation conflicts fail atomically.
    pub fn replace_stack(
        &mut self,
        stack_id: &StackId,
        expected_version: StackVersion,
        definition: StackDefinition,
        context: &MutationContext,
    ) -> Result<Stack, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if stack_operation_is_replay(
            &transaction,
            "stack.revised",
            stack_id,
            expected_version,
            &definition,
            context,
        )? {
            return load_stack_operation_outcome(&transaction, context.operation_id());
        }
        ensure_stack_members_exist(&transaction, &definition)?;
        let mut stack = load_stack_internal(&transaction, stack_id, None, true)?;
        stack.replace_definition(
            expected_version,
            definition,
            context.occurred_at,
            context.actor.clone(),
        )?;
        transaction.execute(
            "UPDATE stacks SET policy = ?1, version = ?2, updated_at_unix_ms = ?3,
                    updated_by = ?4 WHERE stack_id = ?5 AND version = ?6",
            params![
                stack.definition().policy().as_str(),
                stack.version().value(),
                stack.updated_at().value(),
                stack.updated_by().as_str(),
                stack.id().as_str(),
                expected_version.value(),
            ],
        )?;
        replace_stack_projection_members(&transaction, stack.id(), stack.definition())?;
        insert_operation_record(&transaction, "stack.revised", context)?;
        insert_stack_event(
            &transaction,
            "stack.revised",
            &stack,
            expected_version,
            context,
        )?;
        transaction.commit()?;
        Ok(stack)
    }

    /// Loads a Stack by replaying its complete immutable snapshots and comparing
    /// the result with the mutable projection.
    ///
    /// # Errors
    ///
    /// Missing identity or projection/event drift fails closed.
    pub fn stack(&self, stack_id: &StackId) -> Result<Stack, StoreError> {
        load_stack_internal(&self.connection, stack_id, None, true)
    }

    /// Atomically resolves exact current Change heads and active requirements into
    /// an immutable `CompositionCandidate`.
    ///
    /// # Errors
    ///
    /// Missing/stale/reversed dependencies, changed Stack versions, repository
    /// mismatch, absent canonical content, and operation conflicts fail atomically.
    pub fn create_candidate(
        &mut self,
        artifact_store: &ArtifactStore,
        candidate_id: CandidateId,
        target_base: BaseState,
        selection: &CandidateSelection,
        context: &MutationContext,
    ) -> Result<CompositionCandidate, StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(candidate) = candidate_operation_replay(
            &transaction,
            artifact_store,
            &candidate_id,
            &target_base,
            selection,
            context,
        )? {
            return Ok(candidate);
        }
        if candidate_exists(&transaction, &candidate_id)? {
            return Err(StoreError::DuplicateCandidate(candidate_id));
        }
        let (changes, stack_ref) = resolve_candidate_selection(&transaction, selection)?;
        let mut inputs = Vec::with_capacity(changes.len());
        for change_id in &changes {
            let change = load_change_from_connection(&transaction, artifact_store, change_id)?;
            let head = change
                .head()
                .ok_or_else(|| StoreError::ChangeHasNoHead(change_id.clone()))?;
            let revision = change
                .revisions()
                .last()
                .ok_or(StoreError::InvariantViolation(
                    "Change head has no revision",
                ))?;
            if revision.base().repository_id() != target_base.repository_id() {
                return Err(StoreError::CandidateRepositoryMismatch(change_id.clone()));
            }
            inputs.push(CandidateInput::new(change_id.clone(), head.clone()));
        }
        let mut requirements = resolve_candidate_dependencies(&transaction, &inputs)?;
        if let Some(stack) = &stack_ref
            && stack.policy() == StackPolicy::PredecessorDependencies
        {
            for position in 1..inputs.len() {
                requirements.push(ResolvedRequirement::new(
                    ResolvedRequirementSource::StackPredecessor {
                        stack_id: stack.stack_id().clone(),
                        version: stack.version(),
                        downstream_position: position,
                    },
                    inputs[position].clone(),
                    inputs[position - 1].clone(),
                ));
            }
        }
        let candidate = CompositionCandidate::new(
            candidate_id,
            target_base,
            stack_ref,
            inputs,
            requirements,
            context.occurred_at,
            context.actor.clone(),
        )?;
        persist_candidate(&transaction, &candidate, context)?;
        transaction.commit()?;
        Ok(candidate)
    }

    /// Loads and rehashes one immutable candidate.
    ///
    /// # Errors
    ///
    /// Missing content, malformed exact ownership, or field/digest drift fails closed.
    pub fn candidate(
        &self,
        artifact_store: &ArtifactStore,
        candidate_id: &CandidateId,
    ) -> Result<CompositionCandidate, StoreError> {
        load_candidate_internal(&self.connection, artifact_store, candidate_id)
    }

    /// Derives metadata freshness without mutating candidate history.
    ///
    /// Live provider-target freshness is intentionally outside this result.
    ///
    /// # Errors
    ///
    /// Candidate, Change, Stack, and Dependency histories must remain valid.
    pub fn candidate_freshness(
        &self,
        artifact_store: &ArtifactStore,
        candidate_id: &CandidateId,
    ) -> Result<CandidateFreshness, StoreError> {
        let candidate = self.candidate(artifact_store, candidate_id)?;
        let mut advanced_inputs = Vec::new();
        for input in candidate.inputs() {
            let change = self.load_change(artifact_store, input.change_id())?;
            if change.head() != Some(input.revision_id()) {
                advanced_inputs.push(input.change_id().clone());
            }
        }
        let mut changed_dependencies = Vec::new();
        for requirement in candidate.requirements() {
            if let ResolvedRequirementSource::Dependency {
                dependency_id,
                version,
            } = requirement.source()
            {
                let changed = match self.dependency(artifact_store, dependency_id) {
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
            self.stack(stack_ref.stack_id())?.version() != stack_ref.version()
        } else {
            false
        };
        Ok(CandidateFreshness {
            advanced_inputs,
            changed_dependencies,
            stack_changed,
        })
    }

    /// Builds and verifies an exact revision review/validation target.
    ///
    /// # Errors
    ///
    /// The revision must belong to the Change and retain durable canonical content.
    pub fn revision_target(
        &self,
        artifact_store: &ArtifactStore,
        change_id: &ChangeId,
        revision_id: &RevisionId,
    ) -> Result<ExactTarget, StoreError> {
        exact_revision_target(&self.connection, artifact_store, change_id, revision_id)
    }

    /// Builds and verifies an exact immutable candidate target.
    ///
    /// # Errors
    ///
    /// The candidate and every exact source must pass authoritative reconstruction.
    pub fn candidate_target(
        &self,
        artifact_store: &ArtifactStore,
        candidate_id: &CandidateId,
    ) -> Result<ExactTarget, StoreError> {
        exact_candidate_target(&self.connection, artifact_store, candidate_id)
    }

    /// Persists an immutable exact-target review request and finalized reviewer set.
    ///
    /// # Errors
    ///
    /// Target drift, invalid provenance, duplicate identity, and operation conflicts
    /// fail atomically.
    pub fn create_review_request(
        &mut self,
        artifact_store: &ArtifactStore,
        request: &ReviewRequest,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        validate_review_request_provenance(request, context)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(recorded) = recorded_operation(&transaction, context.operation_id())? {
            if recorded.event_kind != "review.requested"
                || recorded.actor_id != context.actor.as_str()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            let outcome = load_review_request_operation_outcome(
                &transaction,
                artifact_store,
                context.operation_id(),
            )?;
            if outcome != *request {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok(());
        }
        verify_exact_target(&transaction, artifact_store, request.target())?;
        ensure_evidence_not_before_target(&transaction, request.target(), request.created_at())?;
        if review_request_exists(&transaction, request.id())? {
            return Err(StoreError::DuplicateReviewRequest(request.id().clone()));
        }
        insert_operation_record(&transaction, "review.requested", context)?;
        insert_review_request(&transaction, request, context)?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads one request and revalidates its exact source, operation, and reviewers.
    ///
    /// # Errors
    ///
    /// Missing identity, canonical content, or immutable-row drift fails closed.
    pub fn review_request(
        &self,
        artifact_store: &ArtifactStore,
        request_id: &ReviewRequestId,
    ) -> Result<ReviewRequest, StoreError> {
        load_review_request_internal(&self.connection, artifact_store, request_id)
    }

    /// Appends one immutable submission by a requested reviewer.
    ///
    /// # Errors
    ///
    /// Request/target drift, invalid provenance, duplicate identity, and operation
    /// conflicts fail atomically.
    pub fn create_review_submission(
        &mut self,
        artifact_store: &ArtifactStore,
        submission: &ReviewSubmission,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        validate_review_submission_provenance(submission, context)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(recorded) = recorded_operation(&transaction, context.operation_id())? {
            if recorded.event_kind != "review.submitted"
                || recorded.actor_id != context.actor.as_str()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            let outcome = load_review_submission_operation_outcome(
                &transaction,
                artifact_store,
                context.operation_id(),
            )?;
            if outcome != *submission {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok(());
        }
        let request =
            load_review_request_internal(&transaction, artifact_store, submission.request_id())?;
        let reconstructed = ReviewSubmission::new(
            submission.id().clone(),
            &request,
            submission.reviewer().clone(),
            submission.outcome(),
            submission.comments().map(str::to_owned),
            submission.submitted_at(),
        )?;
        if reconstructed != *submission {
            return Err(StoreError::InvariantViolation(
                "review submission does not match its exact request",
            ));
        }
        if review_submission_exists(&transaction, submission.id())? {
            return Err(StoreError::DuplicateReviewSubmission(
                submission.id().clone(),
            ));
        }
        insert_operation_record(&transaction, "review.submitted", context)?;
        insert_review_submission(&transaction, submission, context)?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads one immutable review submission through its exact request.
    ///
    /// # Errors
    ///
    /// Missing source or request/submission drift fails closed.
    pub fn review_submission(
        &self,
        artifact_store: &ArtifactStore,
        submission_id: &ReviewSubmissionId,
    ) -> Result<ReviewSubmission, StoreError> {
        load_review_submission_internal(&self.connection, artifact_store, submission_id)
    }

    /// Lists every immutable submission for one request in submission order.
    ///
    /// # Errors
    ///
    /// Every request, target, reviewer, and operation is revalidated.
    pub fn review_submissions(
        &self,
        artifact_store: &ArtifactStore,
        request_id: &ReviewRequestId,
    ) -> Result<Vec<ReviewSubmission>, StoreError> {
        self.review_request(artifact_store, request_id)?;
        let mut statement = self.connection.prepare(
            "SELECT review_submission_id FROM review_submissions
             WHERE review_request_id = ?1
             ORDER BY submitted_at_unix_ms, review_submission_id",
        )?;
        let ids = statement
            .query_map([request_id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| self.review_submission(artifact_store, &ReviewSubmissionId::new(id)?))
            .collect()
    }

    /// Persists one immutable exact-target validation result.
    ///
    /// # Errors
    ///
    /// Target drift, invalid provenance, duplicate identity, and operation conflicts
    /// fail atomically.
    pub fn create_validation_result(
        &mut self,
        artifact_store: &ArtifactStore,
        result: &ValidationResult,
        context: &MutationContext,
    ) -> Result<(), StoreError> {
        validate_validation_provenance(result, context)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(recorded) = recorded_operation(&transaction, context.operation_id())? {
            if recorded.event_kind != "validation.recorded"
                || recorded.actor_id != context.actor.as_str()
            {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            let outcome = load_validation_operation_outcome(
                &transaction,
                artifact_store,
                context.operation_id(),
            )?;
            if outcome != *result {
                return Err(StoreError::OperationIdConflict(
                    context.operation_id.clone(),
                ));
            }
            return Ok(());
        }
        verify_exact_target(&transaction, artifact_store, result.target())?;
        ensure_evidence_not_before_target(&transaction, result.target(), result.validated_at())?;
        if validation_result_exists(&transaction, result.id())? {
            return Err(StoreError::DuplicateValidationResult(result.id().clone()));
        }
        insert_operation_record(&transaction, "validation.recorded", context)?;
        insert_validation_result(&transaction, result, context)?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads one result and revalidates its exact source and operation provenance.
    ///
    /// # Errors
    ///
    /// Missing identity, source content, or immutable-row drift fails closed.
    pub fn validation_result(
        &self,
        artifact_store: &ArtifactStore,
        result_id: &ValidationResultId,
    ) -> Result<ValidationResult, StoreError> {
        load_validation_result_internal(&self.connection, artifact_store, result_id)
    }

    /// Derives exact-target freshness without applying any reuse declaration.
    ///
    /// # Errors
    ///
    /// Exact source histories must remain authoritative and reconstructable.
    pub fn exact_target_freshness(
        &self,
        artifact_store: &ArtifactStore,
        target: &ExactTarget,
    ) -> Result<ExactTargetFreshness, StoreError> {
        verify_exact_target(&self.connection, artifact_store, target)?;
        match target {
            ExactTarget::Revision {
                change_id,
                revision_id,
                ..
            } => {
                let change = self.load_change(artifact_store, change_id)?;
                if change.head() == Some(revision_id) {
                    Ok(ExactTargetFreshness::Current)
                } else {
                    Ok(ExactTargetFreshness::RevisionAdvanced)
                }
            }
            ExactTarget::Candidate { candidate_id, .. } => {
                let freshness = self.candidate_freshness(artifact_store, candidate_id)?;
                if freshness.is_current() {
                    Ok(ExactTargetFreshness::Current)
                } else {
                    Ok(ExactTargetFreshness::CandidateStale(freshness))
                }
            }
        }
    }

    /// Derives one review request's target freshness.
    ///
    /// # Errors
    ///
    /// The request and exact source must pass authoritative reconstruction.
    pub fn review_request_freshness(
        &self,
        artifact_store: &ArtifactStore,
        request_id: &ReviewRequestId,
    ) -> Result<ExactTargetFreshness, StoreError> {
        let request = self.review_request(artifact_store, request_id)?;
        self.exact_target_freshness(artifact_store, request.target())
    }

    /// Derives one validation result's factual target freshness without applying
    /// its optional reusable-scope declaration.
    ///
    /// # Errors
    ///
    /// The result and exact source must pass authoritative reconstruction.
    pub fn validation_result_freshness(
        &self,
        artifact_store: &ArtifactStore,
        result_id: &ValidationResultId,
    ) -> Result<ExactTargetFreshness, StoreError> {
        let result = self.validation_result(artifact_store, result_id)?;
        self.exact_target_freshness(artifact_store, result.target())
    }
}

fn exact_revision_target(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    change_id: &ChangeId,
    revision_id: &RevisionId,
) -> Result<ExactTarget, StoreError> {
    let change = load_change_from_connection(connection, artifact_store, change_id)?;
    let revision = change
        .revisions()
        .iter()
        .find(|revision| revision.revision_id() == revision_id)
        .ok_or_else(|| StoreError::RevisionNotFoundForChange {
            change_id: change_id.clone(),
            revision_id: revision_id.clone(),
        })?;
    Ok(ExactTarget::revision(
        change_id.clone(),
        revision_id.clone(),
        revision.base().clone(),
        revision.artifact().manifest_digest(),
    )?)
}

fn exact_candidate_target(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    candidate_id: &CandidateId,
) -> Result<ExactTarget, StoreError> {
    let candidate = load_candidate_internal(connection, artifact_store, candidate_id)?;
    Ok(ExactTarget::candidate(
        candidate_id.clone(),
        candidate.target_base().clone(),
        candidate.content_digest().as_str(),
    )?)
}

fn verify_exact_target(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    target: &ExactTarget,
) -> Result<(), StoreError> {
    let authoritative = match target {
        ExactTarget::Revision {
            change_id,
            revision_id,
            ..
        } => exact_revision_target(connection, artifact_store, change_id, revision_id)?,
        ExactTarget::Candidate { candidate_id, .. } => {
            exact_candidate_target(connection, artifact_store, candidate_id)?
        }
    };
    if authoritative != *target {
        return Err(StoreError::ExactTargetMismatch);
    }
    Ok(())
}

fn ensure_evidence_not_before_target(
    connection: &Connection,
    target: &ExactTarget,
    evidence_at: UnixMillis,
) -> Result<(), StoreError> {
    let created_at = match target {
        ExactTarget::Revision {
            change_id,
            revision_id,
            ..
        } => connection.query_row(
            "SELECT created_at_unix_ms FROM change_revisions
             WHERE change_id = ?1 AND revision_id = ?2",
            params![change_id.as_str(), revision_id.as_str()],
            |row| row.get::<_, i64>(0),
        )?,
        ExactTarget::Candidate { candidate_id, .. } => connection.query_row(
            "SELECT created_at_unix_ms FROM composition_candidates WHERE candidate_id = ?1",
            [candidate_id.as_str()],
            |row| row.get::<_, i64>(0),
        )?,
    };
    if evidence_at < UnixMillis::new(created_at)? {
        return Err(StoreError::EvidenceBeforeTarget);
    }
    Ok(())
}

fn validate_review_request_provenance(
    request: &ReviewRequest,
    context: &MutationContext,
) -> Result<(), StoreError> {
    if request.requested_by() != &context.actor || request.created_at() != context.occurred_at {
        return Err(StoreError::InvariantViolation(
            "review request does not match mutation provenance",
        ));
    }
    Ok(())
}

fn validate_review_submission_provenance(
    submission: &ReviewSubmission,
    context: &MutationContext,
) -> Result<(), StoreError> {
    if submission.reviewer() != &context.actor || submission.submitted_at() != context.occurred_at {
        return Err(StoreError::InvariantViolation(
            "review submission does not match mutation provenance",
        ));
    }
    Ok(())
}

fn validate_validation_provenance(
    result: &ValidationResult,
    context: &MutationContext,
) -> Result<(), StoreError> {
    if result.validated_by() != &context.actor || result.validated_at() != context.occurred_at {
        return Err(StoreError::InvariantViolation(
            "validation result does not match mutation provenance",
        ));
    }
    Ok(())
}

struct TargetColumns<'a> {
    kind: &'static str,
    change_id: Option<&'a str>,
    revision_id: Option<&'a str>,
    candidate_id: Option<&'a str>,
    repository_id: &'a str,
    context_object_id: &'a str,
    digest: &'a str,
}

fn target_columns(target: &ExactTarget) -> TargetColumns<'_> {
    match target {
        ExactTarget::Revision {
            change_id,
            revision_id,
            base,
            artifact_digest,
        } => TargetColumns {
            kind: "revision",
            change_id: Some(change_id.as_str()),
            revision_id: Some(revision_id.as_str()),
            candidate_id: None,
            repository_id: base.repository_id().as_str(),
            context_object_id: base.object_id(),
            digest: artifact_digest,
        },
        ExactTarget::Candidate {
            candidate_id,
            target_base,
            content_digest,
        } => TargetColumns {
            kind: "candidate",
            change_id: None,
            revision_id: None,
            candidate_id: Some(candidate_id.as_str()),
            repository_id: target_base.repository_id().as_str(),
            context_object_id: target_base.object_id(),
            digest: content_digest,
        },
    }
}

struct StoredTarget {
    kind: String,
    change_id: Option<String>,
    revision_id: Option<String>,
    candidate_id: Option<String>,
    repository_id: String,
    context_object_id: String,
    digest: String,
}

impl StoredTarget {
    fn into_domain(self) -> Result<ExactTarget, StoreError> {
        let base = BaseState::new(
            RepositoryId::new(self.repository_id)?,
            self.context_object_id,
        )?;
        match (
            self.kind.as_str(),
            self.change_id,
            self.revision_id,
            self.candidate_id,
        ) {
            ("revision", Some(change), Some(revision), None) => Ok(ExactTarget::revision(
                ChangeId::new(change)?,
                RevisionId::new(revision)?,
                base,
                self.digest,
            )?),
            ("candidate", None, None, Some(candidate)) => Ok(ExactTarget::candidate(
                CandidateId::new(candidate)?,
                base,
                self.digest,
            )?),
            _ => Err(StoreError::InvalidStoredData(
                "exact target has an invalid revision/candidate shape".to_owned(),
            )),
        }
    }
}

fn review_request_exists(
    connection: &Connection,
    request_id: &ReviewRequestId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM review_requests WHERE review_request_id = ?1)",
        [request_id.as_str()],
        |row| row.get(0),
    )
}

fn insert_review_request(
    transaction: &Transaction<'_>,
    request: &ReviewRequest,
    context: &MutationContext,
) -> Result<(), StoreError> {
    let target = target_columns(request.target());
    transaction.execute(
        "INSERT INTO review_requests (
            review_request_id, target_kind, change_id, revision_id, candidate_id,
            repository_id, context_object_id, content_digest, requested_by,
            reviewer_count, reuse_policy, created_at_unix_ms, operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            request.id().as_str(),
            target.kind,
            target.change_id,
            target.revision_id,
            target.candidate_id,
            target.repository_id,
            target.context_object_id,
            target.digest,
            request.requested_by().as_str(),
            i64::try_from(request.reviewers().len()).map_err(|_| StoreError::CollectionTooLarge)?,
            request.reuse_policy().as_str(),
            request.created_at().value(),
            context.operation_id,
        ],
    )?;
    for (position, reviewer) in request.reviewers().iter().enumerate() {
        transaction.execute(
            "INSERT INTO review_request_reviewers (
                review_request_id, reviewer_position, reviewer_id
             ) VALUES (?1, ?2, ?3)",
            params![
                request.id().as_str(),
                i64::try_from(position).map_err(|_| StoreError::CollectionTooLarge)?,
                reviewer.as_str(),
            ],
        )?;
    }
    Ok(())
}

fn load_review_request_internal(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    request_id: &ReviewRequestId,
) -> Result<ReviewRequest, StoreError> {
    let row = connection
        .query_row(
            "SELECT request.target_kind, request.change_id, request.revision_id,
                request.candidate_id, request.repository_id, request.context_object_id,
                request.content_digest, request.requested_by, request.reviewer_count,
                request.reuse_policy, request.created_at_unix_ms,
                operation.event_kind, operation.actor_id, operation.occurred_at_unix_ms
         FROM review_requests AS request
         JOIN operation_records AS operation USING (operation_id)
         WHERE request.review_request_id = ?1",
            [request_id.as_str()],
            |row| {
                Ok((
                    StoredTarget {
                        kind: row.get(0)?,
                        change_id: row.get(1)?,
                        revision_id: row.get(2)?,
                        candidate_id: row.get(3)?,
                        repository_id: row.get(4)?,
                        context_object_id: row.get(5)?,
                        digest: row.get(6)?,
                    },
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, i64>(13)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::ReviewRequestNotFound(request_id.clone()))?;
    let target = row.0.into_domain()?;
    verify_exact_target(connection, artifact_store, &target)?;
    ensure_evidence_not_before_target(connection, &target, UnixMillis::new(row.4)?)?;
    let mut statement = connection.prepare(
        "SELECT reviewer_position, reviewer_id FROM review_request_reviewers
         WHERE review_request_id = ?1 ORDER BY reviewer_position",
    )?;
    let reviewer_rows = statement
        .query_map([request_id.as_str()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if i64::try_from(reviewer_rows.len()).map_err(|_| StoreError::CollectionTooLarge)? != row.2 {
        return Err(StoreError::InvalidStoredData(
            "reviewer set is not finalized".to_owned(),
        ));
    }
    let reviewers = reviewer_rows
        .into_iter()
        .enumerate()
        .map(|(expected, (position, reviewer))| {
            if position != i64::try_from(expected).map_err(|_| StoreError::CollectionTooLarge)? {
                return Err(StoreError::InvalidStoredData(
                    "reviewer positions are not contiguous".to_owned(),
                ));
            }
            Ok(ActorId::new(reviewer)?)
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let request = ReviewRequest::new(
        request_id.clone(),
        target,
        ActorId::new(row.1)?,
        reviewers,
        UnixMillis::new(row.4)?,
    )?;
    if request.reuse_policy() != ReviewReusePolicy::parse(&row.3)?
        || row.5 != "review.requested"
        || row.6 != request.requested_by().as_str()
        || row.7 != request.created_at().value()
    {
        return Err(StoreError::InvalidStoredData(
            "review request operation provenance drifted".to_owned(),
        ));
    }
    Ok(request)
}

fn load_review_request_operation_outcome(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    operation_id: &str,
) -> Result<ReviewRequest, StoreError> {
    let id = connection
        .query_row(
            "SELECT review_request_id FROM review_requests WHERE operation_id = ?1",
            [operation_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData("review operation has no request outcome".to_owned())
        })?;
    load_review_request_internal(connection, artifact_store, &ReviewRequestId::new(id)?)
}

fn review_submission_exists(
    connection: &Connection,
    submission_id: &ReviewSubmissionId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM review_submissions WHERE review_submission_id = ?1)",
        [submission_id.as_str()],
        |row| row.get(0),
    )
}

fn insert_review_submission(
    transaction: &Transaction<'_>,
    submission: &ReviewSubmission,
    context: &MutationContext,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO review_submissions (
            review_submission_id, review_request_id, reviewer_id, outcome,
            comments, submitted_at_unix_ms, operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            submission.id().as_str(),
            submission.request_id().as_str(),
            submission.reviewer().as_str(),
            submission.outcome().as_str(),
            submission.comments(),
            submission.submitted_at().value(),
            context.operation_id,
        ],
    )?;
    Ok(())
}

fn load_review_submission_internal(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    submission_id: &ReviewSubmissionId,
) -> Result<ReviewSubmission, StoreError> {
    let row = connection
        .query_row(
            "SELECT submission.review_request_id, submission.reviewer_id,
                submission.outcome, submission.comments, submission.submitted_at_unix_ms,
                operation.event_kind, operation.actor_id, operation.occurred_at_unix_ms
         FROM review_submissions AS submission
         JOIN operation_records AS operation USING (operation_id)
         WHERE submission.review_submission_id = ?1",
            [submission_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::ReviewSubmissionNotFound(submission_id.clone()))?;
    let request_id = ReviewRequestId::new(row.0)?;
    let request = load_review_request_internal(connection, artifact_store, &request_id)?;
    let submission = ReviewSubmission::new(
        submission_id.clone(),
        &request,
        ActorId::new(row.1)?,
        ReviewOutcome::parse(&row.2)?,
        row.3,
        UnixMillis::new(row.4)?,
    )?;
    if row.5 != "review.submitted"
        || row.6 != submission.reviewer().as_str()
        || row.7 != submission.submitted_at().value()
    {
        return Err(StoreError::InvalidStoredData(
            "review submission operation provenance drifted".to_owned(),
        ));
    }
    Ok(submission)
}

fn load_review_submission_operation_outcome(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    operation_id: &str,
) -> Result<ReviewSubmission, StoreError> {
    let id = connection
        .query_row(
            "SELECT review_submission_id FROM review_submissions WHERE operation_id = ?1",
            [operation_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData("review operation has no submission outcome".to_owned())
        })?;
    load_review_submission_internal(connection, artifact_store, &ReviewSubmissionId::new(id)?)
}

fn validation_result_exists(
    connection: &Connection,
    result_id: &ValidationResultId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM validation_results WHERE validation_result_id = ?1)",
        [result_id.as_str()],
        |row| row.get(0),
    )
}

fn insert_validation_result(
    transaction: &Transaction<'_>,
    result: &ValidationResult,
    context: &MutationContext,
) -> Result<(), StoreError> {
    let target = target_columns(result.target());
    let (reusable_scope, rationale) = result
        .scope()
        .declaration()
        .map_or((None, None), |(scope, rationale)| {
            (Some(scope), Some(rationale))
        });
    transaction.execute(
        "INSERT INTO validation_results (
            validation_result_id, target_kind, change_id, revision_id, candidate_id,
            repository_id, context_object_id, content_digest, validation_type,
            environment, outcome, execution_id, scope_kind, reusable_scope,
            scope_rationale, validated_by, validated_at_unix_ms, operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                   ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            result.id().as_str(),
            target.kind,
            target.change_id,
            target.revision_id,
            target.candidate_id,
            target.repository_id,
            target.context_object_id,
            target.digest,
            result.validation_type().as_str(),
            result.environment().as_str(),
            result.outcome().as_str(),
            result.execution_id().as_str(),
            result.scope().as_str(),
            reusable_scope,
            rationale,
            result.validated_by().as_str(),
            result.validated_at().value(),
            context.operation_id
        ],
    )?;
    Ok(())
}

fn load_validation_result_internal(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    result_id: &ValidationResultId,
) -> Result<ValidationResult, StoreError> {
    let row = connection
        .query_row(
            "SELECT result.target_kind, result.change_id, result.revision_id,
                result.candidate_id, result.repository_id, result.context_object_id,
                result.content_digest, result.validation_type, result.environment,
                result.outcome, result.execution_id, result.scope_kind,
                result.reusable_scope, result.scope_rationale, result.validated_by,
                result.validated_at_unix_ms, operation.event_kind,
                operation.actor_id, operation.occurred_at_unix_ms
         FROM validation_results AS result
         JOIN operation_records AS operation USING (operation_id)
         WHERE result.validation_result_id = ?1",
            [result_id.as_str()],
            |row| {
                Ok((
                    StoredTarget {
                        kind: row.get(0)?,
                        change_id: row.get(1)?,
                        revision_id: row.get(2)?,
                        candidate_id: row.get(3)?,
                        repository_id: row.get(4)?,
                        context_object_id: row.get(5)?,
                        digest: row.get(6)?,
                    },
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, i64>(15)?,
                    row.get::<_, String>(16)?,
                    row.get::<_, String>(17)?,
                    row.get::<_, i64>(18)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::ValidationResultNotFound(result_id.clone()))?;
    let target = row.0.into_domain()?;
    verify_exact_target(connection, artifact_store, &target)?;
    let scope = match (row.5.as_str(), row.6, row.7) {
        ("exact_target", None, None) => ValidationScope::ExactTarget,
        ("declared_reusable", Some(scope), Some(rationale)) => {
            ValidationScope::declared_reusable(scope, rationale)?
        }
        _ => {
            return Err(StoreError::InvalidStoredData(
                "validation scope shape is invalid".to_owned(),
            ));
        }
    };
    let validated_at = UnixMillis::new(row.9)?;
    ensure_evidence_not_before_target(connection, &target, validated_at)?;
    let result = ValidationResult::new(
        result_id.clone(),
        target,
        ValidationObservation::new(
            ValidationType::new(row.1)?,
            ValidationEnvironment::new(row.2)?,
            ValidationOutcome::parse(&row.3)?,
            ValidationExecutionId::new(row.4)?,
            scope,
        ),
        ActorId::new(row.8)?,
        validated_at,
    );
    if row.10 != "validation.recorded"
        || row.11 != result.validated_by().as_str()
        || row.12 != result.validated_at().value()
    {
        return Err(StoreError::InvalidStoredData(
            "validation operation provenance drifted".to_owned(),
        ));
    }
    Ok(result)
}

fn load_validation_operation_outcome(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    operation_id: &str,
) -> Result<ValidationResult, StoreError> {
    let id = connection
        .query_row(
            "SELECT validation_result_id FROM validation_results WHERE operation_id = ?1",
            [operation_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData("validation operation has no result outcome".to_owned())
        })?;
    load_validation_result_internal(connection, artifact_store, &ValidationResultId::new(id)?)
}

fn validate_new_stack(stack: &Stack, context: &MutationContext) -> Result<(), StoreError> {
    if stack.version() != StackVersion::INITIAL
        || stack.created_at() != context.occurred_at
        || stack.updated_at() != context.occurred_at
        || stack.created_by() != &context.actor
        || stack.updated_by() != &context.actor
    {
        return Err(StoreError::InvariantViolation(
            "new Stack does not match mutation provenance",
        ));
    }
    Ok(())
}

fn stack_exists(connection: &Connection, stack_id: &StackId) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM stacks WHERE stack_id = ?1)",
        [stack_id.as_str()],
        |row| row.get(0),
    )
}

fn ensure_stack_members_exist(
    connection: &Connection,
    definition: &StackDefinition,
) -> Result<(), StoreError> {
    for member in definition.members() {
        ensure_change_exists(connection, member.change_id())?;
    }
    Ok(())
}

fn replace_stack_projection_members(
    transaction: &Transaction<'_>,
    stack_id: &StackId,
    definition: &StackDefinition,
) -> Result<(), StoreError> {
    transaction.execute(
        "DELETE FROM stack_members WHERE stack_id = ?1",
        [stack_id.as_str()],
    )?;
    for (position, member) in definition.members().iter().enumerate() {
        transaction.execute(
            "INSERT INTO stack_members (stack_id, position, change_id, predecessor_change_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                stack_id.as_str(),
                i64::try_from(position).map_err(|_| StoreError::CollectionTooLarge)?,
                member.change_id().as_str(),
                member.predecessor_change_id().map(ChangeId::as_str),
            ],
        )?;
    }
    Ok(())
}

fn insert_stack_event(
    transaction: &Transaction<'_>,
    event_kind: &str,
    stack: &Stack,
    expected_version: StackVersion,
    context: &MutationContext,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO stack_events (
            event_kind, stack_id, expected_version, resulting_version,
            resulting_policy, member_count, operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            event_kind,
            stack.id().as_str(),
            expected_version.value(),
            stack.version().value(),
            stack.definition().policy().as_str(),
            i64::try_from(stack.definition().members().len())
                .map_err(|_| StoreError::CollectionTooLarge)?,
            context.operation_id,
        ],
    )?;
    let event_id = transaction.last_insert_rowid();
    for (position, member) in stack.definition().members().iter().enumerate() {
        transaction.execute(
            "INSERT INTO stack_event_members (
                event_id, position, change_id, predecessor_change_id
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                event_id,
                i64::try_from(position).map_err(|_| StoreError::CollectionTooLarge)?,
                member.change_id().as_str(),
                member.predecessor_change_id().map(ChangeId::as_str),
            ],
        )?;
    }
    Ok(())
}

fn read_stack_definition(
    connection: &Connection,
    event_id: i64,
    policy: &str,
) -> Result<StackDefinition, StoreError> {
    let member_count = connection.query_row(
        "SELECT member_count FROM stack_events WHERE event_id = ?1",
        [event_id],
        |row| row.get::<_, i64>(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT position, change_id, predecessor_change_id
         FROM stack_event_members WHERE event_id = ?1 ORDER BY position",
    )?;
    let rows = statement
        .query_map([event_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if i64::try_from(rows.len()).map_err(|_| StoreError::CollectionTooLarge)? != member_count {
        return Err(StoreError::InvalidStoredData(
            "Stack snapshot member count is not finalized".to_owned(),
        ));
    }
    let mut members = Vec::with_capacity(rows.len());
    for (expected, (position, change_id, predecessor)) in rows.into_iter().enumerate() {
        if position != i64::try_from(expected).map_err(|_| StoreError::CollectionTooLarge)? {
            return Err(StoreError::InvalidStoredData(
                "Stack snapshot positions are not contiguous".to_owned(),
            ));
        }
        members.push(StackMember::new(
            ChangeId::new(change_id)?,
            predecessor.map(ChangeId::new).transpose()?,
        ));
    }
    Ok(StackDefinition::new(StackPolicy::parse(policy)?, members)?)
}

fn read_stack_projection_definition(
    connection: &Connection,
    stack_id: &StackId,
    policy: &str,
) -> Result<StackDefinition, StoreError> {
    let mut statement = connection.prepare(
        "SELECT position, change_id, predecessor_change_id
         FROM stack_members WHERE stack_id = ?1 ORDER BY position",
    )?;
    let rows = statement
        .query_map([stack_id.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut members = Vec::with_capacity(rows.len());
    for (expected, (position, change_id, predecessor)) in rows.into_iter().enumerate() {
        if position != i64::try_from(expected).map_err(|_| StoreError::CollectionTooLarge)? {
            return Err(StoreError::InvalidStoredData(
                "Stack projection positions are not contiguous".to_owned(),
            ));
        }
        members.push(StackMember::new(
            ChangeId::new(change_id)?,
            predecessor.map(ChangeId::new).transpose()?,
        ));
    }
    Ok(StackDefinition::new(StackPolicy::parse(policy)?, members)?)
}

fn load_stack_internal(
    connection: &Connection,
    stack_id: &StackId,
    through_version: Option<StackVersion>,
    compare_projection: bool,
) -> Result<Stack, StoreError> {
    let projection = connection
        .query_row(
            "SELECT policy, version, created_at_unix_ms, created_by,
                    updated_at_unix_ms, updated_by
             FROM stacks WHERE stack_id = ?1",
            [stack_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::StackNotFound(stack_id.clone()))?;
    let mut statement = connection.prepare(
        "SELECT event.event_id, event.event_kind, event.expected_version,
                event.resulting_version, event.resulting_policy,
                operation.actor_id, operation.occurred_at_unix_ms
         FROM stack_events AS event
         JOIN operation_records AS operation USING (operation_id)
         WHERE event.stack_id = ?1 AND (?2 IS NULL OR event.resulting_version <= ?2)
         ORDER BY event.resulting_version",
    )?;
    let events = statement
        .query_map(
            params![stack_id.as_str(), through_version.map(StackVersion::value)],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let Some(created) = events.first() else {
        return Err(StoreError::InvalidStoredData(
            "Stack has no creation snapshot".to_owned(),
        ));
    };
    if created.1 != "stack.created" || created.2 != 0 || created.3 != 1 {
        return Err(StoreError::InvalidStoredData(
            "Stack creation snapshot is invalid".to_owned(),
        ));
    }
    let mut stack = Stack::new(
        stack_id.clone(),
        read_stack_definition(connection, created.0, &created.4)?,
        UnixMillis::new(created.6)?,
        ActorId::new(created.5.clone())?,
    );
    for event in events.iter().skip(1) {
        if event.1 != "stack.revised"
            || event.2 != stack.version().value()
            || event.3 != event.2 + 1
        {
            return Err(StoreError::InvalidStoredData(
                "Stack snapshot history is not linear".to_owned(),
            ));
        }
        stack.replace_definition(
            StackVersion::new(event.2)?,
            read_stack_definition(connection, event.0, &event.4)?,
            UnixMillis::new(event.6)?,
            ActorId::new(event.5.clone())?,
        )?;
    }
    if compare_projection {
        let projection_definition =
            read_stack_projection_definition(connection, stack_id, &projection.0)?;
        if stack.definition() != &projection_definition
            || stack.version() != StackVersion::new(projection.1)?
            || stack.created_at() != UnixMillis::new(projection.2)?
            || stack.created_by() != &ActorId::new(projection.3)?
            || stack.updated_at() != UnixMillis::new(projection.4)?
            || stack.updated_by() != &ActorId::new(projection.5)?
        {
            return Err(StoreError::InvalidStoredData(
                "Stack projection does not match immutable snapshots".to_owned(),
            ));
        }
    }
    Ok(stack)
}

fn stack_operation_is_replay(
    transaction: &Transaction<'_>,
    event_kind: &str,
    stack_id: &StackId,
    expected_version: StackVersion,
    definition: &StackDefinition,
    context: &MutationContext,
) -> Result<bool, StoreError> {
    let Some(operation) = recorded_operation(transaction, context.operation_id())? else {
        return Ok(false);
    };
    if operation.event_kind != event_kind || operation.actor_id != context.actor.as_str() {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    let (recorded_id, recorded_expected, resulting_version, policy, event_id) = transaction
        .query_row(
            "SELECT stack_id, expected_version, resulting_version, resulting_policy, event_id
             FROM stack_events WHERE operation_id = ?1",
            [context.operation_id()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::InvalidStoredData("Stack operation has no event".to_owned()))?;
    let recorded_definition = read_stack_definition(transaction, event_id, &policy)?;
    if recorded_id != stack_id.as_str()
        || recorded_expected != expected_version.value()
        || recorded_definition != *definition
    {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    StackVersion::new(resulting_version)?;
    Ok(true)
}

fn load_stack_operation_outcome(
    connection: &Connection,
    operation_id: &str,
) -> Result<Stack, StoreError> {
    let (id, version) = connection
        .query_row(
            "SELECT stack_id, resulting_version FROM stack_events WHERE operation_id = ?1",
            [operation_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData("Stack operation has no outcome".to_owned())
        })?;
    load_stack_internal(
        connection,
        &StackId::new(id)?,
        Some(StackVersion::new(version)?),
        false,
    )
}

fn resolve_candidate_selection(
    connection: &Connection,
    selection: &CandidateSelection,
) -> Result<(Vec<ChangeId>, Option<CandidateStackRef>), StoreError> {
    match selection {
        CandidateSelection::Changes(changes) => {
            let definition =
                StackDefinition::from_changes(StackPolicy::OrderOnly, changes.clone())?;
            Ok((
                definition
                    .members()
                    .iter()
                    .map(|member| member.change_id().clone())
                    .collect(),
                None,
            ))
        }
        CandidateSelection::Stack {
            stack_id,
            expected_version,
        } => {
            let stack = load_stack_internal(connection, stack_id, None, true)?;
            if stack.version() != *expected_version {
                return Err(StoreError::StaleStackVersion {
                    expected: *expected_version,
                    actual: stack.version(),
                });
            }
            let changes = stack
                .definition()
                .members()
                .iter()
                .map(|member| member.change_id().clone())
                .collect();
            Ok((
                changes,
                Some(CandidateStackRef::new(
                    stack.id().clone(),
                    stack.version(),
                    stack.definition().policy(),
                )),
            ))
        }
    }
}

fn resolve_candidate_dependencies(
    connection: &Connection,
    inputs: &[CandidateInput],
) -> Result<Vec<ResolvedRequirement>, StoreError> {
    let positions = inputs
        .iter()
        .enumerate()
        .map(|(position, input)| (input.change_id(), position))
        .collect::<std::collections::HashMap<_, _>>();
    let mut statement = connection.prepare(
        "SELECT dependency_id FROM dependencies
         WHERE removed_at_unix_ms IS NULL ORDER BY dependency_id",
    )?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut requirements = Vec::new();
    for id in ids {
        let dependency_id = DependencyId::new(id)?;
        let dependency = load_dependency_internal(connection, &dependency_id, None, true)?;
        let Some(&downstream_position) = positions.get(dependency.downstream_change_id()) else {
            continue;
        };
        let Some(&upstream_position) = positions.get(dependency.upstream_change_id()) else {
            return Err(StoreError::CandidateMissingUpstream {
                dependency_id,
                upstream_change_id: dependency.upstream_change_id().clone(),
            });
        };
        if upstream_position >= downstream_position {
            return Err(StoreError::CandidateDependencyOrder(dependency_id));
        }
        let downstream = &inputs[downstream_position];
        let upstream = &inputs[upstream_position];
        if dependency.pins().downstream_revision_id() != downstream.revision_id()
            || dependency.pins().upstream_revision_id() != upstream.revision_id()
        {
            return Err(StoreError::StaleCandidateDependency(dependency_id));
        }
        requirements.push(ResolvedRequirement::new(
            ResolvedRequirementSource::Dependency {
                dependency_id,
                version: dependency.version(),
            },
            downstream.clone(),
            upstream.clone(),
        ));
    }
    Ok(requirements)
}

fn candidate_exists(
    connection: &Connection,
    candidate_id: &CandidateId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM composition_candidates WHERE candidate_id = ?1)",
        [candidate_id.as_str()],
        |row| row.get(0),
    )
}

fn persist_candidate(
    transaction: &Transaction<'_>,
    candidate: &CompositionCandidate,
    context: &MutationContext,
) -> Result<(), StoreError> {
    insert_operation_record(transaction, "candidate.created", context)?;
    transaction.execute(
        "INSERT INTO composition_candidates (
            candidate_id, repository_id, target_object_id, stack_id, stack_version,
            stack_policy, content_digest, created_at_unix_ms, created_by, operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            candidate.id().as_str(),
            candidate.target_base().repository_id().as_str(),
            candidate.target_base().object_id(),
            candidate.stack().map(|stack| stack.stack_id().as_str()),
            candidate.stack().map(|stack| stack.version().value()),
            candidate.stack().map(|stack| stack.policy().as_str()),
            candidate.content_digest().as_str(),
            candidate.created_at().value(),
            candidate.created_by().as_str(),
            context.operation_id,
        ],
    )?;
    for (position, input) in candidate.inputs().iter().enumerate() {
        transaction.execute(
            "INSERT INTO candidate_inputs (candidate_id, position, change_id, revision_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                candidate.id().as_str(),
                i64::try_from(position).map_err(|_| StoreError::CollectionTooLarge)?,
                input.change_id().as_str(),
                input.revision_id().as_str()
            ],
        )?;
    }
    for (index, requirement) in candidate.requirements().iter().enumerate() {
        let (source_kind, source_id, source_version, downstream_position) =
            match requirement.source() {
                ResolvedRequirementSource::Dependency {
                    dependency_id,
                    version,
                } => {
                    let position = candidate
                        .inputs()
                        .iter()
                        .position(|input| input == requirement.downstream())
                        .ok_or(StoreError::InvariantViolation(
                            "candidate requirement input is absent",
                        ))?;
                    (
                        "dependency",
                        dependency_id.as_str(),
                        version.value(),
                        position,
                    )
                }
                ResolvedRequirementSource::StackPredecessor {
                    stack_id,
                    version,
                    downstream_position,
                } => (
                    "stack_predecessor",
                    stack_id.as_str(),
                    version.value(),
                    *downstream_position,
                ),
            };
        transaction.execute(
            "INSERT INTO candidate_requirements (
                candidate_id, requirement_index, source_kind, source_id, source_version,
                downstream_position, downstream_change_id, downstream_revision_id,
                upstream_change_id, upstream_revision_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                candidate.id().as_str(),
                i64::try_from(index).map_err(|_| StoreError::CollectionTooLarge)?,
                source_kind,
                source_id,
                source_version,
                i64::try_from(downstream_position).map_err(|_| StoreError::CollectionTooLarge)?,
                requirement.downstream().change_id().as_str(),
                requirement.downstream().revision_id().as_str(),
                requirement.upstream().change_id().as_str(),
                requirement.upstream().revision_id().as_str()
            ],
        )?;
    }
    Ok(())
}

fn load_candidate_internal(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    candidate_id: &CandidateId,
) -> Result<CompositionCandidate, StoreError> {
    let row = connection
        .query_row(
            "SELECT repository_id, target_object_id, stack_id, stack_version, stack_policy,
                content_digest, created_at_unix_ms, created_by
         FROM composition_candidates WHERE candidate_id = ?1",
            [candidate_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::CandidateNotFound(candidate_id.clone()))?;
    let stack = match (row.2, row.3, row.4) {
        (None, None, None) => None,
        (Some(id), Some(version), Some(policy)) => Some(CandidateStackRef::new(
            StackId::new(id)?,
            StackVersion::new(version)?,
            StackPolicy::parse(&policy)?,
        )),
        _ => {
            return Err(StoreError::InvalidStoredData(
                "candidate Stack snapshot is incomplete".to_owned(),
            ));
        }
    };
    let inputs = read_candidate_inputs(connection, artifact_store, candidate_id)?;
    let requirements = read_candidate_requirements(connection, candidate_id)?;
    let candidate = CompositionCandidate::new(
        candidate_id.clone(),
        BaseState::new(RepositoryId::new(row.0)?, row.1)?,
        stack,
        inputs,
        requirements,
        UnixMillis::new(row.6)?,
        ActorId::new(row.7)?,
    )?;
    verify_candidate_sources(connection, &candidate)?;
    if candidate.content_digest().as_str() != row.5 {
        return Err(StoreError::InvalidStoredData(
            "candidate digest does not match canonical fields".to_owned(),
        ));
    }
    Ok(candidate)
}

fn verify_candidate_sources(
    connection: &Connection,
    candidate: &CompositionCandidate,
) -> Result<(), StoreError> {
    if let Some(stack_ref) = candidate.stack() {
        let stack = load_stack_internal(
            connection,
            stack_ref.stack_id(),
            Some(stack_ref.version()),
            false,
        )?;
        let recorded_changes = stack
            .definition()
            .members()
            .iter()
            .map(StackMember::change_id);
        if stack.version() != stack_ref.version()
            || stack.definition().policy() != stack_ref.policy()
            || !recorded_changes.eq(candidate.inputs().iter().map(CandidateInput::change_id))
        {
            return Err(StoreError::InvalidStoredData(
                "candidate inputs do not match the exact Stack snapshot".to_owned(),
            ));
        }
    }
    for requirement in candidate.requirements() {
        match requirement.source() {
            ResolvedRequirementSource::Dependency {
                dependency_id,
                version,
            } => {
                let dependency =
                    load_dependency_internal(connection, dependency_id, Some(*version), false)?;
                if dependency.version() != *version
                    || !dependency.is_active()
                    || dependency.downstream_change_id() != requirement.downstream().change_id()
                    || dependency.upstream_change_id() != requirement.upstream().change_id()
                    || dependency.pins().downstream_revision_id()
                        != requirement.downstream().revision_id()
                    || dependency.pins().upstream_revision_id()
                        != requirement.upstream().revision_id()
                {
                    return Err(StoreError::InvalidStoredData(
                        "candidate Dependency source does not match exact historical pins"
                            .to_owned(),
                    ));
                }
            }
            ResolvedRequirementSource::StackPredecessor { .. } => {}
        }
    }
    Ok(())
}

fn read_candidate_inputs(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    candidate_id: &CandidateId,
) -> Result<Vec<CandidateInput>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT position, change_id, revision_id FROM candidate_inputs
         WHERE candidate_id = ?1 ORDER BY position",
    )?;
    let rows = statement
        .query_map([candidate_id.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .enumerate()
        .map(|(expected, (position, change, revision))| {
            if position != i64::try_from(expected).map_err(|_| StoreError::CollectionTooLarge)? {
                return Err(StoreError::InvalidStoredData(
                    "candidate input positions are not contiguous".to_owned(),
                ));
            }
            let change_id = ChangeId::new(change)?;
            let revision_id = RevisionId::new(revision)?;
            let loaded = load_change_from_connection(connection, artifact_store, &change_id)?;
            if !loaded
                .revisions()
                .iter()
                .any(|value| value.revision_id() == &revision_id)
            {
                return Err(StoreError::RevisionNotFoundForChange {
                    change_id,
                    revision_id,
                });
            }
            Ok(CandidateInput::new(change_id, revision_id))
        })
        .collect()
}

fn read_candidate_requirements(
    connection: &Connection,
    candidate_id: &CandidateId,
) -> Result<Vec<ResolvedRequirement>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT requirement_index, source_kind, source_id, source_version,
                downstream_position, downstream_change_id, downstream_revision_id,
                upstream_change_id, upstream_revision_id
         FROM candidate_requirements WHERE candidate_id = ?1 ORDER BY requirement_index",
    )?;
    let rows = statement
        .query_map([candidate_id.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .enumerate()
        .map(|(expected, requirement)| {
            if requirement.0
                != i64::try_from(expected).map_err(|_| StoreError::CollectionTooLarge)?
            {
                return Err(StoreError::InvalidStoredData(
                    "candidate requirement indexes are not contiguous".to_owned(),
                ));
            }
            let downstream_position = usize::try_from(requirement.4).map_err(|_| {
                StoreError::InvalidStoredData("negative candidate position".to_owned())
            })?;
            let source = match requirement.1.as_str() {
                "dependency" => ResolvedRequirementSource::Dependency {
                    dependency_id: DependencyId::new(requirement.2)?,
                    version: RelationshipVersion::new(requirement.3)?,
                },
                "stack_predecessor" => ResolvedRequirementSource::StackPredecessor {
                    stack_id: StackId::new(requirement.2)?,
                    version: StackVersion::new(requirement.3)?,
                    downstream_position,
                },
                value => {
                    return Err(StoreError::InvalidStoredData(format!(
                        "invalid candidate requirement source: {value}"
                    )));
                }
            };
            Ok(ResolvedRequirement::new(
                source,
                CandidateInput::new(
                    ChangeId::new(requirement.5)?,
                    RevisionId::new(requirement.6)?,
                ),
                CandidateInput::new(
                    ChangeId::new(requirement.7)?,
                    RevisionId::new(requirement.8)?,
                ),
            ))
        })
        .collect()
}

fn candidate_operation_replay(
    transaction: &Transaction<'_>,
    artifact_store: &ArtifactStore,
    candidate_id: &CandidateId,
    target_base: &BaseState,
    selection: &CandidateSelection,
    context: &MutationContext,
) -> Result<Option<CompositionCandidate>, StoreError> {
    let Some(operation) = recorded_operation(transaction, context.operation_id())? else {
        return Ok(None);
    };
    if operation.event_kind != "candidate.created" || operation.actor_id != context.actor.as_str() {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    let recorded_id = transaction
        .query_row(
            "SELECT candidate_id FROM composition_candidates WHERE operation_id = ?1",
            [context.operation_id()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData("candidate operation has no outcome".to_owned())
        })?;
    if recorded_id != candidate_id.as_str() {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    let candidate = load_candidate_internal(transaction, artifact_store, candidate_id)?;
    let selection_matches = match selection {
        CandidateSelection::Changes(changes) => {
            candidate.stack().is_none()
                && candidate
                    .inputs()
                    .iter()
                    .map(CandidateInput::change_id)
                    .eq(changes)
        }
        CandidateSelection::Stack {
            stack_id,
            expected_version,
        } => candidate.stack().is_some_and(|stack| {
            stack.stack_id() == stack_id && stack.version() == *expected_version
        }),
    };
    if candidate.target_base() != target_base || !selection_matches {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    Ok(Some(candidate))
}

fn load_change_from_connection(
    connection: &Connection,
    artifact_store: &ArtifactStore,
    change_id: &ChangeId,
) -> Result<Change, StoreError> {
    let StoredHead::Found(stored_head) = read_head(connection, change_id)? else {
        return Err(StoreError::ChangeNotFound(change_id.clone()));
    };
    let mut statement = connection.prepare(
        "SELECT sequence, revision_id, parent_revision_id, repository_id,
                base_object_id, artifact_digest, created_at_unix_ms, created_by
         FROM change_revisions WHERE change_id = ?1 ORDER BY sequence",
    )?;
    let rows = statement.query_map([change_id.as_str()], |row| {
        Ok(StoredRevision {
            sequence: row.get(0)?,
            revision_id: row.get(1)?,
            parent_revision_id: row.get(2)?,
            repository_id: row.get(3)?,
            base_object_id: row.get(4)?,
            artifact_digest: row.get(5)?,
            created_at_unix_ms: row.get(6)?,
            created_by: row.get(7)?,
        })
    })?;

    let mut change = Change::new(change_id.clone());
    for (expected_sequence, row) in rows.enumerate() {
        let row = row?;
        let sequence = i64::try_from(expected_sequence).map_err(|_| {
            StoreError::InvalidStoredData("revision sequence exceeds i64".to_owned())
        })?;
        if row.sequence != sequence {
            return Err(StoreError::InvalidStoredData(format!(
                "non-contiguous revision sequence at {}",
                row.sequence
            )));
        }
        let expected_parent = change.head().cloned();
        let stored_parent = optional_revision_id(row.parent_revision_id)?;
        if stored_parent != expected_parent {
            return Err(StoreError::InvalidStoredData(
                "stored revision parent does not match the linear head".to_owned(),
            ));
        }
        let new_revision = NewRevision::new(
            RevisionId::new(row.revision_id)?,
            BaseState::new(RepositoryId::new(row.repository_id)?, row.base_object_id)?,
            ArtifactRef::tree_delta_v1(row.artifact_digest)?,
            UnixMillis::new(row.created_at_unix_ms)?,
            ActorId::new(row.created_by)?,
        );
        let artifact = artifact_store.load_manifest(new_revision.artifact())?;
        if artifact.base() != new_revision.base() {
            return Err(StoreError::ArtifactBaseMismatch);
        }
        change.append_revision(expected_parent.as_ref(), new_revision)?;
    }
    if change.head() != stored_head.as_ref() {
        return Err(StoreError::InvalidStoredData(
            "stored Change head does not match its revision history".to_owned(),
        ));
    }
    Ok(change)
}

fn validate_new_relationship(
    relationship: &Relationship,
    context: &MutationContext,
) -> Result<(), StoreError> {
    if relationship.version() != RelationshipVersion::INITIAL
        || !relationship.is_active()
        || relationship.created_at() != context.occurred_at
        || relationship.created_by() != &context.actor
    {
        return Err(StoreError::InvariantViolation(
            "new relationship must be active at version one with matching provenance",
        ));
    }
    Ok(())
}

fn ensure_change_exists(connection: &Connection, change_id: &ChangeId) -> Result<(), StoreError> {
    if change_exists(connection, change_id)? {
        Ok(())
    } else {
        Err(StoreError::ChangeNotFound(change_id.clone()))
    }
}

fn relationship_identity_exists(
    connection: &Connection,
    relationship_id: &RelationshipId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM relationships WHERE relationship_id = ?1
         )",
        [relationship_id.as_str()],
        |row| row.get(0),
    )
}

fn active_relationship_exists(
    connection: &Connection,
    kind: RelationshipKind,
    endpoints: &RelationshipEndpoints,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM relationships
            WHERE relationship_kind = ?1 AND first_change_id = ?2
              AND second_change_id = ?3 AND removed_at_unix_ms IS NULL
         )",
        params![
            kind.as_str(),
            endpoints.first().as_str(),
            endpoints.second().as_str()
        ],
        |row| row.get(0),
    )
}

struct StoredRelationship {
    kind: String,
    first: String,
    second: String,
    created_at: i64,
    created_by: String,
    version: i64,
    removed_at: Option<i64>,
    removed_by: Option<String>,
}

struct StoredRelationshipEvent {
    kind: String,
    first: String,
    second: String,
    expected_version: i64,
    resulting_version: i64,
    actor: String,
    occurred_at: i64,
}

fn read_stored_relationship(
    connection: &Connection,
    relationship_id: &RelationshipId,
) -> Result<StoredRelationship, StoreError> {
    connection
        .query_row(
            "SELECT relationship_kind, first_change_id, second_change_id,
                    created_at_unix_ms, created_by, version,
                    removed_at_unix_ms, removed_by
             FROM relationships WHERE relationship_id = ?1",
            [relationship_id.as_str()],
            |row| {
                Ok(StoredRelationship {
                    kind: row.get(0)?,
                    first: row.get(1)?,
                    second: row.get(2)?,
                    created_at: row.get(3)?,
                    created_by: row.get(4)?,
                    version: row.get(5)?,
                    removed_at: row.get(6)?,
                    removed_by: row.get(7)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::RelationshipNotFound(relationship_id.clone()))
}

fn read_stored_relationship_events(
    connection: &Connection,
    relationship_id: &RelationshipId,
) -> Result<Vec<StoredRelationshipEvent>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT event.event_kind, event.first_change_id, event.second_change_id,
                event.expected_version, event.resulting_version,
                operation.actor_id, operation.occurred_at_unix_ms
         FROM relationship_events AS event
         JOIN operation_records AS operation USING (operation_id)
         WHERE event.relationship_id = ?1 ORDER BY event.resulting_version",
    )?;
    statement
        .query_map([relationship_id.as_str()], |row| {
            Ok(StoredRelationshipEvent {
                kind: row.get(0)?,
                first: row.get(1)?,
                second: row.get(2)?,
                expected_version: row.get(3)?,
                resulting_version: row.get(4)?,
                actor: row.get(5)?,
                occurred_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Database)
}

fn load_relationship(
    connection: &Connection,
    relationship_id: &RelationshipId,
) -> Result<Relationship, StoreError> {
    let stored = read_stored_relationship(connection, relationship_id)?;
    let events = read_stored_relationship_events(connection, relationship_id)?;
    let Some(created) = events.first() else {
        return Err(StoreError::InvalidStoredData(
            "relationship has no creation event".to_owned(),
        ));
    };
    let endpoints = RelationshipEndpoints::new(
        ChangeId::new(stored.first.clone())?,
        ChangeId::new(stored.second.clone())?,
    )?;
    if endpoints.first().as_str() != stored.first
        || endpoints.second().as_str() != stored.second
        || created.kind != "relationship.created"
        || created.first != stored.first
        || created.second != stored.second
        || created.expected_version != 0
        || created.resulting_version != 1
        || created.actor != stored.created_by
        || created.occurred_at != stored.created_at
    {
        return Err(StoreError::InvalidStoredData(
            "relationship creation history does not match immutable identity".to_owned(),
        ));
    }
    let mut relationship = Relationship::new(
        relationship_id.clone(),
        RelationshipKind::parse(&stored.kind)?,
        endpoints,
        UnixMillis::new(stored.created_at)?,
        ActorId::new(stored.created_by.clone())?,
    );
    for event in events.iter().skip(1) {
        if event.kind != "relationship.removed"
            || event.first != stored.first
            || event.second != stored.second
        {
            return Err(StoreError::InvalidStoredData(
                "relationship has an invalid event sequence".to_owned(),
            ));
        }
        relationship.remove(
            RelationshipVersion::new(event.expected_version)?,
            UnixMillis::new(event.occurred_at)?,
            ActorId::new(event.actor.clone())?,
        )?;
        if relationship.version() != RelationshipVersion::new(event.resulting_version)? {
            return Err(StoreError::InvalidStoredData(
                "relationship event resulting version does not match lifecycle".to_owned(),
            ));
        }
    }
    if relationship.version() != RelationshipVersion::new(stored.version)?
        || relationship.removed_at() != stored.removed_at.map(UnixMillis::new).transpose()?
        || relationship.removed_by().map(ActorId::as_str) != stored.removed_by.as_deref()
    {
        return Err(StoreError::InvalidStoredData(
            "relationship projection does not match immutable event history".to_owned(),
        ));
    }
    Ok(relationship)
}

fn relationship_operation_is_replay(
    transaction: &Transaction<'_>,
    event_kind: &str,
    relationship_id: &RelationshipId,
    expected_version: RelationshipVersion,
    requested: Option<&Relationship>,
    context: &MutationContext,
) -> Result<bool, StoreError> {
    let Some(operation) = recorded_operation(transaction, context.operation_id())? else {
        return Ok(false);
    };
    if operation.event_kind != event_kind || operation.actor_id != context.actor.as_str() {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    let recorded = transaction
        .query_row(
            "SELECT event.relationship_id, event.expected_version,
                    event.first_change_id, event.second_change_id,
                    relationship.relationship_kind, relationship.created_at_unix_ms,
                    relationship.created_by, operation.occurred_at_unix_ms
             FROM relationship_events AS event
             JOIN relationships AS relationship USING (relationship_id)
             JOIN operation_records AS operation USING (operation_id)
             WHERE event.operation_id = ?1",
            [context.operation_id()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData(
                "operation record has no matching relationship event".to_owned(),
            )
        })?;
    if recorded.0 != relationship_id.as_str()
        || recorded.1 != expected_version.value()
        || recorded.7 != context.occurred_at.value()
    {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    if let Some(value) = requested
        && (recorded.2 != value.endpoints().first().as_str()
            || recorded.3 != value.endpoints().second().as_str()
            || recorded.4 != value.kind().as_str()
            || recorded.5 != value.created_at().value()
            || recorded.6 != value.created_by().as_str())
    {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    Ok(true)
}

fn insert_relationship_event(
    transaction: &Transaction<'_>,
    event_kind: &str,
    relationship: &Relationship,
    expected_version: RelationshipVersion,
    context: &MutationContext,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "INSERT INTO relationship_events (
            event_kind, relationship_id, first_change_id, second_change_id,
            expected_version, resulting_version, operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            event_kind,
            relationship.id().as_str(),
            relationship.endpoints().first().as_str(),
            relationship.endpoints().second().as_str(),
            expected_version.value(),
            relationship.version().value(),
            context.operation_id,
        ],
    )?;
    Ok(())
}

fn validate_new_dependency(
    dependency: &Dependency,
    context: &MutationContext,
) -> Result<(), StoreError> {
    if dependency.version() != RelationshipVersion::INITIAL
        || !dependency.is_active()
        || dependency.created_at() != context.occurred_at
        || dependency.created_by() != &context.actor
        || dependency.updated_at() != dependency.created_at()
        || dependency.updated_by() != dependency.created_by()
    {
        return Err(StoreError::InvariantViolation(
            "new dependency must be active at version one with matching provenance",
        ));
    }
    Ok(())
}

fn verify_dependency_pins(
    store: &SqliteStore,
    artifact_store: &ArtifactStore,
    dependency: &Dependency,
) -> Result<(), StoreError> {
    verify_exact_revision(
        store,
        artifact_store,
        dependency.downstream_change_id(),
        dependency.pins().downstream_revision_id(),
    )?;
    verify_exact_revision(
        store,
        artifact_store,
        dependency.upstream_change_id(),
        dependency.pins().upstream_revision_id(),
    )
}

fn verify_dependency_content(
    store: &SqliteStore,
    artifact_store: &ArtifactStore,
    dependency_id: &DependencyId,
) -> Result<(), StoreError> {
    let dependency = load_dependency_internal(&store.connection, dependency_id, None, true)?;
    verify_dependency_pins(store, artifact_store, &dependency)
}

fn dependency_identity_exists(
    connection: &Connection,
    dependency_id: &DependencyId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM dependencies WHERE dependency_id = ?1)",
        [dependency_id.as_str()],
        |row| row.get(0),
    )
}

fn active_dependency_exists(
    connection: &Connection,
    downstream_change_id: &ChangeId,
    upstream_change_id: &ChangeId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM dependencies
            WHERE downstream_change_id = ?1 AND upstream_change_id = ?2
              AND removed_at_unix_ms IS NULL
         )",
        params![downstream_change_id.as_str(), upstream_change_id.as_str()],
        |row| row.get(0),
    )
}

fn dependency_would_cycle(
    connection: &Connection,
    downstream_change_id: &ChangeId,
    upstream_change_id: &ChangeId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "WITH RECURSIVE reachable(change_id) AS (
            SELECT ?1
            UNION
            SELECT dependency.upstream_change_id
            FROM dependencies AS dependency
            JOIN reachable ON dependency.downstream_change_id = reachable.change_id
            WHERE dependency.removed_at_unix_ms IS NULL
         )
         SELECT EXISTS(SELECT 1 FROM reachable WHERE change_id = ?2)",
        params![upstream_change_id.as_str(), downstream_change_id.as_str()],
        |row| row.get(0),
    )
}

fn read_dependency_identity(
    connection: &Connection,
    dependency_id: &DependencyId,
) -> Result<(ChangeId, ChangeId), StoreError> {
    let value = connection
        .query_row(
            "SELECT downstream_change_id, upstream_change_id
             FROM dependencies WHERE dependency_id = ?1",
            [dependency_id.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| StoreError::DependencyNotFound(dependency_id.clone()))?;
    Ok((ChangeId::new(value.0)?, ChangeId::new(value.1)?))
}

fn read_dependency_pins(
    connection: &Connection,
    dependency_id: &DependencyId,
) -> Result<DependencyPins, StoreError> {
    let value = connection
        .query_row(
            "SELECT downstream_revision_id, upstream_revision_id
             FROM dependencies WHERE dependency_id = ?1",
            [dependency_id.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| StoreError::DependencyNotFound(dependency_id.clone()))?;
    Ok(DependencyPins::new(
        RevisionId::new(value.0)?,
        RevisionId::new(value.1)?,
    ))
}

struct StoredDependency {
    downstream_change_id: String,
    upstream_change_id: String,
    downstream_revision_id: String,
    upstream_revision_id: String,
    created_at: i64,
    created_by: String,
    version: i64,
    updated_at: i64,
    updated_by: String,
    removed_at: Option<i64>,
    removed_by: Option<String>,
}

struct StoredDependencyEvent {
    kind: String,
    downstream_change_id: String,
    upstream_change_id: String,
    expected_version: i64,
    resulting_version: i64,
    downstream_revision_id: String,
    upstream_revision_id: String,
    actor: String,
    occurred_at: i64,
}

fn read_stored_dependency(
    connection: &Connection,
    dependency_id: &DependencyId,
) -> Result<StoredDependency, StoreError> {
    connection
        .query_row(
            "SELECT downstream_change_id, upstream_change_id,
                    downstream_revision_id, upstream_revision_id,
                    created_at_unix_ms, created_by, version,
                    updated_at_unix_ms, updated_by,
                    removed_at_unix_ms, removed_by
             FROM dependencies WHERE dependency_id = ?1",
            [dependency_id.as_str()],
            |row| {
                Ok(StoredDependency {
                    downstream_change_id: row.get(0)?,
                    upstream_change_id: row.get(1)?,
                    downstream_revision_id: row.get(2)?,
                    upstream_revision_id: row.get(3)?,
                    created_at: row.get(4)?,
                    created_by: row.get(5)?,
                    version: row.get(6)?,
                    updated_at: row.get(7)?,
                    updated_by: row.get(8)?,
                    removed_at: row.get(9)?,
                    removed_by: row.get(10)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::DependencyNotFound(dependency_id.clone()))
}

fn read_stored_dependency_events(
    connection: &Connection,
    dependency_id: &DependencyId,
    through_version: Option<RelationshipVersion>,
) -> Result<Vec<StoredDependencyEvent>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT event.event_kind, event.downstream_change_id,
                event.upstream_change_id, event.expected_version,
                event.resulting_version, event.resulting_downstream_revision_id,
                event.resulting_upstream_revision_id, operation.actor_id,
                operation.occurred_at_unix_ms
         FROM dependency_events AS event
         JOIN operation_records AS operation USING (operation_id)
         WHERE event.dependency_id = ?1
           AND (?2 IS NULL OR event.resulting_version <= ?2)
         ORDER BY event.resulting_version",
    )?;
    statement
        .query_map(
            params![
                dependency_id.as_str(),
                through_version.map(RelationshipVersion::value)
            ],
            |row| {
                Ok(StoredDependencyEvent {
                    kind: row.get(0)?,
                    downstream_change_id: row.get(1)?,
                    upstream_change_id: row.get(2)?,
                    expected_version: row.get(3)?,
                    resulting_version: row.get(4)?,
                    downstream_revision_id: row.get(5)?,
                    upstream_revision_id: row.get(6)?,
                    actor: row.get(7)?,
                    occurred_at: row.get(8)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Database)
}

fn load_dependency_internal(
    connection: &Connection,
    dependency_id: &DependencyId,
    through_version: Option<RelationshipVersion>,
    compare_projection: bool,
) -> Result<Dependency, StoreError> {
    let stored = read_stored_dependency(connection, dependency_id)?;
    let events = read_stored_dependency_events(connection, dependency_id, through_version)?;
    let Some(created) = events.first() else {
        return Err(StoreError::InvalidStoredData(
            "dependency has no creation event".to_owned(),
        ));
    };
    if created.kind != "dependency.created"
        || created.downstream_change_id != stored.downstream_change_id
        || created.upstream_change_id != stored.upstream_change_id
        || created.expected_version != 0
        || created.resulting_version != 1
        || created.actor != stored.created_by
        || created.occurred_at != stored.created_at
    {
        return Err(StoreError::InvalidStoredData(
            "dependency creation history does not match immutable identity".to_owned(),
        ));
    }
    let mut dependency = Dependency::new(
        dependency_id.clone(),
        ChangeId::new(stored.downstream_change_id.clone())?,
        ChangeId::new(stored.upstream_change_id.clone())?,
        DependencyPins::new(
            RevisionId::new(created.downstream_revision_id.clone())?,
            RevisionId::new(created.upstream_revision_id.clone())?,
        ),
        UnixMillis::new(stored.created_at)?,
        ActorId::new(stored.created_by.clone())?,
    )?;
    for event in events.iter().skip(1) {
        if event.downstream_change_id != stored.downstream_change_id
            || event.upstream_change_id != stored.upstream_change_id
        {
            return Err(StoreError::InvalidStoredData(
                "dependency event changes immutable direction".to_owned(),
            ));
        }
        let expected = RelationshipVersion::new(event.expected_version)?;
        match event.kind.as_str() {
            "dependency.repinned" => dependency.repin(
                expected,
                DependencyPins::new(
                    RevisionId::new(event.downstream_revision_id.clone())?,
                    RevisionId::new(event.upstream_revision_id.clone())?,
                ),
                UnixMillis::new(event.occurred_at)?,
                ActorId::new(event.actor.clone())?,
            )?,
            "dependency.removed" => dependency.remove(
                expected,
                UnixMillis::new(event.occurred_at)?,
                ActorId::new(event.actor.clone())?,
            )?,
            _ => {
                return Err(StoreError::InvalidStoredData(
                    "dependency has an invalid event sequence".to_owned(),
                ));
            }
        }
        if dependency.version() != RelationshipVersion::new(event.resulting_version)?
            || dependency.pins().downstream_revision_id().as_str() != event.downstream_revision_id
            || dependency.pins().upstream_revision_id().as_str() != event.upstream_revision_id
        {
            return Err(StoreError::InvalidStoredData(
                "dependency event outcome does not match lifecycle".to_owned(),
            ));
        }
    }
    if compare_projection {
        validate_dependency_projection(&stored, &dependency)?;
    }
    Ok(dependency)
}

fn validate_dependency_projection(
    stored: &StoredDependency,
    dependency: &Dependency,
) -> Result<(), StoreError> {
    if dependency.pins().downstream_revision_id().as_str() != stored.downstream_revision_id
        || dependency.pins().upstream_revision_id().as_str() != stored.upstream_revision_id
        || dependency.version() != RelationshipVersion::new(stored.version)?
        || dependency.updated_at() != UnixMillis::new(stored.updated_at)?
        || dependency.updated_by().as_str() != stored.updated_by
        || dependency.removed_at() != stored.removed_at.map(UnixMillis::new).transpose()?
        || dependency.removed_by().map(ActorId::as_str) != stored.removed_by.as_deref()
    {
        return Err(StoreError::InvalidStoredData(
            "dependency projection does not match immutable event history".to_owned(),
        ));
    }
    Ok(())
}

fn dependency_operation_is_replay(
    transaction: &Transaction<'_>,
    event_kind: &str,
    dependency_id: &DependencyId,
    expected_version: RelationshipVersion,
    pins: &DependencyPins,
    requested: Option<&Dependency>,
    context: &MutationContext,
) -> Result<bool, StoreError> {
    let Some(operation) = recorded_operation(transaction, context.operation_id())? else {
        return Ok(false);
    };
    if operation.event_kind != event_kind || operation.actor_id != context.actor.as_str() {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    let recorded = transaction
        .query_row(
            "SELECT event.dependency_id, event.expected_version,
                    event.resulting_downstream_revision_id,
                    event.resulting_upstream_revision_id,
                    dependency.downstream_change_id, dependency.upstream_change_id,
                    dependency.created_at_unix_ms, dependency.created_by,
                    operation.occurred_at_unix_ms
             FROM dependency_events AS event
             JOIN dependencies AS dependency USING (dependency_id)
             JOIN operation_records AS operation USING (operation_id)
             WHERE event.operation_id = ?1",
            [context.operation_id()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData(
                "operation record has no matching dependency event".to_owned(),
            )
        })?;
    if recorded.0 != dependency_id.as_str()
        || recorded.1 != expected_version.value()
        || recorded.2 != pins.downstream_revision_id().as_str()
        || recorded.3 != pins.upstream_revision_id().as_str()
        || recorded.8 != context.occurred_at.value()
    {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    if let Some(value) = requested
        && (recorded.4 != value.downstream_change_id().as_str()
            || recorded.5 != value.upstream_change_id().as_str()
            || recorded.6 != value.created_at().value()
            || recorded.7 != value.created_by().as_str())
    {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    Ok(true)
}

fn load_dependency_operation_outcome(
    connection: &Connection,
    operation_id: &str,
) -> Result<Dependency, StoreError> {
    let (id, version) = connection
        .query_row(
            "SELECT dependency_id, resulting_version
             FROM dependency_events WHERE operation_id = ?1",
            [operation_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData(
                "operation record has no matching dependency outcome".to_owned(),
            )
        })?;
    load_dependency_internal(
        connection,
        &DependencyId::new(id)?,
        Some(RelationshipVersion::new(version)?),
        false,
    )
}

fn persist_dependency_transition(
    transaction: &Transaction<'_>,
    event_kind: &str,
    dependency: &Dependency,
    expected_version: RelationshipVersion,
    context: &MutationContext,
) -> Result<(), StoreError> {
    let updated = transaction.execute(
        "UPDATE dependencies
         SET downstream_revision_id = ?1, upstream_revision_id = ?2,
             version = ?3, updated_at_unix_ms = ?4, updated_by = ?5,
             removed_at_unix_ms = ?6, removed_by = ?7
         WHERE dependency_id = ?8 AND version = ?9
           AND removed_at_unix_ms IS NULL",
        params![
            dependency.pins().downstream_revision_id().as_str(),
            dependency.pins().upstream_revision_id().as_str(),
            dependency.version().value(),
            dependency.updated_at().value(),
            dependency.updated_by().as_str(),
            dependency.removed_at().map(UnixMillis::value),
            dependency.removed_by().map(ActorId::as_str),
            dependency.id().as_str(),
            expected_version.value(),
        ],
    )?;
    if updated != 1 {
        return Err(StoreError::InvariantViolation(
            "dependency compare-and-swap updated an unexpected number of rows",
        ));
    }
    insert_operation_record(transaction, event_kind, context)?;
    insert_dependency_event(
        transaction,
        event_kind,
        dependency,
        expected_version,
        context,
    )?;
    Ok(())
}

fn insert_dependency_event(
    transaction: &Transaction<'_>,
    event_kind: &str,
    dependency: &Dependency,
    expected_version: RelationshipVersion,
    context: &MutationContext,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "INSERT INTO dependency_events (
            event_kind, dependency_id, downstream_change_id, upstream_change_id,
            expected_version, resulting_version,
            resulting_downstream_revision_id, resulting_upstream_revision_id,
            operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            event_kind,
            dependency.id().as_str(),
            dependency.downstream_change_id().as_str(),
            dependency.upstream_change_id().as_str(),
            expected_version.value(),
            dependency.version().value(),
            dependency.pins().downstream_revision_id().as_str(),
            dependency.pins().upstream_revision_id().as_str(),
            context.operation_id,
        ],
    )?;
    Ok(())
}

fn verify_exact_revision(
    store: &SqliteStore,
    artifact_store: &ArtifactStore,
    change_id: &ChangeId,
    revision_id: &RevisionId,
) -> Result<(), StoreError> {
    let change = store.load_change(artifact_store, change_id)?;
    if change
        .revisions()
        .iter()
        .any(|revision| revision.revision_id() == revision_id)
    {
        Ok(())
    } else {
        Err(StoreError::RevisionNotFoundForChange {
            change_id: change_id.clone(),
            revision_id: revision_id.clone(),
        })
    }
}

fn verify_materialization_content(
    store: &SqliteStore,
    artifact_store: &ArtifactStore,
    materialization_id: &MaterializationId,
) -> Result<(), StoreError> {
    let binding = store
        .connection
        .query_row(
            "SELECT change_id, revision_id FROM materializations
             WHERE materialization_id = ?1",
            [materialization_id.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| StoreError::MaterializationNotFound(materialization_id.clone()))?;
    verify_exact_revision(
        store,
        artifact_store,
        &ChangeId::new(binding.0)?,
        &RevisionId::new(binding.1)?,
    )
}

fn materialization_identity_exists(
    connection: &Connection,
    materialization_id: &MaterializationId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM materializations WHERE materialization_id = ?1
         )",
        [materialization_id.as_str()],
        |row| row.get(0),
    )
}

struct MaterializationMutationRequest<'a> {
    event_kind: &'a str,
    materialization_id: &'a MaterializationId,
    expected_version: MaterializationVersion,
    state: MaterializationState,
    provider_ref: &'a ProviderRef,
    provider_evidence: &'a ProviderEvidence,
    creation: Option<&'a Materialization>,
}

fn materialization_operation_is_replay(
    transaction: &Transaction<'_>,
    request: &MaterializationMutationRequest<'_>,
    context: &MutationContext,
) -> Result<bool, StoreError> {
    let Some(operation) = recorded_operation(transaction, context.operation_id())? else {
        return Ok(false);
    };
    if operation.event_kind != request.event_kind || operation.actor_id != context.actor.as_str() {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    let recorded = transaction
        .query_row(
            "SELECT event.materialization_id, event.expected_version,
                    event.resulting_state, event.resulting_provider_ref,
                    event.provider_evidence,
                    materialization.change_id, materialization.revision_id,
                    materialization.workspace_id, materialization.provider_id,
                    materialization.created_at_unix_ms, materialization.created_by
             FROM materialization_events AS event
             JOIN materializations AS materialization
               USING (materialization_id, change_id)
             WHERE event.operation_id = ?1",
            [context.operation_id()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData(
                "operation record has no matching materialization event".to_owned(),
            )
        })?;
    if recorded.0 != request.materialization_id.as_str()
        || recorded.1 != request.expected_version.value()
        || recorded.2 != request.state.as_str()
        || recorded.3 != request.provider_ref.as_str()
        || recorded.4 != request.provider_evidence.as_str()
    {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    if let Some(created) = request.creation
        && (recorded.5 != created.change_id().as_str()
            || recorded.6 != created.revision_id().as_str()
            || recorded.7 != created.workspace_id().as_str()
            || recorded.8 != created.provider_id().as_str()
            || recorded.9 != created.created_at().value()
            || recorded.10 != created.created_by().as_str())
    {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    Ok(true)
}

fn insert_materialization_event(
    transaction: &Transaction<'_>,
    event_kind: &str,
    materialization: &Materialization,
    expected_version: MaterializationVersion,
    provider_evidence: &ProviderEvidence,
    context: &MutationContext,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "INSERT INTO materialization_events (
            event_kind, materialization_id, change_id, revision_id,
            expected_version, resulting_version, resulting_state,
            resulting_provider_ref, provider_evidence, operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            event_kind,
            materialization.id().as_str(),
            materialization.change_id().as_str(),
            materialization.revision_id().as_str(),
            expected_version.value(),
            materialization.version().value(),
            materialization.state().as_str(),
            materialization.provider_ref().as_str(),
            provider_evidence.as_str(),
            context.operation_id,
        ],
    )?;
    Ok(())
}

struct StoredMaterialization {
    change_id: String,
    revision_id: String,
    workspace_id: String,
    provider_id: String,
    current_provider_ref: String,
    state: String,
    version: i64,
    created_at: i64,
    created_by: String,
    state_changed_at: i64,
    released_at: Option<i64>,
}

fn read_stored_materialization(
    connection: &Connection,
    materialization_id: &MaterializationId,
) -> Result<StoredMaterialization, StoreError> {
    connection
        .query_row(
            "SELECT change_id, revision_id, workspace_id, provider_id,
                    current_provider_ref, state, version, created_at_unix_ms,
                    created_by, state_changed_at_unix_ms, released_at_unix_ms
             FROM materializations WHERE materialization_id = ?1",
            [materialization_id.as_str()],
            |row| {
                Ok(StoredMaterialization {
                    change_id: row.get(0)?,
                    revision_id: row.get(1)?,
                    workspace_id: row.get(2)?,
                    provider_id: row.get(3)?,
                    current_provider_ref: row.get(4)?,
                    state: row.get(5)?,
                    version: row.get(6)?,
                    created_at: row.get(7)?,
                    created_by: row.get(8)?,
                    state_changed_at: row.get(9)?,
                    released_at: row.get(10)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::MaterializationNotFound(materialization_id.clone()))
}

struct StoredMaterializationEvent {
    kind: String,
    change_id: String,
    revision_id: String,
    expected_version: i64,
    resulting_version: i64,
    state: String,
    provider_ref: String,
    provider_evidence: String,
    actor: String,
    occurred_at: i64,
}

fn read_stored_materialization_events(
    connection: &Connection,
    materialization_id: &MaterializationId,
    through_version: Option<MaterializationVersion>,
) -> Result<Vec<StoredMaterializationEvent>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT event.event_kind, event.change_id, event.revision_id,
                event.expected_version, event.resulting_version,
                event.resulting_state, event.resulting_provider_ref,
                event.provider_evidence, operation.actor_id,
                operation.occurred_at_unix_ms
         FROM materialization_events AS event
         JOIN operation_records AS operation USING (operation_id)
         WHERE event.materialization_id = ?1
           AND (?2 IS NULL OR event.resulting_version <= ?2)
         ORDER BY event.resulting_version",
    )?;
    statement
        .query_map(
            params![
                materialization_id.as_str(),
                through_version.map(MaterializationVersion::value)
            ],
            |row| {
                Ok(StoredMaterializationEvent {
                    kind: row.get(0)?,
                    change_id: row.get(1)?,
                    revision_id: row.get(2)?,
                    expected_version: row.get(3)?,
                    resulting_version: row.get(4)?,
                    state: row.get(5)?,
                    provider_ref: row.get(6)?,
                    provider_evidence: row.get(7)?,
                    actor: row.get(8)?,
                    occurred_at: row.get(9)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Database)
}

fn load_materialization_operation_outcome(
    connection: &Connection,
    operation_id: &str,
) -> Result<Materialization, StoreError> {
    let (id, version) = connection
        .query_row(
            "SELECT materialization_id, resulting_version
             FROM materialization_events WHERE operation_id = ?1",
            [operation_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData(
                "operation record has no matching materialization outcome".to_owned(),
            )
        })?;
    load_materialization_internal(
        connection,
        &MaterializationId::new(id)?,
        Some(MaterializationVersion::new(version)?),
        false,
    )
}

fn load_materialization_internal(
    connection: &Connection,
    materialization_id: &MaterializationId,
    through_version: Option<MaterializationVersion>,
    compare_projection: bool,
) -> Result<Materialization, StoreError> {
    let stored = read_stored_materialization(connection, materialization_id)?;
    let events =
        read_stored_materialization_events(connection, materialization_id, through_version)?;
    let Some(created) = events.first() else {
        return Err(StoreError::InvalidStoredData(
            "materialization has no creation event".to_owned(),
        ));
    };
    validate_materialization_creation(&stored, created)?;
    let mut materialization = Materialization::new(
        materialization_id.clone(),
        ChangeId::new(stored.change_id.clone())?,
        RevisionId::new(stored.revision_id.clone())?,
        MaterializationPlacement::new(
            WorkspaceId::new(stored.workspace_id.clone())?,
            ProviderId::new(stored.provider_id.clone())?,
            ProviderRef::new(created.provider_ref.clone())?,
        ),
        UnixMillis::new(stored.created_at)?,
        ActorId::new(stored.created_by.clone())?,
    );
    for event in events.iter().skip(1) {
        apply_stored_materialization_event(&mut materialization, event)?;
    }
    if compare_projection {
        validate_materialization_projection(&stored, &materialization)?;
    }
    Ok(materialization)
}

fn validate_materialization_creation(
    stored: &StoredMaterialization,
    event: &StoredMaterializationEvent,
) -> Result<(), StoreError> {
    ProviderEvidence::new(event.provider_evidence.clone())?;
    if event.kind != "materialization.created"
        || event.change_id != stored.change_id
        || event.revision_id != stored.revision_id
        || event.expected_version != 0
        || event.resulting_version != 1
        || event.state != "clean"
        || event.actor != stored.created_by
        || event.occurred_at != stored.created_at
    {
        return Err(StoreError::InvalidStoredData(
            "materialization creation event does not match immutable identity".to_owned(),
        ));
    }
    Ok(())
}

fn apply_stored_materialization_event(
    materialization: &mut Materialization,
    event: &StoredMaterializationEvent,
) -> Result<(), StoreError> {
    ProviderEvidence::new(event.provider_evidence.clone())?;
    if event.kind != "materialization.transitioned"
        || event.change_id != materialization.change_id().as_str()
        || event.revision_id != materialization.revision_id().as_str()
    {
        return Err(StoreError::InvalidStoredData(
            "materialization has an invalid event sequence".to_owned(),
        ));
    }
    materialization.transition(
        MaterializationVersion::new(event.expected_version)?,
        MaterializationState::parse(&event.state)?,
        ProviderRef::new(event.provider_ref.clone())?,
        UnixMillis::new(event.occurred_at)?,
    )?;
    if materialization.version() != MaterializationVersion::new(event.resulting_version)? {
        return Err(StoreError::InvalidStoredData(
            "materialization event resulting version does not match lifecycle".to_owned(),
        ));
    }
    Ok(())
}

fn validate_materialization_projection(
    stored: &StoredMaterialization,
    materialization: &Materialization,
) -> Result<(), StoreError> {
    if materialization.provider_ref().as_str() != stored.current_provider_ref
        || materialization.state().as_str() != stored.state
        || materialization.version() != MaterializationVersion::new(stored.version)?
        || materialization.state_changed_at() != UnixMillis::new(stored.state_changed_at)?
        || materialization.released_at() != stored.released_at.map(UnixMillis::new).transpose()?
    {
        return Err(StoreError::InvalidStoredData(
            "materialization projection does not match immutable event history".to_owned(),
        ));
    }
    Ok(())
}

fn load_assignment(
    connection: &Connection,
    assignment_id: &AssignmentId,
) -> Result<Assignment, StoreError> {
    let row = connection
        .query_row(
            "SELECT change_id, subject_kind, subject_id, role, assigned_at_unix_ms,
                    assigned_by, version, released_at_unix_ms, released_by
             FROM assignments WHERE assignment_id = ?1",
            [assignment_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::AssignmentNotFound(assignment_id.clone()))?;
    let (
        change,
        subject_kind,
        subject_id,
        role,
        assigned_at,
        assigned_by,
        version,
        released_at,
        released_by,
    ) = row;
    let mut assignment = Assignment::new(
        assignment_id.clone(),
        ChangeId::new(change)?,
        Subject::new(
            SubjectKind::parse(&subject_kind)?,
            SubjectId::new(subject_id)?,
        ),
        AssignmentRole::parse(&role)?,
        UnixMillis::new(assigned_at)?,
        ActorId::new(assigned_by)?,
    );
    match (released_at, released_by) {
        (None, None) => {}
        (Some(at), Some(actor)) => assignment.release(
            CoordinationVersion::INITIAL,
            UnixMillis::new(at)?,
            ActorId::new(actor)?,
        )?,
        _ => {
            return Err(StoreError::InvalidStoredData(
                "assignment release provenance is incomplete".to_owned(),
            ));
        }
    }
    if assignment.version() != CoordinationVersion::new(version)? {
        return Err(StoreError::InvalidStoredData(
            "assignment version does not match its lifecycle".to_owned(),
        ));
    }
    validate_assignment_events(connection, &assignment)?;
    Ok(assignment)
}

fn validate_assignment_events(
    connection: &Connection,
    assignment: &Assignment,
) -> Result<(), StoreError> {
    let mut statement = connection.prepare(
        "SELECT event.event_kind, event.expected_version, event.resulting_version,
                operation.actor_id, operation.occurred_at_unix_ms
         FROM assignment_events AS event
         JOIN operation_records AS operation USING (operation_id)
         WHERE event.assignment_id = ?1 ORDER BY event.resulting_version",
    )?;
    let events = statement
        .query_map([assignment.id().as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let assigned = (
        "assignment.assigned".to_owned(),
        0,
        1,
        assignment.assigned_by().as_str().to_owned(),
        assignment.assigned_at().value(),
    );
    let valid = match (assignment.released_at(), assignment.released_by()) {
        (None, None) => events.as_slice() == [assigned],
        (Some(released_at), Some(released_by)) => {
            let released = (
                "assignment.released".to_owned(),
                1,
                2,
                released_by.as_str().to_owned(),
                released_at.value(),
            );
            events.as_slice() == [assigned, released]
        }
        _ => false,
    };
    if !valid {
        return Err(StoreError::InvalidStoredData(
            "assignment projection does not match its immutable event history".to_owned(),
        ));
    }
    Ok(())
}

fn insert_assignment_event(
    transaction: &Transaction<'_>,
    event_kind: &str,
    assignment: &Assignment,
    expected_version: CoordinationVersion,
    context: &MutationContext,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "INSERT INTO assignment_events (
            event_kind, assignment_id, change_id, expected_version,
            resulting_version, operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            event_kind,
            assignment.id().as_str(),
            assignment.change_id().as_str(),
            expected_version.value(),
            assignment.version().value(),
            context.operation_id,
        ],
    )?;
    Ok(())
}

fn assignment_operation_is_replay(
    transaction: &Transaction<'_>,
    event_kind: &str,
    assignment_id: &AssignmentId,
    expected_version: CoordinationVersion,
    requested_assignment: Option<&Assignment>,
    context: &MutationContext,
) -> Result<bool, StoreError> {
    let Some(operation) = recorded_operation(transaction, context.operation_id())? else {
        return Ok(false);
    };
    if operation.event_kind != event_kind || operation.actor_id != context.actor.as_str() {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    let event = transaction
        .query_row(
            "SELECT assignment_id, expected_version FROM assignment_events
             WHERE operation_id = ?1",
            [context.operation_id()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData(
                "operation record has no matching assignment event".to_owned(),
            )
        })?;
    if event.0 != assignment_id.as_str() || event.1 != expected_version.value() {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    if let Some(requested) = requested_assignment {
        let stored = load_assignment(transaction, assignment_id)?;
        if stored.change_id() != requested.change_id()
            || stored.subject() != requested.subject()
            || stored.role() != requested.role()
            || stored.assigned_at() != requested.assigned_at()
            || stored.assigned_by() != requested.assigned_by()
        {
            return Err(StoreError::OperationIdConflict(
                context.operation_id.clone(),
            ));
        }
    }
    Ok(true)
}

#[derive(Debug)]
struct LeaseScopeState {
    version: CoordinationVersion,
    current_lease_id: Option<LeaseId>,
    current_expires_at: Option<UnixMillis>,
}

fn read_lease_scope(
    connection: &Connection,
    scope: &LeaseScope,
) -> Result<Option<LeaseScopeState>, StoreError> {
    let row = connection
        .query_row(
            "SELECT version, current_lease_id, current_expires_at_unix_ms
             FROM lease_scopes WHERE change_id = ?1 AND operation_key = ?2",
            params![scope.change_id().as_str(), scope.operation().as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .optional()?;
    row.map(|(version, lease, expiry)| {
        Ok(LeaseScopeState {
            version: CoordinationVersion::new(version)?,
            current_lease_id: lease.map(LeaseId::new).transpose()?,
            current_expires_at: expiry.map(UnixMillis::new).transpose()?,
        })
    })
    .transpose()
}

fn validate_lease_scope_events(
    connection: &Connection,
    scope: &LeaseScope,
    state: &LeaseScopeState,
) -> Result<Option<LeaseId>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT lease_id, event_kind, expected_version, resulting_version,
                resulting_expires_at_unix_ms
         FROM lease_events
         WHERE change_id = ?1 AND operation_key = ?2
         ORDER BY resulting_version",
    )?;
    let events = statement
        .query_map(
            params![scope.change_id().as_str(), scope.operation().as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let expected_count = usize::try_from(state.version.value()).map_err(|_| {
        StoreError::InvalidStoredData("lease scope version exceeds usize".to_owned())
    })?;
    if events.len() != expected_count
        || events.iter().enumerate().any(|(index, event)| {
            i64::try_from(index).map_or(true, |expected| {
                event.2 != expected || event.3 != expected.saturating_add(1)
            })
        })
    {
        return Err(StoreError::InvalidStoredData(
            "lease scope version is not backed by a contiguous event history".to_owned(),
        ));
    }
    let Some(final_event) = events.last() else {
        if state.current_lease_id.is_some() || state.current_expires_at.is_some() {
            return Err(StoreError::InvalidStoredData(
                "zero-version lease scope has current authority".to_owned(),
            ));
        }
        return Ok(None);
    };
    let projection_matches = match (&state.current_lease_id, state.current_expires_at) {
        (Some(current), Some(expiry)) => {
            final_event.0 == current.as_str()
                && final_event.1 != "lease.released"
                && final_event.4 == Some(expiry.value())
        }
        (None, None) => final_event.1 == "lease.released" && final_event.4.is_none(),
        _ => false,
    };
    if !projection_matches {
        return Err(StoreError::InvalidStoredData(
            "lease scope projection does not match its final immutable event".to_owned(),
        ));
    }
    Ok(Some(LeaseId::new(final_event.0.clone())?))
}

fn read_lease_identity(
    connection: &Connection,
    lease_id: &LeaseId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM leases WHERE lease_id = ?1)",
        [lease_id.as_str()],
        |row| row.get(0),
    )
}

fn persist_lease_acquisition(
    transaction: &Transaction<'_>,
    lease: &Lease,
    expected_version: CoordinationVersion,
    context: &MutationContext,
) -> Result<(), StoreError> {
    let scope = lease.scope();
    transaction.execute(
        "INSERT INTO lease_scopes (change_id, operation_key, version)
         VALUES (?1, ?2, 0) ON CONFLICT (change_id, operation_key) DO NOTHING",
        params![scope.change_id().as_str(), scope.operation().as_str()],
    )?;
    transaction.execute(
        "INSERT INTO leases (
            lease_id, change_id, operation_key, holder_kind, holder_id,
            predecessor_lease_id, acquired_at_unix_ms, initial_expires_at_unix_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            lease.id().as_str(),
            scope.change_id().as_str(),
            scope.operation().as_str(),
            lease.holder().kind().as_str(),
            lease.holder().id().as_str(),
            lease.predecessor().map(LeaseId::as_str),
            lease.acquired_at().value(),
            lease.expires_at().value(),
        ],
    )?;
    let updated = transaction.execute(
        "UPDATE lease_scopes
         SET version = ?1, current_lease_id = ?2, current_expires_at_unix_ms = ?3
         WHERE change_id = ?4 AND operation_key = ?5 AND version = ?6",
        params![
            lease.version().value(),
            lease.id().as_str(),
            lease.expires_at().value(),
            scope.change_id().as_str(),
            scope.operation().as_str(),
            expected_version.value(),
        ],
    )?;
    if updated != 1 {
        return Err(StoreError::InvariantViolation(
            "lease acquisition compare-and-swap updated an unexpected number of rows",
        ));
    }
    let event_kind = if lease.predecessor().is_some() {
        "lease.reclaimed"
    } else {
        "lease.acquired"
    };
    insert_operation_record(transaction, event_kind, context)?;
    insert_lease_event(
        transaction,
        event_kind,
        lease,
        expected_version,
        Some(lease.expires_at()),
        context,
    )?;
    Ok(())
}

fn load_lease(connection: &Connection, lease_id: &LeaseId) -> Result<Lease, StoreError> {
    load_lease_through(connection, lease_id, None)
}

fn load_lease_operation_outcome(
    connection: &Connection,
    operation_id: &str,
) -> Result<Lease, StoreError> {
    let (lease_id, resulting_version) = connection
        .query_row(
            "SELECT lease_id, resulting_version FROM lease_events WHERE operation_id = ?1",
            [operation_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData(
                "operation record has no matching lease outcome".to_owned(),
            )
        })?;
    load_lease_through(
        connection,
        &LeaseId::new(lease_id)?,
        Some(CoordinationVersion::new(resulting_version)?),
    )
}

fn load_lease_through(
    connection: &Connection,
    lease_id: &LeaseId,
    through_version: Option<CoordinationVersion>,
) -> Result<Lease, StoreError> {
    let stored = read_stored_lease(connection, lease_id)?;
    let events = read_stored_lease_events(connection, lease_id, through_version)?;
    let Some(first) = events.first() else {
        return Err(StoreError::InvalidStoredData(
            "lease has no acquisition event".to_owned(),
        ));
    };
    validate_acquisition_event(&stored, first)?;
    let mut lease = Lease::new(
        lease_id.clone(),
        LeaseScope::new(
            ChangeId::new(stored.change_id)?,
            LeaseOperation::new(stored.operation)?,
        ),
        Subject::new(
            SubjectKind::parse(&stored.holder_kind)?,
            SubjectId::new(stored.holder_id)?,
        ),
        stored.predecessor.map(LeaseId::new).transpose()?,
        UnixMillis::new(stored.acquired_at)?,
        UnixMillis::new(stored.initial_expiry)?,
        CoordinationVersion::new(first.resulting_version)?,
    )?;
    for event in events.iter().skip(1) {
        apply_stored_lease_event(&mut lease, event)?;
    }
    Ok(lease)
}

struct StoredLease {
    change_id: String,
    operation: String,
    holder_kind: String,
    holder_id: String,
    predecessor: Option<String>,
    acquired_at: i64,
    initial_expiry: i64,
}

fn read_stored_lease(
    connection: &Connection,
    lease_id: &LeaseId,
) -> Result<StoredLease, StoreError> {
    connection
        .query_row(
            "SELECT change_id, operation_key, holder_kind, holder_id,
                    predecessor_lease_id, acquired_at_unix_ms, initial_expires_at_unix_ms
             FROM leases WHERE lease_id = ?1",
            [lease_id.as_str()],
            |row| {
                Ok(StoredLease {
                    change_id: row.get(0)?,
                    operation: row.get(1)?,
                    holder_kind: row.get(2)?,
                    holder_id: row.get(3)?,
                    predecessor: row.get(4)?,
                    acquired_at: row.get(5)?,
                    initial_expiry: row.get(6)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::LeaseNotFound(lease_id.clone()))
}

struct StoredLeaseEvent {
    kind: String,
    expected_version: i64,
    resulting_version: i64,
    expiry: Option<i64>,
    occurred_at: i64,
}

fn read_stored_lease_events(
    connection: &Connection,
    lease_id: &LeaseId,
    through_version: Option<CoordinationVersion>,
) -> Result<Vec<StoredLeaseEvent>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT event.event_kind, event.expected_version, event.resulting_version,
                event.resulting_expires_at_unix_ms, operation.occurred_at_unix_ms
         FROM lease_events AS event
         JOIN operation_records AS operation USING (operation_id)
         WHERE event.lease_id = ?1
           AND (?2 IS NULL OR event.resulting_version <= ?2)
         ORDER BY event.resulting_version",
    )?;
    statement
        .query_map(
            params![
                lease_id.as_str(),
                through_version.map(CoordinationVersion::value)
            ],
            |row| {
                Ok(StoredLeaseEvent {
                    kind: row.get(0)?,
                    expected_version: row.get(1)?,
                    resulting_version: row.get(2)?,
                    expiry: row.get(3)?,
                    occurred_at: row.get(4)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Database)
}

fn validate_acquisition_event(
    stored: &StoredLease,
    first: &StoredLeaseEvent,
) -> Result<(), StoreError> {
    if !matches!(first.kind.as_str(), "lease.acquired" | "lease.reclaimed")
        || first.expiry != Some(stored.initial_expiry)
        || first.occurred_at != stored.acquired_at
        || (first.kind == "lease.reclaimed") != stored.predecessor.is_some()
        || first.expected_version.checked_add(1) != Some(first.resulting_version)
    {
        return Err(StoreError::InvalidStoredData(
            "lease acquisition event does not match immutable lease data".to_owned(),
        ));
    }
    Ok(())
}

fn apply_stored_lease_event(lease: &mut Lease, event: &StoredLeaseEvent) -> Result<(), StoreError> {
    let expected = CoordinationVersion::new(event.expected_version)?;
    match event.kind.as_str() {
        "lease.renewed" => lease.renew(
            expected,
            UnixMillis::new(event.occurred_at)?,
            UnixMillis::new(event.expiry.ok_or_else(|| {
                StoreError::InvalidStoredData("lease renewal has no expiry".to_owned())
            })?)?,
        )?,
        "lease.released" => lease.release(expected, UnixMillis::new(event.occurred_at)?)?,
        _ => {
            return Err(StoreError::InvalidStoredData(
                "lease has an invalid event sequence".to_owned(),
            ));
        }
    }
    if lease.version() != CoordinationVersion::new(event.resulting_version)? {
        return Err(StoreError::InvalidStoredData(
            "lease event resulting version does not match lifecycle".to_owned(),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lease_operation_is_replay(
    transaction: &Transaction<'_>,
    allowed_event_kinds: &[&str],
    lease_id: &LeaseId,
    scope: &LeaseScope,
    expected_version: CoordinationVersion,
    holder: Option<&Subject>,
    resulting_expiry: Option<UnixMillis>,
    context: &MutationContext,
) -> Result<bool, StoreError> {
    let Some(operation) = recorded_operation(transaction, context.operation_id())? else {
        return Ok(false);
    };
    if !allowed_event_kinds.contains(&operation.event_kind.as_str())
        || operation.actor_id != context.actor.as_str()
    {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    let row = transaction
        .query_row(
            "SELECT event.lease_id, event.change_id, event.operation_key,
                    event.expected_version, event.resulting_expires_at_unix_ms,
                    lease.holder_kind, lease.holder_id
             FROM lease_events AS event
             JOIN leases AS lease USING (lease_id, change_id, operation_key)
             WHERE event.operation_id = ?1",
            [context.operation_id()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StoreError::InvalidStoredData("operation record has no matching lease event".to_owned())
        })?;
    let requested_holder = holder.map(|value| (value.kind().as_str(), value.id().as_str()));
    if row.0 != lease_id.as_str()
        || row.1 != scope.change_id().as_str()
        || row.2 != scope.operation().as_str()
        || row.3 != expected_version.value()
        || row.4 != resulting_expiry.map(UnixMillis::value)
        || requested_holder.is_some_and(|value| value != (row.5.as_str(), row.6.as_str()))
    {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    Ok(true)
}

fn insert_lease_event(
    transaction: &Transaction<'_>,
    event_kind: &str,
    lease: &Lease,
    expected_version: CoordinationVersion,
    resulting_expiry: Option<UnixMillis>,
    context: &MutationContext,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "INSERT INTO lease_events (
            event_kind, lease_id, change_id, operation_key, expected_version,
            resulting_version, resulting_expires_at_unix_ms, operation_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            event_kind,
            lease.id().as_str(),
            lease.scope().change_id().as_str(),
            lease.scope().operation().as_str(),
            expected_version.value(),
            lease.version().value(),
            resulting_expiry.map(UnixMillis::value),
            context.operation_id,
        ],
    )?;
    Ok(())
}

fn next_version(version: CoordinationVersion) -> Result<CoordinationVersion, StoreError> {
    let value = version
        .value()
        .checked_add(1)
        .ok_or(CoordinationError::VersionExhausted)?;
    Ok(CoordinationVersion::new(value)?)
}

fn enable_wal(connection: &Connection) -> Result<String, rusqlite::Error> {
    let deadline = Instant::now() + BUSY_TIMEOUT;
    loop {
        match connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0)) {
            Ok(mode) => return Ok(mode),
            Err(error) if is_busy(&error) && Instant::now() < deadline => {
                std::thread::sleep(BUSY_RETRY_INTERVAL);
            }
            Err(error) => return Err(error),
        }
    }
}

fn begin_immediate_with_retry(connection: &Connection) -> Result<(), rusqlite::Error> {
    let deadline = Instant::now() + BUSY_TIMEOUT;
    loop {
        match connection.execute_batch("BEGIN IMMEDIATE") {
            Ok(()) => return Ok(()),
            Err(error) if is_busy(&error) && Instant::now() < deadline => {
                std::thread::sleep(BUSY_RETRY_INTERVAL);
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

#[derive(Debug)]
struct StoredRevision {
    sequence: i64,
    revision_id: String,
    parent_revision_id: Option<String>,
    repository_id: String,
    base_object_id: String,
    artifact_digest: String,
    created_at_unix_ms: i64,
    created_by: String,
}

fn read_head(connection: &Connection, change_id: &ChangeId) -> Result<StoredHead, StoreError> {
    let value = connection
        .query_row(
            "SELECT head_revision_id FROM changes WHERE change_id = ?1",
            [change_id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?;
    match value {
        None => Ok(StoredHead::NotFound),
        Some(head) => Ok(StoredHead::Found(optional_revision_id(head)?)),
    }
}

enum StoredHead {
    NotFound,
    Found(Option<RevisionId>),
}

fn optional_revision_id(value: Option<String>) -> Result<Option<RevisionId>, ChangeError> {
    value.map(RevisionId::new).transpose()
}

fn change_exists(connection: &Connection, change_id: &ChangeId) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM changes WHERE change_id = ?1)",
        [change_id.as_str()],
        |row| row.get(0),
    )
}

fn revision_exists(
    connection: &Connection,
    revision_id: &RevisionId,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM change_revisions WHERE revision_id = ?1)",
        [revision_id.as_str()],
        |row| row.get(0),
    )
}

#[derive(Debug, Eq, PartialEq)]
struct RecordedOperation {
    event_kind: String,
    change_id: String,
    revision_id: Option<String>,
    expected_head: Option<String>,
    resulting_head: Option<String>,
    repository_id: Option<String>,
    base_object_id: Option<String>,
    artifact_version: Option<String>,
    artifact_digest: Option<String>,
    revision_created_at: Option<i64>,
    revision_created_by: Option<String>,
    actor_id: String,
}

#[allow(clippy::too_many_arguments)]
fn operation_is_replay(
    transaction: &Transaction<'_>,
    event_kind: &str,
    change_id: &ChangeId,
    revision: Option<&NewRevision>,
    expected_head: Option<&RevisionId>,
    resulting_head: Option<&RevisionId>,
    context: &MutationContext,
) -> Result<bool, StoreError> {
    let registered = recorded_operation(transaction, context.operation_id())?;
    let Some(registered) = registered else {
        return Ok(false);
    };
    if registered.event_kind != event_kind || registered.actor_id != context.actor.as_str() {
        return Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ));
    }
    let recorded = transaction
        .query_row(
            "SELECT a.event_kind, a.change_id, a.revision_id,
                    a.expected_head_revision_id, a.resulting_head_revision_id,
                    r.repository_id, r.base_object_id, r.artifact_version,
                    r.artifact_digest, r.created_at_unix_ms, r.created_by,
                    a.actor_id
             FROM audit_events AS a
             LEFT JOIN change_revisions AS r
               ON r.revision_id = a.revision_id AND r.change_id = a.change_id
             WHERE a.operation_id = ?1",
            [context.operation_id.as_str()],
            |row| {
                Ok(RecordedOperation {
                    event_kind: row.get(0)?,
                    change_id: row.get(1)?,
                    revision_id: row.get(2)?,
                    expected_head: row.get(3)?,
                    resulting_head: row.get(4)?,
                    repository_id: row.get(5)?,
                    base_object_id: row.get(6)?,
                    artifact_version: row.get(7)?,
                    artifact_digest: row.get(8)?,
                    revision_created_at: row.get(9)?,
                    revision_created_by: row.get(10)?,
                    actor_id: row.get(11)?,
                })
            },
        )
        .optional()?;
    let Some(recorded) = recorded else {
        return Err(StoreError::InvalidStoredData(
            "operation record has no matching Change audit event".to_owned(),
        ));
    };
    let requested = RecordedOperation {
        event_kind: event_kind.to_owned(),
        change_id: change_id.as_str().to_owned(),
        revision_id: revision.map(|value| value.revision_id().as_str().to_owned()),
        expected_head: expected_head.map(|id| id.as_str().to_owned()),
        resulting_head: resulting_head.map(|id| id.as_str().to_owned()),
        repository_id: revision.map(|value| value.base().repository_id().as_str().to_owned()),
        base_object_id: revision.map(|value| value.base().object_id().to_owned()),
        artifact_version: revision.map(|value| value.artifact().version().to_owned()),
        artifact_digest: revision.map(|value| value.artifact().manifest_digest().to_owned()),
        revision_created_at: revision.map(|value| value.created_at().value()),
        revision_created_by: revision.map(|value| value.created_by().as_str().to_owned()),
        actor_id: context.actor.as_str().to_owned(),
    };
    if recorded == requested {
        Ok(true)
    } else {
        Err(StoreError::OperationIdConflict(
            context.operation_id.clone(),
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_audit_event(
    transaction: &Transaction<'_>,
    event_kind: &str,
    change_id: &ChangeId,
    revision: Option<&NewRevision>,
    expected_head: Option<&RevisionId>,
    resulting_head: Option<&RevisionId>,
    context: &MutationContext,
) -> Result<(), rusqlite::Error> {
    insert_operation_record(transaction, event_kind, context)?;
    transaction.execute(
        "INSERT INTO audit_events (
            event_kind, change_id, revision_id, expected_head_revision_id,
            resulting_head_revision_id, operation_id, actor_id, occurred_at_unix_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            event_kind,
            change_id.as_str(),
            revision.map(|value| value.revision_id().as_str()),
            expected_head.map(RevisionId::as_str),
            resulting_head.map(RevisionId::as_str),
            context.operation_id,
            context.actor.as_str(),
            context.occurred_at.value(),
        ],
    )?;
    Ok(())
}

#[derive(Debug)]
struct OperationRecord {
    event_kind: String,
    actor_id: String,
}

fn recorded_operation(
    transaction: &Transaction<'_>,
    operation_id: &str,
) -> Result<Option<OperationRecord>, rusqlite::Error> {
    transaction
        .query_row(
            "SELECT event_kind, actor_id FROM operation_records WHERE operation_id = ?1",
            [operation_id],
            |row| {
                Ok(OperationRecord {
                    event_kind: row.get(0)?,
                    actor_id: row.get(1)?,
                })
            },
        )
        .optional()
}

fn insert_operation_record(
    transaction: &Transaction<'_>,
    event_kind: &str,
    context: &MutationContext,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "INSERT INTO operation_records (
            operation_id, event_kind, actor_id, occurred_at_unix_ms
         ) VALUES (?1, ?2, ?3, ?4)",
        params![
            context.operation_id,
            event_kind,
            context.actor.as_str(),
            context.occurred_at.value(),
        ],
    )?;
    Ok(())
}

#[derive(Debug)]
pub enum StoreError {
    Database(rusqlite::Error),
    Domain(ChangeError),
    Coordination(CoordinationError),
    Materialization(MaterializationError),
    Relationship(RelationshipError),
    Composition(CompositionError),
    Review(ReviewError),
    Integration(IntegrationError),
    Artifact(ArtifactStoreError),
    ArtifactBaseMismatch,
    UnsupportedJournalMode(String),
    UnsupportedSchemaVersion(i64),
    InvalidOperationId,
    ChangeAlreadyExists(ChangeId),
    ChangeNotFound(ChangeId),
    DuplicateRevision(RevisionId),
    RevisionNotFoundForChange {
        change_id: ChangeId,
        revision_id: RevisionId,
    },
    DuplicateMaterialization(MaterializationId),
    MaterializationNotFound(MaterializationId),
    DuplicateRelationship(RelationshipId),
    RelationshipNotFound(RelationshipId),
    ActiveRelationshipExists,
    DuplicateDependency(DependencyId),
    DependencyNotFound(DependencyId),
    ActiveDependencyExists,
    DependencyCycle,
    DuplicateStack(StackId),
    StackNotFound(StackId),
    StaleStackVersion {
        expected: StackVersion,
        actual: StackVersion,
    },
    DuplicateCandidate(CandidateId),
    CandidateNotFound(CandidateId),
    ChangeHasNoHead(ChangeId),
    CandidateRepositoryMismatch(ChangeId),
    CandidateMissingUpstream {
        dependency_id: DependencyId,
        upstream_change_id: ChangeId,
    },
    CandidateDependencyOrder(DependencyId),
    StaleCandidateDependency(DependencyId),
    CollectionTooLarge,
    ExactTargetMismatch,
    EvidenceBeforeTarget,
    DuplicateReviewRequest(ReviewRequestId),
    ReviewRequestNotFound(ReviewRequestId),
    DuplicateReviewSubmission(ReviewSubmissionId),
    ReviewSubmissionNotFound(ReviewSubmissionId),
    DuplicateValidationResult(ValidationResultId),
    ValidationResultNotFound(ValidationResultId),
    IntegrationNotFound(IntegrationId),
    DuplicateIntegration(IntegrationId),
    IntegrationGateRejected(&'static str),
    IntegrationTargetHeld,
    AssignmentNotFound(AssignmentId),
    DuplicateLease(LeaseId),
    LeaseNotFound(LeaseId),
    LeaseNotCurrent(LeaseId),
    LeaseHeld {
        lease_id: LeaseId,
        expires_at: UnixMillis,
    },
    StaleCoordinationVersion {
        expected: CoordinationVersion,
        actual: CoordinationVersion,
    },
    OperationIdConflict(String),
    StaleHead {
        expected: Option<RevisionId>,
        actual: Option<RevisionId>,
    },
    InvalidStoredData(String),
    InvariantViolation(&'static str),
}

impl StoreError {
    fn format_composition(&self, formatter: &mut Formatter<'_>) -> Option<fmt::Result> {
        let result = match self {
            Self::DuplicateStack(id) => write!(formatter, "Stack already exists: {}", id.as_str()),
            Self::StackNotFound(id) => write!(formatter, "Stack not found: {}", id.as_str()),
            Self::StaleStackVersion { expected, actual } => write!(
                formatter,
                "stale Stack version: expected {}, actual {}",
                expected.value(),
                actual.value()
            ),
            Self::DuplicateCandidate(id) => write!(
                formatter,
                "CompositionCandidate already exists: {}",
                id.as_str()
            ),
            Self::CandidateNotFound(id) => {
                write!(formatter, "CompositionCandidate not found: {}", id.as_str())
            }
            Self::ChangeHasNoHead(id) => {
                write!(formatter, "Change has no revision head: {}", id.as_str())
            }
            Self::CandidateRepositoryMismatch(id) => write!(
                formatter,
                "candidate input belongs to a different repository: {}",
                id.as_str()
            ),
            Self::CandidateMissingUpstream {
                dependency_id,
                upstream_change_id,
            } => write!(
                formatter,
                "candidate omits upstream Change {} required by dependency {}",
                upstream_change_id.as_str(),
                dependency_id.as_str()
            ),
            Self::CandidateDependencyOrder(id) => write!(
                formatter,
                "candidate dependency points to an upstream input that is not earlier: {}",
                id.as_str()
            ),
            Self::StaleCandidateDependency(id) => {
                write!(
                    formatter,
                    "candidate dependency pins are stale: {}",
                    id.as_str()
                )
            }
            Self::CollectionTooLarge => {
                formatter.write_str("collection exceeds durable encoding limits")
            }
            _ => return None,
        };
        Some(result)
    }

    fn format_coordination(&self, formatter: &mut Formatter<'_>) -> Option<fmt::Result> {
        let result = match self {
            Self::AssignmentNotFound(id) => {
                write!(formatter, "assignment not found: {}", id.as_str())
            }
            Self::DuplicateLease(id) => write!(formatter, "lease already exists: {}", id.as_str()),
            Self::LeaseNotFound(id) => write!(formatter, "lease not found: {}", id.as_str()),
            Self::LeaseNotCurrent(id) => {
                write!(formatter, "lease is no longer current: {}", id.as_str())
            }
            Self::LeaseHeld {
                lease_id,
                expires_at,
            } => write!(
                formatter,
                "lease scope is held by {} until {}",
                lease_id.as_str(),
                expires_at.value()
            ),
            Self::StaleCoordinationVersion { expected, actual } => write!(
                formatter,
                "stale coordination version: expected {}, actual {}",
                expected.value(),
                actual.value()
            ),
            _ => return None,
        };
        Some(result)
    }

    fn format_review(&self, formatter: &mut Formatter<'_>) -> Option<fmt::Result> {
        let result = match self {
            Self::ExactTargetMismatch => {
                formatter.write_str("exact target does not match its immutable source")
            }
            Self::EvidenceBeforeTarget => {
                formatter.write_str("review or validation evidence predates its exact target")
            }
            Self::DuplicateReviewRequest(id) => {
                write!(formatter, "review request already exists: {}", id.as_str())
            }
            Self::ReviewRequestNotFound(id) => {
                write!(formatter, "review request not found: {}", id.as_str())
            }
            Self::DuplicateReviewSubmission(id) => {
                write!(
                    formatter,
                    "review submission already exists: {}",
                    id.as_str()
                )
            }
            Self::ReviewSubmissionNotFound(id) => {
                write!(formatter, "review submission not found: {}", id.as_str())
            }
            Self::DuplicateValidationResult(id) => {
                write!(
                    formatter,
                    "validation result already exists: {}",
                    id.as_str()
                )
            }
            Self::ValidationResultNotFound(id) => {
                write!(formatter, "validation result not found: {}", id.as_str())
            }
            _ => return None,
        };
        Some(result)
    }

    fn format_graph(&self, formatter: &mut Formatter<'_>) -> Option<fmt::Result> {
        let result = match self {
            Self::DuplicateRelationship(id) => {
                write!(formatter, "relationship already exists: {}", id.as_str())
            }
            Self::RelationshipNotFound(id) => {
                write!(formatter, "relationship not found: {}", id.as_str())
            }
            Self::ActiveRelationshipExists => {
                formatter.write_str("an active relationship already exists for this kind and pair")
            }
            Self::DuplicateDependency(id) => {
                write!(formatter, "dependency already exists: {}", id.as_str())
            }
            Self::DependencyNotFound(id) => {
                write!(formatter, "dependency not found: {}", id.as_str())
            }
            Self::ActiveDependencyExists => {
                formatter.write_str("an active dependency already exists for this directed pair")
            }
            Self::DependencyCycle => formatter.write_str("active dependency would create a cycle"),
            _ => return None,
        };
        Some(result)
    }

    fn format_integration(&self, formatter: &mut Formatter<'_>) -> Option<fmt::Result> {
        let result = match self {
            Self::Integration(error) => write!(formatter, "integration error: {error}"),
            Self::IntegrationNotFound(id) => {
                write!(formatter, "integration attempt not found: {}", id.as_str())
            }
            Self::DuplicateIntegration(id) => {
                write!(
                    formatter,
                    "integration attempt already exists: {}",
                    id.as_str()
                )
            }
            Self::IntegrationGateRejected(reason) => {
                write!(formatter, "integration gate rejected: {reason}")
            }
            Self::IntegrationTargetHeld => {
                formatter.write_str("integration target already has live execution authority")
            }
            _ => return None,
        };
        Some(result)
    }

    fn format_special(&self, formatter: &mut Formatter<'_>) -> Option<fmt::Result> {
        if let Some(result) = self.format_composition(formatter) {
            return Some(result);
        }
        if let Some(result) = self.format_coordination(formatter) {
            return Some(result);
        }
        if let Some(result) = self.format_review(formatter) {
            return Some(result);
        }
        if let Some(result) = self.format_integration(formatter) {
            return Some(result);
        }
        self.format_graph(formatter)
    }
}

impl Display for StoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if let Some(result) = self.format_special(formatter) {
            return result;
        }
        match self {
            Self::Database(error) => write!(formatter, "SQLite error: {error}"),
            Self::Domain(error) => write!(formatter, "domain error: {error}"),
            Self::Coordination(error) => write!(formatter, "coordination error: {error}"),
            Self::Materialization(error) => write!(formatter, "materialization error: {error}"),
            Self::Relationship(error) => write!(formatter, "relationship error: {error}"),
            Self::Composition(error) => write!(formatter, "composition error: {error}"),
            Self::Review(error) => write!(formatter, "review/validation error: {error}"),
            Self::Artifact(error) => write!(formatter, "artifact error: {error}"),
            Self::ArtifactBaseMismatch => {
                formatter.write_str("revision base does not match its canonical artifact")
            }
            Self::UnsupportedJournalMode(mode) => {
                write!(formatter, "SQLite WAL mode is unavailable (got {mode})")
            }
            Self::UnsupportedSchemaVersion(version) => {
                write!(formatter, "unsupported SQLite schema version: {version}")
            }
            Self::InvalidOperationId => formatter.write_str("operation ID cannot be empty"),
            Self::ChangeAlreadyExists(id) => {
                write!(formatter, "Change already exists: {}", id.as_str())
            }
            Self::ChangeNotFound(id) => write!(formatter, "Change not found: {}", id.as_str()),
            Self::DuplicateRevision(id) => {
                write!(formatter, "revision already exists: {}", id.as_str())
            }
            Self::RevisionNotFoundForChange {
                change_id,
                revision_id,
            } => write!(
                formatter,
                "revision {} does not belong to Change {}",
                revision_id.as_str(),
                change_id.as_str()
            ),
            Self::DuplicateMaterialization(id) => {
                write!(formatter, "Materialization already exists: {}", id.as_str())
            }
            Self::MaterializationNotFound(id) => {
                write!(formatter, "Materialization not found: {}", id.as_str())
            }
            Self::OperationIdConflict(id) => {
                write!(
                    formatter,
                    "operation ID was recorded for a different mutation: {id}"
                )
            }
            Self::StaleHead { expected, actual } => write!(
                formatter,
                "stale revision head: expected {}, actual {}",
                display_optional_revision(expected.as_ref()),
                display_optional_revision(actual.as_ref())
            ),
            Self::InvalidStoredData(message) => write!(formatter, "invalid stored data: {message}"),
            Self::InvariantViolation(message) => {
                write!(formatter, "storage invariant failed: {message}")
            }
            Self::DuplicateStack(_)
            | Self::StackNotFound(_)
            | Self::StaleStackVersion { .. }
            | Self::DuplicateCandidate(_)
            | Self::CandidateNotFound(_)
            | Self::ChangeHasNoHead(_)
            | Self::CandidateRepositoryMismatch(_)
            | Self::CandidateMissingUpstream { .. }
            | Self::CandidateDependencyOrder(_)
            | Self::StaleCandidateDependency(_)
            | Self::CollectionTooLarge
            | Self::AssignmentNotFound(_)
            | Self::DuplicateLease(_)
            | Self::LeaseNotFound(_)
            | Self::LeaseNotCurrent(_)
            | Self::LeaseHeld { .. }
            | Self::StaleCoordinationVersion { .. } => unreachable!("error handled above"),
            Self::ExactTargetMismatch
            | Self::EvidenceBeforeTarget
            | Self::DuplicateReviewRequest(_)
            | Self::ReviewRequestNotFound(_)
            | Self::DuplicateReviewSubmission(_)
            | Self::ReviewSubmissionNotFound(_)
            | Self::DuplicateValidationResult(_)
            | Self::ValidationResultNotFound(_) => unreachable!("review error handled above"),
            Self::DuplicateRelationship(_)
            | Self::RelationshipNotFound(_)
            | Self::ActiveRelationshipExists
            | Self::DuplicateDependency(_)
            | Self::DependencyNotFound(_)
            | Self::ActiveDependencyExists
            | Self::DependencyCycle => unreachable!("graph error handled above"),
            Self::Integration(_)
            | Self::IntegrationNotFound(_)
            | Self::DuplicateIntegration(_)
            | Self::IntegrationGateRejected(_)
            | Self::IntegrationTargetHeld => unreachable!("integration error handled above"),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Domain(error) => Some(error),
            Self::Coordination(error) => Some(error),
            Self::Materialization(error) => Some(error),
            Self::Relationship(error) => Some(error),
            Self::Composition(error) => Some(error),
            Self::Review(error) => Some(error),
            Self::Integration(error) => Some(error),
            Self::Artifact(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

impl From<ChangeError> for StoreError {
    fn from(value: ChangeError) -> Self {
        Self::Domain(value)
    }
}

impl From<CoordinationError> for StoreError {
    fn from(value: CoordinationError) -> Self {
        Self::Coordination(value)
    }
}

impl From<MaterializationError> for StoreError {
    fn from(value: MaterializationError) -> Self {
        Self::Materialization(value)
    }
}

impl From<RelationshipError> for StoreError {
    fn from(value: RelationshipError) -> Self {
        Self::Relationship(value)
    }
}

impl From<CompositionError> for StoreError {
    fn from(value: CompositionError) -> Self {
        Self::Composition(value)
    }
}

impl From<ReviewError> for StoreError {
    fn from(value: ReviewError) -> Self {
        Self::Review(value)
    }
}

impl From<IntegrationError> for StoreError {
    fn from(value: IntegrationError) -> Self {
        Self::Integration(value)
    }
}

impl From<ArtifactStoreError> for StoreError {
    fn from(value: ArtifactStoreError) -> Self {
        Self::Artifact(value)
    }
}

fn display_optional_revision(revision: Option<&RevisionId>) -> &str {
    revision.map_or("<none>", RevisionId::as_str)
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::{Arc, Barrier};

    use tempfile::TempDir;
    use weft_artifact::{CanonicalTreeDelta, CasDigest};
    use weft_domain::{
        ConflictResolution, ConflictResolutionId, EffectOperationId, ExecutionLease,
        ExecutionLeaseId, FileMode, GatePolicyEvidence, IntegrationAttempt, IntegrationBinding,
        IntegrationCapabilityEvidence, IntegrationConflictId, IntegrationEvidence, IntegrationGate,
        IntegrationIntent, IntegrationMethod, IntegrationReceiptId, IntegrationState,
        IntegrationStrategy, IntegrationTarget, PathOperation, ReconciliationId,
        ReconciliationOutcome, Subject, SubjectId, SubjectKind, TargetObservation, TargetRef,
        TargetRevision, TreeDelta,
    };

    use super::*;

    const DATABASE_ENV: &str = "WEFT_SQLITE_PROCESS_TEST_DATABASE";
    const ARTIFACT_ENV: &str = "WEFT_SQLITE_PROCESS_TEST_ARTIFACTS";
    const LEASE_DATABASE_ENV: &str = "WEFT_SQLITE_PROCESS_TEST_LEASE_DATABASE";

    fn change_id() -> ChangeId {
        ChangeId::new("change-1").unwrap()
    }

    fn revision(artifacts: &ArtifactStore, id: &str, created_at: i64) -> NewRevision {
        revision_with(
            artifacts,
            id,
            created_at,
            "repository-1",
            "base-object",
            "author-1",
            id.as_bytes(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn revision_with(
        artifacts: &ArtifactStore,
        id: &str,
        created_at: i64,
        repository_id: &str,
        base_object: &str,
        author: &str,
        content: &[u8],
    ) -> NewRevision {
        let base = BaseState::new(RepositoryId::new(repository_id).unwrap(), base_object).unwrap();
        let blob = artifacts.store_blob(content).unwrap();
        let canonical = CanonicalTreeDelta::new(
            base.clone(),
            TreeDelta::new(vec![PathOperation::Upsert {
                path: format!("{id}.txt"),
                mode: FileMode::Regular,
                blob_digest: blob.as_str().to_owned(),
            }])
            .unwrap(),
        );
        let artifact = artifacts.store_manifest(&canonical).unwrap();
        NewRevision::new(
            RevisionId::new(id).unwrap(),
            base,
            artifact,
            UnixMillis::new(created_at).unwrap(),
            ActorId::new(author).unwrap(),
        )
    }

    fn context(operation_id: &str, occurred_at: i64) -> MutationContext {
        MutationContext::new(
            operation_id,
            ActorId::new("operator-1").unwrap(),
            UnixMillis::new(occurred_at).unwrap(),
        )
        .unwrap()
    }

    fn coordination_subject(id: &str) -> Subject {
        Subject::new(SubjectKind::Agent, SubjectId::new(id).unwrap())
    }

    fn lease_scope() -> LeaseScope {
        LeaseScope::new(
            change_id(),
            LeaseOperation::new("revision.capture").unwrap(),
        )
    }

    fn assignment(id: &str, subject_id: &str, at: i64) -> Assignment {
        Assignment::new(
            AssignmentId::new(id).unwrap(),
            change_id(),
            coordination_subject(subject_id),
            AssignmentRole::Implementer,
            UnixMillis::new(at).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
    }

    fn materialization(id: &str, workspace: &str, provider_ref: &str) -> Materialization {
        materialization_at(id, workspace, provider_ref, 3)
    }

    fn materialization_at(
        id: &str,
        workspace: &str,
        provider_ref: &str,
        created_at: i64,
    ) -> Materialization {
        Materialization::new(
            MaterializationId::new(id).unwrap(),
            change_id(),
            RevisionId::new("revision-1").unwrap(),
            MaterializationPlacement::new(
                WorkspaceId::new(workspace).unwrap(),
                ProviderId::new("native-git").unwrap(),
                ProviderRef::new(provider_ref).unwrap(),
            ),
            UnixMillis::new(created_at).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
    }

    fn provider_evidence(value: &str) -> ProviderEvidence {
        ProviderEvidence::new(value).unwrap()
    }

    fn provider_observation(
        state: MaterializationState,
        provider_ref: &str,
        evidence: &str,
    ) -> ProviderObservation {
        ProviderObservation::new(
            state,
            ProviderRef::new(provider_ref).unwrap(),
            provider_evidence(evidence),
        )
    }

    fn seed_revision(store: &mut SqliteStore, artifacts: &ArtifactStore) {
        store
            .create_change(&change_id(), &context("create-1", 1))
            .unwrap();
        store
            .append_revision(
                artifacts,
                &change_id(),
                None,
                &revision(artifacts, "revision-1", 2),
                &context("append-1", 2),
            )
            .unwrap();
    }

    fn seed_named_change(
        store: &mut SqliteStore,
        artifacts: &ArtifactStore,
        change: &str,
        revision_id: &str,
        at: i64,
    ) {
        let change_id = ChangeId::new(change).unwrap();
        store
            .create_change(&change_id, &context(&format!("create-{change}"), at))
            .unwrap();
        store
            .append_revision(
                artifacts,
                &change_id,
                None,
                &revision(artifacts, revision_id, at + 1),
                &context(&format!("append-{revision_id}"), at + 1),
            )
            .unwrap();
    }

    fn append_named_revision(
        store: &mut SqliteStore,
        artifacts: &ArtifactStore,
        change: &str,
        expected_revision: &str,
        revision_id: &str,
        at: i64,
    ) {
        store
            .append_revision(
                artifacts,
                &ChangeId::new(change).unwrap(),
                Some(&RevisionId::new(expected_revision).unwrap()),
                &revision(artifacts, revision_id, at),
                &context(&format!("append-{revision_id}"), at),
            )
            .unwrap();
    }

    fn relationship(id: &str, left: &str, right: &str, at: i64) -> Relationship {
        Relationship::new(
            RelationshipId::new(id).unwrap(),
            RelationshipKind::RelatedTo,
            RelationshipEndpoints::new(ChangeId::new(left).unwrap(), ChangeId::new(right).unwrap())
                .unwrap(),
            UnixMillis::new(at).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
    }

    fn dependency(
        id: &str,
        downstream_change: &str,
        downstream_revision: &str,
        upstream_change: &str,
        upstream_revision: &str,
        at: i64,
    ) -> Dependency {
        Dependency::new(
            DependencyId::new(id).unwrap(),
            ChangeId::new(downstream_change).unwrap(),
            ChangeId::new(upstream_change).unwrap(),
            DependencyPins::new(
                RevisionId::new(downstream_revision).unwrap(),
                RevisionId::new(upstream_revision).unwrap(),
            ),
            UnixMillis::new(at).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
        .unwrap()
    }

    fn seed_default_dependency(store: &mut SqliteStore, artifacts: &ArtifactStore) -> Dependency {
        seed_named_change(store, artifacts, "downstream", "down-r1", 1);
        seed_named_change(store, artifacts, "upstream", "up-r1", 3);
        let value = dependency(
            "dependency-1",
            "downstream",
            "down-r1",
            "upstream",
            "up-r1",
            5,
        );
        store
            .create_dependency(artifacts, &value, &context("dependency-create", 5))
            .unwrap();
        value
    }

    fn stack(id: &str, policy: StackPolicy, changes: &[&str], at: i64) -> Stack {
        Stack::new(
            StackId::new(id).unwrap(),
            StackDefinition::from_changes(
                policy,
                changes
                    .iter()
                    .map(|change| ChangeId::new(*change).unwrap())
                    .collect(),
            )
            .unwrap(),
            UnixMillis::new(at).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
    }

    fn review_request(target: ExactTarget, at: i64) -> ReviewRequest {
        ReviewRequest::new(
            ReviewRequestId::new("review-request-1").unwrap(),
            target,
            ActorId::new("operator-1").unwrap(),
            vec![
                ActorId::new("reviewer-1").unwrap(),
                ActorId::new("reviewer-2").unwrap(),
            ],
            UnixMillis::new(at).unwrap(),
        )
        .unwrap()
    }

    fn reviewer_context(operation_id: &str, reviewer: &str, at: i64) -> MutationContext {
        MutationContext::new(
            operation_id,
            ActorId::new(reviewer).unwrap(),
            UnixMillis::new(at).unwrap(),
        )
        .unwrap()
    }

    fn database() -> (TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("weft.sqlite3");
        (directory, path)
    }

    fn concurrently_open(path: &Path) {
        let barrier = Arc::new(Barrier::new(3));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let path = path.to_path_buf();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    SqliteStore::open(path)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                })
            })
            .collect();
        barrier.wait();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }
    }

    fn create_schema_two_database(path: &Path) -> Connection {
        let connection = Connection::open(path).unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection
    }

    fn create_schema_three_database(path: &Path) -> Connection {
        let connection = create_schema_two_database(path);
        connection.execute_batch(MIGRATION_3).unwrap();
        connection
    }

    fn insert_raw_audit_event(
        store: &SqliteStore,
        event_kind: &str,
        change_id: &ChangeId,
        revision_id: Option<&str>,
        expected_head: Option<&str>,
        resulting_head: Option<&str>,
        operation_id: &str,
    ) -> rusqlite::Error {
        store
            .connection
            .execute_batch("SAVEPOINT invalid_audit")
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO operation_records (
                    operation_id, event_kind, actor_id, occurred_at_unix_ms
                 ) VALUES (?1, ?2, 'operator-1', 3)",
                params![operation_id, event_kind],
            )
            .unwrap();
        let error = store
            .connection
            .execute(
                "INSERT INTO audit_events (
                    event_kind, change_id, revision_id, expected_head_revision_id,
                    resulting_head_revision_id, operation_id, actor_id, occurred_at_unix_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'operator-1', 3)",
                params![
                    event_kind,
                    change_id.as_str(),
                    revision_id,
                    expected_head,
                    resulting_head,
                    operation_id
                ],
            )
            .unwrap_err();
        store
            .connection
            .execute_batch("ROLLBACK TO invalid_audit; RELEASE invalid_audit")
            .unwrap();
        error
    }

    #[test]
    fn migration_reopen_round_trip_preserves_revision_and_audit_history() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let id = change_id();
        {
            let mut store = SqliteStore::open(&path).unwrap();
            assert_eq!(
                store
                    .connection
                    .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                SqliteStore::schema_version()
            );
            assert_eq!(
                store
                    .connection
                    .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                    .unwrap(),
                "wal"
            );
            assert_eq!(
                store
                    .connection
                    .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1
            );

            store.create_change(&id, &context("create-1", 1)).unwrap();
            store
                .append_revision(
                    &artifacts,
                    &id,
                    None,
                    &revision(&artifacts, "revision-1", 2),
                    &context("append-1", 2),
                )
                .unwrap();
            let first = RevisionId::new("revision-1").unwrap();
            store
                .append_revision(
                    &artifacts,
                    &id,
                    Some(&first),
                    &revision(&artifacts, "revision-2", 3),
                    &context("append-2", 3),
                )
                .unwrap();
        }

        let store = SqliteStore::open(&path).unwrap();
        let change = store.load_change(&artifacts, &id).unwrap();
        assert_eq!(change.head().map(RevisionId::as_str), Some("revision-2"));
        assert_eq!(change.revisions().len(), 2);
        assert_eq!(change.revisions()[0].created_by().as_str(), "author-1");
        assert_eq!(change.revisions()[1].created_at().value(), 3);

        let events = store.audit_events(&id).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_kind, "change.created");
        assert_eq!(events[1].expected_head_revision_id, None);
        assert_eq!(
            events[2]
                .expected_head_revision_id
                .as_ref()
                .map(RevisionId::as_str),
            Some("revision-1")
        );
        assert_eq!(
            events[2]
                .resulting_head_revision_id
                .as_ref()
                .map(RevisionId::as_str),
            Some("revision-2")
        );
    }

    #[test]
    fn concurrent_first_open_serializes_migration() {
        let (directory, _) = database();
        for round in 0..10 {
            let path = directory.path().join(format!("migration-{round}.sqlite3"));
            concurrently_open(&path);

            let store = SqliteStore::open(path).unwrap();
            assert_eq!(
                store
                    .connection
                    .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                SqliteStore::schema_version()
            );
        }
    }

    #[test]
    fn concurrent_version_one_upgrade_rechecks_after_migration_lock() {
        let (directory, _) = database();
        for round in 0..10 {
            let path = directory.path().join(format!("upgrade-{round}.sqlite3"));
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(MIGRATION_1).unwrap();
            drop(connection);

            concurrently_open(&path);
            let store = SqliteStore::open(path).unwrap();
            assert_eq!(
                store
                    .connection
                    .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                SqliteStore::schema_version()
            );
        }
    }

    #[test]
    fn concurrent_populated_version_one_upgrade_preserves_revision_history() {
        let (directory, _) = database();
        let path = directory.path().join("populated-v1.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        let sql = format!(
            "INSERT INTO changes (change_id) VALUES ('change-1');
             INSERT INTO audit_events (
                event_kind, change_id, operation_id, actor_id, occurred_at_unix_ms
             ) VALUES ('change.created', 'change-1', 'legacy-create', 'operator-1', 1);
             INSERT INTO change_revisions (
                revision_id, change_id, sequence, parent_revision_id,
                repository_id, base_object_id, artifact_version, artifact_digest,
                created_at_unix_ms, created_by
             ) VALUES (
                'revision-1', 'change-1', 0, NULL, 'repository-1', 'base-1',
                'tree-delta-v1', 'sha256:{digest}', 2, 'operator-1'
             );
             UPDATE changes SET head_revision_id = 'revision-1'
             WHERE change_id = 'change-1';
             INSERT INTO audit_events (
                event_kind, change_id, revision_id, expected_head_revision_id,
                resulting_head_revision_id, operation_id, actor_id,
                occurred_at_unix_ms
             ) VALUES (
                'revision.appended', 'change-1', 'revision-1', NULL,
                'revision-1', 'legacy-append', 'operator-1', 2
             );",
            digest = "0".repeat(64)
        );
        connection.execute_batch(&sql).unwrap();
        drop(connection);

        concurrently_open(&path);
        let store = SqliteStore::open(path).unwrap();
        assert_eq!(store.audit_events(&change_id()).unwrap().len(), 2);
        let (head, revisions) = store
            .connection
            .query_row(
                "SELECT change.head_revision_id,
                        (SELECT count(*) FROM change_revisions AS revision
                         WHERE revision.change_id = change.change_id)
                 FROM changes AS change WHERE change.change_id = 'change-1'",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap();
        assert_eq!(head, "revision-1");
        assert_eq!(revisions, 1);
    }

    #[test]
    fn concurrent_version_two_upgrade_rechecks_after_migration_lock() {
        let (directory, _) = database();
        for round in 0..10 {
            let path = directory.path().join(format!("upgrade-v2-{round}.sqlite3"));
            drop(create_schema_two_database(&path));

            concurrently_open(&path);
            let store = SqliteStore::open(path).unwrap();
            assert_eq!(
                store
                    .connection
                    .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                SqliteStore::schema_version()
            );
        }
    }

    #[test]
    fn concurrent_version_three_upgrade_rechecks_after_migration_lock() {
        let (directory, _) = database();
        for round in 0..10 {
            let path = directory.path().join(format!("upgrade-v3-{round}.sqlite3"));
            drop(create_schema_three_database(&path));

            concurrently_open(&path);
            let store = SqliteStore::open(path).unwrap();
            assert_eq!(
                store
                    .connection
                    .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                SqliteStore::schema_version()
            );
        }
    }

    #[test]
    fn concurrent_version_six_upgrade_rechecks_after_migration_lock() {
        let (directory, _) = database();
        for round in 0..10 {
            let path = directory.path().join(format!("upgrade-v6-{round}.sqlite3"));
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(MIGRATION_1).unwrap();
            connection.execute_batch(MIGRATION_2).unwrap();
            connection.execute_batch(MIGRATION_3).unwrap();
            connection.execute_batch(MIGRATION_4).unwrap();
            connection.execute_batch(MIGRATION_5).unwrap();
            connection.execute_batch(MIGRATION_6).unwrap();
            drop(connection);

            concurrently_open(&path);
            let store = SqliteStore::open(path).unwrap();
            assert_eq!(
                store
                    .connection
                    .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                SqliteStore::schema_version()
            );
        }
    }

    #[test]
    fn revision_append_requires_durable_content_for_the_same_exact_base() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let id = change_id();
        let mut store = SqliteStore::open(path).unwrap();
        store.create_change(&id, &context("create-1", 1)).unwrap();

        let missing = NewRevision::new(
            RevisionId::new("revision-missing").unwrap(),
            BaseState::new(RepositoryId::new("repository-1").unwrap(), "base-object").unwrap(),
            ArtifactRef::tree_delta_v1(format!("sha256:{}", "f".repeat(64))).unwrap(),
            UnixMillis::new(2).unwrap(),
            ActorId::new("author-1").unwrap(),
        );
        assert!(matches!(
            store.append_revision(
                &artifacts,
                &id,
                None,
                &missing,
                &context("append-missing", 2)
            ),
            Err(StoreError::Artifact(ArtifactStoreError::ObjectMissing(_)))
        ));

        let blob = artifacts.store_blob(b"content").unwrap();
        let manifest = CanonicalTreeDelta::new(
            BaseState::new(RepositoryId::new("repository-1").unwrap(), "recorded-base").unwrap(),
            TreeDelta::new(vec![PathOperation::Upsert {
                path: "file.txt".to_owned(),
                mode: FileMode::Regular,
                blob_digest: blob.as_str().to_owned(),
            }])
            .unwrap(),
        );
        let mismatched = NewRevision::new(
            RevisionId::new("revision-mismatch").unwrap(),
            BaseState::new(RepositoryId::new("repository-1").unwrap(), "different-base").unwrap(),
            artifacts.store_manifest(&manifest).unwrap(),
            UnixMillis::new(3).unwrap(),
            ActorId::new("author-1").unwrap(),
        );
        assert!(matches!(
            store.append_revision(
                &artifacts,
                &id,
                None,
                &mismatched,
                &context("append-mismatch", 3)
            ),
            Err(StoreError::ArtifactBaseMismatch)
        ));
        assert_eq!(store.load_change(&artifacts, &id).unwrap().head(), None);
        assert_eq!(store.audit_events(&id).unwrap().len(), 1);
    }

    #[test]
    fn change_load_fails_closed_when_canonical_manifest_disappears() {
        let (directory, path) = database();
        let artifact_path = directory.path().join("artifacts");
        let artifacts = ArtifactStore::open(&artifact_path).unwrap();
        let id = change_id();
        let mut store = SqliteStore::open(path).unwrap();
        store.create_change(&id, &context("create-1", 1)).unwrap();
        let revision = revision(&artifacts, "revision-1", 2);
        let digest = CasDigest::parse(revision.artifact().manifest_digest()).unwrap();
        store
            .append_revision(&artifacts, &id, None, &revision, &context("append-1", 2))
            .unwrap();
        let manifest_path = artifact_path
            .join("objects")
            .join("sha256")
            .join(&digest.hex()[..2])
            .join(&digest.hex()[2..]);
        std::fs::remove_file(manifest_path).unwrap();

        assert!(matches!(
            store.load_change(&artifacts, &id),
            Err(StoreError::Artifact(ArtifactStoreError::ObjectMissing(_)))
        ));
    }

    #[test]
    fn stale_independent_connection_rolls_back_revision_and_audit_event() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let id = change_id();
        let mut first_store = SqliteStore::open(&path).unwrap();
        let mut stale_store = SqliteStore::open(&path).unwrap();
        first_store
            .create_change(&id, &context("create-1", 1))
            .unwrap();
        first_store
            .append_revision(
                &artifacts,
                &id,
                None,
                &revision(&artifacts, "revision-1", 2),
                &context("append-1", 2),
            )
            .unwrap();
        let observed_head = stale_store
            .load_change(&artifacts, &id)
            .unwrap()
            .head()
            .cloned()
            .unwrap();
        first_store
            .append_revision(
                &artifacts,
                &id,
                Some(&observed_head),
                &revision(&artifacts, "revision-2", 3),
                &context("append-2", 3),
            )
            .unwrap();

        let error = stale_store
            .append_revision(
                &artifacts,
                &id,
                Some(&observed_head),
                &revision(&artifacts, "revision-stale", 4),
                &context("append-stale", 4),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            StoreError::StaleHead {
                expected: Some(_),
                actual: Some(_)
            }
        ));
        let change = stale_store.load_change(&artifacts, &id).unwrap();
        assert_eq!(change.head().map(RevisionId::as_str), Some("revision-2"));
        assert_eq!(change.revisions().len(), 2);
        assert_eq!(stale_store.audit_events(&id).unwrap().len(), 3);
    }

    #[test]
    fn exact_operation_replay_returns_recorded_outcome_without_duplication() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let id = change_id();
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .create_change(&id, &context("operation-1", 1))
            .unwrap();
        store
            .create_change(&id, &context("operation-1", 99))
            .unwrap();
        store
            .append_revision(
                &artifacts,
                &id,
                None,
                &revision(&artifacts, "revision-1", 2),
                &context("operation-2", 2),
            )
            .unwrap();
        store
            .append_revision(
                &artifacts,
                &id,
                None,
                &revision(&artifacts, "revision-1", 2),
                &context("operation-2", 99),
            )
            .unwrap();

        let altered_revision = revision_with(
            &artifacts,
            "revision-1",
            22,
            "repository-other",
            "different-base",
            "different-author",
            b"different content",
        );
        let content_conflict = store
            .append_revision(
                &artifacts,
                &id,
                None,
                &altered_revision,
                &context("operation-2", 100),
            )
            .unwrap_err();
        assert!(matches!(
            content_conflict,
            StoreError::OperationIdConflict(_)
        ));
        let actor_conflict = store
            .append_revision(
                &artifacts,
                &id,
                None,
                &revision(&artifacts, "revision-1", 2),
                &MutationContext::new(
                    "operation-2",
                    ActorId::new("different-operator").unwrap(),
                    UnixMillis::new(101).unwrap(),
                )
                .unwrap(),
            )
            .unwrap_err();
        assert!(matches!(actor_conflict, StoreError::OperationIdConflict(_)));
        assert_eq!(
            store
                .load_change(&artifacts, &id)
                .unwrap()
                .head()
                .map(RevisionId::as_str),
            Some("revision-1")
        );
        assert_eq!(store.audit_events(&id).unwrap().len(), 2);

        let conflict = store
            .append_revision(
                &artifacts,
                &id,
                Some(&RevisionId::new("revision-1").unwrap()),
                &revision(&artifacts, "revision-2", 3),
                &context("operation-1", 3),
            )
            .unwrap_err();
        assert!(matches!(conflict, StoreError::OperationIdConflict(_)));
        assert_eq!(store.audit_events(&id).unwrap().len(), 2);
    }

    #[test]
    fn audit_constraints_reject_cross_change_unknown_and_invalid_state() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let first = ChangeId::new("change-1").unwrap();
        let second = ChangeId::new("change-2").unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .create_change(&first, &context("create-1", 1))
            .unwrap();
        store
            .append_revision(
                &artifacts,
                &first,
                None,
                &revision(&artifacts, "revision-1", 2),
                &context("append-1", 2),
            )
            .unwrap();
        let first_head = RevisionId::new("revision-1").unwrap();
        store
            .append_revision(
                &artifacts,
                &first,
                Some(&first_head),
                &revision(&artifacts, "revision-1b", 3),
                &context("append-1b", 3),
            )
            .unwrap();
        store
            .create_change(&second, &context("create-2", 1))
            .unwrap();
        store
            .append_revision(
                &artifacts,
                &second,
                None,
                &revision(&artifacts, "revision-2", 2),
                &context("append-2", 2),
            )
            .unwrap();
        let cross_change = insert_raw_audit_event(
            &store,
            "revision.appended",
            &first,
            Some("revision-2"),
            None,
            Some("revision-2"),
            "raw-cross-change",
        );
        let unknown_expected = insert_raw_audit_event(
            &store,
            "revision.appended",
            &first,
            Some("revision-1"),
            Some("unknown-revision"),
            Some("revision-1"),
            "raw-unknown-head",
        );
        let invalid_shape = insert_raw_audit_event(
            &store,
            "change.created",
            &first,
            Some("revision-1"),
            None,
            Some("revision-1"),
            "raw-invalid-shape",
        );
        let wrong_known_parent = insert_raw_audit_event(
            &store,
            "revision.appended",
            &first,
            Some("revision-1b"),
            None,
            Some("revision-1b"),
            "raw-wrong-parent",
        );
        let duplicate_creation = insert_raw_audit_event(
            &store,
            "change.created",
            &first,
            None,
            None,
            None,
            "raw-duplicate-create",
        );

        assert!(cross_change.to_string().contains("does not match parent"));
        assert!(
            unknown_expected
                .to_string()
                .contains("does not match parent")
        );
        assert!(invalid_shape.to_string().contains("CHECK constraint"));
        assert!(
            wrong_known_parent
                .to_string()
                .contains("does not match parent")
        );
        assert!(duplicate_creation.to_string().contains("UNIQUE constraint"));
        assert_eq!(store.audit_events(&first).unwrap().len(), 3);
        assert_eq!(store.audit_events(&second).unwrap().len(), 2);
    }

    #[test]
    fn identity_revision_and_audit_rows_reject_mutation() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let id = change_id();
        let mut store = SqliteStore::open(&path).unwrap();
        store.create_change(&id, &context("create-1", 1)).unwrap();
        store
            .append_revision(
                &artifacts,
                &id,
                None,
                &revision(&artifacts, "revision-1", 2),
                &context("append-1", 2),
            )
            .unwrap();

        let audit_update = store
            .connection
            .execute("UPDATE audit_events SET actor_id = 'other'", [])
            .unwrap_err();
        let audit_delete = store
            .connection
            .execute("DELETE FROM audit_events", [])
            .unwrap_err();
        let revision_update = store
            .connection
            .execute(
                "UPDATE change_revisions SET base_object_id = 'rewritten'",
                [],
            )
            .unwrap_err();
        let revision_delete = store
            .connection
            .execute("DELETE FROM change_revisions", [])
            .unwrap_err();
        let identity_update = store
            .connection
            .execute("UPDATE changes SET change_id = 'rewritten'", [])
            .unwrap_err();
        let change_delete = store
            .connection
            .execute("DELETE FROM changes", [])
            .unwrap_err();
        assert!(audit_update.to_string().contains("append-only"));
        assert!(audit_delete.to_string().contains("append-only"));
        assert!(revision_update.to_string().contains("immutable"));
        assert!(revision_delete.to_string().contains("cannot be deleted"));
        assert!(identity_update.to_string().contains("immutable"));
        assert!(change_delete.to_string().contains("cannot be deleted"));
        assert_eq!(store.audit_events(&id).unwrap().len(), 2);
    }

    #[test]
    fn version_one_database_migrates_without_losing_change_history() {
        let (directory, path) = database();
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection
            .execute("INSERT INTO changes (change_id) VALUES ('change-1')", [])
            .unwrap();
        connection
            .execute(
                "INSERT INTO audit_events (
                    event_kind, change_id, operation_id, actor_id, occurred_at_unix_ms
                 ) VALUES ('change.created', 'change-1', 'legacy-create', 'operator-1', 1)",
                [],
            )
            .unwrap();
        drop(connection);

        let mut store = SqliteStore::open(&path).unwrap();
        assert_eq!(SqliteStore::schema_version(), 7);
        assert_eq!(store.audit_events(&change_id()).unwrap().len(), 1);
        store
            .create_change(&change_id(), &context("legacy-create", 99))
            .unwrap();
        let assigned = assignment("assignment-1", "agent-1", 2);
        store
            .create_assignment(&assigned, &context("assign-1", 2))
            .unwrap();
        drop(store);

        let reopened = SqliteStore::open(&path).unwrap();
        assert_eq!(reopened.assignments(&change_id()).unwrap(), vec![assigned]);
        assert!(directory.path().exists());
    }

    #[test]
    fn version_two_database_migrates_without_losing_coordination_history() {
        let (_directory, path) = database();
        let connection = create_schema_two_database(&path);
        connection
            .execute_batch(
                "INSERT INTO changes (change_id) VALUES ('change-1');
                 INSERT INTO operation_records (
                    operation_id, event_kind, actor_id, occurred_at_unix_ms
                 ) VALUES ('legacy-create', 'change.created', 'operator-1', 1);
                 INSERT INTO audit_events (
                    event_kind, change_id, operation_id, actor_id, occurred_at_unix_ms
                 ) VALUES ('change.created', 'change-1', 'legacy-create', 'operator-1', 1);
                 INSERT INTO assignments (
                    assignment_id, change_id, subject_kind, subject_id, role,
                    assigned_at_unix_ms, assigned_by, version
                 ) VALUES (
                    'assignment-1', 'change-1', 'agent', 'agent-1',
                    'implementer', 2, 'operator-1', 1
                 );
                 INSERT INTO operation_records (
                    operation_id, event_kind, actor_id, occurred_at_unix_ms
                 ) VALUES ('legacy-assign', 'assignment.assigned', 'operator-1', 2);
                 INSERT INTO assignment_events (
                    event_kind, assignment_id, change_id, expected_version,
                    resulting_version, operation_id
                 ) VALUES (
                    'assignment.assigned', 'assignment-1', 'change-1', 0, 1,
                    'legacy-assign'
                 );",
            )
            .unwrap();
        drop(connection);

        let store = SqliteStore::open(path).unwrap();
        assert_eq!(SqliteStore::schema_version(), 7);
        assert_eq!(store.audit_events(&change_id()).unwrap().len(), 1);
        assert_eq!(store.assignments(&change_id()).unwrap().len(), 1);
        assert_eq!(store.assignment_events(&change_id()).unwrap().len(), 1);
    }

    #[test]
    fn version_three_database_migrates_without_losing_materialization_history() {
        let (_directory, path) = database();
        let connection = create_schema_three_database(&path);
        let sql = format!(
            "INSERT INTO changes (change_id) VALUES ('change-1');
             INSERT INTO operation_records (
                operation_id, event_kind, actor_id, occurred_at_unix_ms
             ) VALUES ('legacy-create', 'change.created', 'operator-1', 1);
             INSERT INTO audit_events (
                event_kind, change_id, operation_id, actor_id, occurred_at_unix_ms
             ) VALUES ('change.created', 'change-1', 'legacy-create', 'operator-1', 1);
             INSERT INTO change_revisions (
                revision_id, change_id, sequence, parent_revision_id,
                repository_id, base_object_id, artifact_version, artifact_digest,
                created_at_unix_ms, created_by
             ) VALUES (
                'revision-1', 'change-1', 0, NULL, 'repository-1', 'base-1',
                'tree-delta-v1', 'sha256:{digest}', 2, 'operator-1'
             );
             UPDATE changes SET head_revision_id = 'revision-1'
             WHERE change_id = 'change-1';
             INSERT INTO operation_records (
                operation_id, event_kind, actor_id, occurred_at_unix_ms
             ) VALUES ('legacy-append', 'revision.appended', 'operator-1', 2);
             INSERT INTO audit_events (
                event_kind, change_id, revision_id, expected_head_revision_id,
                resulting_head_revision_id, operation_id, actor_id,
                occurred_at_unix_ms
             ) VALUES (
                'revision.appended', 'change-1', 'revision-1', NULL,
                'revision-1', 'legacy-append', 'operator-1', 2
             );
             INSERT INTO materializations (
                materialization_id, change_id, revision_id, workspace_id,
                provider_id, current_provider_ref, state, version,
                created_at_unix_ms, created_by, state_changed_at_unix_ms
             ) VALUES (
                'materialization-1', 'change-1', 'revision-1', 'workspace-1',
                'native-git', 'refs/weft/one', 'clean', 1, 3, 'operator-1', 3
             );
             INSERT INTO operation_records (
                operation_id, event_kind, actor_id, occurred_at_unix_ms
             ) VALUES (
                'legacy-materialize', 'materialization.created', 'operator-1', 3
             );
             INSERT INTO materialization_events (
                event_kind, materialization_id, change_id, revision_id,
                expected_version, resulting_version, resulting_state,
                resulting_provider_ref, provider_evidence, operation_id
             ) VALUES (
                'materialization.created', 'materialization-1', 'change-1',
                'revision-1', 0, 1, 'clean', 'refs/weft/one',
                'native-git:tree=one', 'legacy-materialize'
             );",
            digest = "0".repeat(64)
        );
        connection.execute_batch(&sql).unwrap();
        drop(connection);

        let store = SqliteStore::open(path).unwrap();
        assert_eq!(SqliteStore::schema_version(), 7);
        assert_eq!(store.audit_events(&change_id()).unwrap().len(), 2);
        assert_eq!(store.materialization_events(&change_id()).unwrap().len(), 1);
        assert_eq!(
            store
                .connection
                .query_row("SELECT count(*) FROM materializations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
    }

    #[test]
    fn materialization_lifecycle_replays_exact_outcomes_and_survives_reopen() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        seed_revision(&mut store, &artifacts);
        let initial = materialization("materialization-1", "workspace-1", "refs/weft/one");
        store
            .create_materialization(
                &artifacts,
                &initial,
                &provider_evidence("native-git:tree=one"),
                &context("materialize-1", 3),
            )
            .unwrap();
        store
            .create_materialization(
                &artifacts,
                &initial,
                &provider_evidence("native-git:tree=one"),
                &context("materialize-1", 3),
            )
            .unwrap();
        let dirty = store
            .transition_materialization(
                &artifacts,
                initial.id(),
                MaterializationVersion::INITIAL,
                provider_observation(
                    MaterializationState::Dirty,
                    "refs/weft/dirty",
                    "native-git:worktree=dirty-1",
                ),
                &context("materialization-dirty", 4),
            )
            .unwrap();
        let dirty_replay = store
            .transition_materialization(
                &artifacts,
                initial.id(),
                MaterializationVersion::INITIAL,
                provider_observation(
                    MaterializationState::Dirty,
                    "refs/weft/dirty",
                    "native-git:worktree=dirty-1",
                ),
                &context("materialization-dirty", 4),
            )
            .unwrap();
        assert_eq!(dirty, dirty_replay);
        let conflicting_replay = store
            .transition_materialization(
                &artifacts,
                initial.id(),
                MaterializationVersion::INITIAL,
                provider_observation(
                    MaterializationState::Dirty,
                    "refs/weft/dirty",
                    "native-git:worktree=different",
                ),
                &context("materialization-dirty", 4),
            )
            .unwrap_err();
        assert!(matches!(
            conflicting_replay,
            StoreError::OperationIdConflict(_)
        ));
        let released = store
            .transition_materialization(
                &artifacts,
                initial.id(),
                dirty.version(),
                provider_observation(
                    MaterializationState::Released,
                    "refs/weft/dirty",
                    "native-git:worktree=released",
                ),
                &context("materialization-release", 5),
            )
            .unwrap();
        assert_eq!(released.revision_id().as_str(), "revision-1");
        assert_eq!(released.released_at(), Some(UnixMillis::new(5).unwrap()));
        let events = store.materialization_events(&change_id()).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[1].resulting_provider_ref.as_str(), "refs/weft/dirty");
        assert_eq!(
            events[1].provider_evidence.as_str(),
            "native-git:worktree=dirty-1"
        );
        drop(store);

        let reopened = SqliteStore::open(path).unwrap();
        assert_eq!(
            reopened.materialization(&artifacts, initial.id()).unwrap(),
            released
        );
        assert_eq!(
            reopened
                .materializations(&artifacts, &change_id())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn terminal_materialization_frees_active_placement_without_retargeting_history() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_revision(&mut store, &artifacts);
        let initial = materialization("materialization-1", "workspace-1", "refs/weft/one");
        store
            .create_materialization(
                &artifacts,
                &initial,
                &provider_evidence("native-git:tree=one"),
                &context("materialize-1", 3),
            )
            .unwrap();
        store
            .transition_materialization(
                &artifacts,
                initial.id(),
                MaterializationVersion::INITIAL,
                provider_observation(
                    MaterializationState::Invalidated,
                    "refs/weft/one",
                    "native-git:worktree=removed",
                ),
                &context("invalidate-1", 4),
            )
            .unwrap();

        let replacement = materialization_at(
            "materialization-2",
            "workspace-1",
            "refs/weft/replacement",
            5,
        );
        store
            .create_materialization(
                &artifacts,
                &replacement,
                &provider_evidence("native-git:tree=replacement"),
                &context("materialize-2", 5),
            )
            .unwrap();
        let history = store.materializations(&artifacts, &change_id()).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].state(), MaterializationState::Invalidated);
        assert_eq!(history[0].revision_id(), replacement.revision_id());
        assert_eq!(history[1], replacement);
    }

    #[test]
    fn materialization_rejects_stale_writer_and_duplicate_active_placement() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut first = SqliteStore::open(&path).unwrap();
        seed_revision(&mut first, &artifacts);
        let initial = materialization("materialization-1", "workspace-1", "refs/weft/one");
        first
            .create_materialization(
                &artifacts,
                &initial,
                &provider_evidence("native-git:tree=one"),
                &context("materialize-1", 3),
            )
            .unwrap();
        let duplicate = materialization("materialization-2", "workspace-1", "refs/weft/two");
        let duplicate_error = first
            .create_materialization(
                &artifacts,
                &duplicate,
                &provider_evidence("native-git:tree=two"),
                &context("materialize-2", 3),
            )
            .unwrap_err();
        assert!(matches!(duplicate_error, StoreError::Database(_)));

        let mut second = SqliteStore::open(&path).unwrap();
        second
            .transition_materialization(
                &artifacts,
                initial.id(),
                MaterializationVersion::INITIAL,
                provider_observation(
                    MaterializationState::Diverged,
                    "external-rewrite",
                    "native-git:observed=external-rewrite",
                ),
                &context("external-divergence", 4),
            )
            .unwrap();
        let stale = first
            .transition_materialization(
                &artifacts,
                initial.id(),
                MaterializationVersion::INITIAL,
                provider_observation(
                    MaterializationState::Dirty,
                    "dirty-ref",
                    "native-git:worktree=dirty",
                ),
                &context("stale-materialization", 5),
            )
            .unwrap_err();
        assert!(matches!(
            stale,
            StoreError::Materialization(MaterializationError::StaleVersion { .. })
        ));
        assert_eq!(first.materialization_events(&change_id()).unwrap().len(), 2);
    }

    #[test]
    fn materialization_requires_exact_durable_revision_content() {
        let (directory, path) = database();
        let artifact_path = directory.path().join("artifacts");
        let artifacts = ArtifactStore::open(&artifact_path).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_revision(&mut store, &artifacts);
        let initial = materialization("materialization-1", "workspace-1", "refs/weft/one");
        store
            .create_materialization(
                &artifacts,
                &initial,
                &provider_evidence("native-git:tree=one"),
                &context("materialize-1", 3),
            )
            .unwrap();

        let missing_revision = Materialization::new(
            MaterializationId::new("materialization-missing").unwrap(),
            change_id(),
            RevisionId::new("revision-missing").unwrap(),
            MaterializationPlacement::new(
                WorkspaceId::new("workspace-2").unwrap(),
                ProviderId::new("native-git").unwrap(),
                ProviderRef::new("missing").unwrap(),
            ),
            UnixMillis::new(4).unwrap(),
            ActorId::new("operator-1").unwrap(),
        );
        assert!(matches!(
            store.create_materialization(
                &artifacts,
                &missing_revision,
                &provider_evidence("native-git:missing"),
                &context("materialize-missing", 4)
            ),
            Err(StoreError::RevisionNotFoundForChange { .. })
        ));

        let artifact = revision(&artifacts, "revision-1", 2).artifact().clone();
        let digest = CasDigest::parse(artifact.manifest_digest()).unwrap();
        let manifest_path = artifact_path
            .join("objects")
            .join("sha256")
            .join(&digest.hex()[..2])
            .join(&digest.hex()[2..]);
        std::fs::remove_file(manifest_path).unwrap();
        assert!(matches!(
            store.materialization(&artifacts, initial.id()),
            Err(StoreError::Artifact(ArtifactStoreError::ObjectMissing(_)))
        ));
        assert!(matches!(
            store.transition_materialization(
                &artifacts,
                initial.id(),
                MaterializationVersion::INITIAL,
                provider_observation(
                    MaterializationState::Dirty,
                    "dirty",
                    "native-git:worktree=dirty",
                ),
                &context("dirty-without-content", 5)
            ),
            Err(StoreError::Artifact(ArtifactStoreError::ObjectMissing(_)))
        ));
        assert_eq!(store.materialization_events(&change_id()).unwrap().len(), 1);
    }

    #[test]
    fn materialization_reads_reject_missing_and_drifted_event_history() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_revision(&mut store, &artifacts);
        store
            .connection
            .execute(
                "INSERT INTO materializations (
                    materialization_id, change_id, revision_id, workspace_id, provider_id,
                    current_provider_ref, state, version, created_at_unix_ms, created_by,
                    state_changed_at_unix_ms
                 ) VALUES (
                    'materialization-without-event', 'change-1', 'revision-1',
                    'workspace-1', 'native-git', 'provider-ref', 'clean', 1, 3,
                    'operator-1', 3
                 )",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.materialization(
                &artifacts,
                &MaterializationId::new("materialization-without-event").unwrap()
            ),
            Err(StoreError::InvalidStoredData(_))
        ));

        let valid = Materialization::new(
            MaterializationId::new("materialization-drifted").unwrap(),
            change_id(),
            RevisionId::new("revision-1").unwrap(),
            MaterializationPlacement::new(
                WorkspaceId::new("workspace-2").unwrap(),
                ProviderId::new("native-git").unwrap(),
                ProviderRef::new("provider-ref-2").unwrap(),
            ),
            UnixMillis::new(4).unwrap(),
            ActorId::new("operator-1").unwrap(),
        );
        store
            .create_materialization(
                &artifacts,
                &valid,
                &provider_evidence("native-git:tree=valid"),
                &context("materialization-valid", 4),
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE materializations
                 SET current_provider_ref = 'provider-ref-drift', state = 'dirty',
                     version = 2, state_changed_at_unix_ms = 5
                 WHERE materialization_id = 'materialization-drifted'",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.materialization(&artifacts, valid.id()),
            Err(StoreError::InvalidStoredData(_))
        ));
    }

    #[test]
    fn materialization_reads_reject_invalid_provider_evidence() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_revision(&mut store, &artifacts);
        let initial = materialization("materialization-1", "workspace-1", "refs/weft/one");
        store
            .create_materialization(
                &artifacts,
                &initial,
                &provider_evidence("native-git:tree=one"),
                &context("materialize-1", 3),
            )
            .unwrap();

        store
            .connection
            .execute_batch(
                "PRAGMA ignore_check_constraints = ON;
                 DROP TRIGGER materialization_events_are_append_only_update;
                 UPDATE materialization_events SET provider_evidence = ''
                 WHERE materialization_id = 'materialization-1';",
            )
            .unwrap();
        assert!(matches!(
            store.materialization(&artifacts, initial.id()),
            Err(StoreError::Materialization(
                MaterializationError::EmptyIdentifier("ProviderEvidence")
            ))
        ));
    }

    #[test]
    fn relationship_lifecycle_is_symmetric_replayable_and_restart_safe() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        seed_named_change(&mut store, &artifacts, "change-a", "a-r1", 1);
        seed_named_change(&mut store, &artifacts, "change-b", "b-r1", 3);
        let initial = relationship("relationship-1", "change-b", "change-a", 5);
        store
            .create_relationship(&initial, &context("relationship-create", 5))
            .unwrap();
        store
            .create_relationship(&initial, &context("relationship-create", 5))
            .unwrap();
        let duplicate = relationship("relationship-2", "change-a", "change-b", 6);
        assert!(matches!(
            store.create_relationship(&duplicate, &context("relationship-duplicate", 6)),
            Err(StoreError::ActiveRelationshipExists)
        ));
        let removed = store
            .remove_relationship(
                initial.id(),
                RelationshipVersion::INITIAL,
                &context("relationship-remove", 7),
            )
            .unwrap();
        assert_eq!(
            store
                .remove_relationship(
                    initial.id(),
                    RelationshipVersion::INITIAL,
                    &context("relationship-remove", 7),
                )
                .unwrap(),
            removed
        );
        let replacement = relationship("relationship-2", "change-a", "change-b", 8);
        store
            .create_relationship(&replacement, &context("relationship-replacement", 8))
            .unwrap();
        assert_eq!(
            store
                .relationship_events(&ChangeId::new("change-a").unwrap())
                .unwrap()
                .len(),
            3
        );
        drop(store);

        let reopened = SqliteStore::open(path).unwrap();
        let history = reopened
            .relationships(&ChangeId::new("change-b").unwrap())
            .unwrap();
        assert_eq!(history, vec![removed, replacement]);
    }

    #[test]
    fn dependency_lifecycle_pins_exact_revisions_and_replays_historical_outcomes() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        let initial = seed_default_dependency(&mut store, &artifacts);
        let conflicting_replay = dependency(
            "dependency-conflict",
            "downstream",
            "down-r1",
            "upstream",
            "up-r1",
            5,
        );
        assert!(matches!(
            store.create_dependency(
                &artifacts,
                &conflicting_replay,
                &context("dependency-create", 5)
            ),
            Err(StoreError::OperationIdConflict(_))
        ));
        store
            .create_dependency(&artifacts, &initial, &context("dependency-create", 5))
            .unwrap();
        append_named_revision(
            &mut store,
            &artifacts,
            "downstream",
            "down-r1",
            "down-r2",
            6,
        );
        append_named_revision(&mut store, &artifacts, "upstream", "up-r1", "up-r2", 7);
        let pins = DependencyPins::new(
            RevisionId::new("down-r2").unwrap(),
            RevisionId::new("up-r2").unwrap(),
        );
        let repinned = store
            .repin_dependency(
                &artifacts,
                initial.id(),
                RelationshipVersion::INITIAL,
                pins.clone(),
                &context("dependency-repin", 8),
            )
            .unwrap();
        assert_eq!(
            store
                .repin_dependency(
                    &artifacts,
                    initial.id(),
                    RelationshipVersion::INITIAL,
                    pins,
                    &context("dependency-repin", 8),
                )
                .unwrap(),
            repinned
        );
        let removed = store
            .remove_dependency(
                &artifacts,
                initial.id(),
                repinned.version(),
                &context("dependency-remove", 9),
            )
            .unwrap();
        assert_eq!(
            store
                .repin_dependency(
                    &artifacts,
                    initial.id(),
                    RelationshipVersion::INITIAL,
                    DependencyPins::new(
                        RevisionId::new("down-r2").unwrap(),
                        RevisionId::new("up-r2").unwrap(),
                    ),
                    &context("dependency-repin", 8),
                )
                .unwrap(),
            repinned
        );
        assert_eq!(
            store
                .dependency_freshness(&artifacts, initial.id())
                .unwrap(),
            DependencyFreshness::Removed
        );
        drop(store);

        let reopened = SqliteStore::open(path).unwrap();
        assert_eq!(
            reopened.dependency(&artifacts, initial.id()).unwrap(),
            removed
        );
        assert_eq!(
            reopened
                .dependency_events(&artifacts, &ChangeId::new("upstream").unwrap())
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn stack_snapshots_use_version_cas_replay_and_detect_projection_drift() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        seed_named_change(&mut store, &artifacts, "change-a", "a-r1", 1);
        seed_named_change(&mut store, &artifacts, "change-b", "b-r1", 3);
        let initial = stack("stack-1", StackPolicy::OrderOnly, &["change-a"], 5);
        store
            .create_stack(&initial, &context("stack-create", 5))
            .unwrap();
        store
            .create_stack(&initial, &context("stack-create", 5))
            .unwrap();
        let replacement = StackDefinition::from_changes(
            StackPolicy::PredecessorDependencies,
            vec![
                ChangeId::new("change-a").unwrap(),
                ChangeId::new("change-b").unwrap(),
            ],
        )
        .unwrap();
        let revised = store
            .replace_stack(
                initial.id(),
                StackVersion::INITIAL,
                replacement.clone(),
                &context("stack-revise", 6),
            )
            .unwrap();
        assert_eq!(revised.version(), StackVersion::new(2).unwrap());
        assert_eq!(
            store
                .replace_stack(
                    initial.id(),
                    StackVersion::INITIAL,
                    replacement,
                    &context("stack-revise", 6),
                )
                .unwrap(),
            revised
        );
        assert!(matches!(
            store.replace_stack(
                initial.id(),
                StackVersion::INITIAL,
                StackDefinition::from_changes(
                    StackPolicy::OrderOnly,
                    vec![ChangeId::new("change-b").unwrap()]
                )
                .unwrap(),
                &context("stack-stale", 7)
            ),
            Err(StoreError::Composition(
                CompositionError::StaleStackVersion { .. }
            ))
        ));
        drop(store);

        let reopened = SqliteStore::open(&path).unwrap();
        assert_eq!(reopened.stack(initial.id()).unwrap(), revised);
        let creation_event = reopened
            .connection
            .query_row(
                "SELECT event_id FROM stack_events
                 WHERE stack_id = ?1 AND resulting_version = 1",
                [initial.id().as_str()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        let late_member = reopened
            .connection
            .execute(
                "INSERT INTO stack_event_members (
                    event_id, position, change_id, predecessor_change_id
                 ) VALUES (?1, 1, 'change-b', 'change-a')",
                [creation_event],
            )
            .unwrap_err();
        assert!(late_member.to_string().contains("finalized size"));
        reopened
            .connection
            .execute(
                "UPDATE stacks SET updated_by = 'drift' WHERE stack_id = ?1",
                [initial.id().as_str()],
            )
            .unwrap();
        assert!(matches!(
            reopened.stack(initial.id()),
            Err(StoreError::InvalidStoredData(_))
        ));
    }

    #[test]
    fn candidate_resolution_is_exact_replayable_and_derives_staleness() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        let dependency = seed_default_dependency(&mut store, &artifacts);
        let initial_stack = stack(
            "stack-1",
            StackPolicy::PredecessorDependencies,
            &["upstream", "downstream"],
            6,
        );
        store
            .create_stack(&initial_stack, &context("stack-create", 6))
            .unwrap();
        let candidate_id = CandidateId::new("candidate-1").unwrap();
        let target = BaseState::new(
            RepositoryId::new("repository-1").unwrap(),
            "integration-target-1",
        )
        .unwrap();
        let selection = CandidateSelection::Stack {
            stack_id: initial_stack.id().clone(),
            expected_version: StackVersion::INITIAL,
        };
        let candidate = store
            .create_candidate(
                &artifacts,
                candidate_id.clone(),
                target.clone(),
                &selection,
                &context("candidate-create", 7),
            )
            .unwrap();
        assert_eq!(candidate.inputs().len(), 2);
        assert_eq!(candidate.requirements().len(), 2);
        assert!(
            store
                .candidate_freshness(&artifacts, &candidate_id)
                .unwrap()
                .is_current()
        );
        append_named_revision(
            &mut store,
            &artifacts,
            "downstream",
            "down-r1",
            "down-r2",
            8,
        );
        let freshness = store
            .candidate_freshness(&artifacts, &candidate_id)
            .unwrap();
        assert_eq!(
            freshness.advanced_inputs,
            vec![ChangeId::new("downstream").unwrap()]
        );
        assert!(matches!(
            store.create_candidate(
                &artifacts,
                CandidateId::new("candidate-stale").unwrap(),
                target.clone(),
                &selection,
                &context("candidate-stale", 9)
            ),
            Err(StoreError::StaleCandidateDependency(_))
        ));
        store
            .repin_dependency(
                &artifacts,
                dependency.id(),
                RelationshipVersion::INITIAL,
                DependencyPins::new(
                    RevisionId::new("down-r2").unwrap(),
                    RevisionId::new("up-r1").unwrap(),
                ),
                &context("dependency-repin-candidate", 10),
            )
            .unwrap();
        let changed = store
            .candidate_freshness(&artifacts, &candidate_id)
            .unwrap();
        assert_eq!(changed.changed_dependencies, vec![dependency.id().clone()]);
        let replay = store
            .create_candidate(
                &artifacts,
                candidate_id.clone(),
                target,
                &selection,
                &context("candidate-create", 99),
            )
            .unwrap();
        assert_eq!(replay, candidate);
        drop(store);

        let reopened = SqliteStore::open(path).unwrap();
        assert_eq!(
            reopened.candidate(&artifacts, &candidate_id).unwrap(),
            candidate
        );
    }

    #[test]
    fn candidate_rejects_missing_upstream_reversed_order_and_repository_mismatch() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_default_dependency(&mut store, &artifacts);
        let target = BaseState::new(
            RepositoryId::new("repository-1").unwrap(),
            "integration-target-1",
        )
        .unwrap();
        assert!(matches!(
            store.create_candidate(
                &artifacts,
                CandidateId::new("missing-upstream").unwrap(),
                target.clone(),
                &CandidateSelection::Changes(vec![ChangeId::new("downstream").unwrap()]),
                &context("candidate-missing", 6)
            ),
            Err(StoreError::CandidateMissingUpstream { .. })
        ));
        assert!(matches!(
            store.create_candidate(
                &artifacts,
                CandidateId::new("reversed").unwrap(),
                target,
                &CandidateSelection::Changes(vec![
                    ChangeId::new("downstream").unwrap(),
                    ChangeId::new("upstream").unwrap()
                ]),
                &context("candidate-reversed", 6)
            ),
            Err(StoreError::CandidateDependencyOrder(_))
        ));
        assert!(matches!(
            store.create_candidate(
                &artifacts,
                CandidateId::new("wrong-repository").unwrap(),
                BaseState::new(RepositoryId::new("other-repository").unwrap(), "target").unwrap(),
                &CandidateSelection::Changes(vec![ChangeId::new("upstream").unwrap()]),
                &context("candidate-wrong-repository", 6)
            ),
            Err(StoreError::CandidateRepositoryMismatch(_))
        ));
    }

    #[test]
    fn candidate_read_rejects_nonexistent_requirement_provenance() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_named_change(&mut store, &artifacts, "upstream", "up-r1", 1);
        seed_named_change(&mut store, &artifacts, "downstream", "down-r1", 3);
        let inputs = vec![
            CandidateInput::new(
                ChangeId::new("upstream").unwrap(),
                RevisionId::new("up-r1").unwrap(),
            ),
            CandidateInput::new(
                ChangeId::new("downstream").unwrap(),
                RevisionId::new("down-r1").unwrap(),
            ),
        ];
        let candidate = CompositionCandidate::new(
            CandidateId::new("fabricated-source").unwrap(),
            BaseState::new(RepositoryId::new("repository-1").unwrap(), "target").unwrap(),
            None,
            inputs.clone(),
            vec![ResolvedRequirement::new(
                ResolvedRequirementSource::Dependency {
                    dependency_id: DependencyId::new("missing-dependency").unwrap(),
                    version: RelationshipVersion::INITIAL,
                },
                inputs[1].clone(),
                inputs[0].clone(),
            )],
            UnixMillis::new(5).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
        .unwrap();
        let transaction = store
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        persist_candidate(
            &transaction,
            &candidate,
            &context("fabricated-candidate", 5),
        )
        .unwrap();
        transaction.commit().unwrap();
        assert!(matches!(
            store.candidate(&artifacts, candidate.id()),
            Err(StoreError::DependencyNotFound(_))
        ));
    }

    #[test]
    fn concurrent_populated_v4_upgrade_preserves_relationship_dependency_history() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        let dependency = seed_default_dependency(&mut store, &artifacts);
        let relation = relationship("relationship-1", "downstream", "upstream", 6);
        store
            .create_relationship(&relation, &context("relationship-create", 6))
            .unwrap();
        drop(store);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 DROP TABLE conflict_resolution_validation_refs;
                 DROP TABLE conflict_resolutions;
                 DROP TABLE integration_receipts;
                 DROP TABLE integration_conflict_inputs;
                 DROP TABLE integration_conflicts;
                 DROP TABLE integration_events;
                 DROP TABLE integration_attempt_validation_refs;
                 DROP TABLE integration_attempt_review_refs;
                 DROP TABLE integration_attempt_inputs;
                 DROP TABLE integration_attempts;
                 DROP TABLE validation_results;
                 DROP TABLE review_submissions;
                 DROP TABLE review_request_reviewers;
                 DROP TABLE review_requests;
                 DROP TABLE candidate_requirements;
                 DROP TABLE candidate_inputs;
                 DROP TABLE composition_candidates;
                 DROP TABLE stack_event_members;
                 DROP TABLE stack_events;
                 DROP TABLE stack_members;
                 DROP TABLE stacks;
                 PRAGMA user_version = 4;",
            )
            .unwrap();
        drop(connection);

        concurrently_open(&path);
        let migrated = SqliteStore::open(path).unwrap();
        assert_eq!(
            migrated.dependency(&artifacts, dependency.id()).unwrap(),
            dependency
        );
        assert_eq!(
            migrated
                .relationships(&ChangeId::new("upstream").unwrap())
                .unwrap(),
            vec![relation]
        );
        assert_eq!(
            migrated
                .dependency_events(&artifacts, &ChangeId::new("downstream").unwrap())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn revision_review_history_is_exact_replayable_and_becomes_stale() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        seed_revision(&mut store, &artifacts);
        let target = store
            .revision_target(
                &artifacts,
                &change_id(),
                &RevisionId::new("revision-1").unwrap(),
            )
            .unwrap();
        let request = review_request(target, 3);
        store
            .create_review_request(&artifacts, &request, &context("review-request", 3))
            .unwrap();
        store
            .create_review_request(&artifacts, &request, &context("review-request", 3))
            .unwrap();
        let approved = ReviewSubmission::new(
            ReviewSubmissionId::new("submission-1").unwrap(),
            &request,
            ActorId::new("reviewer-1").unwrap(),
            ReviewOutcome::Approved,
            Some("reviewed exact revision".to_owned()),
            UnixMillis::new(4).unwrap(),
        )
        .unwrap();
        store
            .create_review_submission(
                &artifacts,
                &approved,
                &reviewer_context("review-submit-1", "reviewer-1", 4),
            )
            .unwrap();
        store
            .create_review_submission(
                &artifacts,
                &approved,
                &reviewer_context("review-submit-1", "reviewer-1", 4),
            )
            .unwrap();
        let follow_up = ReviewSubmission::new(
            ReviewSubmissionId::new("submission-2").unwrap(),
            &request,
            ActorId::new("reviewer-1").unwrap(),
            ReviewOutcome::ChangesRequested,
            None,
            UnixMillis::new(5).unwrap(),
        )
        .unwrap();
        store
            .create_review_submission(
                &artifacts,
                &follow_up,
                &reviewer_context("review-submit-2", "reviewer-1", 5),
            )
            .unwrap();
        assert_eq!(
            store.review_submissions(&artifacts, request.id()).unwrap(),
            vec![approved.clone(), follow_up]
        );
        assert!(
            store
                .review_request_freshness(&artifacts, request.id())
                .unwrap()
                .is_current()
        );
        append_named_revision(
            &mut store,
            &artifacts,
            "change-1",
            "revision-1",
            "revision-2",
            6,
        );
        assert_eq!(
            store
                .review_request_freshness(&artifacts, request.id())
                .unwrap(),
            ExactTargetFreshness::RevisionAdvanced
        );
        assert_eq!(
            store.review_submission(&artifacts, approved.id()).unwrap(),
            approved
        );
        drop(store);

        let reopened = SqliteStore::open(path).unwrap();
        assert_eq!(
            reopened.review_request(&artifacts, request.id()).unwrap(),
            request
        );
    }

    #[test]
    fn review_operation_ids_reject_payload_conflicts() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_revision(&mut store, &artifacts);
        let target = store
            .revision_target(
                &artifacts,
                &change_id(),
                &RevisionId::new("revision-1").unwrap(),
            )
            .unwrap();
        let request = review_request(target, 3);
        store
            .create_review_request(&artifacts, &request, &context("review-request", 3))
            .unwrap();
        let reversed_replay = ReviewRequest::new(
            request.id().clone(),
            request.target().clone(),
            request.requested_by().clone(),
            request.reviewers().iter().rev().cloned().collect(),
            request.created_at(),
        )
        .unwrap();
        store
            .create_review_request(&artifacts, &reversed_replay, &context("review-request", 3))
            .unwrap();
        let conflicting_request = ReviewRequest::new(
            ReviewRequestId::new("review-request-conflict").unwrap(),
            request.target().clone(),
            request.requested_by().clone(),
            request.reviewers().to_vec(),
            request.created_at(),
        )
        .unwrap();
        assert!(matches!(
            store.create_review_request(
                &artifacts,
                &conflicting_request,
                &context("review-request", 3)
            ),
            Err(StoreError::OperationIdConflict(_))
        ));
        let submission = ReviewSubmission::new(
            ReviewSubmissionId::new("submission-1").unwrap(),
            &request,
            ActorId::new("reviewer-1").unwrap(),
            ReviewOutcome::Approved,
            None,
            UnixMillis::new(4).unwrap(),
        )
        .unwrap();
        store
            .create_review_submission(
                &artifacts,
                &submission,
                &reviewer_context("review-submit", "reviewer-1", 4),
            )
            .unwrap();
        let conflicting_submission = ReviewSubmission::new(
            ReviewSubmissionId::new("submission-conflict").unwrap(),
            &request,
            ActorId::new("reviewer-1").unwrap(),
            ReviewOutcome::Approved,
            None,
            UnixMillis::new(4).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            store.create_review_submission(
                &artifacts,
                &conflicting_submission,
                &reviewer_context("review-submit", "reviewer-1", 4)
            ),
            Err(StoreError::OperationIdConflict(_))
        ));
    }

    #[test]
    fn validation_reusable_scope_never_overrides_factual_revision_staleness() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_revision(&mut store, &artifacts);
        let target = store
            .revision_target(
                &artifacts,
                &change_id(),
                &RevisionId::new("revision-1").unwrap(),
            )
            .unwrap();
        let result = ValidationResult::new(
            ValidationResultId::new("validation-1").unwrap(),
            target,
            ValidationObservation::new(
                ValidationType::new("test").unwrap(),
                ValidationEnvironment::new("linux-x86_64").unwrap(),
                ValidationOutcome::Passed,
                ValidationExecutionId::new("execution-1").unwrap(),
                ValidationScope::declared_reusable(
                    "compiler-independent-unit-tests",
                    "inputs exclude platform-specific behavior",
                )
                .unwrap(),
            ),
            ActorId::new("operator-1").unwrap(),
            UnixMillis::new(3).unwrap(),
        );
        store
            .create_validation_result(&artifacts, &result, &context("validation-record", 3))
            .unwrap();
        let conflicting_result = ValidationResult::new(
            ValidationResultId::new("validation-conflict").unwrap(),
            result.target().clone(),
            ValidationObservation::new(
                result.validation_type().clone(),
                result.environment().clone(),
                result.outcome(),
                result.execution_id().clone(),
                result.scope().clone(),
            ),
            result.validated_by().clone(),
            result.validated_at(),
        );
        assert!(matches!(
            store.create_validation_result(
                &artifacts,
                &conflicting_result,
                &context("validation-record", 3)
            ),
            Err(StoreError::OperationIdConflict(_))
        ));
        store
            .create_validation_result(&artifacts, &result, &context("validation-record", 3))
            .unwrap();
        assert!(
            store
                .validation_result_freshness(&artifacts, result.id())
                .unwrap()
                .is_current()
        );
        append_named_revision(
            &mut store,
            &artifacts,
            "change-1",
            "revision-1",
            "revision-2",
            4,
        );
        assert_eq!(
            store
                .validation_result_freshness(&artifacts, result.id())
                .unwrap(),
            ExactTargetFreshness::RevisionAdvanced
        );
        assert!(matches!(
            store
                .validation_result(&artifacts, result.id())
                .unwrap()
                .scope(),
            ValidationScope::DeclaredReusable { .. }
        ));
    }

    #[test]
    fn candidate_validation_derives_candidate_input_staleness() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_named_change(
            &mut store,
            &artifacts,
            "candidate-change",
            "candidate-r1",
            1,
        );
        let candidate = store
            .create_candidate(
                &artifacts,
                CandidateId::new("candidate-review-target").unwrap(),
                BaseState::new(
                    RepositoryId::new("repository-1").unwrap(),
                    "integration-target",
                )
                .unwrap(),
                &CandidateSelection::Changes(vec![ChangeId::new("candidate-change").unwrap()]),
                &context("candidate-review-create", 3),
            )
            .unwrap();
        let result = ValidationResult::new(
            ValidationResultId::new("candidate-validation").unwrap(),
            store.candidate_target(&artifacts, candidate.id()).unwrap(),
            ValidationObservation::new(
                ValidationType::new("build").unwrap(),
                ValidationEnvironment::new("linux").unwrap(),
                ValidationOutcome::Passed,
                ValidationExecutionId::new("build-1").unwrap(),
                ValidationScope::ExactTarget,
            ),
            ActorId::new("operator-1").unwrap(),
            UnixMillis::new(4).unwrap(),
        );
        store
            .create_validation_result(&artifacts, &result, &context("candidate-validation", 4))
            .unwrap();
        append_named_revision(
            &mut store,
            &artifacts,
            "candidate-change",
            "candidate-r1",
            "candidate-r2",
            5,
        );
        let ExactTargetFreshness::CandidateStale(freshness) = store
            .validation_result_freshness(&artifacts, result.id())
            .unwrap()
        else {
            panic!("candidate validation should become stale");
        };
        assert_eq!(
            freshness.advanced_inputs,
            vec![ChangeId::new("candidate-change").unwrap()]
        );
    }

    #[test]
    fn review_validation_rows_reject_mutation_outsiders_and_source_drift() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_revision(&mut store, &artifacts);
        let exact = store
            .revision_target(
                &artifacts,
                &change_id(),
                &RevisionId::new("revision-1").unwrap(),
            )
            .unwrap();
        let request = review_request(exact.clone(), 3);
        store
            .create_review_request(&artifacts, &request, &context("review-request", 3))
            .unwrap();
        let late_reviewer = store
            .connection
            .execute(
                "INSERT INTO review_request_reviewers (
                review_request_id, reviewer_position, reviewer_id
             ) VALUES (?1, 2, 'late-reviewer')",
                [request.id().as_str()],
            )
            .unwrap_err();
        assert!(late_reviewer.to_string().contains("finalized request size"));
        let delete_reviewer = store
            .connection
            .execute(
                "DELETE FROM review_request_reviewers WHERE review_request_id = ?1",
                [request.id().as_str()],
            )
            .unwrap_err();
        assert!(delete_reviewer.to_string().contains("cannot be deleted"));

        let fabricated = ReviewRequest::new(
            ReviewRequestId::new("fabricated-review").unwrap(),
            ExactTarget::revision(
                change_id(),
                RevisionId::new("revision-1").unwrap(),
                exact.context().clone(),
                format!("sha256:{}", "f".repeat(64)),
            )
            .unwrap(),
            ActorId::new("operator-1").unwrap(),
            vec![ActorId::new("reviewer-1").unwrap()],
            UnixMillis::new(4).unwrap(),
        )
        .unwrap();
        let transaction = store.connection.transaction().unwrap();
        insert_operation_record(
            &transaction,
            "review.requested",
            &context("fabricated-review", 4),
        )
        .unwrap();
        insert_review_request(&transaction, &fabricated, &context("fabricated-review", 4)).unwrap();
        transaction.commit().unwrap();
        assert!(matches!(
            store.review_request(&artifacts, fabricated.id()),
            Err(StoreError::ExactTargetMismatch)
        ));

        let outsider_operation = reviewer_context("outsider-submit", "outsider", 5);
        store
            .connection
            .execute(
                "INSERT INTO operation_records (
                operation_id, event_kind, actor_id, occurred_at_unix_ms
             ) VALUES (?1, 'review.submitted', 'outsider', 5)",
                [outsider_operation.operation_id()],
            )
            .unwrap();
        let outsider = store
            .connection
            .execute(
                "INSERT INTO review_submissions (
                review_submission_id, review_request_id, reviewer_id, outcome,
                submitted_at_unix_ms, operation_id
             ) VALUES ('outsider-submission', ?1, 'outsider', 'approved', 5, ?2)",
                params![request.id().as_str(), outsider_operation.operation_id()],
            )
            .unwrap_err();
        assert!(outsider.to_string().contains("FOREIGN KEY constraint"));
    }

    #[test]
    fn review_validation_evidence_is_temporal_and_immutable() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_revision(&mut store, &artifacts);
        let target = store
            .revision_target(
                &artifacts,
                &change_id(),
                &RevisionId::new("revision-1").unwrap(),
            )
            .unwrap();
        let early = review_request(target.clone(), 1);
        assert!(matches!(
            store.create_review_request(&artifacts, &early, &context("early-review", 1)),
            Err(StoreError::EvidenceBeforeTarget)
        ));
        let request = review_request(target.clone(), 3);
        store
            .create_review_request(&artifacts, &request, &context("review-request", 3))
            .unwrap();
        let submission = ReviewSubmission::new(
            ReviewSubmissionId::new("submission-1").unwrap(),
            &request,
            ActorId::new("reviewer-1").unwrap(),
            ReviewOutcome::Approved,
            None,
            UnixMillis::new(4).unwrap(),
        )
        .unwrap();
        store
            .create_review_submission(
                &artifacts,
                &submission,
                &reviewer_context("submission-1", "reviewer-1", 4),
            )
            .unwrap();
        let validation = ValidationResult::new(
            ValidationResultId::new("validation-1").unwrap(),
            target,
            ValidationObservation::new(
                ValidationType::new("lint").unwrap(),
                ValidationEnvironment::new("linux").unwrap(),
                ValidationOutcome::Passed,
                ValidationExecutionId::new("lint-1").unwrap(),
                ValidationScope::ExactTarget,
            ),
            ActorId::new("operator-1").unwrap(),
            UnixMillis::new(4).unwrap(),
        );
        store
            .create_validation_result(&artifacts, &validation, &context("validation-1", 4))
            .unwrap();
        for (sql, marker) in [
            (
                "UPDATE review_requests SET requested_by = 'drift'",
                "immutable",
            ),
            (
                "UPDATE review_submissions SET outcome = 'rejected'",
                "immutable",
            ),
            ("DELETE FROM validation_results", "cannot be deleted"),
        ] {
            let error = store.connection.execute(sql, []).unwrap_err();
            assert!(error.to_string().contains(marker));
        }
    }

    #[test]
    fn concurrent_populated_v5_upgrade_preserves_candidate_history() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        seed_named_change(&mut store, &artifacts, "change-v5", "revision-v5", 1);
        let candidate = store
            .create_candidate(
                &artifacts,
                CandidateId::new("candidate-v5").unwrap(),
                BaseState::new(RepositoryId::new("repository-1").unwrap(), "target-v5").unwrap(),
                &CandidateSelection::Changes(vec![ChangeId::new("change-v5").unwrap()]),
                &context("candidate-v5", 3),
            )
            .unwrap();
        drop(store);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
             DROP TABLE conflict_resolution_validation_refs;
             DROP TABLE conflict_resolutions;
             DROP TABLE integration_receipts;
             DROP TABLE integration_conflict_inputs;
             DROP TABLE integration_conflicts;
             DROP TABLE integration_events;
             DROP TABLE integration_attempt_validation_refs;
             DROP TABLE integration_attempt_review_refs;
             DROP TABLE integration_attempt_inputs;
             DROP TABLE integration_attempts;
             DROP TABLE validation_results;
             DROP TABLE review_submissions;
             DROP TABLE review_request_reviewers;
             DROP TABLE review_requests;
             PRAGMA user_version = 5;",
            )
            .unwrap();
        drop(connection);

        concurrently_open(&path);
        let migrated = SqliteStore::open(path).unwrap();
        assert_eq!(
            migrated.candidate(&artifacts, candidate.id()).unwrap(),
            candidate
        );
    }

    #[test]
    fn dependency_freshness_tracks_both_heads_without_retargeting_pins() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_named_change(&mut store, &artifacts, "downstream", "down-r1", 1);
        seed_named_change(&mut store, &artifacts, "upstream", "up-r1", 3);
        let initial = dependency(
            "dependency-1",
            "downstream",
            "down-r1",
            "upstream",
            "up-r1",
            5,
        );
        store
            .create_dependency(&artifacts, &initial, &context("dependency-create", 5))
            .unwrap();
        assert_eq!(
            store
                .dependency_freshness(&artifacts, initial.id())
                .unwrap(),
            DependencyFreshness::Current
        );
        append_named_revision(&mut store, &artifacts, "upstream", "up-r1", "up-r2", 6);
        assert_eq!(
            store
                .dependency_freshness(&artifacts, initial.id())
                .unwrap(),
            DependencyFreshness::UpstreamAdvanced
        );
        append_named_revision(
            &mut store,
            &artifacts,
            "downstream",
            "down-r1",
            "down-r2",
            7,
        );
        assert_eq!(
            store
                .dependency_freshness(&artifacts, initial.id())
                .unwrap(),
            DependencyFreshness::BothAdvanced
        );
        let stored = store.dependency(&artifacts, initial.id()).unwrap();
        assert_eq!(stored.pins(), initial.pins());
    }

    #[test]
    fn concurrent_opposite_dependencies_commit_only_one_acyclic_edge() {
        let (directory, path) = database();
        let artifact_path = directory.path().join("artifacts");
        let artifacts = ArtifactStore::open(&artifact_path).unwrap();
        let mut store = SqliteStore::open(&path).unwrap();
        seed_named_change(&mut store, &artifacts, "change-a", "a-r1", 1);
        seed_named_change(&mut store, &artifacts, "change-b", "b-r1", 3);
        drop(store);

        let barrier = Arc::new(Barrier::new(3));
        let handles: Vec<_> = [
            ("dependency-a-b", "change-a", "a-r1", "change-b", "b-r1"),
            ("dependency-b-a", "change-b", "b-r1", "change-a", "a-r1"),
        ]
        .into_iter()
        .map(|(id, downstream, down_revision, upstream, up_revision)| {
            let path = path.clone();
            let artifact_path = artifact_path.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let artifacts = ArtifactStore::open(artifact_path).unwrap();
                let mut store = SqliteStore::open(path).unwrap();
                let value = dependency(id, downstream, down_revision, upstream, up_revision, 5);
                barrier.wait();
                store
                    .create_dependency(&artifacts, &value, &context(&format!("create-{id}"), 5))
                    .map_err(|error| error.to_string())
            })
        })
        .collect();
        barrier.wait();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| result
                    .as_ref()
                    .is_err_and(|error| error.contains("would create a cycle")))
                .count(),
            1
        );
        let reopened = SqliteStore::open(path).unwrap();
        assert_eq!(
            reopened
                .dependencies(&artifacts, &ChangeId::new("change-a").unwrap())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn dependency_cycle_check_rejects_a_multi_hop_back_edge() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_named_change(&mut store, &artifacts, "change-a", "a-r1", 1);
        seed_named_change(&mut store, &artifacts, "change-b", "b-r1", 3);
        seed_named_change(&mut store, &artifacts, "change-c", "c-r1", 5);
        let a_to_b = dependency("dependency-a-b", "change-a", "a-r1", "change-b", "b-r1", 7);
        let b_to_c = dependency("dependency-b-c", "change-b", "b-r1", "change-c", "c-r1", 8);
        let c_to_a = dependency("dependency-c-a", "change-c", "c-r1", "change-a", "a-r1", 9);
        store
            .create_dependency(&artifacts, &a_to_b, &context("dependency-a-b", 7))
            .unwrap();
        store
            .create_dependency(&artifacts, &b_to_c, &context("dependency-b-c", 8))
            .unwrap();
        assert!(matches!(
            store.create_dependency(&artifacts, &c_to_a, &context("dependency-c-a", 9)),
            Err(StoreError::DependencyCycle)
        ));
        assert_eq!(
            store
                .dependencies(&artifacts, &ChangeId::new("change-b").unwrap())
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn dependency_rejects_wrong_revision_ownership_and_projection_drift() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_named_change(&mut store, &artifacts, "downstream", "down-r1", 1);
        seed_named_change(&mut store, &artifacts, "upstream", "up-r1", 3);
        let wrong = dependency(
            "dependency-wrong",
            "downstream",
            "up-r1",
            "upstream",
            "down-r1",
            5,
        );
        assert!(matches!(
            store.create_dependency(&artifacts, &wrong, &context("dependency-wrong", 5)),
            Err(StoreError::RevisionNotFoundForChange { .. })
        ));
        let valid = dependency(
            "dependency-1",
            "downstream",
            "down-r1",
            "upstream",
            "up-r1",
            5,
        );
        store
            .create_dependency(&artifacts, &valid, &context("dependency-create", 5))
            .unwrap();
        append_named_revision(
            &mut store,
            &artifacts,
            "downstream",
            "down-r1",
            "down-r2",
            6,
        );
        store
            .connection
            .execute(
                "UPDATE dependencies
                 SET downstream_revision_id = 'down-r2', version = 2,
                     updated_at_unix_ms = 7, updated_by = 'operator-1'
                 WHERE dependency_id = 'dependency-1'",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.dependency(&artifacts, valid.id()),
            Err(StoreError::InvalidStoredData(_))
        ));
        assert!(matches!(
            store.dependency_events(&artifacts, &ChangeId::new("downstream").unwrap()),
            Err(StoreError::InvalidStoredData(_))
        ));
    }

    #[test]
    fn contextual_kinds_coexist_and_event_listing_rejects_projection_drift() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_named_change(&mut store, &artifacts, "change-a", "a-r1", 1);
        seed_named_change(&mut store, &artifacts, "change-b", "b-r1", 3);
        let related = relationship("related-1", "change-a", "change-b", 5);
        store
            .create_relationship(&related, &context("related-create", 5))
            .unwrap();
        let decomposition = Relationship::new(
            RelationshipId::new("decomposition-1").unwrap(),
            RelationshipKind::TaskDecomposition,
            RelationshipEndpoints::new(
                ChangeId::new("change-b").unwrap(),
                ChangeId::new("change-a").unwrap(),
            )
            .unwrap(),
            UnixMillis::new(6).unwrap(),
            ActorId::new("operator-1").unwrap(),
        );
        store
            .create_relationship(&decomposition, &context("decomposition-create", 6))
            .unwrap();
        assert_eq!(
            store
                .relationships(&ChangeId::new("change-a").unwrap())
                .unwrap()
                .len(),
            2
        );
        store
            .connection
            .execute(
                "UPDATE relationships
                 SET version = 2, removed_at_unix_ms = 7, removed_by = 'operator-1'
                 WHERE relationship_id = 'decomposition-1'",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.relationship_events(&ChangeId::new("change-a").unwrap()),
            Err(StoreError::InvalidStoredData(_))
        ));
    }

    #[test]
    fn dependency_rejects_stale_independent_writer_without_partial_event() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut first = SqliteStore::open(&path).unwrap();
        seed_named_change(&mut first, &artifacts, "downstream", "down-r1", 1);
        seed_named_change(&mut first, &artifacts, "upstream", "up-r1", 3);
        let initial = dependency(
            "dependency-1",
            "downstream",
            "down-r1",
            "upstream",
            "up-r1",
            5,
        );
        first
            .create_dependency(&artifacts, &initial, &context("dependency-create", 5))
            .unwrap();
        append_named_revision(
            &mut first,
            &artifacts,
            "downstream",
            "down-r1",
            "down-r2",
            6,
        );
        append_named_revision(&mut first, &artifacts, "upstream", "up-r1", "up-r2", 7);
        append_named_revision(
            &mut first,
            &artifacts,
            "downstream",
            "down-r2",
            "down-r3",
            8,
        );
        append_named_revision(&mut first, &artifacts, "upstream", "up-r2", "up-r3", 9);

        let mut second = SqliteStore::open(&path).unwrap();
        second
            .repin_dependency(
                &artifacts,
                initial.id(),
                RelationshipVersion::INITIAL,
                DependencyPins::new(
                    RevisionId::new("down-r2").unwrap(),
                    RevisionId::new("up-r2").unwrap(),
                ),
                &context("dependency-repin-second", 10),
            )
            .unwrap();
        let stale = first
            .repin_dependency(
                &artifacts,
                initial.id(),
                RelationshipVersion::INITIAL,
                DependencyPins::new(
                    RevisionId::new("down-r3").unwrap(),
                    RevisionId::new("up-r3").unwrap(),
                ),
                &context("dependency-repin-stale", 11),
            )
            .unwrap_err();
        assert!(matches!(
            stale,
            StoreError::Relationship(RelationshipError::StaleVersion { .. })
        ));
        assert_eq!(
            first
                .dependency_events(&artifacts, &ChangeId::new("downstream").unwrap())
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn relationship_and_dependency_reads_fail_closed_on_missing_history_or_content() {
        let (directory, path) = database();
        let artifact_path = directory.path().join("artifacts");
        let artifacts = ArtifactStore::open(&artifact_path).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        seed_named_change(&mut store, &artifacts, "downstream", "down-r1", 1);
        seed_named_change(&mut store, &artifacts, "upstream", "up-r1", 3);
        store
            .connection
            .execute(
                "INSERT INTO relationships (
                    relationship_id, relationship_kind, first_change_id,
                    second_change_id, created_at_unix_ms, created_by, version
                 ) VALUES (
                    'relationship-without-event', 'related_to', 'downstream',
                    'upstream', 5, 'operator-1', 1
                 )",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.relationship(&RelationshipId::new("relationship-without-event").unwrap()),
            Err(StoreError::InvalidStoredData(_))
        ));
        assert!(matches!(
            store.relationship_events(&ChangeId::new("downstream").unwrap()),
            Err(StoreError::InvalidStoredData(_))
        ));

        let value = dependency(
            "dependency-1",
            "downstream",
            "down-r1",
            "upstream",
            "up-r1",
            5,
        );
        store
            .create_dependency(&artifacts, &value, &context("dependency-create", 5))
            .unwrap();
        let upstream = store
            .load_change(&artifacts, &ChangeId::new("upstream").unwrap())
            .unwrap();
        let artifact = upstream.revisions()[0].artifact().clone();
        let digest = CasDigest::parse(artifact.manifest_digest()).unwrap();
        let manifest_path = artifact_path
            .join("objects")
            .join("sha256")
            .join(&digest.hex()[..2])
            .join(&digest.hex()[2..]);
        std::fs::remove_file(manifest_path).unwrap();
        assert!(matches!(
            store.dependency(&artifacts, value.id()),
            Err(StoreError::Artifact(ArtifactStoreError::ObjectMissing(_)))
        ));
        assert!(matches!(
            store.dependency_events(&artifacts, &ChangeId::new("downstream").unwrap()),
            Err(StoreError::Artifact(ArtifactStoreError::ObjectMissing(_)))
        ));
    }

    #[test]
    fn overlapping_assignments_release_with_versioned_durable_events() {
        let (_directory, path) = database();
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .create_change(&change_id(), &context("create-1", 1))
            .unwrap();
        let first = assignment("assignment-1", "agent-1", 2);
        let second = assignment("assignment-2", "agent-2", 3);
        store
            .create_assignment(&first, &context("assign-1", 2))
            .unwrap();
        store
            .create_assignment(&second, &context("assign-2", 3))
            .unwrap();
        store
            .create_assignment(&first, &context("assign-1", 2))
            .unwrap();
        let cross_kind_operation = store
            .create_assignment(
                &assignment("assignment-conflict", "agent-3", 2),
                &context("create-1", 2),
            )
            .unwrap_err();
        assert!(matches!(
            cross_kind_operation,
            StoreError::OperationIdConflict(_)
        ));

        let stale = store
            .release_assignment(
                first.id(),
                CoordinationVersion::EMPTY,
                &context("release-stale", 4),
            )
            .unwrap_err();
        assert!(matches!(
            stale,
            StoreError::Coordination(CoordinationError::StaleVersion { .. })
        ));
        let released = store
            .release_assignment(
                first.id(),
                CoordinationVersion::INITIAL,
                &context("release-1", 4),
            )
            .unwrap();
        let replay = store
            .release_assignment(
                first.id(),
                CoordinationVersion::INITIAL,
                &context("release-1", 99),
            )
            .unwrap();

        assert_eq!(released, replay);
        assert_eq!(store.assignments(&change_id()).unwrap().len(), 2);
        assert!(store.assignments(&change_id()).unwrap()[1].is_active());
        let events = store.assignment_events(&change_id()).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].event_kind, "assignment.released");
        assert_eq!(events[2].expected_version.value(), 1);
        assert_eq!(events[2].resulting_version.value(), 2);

        drop(store);
        let reopened = SqliteStore::open(&path).unwrap();
        assert_eq!(reopened.assignment_events(&change_id()).unwrap(), events);
        assert!(!reopened.assignments(&change_id()).unwrap()[0].is_active());
    }

    #[test]
    fn lease_scope_rejects_competitors_and_supports_renew_release_and_replay() {
        let (_directory, path) = database();
        let mut first_store = SqliteStore::open(&path).unwrap();
        first_store
            .create_change(&change_id(), &context("create-1", 1))
            .unwrap();
        let mut competing_store = SqliteStore::open(&path).unwrap();
        let scope = lease_scope();
        let lease_id = LeaseId::new("lease-1").unwrap();
        let holder = coordination_subject("agent-1");
        let acquired = first_store
            .acquire_lease(
                &lease_id,
                &scope,
                &holder,
                CoordinationVersion::EMPTY,
                UnixMillis::new(20).unwrap(),
                &context("acquire-1", 10),
            )
            .unwrap();
        let stale = competing_store
            .acquire_lease(
                &LeaseId::new("lease-stale").unwrap(),
                &scope,
                &coordination_subject("agent-2"),
                CoordinationVersion::EMPTY,
                UnixMillis::new(25).unwrap(),
                &context("acquire-stale", 11),
            )
            .unwrap_err();
        assert!(matches!(stale, StoreError::StaleCoordinationVersion { .. }));
        let held = competing_store
            .acquire_lease(
                &LeaseId::new("lease-held").unwrap(),
                &scope,
                &coordination_subject("agent-2"),
                acquired.version(),
                UnixMillis::new(25).unwrap(),
                &context("acquire-held", 11),
            )
            .unwrap_err();
        assert!(matches!(held, StoreError::LeaseHeld { .. }));

        let renewed = first_store
            .renew_lease(
                &lease_id,
                acquired.version(),
                UnixMillis::new(30).unwrap(),
                &context("renew-1", 15),
            )
            .unwrap();
        let replay = first_store
            .renew_lease(
                &lease_id,
                acquired.version(),
                UnixMillis::new(30).unwrap(),
                &context("renew-1", 99),
            )
            .unwrap();
        assert_eq!(renewed, replay);
        let acquire_replay = first_store
            .acquire_lease(
                &lease_id,
                &scope,
                &holder,
                CoordinationVersion::EMPTY,
                UnixMillis::new(20).unwrap(),
                &context("acquire-1", 99),
            )
            .unwrap();
        assert_eq!(acquire_replay.version(), CoordinationVersion::INITIAL);
        assert_eq!(acquire_replay.expires_at(), UnixMillis::new(20).unwrap());
        let conflicting_replay = first_store
            .renew_lease(
                &lease_id,
                acquired.version(),
                UnixMillis::new(31).unwrap(),
                &context("renew-1", 99),
            )
            .unwrap_err();
        assert!(matches!(
            conflicting_replay,
            StoreError::OperationIdConflict(_)
        ));
        let released = first_store
            .release_lease(
                &lease_id,
                renewed.version(),
                &context("release-lease-1", 25),
            )
            .unwrap();
        assert_eq!(released.released_at(), Some(UnixMillis::new(25).unwrap()));
        assert_eq!(first_store.current_lease(&scope).unwrap(), None);
        assert_eq!(first_store.lease_events(&change_id()).unwrap().len(), 3);
    }

    #[test]
    fn expired_lease_is_reclaimed_after_process_restart() {
        let (_directory, path) = database();
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .create_change(&change_id(), &context("create-1", 1))
            .unwrap();
        drop(store);

        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "tests::process_lease_crash_helper"])
            .env(LEASE_DATABASE_ENV, &path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(91));

        let mut reopened = SqliteStore::open(&path).unwrap();
        let crashed = reopened.current_lease(&lease_scope()).unwrap().unwrap();
        assert_eq!(crashed.id().as_str(), "lease-crashed");
        let current = reopened
            .acquire_lease(
                &LeaseId::new("lease-2").unwrap(),
                &lease_scope(),
                &coordination_subject("agent-2"),
                CoordinationVersion::INITIAL,
                UnixMillis::new(40).unwrap(),
                &context("reclaim-2", 20),
            )
            .unwrap();
        assert_eq!(
            current.predecessor().map(LeaseId::as_str),
            Some("lease-crashed")
        );
        let events = reopened.lease_events(&change_id()).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].event_kind, "lease.reclaimed");
        assert_eq!(events[1].expected_version.value(), 1);
        assert_eq!(events[1].resulting_version.value(), 2);
    }

    #[test]
    fn active_lease_rejects_a_competing_process() {
        let (_directory, path) = database();
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .create_change(&change_id(), &context("create-1", 1))
            .unwrap();
        store
            .acquire_lease(
                &LeaseId::new("lease-1").unwrap(),
                &lease_scope(),
                &coordination_subject("agent-1"),
                CoordinationVersion::EMPTY,
                UnixMillis::new(20).unwrap(),
                &context("acquire-1", 10),
            )
            .unwrap();

        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "tests::process_active_lease_contention_helper",
            ])
            .env(LEASE_DATABASE_ENV, &path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child process failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(store.lease_events(&change_id()).unwrap().len(), 1);
    }

    #[test]
    fn coordination_rows_reject_duplicate_authority_and_history_mutation() {
        let (_directory, path) = database();
        let mut store = SqliteStore::open(path).unwrap();
        store
            .create_change(&change_id(), &context("create-1", 1))
            .unwrap();
        store
            .create_assignment(
                &assignment("assignment-1", "agent-1", 2),
                &context("assign-1", 2),
            )
            .unwrap();
        let duplicate = store
            .create_assignment(
                &assignment("assignment-2", "agent-1", 3),
                &context("assign-2", 3),
            )
            .unwrap_err();
        assert!(matches!(duplicate, StoreError::Database(_)));
        store
            .acquire_lease(
                &LeaseId::new("lease-1").unwrap(),
                &lease_scope(),
                &coordination_subject("agent-1"),
                CoordinationVersion::EMPTY,
                UnixMillis::new(20).unwrap(),
                &context("acquire-1", 10),
            )
            .unwrap();

        let assignment_event_update = store
            .connection
            .execute("UPDATE assignment_events SET expected_version = 9", [])
            .unwrap_err();
        let lease_event_delete = store
            .connection
            .execute("DELETE FROM lease_events", [])
            .unwrap_err();
        let lease_update = store
            .connection
            .execute("UPDATE leases SET holder_id = 'other'", [])
            .unwrap_err();
        let operation_delete = store
            .connection
            .execute("DELETE FROM operation_records", [])
            .unwrap_err();
        let scope_jump = store
            .connection
            .execute(
                "UPDATE lease_scopes SET version = version + 2,
                    current_expires_at_unix_ms = current_expires_at_unix_ms + 1",
                [],
            )
            .unwrap_err();

        assert!(assignment_event_update.to_string().contains("append-only"));
        assert!(lease_event_delete.to_string().contains("append-only"));
        assert!(lease_update.to_string().contains("immutable"));
        assert!(operation_delete.to_string().contains("cannot be deleted"));
        assert!(
            scope_jump
                .to_string()
                .contains("valid versioned transition")
        );
        assert_eq!(store.assignment_events(&change_id()).unwrap().len(), 1);
        assert_eq!(store.lease_events(&change_id()).unwrap().len(), 1);
    }

    #[test]
    fn coordination_reads_reject_projection_event_drift() {
        let (_directory, path) = database();
        let mut store = SqliteStore::open(path).unwrap();
        store
            .create_change(&change_id(), &context("create-1", 1))
            .unwrap();
        store
            .create_assignment(
                &assignment("assignment-1", "agent-1", 2),
                &context("assign-1", 2),
            )
            .unwrap();
        store
            .acquire_lease(
                &LeaseId::new("lease-1").unwrap(),
                &lease_scope(),
                &coordination_subject("agent-1"),
                CoordinationVersion::EMPTY,
                UnixMillis::new(20).unwrap(),
                &context("acquire-1", 10),
            )
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO assignments (
                    assignment_id, change_id, subject_kind, subject_id, role,
                    assigned_at_unix_ms, assigned_by, version
                 ) VALUES (
                    'assignment-without-event', 'change-1', 'agent', 'agent-2',
                    'reviewer', 12, 'operator-1', 1
                 )",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.assignments(&change_id()),
            Err(StoreError::InvalidStoredData(_))
        ));
        store
            .connection
            .execute_batch("SAVEPOINT expiry_drift")
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE lease_scopes SET version = version + 1,
                    current_expires_at_unix_ms = current_expires_at_unix_ms + 1",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.current_lease(&lease_scope()),
            Err(StoreError::InvalidStoredData(_))
        ));
        store
            .connection
            .execute_batch("ROLLBACK TO expiry_drift; RELEASE expiry_drift")
            .unwrap();

        store
            .connection
            .execute_batch("SAVEPOINT release_drift")
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE lease_scopes SET version = version + 1,
                    current_lease_id = NULL, current_expires_at_unix_ms = NULL",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.current_lease(&lease_scope()),
            Err(StoreError::InvalidStoredData(_))
        ));
        store
            .connection
            .execute_batch("ROLLBACK TO release_drift; RELEASE release_drift")
            .unwrap();

        store
            .connection
            .execute(
                "INSERT INTO lease_scopes (
                    change_id, operation_key, version, current_lease_id,
                    current_expires_at_unix_ms
                 ) VALUES ('change-1', 'fabricated-empty', 1, NULL, NULL)",
                [],
            )
            .unwrap();
        let fabricated_scope = LeaseScope::new(
            change_id(),
            LeaseOperation::new("fabricated-empty").unwrap(),
        );
        assert!(matches!(
            store.current_lease(&fabricated_scope),
            Err(StoreError::InvalidStoredData(_))
        ));
    }

    #[test]
    #[ignore = "helper simulates abrupt exit after durable lease acquisition"]
    fn process_lease_crash_helper() {
        let Ok(path) = std::env::var(LEASE_DATABASE_ENV) else {
            return;
        };
        let mut store = SqliteStore::open(path).unwrap();
        store
            .acquire_lease(
                &LeaseId::new("lease-crashed").unwrap(),
                &lease_scope(),
                &coordination_subject("agent-1"),
                CoordinationVersion::EMPTY,
                UnixMillis::new(20).unwrap(),
                &context("acquire-crashed", 10),
            )
            .unwrap();
        std::process::exit(91);
    }

    #[test]
    #[ignore = "helper invoked by active_lease_rejects_a_competing_process"]
    fn process_active_lease_contention_helper() {
        let Ok(path) = std::env::var(LEASE_DATABASE_ENV) else {
            return;
        };
        let mut store = SqliteStore::open(path).unwrap();
        let error = store
            .acquire_lease(
                &LeaseId::new("lease-competitor").unwrap(),
                &lease_scope(),
                &coordination_subject("agent-2"),
                CoordinationVersion::INITIAL,
                UnixMillis::new(30).unwrap(),
                &context("acquire-competitor", 11),
            )
            .unwrap_err();
        assert!(matches!(error, StoreError::LeaseHeld { .. }));
    }

    #[test]
    fn separate_process_advancement_makes_parent_writer_stale() {
        let (directory, path) = database();
        let artifact_path = directory.path().join("artifacts");
        let artifacts = ArtifactStore::open(&artifact_path).unwrap();
        let id = change_id();
        let mut store = SqliteStore::open(&path).unwrap();
        store.create_change(&id, &context("create-1", 1)).unwrap();
        store
            .append_revision(
                &artifacts,
                &id,
                None,
                &revision(&artifacts, "revision-1", 2),
                &context("append-1", 2),
            )
            .unwrap();
        let stale_head = RevisionId::new("revision-1").unwrap();

        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "tests::process_append_helper"])
            .env(DATABASE_ENV, &path)
            .env(ARTIFACT_ENV, &artifact_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child process failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let error = store
            .append_revision(
                &artifacts,
                &id,
                Some(&stale_head),
                &revision(&artifacts, "revision-parent-stale", 4),
                &context("append-parent-stale", 4),
            )
            .unwrap_err();
        assert!(matches!(error, StoreError::StaleHead { .. }));
        assert_eq!(
            store
                .load_change(&artifacts, &id)
                .unwrap()
                .head()
                .map(RevisionId::as_str),
            Some("revision-child")
        );
        assert_eq!(store.audit_events(&id).unwrap().len(), 3);
    }

    fn seed_started_integration(
        store: &mut SqliteStore,
        artifacts: &ArtifactStore,
    ) -> (IntegrationAttempt, IntegrationAttempt, TargetRef) {
        seed_named_change(store, artifacts, "integrated", "integrated-r1", 1);
        let candidate = store
            .create_candidate(
                artifacts,
                CandidateId::new("integration-candidate").unwrap(),
                BaseState::new(RepositoryId::new("repository-1").unwrap(), "target-base").unwrap(),
                &CandidateSelection::Changes(vec![ChangeId::new("integrated").unwrap()]),
                &context("integration-candidate", 3),
            )
            .unwrap();
        let target_ref = TargetRef::new("refs/heads/main").unwrap();
        let attempt = IntegrationAttempt::plan(
            IntegrationId::new("integration-1").unwrap(),
            IntegrationIntent::new(
                IntegrationBinding::new(
                    candidate.id().clone(),
                    candidate.content_digest().as_str(),
                    candidate.inputs().to_vec(),
                )
                .unwrap(),
                IntegrationTarget::new(
                    candidate.target_base().repository_id().clone(),
                    target_ref.clone(),
                    TargetRevision::new(candidate.target_base().object_id()).unwrap(),
                ),
                IntegrationMethod::new(
                    ProviderId::new("native-git").unwrap(),
                    IntegrationStrategy::new("merge").unwrap(),
                    EffectOperationId::new("effect-integration-1").unwrap(),
                ),
            ),
            IntegrationGate::new(
                GatePolicyEvidence::new("policy:allowed").unwrap(),
                IntegrationCapabilityEvidence::new("native-git:merge").unwrap(),
                Vec::new(),
                Vec::new(),
                TargetObservation::new(
                    target_ref.clone(),
                    TargetRevision::new("target-base").unwrap(),
                    IntegrationEvidence::new("target:planned").unwrap(),
                ),
            ),
            UnixMillis::new(4).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
        .unwrap();
        store
            .create_integration_attempt(artifacts, &attempt, &context("integration-plan", 4))
            .unwrap();
        let holder = Subject::new(SubjectKind::Agent, SubjectId::new("agent-1").unwrap());
        let lease = ExecutionLease::new(
            ExecutionLeaseId::new("execution-lease-1").unwrap(),
            holder,
            UnixMillis::new(5).unwrap(),
            UnixMillis::new(7).unwrap(),
        )
        .unwrap();
        let running = store
            .start_integration(
                artifacts,
                attempt.id(),
                attempt.version(),
                lease,
                &TargetObservation::new(
                    target_ref.clone(),
                    TargetRevision::new("target-base").unwrap(),
                    IntegrationEvidence::new("target:start-cas").unwrap(),
                ),
                &context("integration-start", 5),
            )
            .unwrap();
        (attempt, running, target_ref)
    }

    fn assert_competing_integration_is_held(
        store: &mut SqliteStore,
        artifacts: &ArtifactStore,
        attempt: &IntegrationAttempt,
    ) -> IntegrationAttempt {
        let competing = IntegrationAttempt::plan(
            IntegrationId::new("integration-2").unwrap(),
            IntegrationIntent::new(
                attempt.intent().binding().clone(),
                attempt.intent().target().clone(),
                IntegrationMethod::new(
                    ProviderId::new("native-git").unwrap(),
                    IntegrationStrategy::new("merge").unwrap(),
                    EffectOperationId::new("effect-integration-2").unwrap(),
                ),
            ),
            attempt.gate().clone(),
            UnixMillis::new(6).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
        .unwrap();
        store
            .create_integration_attempt(artifacts, &competing, &context("integration-plan-2", 6))
            .unwrap();
        let competing_lease = ExecutionLease::new(
            ExecutionLeaseId::new("execution-lease-2").unwrap(),
            Subject::new(SubjectKind::Agent, SubjectId::new("agent-2").unwrap()),
            UnixMillis::new(6).unwrap(),
            UnixMillis::new(8).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            store.start_integration(
                artifacts,
                competing.id(),
                competing.version(),
                competing_lease,
                &TargetObservation::new(
                    TargetRef::new("refs/heads/main").unwrap(),
                    TargetRevision::new("target-base").unwrap(),
                    IntegrationEvidence::new("target:competing-cas").unwrap(),
                ),
                &context("integration-start-2", 6),
            ),
            Err(StoreError::IntegrationTargetHeld)
        ));
        competing
    }

    fn reconcile_started_integration(
        store: &mut SqliteStore,
        artifacts: &ArtifactStore,
        attempt: &IntegrationAttempt,
        running: &IntegrationAttempt,
        target_ref: TargetRef,
    ) -> (IntegrationAttempt, weft_domain::IntegrationReceipt) {
        let (reconciling, _) = store
            .enter_integration_reconciliation(
                artifacts,
                attempt.id(),
                &ReconciliationStart {
                    expected_version: running.version(),
                    reconciliation_id: ReconciliationId::new("reconciliation-1").unwrap(),
                    authority: None,
                    observation: TargetObservation::new(
                        target_ref.clone(),
                        TargetRevision::new("provider-unknown").unwrap(),
                        IntegrationEvidence::new("provider:timeout").unwrap(),
                    ),
                },
                &context("integration-uncertain", 7),
            )
            .unwrap();
        assert_reconciling_attempt_holds_target(store, artifacts, attempt);
        let (verified, _) = store
            .reconcile_integration(
                artifacts,
                attempt.id(),
                &ReconciliationRecord {
                    expected_version: reconciling.version(),
                    reconciliation_id: ReconciliationId::new("reconciliation-2").unwrap(),
                    outcome: ReconciliationOutcome::ResultVerified,
                    observation: TargetObservation::new(
                        target_ref.clone(),
                        TargetRevision::new("target-result").unwrap(),
                        IntegrationEvidence::new("provider:result-verified").unwrap(),
                    ),
                },
                &context("integration-reconciled", 8),
            )
            .unwrap();
        let verification = SuccessVerification {
            expected_version: verified.version(),
            receipt_id: IntegrationReceiptId::new("receipt-1").unwrap(),
            authority: None,
            observation: TargetObservation::new(
                target_ref,
                TargetRevision::new("target-result").unwrap(),
                IntegrationEvidence::new("provider:result-verified").unwrap(),
            ),
        };
        let (succeeded, receipt) = store
            .succeed_integration(
                artifacts,
                attempt.id(),
                &verification,
                &context("integration-success", 9),
            )
            .unwrap();
        assert_eq!(succeeded.state(), IntegrationState::Succeeded);
        assert_eq!(
            receipt.effect_operation_id().as_str(),
            "effect-integration-1"
        );
        let replay = store
            .succeed_integration(
                artifacts,
                attempt.id(),
                &verification,
                &context("integration-success", 9),
            )
            .unwrap();
        assert_eq!(replay, (succeeded.clone(), receipt.clone()));
        (succeeded, receipt)
    }

    fn assert_reconciling_attempt_holds_target(
        store: &mut SqliteStore,
        artifacts: &ArtifactStore,
        original: &IntegrationAttempt,
    ) {
        let competing = IntegrationAttempt::plan(
            IntegrationId::new("integration-reconciling-competitor").unwrap(),
            IntegrationIntent::new(
                original.intent().binding().clone(),
                original.intent().target().clone(),
                IntegrationMethod::new(
                    ProviderId::new("native-git").unwrap(),
                    IntegrationStrategy::new("merge").unwrap(),
                    EffectOperationId::new("effect-reconciling-competitor").unwrap(),
                ),
            ),
            original.gate().clone(),
            UnixMillis::new(7).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
        .unwrap();
        store
            .create_integration_attempt(
                artifacts,
                &competing,
                &context("integration-plan-reconciling-competitor", 7),
            )
            .unwrap();
        let lease = ExecutionLease::new(
            ExecutionLeaseId::new("lease-reconciling-competitor").unwrap(),
            Subject::new(
                SubjectKind::Agent,
                SubjectId::new("agent-reconciling-competitor").unwrap(),
            ),
            UnixMillis::new(7).unwrap(),
            UnixMillis::new(9).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            store.start_integration(
                artifacts,
                competing.id(),
                competing.version(),
                lease,
                &TargetObservation::new(
                    TargetRef::new("refs/heads/main").unwrap(),
                    TargetRevision::new("target-base").unwrap(),
                    IntegrationEvidence::new("target:reconciliation-competitor").unwrap(),
                ),
                &context("integration-start-reconciling-competitor", 7),
            ),
            Err(StoreError::IntegrationTargetHeld)
        ));
    }

    fn assert_historical_integration_replays(
        store: &mut SqliteStore,
        artifacts: &ArtifactStore,
        original: &IntegrationAttempt,
    ) {
        store
            .create_integration_attempt(artifacts, original, &context("integration-plan", 4))
            .unwrap();
        let started = store
            .start_integration(
                artifacts,
                original.id(),
                original.version(),
                ExecutionLease::new(
                    ExecutionLeaseId::new("execution-lease-1").unwrap(),
                    Subject::new(SubjectKind::Agent, SubjectId::new("agent-1").unwrap()),
                    UnixMillis::new(5).unwrap(),
                    UnixMillis::new(7).unwrap(),
                )
                .unwrap(),
                &TargetObservation::new(
                    TargetRef::new("refs/heads/main").unwrap(),
                    TargetRevision::new("target-base").unwrap(),
                    IntegrationEvidence::new("target:start-cas").unwrap(),
                ),
                &context("integration-start", 5),
            )
            .unwrap();
        assert_eq!(started.state(), IntegrationState::Running);
        let (first_reconciliation, observation) = store
            .enter_integration_reconciliation(
                artifacts,
                original.id(),
                &ReconciliationStart {
                    expected_version: started.version(),
                    reconciliation_id: ReconciliationId::new("reconciliation-1").unwrap(),
                    authority: None,
                    observation: TargetObservation::new(
                        TargetRef::new("refs/heads/main").unwrap(),
                        TargetRevision::new("provider-unknown").unwrap(),
                        IntegrationEvidence::new("provider:timeout").unwrap(),
                    ),
                },
                &context("integration-uncertain", 7),
            )
            .unwrap();
        assert_eq!(first_reconciliation.state(), IntegrationState::Reconciling);
        assert_eq!(observation.id().as_str(), "reconciliation-1");
    }

    #[test]
    fn integration_reconciles_expired_authority_and_verifies_immutable_receipt() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        let (attempt, running, target_ref) = seed_started_integration(&mut store, &artifacts);
        let _competing = assert_competing_integration_is_held(&mut store, &artifacts, &attempt);
        let (succeeded, receipt) =
            reconcile_started_integration(&mut store, &artifacts, &attempt, &running, target_ref);
        assert_historical_integration_replays(&mut store, &artifacts, &attempt);
        assert_eq!(succeeded.state(), IntegrationState::Succeeded);
        assert_eq!(
            receipt.effect_operation_id().as_str(),
            "effect-integration-1"
        );

        append_named_revision(
            &mut store,
            &artifacts,
            "integrated",
            "integrated-r1",
            "integrated-r2",
            10,
        );
        assert_eq!(
            store.integration_attempt(&artifacts, attempt.id()).unwrap(),
            succeeded
        );
        store
            .connection
            .execute_batch(
                "DROP TRIGGER integration_receipts_immutable;
                 UPDATE integration_receipts SET verification_evidence = 'drifted';",
            )
            .unwrap();
        assert!(matches!(
            store.integration_attempt(&artifacts, attempt.id()),
            Err(StoreError::InvariantViolation(
                "integration receipt row drift"
            ))
        ));
    }

    #[test]
    fn integration_conflict_resolution_is_separate_exact_and_validated() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        let (attempt, running, _) = seed_started_integration(&mut store, &artifacts);
        let report = ConflictReport {
            expected_version: running.version(),
            conflict_id: IntegrationConflictId::new("conflict-1").unwrap(),
            authority: Some((
                ExecutionLeaseId::new("execution-lease-1").unwrap(),
                Subject::new(SubjectKind::Agent, SubjectId::new("agent-1").unwrap()),
            )),
            provider_state: IntegrationEvidence::new("provider:unmerged-path").unwrap(),
        };
        let (_, conflict) = store
            .conflict_integration(
                &artifacts,
                attempt.id(),
                &report,
                &context("integration-conflict", 6),
            )
            .unwrap();
        assert_eq!(
            store
                .integration_conflict(&artifacts, conflict.id())
                .unwrap(),
            conflict
        );
        let target = store
            .candidate_target(&artifacts, attempt.intent().binding().candidate_id())
            .unwrap();
        let validation = ValidationResult::new(
            ValidationResultId::new("resolution-validation").unwrap(),
            target.clone(),
            ValidationObservation::new(
                ValidationType::new("resolution-check").unwrap(),
                ValidationEnvironment::new("linux").unwrap(),
                ValidationOutcome::Passed,
                ValidationExecutionId::new("resolution-run-1").unwrap(),
                ValidationScope::ExactTarget,
            ),
            ActorId::new("operator-1").unwrap(),
            UnixMillis::new(7).unwrap(),
        );
        store
            .create_validation_result(
                &artifacts,
                &validation,
                &context("resolution-validation", 7),
            )
            .unwrap();
        let resolution = ConflictResolution::new(
            ConflictResolutionId::new("resolution-1").unwrap(),
            &conflict,
            target,
            vec![validation.id().clone()],
            IntegrationEvidence::new("provider:resolved-candidate").unwrap(),
            UnixMillis::new(8).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
        .unwrap();
        store
            .create_conflict_resolution(&artifacts, &resolution, &context("resolution-create", 8))
            .unwrap();
        store
            .create_conflict_resolution(&artifacts, &resolution, &context("resolution-create", 8))
            .unwrap();
        assert_eq!(
            store
                .conflict_resolution(&artifacts, resolution.id())
                .unwrap(),
            resolution
        );
    }

    #[test]
    fn integration_start_revalidates_candidate_after_planning() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        let (attempt, running, _) = seed_started_integration(&mut store, &artifacts);
        let competing = assert_competing_integration_is_held(&mut store, &artifacts, &attempt);
        store
            .conflict_integration(
                &artifacts,
                attempt.id(),
                &ConflictReport {
                    expected_version: running.version(),
                    conflict_id: IntegrationConflictId::new("stale-plan-conflict").unwrap(),
                    authority: Some((
                        ExecutionLeaseId::new("execution-lease-1").unwrap(),
                        Subject::new(SubjectKind::Agent, SubjectId::new("agent-1").unwrap()),
                    )),
                    provider_state: IntegrationEvidence::new("provider:conflict").unwrap(),
                },
                &context("stale-plan-conflict", 6),
            )
            .unwrap();
        append_named_revision(
            &mut store,
            &artifacts,
            "integrated",
            "integrated-r1",
            "integrated-r2",
            7,
        );
        let lease = ExecutionLease::new(
            ExecutionLeaseId::new("stale-plan-lease").unwrap(),
            Subject::new(
                SubjectKind::Agent,
                SubjectId::new("stale-plan-agent").unwrap(),
            ),
            UnixMillis::new(8).unwrap(),
            UnixMillis::new(10).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            store.start_integration(
                &artifacts,
                competing.id(),
                competing.version(),
                lease,
                &TargetObservation::new(
                    TargetRef::new("refs/heads/main").unwrap(),
                    TargetRevision::new("target-base").unwrap(),
                    IntegrationEvidence::new("target:stale-plan").unwrap(),
                ),
                &context("stale-plan-start", 8),
            ),
            Err(StoreError::IntegrationGateRejected("candidate is stale"))
        ));
    }

    fn start_replacement_after_divergence(
        store: &mut SqliteStore,
        artifacts: &ArtifactStore,
        target_ref: TargetRef,
    ) -> IntegrationAttempt {
        let candidate = store
            .create_candidate(
                artifacts,
                CandidateId::new("replacement-candidate").unwrap(),
                BaseState::new(
                    RepositoryId::new("repository-1").unwrap(),
                    "external-target",
                )
                .unwrap(),
                &CandidateSelection::Changes(vec![ChangeId::new("integrated").unwrap()]),
                &context("replacement-candidate", 10),
            )
            .unwrap();
        let replacement = IntegrationAttempt::plan(
            IntegrationId::new("replacement-integration").unwrap(),
            IntegrationIntent::new(
                IntegrationBinding::new(
                    candidate.id().clone(),
                    candidate.content_digest().as_str(),
                    candidate.inputs().to_vec(),
                )
                .unwrap(),
                IntegrationTarget::new(
                    candidate.target_base().repository_id().clone(),
                    target_ref.clone(),
                    TargetRevision::new("external-target").unwrap(),
                ),
                IntegrationMethod::new(
                    ProviderId::new("native-git").unwrap(),
                    IntegrationStrategy::new("merge").unwrap(),
                    EffectOperationId::new("replacement-effect").unwrap(),
                ),
            ),
            IntegrationGate::new(
                GatePolicyEvidence::new("policy:allowed").unwrap(),
                IntegrationCapabilityEvidence::new("native-git:merge").unwrap(),
                Vec::new(),
                Vec::new(),
                TargetObservation::new(
                    target_ref,
                    TargetRevision::new("external-target").unwrap(),
                    IntegrationEvidence::new("provider:replacement-plan").unwrap(),
                ),
            ),
            UnixMillis::new(11).unwrap(),
            ActorId::new("operator-1").unwrap(),
        )
        .unwrap();
        store
            .create_integration_attempt(artifacts, &replacement, &context("replacement-plan", 11))
            .unwrap();
        store
            .start_integration(
                artifacts,
                replacement.id(),
                replacement.version(),
                ExecutionLease::new(
                    ExecutionLeaseId::new("replacement-lease").unwrap(),
                    Subject::new(
                        SubjectKind::Agent,
                        SubjectId::new("replacement-agent").unwrap(),
                    ),
                    UnixMillis::new(12).unwrap(),
                    UnixMillis::new(14).unwrap(),
                )
                .unwrap(),
                replacement.gate().target_observation(),
                &context("replacement-start", 12),
            )
            .unwrap()
    }

    #[test]
    fn diverged_integration_is_explicitly_superseded_before_replanning() {
        let (directory, path) = database();
        let artifacts = ArtifactStore::open(directory.path().join("artifacts")).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        let (attempt, running, target_ref) = seed_started_integration(&mut store, &artifacts);
        let (reconciling, _) = store
            .enter_integration_reconciliation(
                &artifacts,
                attempt.id(),
                &ReconciliationStart {
                    expected_version: running.version(),
                    reconciliation_id: ReconciliationId::new("diverged-enter").unwrap(),
                    authority: None,
                    observation: TargetObservation::new(
                        target_ref.clone(),
                        TargetRevision::new("unknown").unwrap(),
                        IntegrationEvidence::new("provider:unknown").unwrap(),
                    ),
                },
                &context("diverged-enter", 7),
            )
            .unwrap();
        let (diverged, _) = store
            .reconcile_integration(
                &artifacts,
                attempt.id(),
                &ReconciliationRecord {
                    expected_version: reconciling.version(),
                    reconciliation_id: ReconciliationId::new("diverged-observed").unwrap(),
                    outcome: ReconciliationOutcome::Diverged,
                    observation: TargetObservation::new(
                        target_ref.clone(),
                        TargetRevision::new("external-target").unwrap(),
                        IntegrationEvidence::new("provider:external-target").unwrap(),
                    ),
                },
                &context("diverged-observed", 8),
            )
            .unwrap();
        let superseded = store
            .supersede_diverged_integration(
                &artifacts,
                attempt.id(),
                diverged.version(),
                &context("diverged-superseded", 9),
            )
            .unwrap();
        assert_eq!(superseded.state(), IntegrationState::Superseded);
        let running_replacement =
            start_replacement_after_divergence(&mut store, &artifacts, target_ref);
        assert_eq!(running_replacement.state(), IntegrationState::Running);
        store
            .connection
            .execute_batch(
                "DROP TRIGGER integration_conflicts_match_operation;
                 DROP TRIGGER integration_conflicts_no_delete;
                 INSERT INTO operation_records VALUES ('orphan-conflict-op', 'integration.conflicted', 'operator-1', 13);
                 INSERT INTO integration_conflicts VALUES (
                    'orphan-conflict', 'integration-1', 'integration-candidate',
                    (SELECT candidate_digest FROM integration_attempts WHERE integration_id = 'integration-1'),
                    'native-git', 'orphan', 13, 'operator-1', 'orphan-conflict-op'
                 );",
            )
            .unwrap();
        assert!(matches!(
            store.integration_attempt(&artifacts, attempt.id()),
            Err(StoreError::InvariantViolation(
                "integration terminal evidence cardinality drift"
            ))
        ));
        store
            .connection
            .execute_batch(
                "DELETE FROM integration_conflicts WHERE conflict_id = 'orphan-conflict';
                 DROP TRIGGER integration_receipts_match_operation;
                 INSERT INTO operation_records VALUES ('orphan-receipt-op', 'integration.succeeded', 'operator-1', 14);
                 INSERT INTO integration_receipts VALUES (
                    'orphan-receipt', 'integration-1', 'integration-candidate',
                    (SELECT candidate_digest FROM integration_attempts WHERE integration_id = 'integration-1'),
                    'repository-1', 'refs/heads/main', 'target-base', 'orphan-result',
                    'native-git', 'effect-integration-1', 'orphan', 14, 'operator-1',
                    'orphan-receipt-op'
                 );",
            )
            .unwrap();
        assert!(matches!(
            store.integration_attempt(&artifacts, attempt.id()),
            Err(StoreError::InvariantViolation(
                "integration terminal evidence cardinality drift"
            ))
        ));
    }

    #[test]
    #[ignore = "helper invoked by separate_process_advancement_makes_parent_writer_stale"]
    fn process_append_helper() {
        let Ok(path) = std::env::var(DATABASE_ENV) else {
            return;
        };
        let artifact_path = std::env::var(ARTIFACT_ENV).unwrap();
        let artifacts = ArtifactStore::open(artifact_path).unwrap();
        let mut store = SqliteStore::open(path).unwrap();
        let expected = RevisionId::new("revision-1").unwrap();
        store
            .append_revision(
                &artifacts,
                &change_id(),
                Some(&expected),
                &revision(&artifacts, "revision-child", 3),
                &context("append-child", 3),
            )
            .unwrap();
    }
}
