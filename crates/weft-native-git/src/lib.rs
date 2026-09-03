//! Native Git provider adapter.
//!
//! This crate owns Git command normalization only. Durable Change identity,
//! canonical artifacts, and integration history remain in `weft-domain`.

use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};
use std::process::Command;

use weft_domain::RepositoryId;

/// A discovered local Git repository and its stable provider identity inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeGitRepository {
    root: PathBuf,
    git_dir: PathBuf,
    repository_id: RepositoryId,
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
}

fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) -> Result<String, NativeGitError> {
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
    let value = String::from_utf8(output.stdout).map_err(NativeGitError::Utf8)?;
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(NativeGitError::MalformedOutput);
    }
    Ok(value)
}

#[derive(Debug)]
pub enum NativeGitError {
    Io(std::io::Error),
    Utf8(std::string::FromUtf8Error),
    Domain(weft_domain::ChangeError),
    Command { status: Option<i32>, stderr: String },
    MalformedOutput,
}
impl Display for NativeGitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Git invocation failed: {error}"),
            Self::Utf8(error) => write!(f, "Git emitted invalid UTF-8: {error}"),
            Self::Domain(error) => write!(f, "invalid Git identity: {error}"),
            Self::Command { status, stderr } => write!(f, "Git exited with {status:?}: {stderr}"),
            Self::MalformedOutput => f.write_str("Git emitted malformed output"),
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
        fs::remove_dir_all(path).unwrap();
    }
}
