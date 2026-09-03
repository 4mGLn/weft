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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerStatus {
    pub merge_base: String,
    pub target: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerBranch {
    pub name: String,
    pub change_id: String,
    pub commit_id: String,
    pub conflicted: bool,
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
    /// Reads the required target fields from the version-gated status schema.
    /// # Errors
    /// Returns an error for a CLI failure or unsupported status shape.
    pub fn status(&self) -> Result<GitButlerStatus, GitButlerError> {
        let raw = run("but", &self.root, ["--json", "status"])?;
        Ok(GitButlerStatus {
            merge_base: json_string_after(&raw, "mergeBase", "commitId")?,
            target: json_string_after(&raw, "latestCommit", "commitId")?,
        })
    }
    /// Lists normalized virtual branches from supported status JSON.
    /// # Errors
    /// Returns an error for a CLI failure or unsupported branch shape.
    pub fn branches(&self) -> Result<Vec<GitButlerBranch>, GitButlerError> {
        let raw = run("but", &self.root, ["--json", "status"])?;
        branches_from_status(&raw)
    }

    /// Creates a virtual branch commit from the current workspace changes.
    /// # Errors
    /// Returns an error when the CLI fails or the resulting branch is absent.
    pub fn commit_virtual_branch(
        &self,
        name: &str,
        message: &str,
    ) -> Result<GitButlerBranch, GitButlerError> {
        run("but", &self.root, ["commit", "-b", name, "-m", message])?;
        self.branches()?
            .into_iter()
            .find(|branch| branch.name == name)
            .ok_or(GitButlerError::MalformedOutput)
    }
    /// Amends a virtual branch and returns its refreshed provider reference.
    /// # Errors
    /// Returns an error when `GitButler` fails or the branch cannot be observed.
    pub fn amend_virtual_branch(&self, name: &str) -> Result<GitButlerBranch, GitButlerError> {
        run("but", &self.root, ["amend", "-t", name])?;
        self.branches()?
            .into_iter()
            .find(|branch| branch.name == name)
            .ok_or(GitButlerError::MalformedOutput)
    }

    /// Creates a virtual branch anchored above an existing branch.
    ///
    /// # Errors
    ///
    /// Returns an error when `GitButler` rejects the anchor.
    pub fn create_stacked_branch(&self, name: &str, anchor: &str) -> Result<(), GitButlerError> {
        run(
            "but",
            &self.root,
            ["branch", "new", name, "--anchor", anchor],
        )
        .map(|_| ())
    }

    /// Lands one complete virtual stack.
    ///
    /// # Errors
    ///
    /// Returns an error when `GitButler` refuses to land the stack.
    pub fn land_whole_stack(&self, branch: &str) -> Result<(), GitButlerError> {
        run(
            "but",
            &self.root,
            ["land", branch, "--whole-stack", "--yes"],
        )
        .map(|_| ())
    }

    /// Reconciles `GitButler` workspace state with its configured target.
    ///
    /// # Errors
    ///
    /// Returns an error when `GitButler` cannot complete reconciliation.
    pub fn reconcile_target(&self) -> Result<GitButlerStatus, GitButlerError> {
        run("but", &self.root, ["pull"])?;
        self.status()
    }
}

fn branches_from_status(raw: &str) -> Result<Vec<GitButlerBranch>, GitButlerError> {
    let mut result = Vec::new();
    let mut rest = raw;
    while let Some(at) = rest.find("\"name\"") {
        rest = &rest[at..];
        let name = json_string_after(rest, "name", "name")?;
        let change_id = json_string_after(rest, "name", "changeId")?;
        let commit_id = json_string_after(rest, "name", "commitId")?;
        let conflicted = json_bool_after(rest, "name", "conflicted")?;
        result.push(GitButlerBranch {
            name,
            change_id,
            commit_id,
            conflicted,
        });
        rest = &rest[6..];
    }
    Ok(result)
}

fn json_string_after(raw: &str, anchor: &str, key: &str) -> Result<String, GitButlerError> {
    let start = raw.find(anchor).ok_or(GitButlerError::MalformedOutput)?;
    let field = raw[start..]
        .find(key)
        .ok_or(GitButlerError::MalformedOutput)?
        + start
        + key.len();
    let value = raw[field..]
        .split('"')
        .nth(2)
        .map(str::to_owned)
        .ok_or(GitButlerError::MalformedOutput)?;
    if value.is_empty() {
        return Err(GitButlerError::MalformedOutput);
    }
    Ok(value)
}

fn json_bool_after(raw: &str, anchor: &str, key: &str) -> Result<bool, GitButlerError> {
    let start = raw.find(anchor).ok_or(GitButlerError::MalformedOutput)?;
    let field = raw[start..]
        .find(key)
        .ok_or(GitButlerError::MalformedOutput)?
        + start
        + key.len();
    let value_start = raw[field..]
        .find(':')
        .ok_or(GitButlerError::MalformedOutput)?
        + field
        + 1;
    let value = raw[value_start..].trim_start();
    if value.starts_with("true") {
        Ok(true)
    } else if value.starts_with("false") {
        Ok(false)
    } else {
        Err(GitButlerError::MalformedOutput)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projects_a_virtual_branch_from_the_supported_status_shape() {
        let raw = r#"{"mergeBase":{"commitId":"base"},"upstreamState":{"latestCommit":{"commitId":"target"}},"stacks":[{"branches":[{"name":"change-a","commits":[{"changeId":"logical-a","commitId":"provider-a","conflicted": false}]},{"name":"change-b","commits":[{"changeId":"logical-b","commitId":"provider-b","conflicted": true}]}]}]}"#;
        assert_eq!(
            json_string_after(raw, "mergeBase", "commitId").unwrap(),
            "base"
        );
        assert_eq!(
            json_string_after(raw, "latestCommit", "commitId").unwrap(),
            "target"
        );
        assert_eq!(
            branches_from_status(raw).unwrap(),
            vec![
                GitButlerBranch {
                    name: "change-a".to_owned(),
                    change_id: "logical-a".to_owned(),
                    commit_id: "provider-a".to_owned(),
                    conflicted: false,
                },
                GitButlerBranch {
                    name: "change-b".to_owned(),
                    change_id: "logical-b".to_owned(),
                    commit_id: "provider-b".to_owned(),
                    conflicted: true,
                },
            ]
        );
        assert!(json_string_after("{}", "mergeBase", "commitId").is_err());
        assert!(json_bool_after("{}", "name", "conflicted").is_err());
    }
}
