use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::artifact::{PathOperation, is_sha256_digest, sha256_digest};
use crate::{CanonicalArtifact, ChangeError, ChangeId, RevisionId};

const SCHEMA_VERSION: i64 = 2;
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
    MissingChange(ChangeId),
    MissingRevision(RevisionId),
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
            Self::MissingChange(id) => write!(formatter, "missing change: {}", id.as_str()),
            Self::MissingRevision(id) => write!(formatter, "missing revision: {}", id.as_str()),
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

fn valid_lease_value(value: String, kind: &'static str) -> Result<String, StorageError> {
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
}
