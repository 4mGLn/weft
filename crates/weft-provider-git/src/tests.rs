use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use tempfile::TempDir;
use weft_artifact::ArtifactStore;
use weft_domain::{EffectOperationId, MaterializationState, RepositoryId};

use super::*;
#[cfg(unix)]
use crate::command::{CommandPolicy, run_git};

struct Fixture {
    root: TempDir,
    repository: std::path::PathBuf,
    artifacts: ArtifactStore,
    base: String,
    first: String,
    second: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repository");
        command(
            None,
            [
                "init",
                "--quiet",
                "--initial-branch=main",
                path(&repository),
            ],
        );
        command(Some(&repository), ["config", "user.name", "Weft Test"]);
        command(
            Some(&repository),
            ["config", "user.email", "weft@test.invalid"],
        );
        command(Some(&repository), ["config", "commit.gpgSign", "false"]);
        fs::write(repository.join("shared.txt"), b"base\n").unwrap();
        fs::write(repository.join("deleted.txt"), b"delete me\n").unwrap();
        command(Some(&repository), ["add", "--all"]);
        command(Some(&repository), ["commit", "--quiet", "-m", "base"]);
        let base = command(Some(&repository), ["rev-parse", "HEAD"]);

        command(Some(&repository), ["switch", "--quiet", "-c", "change-a"]);
        fs::write(repository.join("shared.txt"), b"first\n").unwrap();
        fs::write(repository.join("binary.bin"), [0, 1, 2, 255]).unwrap();
        fs::create_dir(repository.join("nested")).unwrap();
        fs::write(repository.join("nested/file.txt"), b"nested\n").unwrap();
        fs::remove_file(repository.join("deleted.txt")).unwrap();
        fs::write(repository.join("tool.sh"), b"#!/bin/sh\nexit 0\n").unwrap();
        set_executable(&repository.join("tool.sh"));
        create_test_symlink(&repository.join("link"));
        command(Some(&repository), ["add", "--all"]);
        command(Some(&repository), ["commit", "--quiet", "-m", "first"]);
        let first = command(Some(&repository), ["rev-parse", "HEAD"]);

        command(Some(&repository), ["switch", "--quiet", "-c", "change-b"]);
        fs::write(repository.join("shared.txt"), b"second\n").unwrap();
        fs::write(repository.join("b.txt"), b"b\n").unwrap();
        command(Some(&repository), ["add", "--all"]);
        command(Some(&repository), ["commit", "--quiet", "-m", "second"]);
        let second = command(Some(&repository), ["rev-parse", "HEAD"]);
        let artifacts = ArtifactStore::open(root.path().join("cas")).unwrap();
        Self {
            root,
            repository,
            artifacts,
            base,
            first,
            second,
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn exact_local_workflow_survives_ref_rewrite_and_reconciles_integration() {
    let fixture = Fixture::new();
    let adapter = NativeGit::with_defaults();
    let discovery = adapter.discover(&fixture.repository).unwrap();
    assert_eq!(discovery.object_format, "sha1");
    assert!(
        discovery
            .capabilities
            .supports(GitCapability::GuardedRefUpdate)
    );

    let repository_id = RepositoryId::new("repo-1").unwrap();
    let first = adapter
        .capture_revision(
            &fixture.repository,
            repository_id.clone(),
            &fixture.base,
            &fixture.first,
            &fixture.artifacts,
        )
        .unwrap();
    let second = adapter
        .capture_revision(
            &fixture.repository,
            repository_id.clone(),
            &fixture.first,
            &fixture.second,
            &fixture.artifacts,
        )
        .unwrap();
    assert!(first.changed_paths().contains(&"binary.bin".to_owned()));
    assert!(first.changed_paths().contains(&"deleted.txt".to_owned()));
    let expected_first_tree = first.observation().tree_id().to_owned();
    let expected_second_tree = second.observation().tree_id().to_owned();
    let durable_artifacts = vec![first.artifact_ref().clone(), second.artifact_ref().clone()];

    prune_provider_revisions_and_configure_filter(&fixture);
    let materialized_path = fixture.root.path().join("materialized");
    let materialized = adapter
        .materialize(
            &fixture.repository,
            &repository_id,
            &first,
            &fixture.artifacts,
            &materialized_path,
        )
        .unwrap();
    assert_eq!(materialized.resulting_tree, expected_first_tree);
    assert_eq!(
        fs::read(materialized_path.join("binary.bin")).unwrap(),
        [0, 1, 2, 255]
    );
    assert_materialization_states(&adapter, &fixture, &materialized);

    let candidate_path = fixture.root.path().join("candidate");
    let candidate = adapter
        .compose_candidate(
            &fixture.repository,
            &repository_id,
            &[first, second],
            &fixture.artifacts,
            &candidate_path,
        )
        .unwrap();
    assert_eq!(candidate.resulting_tree(), expected_second_tree);
    assert_eq!(candidate.overlapping_paths(), ["shared.txt"]);

    command(
        Some(&fixture.repository),
        ["update-ref", "refs/heads/target", &fixture.base],
    );
    let effect = EffectOperationId::new("effect\nwith-delimiter;safe").unwrap();
    let plan = adapter
        .plan_integration(
            &fixture.repository,
            &repository_id,
            "refs/heads/target",
            &fixture.base,
            &candidate,
            &effect,
        )
        .unwrap();
    let result = adapter
        .execute_integration(&fixture.repository, &repository_id, &plan)
        .unwrap();
    assert_eq!(result.result_tree, candidate.resulting_tree());
    let restarted_adapter = NativeGit::with_defaults();
    let restarted_candidate = restarted_adapter
        .reconstruct_candidate(
            &fixture.repository,
            &repository_id,
            &fixture.base,
            &durable_artifacts,
            candidate.resulting_tree(),
            &fixture.artifacts,
            &fixture.root.path().join("restarted-candidate"),
        )
        .unwrap();
    assert_eq!(
        restarted_candidate.resulting_tree(),
        candidate.resulting_tree()
    );
    let restarted_plan = restarted_adapter
        .rehydrate_integration_plan(
            &fixture.repository,
            &repository_id,
            &discovery.provider_locator_evidence,
            "refs/heads/target",
            &fixture.base,
            candidate.resulting_tree(),
            &effect,
        )
        .unwrap();
    assert!(matches!(
        restarted_adapter
            .reconcile_integration(&fixture.repository, &repository_id, &restarted_plan, None)
            .unwrap(),
        ReconciliationResult::ResultVerified(observed)
            if observed.result_revision == result.result_revision
    ));
    assert!(!result.evidence.contains("with-delimiter"));
}

fn prune_provider_revisions_and_configure_filter(fixture: &Fixture) {
    command(Some(&fixture.repository), ["switch", "--quiet", "main"]);
    command(
        Some(&fixture.repository),
        ["commit", "--quiet", "--allow-empty", "-m", "external"],
    );
    command(
        Some(&fixture.repository),
        ["branch", "-D", "change-a", "change-b"],
    );
    command(
        Some(&fixture.repository),
        ["reflog", "expire", "--expire=now", "--all"],
    );
    command(Some(&fixture.repository), ["gc", "--prune=now", "--quiet"]);
    let absent = Command::new("git")
        .args(["cat-file", "-e", &format!("{}^{{commit}}", fixture.second)])
        .current_dir(&fixture.repository)
        .output()
        .unwrap();
    assert!(!absent.status.success());
    let attributes = fixture.root.path().join("attributes");
    fs::write(&attributes, b"*.txt filter=weft-test\n").unwrap();
    command(
        Some(&fixture.repository),
        ["config", "core.attributesFile", path(&attributes)],
    );
    command(
        Some(&fixture.repository),
        ["config", "filter.weft-test.clean", "sed s/first/FILTERED/g"],
    );
    command(
        Some(&fixture.repository),
        ["config", "filter.weft-test.smudge", "cat"],
    );
    command(
        Some(&fixture.repository),
        ["config", "filter.weft-test.required", "true"],
    );
}

fn assert_materialization_states(
    adapter: &NativeGit,
    fixture: &Fixture,
    materialized: &MaterializationResult,
) {
    let materialized_path = &materialized.path;
    let (state, _, _) = adapter
        .observe_materialization(
            materialized_path,
            &materialized.base_commit,
            &materialized.resulting_tree,
        )
        .unwrap()
        .into_parts();
    assert_eq!(state, MaterializationState::Clean);
    assert_ancestor_symlink_is_dirty(adapter, fixture, materialized);
    fs::write(materialized_path.join("untracked.txt"), b"untracked\n").unwrap();
    let (state, _, _) = adapter
        .observe_materialization(
            materialized_path,
            &materialized.base_commit,
            &materialized.resulting_tree,
        )
        .unwrap()
        .into_parts();
    assert_eq!(state, MaterializationState::Dirty);
    fs::remove_file(materialized_path.join("untracked.txt")).unwrap();
    fs::write(materialized_path.join("shared.txt"), b"dirty\n").unwrap();
    let (state, _, _) = adapter
        .observe_materialization(
            materialized_path,
            &materialized.base_commit,
            &materialized.resulting_tree,
        )
        .unwrap()
        .into_parts();
    assert_eq!(state, MaterializationState::Dirty);
    command(
        Some(materialized_path),
        ["reset", "--quiet", "--hard", "main"],
    );
    let (state, _, _) = adapter
        .observe_materialization(
            materialized_path,
            &materialized.base_commit,
            &materialized.resulting_tree,
        )
        .unwrap()
        .into_parts();
    assert_eq!(state, MaterializationState::Diverged);
    adapter
        .release_materialization(&fixture.repository, materialized_path)
        .unwrap();
    assert!(!materialized_path.exists());
}

#[cfg(unix)]
fn assert_ancestor_symlink_is_dirty(
    adapter: &NativeGit,
    fixture: &Fixture,
    materialized: &MaterializationResult,
) {
    let nested = materialized.path.join("nested");
    let outside = fixture.root.path().join("outside-nested");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("file.txt"), b"nested\n").unwrap();
    fs::remove_dir_all(&nested).unwrap();
    std::os::unix::fs::symlink(&outside, &nested).unwrap();
    let (state, _, _) = adapter
        .observe_materialization(
            &materialized.path,
            &materialized.base_commit,
            &materialized.resulting_tree,
        )
        .unwrap()
        .into_parts();
    assert_eq!(state, MaterializationState::Dirty);
    fs::remove_file(&nested).unwrap();
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("file.txt"), b"nested\n").unwrap();
}

#[cfg(not(unix))]
fn assert_ancestor_symlink_is_dirty(
    _adapter: &NativeGit,
    _fixture: &Fixture,
    _materialized: &MaterializationResult,
) {
}

fn compose_fixture_candidate(
    adapter: &NativeGit,
    fixture: &Fixture,
    repository_id: &RepositoryId,
    destination: &str,
) -> CandidateComposition {
    let first = adapter
        .capture_revision(
            &fixture.repository,
            repository_id.clone(),
            &fixture.base,
            &fixture.first,
            &fixture.artifacts,
        )
        .unwrap();
    let second = adapter
        .capture_revision(
            &fixture.repository,
            repository_id.clone(),
            &fixture.first,
            &fixture.second,
            &fixture.artifacts,
        )
        .unwrap();
    adapter
        .compose_candidate(
            &fixture.repository,
            repository_id,
            &[first, second],
            &fixture.artifacts,
            &fixture.root.path().join(destination),
        )
        .unwrap()
}

#[test]
fn changed_target_is_guarded_and_reconciliation_reports_divergence() {
    let fixture = Fixture::new();
    let adapter = NativeGit::with_defaults();
    command(
        Some(&fixture.repository),
        ["update-ref", "refs/heads/target", &fixture.base],
    );
    let effect = EffectOperationId::new("effect-diverged").unwrap();
    let repository_id = RepositoryId::new("repo-1").unwrap();
    let candidate = compose_fixture_candidate(
        &adapter,
        &fixture,
        &repository_id,
        "changed-target-candidate",
    );
    assert_exact_base_binding(&adapter, &fixture, &repository_id, &effect);
    let plan = adapter
        .plan_integration(
            &fixture.repository,
            &repository_id,
            "refs/heads/target",
            &fixture.base,
            &candidate,
            &effect,
        )
        .unwrap();
    assert_plan_repository_binding(
        &adapter,
        &fixture,
        &repository_id,
        &candidate,
        &effect,
        &plan,
    );
    command(
        Some(&fixture.repository),
        [
            "update-ref",
            "refs/heads/target",
            &fixture.first,
            &fixture.base,
        ],
    );
    assert!(matches!(
        adapter.execute_integration(&fixture.repository, &repository_id, &plan),
        Err(GitProviderError::ChangedTarget { .. })
    ));
    assert!(matches!(
        adapter
            .reconcile_integration(&fixture.repository, &repository_id, &plan, None)
            .unwrap(),
        ReconciliationResult::Diverged { observed_target, .. }
            if observed_target == fixture.first
    ));
    assert_forged_merge_is_not_result(&adapter, &fixture, &candidate, &plan);

    command(
        Some(&fixture.repository),
        ["update-ref", "refs/heads/no-effect", &fixture.base],
    );
    let no_effect_plan = adapter
        .plan_integration(
            &fixture.repository,
            &repository_id,
            "refs/heads/no-effect",
            &fixture.base,
            &candidate,
            &effect,
        )
        .unwrap();
    assert!(matches!(
        adapter
            .reconcile_integration(&fixture.repository, &repository_id, &no_effect_plan, None,)
            .unwrap(),
        ReconciliationResult::StillUncertain { .. }
    ));
}

fn assert_plan_repository_binding(
    adapter: &NativeGit,
    fixture: &Fixture,
    repository_id: &RepositoryId,
    candidate: &CandidateComposition,
    effect: &EffectOperationId,
    plan: &IntegrationPlan,
) {
    assert!(matches!(
        adapter.execute_integration(
            &fixture.repository,
            &RepositoryId::new("wrong-repository").unwrap(),
            plan,
        ),
        Err(GitProviderError::VerificationFailed(_))
    ));
    let other_clone = fixture.root.path().join("other-clone");
    command(
        None,
        [
            "clone",
            "--quiet",
            path(&fixture.repository),
            path(&other_clone),
        ],
    );
    assert!(matches!(
        adapter.execute_integration(&other_clone, repository_id, plan),
        Err(GitProviderError::VerificationFailed(_))
    ));
    assert!(matches!(
        adapter.plan_integration(
            &fixture.repository,
            repository_id,
            "refs/tags/not-a-target",
            &fixture.base,
            candidate,
            effect,
        ),
        Err(GitProviderError::UnsafeTargetRef(_))
    ));
}

fn assert_exact_base_binding(
    adapter: &NativeGit,
    fixture: &Fixture,
    repository_id: &RepositoryId,
    effect: &EffectOperationId,
) {
    let base_tree = command(
        Some(&fixture.repository),
        ["rev-parse", &format!("{}^{{tree}}", fixture.base)],
    );
    let same_tree_commit = command(
        Some(&fixture.repository),
        [
            "commit-tree",
            &base_tree,
            "-p",
            &fixture.base,
            "-m",
            "same tree",
        ],
    );
    let wrong_revision = adapter
        .capture_revision(
            &fixture.repository,
            repository_id.clone(),
            &same_tree_commit,
            &fixture.first,
            &fixture.artifacts,
        )
        .unwrap();
    let wrong_base = adapter
        .compose_candidate(
            &fixture.repository,
            repository_id,
            &[wrong_revision],
            &fixture.artifacts,
            &fixture.root.path().join("wrong-base-candidate"),
        )
        .unwrap();
    assert!(matches!(
        adapter.plan_integration(
            &fixture.repository,
            repository_id,
            "refs/heads/target",
            &fixture.base,
            &wrong_base,
            effect,
        ),
        Err(GitProviderError::VerificationFailed(_))
    ));
}

fn assert_forged_merge_is_not_result(
    adapter: &NativeGit,
    fixture: &Fixture,
    candidate: &CandidateComposition,
    plan: &IntegrationPlan,
) {
    let forged = command(
        Some(&fixture.repository),
        [
            "commit-tree",
            plan.candidate_tree(),
            "-p",
            &fixture.base,
            "-p",
            &fixture.first,
            "-m",
            "Weft integration",
            "-m",
            "Weft-Effect-Operation-Hex: 6566666563742d6469766572676564",
        ],
    );
    command(
        Some(&fixture.repository),
        ["update-ref", "refs/heads/forged", &fixture.base],
    );
    let repository_id = plan.repository_id().clone();
    let forged_plan = adapter
        .plan_integration(
            &fixture.repository,
            &repository_id,
            "refs/heads/forged",
            &fixture.base,
            candidate,
            &EffectOperationId::new(plan.effect_operation_id()).unwrap(),
        )
        .unwrap();
    command(
        Some(&fixture.repository),
        ["update-ref", "refs/heads/forged", &forged, &fixture.base],
    );
    assert!(matches!(
        adapter
            .reconcile_integration(&fixture.repository, &repository_id, &forged_plan, None)
            .unwrap(),
        ReconciliationResult::Diverged { .. }
    ));
}

#[test]
fn merge_conflicts_are_normalized_to_exact_paths() {
    let fixture = Fixture::new();
    let adapter = NativeGit::with_defaults();
    command(
        Some(&fixture.repository),
        ["switch", "--quiet", "--detach", &fixture.base],
    );
    command(
        Some(&fixture.repository),
        ["switch", "--quiet", "-c", "conflict-left"],
    );
    fs::write(fixture.repository.join("shared.txt"), b"left\n").unwrap();
    command(
        Some(&fixture.repository),
        ["commit", "--quiet", "-am", "left"],
    );
    let left = command(Some(&fixture.repository), ["rev-parse", "HEAD"]);
    command(
        Some(&fixture.repository),
        ["switch", "--quiet", "--detach", &fixture.base],
    );
    command(
        Some(&fixture.repository),
        ["switch", "--quiet", "-c", "conflict-right"],
    );
    fs::write(fixture.repository.join("shared.txt"), b"right\n").unwrap();
    command(
        Some(&fixture.repository),
        ["commit", "--quiet", "-am", "right"],
    );
    let right = command(Some(&fixture.repository), ["rev-parse", "HEAD"]);

    let conflicts = adapter
        .detect_merge_conflicts(
            &fixture.repository,
            &left,
            &right,
            &fixture.root.path().join("conflict-probe"),
        )
        .unwrap();
    assert_eq!(conflicts, vec!["shared.txt"]);
}

#[test]
fn sha256_repository_uses_exact_sixty_four_digit_objects() {
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join("sha256-repository");
    command(
        None,
        [
            "init",
            "--quiet",
            "--initial-branch=main",
            "--object-format=sha256",
            path(&repository),
        ],
    );
    command(Some(&repository), ["config", "user.name", "Weft Test"]);
    command(
        Some(&repository),
        ["config", "user.email", "weft@test.invalid"],
    );
    command(Some(&repository), ["config", "commit.gpgSign", "false"]);
    fs::write(repository.join("file.txt"), b"base\n").unwrap();
    command(Some(&repository), ["add", "--all"]);
    command(Some(&repository), ["commit", "--quiet", "-m", "base"]);
    let base = command(Some(&repository), ["rev-parse", "HEAD"]);
    fs::write(repository.join("file.txt"), b"revision\n").unwrap();
    command(Some(&repository), ["commit", "--quiet", "-am", "revision"]);
    let revision = command(Some(&repository), ["rev-parse", "HEAD"]);
    let artifacts = ArtifactStore::open(root.path().join("cas")).unwrap();
    let adapter = NativeGit::with_defaults();
    assert_eq!(
        adapter.discover(&repository).unwrap().object_format,
        "sha256"
    );
    let repository_id = RepositoryId::new("sha256-repo").unwrap();
    let captured = adapter
        .capture_revision(
            &repository,
            repository_id.clone(),
            &base,
            &revision,
            &artifacts,
        )
        .unwrap();
    assert_eq!(captured.observation().commit_id().len(), 64);
    let materialized = adapter
        .materialize(
            &repository,
            &repository_id,
            &captured,
            &artifacts,
            &root.path().join("materialized"),
        )
        .unwrap();
    assert_eq!(
        materialized.resulting_tree,
        captured.observation().tree_id()
    );
}

#[test]
#[cfg(unix)]
fn failed_guarded_update_with_unchanged_target_is_not_stale() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let adapter = NativeGit::with_defaults();
    let repository_id = RepositoryId::new("repo-1").unwrap();
    command(
        Some(&fixture.repository),
        ["update-ref", "refs/heads/target", &fixture.base],
    );
    let candidate =
        compose_fixture_candidate(&adapter, &fixture, &repository_id, "permission-candidate");
    let plan = adapter
        .plan_integration(
            &fixture.repository,
            &repository_id,
            "refs/heads/target",
            &fixture.base,
            &candidate,
            &EffectOperationId::new("effect-permission").unwrap(),
        )
        .unwrap();
    let refs = fixture.repository.join(".git/refs/heads");
    let original = fs::metadata(&refs).unwrap().permissions();
    fs::set_permissions(&refs, fs::Permissions::from_mode(0o555)).unwrap();
    let result = adapter.execute_integration(&fixture.repository, &repository_id, &plan);
    fs::set_permissions(&refs, original).unwrap();
    assert!(matches!(
        result,
        Err(GitProviderError::CommandFailed {
            operation: "guarded-target-update",
            ..
        })
    ));
}

#[test]
#[cfg(unix)]
fn subprocess_deadline_and_output_limit_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let slow = root.path().join("slow-git");
    write_executable_script(&slow, b"#!/bin/sh\nsh -c 'while :; do :; done' &\nwait\n");
    let adapter = NativeGit::new(&slow, Duration::from_millis(25), 1024);
    let timeout_result = adapter.discover(root.path());
    assert!(
        matches!(
            timeout_result,
            Err(GitProviderError::CommandTimedOut {
                operation: "version"
            })
        ),
        "{timeout_result:?}"
    );

    let nonreading = root.path().join("nonreading-git");
    write_executable_script(
        &nonreading,
        b"#!/bin/sh\nsh -c 'while :; do :; done' &\nwait\n",
    );
    let input = vec![b'x'; 1024 * 1024];
    let started = std::time::Instant::now();
    let blocked_input = run_git(
        &nonreading,
        None,
        "blocked-stdin",
        ["--version"],
        Some(&input),
        CommandPolicy {
            timeout: Duration::from_millis(25),
            max_output_bytes: 1024,
        },
    );
    assert!(matches!(
        blocked_input,
        Err(GitProviderError::CommandTimedOut {
            operation: "blocked-stdin"
        })
    ));
    assert!(started.elapsed() < Duration::from_secs(1));

    let noisy = root.path().join("noisy-git");
    write_executable_script(
        &noisy,
        b"#!/bin/sh\nwhile :; do printf '01234567890123456789012345678901'; done\n",
    );
    let adapter = NativeGit::new(&noisy, Duration::from_secs(2), 128);
    assert!(matches!(
        adapter.discover(root.path()),
        Err(GitProviderError::OutputLimit {
            operation: "version"
        })
    ));
}

fn command<const N: usize>(directory: Option<&Path>, args: [&str; N]) -> String {
    let mut command = Command::new("git");
    command.args(args).env("GIT_CONFIG_NOSYSTEM", "1");
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[cfg(unix)]
fn write_executable_script(path: &Path, bytes: &[u8]) {
    use std::os::unix::fs::PermissionsExt;

    let temporary = path.with_extension("pending");
    let mut file = fs::File::create(&temporary).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    drop(file);
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755)).unwrap();
    fs::rename(temporary, path).unwrap();
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {}

#[cfg(unix)]
fn create_test_symlink(path: &Path) {
    std::os::unix::fs::symlink("shared.txt", path).unwrap();
}

#[cfg(not(unix))]
fn create_test_symlink(_path: &Path) {}
