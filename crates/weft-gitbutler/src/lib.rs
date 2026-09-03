//! Version-gated `GitButler` provider discovery.

use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};
use std::process::Command;
use weft_domain::{
    AuditContext, CanonicalArtifact, ConflictId, ContentStore, IntegrationAttempt,
    IntegrationConflict, IntegrationReceipt, IntegrationReceiptId, IntegrationState,
    ReconciliationId, ReconciliationRecord, RepositoryId, SqliteRepository, StorageError,
};
use weft_native_git::NativeGitRepository;

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

/// Evidence returned only after a whole-stack landing is re-observed at its target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerIntegrationReceipt {
    prior_target: String,
    result_target: String,
    branch: GitButlerBranch,
}

/// Result of reconciling a running `GitButler` landing attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitButlerReconciliation {
    Confirmed { result_target: String },
    Diverged { observed_target: String },
}

/// Durable `GitButler` integration outcome classification.
#[derive(Debug)]
pub enum GitButlerExecutionError {
    Storage(StorageError),
    Provider(GitButlerError),
    Conflict,
    Uncertain,
}

impl Display for GitButlerExecutionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(f, "durable integration state failed: {error}"),
            Self::Provider(error) => write!(f, "GitButler integration failed: {error}"),
            Self::Conflict => f.write_str("GitButler integration conflicted"),
            Self::Uncertain => {
                f.write_str("GitButler landing outcome is uncertain and requires reconciliation")
            }
        }
    }
}

impl std::error::Error for GitButlerExecutionError {}

impl GitButlerIntegrationReceipt {
    #[must_use]
    pub fn prior_target(&self) -> &str {
        &self.prior_target
    }

    #[must_use]
    pub fn result_target(&self) -> &str {
        &self.result_target
    }

    #[must_use]
    pub fn branch(&self) -> &GitButlerBranch {
        &self.branch
    }
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

    /// Exports a current virtual branch as provider-independent canonical content.
    ///
    /// The branch's logical and commit IDs remain provider references. The
    /// returned artifact stores only the exact base and canonical file content.
    ///
    /// # Errors
    ///
    /// Returns an error when the named branch is absent, the base or provider
    /// commit cannot be resolved exactly, or canonical content cannot be saved.
    pub fn export_branch_artifact(
        &self,
        base_commit: &str,
        name: &str,
        content_store: &ContentStore,
    ) -> Result<(GitButlerBranch, CanonicalArtifact), GitButlerError> {
        let branch = self
            .branches()?
            .into_iter()
            .find(|branch| branch.name == name)
            .ok_or(GitButlerError::MalformedOutput)?;
        let native = NativeGitRepository::discover(&self.root).map_err(GitButlerError::Native)?;
        let artifact = native
            .capture_revision(base_commit, &branch.commit_id, content_store)
            .map_err(GitButlerError::Native)?;
        Ok((branch, artifact))
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

    /// Executes a persisted whole-stack landing against the configured target.
    ///
    /// The target is observed before transition to `running` and again after
    /// `GitButler` reports success. A command failure after that transition is
    /// intentionally left running with reconciliation evidence, because the
    /// provider may have mutated target state before it returned an error.
    ///
    /// # Errors
    ///
    /// Returns an explicit durable conflict for a stale target or conflicted
    /// branch, and an explicit uncertain result for an unverified landing.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_integration(
        &self,
        domain: &mut SqliteRepository,
        attempt: &IntegrationAttempt,
        branch_name: &str,
        receipt_id: IntegrationReceiptId,
        conflict_id: ConflictId,
        reconciliation_id: ReconciliationId,
        audit: &AuditContext,
        now_unix_ms: i64,
    ) -> Result<GitButlerIntegrationReceipt, GitButlerExecutionError> {
        if attempt.repository_id() != &self.repository_id || attempt.provider() != "gitbutler" {
            return Err(GitButlerExecutionError::Provider(
                GitButlerError::RepositoryMismatch,
            ));
        }
        let status = self.status().map_err(GitButlerExecutionError::Provider)?;
        if status.target != attempt.expected_target_revision() {
            Self::record_conflict(
                domain,
                attempt,
                conflict_id,
                &status.target,
                "target-observation",
                audit,
            )?;
            return Err(GitButlerExecutionError::Conflict);
        }
        domain
            .start_integration(attempt.id(), &status.target, now_unix_ms)
            .map_err(GitButlerExecutionError::Storage)?;
        let branch = self
            .branches()
            .map_err(GitButlerExecutionError::Provider)?
            .into_iter()
            .find(|branch| branch.name == branch_name)
            .ok_or(GitButlerExecutionError::Provider(
                GitButlerError::MalformedOutput,
            ))?;
        if branch.conflicted {
            Self::record_conflict(
                domain,
                attempt,
                conflict_id,
                &status.target,
                "branch-conflicted",
                audit,
            )?;
            return Err(GitButlerExecutionError::Conflict);
        }
        if self.land_whole_stack(branch_name).is_err() {
            Self::record_uncertain(
                domain,
                attempt,
                reconciliation_id,
                "landing-command-error",
                audit,
            )?;
            return Err(GitButlerExecutionError::Uncertain);
        }
        let observed = if let Ok(status) = self.status() {
            status.target
        } else {
            Self::record_uncertain(
                domain,
                attempt,
                reconciliation_id,
                "landing-postcheck-error",
                audit,
            )?;
            return Err(GitButlerExecutionError::Uncertain);
        };
        let receipt = IntegrationReceipt::new(
            receipt_id,
            attempt.id().clone(),
            attempt.expected_target_revision(),
            &observed,
            format!(
                "gitbutler;branch:{};change:{};commit:{}",
                branch.name, branch.change_id, branch.commit_id
            ),
        )
        .map_err(GitButlerExecutionError::Storage)?;
        domain
            .finish_integration(
                attempt.id(),
                IntegrationState::Succeeded,
                Some(&receipt),
                audit,
            )
            .map_err(GitButlerExecutionError::Storage)?;
        Ok(GitButlerIntegrationReceipt {
            prior_target: status.target,
            result_target: observed,
            branch,
        })
    }

    /// Re-observes a running landing and completes it only at the exact result.
    ///
    /// # Errors
    ///
    /// Returns a provider or persistence error without asserting an outcome.
    pub fn reconcile_integration(
        &self,
        domain: &mut SqliteRepository,
        attempt: &IntegrationAttempt,
        expected_result: &str,
        receipt_id: IntegrationReceiptId,
        reconciliation_id: ReconciliationId,
        audit: &AuditContext,
    ) -> Result<GitButlerReconciliation, GitButlerExecutionError> {
        let observed = self
            .status()
            .map_err(GitButlerExecutionError::Provider)?
            .target;
        let confirmed = observed == expected_result;
        let record = ReconciliationRecord::new(
            reconciliation_id,
            attempt.id().clone(),
            if confirmed {
                "target-confirmed"
            } else {
                "target-diverged"
            },
            format!("expected:{expected_result};observed:{observed}"),
            confirmed,
        )
        .map_err(GitButlerExecutionError::Storage)?;
        domain
            .record_reconciliation(&record, audit)
            .map_err(GitButlerExecutionError::Storage)?;
        if !confirmed {
            return Ok(GitButlerReconciliation::Diverged {
                observed_target: observed,
            });
        }
        let receipt = IntegrationReceipt::new(
            receipt_id,
            attempt.id().clone(),
            attempt.expected_target_revision(),
            expected_result,
            format!("gitbutler;reconciled-target:{}", attempt.target_ref()),
        )
        .map_err(GitButlerExecutionError::Storage)?;
        domain
            .finish_integration(
                attempt.id(),
                IntegrationState::Succeeded,
                Some(&receipt),
                audit,
            )
            .map_err(GitButlerExecutionError::Storage)?;
        Ok(GitButlerReconciliation::Confirmed {
            result_target: observed,
        })
    }

    fn record_conflict(
        domain: &mut SqliteRepository,
        attempt: &IntegrationAttempt,
        conflict_id: ConflictId,
        observed_target: &str,
        operation: &str,
        audit: &AuditContext,
    ) -> Result<(), GitButlerExecutionError> {
        let conflict = IntegrationConflict::new(
            conflict_id,
            attempt.id().clone(),
            attempt.candidate_id().clone(),
            format!("gitbutler-target:{observed_target}"),
            operation,
            None,
            Some(observed_target.to_owned()),
            None,
        )
        .map_err(GitButlerExecutionError::Storage)?;
        domain
            .record_integration_conflict(&conflict, audit)
            .map_err(GitButlerExecutionError::Storage)
    }

    fn record_uncertain(
        domain: &mut SqliteRepository,
        attempt: &IntegrationAttempt,
        reconciliation_id: ReconciliationId,
        detail: &str,
        audit: &AuditContext,
    ) -> Result<(), GitButlerExecutionError> {
        let record = ReconciliationRecord::new(
            reconciliation_id,
            attempt.id().clone(),
            "landing-uncertain",
            detail,
            false,
        )
        .map_err(GitButlerExecutionError::Storage)?;
        domain
            .record_reconciliation(&record, audit)
            .map_err(GitButlerExecutionError::Storage)
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
    Native(weft_native_git::NativeGitError),
    Command(String),
    UnsupportedVersion(String),
    RepositoryMismatch,
    MalformedOutput,
}
impl Display for GitButlerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "GitButler invocation failed: {e}"),
            Self::Utf8(e) => write!(f, "GitButler emitted invalid UTF-8: {e}"),
            Self::Domain(e) => write!(f, "invalid GitButler identity: {e}"),
            Self::Native(e) => write!(f, "GitButler canonical export failed: {e}"),
            Self::Command(e) => write!(f, "GitButler failed: {e}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported GitButler version: {v}"),
            Self::RepositoryMismatch => {
                f.write_str("integration attempt does not belong to this GitButler repository")
            }
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
