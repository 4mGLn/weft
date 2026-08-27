CREATE TABLE review_requests (
    review_request_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(review_request_id)) > 0),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('revision', 'candidate')),
    change_id TEXT,
    revision_id TEXT,
    candidate_id TEXT,
    repository_id TEXT NOT NULL CHECK (length(trim(repository_id)) > 0),
    context_object_id TEXT NOT NULL CHECK (length(trim(context_object_id)) > 0),
    content_digest TEXT NOT NULL CHECK (
        length(content_digest) = 71 AND substr(content_digest, 1, 7) = 'sha256:' AND
        substr(content_digest, 8) NOT GLOB '*[^0-9a-f]*'
    ),
    requested_by TEXT NOT NULL CHECK (length(trim(requested_by)) > 0),
    reviewer_count INTEGER NOT NULL CHECK (reviewer_count > 0),
    reuse_policy TEXT NOT NULL CHECK (reuse_policy = 'new_submission_required'),
    created_at_unix_ms INTEGER NOT NULL CHECK (created_at_unix_ms >= 0),
    operation_id TEXT NOT NULL UNIQUE,
    CHECK (
        (target_kind = 'revision' AND change_id IS NOT NULL AND revision_id IS NOT NULL AND candidate_id IS NULL)
        OR
        (target_kind = 'candidate' AND change_id IS NULL AND revision_id IS NULL AND candidate_id IS NOT NULL)
    ),
    FOREIGN KEY (revision_id, change_id) REFERENCES change_revisions (revision_id, change_id),
    FOREIGN KEY (candidate_id) REFERENCES composition_candidates (candidate_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TABLE review_request_reviewers (
    review_request_id TEXT NOT NULL,
    reviewer_position INTEGER NOT NULL CHECK (reviewer_position >= 0),
    reviewer_id TEXT NOT NULL CHECK (length(trim(reviewer_id)) > 0),
    PRIMARY KEY (review_request_id, reviewer_position),
    UNIQUE (review_request_id, reviewer_id),
    FOREIGN KEY (review_request_id) REFERENCES review_requests (review_request_id)
) STRICT;

CREATE TRIGGER review_request_reviewers_stay_within_finalized_set
BEFORE INSERT ON review_request_reviewers
WHEN NEW.reviewer_position >= (
    SELECT request.reviewer_count FROM review_requests AS request
    WHERE request.review_request_id = NEW.review_request_id
) BEGIN
    SELECT RAISE(ABORT, 'reviewer exceeds finalized request size');
END;

CREATE TABLE review_submissions (
    review_submission_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(review_submission_id)) > 0),
    review_request_id TEXT NOT NULL,
    reviewer_id TEXT NOT NULL CHECK (length(trim(reviewer_id)) > 0),
    outcome TEXT NOT NULL CHECK (outcome IN ('approved', 'changes_requested', 'rejected', 'blocked')),
    comments TEXT CHECK (comments IS NULL OR length(trim(comments)) > 0),
    submitted_at_unix_ms INTEGER NOT NULL CHECK (submitted_at_unix_ms >= 0),
    operation_id TEXT NOT NULL UNIQUE,
    FOREIGN KEY (review_request_id, reviewer_id)
        REFERENCES review_request_reviewers (review_request_id, reviewer_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TABLE validation_results (
    validation_result_id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(validation_result_id)) > 0),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('revision', 'candidate')),
    change_id TEXT,
    revision_id TEXT,
    candidate_id TEXT,
    repository_id TEXT NOT NULL CHECK (length(trim(repository_id)) > 0),
    context_object_id TEXT NOT NULL CHECK (length(trim(context_object_id)) > 0),
    content_digest TEXT NOT NULL CHECK (
        length(content_digest) = 71 AND substr(content_digest, 1, 7) = 'sha256:' AND
        substr(content_digest, 8) NOT GLOB '*[^0-9a-f]*'
    ),
    validation_type TEXT NOT NULL CHECK (length(trim(validation_type)) > 0),
    environment TEXT NOT NULL CHECK (length(trim(environment)) > 0),
    outcome TEXT NOT NULL CHECK (outcome IN ('passed', 'failed', 'blocked', 'error')),
    execution_id TEXT NOT NULL CHECK (length(trim(execution_id)) > 0),
    scope_kind TEXT NOT NULL CHECK (scope_kind IN ('exact_target', 'declared_reusable')),
    reusable_scope TEXT,
    scope_rationale TEXT,
    validated_by TEXT NOT NULL CHECK (length(trim(validated_by)) > 0),
    validated_at_unix_ms INTEGER NOT NULL CHECK (validated_at_unix_ms >= 0),
    operation_id TEXT NOT NULL UNIQUE,
    CHECK (
        (target_kind = 'revision' AND change_id IS NOT NULL AND revision_id IS NOT NULL AND candidate_id IS NULL)
        OR
        (target_kind = 'candidate' AND change_id IS NULL AND revision_id IS NULL AND candidate_id IS NOT NULL)
    ),
    CHECK (
        (scope_kind = 'exact_target' AND reusable_scope IS NULL AND scope_rationale IS NULL)
        OR
        (scope_kind = 'declared_reusable' AND length(trim(reusable_scope)) > 0 AND length(trim(scope_rationale)) > 0)
    ),
    FOREIGN KEY (revision_id, change_id) REFERENCES change_revisions (revision_id, change_id),
    FOREIGN KEY (candidate_id) REFERENCES composition_candidates (candidate_id),
    FOREIGN KEY (operation_id) REFERENCES operation_records (operation_id)
) STRICT;

CREATE TRIGGER review_requests_match_operation
BEFORE INSERT ON review_requests
WHEN NOT EXISTS (
    SELECT 1 FROM operation_records AS operation
    WHERE operation.operation_id = NEW.operation_id
      AND operation.event_kind = 'review.requested'
      AND operation.actor_id = NEW.requested_by
      AND operation.occurred_at_unix_ms = NEW.created_at_unix_ms
) BEGIN
    SELECT RAISE(ABORT, 'review request requires matching operation provenance');
END;

CREATE TRIGGER review_submissions_match_operation
BEFORE INSERT ON review_submissions
WHEN NOT EXISTS (
    SELECT 1 FROM operation_records AS operation
    WHERE operation.operation_id = NEW.operation_id
      AND operation.event_kind = 'review.submitted'
      AND operation.actor_id = NEW.reviewer_id
      AND operation.occurred_at_unix_ms = NEW.submitted_at_unix_ms
) BEGIN
    SELECT RAISE(ABORT, 'review submission requires matching operation provenance');
END;

CREATE TRIGGER validation_results_match_operation
BEFORE INSERT ON validation_results
WHEN NOT EXISTS (
    SELECT 1 FROM operation_records AS operation
    WHERE operation.operation_id = NEW.operation_id
      AND operation.event_kind = 'validation.recorded'
      AND operation.actor_id = NEW.validated_by
      AND operation.occurred_at_unix_ms = NEW.validated_at_unix_ms
) BEGIN
    SELECT RAISE(ABORT, 'validation result requires matching operation provenance');
END;

CREATE TRIGGER review_requests_are_immutable_update BEFORE UPDATE ON review_requests BEGIN
    SELECT RAISE(ABORT, 'review requests are immutable');
END;
CREATE TRIGGER review_requests_cannot_be_deleted BEFORE DELETE ON review_requests BEGIN
    SELECT RAISE(ABORT, 'review requests cannot be deleted');
END;
CREATE TRIGGER review_request_reviewers_are_immutable_update BEFORE UPDATE ON review_request_reviewers BEGIN
    SELECT RAISE(ABORT, 'review request reviewers are immutable');
END;
CREATE TRIGGER review_request_reviewers_cannot_be_deleted BEFORE DELETE ON review_request_reviewers BEGIN
    SELECT RAISE(ABORT, 'review request reviewers cannot be deleted');
END;
CREATE TRIGGER review_submissions_are_immutable_update BEFORE UPDATE ON review_submissions BEGIN
    SELECT RAISE(ABORT, 'review submissions are immutable');
END;
CREATE TRIGGER review_submissions_cannot_be_deleted BEFORE DELETE ON review_submissions BEGIN
    SELECT RAISE(ABORT, 'review submissions cannot be deleted');
END;
CREATE TRIGGER validation_results_are_immutable_update BEFORE UPDATE ON validation_results BEGIN
    SELECT RAISE(ABORT, 'validation results are immutable');
END;
CREATE TRIGGER validation_results_cannot_be_deleted BEFORE DELETE ON validation_results BEGIN
    SELECT RAISE(ABORT, 'validation results cannot be deleted');
END;

PRAGMA user_version = 6;
