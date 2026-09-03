CREATE TABLE relationships (
    relationship_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(relationship_id)) > 0),
    relationship_kind TEXT NOT NULL CHECK (relationship_kind IN (
        'task_decomposition', 'related_to'
    )),
    first_change_id TEXT NOT NULL,
    second_change_id TEXT NOT NULL,
    created_at_unix_ms INTEGER NOT NULL CHECK (created_at_unix_ms >= 0),
    created_by TEXT NOT NULL CHECK (length(trim(created_by)) > 0),
    version INTEGER NOT NULL CHECK (version IN (1, 2)),
    removed_at_unix_ms INTEGER,
    removed_by TEXT,
    UNIQUE (relationship_id, first_change_id, second_change_id),
    CHECK (first_change_id < second_change_id),
    CHECK (
        (version = 1 AND removed_at_unix_ms IS NULL AND removed_by IS NULL)
        OR
        (version = 2 AND removed_at_unix_ms >= created_at_unix_ms AND
            removed_by IS NOT NULL AND length(trim(removed_by)) > 0)
    ),
    FOREIGN KEY (first_change_id) REFERENCES changes (change_id),
    FOREIGN KEY (second_change_id) REFERENCES changes (change_id)
) STRICT;

CREATE UNIQUE INDEX one_active_symmetric_relationship
    ON relationships (relationship_kind, first_change_id, second_change_id)
    WHERE removed_at_unix_ms IS NULL;

CREATE TRIGGER relationship_updates_are_removal_only
BEFORE UPDATE ON relationships
WHEN NOT (
    OLD.version = 1 AND NEW.version = 2 AND
    NEW.relationship_id = OLD.relationship_id AND
    NEW.relationship_kind = OLD.relationship_kind AND
    NEW.first_change_id = OLD.first_change_id AND
    NEW.second_change_id = OLD.second_change_id AND
    NEW.created_at_unix_ms = OLD.created_at_unix_ms AND
    NEW.created_by = OLD.created_by AND
    NEW.removed_at_unix_ms >= OLD.created_at_unix_ms AND
    NEW.removed_by IS NOT NULL AND length(trim(NEW.removed_by)) > 0
) BEGIN
    SELECT RAISE(ABORT, 'relationship update may only remove an active record');
END;

CREATE TRIGGER relationships_cannot_be_deleted
BEFORE DELETE ON relationships BEGIN
    SELECT RAISE(ABORT, 'relationships cannot be deleted');
END;

CREATE TABLE relationship_events (
    event_id INTEGER PRIMARY KEY,
    event_kind TEXT NOT NULL CHECK (event_kind IN (
        'relationship.created', 'relationship.removed'
    )),
    relationship_id TEXT NOT NULL,
    first_change_id TEXT NOT NULL,
    second_change_id TEXT NOT NULL,
    expected_version INTEGER NOT NULL CHECK (expected_version >= 0),
    resulting_version INTEGER NOT NULL CHECK (resulting_version = expected_version + 1),
    operation_id TEXT NOT NULL UNIQUE,
    CHECK (
        (event_kind = 'relationship.created' AND expected_version = 0 AND resulting_version = 1)
        OR
        (event_kind = 'relationship.removed' AND expected_version = 1 AND resulting_version = 2)
    ),
    UNIQUE (relationship_id, resulting_version),
    FOREIGN KEY (relationship_id, first_change_id, second_change_id)
        REFERENCES relationships (relationship_id, first_change_id, second_change_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TRIGGER relationship_events_require_matching_operation
BEFORE INSERT ON relationship_events
WHEN NOT EXISTS (
    SELECT 1 FROM operation_records AS operation
    WHERE operation.operation_id = NEW.operation_id
      AND operation.event_kind = NEW.event_kind
) BEGIN
    SELECT RAISE(ABORT, 'relationship event requires a matching operation record');
END;

CREATE TRIGGER relationship_events_match_projection
BEFORE INSERT ON relationship_events
WHEN NOT EXISTS (
    SELECT 1 FROM relationships AS relationship
    JOIN operation_records AS operation ON operation.operation_id = NEW.operation_id
    WHERE relationship.relationship_id = NEW.relationship_id
      AND relationship.first_change_id = NEW.first_change_id
      AND relationship.second_change_id = NEW.second_change_id
      AND relationship.version = NEW.resulting_version
      AND (
          (NEW.event_kind = 'relationship.created' AND
              operation.actor_id = relationship.created_by AND
              operation.occurred_at_unix_ms = relationship.created_at_unix_ms)
          OR
          (NEW.event_kind = 'relationship.removed' AND
              operation.actor_id = relationship.removed_by AND
              operation.occurred_at_unix_ms = relationship.removed_at_unix_ms)
      )
) BEGIN
    SELECT RAISE(ABORT, 'relationship event does not match its projection');
END;

CREATE TRIGGER relationship_events_are_append_only_update
BEFORE UPDATE ON relationship_events BEGIN
    SELECT RAISE(ABORT, 'relationship events are append-only');
END;

CREATE TRIGGER relationship_events_are_append_only_delete
BEFORE DELETE ON relationship_events BEGIN
    SELECT RAISE(ABORT, 'relationship events are append-only');
END;

CREATE TABLE dependencies (
    dependency_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(dependency_id)) > 0),
    downstream_change_id TEXT NOT NULL,
    upstream_change_id TEXT NOT NULL,
    downstream_revision_id TEXT NOT NULL,
    upstream_revision_id TEXT NOT NULL,
    created_at_unix_ms INTEGER NOT NULL CHECK (created_at_unix_ms >= 0),
    created_by TEXT NOT NULL CHECK (length(trim(created_by)) > 0),
    version INTEGER NOT NULL CHECK (version >= 1),
    updated_at_unix_ms INTEGER NOT NULL CHECK (updated_at_unix_ms >= created_at_unix_ms),
    updated_by TEXT NOT NULL CHECK (length(trim(updated_by)) > 0),
    removed_at_unix_ms INTEGER,
    removed_by TEXT,
    UNIQUE (dependency_id, downstream_change_id, upstream_change_id),
    CHECK (downstream_change_id != upstream_change_id),
    CHECK (
        (removed_at_unix_ms IS NULL AND removed_by IS NULL)
        OR
        (removed_at_unix_ms = updated_at_unix_ms AND removed_by = updated_by)
    ),
    FOREIGN KEY (downstream_change_id) REFERENCES changes (change_id),
    FOREIGN KEY (upstream_change_id) REFERENCES changes (change_id),
    FOREIGN KEY (downstream_revision_id, downstream_change_id)
        REFERENCES change_revisions (revision_id, change_id),
    FOREIGN KEY (upstream_revision_id, upstream_change_id)
        REFERENCES change_revisions (revision_id, change_id)
) STRICT;

CREATE UNIQUE INDEX one_active_directed_dependency
    ON dependencies (downstream_change_id, upstream_change_id)
    WHERE removed_at_unix_ms IS NULL;

CREATE TRIGGER dependencies_reject_cycles
BEFORE INSERT ON dependencies
WHEN EXISTS (
    WITH RECURSIVE reachable(change_id) AS (
        SELECT NEW.upstream_change_id
        UNION
        SELECT dependency.upstream_change_id
        FROM dependencies AS dependency
        JOIN reachable ON dependency.downstream_change_id = reachable.change_id
        WHERE dependency.removed_at_unix_ms IS NULL
    )
    SELECT 1 FROM reachable WHERE change_id = NEW.downstream_change_id
) BEGIN
    SELECT RAISE(ABORT, 'active dependency would create a cycle');
END;

CREATE TRIGGER dependency_updates_are_versioned
BEFORE UPDATE ON dependencies
WHEN NOT (
    OLD.removed_at_unix_ms IS NULL AND
    NEW.dependency_id = OLD.dependency_id AND
    NEW.downstream_change_id = OLD.downstream_change_id AND
    NEW.upstream_change_id = OLD.upstream_change_id AND
    NEW.created_at_unix_ms = OLD.created_at_unix_ms AND
    NEW.created_by = OLD.created_by AND
    NEW.version = OLD.version + 1 AND
    NEW.updated_at_unix_ms >= OLD.updated_at_unix_ms AND
    (
        (NEW.removed_at_unix_ms IS NULL AND NEW.removed_by IS NULL AND
            (NEW.downstream_revision_id != OLD.downstream_revision_id OR
                NEW.upstream_revision_id != OLD.upstream_revision_id))
        OR
        (NEW.downstream_revision_id = OLD.downstream_revision_id AND
            NEW.upstream_revision_id = OLD.upstream_revision_id AND
            NEW.removed_at_unix_ms = NEW.updated_at_unix_ms AND
            NEW.removed_by = NEW.updated_by)
    )
) BEGIN
    SELECT RAISE(ABORT, 'dependency update is not a valid repin or removal');
END;

CREATE TRIGGER dependencies_cannot_be_deleted
BEFORE DELETE ON dependencies BEGIN
    SELECT RAISE(ABORT, 'dependencies cannot be deleted');
END;

CREATE TABLE dependency_events (
    event_id INTEGER PRIMARY KEY,
    event_kind TEXT NOT NULL CHECK (event_kind IN (
        'dependency.created', 'dependency.repinned', 'dependency.removed'
    )),
    dependency_id TEXT NOT NULL,
    downstream_change_id TEXT NOT NULL,
    upstream_change_id TEXT NOT NULL,
    expected_version INTEGER NOT NULL CHECK (expected_version >= 0),
    resulting_version INTEGER NOT NULL CHECK (resulting_version = expected_version + 1),
    resulting_downstream_revision_id TEXT NOT NULL,
    resulting_upstream_revision_id TEXT NOT NULL,
    operation_id TEXT NOT NULL UNIQUE,
    CHECK (
        (event_kind = 'dependency.created' AND expected_version = 0 AND resulting_version = 1)
        OR
        (event_kind IN ('dependency.repinned', 'dependency.removed') AND expected_version >= 1)
    ),
    UNIQUE (dependency_id, resulting_version),
    FOREIGN KEY (dependency_id, downstream_change_id, upstream_change_id)
        REFERENCES dependencies (dependency_id, downstream_change_id, upstream_change_id),
    FOREIGN KEY (resulting_downstream_revision_id, downstream_change_id)
        REFERENCES change_revisions (revision_id, change_id),
    FOREIGN KEY (resulting_upstream_revision_id, upstream_change_id)
        REFERENCES change_revisions (revision_id, change_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TRIGGER dependency_events_require_matching_operation
BEFORE INSERT ON dependency_events
WHEN NOT EXISTS (
    SELECT 1 FROM operation_records AS operation
    WHERE operation.operation_id = NEW.operation_id
      AND operation.event_kind = NEW.event_kind
) BEGIN
    SELECT RAISE(ABORT, 'dependency event requires a matching operation record');
END;

CREATE TRIGGER dependency_events_match_projection
BEFORE INSERT ON dependency_events
WHEN NOT EXISTS (
    SELECT 1 FROM dependencies AS dependency
    JOIN operation_records AS operation ON operation.operation_id = NEW.operation_id
    WHERE dependency.dependency_id = NEW.dependency_id
      AND dependency.downstream_change_id = NEW.downstream_change_id
      AND dependency.upstream_change_id = NEW.upstream_change_id
      AND dependency.version = NEW.resulting_version
      AND dependency.downstream_revision_id = NEW.resulting_downstream_revision_id
      AND dependency.upstream_revision_id = NEW.resulting_upstream_revision_id
      AND operation.actor_id = dependency.updated_by
      AND operation.occurred_at_unix_ms = dependency.updated_at_unix_ms
      AND (
          (NEW.event_kind != 'dependency.removed' AND dependency.removed_at_unix_ms IS NULL)
          OR
          (NEW.event_kind = 'dependency.removed' AND
              dependency.removed_at_unix_ms = operation.occurred_at_unix_ms)
      )
) BEGIN
    SELECT RAISE(ABORT, 'dependency event does not match its projection');
END;

CREATE TRIGGER dependency_events_are_append_only_update
BEFORE UPDATE ON dependency_events BEGIN
    SELECT RAISE(ABORT, 'dependency events are append-only');
END;

CREATE TRIGGER dependency_events_are_append_only_delete
BEFORE DELETE ON dependency_events BEGIN
    SELECT RAISE(ABORT, 'dependency events are append-only');
END;

PRAGMA user_version = 4;
