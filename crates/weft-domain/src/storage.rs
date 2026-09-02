use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::artifact::{PathOperation, is_sha256_digest, sha256_digest};
use crate::{
    AssignmentId, BaseState, CandidateId, CanonicalArtifact, ChangeError, ChangeId,
    MaterializationId, RepositoryId, RevisionId, WorkspaceId,
};

const SCHEMA_VERSION: i64 = 4;
static NEXT_TEMPORARY_OBJECT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct ContentStore {
    root: PathBuf,
}

impl ContentStore {
    /// # Errors
    ///
    /// Returns an error when the content-store directories cannot be created.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        fs::create_dir_all(root.join("blobs").join("sha256"))?;
        fs::create_dir_all(root.join("artifacts").join("sha256"))?;
        Ok(Self { root })
    }

    /// # Errors
    ///
    /// Returns an error when the blob cannot be durably written or verified.
    pub fn put_blob(&self, content: &[u8]) -> Result<String, StorageError> {
        let digest = sha256_digest(content);
        self.put_addressed("blobs", &digest, content)?;
        Ok(digest)
    }

    /// # Errors
    ///
    /// Returns an error for an invalid, missing, or mismatched digest.
    pub fn read_blob(&self, digest: &str) -> Result<Vec<u8>, StorageError> {
        self.read_addressed("blobs", digest)
    }

    /// # Errors
    ///
    /// Returns an error if any referenced blob is absent or cannot be verified.
    pub fn put_artifact(&self, artifact: &CanonicalArtifact) -> Result<(), StorageError> {
        for operation in artifact.tree_delta().operations() {
            if let PathOperation::Upsert { blob_digest, .. } = operation {
                self.read_blob(blob_digest)?;
            }
        }
        let bytes = artifact.canonical_bytes();
        self.put_addressed("artifacts", artifact.digest(), &bytes)
    }

    /// # Errors
    ///
    /// Returns an error for a missing, tampered, or malformed artifact.
    pub fn read_artifact(&self, digest: &str) -> Result<CanonicalArtifact, StorageError> {
        let bytes = self.read_addressed("artifacts", digest)?;
        CanonicalArtifact::from_canonical_bytes_with_digest(&bytes, digest)
            .map_err(StorageError::InvalidArtifact)
    }

    fn put_addressed(&self, kind: &str, digest: &str, content: &[u8]) -> Result<(), StorageError> {
        validate_digest(digest)?;
        if sha256_digest(content) != digest {
            return Err(StorageError::DigestMismatch(digest.to_owned()));
        }
        let path = self.address_path(kind, digest)?;
        let temporary = path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY_OBJECT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(mut file) => {
                file.write_all(content)?;
                file.sync_all()?;
                drop(file);
                match fs::hard_link(&temporary, &path) {
                    Ok(()) => {
                        fs::remove_file(temporary)?;
                        Ok(())
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        fs::remove_file(temporary)?;
                        let existing = self.read_addressed(kind, digest)?;
                        if existing == content {
                            Ok(())
                        } else {
                            Err(StorageError::DigestMismatch(digest.to_owned()))
                        }
                    }
                    Err(error) => {
                        let _ = fs::remove_file(temporary);
                        Err(StorageError::Io(error))
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(temporary);
                let existing = self.read_addressed(kind, digest)?;
                if existing == content {
                    Ok(())
                } else {
                    Err(StorageError::DigestMismatch(digest.to_owned()))
                }
            }
            Err(error) => Err(StorageError::Io(error)),
        }
    }

    fn read_addressed(&self, kind: &str, digest: &str) -> Result<Vec<u8>, StorageError> {
        validate_digest(digest)?;
        let path = self.address_path(kind, digest)?;
        let mut file = File::open(path)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;
        if sha256_digest(&content) != digest {
            return Err(StorageError::DigestMismatch(digest.to_owned()));
        }
        Ok(content)
    }

    fn address_path(&self, kind: &str, digest: &str) -> Result<PathBuf, StorageError> {
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or_else(|| StorageError::InvalidDigest(digest.to_owned()))?;
        Ok(self.root.join(kind).join("sha256").join(hex))
    }
}

fn validate_digest(digest: &str) -> Result<(), StorageError> {
    if is_sha256_digest(digest) {
        Ok(())
    } else {
        Err(StorageError::InvalidDigest(digest.to_owned()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredChange {
    id: ChangeId,
    head: Option<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lease {
    change_id: ChangeId,
    operation: String,
    holder: String,
    expires_at_unix_ms: i64,
}

impl Lease {
    #[must_use]
    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    #[must_use]
    pub fn holder(&self) -> &str {
        &self.holder
    }

    #[must_use]
    pub const fn expires_at_unix_ms(&self) -> i64 {
        self.expires_at_unix_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEvent {
    event_id: i64,
    change_id: ChangeId,
    kind: String,
    detail: String,
}

/// A durable record that a subject holds a role on a Change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Assignment {
    id: AssignmentId,
    change_id: ChangeId,
    subject: String,
    role: String,
    actor: String,
    assigned_at_unix_ms: i64,
}

impl Assignment {
    /// Creates a validated assignment event.
    ///
    /// # Errors
    ///
    /// Returns an error for blank, padded, control-character, or oversized values.
    pub fn new(
        assignment_id: AssignmentId,
        change_id: ChangeId,
        subject: impl Into<String>,
        role: impl Into<String>,
        actor: impl Into<String>,
        assigned_at_unix_ms: i64,
    ) -> Result<Self, StorageError> {
        Ok(Self {
            id: assignment_id,
            change_id,
            subject: valid_event_value(subject.into(), "subject")?,
            role: valid_event_value(role.into(), "role")?,
            actor: valid_event_value(actor.into(), "actor")?,
            assigned_at_unix_ms,
        })
    }

    #[must_use]
    pub fn assignment_id(&self) -> &AssignmentId {
        &self.id
    }
    #[must_use]
    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }
    #[must_use]
    pub fn actor(&self) -> &str {
        &self.actor
    }
    #[must_use]
    pub const fn assigned_at_unix_ms(&self) -> i64 {
        self.assigned_at_unix_ms
    }
}

/// The provider-facing lifecycle of a realization of one immutable revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaterializationState {
    Clean,
    Dirty,
    Diverged,
    Suspended,
    Released,
    Invalidated,
}

impl MaterializationState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::Diverged => "diverged",
            Self::Suspended => "suspended",
            Self::Released => "released",
            Self::Invalidated => "invalidated",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "clean" => Ok(Self::Clean),
            "dirty" => Ok(Self::Dirty),
            "diverged" => Ok(Self::Diverged),
            "suspended" => Ok(Self::Suspended),
            "released" => Ok(Self::Released),
            "invalidated" => Ok(Self::Invalidated),
            _ => Err(StorageError::Invariant("unknown materialization state")),
        }
    }

    const fn may_transition_to(self, next: Self) -> bool {
        match self {
            Self::Clean => matches!(
                next,
                Self::Dirty | Self::Diverged | Self::Suspended | Self::Released | Self::Invalidated
            ),
            Self::Dirty => matches!(
                next,
                Self::Diverged | Self::Suspended | Self::Released | Self::Invalidated
            ),
            Self::Diverged => matches!(next, Self::Suspended | Self::Released | Self::Invalidated),
            Self::Suspended => matches!(
                next,
                Self::Clean | Self::Dirty | Self::Diverged | Self::Released | Self::Invalidated
            ),
            Self::Released | Self::Invalidated => false,
        }
    }
}

/// Durable materialization metadata and its exact revision target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Materialization {
    id: MaterializationId,
    revision_id: RevisionId,
    workspace_id: WorkspaceId,
    provider: String,
    provider_ref: String,
    state: MaterializationState,
}

impl Materialization {
    #[must_use]
    pub fn materialization_id(&self) -> &MaterializationId {
        &self.id
    }
    #[must_use]
    pub fn revision_id(&self) -> &RevisionId {
        &self.revision_id
    }
    #[must_use]
    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }
    #[must_use]
    pub fn provider_ref(&self) -> &str {
        &self.provider_ref
    }
    #[must_use]
    pub const fn state(&self) -> MaterializationState {
        self.state
    }
}

/// An exact upstream revision required by a downstream Change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dependency {
    upstream_change: ChangeId,
    upstream_revision: RevisionId,
    downstream_change: ChangeId,
}

impl Dependency {
    #[must_use]
    pub const fn new(
        upstream_change_id: ChangeId,
        upstream_revision_id: RevisionId,
        downstream_change_id: ChangeId,
    ) -> Self {
        Self {
            upstream_change: upstream_change_id,
            upstream_revision: upstream_revision_id,
            downstream_change: downstream_change_id,
        }
    }

    #[must_use]
    pub fn upstream_change_id(&self) -> &ChangeId {
        &self.upstream_change
    }

    #[must_use]
    pub fn upstream_revision_id(&self) -> &RevisionId {
        &self.upstream_revision
    }

    #[must_use]
    pub fn downstream_change_id(&self) -> &ChangeId {
        &self.downstream_change
    }
}

/// One ordered, exact Change revision in an immutable composition candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateInput {
    change_id: ChangeId,
    revision_id: RevisionId,
}

impl CandidateInput {
    #[must_use]
    pub const fn new(change_id: ChangeId, revision_id: RevisionId) -> Self {
        Self {
            change_id,
            revision_id,
        }
    }

    #[must_use]
    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub fn revision_id(&self) -> &RevisionId {
        &self.revision_id
    }
}

/// A durable snapshot of ordered revisions and their dependency pins.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionCandidate {
    candidate_id: CandidateId,
    target_base: BaseState,
    inputs: Vec<CandidateInput>,
    resolved_dependencies: Vec<Dependency>,
    content_digest: String,
}

impl CompositionCandidate {
    #[must_use]
    pub fn candidate_id(&self) -> &CandidateId {
        &self.candidate_id
    }

    #[must_use]
    pub fn target_base(&self) -> &BaseState {
        &self.target_base
    }

    #[must_use]
    pub fn inputs(&self) -> &[CandidateInput] {
        &self.inputs
    }

    #[must_use]
    pub fn resolved_dependencies(&self) -> &[Dependency] {
        &self.resolved_dependencies
    }

    #[must_use]
    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }
}

impl AuditEvent {
    #[must_use]
    pub const fn event_id(&self) -> i64 {
        self.event_id
    }

    #[must_use]
    pub fn change_id(&self) -> &ChangeId {
        &self.change_id
    }

    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl StoredChange {
    #[must_use]
    pub fn id(&self) -> &ChangeId {
        &self.id
    }

    #[must_use]
    pub fn head(&self) -> Option<&RevisionId> {
        self.head.as_ref()
    }
}

pub struct SqliteRepository {
    connection: Connection,
    content_store: ContentStore,
}

impl SqliteRepository {
    /// # Errors
    ///
    /// Returns an error when `SQLite` cannot open, configure, or migrate the database.
    pub fn open(
        database_path: impl AsRef<Path>,
        content_store: ContentStore,
    ) -> Result<Self, StorageError> {
        let connection = Connection::open(database_path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS schema_migrations (
                 version INTEGER PRIMARY KEY
             );
             CREATE TABLE IF NOT EXISTS changes (
                 change_id TEXT PRIMARY KEY NOT NULL,
                 head_revision_id TEXT NULL
             );
             CREATE TABLE IF NOT EXISTS revisions (
                 revision_id TEXT PRIMARY KEY NOT NULL,
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 parent_revision_id TEXT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 repository_id TEXT NOT NULL,
                 base_object_id TEXT NOT NULL,
                 artifact_digest TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS leases (
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 operation TEXT NOT NULL,
                 holder TEXT NOT NULL,
                 expires_at_unix_ms INTEGER NOT NULL,
                 PRIMARY KEY(change_id, operation)
             );
             CREATE TABLE IF NOT EXISTS audit_events (
                 event_id INTEGER PRIMARY KEY AUTOINCREMENT,
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 kind TEXT NOT NULL,
                 detail TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS dependencies (
                 upstream_change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 upstream_revision_id TEXT NOT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 downstream_change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 PRIMARY KEY(upstream_change_id, downstream_change_id),
                 CHECK(upstream_change_id <> downstream_change_id)
             );
             CREATE TABLE IF NOT EXISTS candidates (
                 candidate_id TEXT PRIMARY KEY NOT NULL,
                 repository_id TEXT NOT NULL,
                 target_base_object_id TEXT NOT NULL,
                 content_digest TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS candidate_inputs (
                 candidate_id TEXT NOT NULL REFERENCES candidates(candidate_id) ON DELETE RESTRICT,
                 position INTEGER NOT NULL CHECK(position >= 0),
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 revision_id TEXT NOT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 PRIMARY KEY(candidate_id, position),
                 UNIQUE(candidate_id, change_id)
             );
             CREATE TABLE IF NOT EXISTS candidate_dependencies (
                 candidate_id TEXT NOT NULL REFERENCES candidates(candidate_id) ON DELETE RESTRICT,
                 upstream_change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 upstream_revision_id TEXT NOT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 downstream_change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 PRIMARY KEY(candidate_id, upstream_change_id, downstream_change_id)
             );
             CREATE TABLE IF NOT EXISTS assignments (
                 assignment_id TEXT PRIMARY KEY NOT NULL,
                 change_id TEXT NOT NULL REFERENCES changes(change_id) ON DELETE RESTRICT,
                 subject TEXT NOT NULL,
                 role TEXT NOT NULL,
                 actor TEXT NOT NULL,
                 assigned_at_unix_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS materializations (
                 materialization_id TEXT PRIMARY KEY NOT NULL,
                 revision_id TEXT NOT NULL REFERENCES revisions(revision_id) ON DELETE RESTRICT,
                 workspace_id TEXT NOT NULL,
                 provider TEXT NOT NULL,
                 provider_ref TEXT NOT NULL,
                 state TEXT NOT NULL,
                 version INTEGER NOT NULL
             );",
        )?;
        let stored_version: Option<i64> =
            connection.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })?;
        if stored_version.is_some_and(|version| version > SCHEMA_VERSION) {
            return Err(StorageError::UnsupportedSchemaVersion(
                stored_version.unwrap_or_default(),
            ));
        }
        connection.execute(
            "INSERT OR IGNORE INTO schema_migrations(version) VALUES (?1)",
            [SCHEMA_VERSION],
        )?;
        Ok(Self {
            connection,
            content_store,
        })
    }

    /// # Errors
    ///
    /// Returns an error if the Change identity already exists or `SQLite` fails.
    pub fn create_change(&mut self, change_id: ChangeId) -> Result<(), StorageError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO changes(change_id, head_revision_id) VALUES (?1, NULL)",
            [change_id.as_str()],
        )?;
        if changed == 0 {
            return Err(StorageError::DuplicateChange(change_id));
        }
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![change_id.as_str(), "change-created", change_id.as_str()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error if the Change is missing, corrupted, or `SQLite` fails.
    pub fn load_change(&self, change_id: &ChangeId) -> Result<StoredChange, StorageError> {
        self.connection
            .query_row(
                "SELECT head_revision_id FROM changes WHERE change_id = ?1",
                [change_id.as_str()],
                |row| {
                    let head: Option<String> = row.get(0)?;
                    let head = head.map(RevisionId::new).transpose().map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok(StoredChange {
                        id: change_id.clone(),
                        head,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingChange(change_id.clone()))
    }

    /// Appends exactly one successor when the persisted head matches.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing Change, stale head, duplicate revision,
    /// missing/tampered canonical content, or `SQLite` failure.
    pub fn append_revision(
        &mut self,
        change_id: &ChangeId,
        expected_head: Option<&RevisionId>,
        revision_id: RevisionId,
        artifact: &CanonicalArtifact,
    ) -> Result<(), StorageError> {
        self.content_store.put_artifact(artifact)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual: Option<String> = transaction
            .query_row(
                "SELECT head_revision_id FROM changes WHERE change_id = ?1",
                [change_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingChange(change_id.clone()))?;
        let expected = expected_head.map(RevisionId::as_str);
        if actual.as_deref() != expected {
            return Err(StorageError::StaleHead {
                expected: expected_head.cloned(),
                actual: actual
                    .map(RevisionId::new)
                    .transpose()
                    .map_err(StorageError::Domain)?,
            });
        }
        let duplicate = transaction
            .query_row(
                "SELECT 1 FROM revisions WHERE revision_id = ?1",
                [revision_id.as_str()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if duplicate {
            return Err(StorageError::DuplicateRevision(revision_id));
        }
        transaction.execute(
            "INSERT INTO revisions(
                revision_id, change_id, parent_revision_id, repository_id, base_object_id, artifact_digest
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                revision_id.as_str(),
                change_id.as_str(),
                expected,
                artifact.base().repository_id().as_str(),
                artifact.base().object_id(),
                artifact.digest(),
            ],
        )?;
        let changed = transaction.execute(
            "UPDATE changes SET head_revision_id = ?1
             WHERE change_id = ?2 AND head_revision_id IS ?3",
            params![revision_id.as_str(), change_id.as_str(), expected],
        )?;
        if changed != 1 {
            return Err(StorageError::Invariant(
                "head changed during immediate transaction",
            ));
        }
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![
                change_id.as_str(),
                "revision-appended",
                revision_id.as_str()
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Acquires an expired-or-unheld exclusive operation lease.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid inputs, a missing Change, or an active
    /// lease held by another actor.
    pub fn acquire_lease(
        &mut self,
        change_id: &ChangeId,
        operation: impl Into<String>,
        holder: impl Into<String>,
        now_unix_ms: i64,
        expires_at_unix_ms: i64,
    ) -> Result<Lease, StorageError> {
        let operation = valid_lease_value(operation.into(), "operation")?;
        let holder = valid_lease_value(holder.into(), "holder")?;
        if expires_at_unix_ms <= now_unix_ms {
            return Err(StorageError::InvalidLeaseExpiry);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists = transaction
            .query_row(
                "SELECT 1 FROM changes WHERE change_id = ?1",
                [change_id.as_str()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(StorageError::MissingChange(change_id.clone()));
        }
        let existing: Option<(String, i64)> = transaction
            .query_row(
                "SELECT holder, expires_at_unix_ms FROM leases WHERE change_id = ?1 AND operation = ?2",
                params![change_id.as_str(), operation],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((existing_holder, expiry)) = existing
            && expiry > now_unix_ms
        {
            return Err(StorageError::LeaseHeld {
                holder: existing_holder,
                expires_at_unix_ms: expiry,
            });
        }
        transaction.execute(
            "INSERT INTO leases(change_id, operation, holder, expires_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(change_id, operation) DO UPDATE SET
                 holder = excluded.holder,
                 expires_at_unix_ms = excluded.expires_at_unix_ms",
            params![change_id.as_str(), operation, holder, expires_at_unix_ms],
        )?;
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![change_id.as_str(), "lease-acquired", operation],
        )?;
        transaction.commit()?;
        Ok(Lease {
            change_id: change_id.clone(),
            operation,
            holder,
            expires_at_unix_ms,
        })
    }

    /// Returns durable events in creation order for a Change.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing Change or `SQLite` failure.
    pub fn audit_events(&self, change_id: &ChangeId) -> Result<Vec<AuditEvent>, StorageError> {
        self.load_change(change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT event_id, kind, detail FROM audit_events
             WHERE change_id = ?1 ORDER BY event_id",
        )?;
        let rows = statement.query_map([change_id.as_str()], |row| {
            Ok(AuditEvent {
                event_id: row.get(0)?,
                change_id: change_id.clone(),
                kind: row.get(1)?,
                detail: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Sqlite)
    }

    /// # Errors
    ///
    /// Returns an error if the revision is missing or its artifact cannot be verified.
    pub fn load_artifact_for_revision(
        &self,
        revision_id: &RevisionId,
    ) -> Result<CanonicalArtifact, StorageError> {
        let digest: String = self
            .connection
            .query_row(
                "SELECT artifact_digest FROM revisions WHERE revision_id = ?1",
                [revision_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingRevision(revision_id.clone()))?;
        self.content_store.read_artifact(&digest)
    }

    /// Adds a durable exact-revision dependency after atomically rejecting a cycle.
    ///
    /// # Errors
    ///
    /// Returns an error if an endpoint or pinned revision is missing, the pin does
    /// not belong to its declared upstream Change, the edge already exists, or it
    /// would create a dependency cycle.
    pub fn add_dependency(&mut self, dependency: &Dependency) -> Result<(), StorageError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_change_exists(&transaction, dependency.upstream_change_id())?;
        ensure_change_exists(&transaction, dependency.downstream_change_id())?;
        let revision_change: String = transaction
            .query_row(
                "SELECT change_id FROM revisions WHERE revision_id = ?1",
                [dependency.upstream_revision_id().as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                StorageError::MissingRevision(dependency.upstream_revision_id().clone())
            })?;
        if revision_change != dependency.upstream_change_id().as_str() {
            return Err(StorageError::RevisionDoesNotBelongToChange {
                revision_id: dependency.upstream_revision_id().clone(),
                change_id: dependency.upstream_change_id().clone(),
            });
        }
        let creates_cycle: bool = transaction.query_row(
            "WITH RECURSIVE reachable(change_id) AS (
                 SELECT downstream_change_id FROM dependencies WHERE upstream_change_id = ?1
                 UNION
                 SELECT dependencies.downstream_change_id
                 FROM dependencies JOIN reachable ON dependencies.upstream_change_id = reachable.change_id
             )
             SELECT EXISTS(SELECT 1 FROM reachable WHERE change_id = ?2)",
            params![dependency.downstream_change_id().as_str(), dependency.upstream_change_id().as_str()],
            |row| row.get(0),
        )?;
        if creates_cycle {
            return Err(StorageError::DependencyCycle);
        }
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO dependencies(
                upstream_change_id, upstream_revision_id, downstream_change_id
             ) VALUES (?1, ?2, ?3)",
            params![
                dependency.upstream_change_id().as_str(),
                dependency.upstream_revision_id().as_str(),
                dependency.downstream_change_id().as_str(),
            ],
        )?;
        if changed == 0 {
            return Err(StorageError::DuplicateDependency);
        }
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![
                dependency.downstream_change_id().as_str(),
                "dependency-added",
                format!(
                    "{}@{}",
                    dependency.upstream_change_id().as_str(),
                    dependency.upstream_revision_id().as_str()
                ),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Returns the current exact dependency contracts for one downstream Change.
    ///
    /// # Errors
    ///
    /// Returns an error if the Change is missing or persisted identifiers are invalid.
    pub fn dependencies_for(
        &self,
        downstream_change_id: &ChangeId,
    ) -> Result<Vec<Dependency>, StorageError> {
        self.load_change(downstream_change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT upstream_change_id, upstream_revision_id FROM dependencies
             WHERE downstream_change_id = ?1 ORDER BY upstream_change_id",
        )?;
        let rows = statement.query_map([downstream_change_id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (change, revision) = row?;
            Ok(Dependency::new(
                ChangeId::new(change).map_err(StorageError::Domain)?,
                RevisionId::new(revision).map_err(StorageError::Domain)?,
                downstream_change_id.clone(),
            ))
        })
        .collect()
    }

    /// Creates an immutable candidate from ordered exact revision inputs.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate/empty inputs, mismatched repository or
    /// revision ownership, unresolved dependency pins, or duplicate candidate ID.
    pub fn create_candidate(
        &mut self,
        candidate_id: CandidateId,
        target_base: BaseState,
        inputs: Vec<CandidateInput>,
    ) -> Result<CompositionCandidate, StorageError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let resolved_dependencies = validate_candidate_inputs(&transaction, &target_base, &inputs)?;
        let content_digest = candidate_digest(&target_base, &inputs, &resolved_dependencies);
        let inserted_candidate = transaction.execute(
            "INSERT OR IGNORE INTO candidates(
                candidate_id, repository_id, target_base_object_id, content_digest
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                candidate_id.as_str(),
                target_base.repository_id().as_str(),
                target_base.object_id(),
                content_digest,
            ],
        )?;
        if inserted_candidate == 0 {
            return Err(StorageError::DuplicateCandidate(candidate_id));
        }
        for (position, input) in inputs.iter().enumerate() {
            transaction.execute(
                "INSERT INTO candidate_inputs(candidate_id, position, change_id, revision_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    candidate_id.as_str(),
                    position,
                    input.change_id().as_str(),
                    input.revision_id().as_str()
                ],
            )?;
            transaction.execute(
                "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
                params![
                    input.change_id().as_str(),
                    "candidate-created",
                    candidate_id.as_str()
                ],
            )?;
        }
        for dependency in &resolved_dependencies {
            transaction.execute(
                "INSERT INTO candidate_dependencies(
                    candidate_id, upstream_change_id, upstream_revision_id, downstream_change_id
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    candidate_id.as_str(),
                    dependency.upstream_change_id().as_str(),
                    dependency.upstream_revision_id().as_str(),
                    dependency.downstream_change_id().as_str(),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(CompositionCandidate {
            candidate_id,
            target_base,
            inputs,
            resolved_dependencies,
            content_digest,
        })
    }

    /// Loads a candidate's immutable snapshot. It never resolves current heads.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing or malformed persisted candidate.
    pub fn load_candidate(
        &self,
        candidate_id: &CandidateId,
    ) -> Result<CompositionCandidate, StorageError> {
        let (repository_id, object_id, content_digest): (String, String, String) = self
            .connection
            .query_row(
                "SELECT repository_id, target_base_object_id, content_digest
                 FROM candidates WHERE candidate_id = ?1",
                [candidate_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingCandidate(candidate_id.clone()))?;
        let target_base = BaseState::new(RepositoryId::new(repository_id)?, object_id)?;
        let inputs = candidate_inputs(&self.connection, candidate_id)?;
        let dependencies = candidate_dependencies(&self.connection, candidate_id)?;
        if candidate_digest(&target_base, &inputs, &dependencies) != content_digest {
            return Err(StorageError::Invariant("candidate content digest mismatch"));
        }
        Ok(CompositionCandidate {
            candidate_id: candidate_id.clone(),
            target_base,
            inputs,
            resolved_dependencies: dependencies,
            content_digest,
        })
    }

    /// Reports whether any candidate input or dependency pin has advanced.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing or corrupt candidate.
    pub fn candidate_is_stale(&self, candidate_id: &CandidateId) -> Result<bool, StorageError> {
        let candidate = self.load_candidate(candidate_id)?;
        for input in candidate.inputs() {
            let head = self.load_change(input.change_id())?.head().cloned();
            if head.as_ref() != Some(input.revision_id()) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Records an immutable assignment event for an existing Change.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing Change, duplicate assignment identity, or storage failure.
    pub fn record_assignment(&mut self, assignment: &Assignment) -> Result<(), StorageError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_change_exists(&transaction, assignment.change_id())?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO assignments(
                assignment_id, change_id, subject, role, actor, assigned_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                assignment.assignment_id().as_str(),
                assignment.change_id().as_str(),
                assignment.subject(),
                assignment.role(),
                assignment.actor(),
                assignment.assigned_at_unix_ms(),
            ],
        )?;
        if inserted == 0 {
            return Err(StorageError::DuplicateAssignment(
                assignment.assignment_id().clone(),
            ));
        }
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![
                assignment.change_id().as_str(),
                "assignment-recorded",
                assignment.assignment_id().as_str()
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Returns assignment history in its durable event order.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing Change or malformed persisted assignment.
    pub fn assignments_for(&self, change_id: &ChangeId) -> Result<Vec<Assignment>, StorageError> {
        self.load_change(change_id)?;
        let mut statement = self.connection.prepare(
            "SELECT assignment_id, subject, role, actor, assigned_at_unix_ms FROM assignments
             WHERE change_id = ?1 ORDER BY assigned_at_unix_ms, assignment_id",
        )?;
        let rows = statement.query_map([change_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (id, subject, role, actor, timestamp) = row?;
            Assignment::new(
                AssignmentId::new(id)?,
                change_id.clone(),
                subject,
                role,
                actor,
                timestamp,
            )
        })
        .collect()
    }

    /// Creates a clean materialization for one exact revision.
    ///
    /// # Errors
    ///
    /// Returns an error if the revision is absent, identity is duplicated, or provider metadata is invalid.
    pub fn create_materialization(
        &mut self,
        materialization_id: MaterializationId,
        revision_id: RevisionId,
        workspace_id: WorkspaceId,
        provider: impl Into<String>,
        provider_ref: impl Into<String>,
    ) -> Result<Materialization, StorageError> {
        let provider = valid_event_value(provider.into(), "provider")?;
        let provider_ref = valid_event_value(provider_ref.into(), "provider reference")?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let change_id: String = transaction
            .query_row(
                "SELECT change_id FROM revisions WHERE revision_id = ?1",
                [revision_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::MissingRevision(revision_id.clone()))?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO materializations(
                materialization_id, revision_id, workspace_id, provider, provider_ref, state, version
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'clean', 0)",
            params![materialization_id.as_str(), revision_id.as_str(), workspace_id.as_str(), provider, provider_ref],
        )?;
        if inserted == 0 {
            return Err(StorageError::DuplicateMaterialization(materialization_id));
        }
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![
                change_id,
                "materialization-created",
                materialization_id.as_str()
            ],
        )?;
        transaction.commit()?;
        Ok(Materialization {
            id: materialization_id,
            revision_id,
            workspace_id,
            provider,
            provider_ref,
            state: MaterializationState::Clean,
        })
    }

    /// Loads a materialization without consulting provider state.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing or malformed persisted materialization.
    pub fn load_materialization(
        &self,
        materialization_id: &MaterializationId,
    ) -> Result<Materialization, StorageError> {
        let row: Option<(String, String, String, String, String)> = self.connection.query_row(
            "SELECT revision_id, workspace_id, provider, provider_ref, state FROM materializations WHERE materialization_id = ?1",
            [materialization_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).optional()?;
        let (revision_id, workspace_id, provider, provider_ref, state) =
            row.ok_or_else(|| StorageError::MissingMaterialization(materialization_id.clone()))?;
        Ok(Materialization {
            id: materialization_id.clone(),
            revision_id: RevisionId::new(revision_id)?,
            workspace_id: WorkspaceId::new(workspace_id)?,
            provider,
            provider_ref,
            state: MaterializationState::parse(&state)?,
        })
    }

    /// Transitions a materialization only from the caller's expected state.
    ///
    /// # Errors
    ///
    /// Returns a stale-state error rather than overwriting concurrent provider observations.
    pub fn transition_materialization(
        &mut self,
        materialization_id: &MaterializationId,
        expected: MaterializationState,
        next: MaterializationState,
    ) -> Result<Materialization, StorageError> {
        if !expected.may_transition_to(next) {
            return Err(StorageError::InvalidMaterializationTransition { expected, next });
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, String, String, String, String)> = transaction.query_row(
            "SELECT revision_id, workspace_id, provider, provider_ref, state FROM materializations WHERE materialization_id = ?1",
            [materialization_id.as_str()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).optional()?;
        let (revision, workspace, provider, provider_ref, actual) =
            row.ok_or_else(|| StorageError::MissingMaterialization(materialization_id.clone()))?;
        let actual = MaterializationState::parse(&actual)?;
        if actual != expected {
            return Err(StorageError::StaleMaterializationState { expected, actual });
        }
        let updated = transaction.execute(
            "UPDATE materializations SET state = ?1, version = version + 1 WHERE materialization_id = ?2 AND state = ?3",
            params![next.as_str(), materialization_id.as_str(), expected.as_str()],
        )?;
        if updated != 1 {
            return Err(StorageError::Invariant(
                "materialization changed during immediate transaction",
            ));
        }
        let change_id: String = transaction.query_row(
            "SELECT change_id FROM revisions WHERE revision_id = ?1",
            [revision.as_str()],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO audit_events(change_id, kind, detail) VALUES (?1, ?2, ?3)",
            params![
                change_id,
                "materialization-transitioned",
                format!("{}:{}", materialization_id.as_str(), next.as_str())
            ],
        )?;
        transaction.commit()?;
        Ok(Materialization {
            id: materialization_id.clone(),
            revision_id: RevisionId::new(revision)?,
            workspace_id: WorkspaceId::new(workspace)?,
            provider,
            provider_ref,
            state: next,
        })
    }
}

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Domain(ChangeError),
    InvalidArtifact(ChangeError),
    InvalidDigest(String),
    DigestMismatch(String),
    DuplicateChange(ChangeId),
    DuplicateRevision(RevisionId),
    DuplicateCandidate(CandidateId),
    DuplicateAssignment(AssignmentId),
    DuplicateMaterialization(MaterializationId),
    DuplicateCandidateInput(ChangeId),
    DuplicateDependency,
    MissingChange(ChangeId),
    MissingRevision(RevisionId),
    MissingCandidate(CandidateId),
    MissingMaterialization(MaterializationId),
    EmptyCandidate,
    RevisionDoesNotBelongToChange {
        revision_id: RevisionId,
        change_id: ChangeId,
    },
    CandidateRepositoryMismatch {
        revision_id: RevisionId,
        expected: RepositoryId,
    },
    UnresolvedDependency(Dependency),
    CandidateDependencyOrder(Dependency),
    InvalidMaterializationTransition {
        expected: MaterializationState,
        next: MaterializationState,
    },
    StaleMaterializationState {
        expected: MaterializationState,
        actual: MaterializationState,
    },
    DependencyCycle,
    UnsupportedSchemaVersion(i64),
    InvalidLeaseValue(&'static str),
    InvalidLeaseExpiry,
    LeaseHeld {
        holder: String,
        expires_at_unix_ms: i64,
    },
    StaleHead {
        expected: Option<RevisionId>,
        actual: Option<RevisionId>,
    },
    Invariant(&'static str),
}

impl Display for StorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "storage I/O failure: {error}"),
            Self::Sqlite(error) => write!(formatter, "SQLite failure: {error}"),
            Self::Domain(error) | Self::InvalidArtifact(error) => {
                write!(formatter, "invalid domain state: {error}")
            }
            Self::InvalidDigest(digest) => write!(formatter, "invalid content digest: {digest}"),
            Self::DigestMismatch(digest) => write!(formatter, "content digest mismatch: {digest}"),
            Self::DuplicateChange(id) => write!(formatter, "duplicate change: {}", id.as_str()),
            Self::DuplicateRevision(id) => write!(formatter, "duplicate revision: {}", id.as_str()),
            Self::DuplicateCandidate(id) => {
                write!(formatter, "duplicate candidate: {}", id.as_str())
            }
            Self::DuplicateAssignment(id) => {
                write!(formatter, "duplicate assignment: {}", id.as_str())
            }
            Self::DuplicateMaterialization(id) => {
                write!(formatter, "duplicate materialization: {}", id.as_str())
            }
            Self::DuplicateCandidateInput(id) => {
                write!(
                    formatter,
                    "duplicate candidate input change: {}",
                    id.as_str()
                )
            }
            Self::DuplicateDependency => formatter.write_str("duplicate dependency"),
            Self::MissingChange(id) => write!(formatter, "missing change: {}", id.as_str()),
            Self::MissingRevision(id) => write!(formatter, "missing revision: {}", id.as_str()),
            Self::MissingCandidate(id) => write!(formatter, "missing candidate: {}", id.as_str()),
            Self::MissingMaterialization(id) => {
                write!(formatter, "missing materialization: {}", id.as_str())
            }
            Self::EmptyCandidate => formatter.write_str("candidate requires at least one input"),
            Self::RevisionDoesNotBelongToChange {
                revision_id,
                change_id,
            } => write!(
                formatter,
                "revision {} does not belong to change {}",
                revision_id.as_str(),
                change_id.as_str()
            ),
            Self::CandidateRepositoryMismatch {
                revision_id,
                expected,
            } => write!(
                formatter,
                "revision {} is not in candidate repository {}",
                revision_id.as_str(),
                expected.as_str()
            ),
            Self::UnresolvedDependency(dependency) => write!(
                formatter,
                "candidate lacks required dependency {}@{} for {}",
                dependency.upstream_change_id().as_str(),
                dependency.upstream_revision_id().as_str(),
                dependency.downstream_change_id().as_str()
            ),
            Self::CandidateDependencyOrder(dependency) => write!(
                formatter,
                "candidate orders dependent {} before required upstream {}",
                dependency.downstream_change_id().as_str(),
                dependency.upstream_change_id().as_str()
            ),
            Self::InvalidMaterializationTransition { expected, next } => write!(
                formatter,
                "invalid materialization transition from {} to {}",
                expected.as_str(),
                next.as_str()
            ),
            Self::StaleMaterializationState { expected, actual } => write!(
                formatter,
                "stale materialization state: expected {}, actual {}",
                expected.as_str(),
                actual.as_str()
            ),
            Self::DependencyCycle => formatter.write_str("dependency would create a cycle"),
            Self::UnsupportedSchemaVersion(version) => write!(
                formatter,
                "database schema version {version} is newer than this Weft build"
            ),
            Self::InvalidLeaseValue(kind) => write!(formatter, "invalid lease {kind}"),
            Self::InvalidLeaseExpiry => formatter.write_str("lease expiry must be in the future"),
            Self::LeaseHeld {
                holder,
                expires_at_unix_ms,
            } => write!(
                formatter,
                "lease held by {holder} until {expires_at_unix_ms}"
            ),
            Self::StaleHead { expected, actual } => write!(
                formatter,
                "stale revision head: expected {:?}, actual {:?}",
                expected.as_ref().map(RevisionId::as_str),
                actual.as_ref().map(RevisionId::as_str)
            ),
            Self::Invariant(message) => write!(formatter, "storage invariant failed: {message}"),
        }
    }
}

impl Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<ChangeError> for StorageError {
    fn from(error: ChangeError) -> Self {
        Self::Domain(error)
    }
}

fn ensure_change_exists(
    transaction: &rusqlite::Transaction<'_>,
    change_id: &ChangeId,
) -> Result<(), StorageError> {
    let exists = transaction
        .query_row(
            "SELECT 1 FROM changes WHERE change_id = ?1",
            [change_id.as_str()],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(StorageError::MissingChange(change_id.clone()))
    }
}

fn dependencies_for_inputs(
    transaction: &rusqlite::Transaction<'_>,
    inputs: &[CandidateInput],
) -> Result<Vec<Dependency>, StorageError> {
    let mut dependencies = Vec::new();
    for input in inputs {
        let mut statement = transaction.prepare(
            "SELECT upstream_change_id, upstream_revision_id FROM dependencies
             WHERE downstream_change_id = ?1 ORDER BY upstream_change_id",
        )?;
        let rows = statement.query_map([input.change_id().as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (upstream_change, upstream_revision) = row?;
            dependencies.push(Dependency::new(
                ChangeId::new(upstream_change)?,
                RevisionId::new(upstream_revision)?,
                input.change_id().clone(),
            ));
        }
    }
    Ok(dependencies)
}

fn validate_candidate_inputs(
    transaction: &rusqlite::Transaction<'_>,
    target_base: &BaseState,
    inputs: &[CandidateInput],
) -> Result<Vec<Dependency>, StorageError> {
    if inputs.is_empty() {
        return Err(StorageError::EmptyCandidate);
    }
    let mut candidate_changes = HashSet::new();
    for input in inputs {
        if !candidate_changes.insert(input.change_id().as_str()) {
            return Err(StorageError::DuplicateCandidateInput(
                input.change_id().clone(),
            ));
        }
        let row: Option<(String, String)> = transaction
            .query_row(
                "SELECT change_id, repository_id FROM revisions WHERE revision_id = ?1",
                [input.revision_id().as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (revision_change, repository_id) =
            row.ok_or_else(|| StorageError::MissingRevision(input.revision_id().clone()))?;
        if revision_change != input.change_id().as_str() {
            return Err(StorageError::RevisionDoesNotBelongToChange {
                revision_id: input.revision_id().clone(),
                change_id: input.change_id().clone(),
            });
        }
        if repository_id != target_base.repository_id().as_str() {
            return Err(StorageError::CandidateRepositoryMismatch {
                revision_id: input.revision_id().clone(),
                expected: target_base.repository_id().clone(),
            });
        }
    }
    let dependencies = dependencies_for_inputs(transaction, inputs)?;
    let positions: HashMap<_, _> = inputs
        .iter()
        .enumerate()
        .map(|(position, input)| (input.change_id(), position))
        .collect();
    for dependency in &dependencies {
        let Some(&upstream) = positions.get(dependency.upstream_change_id()) else {
            return Err(StorageError::UnresolvedDependency(dependency.clone()));
        };
        let Some(&downstream) = positions.get(dependency.downstream_change_id()) else {
            return Err(StorageError::UnresolvedDependency(dependency.clone()));
        };
        if inputs[upstream].revision_id() != dependency.upstream_revision_id() {
            return Err(StorageError::UnresolvedDependency(dependency.clone()));
        }
        if upstream >= downstream {
            return Err(StorageError::CandidateDependencyOrder(dependency.clone()));
        }
    }
    Ok(dependencies)
}

fn candidate_inputs(
    connection: &Connection,
    candidate_id: &CandidateId,
) -> Result<Vec<CandidateInput>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT change_id, revision_id FROM candidate_inputs
         WHERE candidate_id = ?1 ORDER BY position",
    )?;
    let rows = statement.query_map([candidate_id.as_str()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (change, revision) = row?;
        Ok(CandidateInput::new(
            ChangeId::new(change)?,
            RevisionId::new(revision)?,
        ))
    })
    .collect()
}

fn candidate_dependencies(
    connection: &Connection,
    candidate_id: &CandidateId,
) -> Result<Vec<Dependency>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT upstream_change_id, upstream_revision_id, downstream_change_id
         FROM candidate_dependencies WHERE candidate_id = ?1
         ORDER BY downstream_change_id, upstream_change_id",
    )?;
    let rows = statement.query_map([candidate_id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (upstream_change, upstream_revision, downstream_change) = row?;
        Ok(Dependency::new(
            ChangeId::new(upstream_change)?,
            RevisionId::new(upstream_revision)?,
            ChangeId::new(downstream_change)?,
        ))
    })
    .collect()
}

fn candidate_digest(
    target_base: &BaseState,
    inputs: &[CandidateInput],
    dependencies: &[Dependency],
) -> String {
    let mut bytes = b"weft/composition-candidate-v1\0".to_vec();
    write_candidate_string(&mut bytes, target_base.repository_id().as_str());
    write_candidate_string(&mut bytes, target_base.object_id());
    write_candidate_count(&mut bytes, inputs.len());
    for input in inputs {
        write_candidate_string(&mut bytes, input.change_id().as_str());
        write_candidate_string(&mut bytes, input.revision_id().as_str());
    }
    let mut ordered_dependencies: Vec<_> = dependencies.iter().collect();
    ordered_dependencies.sort_by(|left, right| {
        (
            left.downstream_change_id().as_str(),
            left.upstream_change_id().as_str(),
            left.upstream_revision_id().as_str(),
        )
            .cmp(&(
                right.downstream_change_id().as_str(),
                right.upstream_change_id().as_str(),
                right.upstream_revision_id().as_str(),
            ))
    });
    write_candidate_count(&mut bytes, ordered_dependencies.len());
    for dependency in ordered_dependencies {
        write_candidate_string(&mut bytes, dependency.upstream_change_id().as_str());
        write_candidate_string(&mut bytes, dependency.upstream_revision_id().as_str());
        write_candidate_string(&mut bytes, dependency.downstream_change_id().as_str());
    }
    sha256_digest(&bytes)
}

fn write_candidate_string(bytes: &mut Vec<u8>, value: &str) {
    write_candidate_count(bytes, value.len());
    bytes.extend_from_slice(value.as_bytes());
}

fn write_candidate_count(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend_from_slice(&(value as u64).to_be_bytes());
}

fn valid_lease_value(value: String, kind: &'static str) -> Result<String, StorageError> {
    valid_event_value(value, kind)
}

fn valid_event_value(value: String, kind: &'static str) -> Result<String, StorageError> {
    if value.trim().is_empty() || value != value.trim() || value.chars().any(char::is_control) {
        return Err(StorageError::InvalidLeaseValue(kind));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;
    use crate::{BaseState, FileMode, PathOperation, RepositoryId, TreeDelta};

    static NEXT_TEMPORARY_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory() -> PathBuf {
        let sequence = NEXT_TEMPORARY_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weft-domain-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn artifact(store: &ContentStore) -> CanonicalArtifact {
        let binary = store.put_blob(b"\0binary\xff").unwrap();
        let symlink = store.put_blob(b"target").unwrap();
        CanonicalArtifact::new(
            BaseState::new(RepositoryId::new("repo-1").unwrap(), "git:deadbeef").unwrap(),
            TreeDelta::new(vec![
                PathOperation::Upsert {
                    path: "bin/tool".to_owned(),
                    mode: FileMode::Executable,
                    blob_digest: binary,
                },
                PathOperation::Delete {
                    path: "old.txt".to_owned(),
                },
                PathOperation::Upsert {
                    path: "src/link".to_owned(),
                    mode: FileMode::SymbolicLink,
                    blob_digest: symlink,
                },
            ])
            .unwrap(),
        )
    }

    #[test]
    fn persists_and_reopens_verified_canonical_artifacts() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let database = root.join("weft.sqlite");
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, revision.clone(), &artifact)
                .unwrap();
        }
        let repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(
            repository.load_change(&change).unwrap().head(),
            Some(&revision)
        );
        assert_eq!(
            repository.load_artifact_for_revision(&revision).unwrap(),
            artifact
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_tampered_content_addressed_artifacts() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        store.put_artifact(&artifact).unwrap();
        let digest = artifact.digest().strip_prefix("sha256:").unwrap();
        let artifact_path = store.root.join("artifacts").join("sha256").join(digest);
        fs::write(artifact_path, b"tampered").unwrap();
        assert!(matches!(
            store.read_artifact(artifact.digest()),
            Err(StorageError::DigestMismatch(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_to_persist_an_artifact_with_a_missing_blob() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let missing = sha256_digest(b"missing");
        let artifact = CanonicalArtifact::new(
            BaseState::new(RepositoryId::new("repo-1").unwrap(), "git:deadbeef").unwrap(),
            TreeDelta::new(vec![PathOperation::Upsert {
                path: "missing".to_owned(),
                mode: FileMode::Regular,
                blob_digest: missing,
            }])
            .unwrap(),
        );
        assert!(matches!(
            store.put_artifact(&artifact),
            Err(StorageError::Io(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_a_database_created_by_a_newer_build() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        {
            let repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository
                .connection
                .execute(
                    "INSERT INTO schema_migrations(version) VALUES (?1)",
                    [SCHEMA_VERSION + 1],
                )
                .unwrap();
        }
        assert!(matches!(
            SqliteRepository::open(database, store),
            Err(StorageError::UnsupportedSchemaVersion(version)) if version == SCHEMA_VERSION + 1
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn records_atomic_history_and_recovers_expired_leases() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let database = root.join("weft.sqlite");
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        let mut repository = SqliteRepository::open(&database, store).unwrap();
        repository.create_change(change.clone()).unwrap();
        repository
            .append_revision(&change, None, revision, &artifact)
            .unwrap();
        let lease = repository
            .acquire_lease(&change, "integrate", "agent-a", 100, 200)
            .unwrap();
        assert_eq!(lease.holder(), "agent-a");
        assert!(matches!(
            repository.acquire_lease(&change, "integrate", "agent-b", 150, 300),
            Err(StorageError::LeaseHeld { .. })
        ));
        let recovered = repository
            .acquire_lease(&change, "integrate", "agent-b", 200, 300)
            .unwrap();
        assert_eq!(recovered.holder(), "agent-b");
        let kinds: Vec<_> = repository
            .audit_events(&change)
            .unwrap()
            .into_iter()
            .map(|event| event.kind().to_owned())
            .collect();
        assert_eq!(
            kinds,
            vec![
                "change-created",
                "revision-appended",
                "lease-acquired",
                "lease-acquired"
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_competing_successors_from_independent_connections() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let change = ChangeId::new("change-1").unwrap();
        let root_revision = RevisionId::new("revision-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, root_revision.clone(), &artifact)
                .unwrap();
        }
        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for revision in ["revision-2a", "revision-2b"] {
            let database = database.clone();
            let store = store.clone();
            let change = change.clone();
            let expected = root_revision.clone();
            let artifact = artifact.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                let mut repository = SqliteRepository::open(database, store).unwrap();
                barrier.wait();
                repository.append_revision(
                    &change,
                    Some(&expected),
                    RevisionId::new(revision).unwrap(),
                    &artifact,
                )
            }));
        }
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(StorageError::StaleHead { .. })))
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_exact_dependency_candidate_and_marks_it_stale_after_an_advance() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let upstream = ChangeId::new("upstream").unwrap();
        let downstream = ChangeId::new("downstream").unwrap();
        let upstream_revision = RevisionId::new("upstream-r1").unwrap();
        let downstream_revision = RevisionId::new("downstream-r1").unwrap();
        let candidate_id = CandidateId::new("candidate-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(upstream.clone()).unwrap();
            repository.create_change(downstream.clone()).unwrap();
            repository
                .append_revision(&upstream, None, upstream_revision.clone(), &artifact)
                .unwrap();
            repository
                .append_revision(&downstream, None, downstream_revision.clone(), &artifact)
                .unwrap();
            repository
                .add_dependency(&Dependency::new(
                    upstream.clone(),
                    upstream_revision.clone(),
                    downstream.clone(),
                ))
                .unwrap();
            let candidate = repository
                .create_candidate(
                    candidate_id.clone(),
                    artifact.base().clone(),
                    vec![
                        CandidateInput::new(upstream.clone(), upstream_revision.clone()),
                        CandidateInput::new(downstream.clone(), downstream_revision.clone()),
                    ],
                )
                .unwrap();
            assert_eq!(candidate.resolved_dependencies().len(), 1);
            assert!(!repository.candidate_is_stale(&candidate_id).unwrap());
        }
        let mut repository = SqliteRepository::open(&database, store).unwrap();
        let persisted = repository.load_candidate(&candidate_id).unwrap();
        assert_eq!(persisted.inputs()[0].revision_id(), &upstream_revision);
        assert_eq!(persisted.inputs()[1].revision_id(), &downstream_revision);
        repository
            .append_revision(
                &upstream,
                Some(&upstream_revision),
                RevisionId::new("upstream-r2").unwrap(),
                &artifact,
            )
            .unwrap();
        assert!(repository.candidate_is_stale(&candidate_id).unwrap());
        assert_eq!(
            repository.load_candidate(&candidate_id).unwrap().inputs()[0].revision_id(),
            &upstream_revision
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_dependency_cycles_and_candidates_without_exact_pins() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let mut repository = SqliteRepository::open(root.join("weft.sqlite"), store).unwrap();
        let changes: Vec<_> = ["a", "b", "c"]
            .into_iter()
            .map(|id| ChangeId::new(id).unwrap())
            .collect();
        let revisions: Vec<_> = ["a-r1", "b-r1", "c-r1"]
            .into_iter()
            .map(|id| RevisionId::new(id).unwrap())
            .collect();
        for (change, revision) in changes.iter().zip(&revisions) {
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(change, None, revision.clone(), &artifact)
                .unwrap();
        }
        repository
            .add_dependency(&Dependency::new(
                changes[0].clone(),
                revisions[0].clone(),
                changes[1].clone(),
            ))
            .unwrap();
        repository
            .add_dependency(&Dependency::new(
                changes[1].clone(),
                revisions[1].clone(),
                changes[2].clone(),
            ))
            .unwrap();
        assert!(matches!(
            repository.add_dependency(&Dependency::new(
                changes[2].clone(),
                revisions[2].clone(),
                changes[0].clone(),
            )),
            Err(StorageError::DependencyCycle)
        ));
        assert!(matches!(
            repository.create_candidate(
                CandidateId::new("candidate-missing-pin").unwrap(),
                artifact.base().clone(),
                vec![CandidateInput::new(
                    changes[1].clone(),
                    revisions[1].clone()
                )],
            ),
            Err(StorageError::UnresolvedDependency(_))
        ));
        assert!(matches!(
            repository.create_candidate(
                CandidateId::new("candidate-wrong-order").unwrap(),
                artifact.base().clone(),
                vec![
                    CandidateInput::new(changes[1].clone(), revisions[1].clone()),
                    CandidateInput::new(changes[0].clone(), revisions[0].clone()),
                ],
            ),
            Err(StorageError::CandidateDependencyOrder(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn candidate_order_is_immutable_and_changes_its_digest() {
        let root = temporary_directory();
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let mut repository = SqliteRepository::open(root.join("weft.sqlite"), store).unwrap();
        let first_change = ChangeId::new("first").unwrap();
        let second_change = ChangeId::new("second").unwrap();
        let first_revision = RevisionId::new("first-r1").unwrap();
        let second_revision = RevisionId::new("second-r1").unwrap();
        for (change, revision) in [
            (&first_change, &first_revision),
            (&second_change, &second_revision),
        ] {
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(change, None, revision.clone(), &artifact)
                .unwrap();
        }
        let forward = repository
            .create_candidate(
                CandidateId::new("candidate-forward").unwrap(),
                artifact.base().clone(),
                vec![
                    CandidateInput::new(first_change.clone(), first_revision.clone()),
                    CandidateInput::new(second_change.clone(), second_revision.clone()),
                ],
            )
            .unwrap();
        let reverse = repository
            .create_candidate(
                CandidateId::new("candidate-reverse").unwrap(),
                artifact.base().clone(),
                vec![
                    CandidateInput::new(second_change, second_revision),
                    CandidateInput::new(first_change, first_revision),
                ],
            )
            .unwrap();
        assert_ne!(forward.content_digest(), reverse.content_digest());
        assert_eq!(
            forward.inputs(),
            repository
                .load_candidate(forward.candidate_id())
                .unwrap()
                .inputs()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_assignment_history_and_guards_materialization_transitions() {
        let root = temporary_directory();
        let database = root.join("weft.sqlite");
        let store = ContentStore::open(root.join("cas")).unwrap();
        let artifact = artifact(&store);
        let change = ChangeId::new("change-1").unwrap();
        let revision = RevisionId::new("revision-1").unwrap();
        let materialization = MaterializationId::new("materialization-1").unwrap();
        {
            let mut repository = SqliteRepository::open(&database, store.clone()).unwrap();
            repository.create_change(change.clone()).unwrap();
            repository
                .append_revision(&change, None, revision.clone(), &artifact)
                .unwrap();
            let assignment = Assignment::new(
                AssignmentId::new("assignment-1").unwrap(),
                change.clone(),
                "agent-1",
                "implementer",
                "operator",
                100,
            )
            .unwrap();
            repository.record_assignment(&assignment).unwrap();
            assert_eq!(
                repository.assignments_for(&change).unwrap(),
                vec![assignment]
            );
            assert_eq!(
                repository
                    .create_materialization(
                        materialization.clone(),
                        revision.clone(),
                        WorkspaceId::new("workspace-1").unwrap(),
                        "native-git",
                        "worktree:one",
                    )
                    .unwrap()
                    .state(),
                MaterializationState::Clean
            );
            assert!(matches!(
                repository.transition_materialization(
                    &materialization,
                    MaterializationState::Clean,
                    MaterializationState::Clean,
                ),
                Err(StorageError::InvalidMaterializationTransition { .. })
            ));
            assert_eq!(
                repository
                    .transition_materialization(
                        &materialization,
                        MaterializationState::Clean,
                        MaterializationState::Dirty,
                    )
                    .unwrap()
                    .state(),
                MaterializationState::Dirty
            );
            assert!(matches!(
                repository.transition_materialization(
                    &materialization,
                    MaterializationState::Clean,
                    MaterializationState::Released,
                ),
                Err(StorageError::StaleMaterializationState { .. })
            ));
        }
        let repository = SqliteRepository::open(&database, store).unwrap();
        assert_eq!(
            repository.assignments_for(&change).unwrap()[0].role(),
            "implementer"
        );
        assert_eq!(
            repository
                .load_materialization(&materialization)
                .unwrap()
                .state(),
            MaterializationState::Dirty
        );
        fs::remove_dir_all(root).unwrap();
    }
}
