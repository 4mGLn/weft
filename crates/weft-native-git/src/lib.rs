//! Native Git provider adapter.
//!
//! This crate owns Git command normalization only. Durable Change identity,
//! canonical artifacts, and integration history remain in `weft-domain`.

use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use weft_domain::{
    BaseState, CanonicalArtifact, ContentStore, FileMode, PathOperation, RepositoryId, TreeDelta,
};

/// A discovered local Git repository and its stable provider identity inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeGitRepository {
    root: PathBuf,
    git_dir: PathBuf,
    repository_id: RepositoryId,
}

/// An isolated worktree reconstructed from one canonical artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeGitMaterialization {
    path: PathBuf,
    base_commit: String,
    tree_id: String,
}

impl NativeGitMaterialization {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn base_commit(&self) -> &str {
        &self.base_commit
    }

    /// The exact Git tree created by staging the reconstructed content.
    #[must_use]
    pub fn tree_id(&self) -> &str {
        &self.tree_id
    }
}

impl NativeGitRepository {
    /// Discovers a repository from any path beneath its worktree.
    ///
    /// # Errors
    /// Returns an error when Git is unavailable, the path is outside a worktree,
    /// or provider output is malformed.
    pub fn discover(path: impl AsRef<Path>) -> Result<Self, NativeGitError> {
        let path = path.as_ref();
        let root = run_git(path, ["rev-parse", "--show-toplevel"])?;
        let git_dir = run_git(path, ["rev-parse", "--absolute-git-dir"])?;
        let object_format = run_git(path, ["rev-parse", "--show-object-format"])?;
        let repository_id = RepositoryId::new(format!("native-git:{git_dir}:{object_format}"))
            .map_err(NativeGitError::Domain)?;
        Ok(Self {
            root: PathBuf::from(root),
            git_dir: PathBuf::from(git_dir),
            repository_id,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
    #[must_use]
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }
    #[must_use]
    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Resolves an exact provider object ID; symbolic refs are never returned.
    /// # Errors
    /// Returns an error for an absent or non-commit-ish ref.
    pub fn resolve_commit(&self, reference: &str) -> Result<String, NativeGitError> {
        run_git(
            &self.root,
            [
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{reference}^{{commit}}"),
            ],
        )
    }

    /// Compares an observed provider ref with one exact recorded commit.
    /// # Errors
    /// Returns an error if the observed ref cannot resolve to a commit.
    pub fn target_matches(&self, reference: &str, expected: &str) -> Result<bool, NativeGitError> {
        Ok(self.resolve_commit(reference)? == expected)
    }

    /// Captures the exact tree difference between two commits as a
    /// provider-independent canonical artifact.
    ///
    /// Git object IDs remain provider references. File bytes are copied into
    /// the Weft content store and the returned artifact records only their
    /// SHA-256 addresses, paths, modes, and exact base commit.
    ///
    /// # Errors
    ///
    /// Returns an error when either reference is not a commit, a changed path
    /// cannot be represented by the v1 canonical artifact, or the content
    /// store cannot durably accept a referenced blob.
    pub fn capture_revision(
        &self,
        base_reference: &str,
        revision_reference: &str,
        content_store: &ContentStore,
    ) -> Result<CanonicalArtifact, NativeGitError> {
        let base_commit = self.resolve_commit(base_reference)?;
        let revision_commit = self.resolve_commit(revision_reference)?;
        let output = run_git_bytes(
            &self.root,
            [
                "diff-tree",
                "--no-commit-id",
                "--no-renames",
                "-r",
                "--name-status",
                "-z",
                &base_commit,
                &revision_commit,
            ],
        )?;
        let mut fields = output.split(|byte| *byte == b'\0');
        let mut operations = Vec::new();
        while let Some(status) = fields.next() {
            if status.is_empty() {
                continue;
            }
            let path = fields.next().ok_or(NativeGitError::MalformedOutput)?;
            let status = std::str::from_utf8(status).map_err(NativeGitError::PathEncoding)?;
            let path = std::str::from_utf8(path).map_err(NativeGitError::PathEncoding)?;
            match status.as_bytes().first() {
                Some(b'D') => operations.push(PathOperation::Delete {
                    path: path.to_owned(),
                }),
                Some(b'A' | b'M' | b'T') => {
                    operations.push(self.capture_upsert(&revision_commit, path, content_store)?);
                }
                _ => return Err(NativeGitError::UnsupportedChange(status.to_owned())),
            }
        }
        operations.sort_by(|left, right| left.path().cmp(right.path()));
        let tree_delta = TreeDelta::new(operations).map_err(NativeGitError::Artifact)?;
        let base = BaseState::new(self.repository_id.clone(), format!("git:{base_commit}"))
            .map_err(NativeGitError::Domain)?;
        Ok(CanonicalArtifact::new(base, tree_delta))
    }

    fn capture_upsert(
        &self,
        revision_commit: &str,
        path: &str,
        content_store: &ContentStore,
    ) -> Result<PathOperation, NativeGitError> {
        let output = run_git_bytes(&self.root, ["ls-tree", "-z", revision_commit, "--", path])?;
        let entry = output
            .strip_suffix(b"\0")
            .ok_or(NativeGitError::MalformedOutput)?;
        if entry.contains(&b'\0') {
            return Err(NativeGitError::MalformedOutput);
        }
        let separator = entry
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or(NativeGitError::MalformedOutput)?;
        let (header, returned_path) = (&entry[..separator], &entry[separator + 1..]);
        if returned_path != path.as_bytes() {
            return Err(NativeGitError::MalformedOutput);
        }
        let mut fields = header.split(|byte| *byte == b' ');
        let mode = fields.next().ok_or(NativeGitError::MalformedOutput)?;
        let kind = fields.next().ok_or(NativeGitError::MalformedOutput)?;
        let object_id = fields.next().ok_or(NativeGitError::MalformedOutput)?;
        if fields.next().is_some() || kind != b"blob" {
            return Err(NativeGitError::MalformedOutput);
        }
        let mode = match mode {
            b"100644" => FileMode::Regular,
            b"100755" => FileMode::Executable,
            b"120000" => FileMode::SymbolicLink,
            _ => {
                return Err(NativeGitError::UnsupportedMode(
                    String::from_utf8_lossy(mode).into_owned(),
                ));
            }
        };
        let object_id = std::str::from_utf8(object_id).map_err(NativeGitError::PathEncoding)?;
        let content = run_git_bytes(&self.root, ["cat-file", "blob", object_id])?;
        let blob_digest = content_store
            .put_blob(&content)
            .map_err(NativeGitError::Storage)?;
        Ok(PathOperation::Upsert {
            path: path.to_owned(),
            mode,
            blob_digest,
        })
    }

    /// Creates a detached worktree at an artifact's exact base and applies its
    /// canonical delta to its index and working tree.
    ///
    /// The caller owns the destination and must later remove the worktree with
    /// Git. A returned tree ID provides a provider-native receipt of the
    /// reconstructed content; it is not used as canonical revision identity.
    ///
    /// # Errors
    ///
    /// Returns an error without creating a worktree when the artifact belongs
    /// to another repository or its recorded base is no longer resolvable.
    /// Errors after `git worktree add` leave the worktree for explicit caller
    /// inspection and reconciliation.
    pub fn materialize_artifact(
        &self,
        artifact: &CanonicalArtifact,
        content_store: &ContentStore,
        destination: impl AsRef<Path>,
    ) -> Result<NativeGitMaterialization, NativeGitError> {
        if artifact.base().repository_id() != &self.repository_id {
            return Err(NativeGitError::RepositoryMismatch);
        }
        let base_commit = artifact
            .base()
            .object_id()
            .strip_prefix("git:")
            .ok_or(NativeGitError::InvalidBaseObject)?;
        if self.resolve_commit(base_commit)? != base_commit {
            return Err(NativeGitError::InvalidBaseObject);
        }
        let destination = destination.as_ref();
        let destination_text = destination
            .to_str()
            .ok_or(NativeGitError::InvalidDestination)?;
        if destination.exists() {
            return Err(NativeGitError::DestinationExists(destination.to_path_buf()));
        }
        run_git_bytes(
            &self.root,
            ["worktree", "add", "--detach", destination_text, base_commit],
        )?;

        for operation in artifact.tree_delta().operations() {
            if let PathOperation::Delete { path } = operation {
                remove_path(destination, path)?;
            }
        }
        for operation in artifact.tree_delta().operations() {
            if let PathOperation::Upsert {
                path,
                mode,
                blob_digest,
            } = operation
            {
                write_path(
                    destination,
                    path,
                    *mode,
                    &content_store
                        .read_blob(blob_digest)
                        .map_err(NativeGitError::Storage)?,
                )?;
            }
        }
        run_git_bytes(destination, ["add", "--all"])?;
        let tree_id = run_git(destination, ["write-tree"])?;
        Ok(NativeGitMaterialization {
            path: destination.to_path_buf(),
            base_commit: base_commit.to_owned(),
            tree_id,
        })
    }
}

fn remove_path(root: &Path, relative: &str) -> Result<(), NativeGitError> {
    let path = root.join(relative);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            fs::remove_dir(&path).map_err(NativeGitError::Io)
        }
        Ok(_) => fs::remove_file(&path).map_err(NativeGitError::Io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(NativeGitError::Io(error)),
    }
}

#[cfg(unix)]
fn write_path(
    root: &Path,
    relative: &str,
    mode: FileMode,
    content: &[u8],
) -> Result<(), NativeGitError> {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(NativeGitError::Io)?;
    }
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            fs::remove_dir_all(&path).map_err(NativeGitError::Io)?;
        }
        Ok(_) => fs::remove_file(&path).map_err(NativeGitError::Io)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(NativeGitError::Io(error)),
    }
    match mode {
        FileMode::Regular | FileMode::Executable => {
            fs::write(&path, content).map_err(NativeGitError::Io)?;
            let permissions = if mode == FileMode::Executable {
                0o755
            } else {
                0o644
            };
            fs::set_permissions(path, fs::Permissions::from_mode(permissions))
                .map_err(NativeGitError::Io)
        }
        FileMode::SymbolicLink => {
            let target = std::str::from_utf8(content).map_err(NativeGitError::PathEncoding)?;
            symlink(target, path).map_err(NativeGitError::Io)
        }
    }
}

#[cfg(not(unix))]
fn write_path(
    _root: &Path,
    _relative: &str,
    _mode: FileMode,
    _content: &[u8],
) -> Result<(), NativeGitError> {
    Err(NativeGitError::UnsupportedPlatform)
}

fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) -> Result<String, NativeGitError> {
    let value = String::from_utf8(run_git_bytes(cwd, args)?).map_err(NativeGitError::Utf8)?;
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(NativeGitError::MalformedOutput);
    }
    Ok(value)
}

fn run_git_bytes<const N: usize>(cwd: &Path, args: [&str; N]) -> Result<Vec<u8>, NativeGitError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(NativeGitError::Io)?;
    if !output.status.success() {
        return Err(NativeGitError::Command {
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(output.stdout)
}

#[derive(Debug)]
pub enum NativeGitError {
    Io(std::io::Error),
    Utf8(std::string::FromUtf8Error),
    PathEncoding(std::str::Utf8Error),
    Domain(weft_domain::ChangeError),
    Artifact(weft_domain::ArtifactError),
    Storage(weft_domain::StorageError),
    Command { status: Option<i32>, stderr: String },
    MalformedOutput,
    UnsupportedChange(String),
    UnsupportedMode(String),
    RepositoryMismatch,
    InvalidBaseObject,
    InvalidDestination,
    DestinationExists(PathBuf),
    UnsupportedPlatform,
}
impl Display for NativeGitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Git invocation failed: {error}"),
            Self::Utf8(error) => write!(f, "Git emitted invalid UTF-8: {error}"),
            Self::PathEncoding(error) => write!(f, "Git emitted a non-UTF-8 path: {error}"),
            Self::Domain(error) => write!(f, "invalid Git identity: {error}"),
            Self::Artifact(error) => write!(f, "invalid canonical artifact: {error}"),
            Self::Storage(error) => write!(f, "content store failed: {error}"),
            Self::Command { status, stderr } => write!(f, "Git exited with {status:?}: {stderr}"),
            Self::MalformedOutput => f.write_str("Git emitted malformed output"),
            Self::UnsupportedChange(status) => write!(f, "unsupported Git change status: {status}"),
            Self::UnsupportedMode(mode) => write!(f, "unsupported Git tree mode: {mode}"),
            Self::RepositoryMismatch => f.write_str("artifact belongs to a different repository"),
            Self::InvalidBaseObject => f.write_str("artifact base is not an exact Git commit"),
            Self::InvalidDestination => f.write_str("worktree destination is not valid UTF-8"),
            Self::DestinationExists(path) => {
                write!(f, "worktree destination already exists: {}", path.display())
            }
            Self::UnsupportedPlatform => {
                f.write_str("native worktree materialization requires Unix file modes and symlinks")
            }
        }
    }
}
impl std::error::Error for NativeGitError {}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn expected_operations() -> Vec<PathOperation> {
        vec![
            PathOperation::Upsert {
                path: "README".to_owned(),
                mode: FileMode::Regular,
                blob_digest: weft_domain::sha256_digest(b"updated\n"),
            },
            PathOperation::Upsert {
                path: "binary.dat".to_owned(),
                mode: FileMode::Regular,
                blob_digest: weft_domain::sha256_digest(&[0, 255, 1, 2]),
            },
            PathOperation::Delete {
                path: "delete.txt".to_owned(),
            },
            PathOperation::Upsert {
                path: "link".to_owned(),
                mode: FileMode::SymbolicLink,
                blob_digest: weft_domain::sha256_digest(b"binary.dat"),
            },
            PathOperation::Upsert {
                path: "script".to_owned(),
                mode: FileMode::Executable,
                blob_digest: weft_domain::sha256_digest(b"#!/bin/sh\necho weft\n"),
            },
        ]
    }

    fn temporary_repository() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "weft-native-git-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.name", "Weft test"],
            vec!["config", "user.email", "test@weft.invalid"],
            vec!["config", "commit.gpgSign", "false"],
        ] {
            let status = Command::new("git")
                .args(args)
                .current_dir(&path)
                .status()
                .unwrap();
            assert!(status.success());
        }
        fs::write(path.join("README"), "base\n").unwrap();
        for args in [
            vec!["add", "README"],
            vec!["commit", "--quiet", "-m", "base"],
        ] {
            let status = Command::new("git")
                .args(args)
                .current_dir(&path)
                .status()
                .unwrap();
            assert!(status.success());
        }
        path
    }

    #[test]
    fn discovers_and_resolves_exact_head() {
        let path = temporary_repository();
        let repository = NativeGitRepository::discover(&path).unwrap();
        let expected = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        assert_eq!(repository.resolve_commit("HEAD").unwrap(), expected);
        assert!(
            repository
                .repository_id()
                .as_str()
                .starts_with("native-git:")
        );
        assert!(repository.target_matches("HEAD", &expected).unwrap());
        fs::write(path.join("external"), "advance\n").unwrap();
        let status = Command::new("git")
            .args(["add", "external"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "--quiet", "-m", "external"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!repository.target_matches("HEAD", &expected).unwrap());
        fs::remove_dir_all(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn captures_canonical_content_and_modes_from_exact_git_trees() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let path = temporary_repository();
        fs::write(path.join("delete.txt"), "remove me\n").unwrap();
        fs::write(path.join("unchanged.txt"), "unchanged\n").unwrap();
        let status = Command::new("git")
            .args(["add", "delete.txt", "unchanged.txt"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "--quiet", "-m", "capture base"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let base = run_git(&path, ["rev-parse", "HEAD"]).unwrap();

        fs::remove_file(path.join("delete.txt")).unwrap();
        fs::write(path.join("binary.dat"), [0, 255, 1, 2]).unwrap();
        fs::write(path.join("script"), "#!/bin/sh\necho weft\n").unwrap();
        let mut permissions = fs::metadata(path.join("script")).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path.join("script"), permissions).unwrap();
        symlink("binary.dat", path.join("link")).unwrap();
        fs::write(path.join("README"), "updated\n").unwrap();
        let status = Command::new("git")
            .args(["add", "--all"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "--quiet", "-m", "capture target"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());

        let repository = NativeGitRepository::discover(&path).unwrap();
        let store = ContentStore::open(path.join("weft-content")).unwrap();
        let artifact = repository.capture_revision(&base, "HEAD", &store).unwrap();
        let target_tree = run_git(&path, ["rev-parse", "HEAD^{tree}"]).unwrap();
        assert_eq!(artifact.base().repository_id(), repository.repository_id());
        assert_eq!(artifact.base().object_id(), format!("git:{base}"));
        assert_eq!(artifact.tree_delta().operations(), expected_operations());
        store.put_artifact(&artifact).unwrap();
        assert_eq!(store.read_artifact(artifact.digest()).unwrap(), artifact);
        assert_eq!(
            store
                .read_blob(&weft_domain::sha256_digest(&[0, 255, 1, 2]))
                .unwrap(),
            vec![0, 255, 1, 2]
        );
        let materialized_path = path.with_file_name(format!(
            "weft-native-git-materialized-{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let materialization = repository
            .materialize_artifact(&artifact, &store, &materialized_path)
            .unwrap();
        assert_eq!(materialization.base_commit(), base);
        assert_eq!(materialization.tree_id(), target_tree);
        assert_eq!(
            run_git(materialization.path(), ["rev-parse", "HEAD"]).unwrap(),
            base
        );
        assert_eq!(
            run_git(materialization.path(), ["status", "--porcelain"]).unwrap(),
            "M  README\nA  binary.dat\nD  delete.txt\nA  link\nA  script"
        );
        let status = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&materialized_path)
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(path).unwrap();
    }
}
