CREATE TABLE integration_attempts (
    integration_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(integration_id)) > 0),
    candidate_id TEXT NOT NULL,
    candidate_digest TEXT NOT NULL CHECK (length(candidate_digest) = 71 AND substr(candidate_digest, 1, 7) = 'sha256:'),
    input_count INTEGER NOT NULL CHECK (input_count > 0),
    repository_id TEXT NOT NULL CHECK (length(trim(repository_id)) > 0),
    target_ref TEXT NOT NULL CHECK (length(trim(target_ref)) > 0),
    expected_target_revision TEXT NOT NULL CHECK (length(trim(expected_target_revision)) > 0),
    provider_id TEXT NOT NULL CHECK (length(trim(provider_id)) > 0),
    strategy TEXT NOT NULL CHECK (length(trim(strategy)) > 0),
    effect_operation_id TEXT NOT NULL UNIQUE CHECK (length(trim(effect_operation_id)) > 0),
    policy_evidence TEXT NOT NULL CHECK (length(trim(policy_evidence)) > 0),
    capability_evidence TEXT NOT NULL CHECK (length(trim(capability_evidence)) > 0),
    review_ref_count INTEGER NOT NULL CHECK (review_ref_count >= 0),
    validation_ref_count INTEGER NOT NULL CHECK (validation_ref_count >= 0),
    planned_observed_revision TEXT NOT NULL CHECK (length(trim(planned_observed_revision)) > 0),
    planned_observation_evidence TEXT NOT NULL CHECK (length(trim(planned_observation_evidence)) > 0),
    created_at_unix_ms INTEGER NOT NULL CHECK (created_at_unix_ms >= 0),
    created_by TEXT NOT NULL CHECK (length(trim(created_by)) > 0),
    operation_id TEXT NOT NULL UNIQUE,
    FOREIGN KEY (candidate_id) REFERENCES composition_candidates (candidate_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TABLE integration_attempt_inputs (
    integration_id TEXT NOT NULL,
    input_position INTEGER NOT NULL CHECK (input_position >= 0),
    change_id TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    PRIMARY KEY (integration_id, input_position),
    UNIQUE (integration_id, change_id),
    FOREIGN KEY (integration_id) REFERENCES integration_attempts (integration_id),
    FOREIGN KEY (revision_id, change_id) REFERENCES change_revisions (revision_id, change_id)
) STRICT;

CREATE TABLE integration_attempt_review_refs (
    integration_id TEXT NOT NULL,
    ref_position INTEGER NOT NULL CHECK (ref_position >= 0),
    review_submission_id TEXT NOT NULL,
    PRIMARY KEY (integration_id, ref_position),
    UNIQUE (integration_id, review_submission_id),
    FOREIGN KEY (integration_id) REFERENCES integration_attempts (integration_id),
    FOREIGN KEY (review_submission_id) REFERENCES review_submissions (review_submission_id)
) STRICT;

CREATE TABLE integration_attempt_validation_refs (
    integration_id TEXT NOT NULL,
    ref_position INTEGER NOT NULL CHECK (ref_position >= 0),
    validation_result_id TEXT NOT NULL,
    PRIMARY KEY (integration_id, ref_position),
    UNIQUE (integration_id, validation_result_id),
    FOREIGN KEY (integration_id) REFERENCES integration_attempts (integration_id),
    FOREIGN KEY (validation_result_id) REFERENCES validation_results (validation_result_id)
) STRICT;

CREATE TABLE integration_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    integration_id TEXT NOT NULL,
    event_kind TEXT NOT NULL CHECK (event_kind IN ('integration.planned', 'integration.started', 'integration.lease_renewed', 'integration.reconciliation_entered', 'integration.reconciled', 'integration.conflicted', 'integration.failed', 'integration.succeeded', 'integration.aborted', 'integration.superseded')),
    expected_version INTEGER NOT NULL CHECK (expected_version >= 0),
    resulting_version INTEGER NOT NULL CHECK (resulting_version > expected_version),
    resulting_state TEXT NOT NULL CHECK (resulting_state IN ('planned', 'running', 'reconciling', 'conflicted', 'failed', 'succeeded', 'aborted', 'superseded')),
    observed_revision TEXT,
    observation_evidence TEXT,
    lease_id TEXT,
    lease_holder_kind TEXT CHECK (lease_holder_kind IS NULL OR lease_holder_kind IN ('agent', 'human', 'service')),
    lease_holder_id TEXT,
    lease_acquired_at_unix_ms INTEGER,
    lease_expires_at_unix_ms INTEGER,
    lease_version INTEGER,
    reconciliation_id TEXT UNIQUE,
    reconciliation_outcome TEXT CHECK (reconciliation_outcome IS NULL OR reconciliation_outcome IN ('still_uncertain', 'no_effect_verified', 'result_verified', 'diverged')),
    conflict_id TEXT UNIQUE,
    provider_state TEXT,
    receipt_id TEXT UNIQUE,
    result_revision TEXT,
    operation_id TEXT NOT NULL UNIQUE,
    actor_id TEXT NOT NULL CHECK (length(trim(actor_id)) > 0),
    occurred_at_unix_ms INTEGER NOT NULL CHECK (occurred_at_unix_ms >= 0),
    CHECK ((lease_id IS NULL AND lease_holder_kind IS NULL AND lease_holder_id IS NULL AND lease_acquired_at_unix_ms IS NULL AND lease_expires_at_unix_ms IS NULL AND lease_version IS NULL) OR (lease_id IS NOT NULL AND lease_holder_kind IS NOT NULL AND length(trim(lease_holder_id)) > 0 AND lease_acquired_at_unix_ms IS NOT NULL AND lease_expires_at_unix_ms > lease_acquired_at_unix_ms AND lease_version > 0)),
    CHECK ((reconciliation_id IS NULL AND reconciliation_outcome IS NULL) OR (reconciliation_id IS NOT NULL AND reconciliation_outcome IS NOT NULL AND observed_revision IS NOT NULL AND observation_evidence IS NOT NULL)),
    CHECK ((conflict_id IS NULL AND provider_state IS NULL) OR (conflict_id IS NOT NULL AND provider_state IS NOT NULL)),
    CHECK ((receipt_id IS NULL AND result_revision IS NULL) OR (receipt_id IS NOT NULL AND result_revision IS NOT NULL AND observed_revision = result_revision)),
    FOREIGN KEY (integration_id) REFERENCES integration_attempts (integration_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE UNIQUE INDEX integration_event_version_once ON integration_events (integration_id, resulting_version);

CREATE TABLE integration_conflicts (
    conflict_id TEXT PRIMARY KEY NOT NULL,
    integration_id TEXT NOT NULL UNIQUE,
    candidate_id TEXT NOT NULL,
    candidate_digest TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    provider_state TEXT NOT NULL,
    created_at_unix_ms INTEGER NOT NULL,
    created_by TEXT NOT NULL,
    operation_id TEXT NOT NULL UNIQUE,
    FOREIGN KEY (integration_id) REFERENCES integration_attempts (integration_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TABLE integration_conflict_inputs (
    conflict_id TEXT NOT NULL,
    input_position INTEGER NOT NULL,
    change_id TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    PRIMARY KEY (conflict_id, input_position),
    FOREIGN KEY (conflict_id) REFERENCES integration_conflicts (conflict_id),
    FOREIGN KEY (revision_id, change_id) REFERENCES change_revisions (revision_id, change_id)
) STRICT;

CREATE TABLE integration_receipts (
    receipt_id TEXT PRIMARY KEY NOT NULL,
    integration_id TEXT NOT NULL UNIQUE,
    candidate_id TEXT NOT NULL,
    candidate_digest TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    target_ref TEXT NOT NULL,
    prior_revision TEXT NOT NULL,
    result_revision TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    effect_operation_id TEXT NOT NULL UNIQUE,
    verification_evidence TEXT NOT NULL,
    verified_at_unix_ms INTEGER NOT NULL,
    verified_by TEXT NOT NULL,
    operation_id TEXT NOT NULL UNIQUE,
    FOREIGN KEY (integration_id) REFERENCES integration_attempts (integration_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TABLE conflict_resolutions (
    resolution_id TEXT PRIMARY KEY NOT NULL,
    conflict_id TEXT NOT NULL,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('revision', 'candidate')),
    change_id TEXT,
    revision_id TEXT,
    candidate_id TEXT,
    repository_id TEXT NOT NULL,
    context_object_id TEXT NOT NULL,
    content_digest TEXT NOT NULL,
    validation_ref_count INTEGER NOT NULL CHECK (validation_ref_count > 0),
    provider_evidence TEXT NOT NULL,
    resolved_at_unix_ms INTEGER NOT NULL,
    resolved_by TEXT NOT NULL,
    operation_id TEXT NOT NULL UNIQUE,
    CHECK ((target_kind = 'revision' AND change_id IS NOT NULL AND revision_id IS NOT NULL AND candidate_id IS NULL) OR (target_kind = 'candidate' AND change_id IS NULL AND revision_id IS NULL AND candidate_id IS NOT NULL)),
    FOREIGN KEY (conflict_id) REFERENCES integration_conflicts (conflict_id),
    FOREIGN KEY (revision_id, change_id) REFERENCES change_revisions (revision_id, change_id),
    FOREIGN KEY (candidate_id) REFERENCES composition_candidates (candidate_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TABLE conflict_resolution_validation_refs (
    resolution_id TEXT NOT NULL,
    ref_position INTEGER NOT NULL,
    validation_result_id TEXT NOT NULL,
    PRIMARY KEY (resolution_id, ref_position),
    UNIQUE (resolution_id, validation_result_id),
    FOREIGN KEY (resolution_id) REFERENCES conflict_resolutions (resolution_id),
    FOREIGN KEY (validation_result_id) REFERENCES validation_results (validation_result_id)
) STRICT;

CREATE TRIGGER integration_attempts_match_operation BEFORE INSERT ON integration_attempts WHEN NOT EXISTS (SELECT 1 FROM operation_records WHERE operation_id = NEW.operation_id AND event_kind = 'integration.planned' AND actor_id = NEW.created_by AND occurred_at_unix_ms = NEW.created_at_unix_ms) BEGIN SELECT RAISE(ABORT, 'integration plan requires matching operation'); END;
CREATE TRIGGER integration_events_match_operation BEFORE INSERT ON integration_events WHEN NOT EXISTS (SELECT 1 FROM operation_records WHERE operation_id = NEW.operation_id AND event_kind = NEW.event_kind AND actor_id = NEW.actor_id AND occurred_at_unix_ms = NEW.occurred_at_unix_ms) BEGIN SELECT RAISE(ABORT, 'integration event requires matching operation'); END;
CREATE TRIGGER integration_conflicts_match_operation BEFORE INSERT ON integration_conflicts WHEN NOT EXISTS (SELECT 1 FROM operation_records AS operation JOIN integration_events AS event USING (operation_id) WHERE operation.operation_id = NEW.operation_id AND operation.event_kind = 'integration.conflicted' AND operation.actor_id = NEW.created_by AND operation.occurred_at_unix_ms = NEW.created_at_unix_ms AND event.integration_id = NEW.integration_id AND event.conflict_id = NEW.conflict_id AND event.resulting_state = 'conflicted') BEGIN SELECT RAISE(ABORT, 'integration conflict requires matching event'); END;
CREATE TRIGGER integration_receipts_match_operation BEFORE INSERT ON integration_receipts WHEN NOT EXISTS (SELECT 1 FROM operation_records AS operation JOIN integration_events AS event USING (operation_id) WHERE operation.operation_id = NEW.operation_id AND operation.event_kind = 'integration.succeeded' AND operation.actor_id = NEW.verified_by AND operation.occurred_at_unix_ms = NEW.verified_at_unix_ms AND event.integration_id = NEW.integration_id AND event.receipt_id = NEW.receipt_id AND event.resulting_state = 'succeeded' AND event.result_revision = NEW.result_revision) BEGIN SELECT RAISE(ABORT, 'integration receipt requires matching event'); END;
CREATE TRIGGER conflict_resolutions_match_operation BEFORE INSERT ON conflict_resolutions WHEN NOT EXISTS (SELECT 1 FROM operation_records WHERE operation_id = NEW.operation_id AND event_kind = 'integration.conflict_resolved' AND actor_id = NEW.resolved_by AND occurred_at_unix_ms = NEW.resolved_at_unix_ms) BEGIN SELECT RAISE(ABORT, 'conflict resolution requires matching operation'); END;

CREATE TRIGGER integration_attempts_immutable BEFORE UPDATE ON integration_attempts BEGIN SELECT RAISE(ABORT, 'integration attempts are immutable'); END;
CREATE TRIGGER integration_attempts_no_delete BEFORE DELETE ON integration_attempts BEGIN SELECT RAISE(ABORT, 'integration attempts cannot be deleted'); END;
CREATE TRIGGER integration_attempt_inputs_immutable BEFORE UPDATE ON integration_attempt_inputs BEGIN SELECT RAISE(ABORT, 'integration inputs are immutable'); END;
CREATE TRIGGER integration_attempt_inputs_no_delete BEFORE DELETE ON integration_attempt_inputs BEGIN SELECT RAISE(ABORT, 'integration inputs cannot be deleted'); END;
CREATE TRIGGER integration_attempt_review_refs_immutable BEFORE UPDATE ON integration_attempt_review_refs BEGIN SELECT RAISE(ABORT, 'integration review refs are immutable'); END;
CREATE TRIGGER integration_attempt_review_refs_no_delete BEFORE DELETE ON integration_attempt_review_refs BEGIN SELECT RAISE(ABORT, 'integration review refs cannot be deleted'); END;
CREATE TRIGGER integration_attempt_validation_refs_immutable BEFORE UPDATE ON integration_attempt_validation_refs BEGIN SELECT RAISE(ABORT, 'integration validation refs are immutable'); END;
CREATE TRIGGER integration_attempt_validation_refs_no_delete BEFORE DELETE ON integration_attempt_validation_refs BEGIN SELECT RAISE(ABORT, 'integration validation refs cannot be deleted'); END;
CREATE TRIGGER integration_events_immutable BEFORE UPDATE ON integration_events BEGIN SELECT RAISE(ABORT, 'integration events are immutable'); END;
CREATE TRIGGER integration_events_no_delete BEFORE DELETE ON integration_events BEGIN SELECT RAISE(ABORT, 'integration events cannot be deleted'); END;
CREATE TRIGGER integration_conflicts_immutable BEFORE UPDATE ON integration_conflicts BEGIN SELECT RAISE(ABORT, 'integration conflicts are immutable'); END;
CREATE TRIGGER integration_conflicts_no_delete BEFORE DELETE ON integration_conflicts BEGIN SELECT RAISE(ABORT, 'integration conflicts cannot be deleted'); END;
CREATE TRIGGER integration_conflict_inputs_immutable BEFORE UPDATE ON integration_conflict_inputs BEGIN SELECT RAISE(ABORT, 'integration conflict inputs are immutable'); END;
CREATE TRIGGER integration_conflict_inputs_no_delete BEFORE DELETE ON integration_conflict_inputs BEGIN SELECT RAISE(ABORT, 'integration conflict inputs cannot be deleted'); END;
CREATE TRIGGER integration_receipts_immutable BEFORE UPDATE ON integration_receipts BEGIN SELECT RAISE(ABORT, 'integration receipts are immutable'); END;
CREATE TRIGGER integration_receipts_no_delete BEFORE DELETE ON integration_receipts BEGIN SELECT RAISE(ABORT, 'integration receipts cannot be deleted'); END;
CREATE TRIGGER conflict_resolutions_immutable BEFORE UPDATE ON conflict_resolutions BEGIN SELECT RAISE(ABORT, 'conflict resolutions are immutable'); END;
CREATE TRIGGER conflict_resolutions_no_delete BEFORE DELETE ON conflict_resolutions BEGIN SELECT RAISE(ABORT, 'conflict resolutions cannot be deleted'); END;
CREATE TRIGGER conflict_resolution_validation_refs_immutable BEFORE UPDATE ON conflict_resolution_validation_refs BEGIN SELECT RAISE(ABORT, 'conflict resolution validation refs are immutable'); END;
CREATE TRIGGER conflict_resolution_validation_refs_no_delete BEFORE DELETE ON conflict_resolution_validation_refs BEGIN SELECT RAISE(ABORT, 'conflict resolution validation refs cannot be deleted'); END;

PRAGMA user_version = 7;
