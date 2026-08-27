CREATE TABLE stacks (
    stack_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(stack_id)) > 0),
    policy TEXT NOT NULL CHECK (policy IN ('order_only', 'predecessor_dependencies')),
    version INTEGER NOT NULL CHECK (version >= 1),
    created_at_unix_ms INTEGER NOT NULL CHECK (created_at_unix_ms >= 0),
    created_by TEXT NOT NULL CHECK (length(trim(created_by)) > 0),
    updated_at_unix_ms INTEGER NOT NULL CHECK (updated_at_unix_ms >= created_at_unix_ms),
    updated_by TEXT NOT NULL CHECK (length(trim(updated_by)) > 0)
) STRICT;

CREATE TABLE stack_members (
    stack_id TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    change_id TEXT NOT NULL,
    predecessor_change_id TEXT,
    PRIMARY KEY (stack_id, position),
    UNIQUE (stack_id, change_id),
    CHECK (
        (position = 0 AND predecessor_change_id IS NULL) OR
        (position > 0 AND predecessor_change_id IS NOT NULL AND predecessor_change_id != change_id)
    ),
    FOREIGN KEY (stack_id) REFERENCES stacks (stack_id),
    FOREIGN KEY (change_id) REFERENCES changes (change_id),
    FOREIGN KEY (predecessor_change_id) REFERENCES changes (change_id)
) STRICT;

CREATE TABLE stack_events (
    event_id INTEGER PRIMARY KEY,
    event_kind TEXT NOT NULL CHECK (event_kind IN ('stack.created', 'stack.revised')),
    stack_id TEXT NOT NULL,
    expected_version INTEGER NOT NULL CHECK (expected_version >= 0),
    resulting_version INTEGER NOT NULL CHECK (resulting_version = expected_version + 1),
    resulting_policy TEXT NOT NULL CHECK (resulting_policy IN ('order_only', 'predecessor_dependencies')),
    member_count INTEGER NOT NULL CHECK (member_count > 0),
    operation_id TEXT NOT NULL UNIQUE,
    CHECK (
        (event_kind = 'stack.created' AND expected_version = 0 AND resulting_version = 1) OR
        (event_kind = 'stack.revised' AND expected_version >= 1)
    ),
    UNIQUE (stack_id, resulting_version),
    FOREIGN KEY (stack_id) REFERENCES stacks (stack_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TABLE stack_event_members (
    event_id INTEGER NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    change_id TEXT NOT NULL,
    predecessor_change_id TEXT,
    PRIMARY KEY (event_id, position),
    UNIQUE (event_id, change_id),
    CHECK (
        (position = 0 AND predecessor_change_id IS NULL) OR
        (position > 0 AND predecessor_change_id IS NOT NULL AND predecessor_change_id != change_id)
    ),
    FOREIGN KEY (event_id) REFERENCES stack_events (event_id),
    FOREIGN KEY (change_id) REFERENCES changes (change_id),
    FOREIGN KEY (predecessor_change_id) REFERENCES changes (change_id)
) STRICT;

CREATE TRIGGER stack_event_members_stay_within_finalized_snapshot
BEFORE INSERT ON stack_event_members
WHEN NEW.position >= (
    SELECT event.member_count FROM stack_events AS event WHERE event.event_id = NEW.event_id
) BEGIN
    SELECT RAISE(ABORT, 'Stack snapshot member exceeds its finalized size');
END;

CREATE TRIGGER stacks_cannot_be_deleted BEFORE DELETE ON stacks BEGIN
    SELECT RAISE(ABORT, 'Stacks cannot be deleted');
END;
CREATE TRIGGER stack_events_are_append_only_update BEFORE UPDATE ON stack_events BEGIN
    SELECT RAISE(ABORT, 'Stack events are append-only');
END;
CREATE TRIGGER stack_events_are_append_only_delete BEFORE DELETE ON stack_events BEGIN
    SELECT RAISE(ABORT, 'Stack events are append-only');
END;
CREATE TRIGGER stack_event_members_are_append_only_update BEFORE UPDATE ON stack_event_members BEGIN
    SELECT RAISE(ABORT, 'Stack event members are append-only');
END;
CREATE TRIGGER stack_event_members_are_append_only_delete BEFORE DELETE ON stack_event_members BEGIN
    SELECT RAISE(ABORT, 'Stack event members are append-only');
END;

CREATE TABLE composition_candidates (
    candidate_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(candidate_id)) > 0),
    repository_id TEXT NOT NULL CHECK (length(trim(repository_id)) > 0),
    target_object_id TEXT NOT NULL CHECK (length(trim(target_object_id)) > 0),
    stack_id TEXT,
    stack_version INTEGER,
    stack_policy TEXT CHECK (stack_policy IN ('order_only', 'predecessor_dependencies')),
    content_digest TEXT NOT NULL CHECK (
        length(content_digest) = 71 AND substr(content_digest, 1, 7) = 'sha256:' AND
        substr(content_digest, 8) NOT GLOB '*[^0-9a-f]*'
    ),
    created_at_unix_ms INTEGER NOT NULL CHECK (created_at_unix_ms >= 0),
    created_by TEXT NOT NULL CHECK (length(trim(created_by)) > 0),
    operation_id TEXT NOT NULL UNIQUE,
    CHECK (
        (stack_id IS NULL AND stack_version IS NULL AND stack_policy IS NULL) OR
        (stack_id IS NOT NULL AND stack_version >= 1 AND stack_policy IS NOT NULL)
    ),
    FOREIGN KEY (stack_id) REFERENCES stacks (stack_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TABLE candidate_inputs (
    candidate_id TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    change_id TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    PRIMARY KEY (candidate_id, position),
    UNIQUE (candidate_id, change_id),
    FOREIGN KEY (candidate_id) REFERENCES composition_candidates (candidate_id),
    FOREIGN KEY (revision_id, change_id) REFERENCES change_revisions (revision_id, change_id)
) STRICT;

CREATE TABLE candidate_requirements (
    candidate_id TEXT NOT NULL,
    requirement_index INTEGER NOT NULL CHECK (requirement_index >= 0),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('dependency', 'stack_predecessor')),
    source_id TEXT NOT NULL,
    source_version INTEGER NOT NULL CHECK (source_version >= 1),
    downstream_position INTEGER NOT NULL CHECK (downstream_position > 0),
    downstream_change_id TEXT NOT NULL,
    downstream_revision_id TEXT NOT NULL,
    upstream_change_id TEXT NOT NULL,
    upstream_revision_id TEXT NOT NULL,
    PRIMARY KEY (candidate_id, requirement_index),
    UNIQUE (
        candidate_id, source_kind, source_id, source_version, downstream_position,
        downstream_change_id, downstream_revision_id, upstream_change_id, upstream_revision_id
    ),
    CHECK (downstream_change_id != upstream_change_id),
    FOREIGN KEY (candidate_id) REFERENCES composition_candidates (candidate_id),
    FOREIGN KEY (downstream_revision_id, downstream_change_id)
        REFERENCES change_revisions (revision_id, change_id),
    FOREIGN KEY (upstream_revision_id, upstream_change_id)
        REFERENCES change_revisions (revision_id, change_id)
) STRICT;

CREATE TRIGGER composition_candidates_are_immutable_update BEFORE UPDATE ON composition_candidates BEGIN
    SELECT RAISE(ABORT, 'CompositionCandidates are immutable');
END;
CREATE TRIGGER composition_candidates_cannot_be_deleted BEFORE DELETE ON composition_candidates BEGIN
    SELECT RAISE(ABORT, 'CompositionCandidates cannot be deleted');
END;
CREATE TRIGGER candidate_inputs_are_immutable_update BEFORE UPDATE ON candidate_inputs BEGIN
    SELECT RAISE(ABORT, 'candidate inputs are immutable');
END;
CREATE TRIGGER candidate_inputs_cannot_be_deleted BEFORE DELETE ON candidate_inputs BEGIN
    SELECT RAISE(ABORT, 'candidate inputs cannot be deleted');
END;
CREATE TRIGGER candidate_requirements_are_immutable_update BEFORE UPDATE ON candidate_requirements BEGIN
    SELECT RAISE(ABORT, 'candidate requirements are immutable');
END;
CREATE TRIGGER candidate_requirements_cannot_be_deleted BEFORE DELETE ON candidate_requirements BEGIN
    SELECT RAISE(ABORT, 'candidate requirements cannot be deleted');
END;

PRAGMA user_version = 5;
