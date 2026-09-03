//! Version-gated `GitButler` provider discovery.

use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};
use std::process::Command;
use weft_domain::RepositoryId;

const SUPPORTED_VERSION: &str = "0.22.";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerRepository {
    root: PathBuf,
    repository_id: RepositoryId,
}
impl GitButlerRepository {
    /// Discovers a supported `GitButler` workspace.
    ///
    /// # Errors
    ///
    /// Returns an error when the CLI is unavailable, its version is unsupported,
    /// or the path is not inside a Git repository.
    pub fn discover(path: impl AsRef<Path>) -> Result<Self, GitButlerError> {
        let path = path.as_ref();
        let version = run("but", path, ["--version"])?;
        if !version
            .strip_prefix("but ")
            .is_some_and(|value| value.starts_with(SUPPORTED_VERSION))
        {
            return Err(GitButlerError::UnsupportedVersion(version));
        }
        let root = run("git", path, ["rev-parse", "--show-toplevel"])?;
        let git_dir = run("git", path, ["rev-parse", "--absolute-git-dir"])?;
        let repository_id = RepositoryId::new(format!("gitbutler:{git_dir}:{SUPPORTED_VERSION}"))
            .map_err(GitButlerError::Domain)?;
        Ok(Self {
            root: PathBuf::from(root),
            repository_id,
        })
    }
    #[must_use]
    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}
fn run<const N: usize>(
    program: &str,
    cwd: &Path,
    args: [&str; N],
) -> Result<String, GitButlerError> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(GitButlerError::Io)?;
    if !output.status.success() {
        return Err(GitButlerError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let value = String::from_utf8(output.stdout)
        .map_err(GitButlerError::Utf8)?
        .trim()
        .to_owned();
    if value.is_empty() {
        return Err(GitButlerError::MalformedOutput);
    }
    Ok(value)
}
#[derive(Debug)]
pub enum GitButlerError {
    Io(std::io::Error),
    Utf8(std::string::FromUtf8Error),
    Domain(weft_domain::ChangeError),
    Command(String),
    UnsupportedVersion(String),
    MalformedOutput,
}
impl Display for GitButlerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "GitButler invocation failed: {e}"),
            Self::Utf8(e) => write!(f, "GitButler emitted invalid UTF-8: {e}"),
            Self::Domain(e) => write!(f, "invalid GitButler identity: {e}"),
            Self::Command(e) => write!(f, "GitButler failed: {e}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported GitButler version: {v}"),
            Self::MalformedOutput => f.write_str("GitButler emitted malformed output"),
        }
    }
}
impl std::error::Error for GitButlerError {}
