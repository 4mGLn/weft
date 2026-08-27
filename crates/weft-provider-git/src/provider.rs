use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use weft_artifact::{ArtifactStore, CanonicalTreeDelta};
use weft_domain::{
    ArtifactRef, BaseState, EffectOperationId, FileMode, IntegrationEvidence, MaterializationState,
    PathOperation, ProviderEvidence, ProviderObservation, ProviderRef, RepositoryId,
    TargetObservation, TargetRef, TargetRevision, TreeDelta,
};

use crate::GitProviderError;
use crate::command::{CommandOutput, CommandPolicy, run_git};

const MINIMUM_GIT_MAJOR: u32 = 2;
const MINIMUM_GIT_MINOR: u32 = 38;
const PROVIDER_ID: &str = "native-git";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GitCapability {
    ExactRevisionInspection,
    CanonicalCapture,
    DetachedWorktrees,
    CandidateComposition,
    GuardedRefUpdate,
    ConflictCapture,
    Reconciliation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitCapabilities(BTreeSet<GitCapability>);

impl GitCapabilities {
    fn local_v1() -> Self {
        Self(BTreeSet::from([
            GitCapability::ExactRevisionInspection,
            GitCapability::CanonicalCapture,
            GitCapability::DetachedWorktrees,
            GitCapability::CandidateComposition,
            GitCapability::GuardedRefUpdate,
            GitCapability::ConflictCapture,
            GitCapability::Reconciliation,
        ]))
    }

    #[must_use]
    pub fn supports(&self, capability: GitCapability) -> bool {
        self.0.contains(&capability)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryDiscovery {
    pub worktree_root: PathBuf,
    pub common_git_directory: PathBuf,
    pub provider_locator_evidence: String,
    pub object_format: String,
    pub git_version: String,
    pub capabilities: GitCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionObservation {
    commit_id: String,
    tree_id: String,
    evidence: String,
}

impl RevisionObservation {
    #[must_use]
    pub fn commit_id(&self) -> &str {
        &self.commit_id
    }

    #[must_use]
    pub fn tree_id(&self) -> &str {
        &self.tree_id
    }

    #[must_use]
    pub fn evidence(&self) -> &str {
        &self.evidence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedRevision {
    observation: RevisionObservation,
    artifact_ref: ArtifactRef,
    changed_paths: Vec<String>,
}

impl CapturedRevision {
    #[must_use]
    pub const fn observation(&self) -> &RevisionObservation {
        &self.observation
    }

    #[must_use]
    pub const fn artifact_ref(&self) -> &ArtifactRef {
        &self.artifact_ref
    }

    #[must_use]
    pub fn changed_paths(&self) -> &[String] {
        &self.changed_paths
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializationResult {
    pub path: PathBuf,
    pub base_commit: String,
    pub resulting_tree: String,
    pub provider_ref: String,
    pub evidence: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitCandidateInput {
    commit_id: String,
    tree_id: String,
    artifact_ref: ArtifactRef,
}

impl GitCandidateInput {
    #[must_use]
    pub fn commit_id(&self) -> &str {
        &self.commit_id
    }

    #[must_use]
    pub fn tree_id(&self) -> &str {
        &self.tree_id
    }

    #[must_use]
    pub const fn artifact_ref(&self) -> &ArtifactRef {
        &self.artifact_ref
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateComposition {
    repository_id: RepositoryId,
    base_commit: String,
    tip_commit: String,
    resulting_tree: String,
    changed_paths: Vec<String>,
    overlapping_paths: Vec<String>,
    inputs: Vec<GitCandidateInput>,
    evidence: String,
}

impl CandidateComposition {
    #[must_use]
    pub const fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    #[must_use]
    pub fn base_commit(&self) -> &str {
        &self.base_commit
    }

    #[must_use]
    pub fn tip_commit(&self) -> &str {
        &self.tip_commit
    }

    #[must_use]
    pub fn resulting_tree(&self) -> &str {
        &self.resulting_tree
    }

    #[must_use]
    pub fn changed_paths(&self) -> &[String] {
        &self.changed_paths
    }

    #[must_use]
    pub fn overlapping_paths(&self) -> &[String] {
        &self.overlapping_paths
    }

    #[must_use]
    pub fn inputs(&self) -> &[GitCandidateInput] {
        &self.inputs
    }

    #[must_use]
    pub fn evidence(&self) -> &str {
        &self.evidence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationPlan {
    repository_id: RepositoryId,
    provider_locator: PathBuf,
    candidate_inputs: Vec<GitCandidateInput>,
    target_ref: String,
    expected_target: String,
    candidate_tree: String,
    effect_operation_id: String,
}

impl IntegrationPlan {
    #[must_use]
    pub const fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    #[must_use]
    pub fn target_ref(&self) -> &str {
        &self.target_ref
    }

    #[must_use]
    pub fn expected_target(&self) -> &str {
        &self.expected_target
    }

    #[must_use]
    pub fn candidate_tree(&self) -> &str {
        &self.candidate_tree
    }

    #[must_use]
    pub fn effect_operation_id(&self) -> &str {
        &self.effect_operation_id
    }

    #[must_use]
    pub fn candidate_inputs(&self) -> &[GitCandidateInput] {
        &self.candidate_inputs
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationResult {
    pub prior_target: String,
    pub result_revision: String,
    pub result_tree: String,
    pub evidence: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconciliationResult {
    ResultVerified(IntegrationResult),
    Diverged {
        observed_target: String,
        evidence: String,
    },
    StillUncertain {
        evidence: String,
    },
}

#[derive(Clone, Debug)]
pub struct NativeGit {
    git_binary: PathBuf,
    policy: CommandPolicy,
}

impl NativeGit {
    #[must_use]
    pub fn new(git_binary: impl Into<PathBuf>, timeout: Duration, max_output_bytes: usize) -> Self {
        Self {
            git_binary: git_binary.into(),
            policy: CommandPolicy {
                timeout,
                max_output_bytes,
            },
        }
    }

    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new("git", Duration::from_secs(30), 16 * 1024 * 1024)
    }

    /// Discovers one worktree and its version-gated local capabilities.
    ///
    /// # Errors
    ///
    /// Fails when Git is unavailable, unsupported, bounded execution fails, or
    /// the path is not a worktree.
    pub fn discover(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<RepositoryDiscovery, GitProviderError> {
        let path = path.as_ref();
        let version_output = self.command(None, "version", [OsStr::new("--version")], None)?;
        ensure_success(&version_output, "version")?;
        let git_version = one_line(&version_output.stdout, "version")?;
        validate_version(&git_version)?;

        let root_output = self.command(
            Some(path),
            "discover-worktree",
            [OsStr::new("rev-parse"), OsStr::new("--show-toplevel")],
            None,
        )?;
        if !root_output.status.success() {
            return Err(GitProviderError::RepositoryNotFound(path.to_path_buf()));
        }
        let worktree_root =
            canonicalize_reported(path, &one_line(&root_output.stdout, "discover-worktree")?)?;
        let common_output = self.command(
            Some(&worktree_root),
            "discover-common-dir",
            [OsStr::new("rev-parse"), OsStr::new("--git-common-dir")],
            None,
        )?;
        ensure_success(&common_output, "discover-common-dir")?;
        let common_git_directory = canonicalize_reported(
            &worktree_root,
            &one_line(&common_output.stdout, "discover-common-dir")?,
        )?;
        let format_output = self.command(
            Some(&worktree_root),
            "discover-object-format",
            [OsStr::new("rev-parse"), OsStr::new("--show-object-format")],
            None,
        )?;
        ensure_success(&format_output, "discover-object-format")?;
        let object_format = one_line(&format_output.stdout, "discover-object-format")?;
        if object_format != "sha1" && object_format != "sha256" {
            return Err(GitProviderError::Unsupported {
                capability: "object-format",
                reason: object_format,
            });
        }
        Ok(RepositoryDiscovery {
            provider_locator_evidence: evidence(
                "discover-repository",
                &[&common_git_directory.display().to_string(), &object_format],
            ),
            worktree_root,
            common_git_directory,
            object_format,
            git_version,
            capabilities: GitCapabilities::local_v1(),
        })
    }

    /// Resolves and verifies one exact commit and tree.
    ///
    /// # Errors
    ///
    /// Fails when the revision is absent, ambiguous, or Git fails closed.
    pub fn inspect_revision(
        &self,
        repository: &Path,
        revision: &str,
    ) -> Result<RevisionObservation, GitProviderError> {
        let commit_id = self.resolve(repository, revision, "commit")?;
        let tree_id = self.resolve(repository, &commit_id, "tree")?;
        Ok(RevisionObservation {
            evidence: evidence("inspect", &[&commit_id, &tree_id]),
            commit_id,
            tree_id,
        })
    }

    /// Produces domain-compatible evidence for one exact local target ref.
    ///
    /// # Errors
    ///
    /// Fails for unsafe refs, missing commits, invalid output, or command failure.
    pub fn observe_target(
        &self,
        repository: &Path,
        target_ref: &str,
    ) -> Result<TargetObservation, GitProviderError> {
        self.validate_target_ref(repository, target_ref)?;
        let observed = self.resolve(repository, target_ref, "commit")?;
        Ok(TargetObservation::new(
            TargetRef::new(target_ref).map_err(domain_error)?,
            TargetRevision::new(observed.clone()).map_err(domain_error)?,
            IntegrationEvidence::new(evidence("observe-target", &[target_ref, &observed]))
                .map_err(domain_error)?,
        ))
    }

    /// Captures an exact commit delta into provider-independent storage.
    ///
    /// # Errors
    ///
    /// Fails for unsupported paths or modes, empty deltas, corrupt objects,
    /// invalid domain values, artifact errors, or bounded command failures.
    pub fn capture_revision(
        &self,
        repository: &Path,
        repository_id: RepositoryId,
        base_revision: &str,
        revision: &str,
        artifacts: &ArtifactStore,
    ) -> Result<CapturedRevision, GitProviderError> {
        let base = self.inspect_revision(repository, base_revision)?;
        let observation = self.inspect_revision(repository, revision)?;
        let changed = self.changed_status(repository, &base.commit_id, &observation.commit_id)?;
        if changed.is_empty() {
            return Err(GitProviderError::Unsupported {
                capability: "canonical-capture",
                reason: "no-op revisions have no tree-delta-v1 operations".to_owned(),
            });
        }
        let mut operations = Vec::with_capacity(changed.len());
        for (status, path) in &changed {
            if *status == b'D' {
                operations.push(PathOperation::Delete { path: path.clone() });
                continue;
            }
            let (mode, blob_id) = self.tree_entry(repository, &observation.commit_id, path)?;
            let blob = self.cat_blob(repository, &blob_id)?;
            let digest = artifacts.store_blob(&blob)?;
            operations.push(PathOperation::Upsert {
                path: path.clone(),
                mode,
                blob_digest: digest.as_str().to_owned(),
            });
        }
        operations.sort_by(|left, right| left.path().cmp(right.path()));
        let delta = TreeDelta::new(operations).map_err(domain_error)?;
        let manifest = CanonicalTreeDelta::new(
            BaseState::new(repository_id, base.commit_id).map_err(domain_error)?,
            delta,
        );
        let artifact_ref = artifacts.store_manifest(&manifest)?;
        Ok(CapturedRevision {
            observation,
            artifact_ref,
            changed_paths: changed.into_iter().map(|(_, path)| path).collect(),
        })
    }

    /// Creates a detached exact-base worktree and applies one artifact.
    ///
    /// # Errors
    ///
    /// Fails if the destination exists or exact-base, artifact, filesystem, Git,
    /// or resulting-tree verification fails.
    pub fn materialize(
        &self,
        repository: &Path,
        repository_id: &RepositoryId,
        revision: &CapturedRevision,
        artifacts: &ArtifactStore,
        destination: &Path,
    ) -> Result<MaterializationResult, GitProviderError> {
        if destination.exists() {
            return Err(GitProviderError::DestinationExists(
                destination.to_path_buf(),
            ));
        }
        let manifest = artifacts.load_manifest(&revision.artifact_ref)?;
        verify_repository_id(repository_id, manifest.base().repository_id())?;
        let base = self.inspect_revision(repository, manifest.base().object_id())?;
        let base_commit = base.commit_id;
        self.add_worktree(repository, destination, &base_commit)?;
        let result = (|| {
            self.apply_manifest(destination, &manifest, &base.tree_id, artifacts)?;
            let resulting_tree = self.write_tree(destination)?;
            if resulting_tree != revision.observation.tree_id {
                return Err(GitProviderError::VerificationFailed(format!(
                    "materialized tree {resulting_tree} does not match exact revision tree {}",
                    revision.observation.tree_id
                )));
            }
            Ok(MaterializationResult {
                path: destination.to_path_buf(),
                base_commit: base_commit.clone(),
                provider_ref: format!("worktree:{}", destination.display()),
                evidence: evidence("materialize", &[&base_commit, &resulting_tree]),
                resulting_tree,
            })
        })();
        if result.is_err() {
            let _ = self.remove_worktree(repository, destination, true);
        }
        result
    }

    /// Applies exact chained artifacts in order and reports path overlaps.
    ///
    /// # Errors
    ///
    /// Fails for empty inputs, an existing destination, a non-chained base tree,
    /// or any artifact, filesystem, command, or verification failure.
    pub fn compose_candidate(
        &self,
        repository: &Path,
        repository_id: &RepositoryId,
        ordered_revisions: &[CapturedRevision],
        artifacts: &ArtifactStore,
        destination: &Path,
    ) -> Result<CandidateComposition, GitProviderError> {
        let first = ordered_revisions
            .first()
            .ok_or_else(|| GitProviderError::Unsupported {
                capability: "candidate-composition",
                reason: "candidate has no exact revision artifacts".to_owned(),
            })?;
        if destination.exists() {
            return Err(GitProviderError::DestinationExists(
                destination.to_path_buf(),
            ));
        }
        let first_manifest = artifacts.load_manifest(&first.artifact_ref)?;
        verify_repository_id(repository_id, first_manifest.base().repository_id())?;
        let base_commit = self
            .inspect_revision(repository, first_manifest.base().object_id())?
            .commit_id;
        self.add_worktree(repository, destination, &base_commit)?;
        let result = (|| {
            let mut changed = BTreeSet::new();
            let mut overlaps = BTreeSet::new();
            let mut prior_revision: Option<&RevisionObservation> = None;
            for revision in ordered_revisions {
                let manifest = artifacts.load_manifest(&revision.artifact_ref)?;
                verify_repository_id(repository_id, manifest.base().repository_id())?;
                let expected_base_tree = if let Some(prior) = prior_revision {
                    if manifest.base().object_id() != prior.commit_id {
                        return Err(GitProviderError::VerificationFailed(format!(
                            "ordered input base commit {} does not equal prior exact revision {}",
                            manifest.base().object_id(),
                            prior.commit_id
                        )));
                    }
                    prior.tree_id.clone()
                } else {
                    self.inspect_revision(repository, manifest.base().object_id())?
                        .tree_id
                };
                let current_tree = self.write_tree(destination)?;
                if current_tree != expected_base_tree {
                    return Err(GitProviderError::VerificationFailed(format!(
                        "ordered input base tree {expected_base_tree} does not match composed tree {current_tree}"
                    )));
                }
                for operation in manifest.delta().operations() {
                    if !changed.insert(operation.path().to_owned()) {
                        overlaps.insert(operation.path().to_owned());
                    }
                }
                self.apply_manifest(destination, &manifest, &expected_base_tree, artifacts)?;
                let resulting_tree = self.write_tree(destination)?;
                if resulting_tree != revision.observation.tree_id {
                    return Err(GitProviderError::VerificationFailed(format!(
                        "composed input tree {resulting_tree} does not match exact revision tree {}",
                        revision.observation.tree_id
                    )));
                }
                prior_revision = Some(&revision.observation);
            }
            let resulting_tree = self.write_tree(destination)?;
            Ok(CandidateComposition {
                repository_id: repository_id.clone(),
                base_commit: base_commit.clone(),
                tip_commit: ordered_revisions
                    .last()
                    .map(|revision| revision.observation.commit_id.clone())
                    .unwrap_or_default(),
                resulting_tree: resulting_tree.clone(),
                changed_paths: changed.into_iter().collect(),
                overlapping_paths: overlaps.into_iter().collect(),
                inputs: ordered_revisions
                    .iter()
                    .map(|revision| GitCandidateInput {
                        commit_id: revision.observation.commit_id.clone(),
                        tree_id: revision.observation.tree_id.clone(),
                        artifact_ref: revision.artifact_ref.clone(),
                    })
                    .collect(),
                evidence: evidence("compose", &[&base_commit, &resulting_tree]),
            })
        })();
        if result.is_err() {
            let _ = self.remove_worktree(repository, destination, true);
        }
        result
    }

    /// Reconstructs a previously sealed candidate tree from durable canonical
    /// artifacts after source provider commits may have disappeared.
    ///
    /// The caller supplies the persisted exact base commit and final tree from the
    /// original provider plan. Every artifact is reapplied in order, repository
    /// identity is rechecked, and the final tree must match before a sealed
    /// composition is returned.
    ///
    /// # Errors
    ///
    /// Fails for empty inputs, an existing destination, a missing exact first
    /// base, repository drift, invalid artifacts, or a final-tree mismatch.
    #[allow(clippy::too_many_arguments)]
    pub fn reconstruct_candidate(
        &self,
        repository: &Path,
        repository_id: &RepositoryId,
        base_commit: &str,
        ordered_artifacts: &[ArtifactRef],
        expected_tree: &str,
        artifacts: &ArtifactStore,
        destination: &Path,
    ) -> Result<CandidateComposition, GitProviderError> {
        let first = ordered_artifacts
            .first()
            .ok_or_else(|| GitProviderError::Unsupported {
                capability: "candidate-reconstruction",
                reason: "candidate has no exact revision artifacts".to_owned(),
            })?;
        if destination.exists() {
            return Err(GitProviderError::DestinationExists(
                destination.to_path_buf(),
            ));
        }
        let base = self.inspect_revision(repository, base_commit)?;
        let first_manifest = artifacts.load_manifest(first)?;
        verify_repository_id(repository_id, first_manifest.base().repository_id())?;
        if first_manifest.base().object_id() != base.commit_id {
            return Err(GitProviderError::VerificationFailed(
                "first canonical artifact base differs from the durable integration base"
                    .to_owned(),
            ));
        }
        self.add_worktree(repository, destination, &base.commit_id)?;
        let result = (|| {
            let mut changed = BTreeSet::new();
            let mut overlaps = BTreeSet::new();
            let mut current_tree = base.tree_id.clone();
            for artifact in ordered_artifacts {
                let manifest = artifacts.load_manifest(artifact)?;
                verify_repository_id(repository_id, manifest.base().repository_id())?;
                for operation in manifest.delta().operations() {
                    if !changed.insert(operation.path().to_owned()) {
                        overlaps.insert(operation.path().to_owned());
                    }
                }
                self.apply_manifest(destination, &manifest, &current_tree, artifacts)?;
                current_tree = self.write_tree(destination)?;
            }
            if current_tree != expected_tree {
                return Err(GitProviderError::VerificationFailed(format!(
                    "reconstructed candidate tree {current_tree} does not match durable tree {expected_tree}"
                )));
            }
            Ok(CandidateComposition {
                repository_id: repository_id.clone(),
                base_commit: base.commit_id.clone(),
                tip_commit: String::new(),
                resulting_tree: current_tree.clone(),
                changed_paths: changed.into_iter().collect(),
                overlapping_paths: overlaps.into_iter().collect(),
                inputs: Vec::new(),
                evidence: evidence("reconstruct-candidate", &[&base.commit_id, &current_tree]),
            })
        })();
        if result.is_err() {
            let _ = self.remove_worktree(repository, destination, true);
        }
        result
    }

    /// Re-observes a canonical detached materialization without changing it.
    ///
    /// # Errors
    ///
    /// Fails when expected identities are invalid or Git cannot provide bounded,
    /// exact HEAD, index, and worktree observations.
    pub fn observe_materialization(
        &self,
        worktree: &Path,
        expected_base_commit: &str,
        expected_tree: &str,
    ) -> Result<ProviderObservation, GitProviderError> {
        let head = self.resolve(worktree, "HEAD", "commit")?;
        let expected_base = self.resolve(worktree, expected_base_commit, "commit")?;
        let index_tree = self.write_tree(worktree)?;
        let expected_tree = self.resolve(worktree, expected_tree, "tree")?;
        let worktree_matches = self.worktree_matches_tree(worktree, &expected_tree)?;
        let untracked = self.command(
            Some(worktree),
            "observe-untracked",
            [
                OsStr::new("ls-files"),
                OsStr::new("--others"),
                OsStr::new("--exclude-standard"),
                OsStr::new("-z"),
            ],
            None,
        )?;
        ensure_success(&untracked, "observe-untracked")?;
        let state = if head != expected_base {
            MaterializationState::Diverged
        } else if index_tree != expected_tree || !worktree_matches || !untracked.stdout.is_empty() {
            MaterializationState::Dirty
        } else {
            MaterializationState::Clean
        };
        Ok(ProviderObservation::new(
            state,
            ProviderRef::new(format!("worktree:{}", worktree.display())).map_err(domain_error)?,
            ProviderEvidence::new(evidence(
                "observe-materialization",
                &[&head, &index_tree, state.as_str()],
            ))
            .map_err(domain_error)?,
        ))
    }

    fn worktree_matches_tree(
        &self,
        worktree: &Path,
        expected_tree: &str,
    ) -> Result<bool, GitProviderError> {
        let output = self.command(
            Some(worktree),
            "inspect-expected-worktree",
            [
                OsStr::new("ls-tree"),
                OsStr::new("-r"),
                OsStr::new("-z"),
                OsStr::new(expected_tree),
            ],
            None,
        )?;
        ensure_success(&output, "inspect-expected-worktree")?;
        for entry in parse_tree_entries(&output.stdout)? {
            if !safe_worktree_ancestors(worktree, &entry.path)? {
                return Ok(false);
            }
            let path = worktree.join(&entry.path);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error.into()),
            };
            if !mode_matches(entry.mode, &metadata) {
                return Ok(false);
            }
            let bytes = if entry.mode == FileMode::SymbolicLink {
                symlink_bytes(&path)?
            } else {
                fs::read(&path)?
            };
            let hash = self.command(
                Some(worktree),
                "hash-worktree-path",
                [
                    OsStr::new("hash-object"),
                    OsStr::new("--no-filters"),
                    OsStr::new("--stdin"),
                ],
                Some(&bytes),
            )?;
            ensure_success(&hash, "hash-worktree-path")?;
            if parse_object_id(&one_line(&hash.stdout, "hash-worktree-path")?)? != entry.object_id {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Releases a clean materialization through Git's registered worktree API.
    ///
    /// # Errors
    ///
    /// Fails when the worktree is dirty, absent, unregistered, or Git cannot
    /// complete the bounded removal. It never forces loss of caller work.
    pub fn release_materialization(
        &self,
        repository: &Path,
        worktree: &Path,
    ) -> Result<(), GitProviderError> {
        self.remove_worktree(repository, worktree, false)
    }

    /// Probes an exact commit pair in a disposable detached worktree.
    ///
    /// # Errors
    ///
    /// Fails if scratch exists, revisions are invalid, a merge failure cannot be
    /// normalized to paths, or cleanup fails.
    pub fn detect_merge_conflicts(
        &self,
        repository: &Path,
        left_revision: &str,
        right_revision: &str,
        scratch: &Path,
    ) -> Result<Vec<String>, GitProviderError> {
        if scratch.exists() {
            return Err(GitProviderError::DestinationExists(scratch.to_path_buf()));
        }
        let left = self.inspect_revision(repository, left_revision)?.commit_id;
        let right = self.inspect_revision(repository, right_revision)?.commit_id;
        self.add_worktree(repository, scratch, &right)?;
        let merge = self.command(
            Some(scratch),
            "conflict-probe",
            [
                OsStr::new("merge"),
                OsStr::new("--no-commit"),
                OsStr::new("--no-ff"),
                OsStr::new(&left),
            ],
            None,
        );
        let result = match merge {
            Ok(output) if output.status.success() => Ok(Vec::new()),
            Ok(output) => {
                let paths = self.unmerged_paths(scratch)?;
                if paths.is_empty() {
                    Err(GitProviderError::CommandFailed {
                        operation: "conflict-probe",
                        code: None,
                        redacted_stderr_bytes: output.stderr.len(),
                    })
                } else {
                    Ok(paths)
                }
            }
            Err(error) => Err(error),
        };
        let _ = self.command(
            Some(scratch),
            "abort-conflict-probe",
            [OsStr::new("merge"), OsStr::new("--abort")],
            None,
        );
        let cleanup = self.remove_worktree(repository, scratch, true);
        cleanup?;
        result
    }

    /// Freezes the exact target, candidate tree, and stable effect operation.
    ///
    /// # Errors
    ///
    /// Fails for unsafe refs, invalid objects, changed targets, or Git failures.
    pub fn plan_integration(
        &self,
        repository: &Path,
        repository_id: &RepositoryId,
        target_ref: &str,
        expected_target: &str,
        candidate: &CandidateComposition,
        effect_operation_id: &EffectOperationId,
    ) -> Result<IntegrationPlan, GitProviderError> {
        self.validate_target_ref(repository, target_ref)?;
        let observed = self.resolve(repository, target_ref, "commit")?;
        let expected = self.resolve(repository, expected_target, "commit")?;
        verify_repository_id(repository_id, candidate.repository_id())?;
        if candidate.base_commit() != expected {
            return Err(GitProviderError::VerificationFailed(format!(
                "candidate base {} does not equal expected target {expected}",
                candidate.base_commit()
            )));
        }
        if observed != expected {
            return Err(GitProviderError::ChangedTarget { expected, observed });
        }
        let tree = self.resolve(repository, candidate.resulting_tree(), "tree")?;
        let provider_locator = self.discover(repository)?.common_git_directory;
        Ok(IntegrationPlan {
            repository_id: repository_id.clone(),
            provider_locator,
            candidate_inputs: candidate.inputs().to_vec(),
            target_ref: target_ref.to_owned(),
            expected_target: expected,
            candidate_tree: tree,
            effect_operation_id: effect_operation_id.as_str().to_owned(),
        })
    }

    /// Rehydrates an exact immutable plan from caller-persisted sealed fields
    /// without requiring source provider commits or a pre-mutation live target.
    ///
    /// The locator evidence must be the exact evidence returned by discovery when
    /// the plan was created. The live repository is rediscovered and must match it.
    /// Live target classification remains the responsibility of
    /// [`Self::reconcile_integration`].
    ///
    /// # Errors
    ///
    /// Fails for unsafe refs, invalid exact objects, repository drift, or a
    /// candidate tree that is unavailable or malformed.
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate_integration_plan(
        &self,
        repository: &Path,
        repository_id: &RepositoryId,
        provider_locator_evidence: &str,
        target_ref: &str,
        expected_target: &str,
        candidate_tree: &str,
        effect_operation_id: &EffectOperationId,
    ) -> Result<IntegrationPlan, GitProviderError> {
        let discovery = self.discover(repository)?;
        if discovery.provider_locator_evidence != provider_locator_evidence {
            return Err(GitProviderError::VerificationFailed(
                "integration provider locator differs from the durable plan".to_owned(),
            ));
        }
        self.validate_target_ref(&discovery.worktree_root, target_ref)?;
        let expected = self.resolve(repository, expected_target, "commit")?;
        let tree = self.resolve(repository, candidate_tree, "tree")?;
        Ok(IntegrationPlan {
            repository_id: repository_id.clone(),
            provider_locator: discovery.common_git_directory,
            candidate_inputs: Vec::new(),
            target_ref: target_ref.to_owned(),
            expected_target: expected,
            candidate_tree: tree,
            effect_operation_id: effect_operation_id.as_str().to_owned(),
        })
    }

    /// Creates a squash commit and advances the target through exact ref CAS.
    ///
    /// # Errors
    ///
    /// Fails closed for a changed target, command ambiguity, output bounds, or a
    /// mismatch while re-observing the resulting commit and tree.
    pub fn execute_integration(
        &self,
        repository: &Path,
        repository_id: &RepositoryId,
        plan: &IntegrationPlan,
    ) -> Result<IntegrationResult, GitProviderError> {
        self.validate_plan_repository(repository, repository_id, plan)?;
        let observed = self.resolve(repository, &plan.target_ref, "commit")?;
        if observed != plan.expected_target {
            return Err(GitProviderError::ChangedTarget {
                expected: plan.expected_target.clone(),
                observed,
            });
        }
        let message = integration_message(&plan.effect_operation_id);
        let commit = self.command(
            Some(repository),
            "create-integration-commit",
            [
                OsStr::new("-c"),
                OsStr::new("commit.gpgSign=false"),
                OsStr::new("commit-tree"),
                OsStr::new(&plan.candidate_tree),
                OsStr::new("-p"),
                OsStr::new(&plan.expected_target),
            ],
            Some(message.as_bytes()),
        )?;
        ensure_success(&commit, "create-integration-commit")?;
        let result_revision =
            parse_object_id(&one_line(&commit.stdout, "create-integration-commit")?)?;
        let update = self.command(
            Some(repository),
            "guarded-target-update",
            [
                OsStr::new("update-ref"),
                OsStr::new(&plan.target_ref),
                OsStr::new(&result_revision),
                OsStr::new(&plan.expected_target),
            ],
            None,
        )?;
        if !update.status.success() {
            let observed = self.resolve(repository, &plan.target_ref, "commit")?;
            return if observed == plan.expected_target {
                Err(GitProviderError::CommandFailed {
                    operation: "guarded-target-update",
                    code: update.status.code(),
                    redacted_stderr_bytes: update.stderr.len(),
                })
            } else {
                Err(GitProviderError::ChangedTarget {
                    expected: plan.expected_target.clone(),
                    observed,
                })
            };
        }
        self.verify_result(repository, plan, &result_revision)
    }

    /// Classifies provider state after an uncertain integration execution.
    ///
    /// # Errors
    ///
    /// Fails when Git cannot produce a bounded trustworthy observation or an
    /// apparent result fails parent, tree, operation, or ref verification.
    pub fn reconcile_integration(
        &self,
        repository: &Path,
        repository_id: &RepositoryId,
        plan: &IntegrationPlan,
        result_hint: Option<&str>,
    ) -> Result<ReconciliationResult, GitProviderError> {
        self.validate_plan_repository(repository, repository_id, plan)?;
        let observed = match self.resolve(repository, &plan.target_ref, "commit") {
            Ok(value) => value,
            Err(GitProviderError::CommandFailed { .. }) => {
                return Ok(ReconciliationResult::StillUncertain {
                    evidence: evidence("reconcile-missing-target", &[&plan.target_ref]),
                });
            }
            Err(error) => return Err(error),
        };
        if observed == plan.expected_target {
            return Ok(ReconciliationResult::StillUncertain {
                evidence: evidence(
                    "reconcile-expected-target-still-uncertain",
                    &[&plan.target_ref, &observed],
                ),
            });
        }
        if result_hint.is_some_and(|hint| hint == observed)
            || self.is_exact_integration_result(repository, plan, &observed)?
        {
            return self
                .verify_result(repository, plan, &observed)
                .map(ReconciliationResult::ResultVerified);
        }
        Ok(ReconciliationResult::Diverged {
            evidence: evidence("reconcile-diverged", &[&plan.target_ref, &observed]),
            observed_target: observed,
        })
    }

    fn verify_result(
        &self,
        repository: &Path,
        plan: &IntegrationPlan,
        result_revision: &str,
    ) -> Result<IntegrationResult, GitProviderError> {
        let observed = self.resolve(repository, &plan.target_ref, "commit")?;
        if observed != result_revision {
            return Err(GitProviderError::VerificationFailed(format!(
                "target observed {observed} instead of result {result_revision}"
            )));
        }
        let result = self.inspect_revision(repository, result_revision)?;
        if result.tree_id != plan.candidate_tree {
            return Err(GitProviderError::VerificationFailed(format!(
                "result tree {} does not match candidate tree {}",
                result.tree_id, plan.candidate_tree
            )));
        }
        if !self.is_exact_integration_result(repository, plan, result_revision)? {
            return Err(GitProviderError::VerificationFailed(
                "result commit does not bind expected parent and effect operation".to_owned(),
            ));
        }
        Ok(IntegrationResult {
            prior_target: plan.expected_target.clone(),
            result_revision: result.commit_id.clone(),
            result_tree: result.tree_id,
            evidence: evidence(
                "verify-integration",
                &[
                    &plan.target_ref,
                    &result.commit_id,
                    &plan.effect_operation_id,
                ],
            ),
        })
    }

    fn validate_plan_repository(
        &self,
        repository: &Path,
        repository_id: &RepositoryId,
        plan: &IntegrationPlan,
    ) -> Result<(), GitProviderError> {
        verify_repository_id(repository_id, &plan.repository_id)?;
        self.validate_target_ref(repository, &plan.target_ref)?;
        let observed_locator = self.discover(repository)?.common_git_directory;
        if observed_locator != plan.provider_locator {
            return Err(GitProviderError::VerificationFailed(format!(
                "integration provider locator {} does not match planned locator {}",
                observed_locator.display(),
                plan.provider_locator.display()
            )));
        }
        Ok(())
    }

    fn is_exact_integration_result(
        &self,
        repository: &Path,
        plan: &IntegrationPlan,
        revision: &str,
    ) -> Result<bool, GitProviderError> {
        let output = self.command(
            Some(repository),
            "inspect-integration-commit",
            [
                OsStr::new("cat-file"),
                OsStr::new("commit"),
                OsStr::new(revision),
            ],
            None,
        )?;
        ensure_success(&output, "inspect-integration-commit")?;
        let text =
            std::str::from_utf8(&output.stdout).map_err(|_| GitProviderError::InvalidOutput {
                operation: "inspect-integration-commit",
                reason: "commit object is not UTF-8".to_owned(),
            })?;
        let (headers, message) =
            text.split_once("\n\n")
                .ok_or_else(|| GitProviderError::InvalidOutput {
                    operation: "inspect-integration-commit",
                    reason: "commit object has no header/message boundary".to_owned(),
                })?;
        let mut lines = headers.lines();
        let tree = format!("tree {}", plan.candidate_tree);
        let tree_matches = lines.next() == Some(tree.as_str());
        let parents: Vec<_> = lines
            .filter_map(|line| line.strip_prefix("parent "))
            .collect();
        let trailer = format!(
            "Weft-Effect-Operation-Hex: {}",
            hex_encode(&plan.effect_operation_id)
        );
        Ok(tree_matches
            && parents == [plan.expected_target.as_str()]
            && message.lines().any(|line| line == trailer))
    }

    fn changed_status(
        &self,
        repository: &Path,
        base: &str,
        revision: &str,
    ) -> Result<Vec<(u8, String)>, GitProviderError> {
        let output = self.command(
            Some(repository),
            "changed-paths",
            [
                OsStr::new("diff"),
                OsStr::new("--name-status"),
                OsStr::new("-z"),
                OsStr::new("--no-renames"),
                OsStr::new("--no-ext-diff"),
                OsStr::new("--no-textconv"),
                OsStr::new(base),
                OsStr::new(revision),
                OsStr::new("--"),
            ],
            None,
        )?;
        ensure_success(&output, "changed-paths")?;
        parse_name_status(&output.stdout)
    }

    fn tree_entry(
        &self,
        repository: &Path,
        revision: &str,
        path: &str,
    ) -> Result<(FileMode, String), GitProviderError> {
        let args = vec![
            OsString::from("ls-tree"),
            OsString::from("-z"),
            OsString::from(revision),
            OsString::from("--"),
            OsString::from(path),
        ];
        let output = self.command(Some(repository), "inspect-tree-entry", args, None)?;
        ensure_success(&output, "inspect-tree-entry")?;
        parse_tree_entry(&output.stdout, path)
    }

    fn cat_blob(&self, repository: &Path, object_id: &str) -> Result<Vec<u8>, GitProviderError> {
        let output = self.command(
            Some(repository),
            "read-blob",
            [
                OsStr::new("cat-file"),
                OsStr::new("blob"),
                OsStr::new(object_id),
            ],
            None,
        )?;
        ensure_success(&output, "read-blob")?;
        Ok(output.stdout)
    }

    fn apply_manifest(
        &self,
        worktree: &Path,
        manifest: &CanonicalTreeDelta,
        expected_base_tree: &str,
        artifacts: &ArtifactStore,
    ) -> Result<(), GitProviderError> {
        let current = self.write_tree(worktree)?;
        if current != expected_base_tree {
            return Err(GitProviderError::VerificationFailed(format!(
                "materialization tree {current} does not match artifact base tree {expected_base_tree}"
            )));
        }
        for operation in manifest.delta().operations() {
            let PathOperation::Delete { .. } = operation else {
                continue;
            };
            let path = worktree.join(operation.path());
            remove_path(&path)?;
            self.remove_index_path(worktree, operation.path())?;
        }
        for operation in manifest.delta().operations() {
            let PathOperation::Upsert {
                mode, blob_digest, ..
            } = operation
            else {
                continue;
            };
            let bytes = artifacts.load_blob(blob_digest)?;
            let path = worktree.join(operation.path());
            write_path(&path, *mode, &bytes)?;
            self.stage_raw_blob(worktree, operation.path(), *mode, &bytes)?;
        }
        Ok(())
    }

    fn remove_index_path(&self, worktree: &Path, path: &str) -> Result<(), GitProviderError> {
        let output = self.command(
            Some(worktree),
            "remove-index-path",
            [
                OsStr::new("update-index"),
                OsStr::new("--force-remove"),
                OsStr::new("--"),
                OsStr::new(path),
            ],
            None,
        )?;
        ensure_success(&output, "remove-index-path")
    }

    fn stage_raw_blob(
        &self,
        worktree: &Path,
        path: &str,
        mode: FileMode,
        bytes: &[u8],
    ) -> Result<(), GitProviderError> {
        let hash = self.command(
            Some(worktree),
            "store-provider-blob",
            [
                OsStr::new("hash-object"),
                OsStr::new("-w"),
                OsStr::new("--stdin"),
            ],
            Some(bytes),
        )?;
        ensure_success(&hash, "store-provider-blob")?;
        let object_id = parse_object_id(&one_line(&hash.stdout, "store-provider-blob")?)?;
        let mode = match mode {
            FileMode::Regular => "100644",
            FileMode::Executable => "100755",
            FileMode::SymbolicLink => "120000",
        };
        let output = self.command(
            Some(worktree),
            "stage-provider-blob",
            [
                OsStr::new("update-index"),
                OsStr::new("--add"),
                OsStr::new("--cacheinfo"),
                OsStr::new(mode),
                OsStr::new(&object_id),
                OsStr::new(path),
            ],
            None,
        )?;
        ensure_success(&output, "stage-provider-blob")
    }

    fn add_worktree(
        &self,
        repository: &Path,
        destination: &Path,
        revision: &str,
    ) -> Result<(), GitProviderError> {
        let args = vec![
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("--detach"),
            destination.as_os_str().to_os_string(),
            OsString::from(revision),
        ];
        let output = self.command(Some(repository), "add-worktree", args, None)?;
        ensure_success(&output, "add-worktree")
    }

    fn remove_worktree(
        &self,
        repository: &Path,
        path: &Path,
        force: bool,
    ) -> Result<(), GitProviderError> {
        let mut args = vec![OsString::from("worktree"), OsString::from("remove")];
        if force {
            args.push(OsString::from("--force"));
        }
        args.push(path.as_os_str().to_os_string());
        let output = self.command(Some(repository), "remove-worktree", args, None)?;
        ensure_success(&output, "remove-worktree")
    }

    fn write_tree(&self, repository: &Path) -> Result<String, GitProviderError> {
        let output = self.command(
            Some(repository),
            "write-tree",
            [OsStr::new("write-tree")],
            None,
        )?;
        ensure_success(&output, "write-tree")?;
        parse_object_id(&one_line(&output.stdout, "write-tree")?)
    }

    fn unmerged_paths(&self, repository: &Path) -> Result<Vec<String>, GitProviderError> {
        let output = self.command(
            Some(repository),
            "list-conflicts",
            [
                OsStr::new("diff"),
                OsStr::new("--name-only"),
                OsStr::new("--diff-filter=U"),
                OsStr::new("-z"),
            ],
            None,
        )?;
        ensure_success(&output, "list-conflicts")?;
        parse_nul_paths(&output.stdout, "list-conflicts")
    }

    fn validate_target_ref(
        &self,
        repository: &Path,
        target_ref: &str,
    ) -> Result<(), GitProviderError> {
        if !target_ref.starts_with("refs/heads/") {
            return Err(GitProviderError::UnsafeTargetRef(target_ref.to_owned()));
        }
        let output = self.command(
            Some(repository),
            "validate-target-ref",
            [
                OsStr::new("check-ref-format"),
                OsStr::new("--branch"),
                OsStr::new(&target_ref[11..]),
            ],
            None,
        )?;
        if output.status.success() {
            Ok(())
        } else {
            Err(GitProviderError::UnsafeTargetRef(target_ref.to_owned()))
        }
    }

    fn resolve(
        &self,
        repository: &Path,
        revision: &str,
        kind: &'static str,
    ) -> Result<String, GitProviderError> {
        let expression = match kind {
            "commit" => format!("{revision}^{{commit}}"),
            "tree" => format!("{revision}^{{tree}}"),
            _ => revision.to_owned(),
        };
        let output = self.command(
            Some(repository),
            "resolve-object",
            [
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new(&expression),
            ],
            None,
        )?;
        ensure_success(&output, "resolve-object")?;
        parse_object_id(&one_line(&output.stdout, "resolve-object")?)
    }

    fn command<I, S>(
        &self,
        directory: Option<&Path>,
        operation: &'static str,
        args: I,
        input: Option<&[u8]>,
    ) -> Result<CommandOutput, GitProviderError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run_git(
            &self.git_binary,
            directory,
            operation,
            args,
            input,
            self.policy,
        )
    }
}

fn ensure_success(output: &CommandOutput, operation: &'static str) -> Result<(), GitProviderError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(GitProviderError::CommandFailed {
            operation,
            code: output.status.code(),
            redacted_stderr_bytes: output.stderr.len(),
        })
    }
}

fn validate_version(value: &str) -> Result<(), GitProviderError> {
    let version =
        value
            .strip_prefix("git version ")
            .ok_or_else(|| GitProviderError::InvalidOutput {
                operation: "version",
                reason: "missing git version prefix".to_owned(),
            })?;
    let mut parts = version.split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    if !matches!((major, minor), (Some(major), Some(minor)) if major > MINIMUM_GIT_MAJOR || (major == MINIMUM_GIT_MAJOR && minor >= MINIMUM_GIT_MINOR))
    {
        return Err(GitProviderError::Unsupported {
            capability: "git-version",
            reason: format!(
                "requires Git >= {MINIMUM_GIT_MAJOR}.{MINIMUM_GIT_MINOR}, observed {value}"
            ),
        });
    }
    Ok(())
}

fn canonicalize_reported(base: &Path, value: &str) -> Result<PathBuf, GitProviderError> {
    let path = Path::new(value);
    Ok(fs::canonicalize(if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    })?)
}

fn one_line(bytes: &[u8], operation: &'static str) -> Result<String, GitProviderError> {
    let value = std::str::from_utf8(bytes).map_err(|_| GitProviderError::InvalidOutput {
        operation,
        reason: "output is not UTF-8".to_owned(),
    })?;
    let value = value.trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.contains(['\r', '\n', '\0']) {
        return Err(GitProviderError::InvalidOutput {
            operation,
            reason: "expected exactly one non-empty line".to_owned(),
        });
    }
    Ok(value.to_owned())
}

fn parse_object_id(value: &str) -> Result<String, GitProviderError> {
    if matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value.to_owned())
    } else {
        Err(GitProviderError::InvalidOutput {
            operation: "parse-object-id",
            reason: "non-canonical object identity".to_owned(),
        })
    }
}

fn parse_name_status(bytes: &[u8]) -> Result<Vec<(u8, String)>, GitProviderError> {
    let fields: Vec<&[u8]> = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect();
    if !fields.len().is_multiple_of(2) {
        return Err(GitProviderError::InvalidOutput {
            operation: "changed-paths",
            reason: "unpaired status/path fields".to_owned(),
        });
    }
    let mut result = Vec::with_capacity(fields.len() / 2);
    for pair in fields.chunks_exact(2) {
        let status = *pair[0]
            .first()
            .ok_or_else(|| GitProviderError::InvalidOutput {
                operation: "changed-paths",
                reason: "empty status".to_owned(),
            })?;
        if !matches!(status, b'A' | b'M' | b'D' | b'T') {
            return Err(GitProviderError::Unsupported {
                capability: "canonical-capture",
                reason: format!("unsupported diff status {}", char::from(status)),
            });
        }
        let path = std::str::from_utf8(pair[1]).map_err(|_| GitProviderError::Unsupported {
            capability: "canonical-path",
            reason: "non-UTF-8 repository paths are not representable in tree-delta-v1".to_owned(),
        })?;
        result.push((status, path.to_owned()));
    }
    Ok(result)
}

fn parse_tree_entry(
    bytes: &[u8],
    expected_path: &str,
) -> Result<(FileMode, String), GitProviderError> {
    let value = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    let separator = value
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| GitProviderError::InvalidOutput {
            operation: "inspect-tree-entry",
            reason: "missing tree-entry separator".to_owned(),
        })?;
    let (metadata, path_with_separator) = value.split_at(separator);
    let path = &path_with_separator[1..];
    if path != expected_path.as_bytes() {
        return Err(GitProviderError::InvalidOutput {
            operation: "inspect-tree-entry",
            reason: "tree entry path mismatch".to_owned(),
        });
    }
    let metadata = std::str::from_utf8(metadata).map_err(|_| GitProviderError::InvalidOutput {
        operation: "inspect-tree-entry",
        reason: "tree entry metadata is not UTF-8".to_owned(),
    })?;
    let mut fields = metadata.split(' ');
    let mode = match fields.next() {
        Some("100644") => FileMode::Regular,
        Some("100755") => FileMode::Executable,
        Some("120000") => FileMode::SymbolicLink,
        Some(value) => {
            return Err(GitProviderError::Unsupported {
                capability: "canonical-file-mode",
                reason: value.to_owned(),
            });
        }
        None => {
            return Err(GitProviderError::InvalidOutput {
                operation: "inspect-tree-entry",
                reason: "missing mode".to_owned(),
            });
        }
    };
    if fields.next() != Some("blob") {
        return Err(GitProviderError::Unsupported {
            capability: "canonical-file-type",
            reason: "tree entry is not a blob".to_owned(),
        });
    }
    let object_id = fields
        .next()
        .ok_or_else(|| GitProviderError::InvalidOutput {
            operation: "inspect-tree-entry",
            reason: "missing blob identity".to_owned(),
        })?;
    if fields.next().is_some() {
        return Err(GitProviderError::InvalidOutput {
            operation: "inspect-tree-entry",
            reason: "unexpected tree-entry metadata".to_owned(),
        });
    }
    Ok((mode, parse_object_id(object_id)?))
}

#[derive(Debug)]
struct TreeEntry {
    mode: FileMode,
    object_id: String,
    path: String,
}

fn parse_tree_entries(bytes: &[u8]) -> Result<Vec<TreeEntry>, GitProviderError> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let separator = entry
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(|| GitProviderError::InvalidOutput {
                    operation: "inspect-expected-worktree",
                    reason: "missing tree-entry separator".to_owned(),
                })?;
            let metadata = std::str::from_utf8(&entry[..separator]).map_err(|_| {
                GitProviderError::InvalidOutput {
                    operation: "inspect-expected-worktree",
                    reason: "tree metadata is not UTF-8".to_owned(),
                }
            })?;
            let path = std::str::from_utf8(&entry[separator + 1..]).map_err(|_| {
                GitProviderError::Unsupported {
                    capability: "canonical-path",
                    reason: "expected tree contains a non-UTF-8 path".to_owned(),
                }
            })?;
            let mut fields = metadata.split(' ');
            let mode = parse_mode(fields.next())?;
            if fields.next() != Some("blob") {
                return Err(GitProviderError::Unsupported {
                    capability: "canonical-file-type",
                    reason: format!("non-blob entry at {path}"),
                });
            }
            let object_id = fields
                .next()
                .ok_or_else(|| GitProviderError::InvalidOutput {
                    operation: "inspect-expected-worktree",
                    reason: "missing object identity".to_owned(),
                })?;
            if fields.next().is_some() {
                return Err(GitProviderError::InvalidOutput {
                    operation: "inspect-expected-worktree",
                    reason: "unexpected tree metadata".to_owned(),
                });
            }
            Ok(TreeEntry {
                mode,
                object_id: parse_object_id(object_id)?,
                path: path.to_owned(),
            })
        })
        .collect()
}

fn parse_mode(value: Option<&str>) -> Result<FileMode, GitProviderError> {
    match value {
        Some("100644") => Ok(FileMode::Regular),
        Some("100755") => Ok(FileMode::Executable),
        Some("120000") => Ok(FileMode::SymbolicLink),
        Some(value) => Err(GitProviderError::Unsupported {
            capability: "canonical-file-mode",
            reason: value.to_owned(),
        }),
        None => Err(GitProviderError::InvalidOutput {
            operation: "inspect-expected-worktree",
            reason: "missing file mode".to_owned(),
        }),
    }
}

fn parse_nul_paths(bytes: &[u8], operation: &'static str) -> Result<Vec<String>, GitProviderError> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path).map(str::to_owned).map_err(|_| {
                GitProviderError::Unsupported {
                    capability: "canonical-path",
                    reason: format!("{operation} returned a non-UTF-8 path"),
                }
            })
        })
        .collect()
}

fn remove_path(path: &Path) -> Result<(), GitProviderError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(GitProviderError::VerificationFailed(format!(
                "delete path is absent from exact base: {}",
                path.display()
            )));
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn safe_worktree_ancestors(root: &Path, relative: &str) -> Result<bool, GitProviderError> {
    let mut current = root.to_path_buf();
    let Some(parent) = Path::new(relative).parent() else {
        return Ok(true);
    };
    for component in parent.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(GitProviderError::VerificationFailed(format!(
                "non-normal worktree path during observation: {relative}"
            )));
        };
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn write_path(path: &Path, mode: FileMode, bytes: &[u8]) -> Result<(), GitProviderError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match mode {
        FileMode::Regular | FileMode::Executable => {
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            set_mode(path, mode)?;
        }
        FileMode::SymbolicLink => create_symlink(bytes, path)?,
    }
    Ok(())
}

#[cfg(unix)]
fn mode_matches(mode: FileMode, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    match mode {
        FileMode::SymbolicLink => metadata.file_type().is_symlink(),
        FileMode::Regular => {
            metadata.file_type().is_file() && metadata.permissions().mode() & 0o111 == 0
        }
        FileMode::Executable => {
            metadata.file_type().is_file() && metadata.permissions().mode() & 0o111 != 0
        }
    }
}

#[cfg(not(unix))]
fn mode_matches(mode: FileMode, metadata: &fs::Metadata) -> bool {
    match mode {
        FileMode::SymbolicLink => metadata.file_type().is_symlink(),
        FileMode::Regular | FileMode::Executable => metadata.file_type().is_file(),
    }
}

#[cfg(unix)]
fn symlink_bytes(path: &Path) -> Result<Vec<u8>, GitProviderError> {
    use std::os::unix::ffi::OsStrExt;

    Ok(fs::read_link(path)?.as_os_str().as_bytes().to_vec())
}

#[cfg(not(unix))]
fn symlink_bytes(path: &Path) -> Result<Vec<u8>, GitProviderError> {
    fs::read_link(path)?
        .to_str()
        .map(|value| value.as_bytes().to_vec())
        .ok_or_else(|| GitProviderError::Unsupported {
            capability: "symbolic-link-materialization",
            reason: path.display().to_string(),
        })
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: FileMode) -> Result<(), GitProviderError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(if mode == FileMode::Executable {
        0o755
    } else {
        0o644
    });
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: FileMode) -> Result<(), GitProviderError> {
    Ok(())
}

#[cfg(unix)]
fn create_symlink(bytes: &[u8], path: &Path) -> Result<(), GitProviderError> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    if bytes.contains(&0) {
        return Err(GitProviderError::VerificationFailed(
            "symbolic-link target contains NUL".to_owned(),
        ));
    }
    symlink(OsStr::from_bytes(bytes), path)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_symlink(_bytes: &[u8], path: &Path) -> Result<(), GitProviderError> {
    Err(GitProviderError::Unsupported {
        capability: "symbolic-link-materialization",
        reason: path.display().to_string(),
    })
}

fn integration_message(effect_operation_id: &str) -> String {
    format!(
        "Weft integration\n\nWeft-Effect-Operation-Hex: {}\n",
        hex_encode(effect_operation_id)
    )
}

fn evidence(operation: &str, values: &[&str]) -> String {
    let mut fields = BTreeMap::new();
    fields.insert("operation", operation);
    for (index, value) in values.iter().enumerate() {
        fields.insert(
            match index {
                0 => "value0",
                1 => "value1",
                2 => "value2",
                _ => "value",
            },
            value,
        );
    }
    let details = fields
        .into_iter()
        .map(|(key, value)| format!("{key}={}", hex_encode(value)))
        .collect::<Vec<_>>()
        .join(";");
    format!("provider={PROVIDER_ID};schema=v1;{details}")
}

fn hex_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn domain_error(error: impl std::fmt::Display) -> GitProviderError {
    GitProviderError::Domain(error.to_string())
}

fn verify_repository_id(
    expected: &RepositoryId,
    observed: &RepositoryId,
) -> Result<(), GitProviderError> {
    if expected == observed {
        Ok(())
    } else {
        Err(GitProviderError::VerificationFailed(format!(
            "artifact repository {} does not match requested repository {}",
            observed.as_str(),
            expected.as_str()
        )))
    }
}
