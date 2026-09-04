CREATE TABLE materializations (
    materialization_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(materialization_id)) > 0),
    change_id TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL CHECK (length(trim(workspace_id)) > 0),
    provider_id TEXT NOT NULL CHECK (length(trim(provider_id)) > 0),
    current_provider_ref TEXT NOT NULL CHECK (length(trim(current_provider_ref)) > 0),
    state TEXT NOT NULL CHECK (state IN (
        'clean', 'dirty', 'diverged', 'suspended', 'released', 'invalidated'
    )),
    version INTEGER NOT NULL CHECK (version >= 1),
    created_at_unix_ms INTEGER NOT NULL CHECK (created_at_unix_ms >= 0),
    created_by TEXT NOT NULL CHECK (length(trim(created_by)) > 0),
    state_changed_at_unix_ms INTEGER NOT NULL CHECK (
        state_changed_at_unix_ms >= created_at_unix_ms
    ),
    released_at_unix_ms INTEGER,
    UNIQUE (materialization_id, change_id),
    CHECK (
        (state = 'released' AND released_at_unix_ms = state_changed_at_unix_ms)
        OR
        (state != 'released' AND released_at_unix_ms IS NULL)
    ),
    FOREIGN KEY (revision_id, change_id)
        REFERENCES change_revisions (revision_id, change_id)
) STRICT;

CREATE UNIQUE INDEX one_active_change_materialization_per_workspace_provider
    ON materializations (change_id, workspace_id, provider_id)
    WHERE state NOT IN ('released', 'invalidated');

CREATE TRIGGER materialization_updates_are_versioned_transitions
BEFORE UPDATE ON materializations
WHEN NOT (
    OLD.state NOT IN ('released', 'invalidated') AND
    NEW.materialization_id = OLD.materialization_id AND
    NEW.change_id = OLD.change_id AND
    NEW.revision_id = OLD.revision_id AND
    NEW.workspace_id = OLD.workspace_id AND
    NEW.provider_id = OLD.provider_id AND
    NEW.created_at_unix_ms = OLD.created_at_unix_ms AND
    NEW.created_by = OLD.created_by AND
    NEW.version = OLD.version + 1 AND
    NEW.state_changed_at_unix_ms >= OLD.state_changed_at_unix_ms AND
    (NEW.state != OLD.state OR NEW.current_provider_ref != OLD.current_provider_ref) AND
    (
        (NEW.state = 'released' AND
            NEW.released_at_unix_ms = NEW.state_changed_at_unix_ms)
        OR
        (NEW.state != 'released' AND NEW.released_at_unix_ms IS NULL)
    )
) BEGIN
    SELECT RAISE(ABORT, 'materialization update is not a valid versioned transition');
END;

CREATE TRIGGER materializations_cannot_be_deleted
BEFORE DELETE ON materializations BEGIN
    SELECT RAISE(ABORT, 'materializations cannot be deleted');
END;

CREATE TABLE materialization_events (
    event_id INTEGER PRIMARY KEY,
    event_kind TEXT NOT NULL CHECK (event_kind IN (
        'materialization.created', 'materialization.transitioned'
    )),
    materialization_id TEXT NOT NULL,
    change_id TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    expected_version INTEGER NOT NULL CHECK (expected_version >= 0),
    resulting_version INTEGER NOT NULL CHECK (resulting_version = expected_version + 1),
    resulting_state TEXT NOT NULL CHECK (resulting_state IN (
        'clean', 'dirty', 'diverged', 'suspended', 'released', 'invalidated'
    )),
    resulting_provider_ref TEXT NOT NULL CHECK (length(trim(resulting_provider_ref)) > 0),
    provider_evidence TEXT NOT NULL CHECK (length(trim(provider_evidence)) > 0),
    operation_id TEXT NOT NULL UNIQUE,
    CHECK (
        (event_kind = 'materialization.created' AND expected_version = 0 AND
            resulting_version = 1 AND resulting_state = 'clean')
        OR
        (event_kind = 'materialization.transitioned' AND expected_version >= 1)
    ),
    UNIQUE (materialization_id, resulting_version),
    FOREIGN KEY (materialization_id, change_id)
        REFERENCES materializations (materialization_id, change_id),
    FOREIGN KEY (revision_id, change_id)
        REFERENCES change_revisions (revision_id, change_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TRIGGER materialization_events_require_matching_operation
BEFORE INSERT ON materialization_events
WHEN NOT EXISTS (
    SELECT 1 FROM operation_records AS operation
    WHERE operation.operation_id = NEW.operation_id
      AND operation.event_kind = NEW.event_kind
) BEGIN
    SELECT RAISE(ABORT, 'materialization event requires a matching operation record');
END;

CREATE TRIGGER materialization_events_match_projection
BEFORE INSERT ON materialization_events
WHEN NOT EXISTS (
    SELECT 1 FROM materializations AS materialization
    JOIN operation_records AS operation ON operation.operation_id = NEW.operation_id
    WHERE materialization.materialization_id = NEW.materialization_id
      AND materialization.change_id = NEW.change_id
      AND materialization.revision_id = NEW.revision_id
      AND materialization.version = NEW.resulting_version
      AND materialization.state = NEW.resulting_state
      AND materialization.current_provider_ref = NEW.resulting_provider_ref
      AND operation.occurred_at_unix_ms = materialization.state_changed_at_unix_ms
      AND (
          NEW.event_kind = 'materialization.transitioned'
          OR
          (operation.actor_id = materialization.created_by AND
              operation.occurred_at_unix_ms = materialization.created_at_unix_ms)
      )
) BEGIN
    SELECT RAISE(ABORT, 'materialization event does not match its projection');
END;

CREATE TRIGGER materialization_events_are_append_only_update
BEFORE UPDATE ON materialization_events BEGIN
    SELECT RAISE(ABORT, 'materialization events are append-only');
END;

CREATE TRIGGER materialization_events_are_append_only_delete
BEFORE DELETE ON materialization_events BEGIN
    SELECT RAISE(ABORT, 'materialization events are append-only');
END;

PRAGMA user_version = 3;
