CREATE TABLE changes (
    change_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(change_id)) > 0),
    head_revision_id TEXT,
    FOREIGN KEY (head_revision_id, change_id)
        REFERENCES change_revisions (revision_id, change_id)
        DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE change_revisions (
    revision_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(revision_id)) > 0),
    change_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    parent_revision_id TEXT,
    repository_id TEXT NOT NULL CHECK (length(trim(repository_id)) > 0),
    base_object_id TEXT NOT NULL CHECK (length(trim(base_object_id)) > 0),
    artifact_version TEXT NOT NULL CHECK (artifact_version = 'tree-delta-v1'),
    artifact_digest TEXT NOT NULL CHECK (
        length(artifact_digest) = 71 AND
        substr(artifact_digest, 1, 7) = 'sha256:' AND
        substr(artifact_digest, 8) NOT GLOB '*[^0-9a-f]*'
    ),
    created_at_unix_ms INTEGER NOT NULL CHECK (created_at_unix_ms >= 0),
    created_by TEXT NOT NULL CHECK (length(trim(created_by)) > 0),
    UNIQUE (revision_id, change_id),
    UNIQUE (change_id, sequence),
    FOREIGN KEY (change_id) REFERENCES changes (change_id),
    FOREIGN KEY (parent_revision_id, change_id)
        REFERENCES change_revisions (revision_id, change_id)
        DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE UNIQUE INDEX one_root_revision_per_change
    ON change_revisions (change_id) WHERE parent_revision_id IS NULL;
CREATE UNIQUE INDEX one_child_per_revision
    ON change_revisions (change_id, parent_revision_id)
    WHERE parent_revision_id IS NOT NULL;

CREATE TRIGGER change_identity_is_immutable
BEFORE UPDATE OF change_id ON changes BEGIN
    SELECT RAISE(ABORT, 'Change identity is immutable');
END;

CREATE TRIGGER changes_cannot_be_deleted
BEFORE DELETE ON changes BEGIN
    SELECT RAISE(ABORT, 'Changes cannot be deleted');
END;

CREATE TRIGGER change_revisions_are_immutable
BEFORE UPDATE ON change_revisions BEGIN
    SELECT RAISE(ABORT, 'Change revisions are immutable');
END;

CREATE TRIGGER change_revisions_cannot_be_deleted
BEFORE DELETE ON change_revisions BEGIN
    SELECT RAISE(ABORT, 'Change revisions cannot be deleted');
END;

CREATE TABLE audit_events (
    event_id INTEGER PRIMARY KEY,
    event_kind TEXT NOT NULL CHECK (event_kind IN ('change.created', 'revision.appended')),
    change_id TEXT NOT NULL,
    revision_id TEXT,
    expected_head_revision_id TEXT,
    resulting_head_revision_id TEXT,
    operation_id TEXT NOT NULL UNIQUE CHECK (length(trim(operation_id)) > 0),
    actor_id TEXT NOT NULL CHECK (length(trim(actor_id)) > 0),
    occurred_at_unix_ms INTEGER NOT NULL CHECK (occurred_at_unix_ms >= 0),
    CHECK (
        (event_kind = 'change.created' AND
            revision_id IS NULL AND
            expected_head_revision_id IS NULL AND
            resulting_head_revision_id IS NULL)
        OR
        (event_kind = 'revision.appended' AND
            revision_id IS NOT NULL AND
            resulting_head_revision_id = revision_id)
    ),
    FOREIGN KEY (change_id) REFERENCES changes (change_id),
    FOREIGN KEY (revision_id, change_id)
        REFERENCES change_revisions (revision_id, change_id),
    FOREIGN KEY (expected_head_revision_id, change_id)
        REFERENCES change_revisions (revision_id, change_id),
    FOREIGN KEY (resulting_head_revision_id, change_id)
        REFERENCES change_revisions (revision_id, change_id)
) STRICT;

CREATE UNIQUE INDEX one_change_creation_event
    ON audit_events (change_id) WHERE event_kind = 'change.created';
CREATE UNIQUE INDEX one_revision_append_event
    ON audit_events (change_id, revision_id) WHERE event_kind = 'revision.appended';

CREATE TRIGGER revision_audit_matches_parent
BEFORE INSERT ON audit_events
WHEN NEW.event_kind = 'revision.appended' AND NOT EXISTS (
    SELECT 1 FROM change_revisions AS revision
    WHERE revision.revision_id = NEW.revision_id
      AND revision.change_id = NEW.change_id
      AND revision.parent_revision_id IS NEW.expected_head_revision_id
) BEGIN
    SELECT RAISE(ABORT, 'revision audit expected head does not match parent');
END;

CREATE TRIGGER audit_events_are_append_only_update
BEFORE UPDATE ON audit_events BEGIN
    SELECT RAISE(ABORT, 'audit events are append-only');
END;

CREATE TRIGGER audit_events_are_append_only_delete
BEFORE DELETE ON audit_events BEGIN
    SELECT RAISE(ABORT, 'audit events are append-only');
END;

PRAGMA user_version = 1;
