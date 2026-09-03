//! Native Git provider adapter.
//!
//! This crate owns Git command normalization only. Durable Change identity,
//! canonical artifacts, and integration history remain in `weft-domain`.

use std::collections::BTreeSet;
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

/// Provider evidence returned only after a target compare-and-swap is verified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeGitIntegrationReceipt {
    target_ref: String,
    prior_target: String,
    result_commit: String,
    result_tree: String,
}

impl NativeGitIntegrationReceipt {
    #[must_use]
    pub fn target_ref(&self) -> &str {
        &self.target_ref
    }
    #[must_use]
    pub fn prior_target(&self) -> &str {
        &self.prior_target
    }
    #[must_use]
    pub fn result_commit(&self) -> &str {
        &self.result_commit
    }
    #[must_use]
    pub fn result_tree(&self) -> &str {
        &self.result_tree
    }
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

    /// Lists canonical repository-relative paths changed by two exact commits.
    ///
    /// Rename inference is deliberately disabled: an inferred rename is an
    /// upsert and delete in `tree-delta-v1`, not a mutable provider label.
    ///
    /// # Errors
    ///
    /// Returns an error if either reference cannot resolve to a commit or Git
    /// returns a path that v1's UTF-8 artifact contract cannot represent.
    pub fn changed_paths(
        &self,
        base_reference: &str,
        revision_reference: &str,
    ) -> Result<Vec<String>, NativeGitError> {
        let base_commit = self.resolve_commit(base_reference)?;
        let revision_commit = self.resolve_commit(revision_reference)?;
        self.changed_paths_for_commits(&base_commit, &revision_commit)
    }

    /// Reports paths changed by both revisions relative to one exact base.
    ///
    /// The result is sorted and duplicate-free, making it suitable as durable
    /// overlap evidence after callers bind it to exact revision identities.
    ///
    /// # Errors
    ///
    /// Returns an error if the base or either revision cannot resolve exactly.
    pub fn overlapping_paths(
        &self,
        base_reference: &str,
        left_reference: &str,
        right_reference: &str,
    ) -> Result<Vec<String>, NativeGitError> {
        let base_commit = self.resolve_commit(base_reference)?;
        let left_commit = self.resolve_commit(left_reference)?;
        let right_commit = self.resolve_commit(right_reference)?;
        let left = self
            .changed_paths_for_commits(&base_commit, &left_commit)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        let right = self
            .changed_paths_for_commits(&base_commit, &right_commit)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        Ok(left.intersection(&right).cloned().collect())
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

    fn changed_paths_for_commits(
        &self,
        base_commit: &str,
        revision_commit: &str,
    ) -> Result<Vec<String>, NativeGitError> {
        let output = run_git_bytes(
            &self.root,
            [
                "diff-tree",
                "--no-commit-id",
                "--no-renames",
                "-r",
                "--name-only",
                "-z",
                base_commit,
                revision_commit,
            ],
        )?;
        let mut paths = output
            .split(|byte| *byte == b'\0')
            .filter(|path| !path.is_empty())
            .map(|path| {
                std::str::from_utf8(path)
                    .map(str::to_owned)
                    .map_err(NativeGitError::PathEncoding)
            })
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort();
        paths.dedup();
        Ok(paths)
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

        apply_artifact_operations(destination, artifact, content_store)?;
        run_git_bytes(destination, ["add", "--all"])?;
        let tree_id = run_git(destination, ["write-tree"])?;
        Ok(NativeGitMaterialization {
            path: destination.to_path_buf(),
            base_commit: base_commit.to_owned(),
            tree_id,
        })
    }

    /// Materializes ordered canonical artifacts into one detached worktree.
    ///
    /// A later artifact is applied only where the staged tree still matches
    /// its recorded exact base. Tree deltas contain no merge preimages, so an
    /// overlapping divergent path is an explicit conflict, never a guessed
    /// provider merge.
    ///
    /// # Errors
    ///
    /// Returns [`NativeGitError::CompositionConflict`] with sorted paths when
    /// earlier inputs have changed a later input's base-relative paths.
    pub fn compose_artifacts(
        &self,
        artifacts: &[CanonicalArtifact],
        content_store: &ContentStore,
        destination: impl AsRef<Path>,
    ) -> Result<NativeGitMaterialization, NativeGitError> {
        let (first, rest) = artifacts
            .split_first()
            .ok_or(NativeGitError::EmptyComposition)?;
        let mut materialization = self.materialize_artifact(first, content_store, destination)?;
        for artifact in rest {
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
            let changed = staged_paths_against(materialization.path(), base_commit)?;
            let artifact_paths = artifact
                .tree_delta()
                .operations()
                .iter()
                .map(PathOperation::path)
                .collect::<BTreeSet<_>>();
            let conflicts = changed
                .into_iter()
                .filter(|path| artifact_paths.contains(path.as_str()))
                .collect::<Vec<_>>();
            if !conflicts.is_empty() {
                return Err(NativeGitError::CompositionConflict(conflicts));
            }
            apply_artifact_operations(materialization.path(), artifact, content_store)?;
            run_git_bytes(materialization.path(), ["add", "--all"])?;
            materialization.tree_id = run_git(materialization.path(), ["write-tree"])?;
        }
        Ok(materialization)
    }

    /// Commits a verified tree and atomically advances one target ref.
    ///
    /// This operation never retries a failed compare-and-swap. Callers must
    /// reconcile an uncertain result instead of reporting success.
    ///
    /// # Errors
    ///
    /// Returns [`NativeGitError::TargetMismatch`] without mutating the target
    /// when its observed commit is not `expected_target`. A successful CAS
    /// followed by a different observed target is uncertain, not successful.
    pub fn integrate_tree(
        &self,
        target_ref: &str,
        expected_target: &str,
        tree_id: &str,
        message: &str,
    ) -> Result<NativeGitIntegrationReceipt, NativeGitError> {
        if self.resolve_commit(expected_target)? != expected_target {
            return Err(NativeGitError::InvalidBaseObject);
        }
        let observed = self.resolve_commit(target_ref)?;
        if observed != expected_target {
            return Err(NativeGitError::TargetMismatch {
                expected: expected_target.to_owned(),
                actual: observed,
            });
        }
        if run_git(
            &self.root,
            [
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{tree_id}^{{tree}}"),
            ],
        )? != tree_id
        {
            return Err(NativeGitError::InvalidTreeObject);
        }
        let result_commit = create_commit(&self.root, tree_id, expected_target, message)?;
        if let Err(error) = run_git_bytes(
            &self.root,
            [
                "update-ref",
                "--no-deref",
                target_ref,
                &result_commit,
                expected_target,
            ],
        ) {
            let actual = self.resolve_commit(target_ref)?;
            if actual != expected_target {
                return Err(NativeGitError::TargetMismatch {
                    expected: expected_target.to_owned(),
                    actual,
                });
            }
            return Err(error);
        }
        let actual = self.resolve_commit(target_ref)?;
        if actual != result_commit {
            return Err(NativeGitError::UncertainTarget {
                expected_result: result_commit,
                actual,
            });
        }
        Ok(NativeGitIntegrationReceipt {
            target_ref: target_ref.to_owned(),
            prior_target: expected_target.to_owned(),
            result_commit: actual,
            result_tree: tree_id.to_owned(),
        })
    }
}

fn apply_artifact_operations(
    destination: &Path,
    artifact: &CanonicalArtifact,
    content_store: &ContentStore,
) -> Result<(), NativeGitError> {
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
            let content = content_store
                .read_blob(blob_digest)
                .map_err(NativeGitError::Storage)?;
            write_path(destination, path, *mode, &content)?;
        }
    }
    Ok(())
}

fn staged_paths_against(
    worktree: &Path,
    base_commit: &str,
) -> Result<BTreeSet<String>, NativeGitError> {
    let output = run_git_bytes(
        worktree,
        [
            "diff",
            "--cached",
            "--no-renames",
            "--name-only",
            "-z",
            base_commit,
        ],
    )?;
    output
        .split(|byte| *byte == b'\0')
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .map(str::to_owned)
                .map_err(NativeGitError::PathEncoding)
        })
        .collect()
}

fn create_commit(
    repository: &Path,
    tree_id: &str,
    parent: &str,
    message: &str,
) -> Result<String, NativeGitError> {
    let output = Command::new("git")
        .args(["commit-tree", tree_id, "-p", parent, "-m", message])
        .env("GIT_AUTHOR_NAME", "Weft")
        .env("GIT_AUTHOR_EMAIL", "weft@localhost")
        .env("GIT_COMMITTER_NAME", "Weft")
        .env("GIT_COMMITTER_EMAIL", "weft@localhost")
        .current_dir(repository)
        .output()
        .map_err(NativeGitError::Io)?;
    if !output.status.success() {
        return Err(NativeGitError::Command {
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let commit = String::from_utf8(output.stdout).map_err(NativeGitError::Utf8)?;
    let commit = commit.trim().to_owned();
    if commit.is_empty() {
        return Err(NativeGitError::MalformedOutput);
    }
    Ok(commit)
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
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    };

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
            symlink(std::ffi::OsStr::from_bytes(content), path).map_err(NativeGitError::Io)
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
    Command {
        status: Option<i32>,
        stderr: String,
    },
    MalformedOutput,
    UnsupportedChange(String),
    UnsupportedMode(String),
    RepositoryMismatch,
    InvalidBaseObject,
    InvalidDestination,
    DestinationExists(PathBuf),
    UnsupportedPlatform,
    EmptyComposition,
    CompositionConflict(Vec<String>),
    InvalidTreeObject,
    TargetMismatch {
        expected: String,
        actual: String,
    },
    UncertainTarget {
        expected_result: String,
        actual: String,
    },
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
            Self::EmptyComposition => f.write_str("composition requires at least one artifact"),
            Self::CompositionConflict(paths) => {
                write!(f, "canonical artifacts conflict at: {}", paths.join(", "))
            }
            Self::InvalidTreeObject => f.write_str("integration tree is not an exact Git tree"),
            Self::TargetMismatch { expected, actual } => {
                write!(f, "target changed: expected {expected}, observed {actual}")
            }
            Self::UncertainTarget {
                expected_result,
                actual,
            } => write!(
                f,
                "target changed after compare-and-swap: expected {expected_result}, observed {actual}"
            ),
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
                blob_digest: weft_domain::sha256_digest(b"target-\xff"),
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

    fn commit_files(path: &Path, message: &str, files: &[&str]) {
        let status = Command::new("git")
            .arg("add")
            .args(files)
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "--quiet", "-m", message])
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn remove_worktree(repository: &Path, destination: &Path) {
        let status = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(destination)
            .current_dir(repository)
            .status()
            .unwrap();
        assert!(status.success());
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

    #[test]
    fn finds_sorted_path_overlap_from_one_exact_base() {
        let path = temporary_repository();
        let base = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        fs::write(path.join("shared"), "left\n").unwrap();
        fs::write(path.join("left-only"), "left\n").unwrap();
        let status = Command::new("git")
            .args(["add", "shared", "left-only"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "--quiet", "-m", "left"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let left = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        let status = Command::new("git")
            .args(["checkout", "--quiet", "-b", "overlap-right", &base])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(path.join("shared"), "right\n").unwrap();
        fs::write(path.join("right-only"), "right\n").unwrap();
        let status = Command::new("git")
            .args(["add", "shared", "right-only"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "--quiet", "-m", "right"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let right = run_git(&path, ["rev-parse", "HEAD"]).unwrap();

        let repository = NativeGitRepository::discover(&path).unwrap();
        assert_eq!(
            repository.changed_paths(&base, &left).unwrap(),
            ["left-only", "shared"]
        );
        assert_eq!(
            repository.overlapping_paths(&base, &left, &right).unwrap(),
            ["shared"]
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn composes_disjoint_artifacts_and_reports_ambiguous_paths() {
        let path = temporary_repository();
        let base = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        fs::write(path.join("a"), "a\n").unwrap();
        commit_files(&path, "a", &["a"]);
        let a = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        let status = Command::new("git")
            .args(["checkout", "--quiet", "-b", "candidate-b", &base])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(path.join("b"), "b\n").unwrap();
        commit_files(&path, "b", &["b"]);
        let b = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        let repository = NativeGitRepository::discover(&path).unwrap();
        let store = ContentStore::open(path.join("weft-content")).unwrap();
        let artifact_a = repository.capture_revision(&base, &a, &store).unwrap();
        let artifact_b = repository.capture_revision(&base, &b, &store).unwrap();

        let status = Command::new("git")
            .args(["checkout", "--quiet", "-b", "candidate-expected", &base])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(path.join("a"), "a\n").unwrap();
        fs::write(path.join("b"), "b\n").unwrap();
        commit_files(&path, "expected", &["a", "b"]);
        let expected_tree = run_git(&path, ["rev-parse", "HEAD^{tree}"]).unwrap();
        let destination = path.with_file_name(format!(
            "weft-native-git-compose-{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let composition = repository
            .compose_artifacts(&[artifact_a.clone(), artifact_b], &store, &destination)
            .unwrap();
        assert_eq!(composition.tree_id(), expected_tree);
        remove_worktree(&path, &destination);

        let status = Command::new("git")
            .args([
                "checkout",
                "--quiet",
                "-b",
                "candidate-conflict-left",
                &base,
            ])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(path.join("README"), "left\n").unwrap();
        commit_files(&path, "conflict left", &["README"]);
        let left = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        let artifact_left = repository.capture_revision(&base, &left, &store).unwrap();
        let status = Command::new("git")
            .args([
                "checkout",
                "--quiet",
                "-b",
                "candidate-conflict-right",
                &base,
            ])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(path.join("README"), "right\n").unwrap();
        commit_files(&path, "conflict right", &["README"]);
        let right = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        let artifact_right = repository.capture_revision(&base, &right, &store).unwrap();
        let conflict_destination = path.with_file_name(format!(
            "weft-native-git-conflict-{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(matches!(
            repository.compose_artifacts(
                &[artifact_left, artifact_right],
                &store,
                &conflict_destination,
            ),
            Err(NativeGitError::CompositionConflict(paths)) if paths == ["README"]
        ));
        remove_worktree(&path, &conflict_destination);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn integrates_a_tree_with_target_compare_and_swap() {
        let path = temporary_repository();
        let base = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        fs::write(path.join("integrated"), "result\n").unwrap();
        commit_files(&path, "candidate result", &["integrated"]);
        let tree = run_git(&path, ["rev-parse", "HEAD^{tree}"]).unwrap();
        let status = Command::new("git")
            .args(["branch", "target", &base])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let repository = NativeGitRepository::discover(&path).unwrap();
        let receipt = repository
            .integrate_tree("refs/heads/target", &base, &tree, "integrate candidate")
            .unwrap();
        assert_eq!(receipt.prior_target(), base);
        assert_eq!(receipt.result_tree(), tree);
        assert_eq!(
            run_git(&path, ["rev-parse", "refs/heads/target"]).unwrap(),
            receipt.result_commit()
        );
        assert_eq!(
            run_git(&path, ["rev-parse", "refs/heads/target^{tree}"]).unwrap(),
            tree
        );
        assert_eq!(
            run_git(&path, ["rev-parse", "refs/heads/target^"]).unwrap(),
            base
        );

        fs::write(path.join("external"), "advance\n").unwrap();
        commit_files(&path, "external", &["external"]);
        let external = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        let status = Command::new("git")
            .args(["update-ref", "refs/heads/target", &external])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(matches!(
            repository.integrate_tree(
                "refs/heads/target",
                receipt.result_commit(),
                &tree,
                "must not replace external",
            ),
            Err(NativeGitError::TargetMismatch { expected, actual })
                if expected == receipt.result_commit() && actual == external
        ));
        assert_eq!(
            run_git(&path, ["rev-parse", "refs/heads/target"]).unwrap(),
            external
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn captures_canonical_content_and_modes_from_exact_git_trees() {
        use std::os::unix::{
            ffi::OsStrExt,
            fs::{PermissionsExt, symlink},
        };

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
        symlink(
            std::ffi::OsStr::from_bytes(b"target-\xff"),
            path.join("link"),
        )
        .unwrap();
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
