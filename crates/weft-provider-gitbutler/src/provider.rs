use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use weft_artifact::ArtifactStore;
use weft_domain::{
    EffectOperationId, MaterializationState, ProviderEvidence, ProviderObservation, ProviderRef,
    RepositoryId,
};
use weft_provider_git::{CapturedRevision, NativeGit};

use crate::GitButlerProviderError;
use crate::command::{CommandOutput, CommandPolicy, run};
use crate::schema::{BranchJson, CommitJson, StackJson, StatusJson, UncommittedChangeJson};

const SUPPORTED_VERSION: &str = "0.22.0";
const PROVIDER_ID: &str = "gitbutler";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GitButlerCapability {
    StatusInspection,
    ParallelMaterializations,
    StackMapping,
    CanonicalExport,
    ConflictMapping,
    ExternalStateReconciliation,
    GuardedLocalFastForwardLanding,
    CanonicalImport,
    ProviderReconnect,
    RemoteLanding,
}

impl GitButlerCapability {
    const fn name(self) -> &'static str {
        match self {
            Self::StatusInspection => "status-inspection",
            Self::ParallelMaterializations => "parallel-materializations",
            Self::StackMapping => "stack-mapping",
            Self::CanonicalExport => "canonical-export",
            Self::ConflictMapping => "conflict-mapping",
            Self::ExternalStateReconciliation => "external-state-reconciliation",
            Self::GuardedLocalFastForwardLanding => "guarded-local-fast-forward-landing",
            Self::CanonicalImport => "canonical-import",
            Self::ProviderReconnect => "provider-reconnect",
            Self::RemoteLanding => "remote-landing",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerCapabilities(BTreeSet<GitButlerCapability>);

impl GitButlerCapabilities {
    fn v0_22(local_target: bool) -> Self {
        let mut values = BTreeSet::from([
            GitButlerCapability::StatusInspection,
            GitButlerCapability::ParallelMaterializations,
            GitButlerCapability::StackMapping,
            GitButlerCapability::CanonicalExport,
            GitButlerCapability::ConflictMapping,
            GitButlerCapability::ExternalStateReconciliation,
        ]);
        if local_target {
            values.insert(GitButlerCapability::GuardedLocalFastForwardLanding);
        }
        Self(values)
    }

    #[must_use]
    pub fn supports(&self, capability: GitButlerCapability) -> bool {
        self.0.contains(&capability)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerDiscovery {
    repository_id: RepositoryId,
    worktree_root: PathBuf,
    common_git_directory: PathBuf,
    version: String,
    target_ref: String,
    local_target: bool,
    capabilities: GitButlerCapabilities,
    evidence: String,
}

impl GitButlerDiscovery {
    #[must_use]
    pub const fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    #[must_use]
    pub fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    #[must_use]
    pub fn common_git_directory(&self) -> &Path {
        &self.common_git_directory
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn target_ref(&self) -> &str {
        &self.target_ref
    }

    #[must_use]
    pub const fn local_target(&self) -> bool {
        self.local_target
    }

    #[must_use]
    pub const fn capabilities(&self) -> &GitButlerCapabilities {
        &self.capabilities
    }

    #[must_use]
    pub fn evidence(&self) -> &str {
        &self.evidence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerChange {
    provider_ref: ProviderRef,
    commit_id: String,
    branch_name: String,
    conflicted: bool,
}

impl GitButlerChange {
    #[must_use]
    pub const fn provider_ref(&self) -> &ProviderRef {
        &self.provider_ref
    }

    #[must_use]
    pub fn commit_id(&self) -> &str {
        &self.commit_id
    }

    #[must_use]
    pub fn branch_name(&self) -> &str {
        &self.branch_name
    }

    #[must_use]
    pub const fn conflicted(&self) -> bool {
        self.conflicted
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerStack {
    cli_id: String,
    changes_base_to_tip: Vec<GitButlerChange>,
    branch_names_base_to_tip: Vec<String>,
    top_branch_name: String,
    top_branch_cli_id: String,
}

impl GitButlerStack {
    #[must_use]
    pub fn cli_id(&self) -> &str {
        &self.cli_id
    }

    #[must_use]
    pub fn changes_base_to_tip(&self) -> &[GitButlerChange] {
        &self.changes_base_to_tip
    }

    #[must_use]
    pub fn branch_names_base_to_tip(&self) -> &[String] {
        &self.branch_names_base_to_tip
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerConflict {
    provider_ref: ProviderRef,
    commit_id: String,
    branch_name: String,
    evidence: String,
}

impl GitButlerConflict {
    #[must_use]
    pub const fn provider_ref(&self) -> &ProviderRef {
        &self.provider_ref
    }

    #[must_use]
    pub fn commit_id(&self) -> &str {
        &self.commit_id
    }

    #[must_use]
    pub fn branch_name(&self) -> &str {
        &self.branch_name
    }

    #[must_use]
    pub fn evidence(&self) -> &str {
        &self.evidence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectObservation {
    repository_id: RepositoryId,
    provider_locator: PathBuf,
    merge_base: String,
    upstream_target: String,
    stacks: Vec<GitButlerStack>,
    conflicts: Vec<GitButlerConflict>,
    uncommitted_change_count: usize,
    signature: String,
    evidence: String,
}

impl ProjectObservation {
    #[must_use]
    pub const fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    #[must_use]
    pub fn merge_base(&self) -> &str {
        &self.merge_base
    }

    #[must_use]
    pub fn upstream_target(&self) -> &str {
        &self.upstream_target
    }

    #[must_use]
    pub fn stacks(&self) -> &[GitButlerStack] {
        &self.stacks
    }

    #[must_use]
    pub fn conflicts(&self) -> &[GitButlerConflict] {
        &self.conflicts
    }

    #[must_use]
    pub const fn uncommitted_change_count(&self) -> usize {
        self.uncommitted_change_count
    }

    #[must_use]
    pub fn evidence(&self) -> &str {
        &self.evidence
    }

    #[must_use]
    pub fn find_change(&self, provider_ref: &ProviderRef) -> Option<&GitButlerChange> {
        self.stacks
            .iter()
            .flat_map(|stack| &stack.changes_base_to_tip)
            .find(|change| &change.provider_ref == provider_ref)
    }

    /// Converts an exact `GitButler` Change observation into domain-compatible
    /// materialization evidence without using the provider reference as identity.
    ///
    /// # Errors
    ///
    /// Returns an explicit stale-state error when the provider reference is absent.
    pub fn materialization_observation(
        &self,
        provider_ref: &ProviderRef,
    ) -> Result<ProviderObservation, GitButlerProviderError> {
        let change = self.find_change(provider_ref).ok_or_else(|| {
            GitButlerProviderError::StaleProviderState(format!(
                "provider Change {} is absent; reconnect is not inferred",
                provider_ref.as_str()
            ))
        })?;
        let state = if change.conflicted {
            MaterializationState::Diverged
        } else if self.uncommitted_change_count != 0 {
            MaterializationState::Dirty
        } else {
            MaterializationState::Clean
        };
        Ok(ProviderObservation::new(
            state,
            provider_ref.clone(),
            ProviderEvidence::new(evidence(
                "materialization",
                &[
                    provider_ref.as_str(),
                    &change.commit_id,
                    state.as_str(),
                    &self.signature,
                ],
            ))
            .map_err(domain_error)?,
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerCandidateInput {
    provider_ref: ProviderRef,
    commit_id: String,
    branch_name: String,
}

impl GitButlerCandidateInput {
    #[must_use]
    pub const fn provider_ref(&self) -> &ProviderRef {
        &self.provider_ref
    }

    #[must_use]
    pub fn commit_id(&self) -> &str {
        &self.commit_id
    }

    #[must_use]
    pub fn branch_name(&self) -> &str {
        &self.branch_name
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitButlerCandidate {
    repository_id: RepositoryId,
    provider_locator: PathBuf,
    stack_cli_id: String,
    inputs: Vec<GitButlerCandidateInput>,
    top_branch_name: String,
    top_branch_cli_id: String,
    merge_base: String,
    observation_signature: String,
    evidence: String,
}

impl GitButlerCandidate {
    #[must_use]
    pub const fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    #[must_use]
    pub fn inputs(&self) -> &[GitButlerCandidateInput] {
        &self.inputs
    }

    #[must_use]
    pub fn merge_base(&self) -> &str {
        &self.merge_base
    }

    #[must_use]
    pub fn evidence(&self) -> &str {
        &self.evidence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalExport {
    provider_ref: ProviderRef,
    commit_id: String,
    captured: CapturedRevision,
    evidence: String,
}

impl CanonicalExport {
    #[must_use]
    pub const fn provider_ref(&self) -> &ProviderRef {
        &self.provider_ref
    }

    #[must_use]
    pub fn commit_id(&self) -> &str {
        &self.commit_id
    }

    #[must_use]
    pub const fn captured(&self) -> &CapturedRevision {
        &self.captured
    }

    #[must_use]
    pub fn evidence(&self) -> &str {
        &self.evidence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LandingPlan {
    repository_id: RepositoryId,
    provider_locator: PathBuf,
    target_ref: String,
    expected_target: String,
    stack_cli_id: String,
    inputs: Vec<GitButlerCandidateInput>,
    top_branch_name: String,
    top_branch_cli_id: String,
    result_revision: String,
    result_tree: String,
    effect_operation_id: String,
    observation_signature: String,
}

impl LandingPlan {
    #[must_use]
    pub fn target_ref(&self) -> &str {
        &self.target_ref
    }

    #[must_use]
    pub fn expected_target(&self) -> &str {
        &self.expected_target
    }

    #[must_use]
    pub fn result_revision(&self) -> &str {
        &self.result_revision
    }

    #[must_use]
    pub fn result_tree(&self) -> &str {
        &self.result_tree
    }

    #[must_use]
    pub fn effect_operation_id(&self) -> &str {
        &self.effect_operation_id
    }

    #[must_use]
    pub fn inputs(&self) -> &[GitButlerCandidateInput] {
        &self.inputs
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LandingResultEvidence {
    pub prior_target: String,
    pub result_revision: String,
    pub result_tree: String,
    pub effect_operation_id: String,
    pub evidence: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LandingReconciliation {
    ResultVerified(LandingResultEvidence),
    Diverged {
        observed_target: String,
        evidence: String,
    },
    StillUncertain {
        observed_target: String,
        evidence: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectReconciliation {
    pub previous_target: String,
    pub observed_target: String,
    pub rewritten_provider_refs: Vec<ProviderRef>,
    pub missing_provider_refs: Vec<ProviderRef>,
    pub new_provider_refs: Vec<ProviderRef>,
    pub conflicts: Vec<GitButlerConflict>,
    pub evidence: String,
    pub observation: ProjectObservation,
}

#[derive(Clone, Debug)]
pub struct GitButler {
    but_binary: PathBuf,
    git_binary: PathBuf,
    policy: CommandPolicy,
    environment: Vec<(OsString, OsString)>,
}

impl GitButler {
    #[must_use]
    pub fn new(
        but_binary: impl Into<PathBuf>,
        git_binary: impl Into<PathBuf>,
        timeout: Duration,
        max_output_bytes: usize,
    ) -> Self {
        Self {
            but_binary: but_binary.into(),
            git_binary: git_binary.into(),
            policy: CommandPolicy {
                timeout,
                max_output_bytes,
                #[cfg(test)]
                inject_post_spawn_failure: false,
            },
            environment: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new("but", "git", Duration::from_secs(30), 16 * 1024 * 1024)
    }

    /// Supplies child-only environment values, primarily for hermetic provider
    /// registries in tests and isolated runtimes.
    #[must_use]
    pub fn with_environment<I, K, V>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        self.environment = values
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        self
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_post_spawn_failure(mut self) -> Self {
        self.policy.inject_post_spawn_failure = true;
        self
    }

    /// Discovers an initialized `GitButler` project at the exact supported CLI
    /// version and binds it to a caller-owned Weft repository identity.
    ///
    /// # Errors
    ///
    /// Rejects unsupported versions, non-SHA-1 repositories, missing projects,
    /// malformed JSON, target disagreement, and bounded command failures.
    pub fn discover(
        &self,
        path: impl AsRef<Path>,
        repository_id: RepositoryId,
    ) -> Result<GitButlerDiscovery, GitButlerProviderError> {
        let version = self.version()?;
        let native = self.native_git();
        let git = native.discover(path.as_ref())?;
        if git.object_format != "sha1" {
            return Err(GitButlerProviderError::Unsupported {
                capability: "object-format",
                reason: format!(
                    "GitButler {SUPPORTED_VERSION} was evidenced only with SHA-1, observed {}",
                    git.object_format
                ),
            });
        }
        let target_ref = self.git_config(&git.worktree_root, "gitbutler.project.targetref")?;
        let local_target = self.is_local_target(&git.worktree_root, &target_ref)?;
        let status = self.status_json(&git.worktree_root)?;
        let discovery = GitButlerDiscovery {
            evidence: evidence(
                "discover",
                &[
                    repository_id.as_str(),
                    &version,
                    &git.common_git_directory.display().to_string(),
                    &target_ref,
                    if local_target { "local" } else { "remote" },
                ],
            ),
            repository_id,
            worktree_root: git.worktree_root,
            common_git_directory: git.common_git_directory,
            version,
            target_ref,
            local_target,
            capabilities: GitButlerCapabilities::v0_22(local_target),
        };
        self.normalize(&discovery, status)?;
        Ok(discovery)
    }

    /// Returns a normalized exact observation after rechecking repository,
    /// configured target, CLI version, and the complete supported JSON shape.
    ///
    /// # Errors
    ///
    /// Fails closed on identity/locator drift, schema drift, unsupported provider
    /// state, target disagreement, or command bounds.
    pub fn observe(
        &self,
        discovery: &GitButlerDiscovery,
        repository_id: &RepositoryId,
    ) -> Result<ProjectObservation, GitButlerProviderError> {
        self.validate_discovery(discovery, repository_id)?;
        self.normalize(discovery, self.status_json(&discovery.worktree_root)?)
    }

    /// Requires a discovered provider capability and returns an explicit error
    /// for unproven behavior instead of approximating another provider.
    ///
    /// # Errors
    ///
    /// Returns [`GitButlerProviderError::Unsupported`] when unavailable.
    pub fn require(
        &self,
        discovery: &GitButlerDiscovery,
        capability: GitButlerCapability,
    ) -> Result<(), GitButlerProviderError> {
        if discovery.capabilities.supports(capability) {
            return Ok(());
        }
        let reason = match capability {
            GitButlerCapability::CanonicalImport => {
                "applying canonical tree-delta-v1 content through GitButler was not evidenced"
            }
            GitButlerCapability::ProviderReconnect => {
                "provider metadata removal/reconnect was not proven"
            }
            GitButlerCapability::RemoteLanding => {
                "remote policy, authentication, atomicity, and recovery were not proven"
            }
            GitButlerCapability::GuardedLocalFastForwardLanding => {
                "the configured GitButler target is not the repository-local gb-local remote"
            }
            _ => "the capability is unavailable at this version or project configuration",
        };
        Err(GitButlerProviderError::Unsupported {
            capability: capability.name(),
            reason: reason.to_owned(),
        })
    }

    /// Seals one observed `GitButler` stack in base-to-tip Change order.
    ///
    /// # Errors
    ///
    /// Rejects a missing or empty stack.
    pub fn candidate(
        &self,
        observation: &ProjectObservation,
        stack_cli_id: &str,
    ) -> Result<GitButlerCandidate, GitButlerProviderError> {
        let stack = observation
            .stacks
            .iter()
            .find(|stack| stack.cli_id == stack_cli_id)
            .ok_or_else(|| {
                GitButlerProviderError::StaleProviderState(format!(
                    "stack CLI ID {stack_cli_id} is absent"
                ))
            })?;
        if stack.changes_base_to_tip.is_empty() {
            return Err(GitButlerProviderError::Unsupported {
                capability: "stack-mapping",
                reason: "an empty GitButler stack cannot resolve an exact candidate".to_owned(),
            });
        }
        let inputs = stack
            .changes_base_to_tip
            .iter()
            .map(|change| GitButlerCandidateInput {
                provider_ref: change.provider_ref.clone(),
                commit_id: change.commit_id.clone(),
                branch_name: change.branch_name.clone(),
            })
            .collect::<Vec<_>>();
        Ok(GitButlerCandidate {
            evidence: evidence(
                "candidate",
                &[
                    observation.repository_id.as_str(),
                    &observation.merge_base,
                    &observation.signature,
                    stack_cli_id,
                ],
            ),
            repository_id: observation.repository_id.clone(),
            provider_locator: observation.provider_locator.clone(),
            stack_cli_id: stack.cli_id.clone(),
            inputs,
            top_branch_name: stack.top_branch_name.clone(),
            top_branch_cli_id: stack.top_branch_cli_id.clone(),
            merge_base: observation.merge_base.clone(),
            observation_signature: observation.signature.clone(),
        })
    }

    /// Exports one exact, currently observed `GitButler` Change into Weft's
    /// provider-independent canonical artifact store through Native Git objects.
    ///
    /// # Errors
    ///
    /// Rejects identity drift, rewritten/missing provider references, invalid
    /// exact bases, unsupported content, or artifact/command failures.
    pub fn export_canonical(
        &self,
        discovery: &GitButlerDiscovery,
        repository_id: &RepositoryId,
        provider_ref: &ProviderRef,
        expected_commit: &str,
        exact_base: &str,
        artifacts: &ArtifactStore,
    ) -> Result<CanonicalExport, GitButlerProviderError> {
        self.require(discovery, GitButlerCapability::CanonicalExport)?;
        let observation = self.observe(discovery, repository_id)?;
        let change = observation.find_change(provider_ref).ok_or_else(|| {
            GitButlerProviderError::StaleProviderState(format!(
                "provider Change {} is absent",
                provider_ref.as_str()
            ))
        })?;
        if change.commit_id != expected_commit {
            return Err(GitButlerProviderError::StaleProviderState(format!(
                "provider Change {} rewrote from expected commit {expected_commit} to {}",
                provider_ref.as_str(),
                change.commit_id
            )));
        }
        if change.conflicted {
            return Err(GitButlerProviderError::Unsupported {
                capability: "canonical-export",
                reason: "conflicted GitButler commits are not canonical revisions".to_owned(),
            });
        }
        let observed_parent = self.first_parent(&discovery.worktree_root, expected_commit)?;
        if observed_parent != exact_base {
            return Err(GitButlerProviderError::StaleProviderState(format!(
                "provider Change {} first parent {observed_parent} differs from exact base {exact_base}",
                provider_ref.as_str()
            )));
        }
        let captured = self.native_git().capture_revision(
            &discovery.worktree_root,
            repository_id.clone(),
            exact_base,
            expected_commit,
            artifacts,
        )?;
        Ok(CanonicalExport {
            provider_ref: provider_ref.clone(),
            commit_id: expected_commit.to_owned(),
            evidence: evidence(
                "canonical-export",
                &[
                    provider_ref.as_str(),
                    exact_base,
                    expected_commit,
                    captured.artifact_ref().manifest_digest(),
                ],
            ),
            captured,
        })
    }

    /// Creates a guarded local fast-forward landing plan from an exact observed
    /// stack. General merge and remote landing remain unsupported.
    ///
    /// # Errors
    ///
    /// Rejects stale candidates/targets, conflicts, dirty workspaces, remote
    /// targets, or a candidate that cannot land as the exact observed tip.
    pub fn plan_local_landing(
        &self,
        discovery: &GitButlerDiscovery,
        repository_id: &RepositoryId,
        candidate: &GitButlerCandidate,
        expected_target: &str,
        effect_operation_id: &EffectOperationId,
    ) -> Result<LandingPlan, GitButlerProviderError> {
        self.require(
            discovery,
            GitButlerCapability::GuardedLocalFastForwardLanding,
        )?;
        self.validate_candidate(discovery, repository_id, candidate)?;
        let observed = self.observe(discovery, repository_id)?;
        self.verify_candidate_current(candidate, &observed)?;
        if observed.upstream_target != expected_target || observed.merge_base != expected_target {
            return Err(GitButlerProviderError::ChangedTarget {
                expected: expected_target.to_owned(),
                observed: observed.upstream_target,
            });
        }
        if observed.uncommitted_change_count != 0 {
            return Err(GitButlerProviderError::StaleProviderState(
                "workspace has uncommitted changes".to_owned(),
            ));
        }
        if !observed.conflicts.is_empty() {
            return Err(GitButlerProviderError::StaleProviderState(
                "candidate contains conflicted commits".to_owned(),
            ));
        }
        let result_revision = candidate
            .inputs
            .last()
            .ok_or_else(|| {
                GitButlerProviderError::StaleProviderState("empty candidate".to_owned())
            })?
            .commit_id
            .clone();
        let result_tree = self
            .native_git()
            .inspect_revision(&discovery.worktree_root, &result_revision)?
            .tree_id()
            .to_owned();
        Ok(LandingPlan {
            repository_id: repository_id.clone(),
            provider_locator: discovery.common_git_directory.clone(),
            target_ref: discovery.target_ref.clone(),
            expected_target: expected_target.to_owned(),
            stack_cli_id: candidate.stack_cli_id.clone(),
            inputs: candidate.inputs.clone(),
            top_branch_name: candidate.top_branch_name.clone(),
            top_branch_cli_id: candidate.top_branch_cli_id.clone(),
            result_revision,
            result_tree,
            effect_operation_id: effect_operation_id.as_str().to_owned(),
            observation_signature: candidate.observation_signature.clone(),
        })
    }

    /// Executes an exact local fast-forward landing and immediately reconciles
    /// the target. Any bounded command failure is treated as ambiguous, never as
    /// proof of failure or success.
    ///
    /// # Errors
    ///
    /// Pre-mutation validation failures and unavailable reconciliation evidence
    /// are returned as errors. Mutation command ambiguity is reflected in the
    /// conservative reconciliation result.
    pub fn execute_local_landing(
        &self,
        discovery: &GitButlerDiscovery,
        repository_id: &RepositoryId,
        plan: &LandingPlan,
    ) -> Result<LandingReconciliation, GitButlerProviderError> {
        self.validate_plan(discovery, repository_id, plan)?;
        let observed = self.observe(discovery, repository_id)?;
        Self::verify_plan_current(plan, &observed)?;
        let _command_outcome = self.command(
            None,
            "land-local-stack",
            [
                OsStr::new("-C"),
                discovery.worktree_root.as_os_str(),
                OsStr::new("land"),
                OsStr::new(&plan.top_branch_cli_id),
                OsStr::new("--whole-stack"),
                OsStr::new("--yes"),
                OsStr::new("--json"),
            ],
        );
        self.reconcile_local_landing(discovery, repository_id, plan)
    }

    /// Classifies exact provider state after a possibly ambiguous local landing.
    /// The unchanged expected target remains uncertain because an effect followed
    /// by an external reset cannot be disproven from current provider state.
    ///
    /// # Errors
    ///
    /// Fails if repository binding or exact result-tree verification is lost.
    pub fn reconcile_local_landing(
        &self,
        discovery: &GitButlerDiscovery,
        repository_id: &RepositoryId,
        plan: &LandingPlan,
    ) -> Result<LandingReconciliation, GitButlerProviderError> {
        self.validate_plan(discovery, repository_id, plan)?;
        let observed = self.resolve_target(&discovery.worktree_root, &plan.target_ref)?;
        if observed == plan.expected_target {
            return Ok(LandingReconciliation::StillUncertain {
                evidence: evidence(
                    "landing-still-uncertain",
                    &[&plan.target_ref, &observed, &plan.effect_operation_id],
                ),
                observed_target: observed,
            });
        }
        if observed == plan.result_revision {
            let result = self
                .native_git()
                .inspect_revision(&discovery.worktree_root, &observed)?;
            if result.tree_id() != plan.result_tree {
                return Err(GitButlerProviderError::VerificationFailed(format!(
                    "landed result tree {} differs from planned {}",
                    result.tree_id(),
                    plan.result_tree
                )));
            }
            return Ok(LandingReconciliation::ResultVerified(
                LandingResultEvidence {
                    prior_target: plan.expected_target.clone(),
                    result_revision: observed,
                    result_tree: result.tree_id().to_owned(),
                    effect_operation_id: plan.effect_operation_id.clone(),
                    evidence: evidence(
                        "landing-result-verified",
                        &[
                            &plan.target_ref,
                            &plan.result_revision,
                            &plan.result_tree,
                            &plan.effect_operation_id,
                        ],
                    ),
                },
            ));
        }
        Ok(LandingReconciliation::Diverged {
            evidence: evidence(
                "landing-diverged",
                &[&plan.target_ref, &observed, &plan.effect_operation_id],
            ),
            observed_target: observed,
        })
    }

    /// Compares two exact provider observations without mutating `GitButler`. A
    /// missing provider reference is evidence, not an inferred release/reconnect.
    ///
    /// # Errors
    ///
    /// Fails on repository/schema/target observation errors.
    pub fn reconcile_project(
        &self,
        discovery: &GitButlerDiscovery,
        repository_id: &RepositoryId,
        previous: &ProjectObservation,
    ) -> Result<ProjectReconciliation, GitButlerProviderError> {
        if &previous.repository_id != repository_id
            || previous.provider_locator != discovery.common_git_directory
        {
            return Err(GitButlerProviderError::RepositoryMismatch);
        }
        let observation = self.observe(discovery, repository_id)?;
        let old = changes_by_ref(previous);
        let new = changes_by_ref(&observation);
        let rewritten_provider_refs = old
            .iter()
            .filter_map(|(provider_ref, old_commit)| {
                new.get(provider_ref)
                    .is_some_and(|new_commit| new_commit != old_commit)
                    .then_some(provider_ref.clone())
            })
            .collect::<Vec<_>>();
        let missing_provider_refs = old
            .keys()
            .filter(|provider_ref| !new.contains_key(*provider_ref))
            .cloned()
            .collect::<Vec<_>>();
        let new_provider_refs = new
            .keys()
            .filter(|provider_ref| !old.contains_key(*provider_ref))
            .cloned()
            .collect::<Vec<_>>();
        Ok(ProjectReconciliation {
            evidence: evidence(
                "project-reconciliation",
                &[
                    &previous.signature,
                    &observation.signature,
                    &rewritten_provider_refs.len().to_string(),
                    &missing_provider_refs.len().to_string(),
                    &new_provider_refs.len().to_string(),
                ],
            ),
            previous_target: previous.upstream_target.clone(),
            observed_target: observation.upstream_target.clone(),
            rewritten_provider_refs,
            missing_provider_refs,
            new_provider_refs,
            conflicts: observation.conflicts.clone(),
            observation,
        })
    }

    fn normalize(
        &self,
        discovery: &GitButlerDiscovery,
        status: StatusJson,
    ) -> Result<ProjectObservation, GitButlerProviderError> {
        validate_base_commit(&status.merge_base, "mergeBase")?;
        validate_base_commit(
            &status.upstream_state.latest_commit,
            "upstreamState.latestCommit",
        )?;
        if status.upstream_state.latest_commit.commit_id
            != self.resolve_target(&discovery.worktree_root, &discovery.target_ref)?
        {
            return Err(GitButlerProviderError::VerificationFailed(
                "GitButler upstreamState disagrees with configured target ref".to_owned(),
            ));
        }
        validate_optional_text(status.upstream_state.last_fetched.as_deref(), "lastFetched")?;
        let behind = status.upstream_state.behind.to_string();
        validate_uncommitted_changes(&status.uncommitted_changes)?;

        let merge_base = status.merge_base.commit_id;
        let upstream_target = status.upstream_state.latest_commit.commit_id;
        let mut stacks = Vec::with_capacity(status.stacks.len());
        let mut seen_stack_ids = BTreeSet::new();
        let mut seen_branch_cli_ids = BTreeSet::new();
        let mut seen_branch_names = BTreeSet::new();
        let mut seen_provider_refs = BTreeSet::new();
        let mut conflicts = Vec::new();
        for stack in status.stacks {
            require_nonempty(&stack.cli_id, "stacks.cliId")?;
            if !seen_stack_ids.insert(stack.cli_id.clone()) {
                return invalid_status("duplicate stack cliId");
            }
            let normalized = self.normalize_stack(
                &discovery.worktree_root,
                &merge_base,
                stack,
                &mut seen_branch_cli_ids,
                &mut seen_branch_names,
                &mut seen_provider_refs,
            )?;
            conflicts.extend(
                normalized
                    .changes_base_to_tip
                    .iter()
                    .filter(|change| change.conflicted)
                    .map(|change| GitButlerConflict {
                        provider_ref: change.provider_ref.clone(),
                        commit_id: change.commit_id.clone(),
                        branch_name: change.branch_name.clone(),
                        evidence: evidence(
                            "conflict",
                            &[
                                change.provider_ref.as_str(),
                                &change.commit_id,
                                &change.branch_name,
                            ],
                        ),
                    }),
            );
            stacks.push(normalized);
        }
        let mut signature_fields = vec![
            merge_base.as_str(),
            upstream_target.as_str(),
            behind.as_str(),
        ];
        for stack in &stacks {
            signature_fields.push(&stack.cli_id);
            for change in &stack.changes_base_to_tip {
                signature_fields.push(change.provider_ref.as_str());
                signature_fields.push(&change.commit_id);
                signature_fields.push(if change.conflicted {
                    "conflict"
                } else {
                    "clean"
                });
            }
        }
        let uncommitted_change_count = status.uncommitted_changes.len();
        let count = uncommitted_change_count.to_string();
        signature_fields.push(&count);
        let signature = evidence("status-signature", &signature_fields);
        Ok(ProjectObservation {
            evidence: evidence(
                "status",
                &[
                    discovery.repository_id.as_str(),
                    &merge_base,
                    &upstream_target,
                    &signature,
                ],
            ),
            repository_id: discovery.repository_id.clone(),
            provider_locator: discovery.common_git_directory.clone(),
            merge_base,
            upstream_target,
            stacks,
            conflicts,
            uncommitted_change_count,
            signature,
        })
    }

    fn normalize_stack(
        &self,
        repository: &Path,
        merge_base: &str,
        stack: StackJson,
        seen_branch_cli_ids: &mut BTreeSet<String>,
        seen_branch_names: &mut BTreeSet<String>,
        seen_provider_refs: &mut BTreeSet<ProviderRef>,
    ) -> Result<GitButlerStack, GitButlerProviderError> {
        if !stack.assigned_changes.is_empty() {
            return Err(GitButlerProviderError::Unsupported {
                capability: "assigned-uncommitted-changes",
                reason: "GitButler assignedChanges nested schema was not established by Phase 0"
                    .to_owned(),
            });
        }
        if stack.branches.is_empty() {
            return Err(GitButlerProviderError::InvalidOutput {
                operation: "status",
                reason: "stack has no branches".to_owned(),
            });
        }
        let top_branch_name = stack.branches[0].name.clone();
        let top_branch_cli_id = stack.branches[0].cli_id.clone();
        let mut changes = Vec::new();
        let mut branch_names = Vec::new();
        for branch in stack.branches.into_iter().rev() {
            let branch_name = validate_branch(&branch)?;
            if !seen_branch_cli_ids.insert(branch.cli_id.clone()) {
                return invalid_status("duplicate branch cliId");
            }
            if !seen_branch_names.insert(branch_name.clone()) {
                return invalid_status("duplicate branch name");
            }
            branch_names.push(branch_name.clone());
            if branch.commits.is_empty() {
                return Err(GitButlerProviderError::Unsupported {
                    capability: "stack-mapping",
                    reason: "an empty GitButler branch segment has no exact Weft Change input"
                        .to_owned(),
                });
            }
            for commit in branch.commits.into_iter().rev() {
                let change = normalize_change(&branch_name, commit)?;
                if !seen_provider_refs.insert(change.provider_ref.clone()) {
                    return invalid_status("duplicate GitButler changeId");
                }
                changes.push(change);
            }
        }
        let mut previous = merge_base;
        for change in &changes {
            let parent = self.first_parent(repository, &change.commit_id)?;
            if parent != previous {
                return Err(GitButlerProviderError::Unsupported {
                    capability: "stack-mapping",
                    reason: format!(
                        "commit {} first parent {parent} does not equal exact predecessor {previous}",
                        change.commit_id
                    ),
                });
            }
            previous = &change.commit_id;
        }
        Ok(GitButlerStack {
            cli_id: stack.cli_id,
            changes_base_to_tip: changes,
            branch_names_base_to_tip: branch_names,
            top_branch_name,
            top_branch_cli_id,
        })
    }

    fn validate_discovery(
        &self,
        discovery: &GitButlerDiscovery,
        repository_id: &RepositoryId,
    ) -> Result<(), GitButlerProviderError> {
        if &discovery.repository_id != repository_id {
            return Err(GitButlerProviderError::RepositoryMismatch);
        }
        if self.version()? != discovery.version {
            return Err(GitButlerProviderError::RepositoryMismatch);
        }
        let git = self.native_git().discover(&discovery.worktree_root)?;
        if git.common_git_directory != discovery.common_git_directory
            || self.git_config(&git.worktree_root, "gitbutler.project.targetref")?
                != discovery.target_ref
        {
            return Err(GitButlerProviderError::RepositoryMismatch);
        }
        Ok(())
    }

    fn validate_candidate(
        &self,
        discovery: &GitButlerDiscovery,
        repository_id: &RepositoryId,
        candidate: &GitButlerCandidate,
    ) -> Result<(), GitButlerProviderError> {
        self.validate_discovery(discovery, repository_id)?;
        if &candidate.repository_id != repository_id
            || candidate.provider_locator != discovery.common_git_directory
        {
            return Err(GitButlerProviderError::RepositoryMismatch);
        }
        Ok(())
    }

    fn validate_plan(
        &self,
        discovery: &GitButlerDiscovery,
        repository_id: &RepositoryId,
        plan: &LandingPlan,
    ) -> Result<(), GitButlerProviderError> {
        self.validate_discovery(discovery, repository_id)?;
        if &plan.repository_id != repository_id
            || plan.provider_locator != discovery.common_git_directory
            || plan.target_ref != discovery.target_ref
        {
            return Err(GitButlerProviderError::RepositoryMismatch);
        }
        Ok(())
    }

    fn verify_candidate_current(
        &self,
        candidate: &GitButlerCandidate,
        observation: &ProjectObservation,
    ) -> Result<(), GitButlerProviderError> {
        if candidate.observation_signature != observation.signature
            || candidate.merge_base != observation.merge_base
        {
            return Err(GitButlerProviderError::StaleProviderState(
                "candidate observation changed".to_owned(),
            ));
        }
        let current = self.candidate(observation, &candidate.stack_cli_id)?;
        if current.inputs != candidate.inputs
            || current.top_branch_name != candidate.top_branch_name
            || current.top_branch_cli_id != candidate.top_branch_cli_id
        {
            return Err(GitButlerProviderError::StaleProviderState(
                "candidate stack changed".to_owned(),
            ));
        }
        Ok(())
    }

    fn verify_plan_current(
        plan: &LandingPlan,
        observation: &ProjectObservation,
    ) -> Result<(), GitButlerProviderError> {
        if observation.upstream_target != plan.expected_target
            || observation.merge_base != plan.expected_target
        {
            return Err(GitButlerProviderError::ChangedTarget {
                expected: plan.expected_target.clone(),
                observed: observation.upstream_target.clone(),
            });
        }
        if observation.signature != plan.observation_signature
            || observation.uncommitted_change_count != 0
            || !observation.conflicts.is_empty()
        {
            return Err(GitButlerProviderError::StaleProviderState(
                "landing observation changed".to_owned(),
            ));
        }
        let stack = observation
            .stacks
            .iter()
            .find(|stack| stack.cli_id == plan.stack_cli_id)
            .ok_or_else(|| {
                GitButlerProviderError::StaleProviderState("landing stack is absent".to_owned())
            })?;
        let inputs = stack
            .changes_base_to_tip
            .iter()
            .map(|change| GitButlerCandidateInput {
                provider_ref: change.provider_ref.clone(),
                commit_id: change.commit_id.clone(),
                branch_name: change.branch_name.clone(),
            })
            .collect::<Vec<_>>();
        if inputs != plan.inputs
            || stack.top_branch_name != plan.top_branch_name
            || stack.top_branch_cli_id != plan.top_branch_cli_id
        {
            return Err(GitButlerProviderError::StaleProviderState(
                "landing inputs changed".to_owned(),
            ));
        }
        Ok(())
    }

    fn version(&self) -> Result<String, GitButlerProviderError> {
        let output = self.command(None, "version", [OsStr::new("--version")])?;
        ensure_success(&output, "version")?;
        let line = one_line(&output.stdout, "version")?;
        let observed =
            line.strip_prefix("but ")
                .ok_or_else(|| GitButlerProviderError::InvalidOutput {
                    operation: "version",
                    reason: "expected `but <version>`".to_owned(),
                })?;
        if observed != SUPPORTED_VERSION {
            return Err(GitButlerProviderError::Unsupported {
                capability: "cli-version",
                reason: format!(
                    "status schema is pinned to {SUPPORTED_VERSION}, observed {observed}"
                ),
            });
        }
        Ok(observed.to_owned())
    }

    fn status_json(&self, repository: &Path) -> Result<StatusJson, GitButlerProviderError> {
        let output = self.command(
            None,
            "status",
            [
                OsStr::new("-C"),
                repository.as_os_str(),
                OsStr::new("--json"),
                OsStr::new("status"),
            ],
        )?;
        ensure_success(&output, "status")?;
        serde_json::from_slice(&output.stdout).map_err(|error| {
            GitButlerProviderError::InvalidOutput {
                operation: "status",
                reason: format!("unsupported GitButler {SUPPORTED_VERSION} JSON shape: {error}"),
            }
        })
    }

    fn git_config(&self, repository: &Path, key: &str) -> Result<String, GitButlerProviderError> {
        let output = self.git_command(
            repository,
            "gitbutler-config",
            [OsStr::new("config"), OsStr::new("--get"), OsStr::new(key)],
        )?;
        ensure_success(&output, "gitbutler-config")?;
        one_line(&output.stdout, "gitbutler-config")
    }

    fn is_local_target(
        &self,
        repository: &Path,
        target_ref: &str,
    ) -> Result<bool, GitButlerProviderError> {
        let Some(rest) = target_ref.strip_prefix("refs/remotes/") else {
            return Ok(false);
        };
        let Some((remote, _branch)) = rest.split_once('/') else {
            return Ok(false);
        };
        if remote != "gb-local" {
            return Ok(false);
        }
        let output = self.git_command(
            repository,
            "target-remote-url",
            [
                OsStr::new("remote"),
                OsStr::new("get-url"),
                OsStr::new(remote),
            ],
        )?;
        ensure_success(&output, "target-remote-url")?;
        let url = one_line(&output.stdout, "target-remote-url")?;
        let path = Path::new(&url);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            repository.join(path)
        };
        Ok(resolved.canonicalize()? == repository.canonicalize()?)
    }

    fn first_parent(
        &self,
        repository: &Path,
        commit: &str,
    ) -> Result<String, GitButlerProviderError> {
        let revision = format!("{commit}^1");
        let output = self.git_command(
            repository,
            "first-parent",
            [
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new(&format!("{revision}^{{commit}}")),
            ],
        )?;
        ensure_success(&output, "first-parent")?;
        let parent = one_line(&output.stdout, "first-parent")?;
        validate_object_id(&parent, "first-parent")?;
        Ok(parent)
    }

    fn resolve_target(
        &self,
        repository: &Path,
        target_ref: &str,
    ) -> Result<String, GitButlerProviderError> {
        if !target_ref.starts_with("refs/remotes/") {
            return Err(GitButlerProviderError::Unsupported {
                capability: "target-ref",
                reason: format!("expected an exact refs/remotes target, observed {target_ref}"),
            });
        }
        let checked = self.git_command(
            repository,
            "validate-target-ref",
            [OsStr::new("check-ref-format"), OsStr::new(target_ref)],
        )?;
        ensure_success(&checked, "validate-target-ref")?;
        let revision = format!("{target_ref}^{{commit}}");
        let output = self.git_command(
            repository,
            "observe-target",
            [
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new(&revision),
            ],
        )?;
        ensure_success(&output, "observe-target")?;
        let commit = one_line(&output.stdout, "observe-target")?;
        validate_object_id(&commit, "observe-target")?;
        Ok(commit)
    }

    fn native_git(&self) -> NativeGit {
        NativeGit::new(
            self.git_binary.clone(),
            self.policy.timeout,
            self.policy.max_output_bytes,
        )
    }

    fn command<I, S>(
        &self,
        directory: Option<&Path>,
        operation: &'static str,
        args: I,
    ) -> Result<CommandOutput, GitButlerProviderError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run(
            &self.but_binary,
            directory,
            operation,
            args,
            self.policy,
            &self.environment,
        )
    }

    fn git_command<I, S>(
        &self,
        repository: &Path,
        operation: &'static str,
        args: I,
    ) -> Result<CommandOutput, GitButlerProviderError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run(
            &self.git_binary,
            Some(repository),
            operation,
            args,
            self.policy,
            &self.environment,
        )
    }
}

fn validate_branch(branch: &BranchJson) -> Result<String, GitButlerProviderError> {
    require_nonempty(&branch.cli_id, "branches.cliId")?;
    require_nonempty(&branch.name, "branches.name")?;
    require_nonempty(&branch.branch_status, "branches.branchStatus")?;
    if !branch.upstream_commits.is_empty() {
        return Err(GitButlerProviderError::Unsupported {
            capability: "published-stack-mapping",
            reason: "non-empty upstreamCommits mapping was not established by Phase 0".to_owned(),
        });
    }
    if branch.review_id.is_some() || branch.ci.is_some() {
        return Err(GitButlerProviderError::Unsupported {
            capability: "extended-status-branch-shape",
            reason: "branch reviewId/ci nested data was not established by Phase 0".to_owned(),
        });
    }
    Ok(branch.name.clone())
}

fn validate_uncommitted_changes(
    changes: &[UncommittedChangeJson],
) -> Result<(), GitButlerProviderError> {
    for change in changes {
        require_nonempty(&change.cli_id, "uncommittedChanges.cliId")?;
        require_nonempty(&change.file_path, "uncommittedChanges.filePath")?;
        require_nonempty(&change.change_type, "uncommittedChanges.changeType")?;
    }
    Ok(())
}

fn normalize_change(
    branch_name: &str,
    commit: CommitJson,
) -> Result<GitButlerChange, GitButlerProviderError> {
    validate_commit_common(&commit, "stacks.branches.commits")?;
    require_nonempty(&commit.cli_id, "stacks.branches.commits.cliId")?;
    let change_id =
        commit
            .change_id
            .as_deref()
            .ok_or_else(|| GitButlerProviderError::InvalidOutput {
                operation: "status",
                reason: "stack commit has no changeId".to_owned(),
            })?;
    if change_id.len() != 32 || !change_id.bytes().all(|byte| byte.is_ascii_lowercase()) {
        return invalid_status("changeId is not the evidenced 32-lowercase-letter shape");
    }
    let conflicted = commit
        .conflicted
        .ok_or_else(|| GitButlerProviderError::InvalidOutput {
            operation: "status",
            reason: "stack commit has null conflicted state".to_owned(),
        })?;
    Ok(GitButlerChange {
        provider_ref: ProviderRef::new(change_id).map_err(domain_error)?,
        commit_id: commit.commit_id,
        branch_name: branch_name.to_owned(),
        conflicted,
    })
}

fn validate_base_commit(commit: &CommitJson, location: &str) -> Result<(), GitButlerProviderError> {
    validate_commit_common(commit, location)?;
    if commit.change_id.is_some() || commit.conflicted.is_some() {
        return invalid_status(&format!(
            "{location} unexpectedly contains Change/conflict identity"
        ));
    }
    Ok(())
}

fn validate_commit_common(
    commit: &CommitJson,
    location: &str,
) -> Result<(), GitButlerProviderError> {
    validate_object_id(&commit.commit_id, location)?;
    require_nonempty(&commit.created_at, location)?;
    require_nonempty(&commit.author_name, location)?;
    require_nonempty(&commit.author_email, location)?;
    if commit.message.contains('\0') {
        return invalid_status(&format!("{location} message contains NUL"));
    }
    if commit.review_id.is_some() || commit.changes.is_some() {
        return Err(GitButlerProviderError::Unsupported {
            capability: "extended-status-commit-shape",
            reason: format!("{location} contains unvalidated reviewId/changes data"),
        });
    }
    Ok(())
}

fn validate_object_id(value: &str, location: &str) -> Result<(), GitButlerProviderError> {
    if value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        invalid_status(&format!("{location} has a non-SHA-1 commitId"))
    }
}

fn validate_optional_text(
    value: Option<&str>,
    location: &str,
) -> Result<(), GitButlerProviderError> {
    if value.is_some_and(str::is_empty) {
        return invalid_status(&format!("{location} is empty"));
    }
    Ok(())
}

fn require_nonempty(value: &str, location: &str) -> Result<(), GitButlerProviderError> {
    if value.is_empty() {
        invalid_status(&format!("{location} is empty"))
    } else {
        Ok(())
    }
}

fn changes_by_ref(observation: &ProjectObservation) -> BTreeMap<ProviderRef, String> {
    observation
        .stacks
        .iter()
        .flat_map(|stack| &stack.changes_base_to_tip)
        .map(|change| (change.provider_ref.clone(), change.commit_id.clone()))
        .collect()
}

fn ensure_success(
    output: &CommandOutput,
    operation: &'static str,
) -> Result<(), GitButlerProviderError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(GitButlerProviderError::CommandFailed {
            operation,
            code: output.status.code(),
            redacted_stderr_bytes: output.stderr.len(),
        })
    }
}

fn one_line(bytes: &[u8], operation: &'static str) -> Result<String, GitButlerProviderError> {
    let value = std::str::from_utf8(bytes).map_err(|_| GitButlerProviderError::InvalidOutput {
        operation,
        reason: "output is not UTF-8".to_owned(),
    })?;
    let value = value.trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.contains(['\r', '\n']) {
        return Err(GitButlerProviderError::InvalidOutput {
            operation,
            reason: "expected exactly one non-empty line".to_owned(),
        });
    }
    Ok(value.to_owned())
}

fn invalid_status<T>(reason: &str) -> Result<T, GitButlerProviderError> {
    Err(GitButlerProviderError::InvalidOutput {
        operation: "status",
        reason: reason.to_owned(),
    })
}

fn domain_error(error: impl std::fmt::Display) -> GitButlerProviderError {
    GitButlerProviderError::Domain(error.to_string())
}

fn evidence(kind: &str, fields: &[&str]) -> String {
    let mut value = format!("{PROVIDER_ID}:{SUPPORTED_VERSION}:{kind}");
    for field in fields {
        value.push(':');
        value.push_str(&field.len().to_string());
        value.push(':');
        value.push_str(field);
    }
    value
}
