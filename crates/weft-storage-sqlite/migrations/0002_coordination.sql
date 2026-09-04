CREATE TABLE operation_records (
    operation_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(operation_id)) > 0),
    event_kind TEXT NOT NULL CHECK (length(trim(event_kind)) > 0),
    actor_id TEXT NOT NULL CHECK (length(trim(actor_id)) > 0),
    occurred_at_unix_ms INTEGER NOT NULL CHECK (occurred_at_unix_ms >= 0)
) STRICT;

INSERT INTO operation_records (operation_id, event_kind, actor_id, occurred_at_unix_ms)
SELECT operation_id, event_kind, actor_id, occurred_at_unix_ms FROM audit_events;

CREATE TRIGGER operation_records_are_immutable
BEFORE UPDATE ON operation_records BEGIN
    SELECT RAISE(ABORT, 'operation records are immutable');
END;

CREATE TRIGGER operation_records_cannot_be_deleted
BEFORE DELETE ON operation_records BEGIN
    SELECT RAISE(ABORT, 'operation records cannot be deleted');
END;

CREATE TRIGGER audit_events_require_matching_operation
BEFORE INSERT ON audit_events
WHEN NOT EXISTS (
    SELECT 1 FROM operation_records AS operation
    WHERE operation.operation_id = NEW.operation_id
      AND operation.event_kind = NEW.event_kind
      AND operation.actor_id = NEW.actor_id
      AND operation.occurred_at_unix_ms = NEW.occurred_at_unix_ms
) BEGIN
    SELECT RAISE(ABORT, 'audit event requires a matching operation record');
END;

CREATE TABLE assignments (
    assignment_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(assignment_id)) > 0),
    change_id TEXT NOT NULL,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('human', 'agent', 'session', 'integration')),
    subject_id TEXT NOT NULL CHECK (length(trim(subject_id)) > 0),
    role TEXT NOT NULL CHECK (role IN ('owner', 'implementer', 'reviewer', 'resolver', 'integrator', 'observer')),
    assigned_at_unix_ms INTEGER NOT NULL CHECK (assigned_at_unix_ms >= 0),
    assigned_by TEXT NOT NULL CHECK (length(trim(assigned_by)) > 0),
    version INTEGER NOT NULL CHECK (version IN (1, 2)),
    released_at_unix_ms INTEGER,
    released_by TEXT,
    UNIQUE (assignment_id, change_id),
    CHECK (
        (version = 1 AND released_at_unix_ms IS NULL AND released_by IS NULL)
        OR
        (version = 2 AND released_at_unix_ms >= assigned_at_unix_ms AND
            released_by IS NOT NULL AND length(trim(released_by)) > 0)
    ),
    FOREIGN KEY (change_id) REFERENCES changes (change_id)
) STRICT;

CREATE UNIQUE INDEX one_active_subject_role_assignment
    ON assignments (change_id, subject_kind, subject_id, role)
    WHERE released_at_unix_ms IS NULL;

CREATE TRIGGER assignment_updates_are_release_only
BEFORE UPDATE ON assignments
WHEN NOT (
    OLD.version = 1 AND NEW.version = 2 AND
    NEW.assignment_id = OLD.assignment_id AND
    NEW.change_id = OLD.change_id AND
    NEW.subject_kind = OLD.subject_kind AND
    NEW.subject_id = OLD.subject_id AND
    NEW.role = OLD.role AND
    NEW.assigned_at_unix_ms = OLD.assigned_at_unix_ms AND
    NEW.assigned_by = OLD.assigned_by AND
    NEW.released_at_unix_ms >= OLD.assigned_at_unix_ms AND
    NEW.released_by IS NOT NULL
) BEGIN
    SELECT RAISE(ABORT, 'assignment updates may only release an active assignment');
END;

CREATE TRIGGER assignments_cannot_be_deleted
BEFORE DELETE ON assignments BEGIN
    SELECT RAISE(ABORT, 'assignments cannot be deleted');
END;

CREATE TABLE assignment_events (
    event_id INTEGER PRIMARY KEY,
    event_kind TEXT NOT NULL CHECK (event_kind IN ('assignment.assigned', 'assignment.released')),
    assignment_id TEXT NOT NULL,
    change_id TEXT NOT NULL,
    expected_version INTEGER NOT NULL,
    resulting_version INTEGER NOT NULL,
    operation_id TEXT NOT NULL UNIQUE,
    CHECK (
        (event_kind = 'assignment.assigned' AND expected_version = 0 AND resulting_version = 1)
        OR
        (event_kind = 'assignment.released' AND expected_version = 1 AND resulting_version = 2)
    ),
    UNIQUE (assignment_id, resulting_version),
    FOREIGN KEY (assignment_id, change_id) REFERENCES assignments (assignment_id, change_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TRIGGER assignment_events_require_matching_operation
BEFORE INSERT ON assignment_events
WHEN NOT EXISTS (
    SELECT 1 FROM operation_records AS operation
    WHERE operation.operation_id = NEW.operation_id
      AND operation.event_kind = NEW.event_kind
) BEGIN
    SELECT RAISE(ABORT, 'assignment event requires a matching operation record');
END;

CREATE TRIGGER assignment_events_match_projection
BEFORE INSERT ON assignment_events
WHEN NOT EXISTS (
    SELECT 1 FROM assignments AS assignment
    JOIN operation_records AS operation ON operation.operation_id = NEW.operation_id
    WHERE assignment.assignment_id = NEW.assignment_id
      AND assignment.change_id = NEW.change_id
      AND assignment.version >= NEW.resulting_version
      AND (
          (NEW.event_kind = 'assignment.assigned' AND
              operation.actor_id = assignment.assigned_by AND
              operation.occurred_at_unix_ms = assignment.assigned_at_unix_ms)
          OR
          (NEW.event_kind = 'assignment.released' AND assignment.version = 2 AND
              operation.actor_id = assignment.released_by AND
              operation.occurred_at_unix_ms = assignment.released_at_unix_ms)
      )
) BEGIN
    SELECT RAISE(ABORT, 'assignment event does not match assignment projection');
END;

CREATE TRIGGER assignment_events_are_append_only_update
BEFORE UPDATE ON assignment_events BEGIN
    SELECT RAISE(ABORT, 'assignment events are append-only');
END;

CREATE TRIGGER assignment_events_are_append_only_delete
BEFORE DELETE ON assignment_events BEGIN
    SELECT RAISE(ABORT, 'assignment events are append-only');
END;

CREATE TABLE lease_scopes (
    change_id TEXT NOT NULL,
    operation_key TEXT NOT NULL CHECK (length(trim(operation_key)) > 0),
    version INTEGER NOT NULL CHECK (version >= 0),
    current_lease_id TEXT,
    current_expires_at_unix_ms INTEGER,
    PRIMARY KEY (change_id, operation_key),
    CHECK ((current_lease_id IS NULL) = (current_expires_at_unix_ms IS NULL)),
    FOREIGN KEY (change_id) REFERENCES changes (change_id)
) STRICT;

CREATE TABLE leases (
    lease_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(lease_id)) > 0),
    change_id TEXT NOT NULL,
    operation_key TEXT NOT NULL,
    holder_kind TEXT NOT NULL CHECK (holder_kind IN ('human', 'agent', 'session', 'integration')),
    holder_id TEXT NOT NULL CHECK (length(trim(holder_id)) > 0),
    predecessor_lease_id TEXT,
    acquired_at_unix_ms INTEGER NOT NULL CHECK (acquired_at_unix_ms >= 0),
    initial_expires_at_unix_ms INTEGER NOT NULL CHECK (initial_expires_at_unix_ms > acquired_at_unix_ms),
    UNIQUE (lease_id, change_id, operation_key),
    FOREIGN KEY (change_id, operation_key) REFERENCES lease_scopes (change_id, operation_key),
    FOREIGN KEY (predecessor_lease_id) REFERENCES leases (lease_id)
) STRICT;

CREATE TRIGGER leases_are_immutable
BEFORE UPDATE ON leases BEGIN
    SELECT RAISE(ABORT, 'leases are immutable');
END;

CREATE TRIGGER leases_cannot_be_deleted
BEFORE DELETE ON leases BEGIN
    SELECT RAISE(ABORT, 'leases cannot be deleted');
END;

CREATE TRIGGER lease_scope_updates_are_versioned
BEFORE UPDATE ON lease_scopes
WHEN NOT (
    NEW.change_id = OLD.change_id AND
    NEW.operation_key = OLD.operation_key AND
    NEW.version = OLD.version + 1 AND
    (
        (OLD.current_lease_id IS NOT NULL AND
            NEW.current_lease_id = OLD.current_lease_id AND
            NEW.current_expires_at_unix_ms > OLD.current_expires_at_unix_ms)
        OR
        (OLD.current_lease_id IS NOT NULL AND NEW.current_lease_id IS NULL)
        OR
        (NEW.current_lease_id IS NOT NULL AND NEW.current_lease_id IS NOT OLD.current_lease_id AND
            EXISTS (
                SELECT 1 FROM leases AS lease
                WHERE lease.lease_id = NEW.current_lease_id
                  AND lease.change_id = NEW.change_id
                  AND lease.operation_key = NEW.operation_key
                  AND lease.predecessor_lease_id IS OLD.current_lease_id
                  AND lease.initial_expires_at_unix_ms = NEW.current_expires_at_unix_ms
            ))
    )
) BEGIN
    SELECT RAISE(ABORT, 'lease scope update is not a valid versioned transition');
END;

CREATE TABLE lease_events (
    event_id INTEGER PRIMARY KEY,
    event_kind TEXT NOT NULL CHECK (event_kind IN (
        'lease.acquired', 'lease.reclaimed', 'lease.renewed', 'lease.released'
    )),
    lease_id TEXT NOT NULL,
    change_id TEXT NOT NULL,
    operation_key TEXT NOT NULL,
    expected_version INTEGER NOT NULL CHECK (expected_version >= 0),
    resulting_version INTEGER NOT NULL CHECK (resulting_version = expected_version + 1),
    resulting_expires_at_unix_ms INTEGER,
    operation_id TEXT NOT NULL UNIQUE,
    CHECK (
        (event_kind = 'lease.released' AND resulting_expires_at_unix_ms IS NULL)
        OR
        (event_kind != 'lease.released' AND resulting_expires_at_unix_ms IS NOT NULL)
    ),
    UNIQUE (change_id, operation_key, resulting_version),
    FOREIGN KEY (lease_id, change_id, operation_key)
        REFERENCES leases (lease_id, change_id, operation_key),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TRIGGER lease_events_require_matching_operation
BEFORE INSERT ON lease_events
WHEN NOT EXISTS (
    SELECT 1 FROM operation_records AS operation
    WHERE operation.operation_id = NEW.operation_id
      AND operation.event_kind = NEW.event_kind
) BEGIN
    SELECT RAISE(ABORT, 'lease event requires a matching operation record');
END;

CREATE TRIGGER lease_events_match_projection
BEFORE INSERT ON lease_events
WHEN NOT EXISTS (
    SELECT 1 FROM lease_scopes AS scope
    WHERE scope.change_id = NEW.change_id
      AND scope.operation_key = NEW.operation_key
      AND scope.version = NEW.resulting_version
      AND (
          (NEW.event_kind = 'lease.released' AND scope.current_lease_id IS NULL)
          OR
          (NEW.event_kind != 'lease.released' AND
              scope.current_lease_id = NEW.lease_id AND
              scope.current_expires_at_unix_ms = NEW.resulting_expires_at_unix_ms)
      )
) BEGIN
    SELECT RAISE(ABORT, 'lease event does not match lease projection');
END;

CREATE TRIGGER lease_events_are_append_only_update
BEFORE UPDATE ON lease_events BEGIN
    SELECT RAISE(ABORT, 'lease events are append-only');
END;

CREATE TRIGGER lease_events_are_append_only_delete
BEFORE DELETE ON lease_events BEGIN
    SELECT RAISE(ABORT, 'lease events are append-only');
END;

PRAGMA user_version = 2;
