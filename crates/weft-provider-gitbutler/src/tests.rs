use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;
use weft_artifact::ArtifactStore;
use weft_domain::{EffectOperationId, MaterializationState, ProviderRef, RepositoryId};

use super::*;
#[cfg(unix)]
use crate::command::{CommandPolicy, run};

const CHANGE_ONE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CHANGE_TWO: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct Fixture {
    root: TempDir,
    repository: PathBuf,
    status_path: PathBuf,
    but: PathBuf,
    base: String,
    first: String,
    second: String,
    repository_id: RepositoryId,
}

impl Fixture {
    #[allow(clippy::too_many_lines)]
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repository");
        command(
            None,
            [
                OsString::from("init"),
                OsString::from("--quiet"),
                OsString::from("--initial-branch=main"),
                repository.as_os_str().to_owned(),
            ],
        );
        command(
            Some(&repository),
            ["config", "user.name", "Weft GitButler Test"].map(OsString::from),
        );
        command(
            Some(&repository),
            ["config", "user.email", "weft-gitbutler@test.invalid"].map(OsString::from),
        );
        command(
            Some(&repository),
            ["config", "commit.gpgSign", "false"].map(OsString::from),
        );
        fs::write(repository.join("base.txt"), b"base\n").unwrap();
        command(Some(&repository), ["add", "--all"].map(OsString::from));
        command(
            Some(&repository),
            ["commit", "--quiet", "-m", "base"].map(OsString::from),
        );
        let base = output(Some(&repository), ["rev-parse", "HEAD"].map(OsString::from));

        fs::write(repository.join("one.txt"), b"one\n").unwrap();
        command(Some(&repository), ["add", "--all"].map(OsString::from));
        command(
            Some(&repository),
            ["commit", "--quiet", "-m", "one"].map(OsString::from),
        );
        let first = output(Some(&repository), ["rev-parse", "HEAD"].map(OsString::from));

        fs::write(repository.join("two.txt"), b"two\n").unwrap();
        command(Some(&repository), ["add", "--all"].map(OsString::from));
        command(
            Some(&repository),
            ["commit", "--quiet", "-m", "two"].map(OsString::from),
        );
        let second = output(Some(&repository), ["rev-parse", "HEAD"].map(OsString::from));

        command(
            Some(&repository),
            [
                "config",
                "gitbutler.project.targetref",
                "refs/remotes/gb-local/main",
            ]
            .map(OsString::from),
        );
        command(
            Some(&repository),
            ["remote", "add", "gb-local", repository.to_str().unwrap()].map(OsString::from),
        );
        command(
            Some(&repository),
            [
                OsString::from("update-ref"),
                OsString::from("refs/remotes/gb-local/main"),
                OsString::from(&base),
            ],
        );

        let status_path = root.path().join("status.json");
        fs::write(
            &status_path,
            serde_json::to_vec_pretty(&status(&base, &first, &second)).unwrap(),
        )
        .unwrap();
        let but = root.path().join("fake-but");
        fs::write(
            &but,
            br#"#!/bin/sh
set -eu
if [ "${1-}" = "--version" ]; then
  printf 'but 0.22.0\n'
  exit 0
fi
if [ "${3-}" = "--json" ] && [ "${4-}" = "status" ]; then
  exec /bin/cat "$WEFT_FAKE_STATUS"
fi
if [ "${3-}" = "land" ]; then
  case "${WEFT_FAKE_LAND_MODE-success}" in
    before-update-timeout) exec /bin/sleep 5 ;;
    success|after-update-timeout)
      git -C "$WEFT_FAKE_REPO" update-ref "$WEFT_FAKE_TARGET" "$WEFT_FAKE_RESULT" "$WEFT_FAKE_EXPECTED"
      if [ "${WEFT_FAKE_LAND_MODE-success}" = "after-update-timeout" ]; then
        exec /bin/sleep 5
      fi
      printf '{"landed":true}\n'
      exit 0
      ;;
    diverged)
      git -C "$WEFT_FAKE_REPO" update-ref "$WEFT_FAKE_TARGET" "$WEFT_FAKE_DIVERGED" "$WEFT_FAKE_EXPECTED"
      exit 0
      ;;
  esac
fi
exit 71
"#,
        )
        .unwrap();
        #[cfg(unix)]
        fs::set_permissions(&but, fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            root,
            repository,
            status_path,
            but,
            base,
            first,
            second,
            repository_id: RepositoryId::new("repo-gitbutler").unwrap(),
        }
    }

    fn adapter(&self) -> GitButler {
        self.adapter_with(Duration::from_secs(2), "success")
    }

    fn adapter_with(&self, timeout: Duration, mode: &str) -> GitButler {
        GitButler::new(&self.but, "git", timeout, 1024 * 1024).with_environment([
            ("WEFT_FAKE_STATUS", self.status_path.as_os_str()),
            ("WEFT_FAKE_REPO", self.repository.as_os_str()),
            (
                "WEFT_FAKE_TARGET",
                Path::new("refs/remotes/gb-local/main").as_os_str(),
            ),
            ("WEFT_FAKE_RESULT", Path::new(&self.second).as_os_str()),
            ("WEFT_FAKE_DIVERGED", Path::new(&self.first).as_os_str()),
            ("WEFT_FAKE_EXPECTED", Path::new(&self.base).as_os_str()),
            ("WEFT_FAKE_LAND_MODE", Path::new(mode).as_os_str()),
        ])
    }

    fn reset_target(&self) {
        command(
            Some(&self.repository),
            [
                OsString::from("update-ref"),
                OsString::from("refs/remotes/gb-local/main"),
                OsString::from(&self.base),
            ],
        );
    }
}

#[test]
fn strict_discovery_normalizes_exact_change_and_stack_identity() {
    let fixture = Fixture::new();
    let adapter = fixture.adapter();
    let discovery = adapter
        .discover(&fixture.repository, fixture.repository_id.clone())
        .unwrap();
    assert_eq!(discovery.version(), "0.22.0");
    assert!(discovery.local_target());
    assert!(
        discovery
            .capabilities()
            .supports(GitButlerCapability::StackMapping)
    );
    assert!(
        discovery
            .capabilities()
            .supports(GitButlerCapability::GuardedLocalFastForwardLanding)
    );
    assert!(
        adapter
            .require(&discovery, GitButlerCapability::ProviderReconnect)
            .is_err()
    );
    assert!(
        adapter
            .require(&discovery, GitButlerCapability::CanonicalImport)
            .is_err()
    );

    let observation = adapter.observe(&discovery, &fixture.repository_id).unwrap();
    assert_eq!(observation.merge_base(), fixture.base);
    assert_eq!(observation.upstream_target(), fixture.base);
    assert_eq!(observation.stacks().len(), 1);
    let stack = &observation.stacks()[0];
    assert_eq!(stack.branch_names_base_to_tip(), ["lower", "upper"]);
    assert_eq!(
        stack
            .changes_base_to_tip()
            .iter()
            .map(|change| change.provider_ref().as_str())
            .collect::<Vec<_>>(),
        [CHANGE_ONE, CHANGE_TWO]
    );
    assert_eq!(
        observation
            .materialization_observation(&ProviderRef::new(CHANGE_ONE).unwrap())
            .unwrap()
            .into_parts()
            .0,
        MaterializationState::Clean
    );

    let mut dirty = status(&fixture.base, &fixture.first, &fixture.second);
    dirty["uncommittedChanges"] = json!([{
        "cliId": "d1",
        "filePath": "dirty.txt",
        "changeType": "added"
    }]);
    fs::write(
        &fixture.status_path,
        serde_json::to_vec_pretty(&dirty).unwrap(),
    )
    .unwrap();
    let dirty_observation = adapter.observe(&discovery, &fixture.repository_id).unwrap();
    assert_eq!(
        dirty_observation
            .materialization_observation(&ProviderRef::new(CHANGE_ONE).unwrap())
            .unwrap()
            .into_parts()
            .0,
        MaterializationState::Dirty
    );
    fs::write(
        &fixture.status_path,
        serde_json::to_vec_pretty(&status(&fixture.base, &fixture.first, &fixture.second)).unwrap(),
    )
    .unwrap();

    let candidate = adapter.candidate(&observation, "s1").unwrap();
    assert_eq!(candidate.inputs().len(), 2);
    assert_eq!(candidate.inputs()[1].commit_id(), fixture.second);
    assert_ne!(candidate.repository_id().as_str(), CHANGE_ONE);
}

#[test]
fn canonical_export_survives_provider_status_removal() {
    let fixture = Fixture::new();
    let adapter = fixture.adapter();
    let discovery = adapter
        .discover(&fixture.repository, fixture.repository_id.clone())
        .unwrap();
    let artifacts = ArtifactStore::open(fixture.root.path().join("cas")).unwrap();
    let exported = adapter
        .export_canonical(
            &discovery,
            &fixture.repository_id,
            &ProviderRef::new(CHANGE_ONE).unwrap(),
            &fixture.first,
            &fixture.base,
            &artifacts,
        )
        .unwrap();
    assert_eq!(exported.commit_id(), fixture.first);
    assert_eq!(exported.captured().changed_paths(), ["one.txt"]);

    fs::write(
        &fixture.status_path,
        serde_json::to_vec_pretty(&empty_status(&fixture.base)).unwrap(),
    )
    .unwrap();
    assert!(
        artifacts
            .load_manifest(exported.captured().artifact_ref())
            .is_ok()
    );
    assert!(
        adapter
            .export_canonical(
                &discovery,
                &fixture.repository_id,
                &ProviderRef::new(CHANGE_ONE).unwrap(),
                &fixture.first,
                &fixture.base,
                &artifacts,
            )
            .is_err()
    );
}

#[test]
fn unknown_json_and_wrong_identity_fail_closed() {
    let fixture = Fixture::new();
    let adapter = fixture.adapter();
    let discovery = adapter
        .discover(&fixture.repository, fixture.repository_id.clone())
        .unwrap();
    assert!(
        adapter
            .observe(&discovery, &RepositoryId::new("wrong").unwrap())
            .is_err()
    );

    let mut value = status(&fixture.base, &fixture.first, &fixture.second);
    value
        .as_object_mut()
        .unwrap()
        .insert("futureField".to_owned(), json!(true));
    fs::write(&fixture.status_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let error = adapter
        .observe(&discovery, &fixture.repository_id)
        .unwrap_err();
    assert!(matches!(
        error,
        GitButlerProviderError::InvalidOutput { .. }
    ));
}

#[test]
fn malformed_nested_schema_identity_and_stack_shapes_fail_closed() {
    let fixture = Fixture::new();
    let adapter = fixture.adapter();
    let discovery = adapter
        .discover(&fixture.repository, fixture.repository_id.clone())
        .unwrap();

    let mut cases = Vec::new();
    let mut nested_unknown = status(&fixture.base, &fixture.first, &fixture.second);
    nested_unknown["stacks"][0]["branches"][0]["futureField"] = json!(true);
    cases.push(nested_unknown);

    let mut duplicate_branch_cli = status(&fixture.base, &fixture.first, &fixture.second);
    duplicate_branch_cli["stacks"][0]["branches"][1]["cliId"] = json!("u1");
    cases.push(duplicate_branch_cli);

    let mut duplicate_branch_name = status(&fixture.base, &fixture.first, &fixture.second);
    duplicate_branch_name["stacks"][0]["branches"][1]["name"] = json!("upper");
    cases.push(duplicate_branch_name);

    let cross_stack = one_change_status(&fixture.base, CHANGE_ONE, &fixture.first, false);
    let mut duplicate_top_cli_across_stacks = cross_stack.clone();
    let mut second_stack = duplicate_top_cli_across_stacks["stacks"][0].clone();
    second_stack["cliId"] = json!("s2");
    second_stack["branches"][0]["name"] = json!("parallel");
    duplicate_top_cli_across_stacks["stacks"]
        .as_array_mut()
        .unwrap()
        .push(second_stack);
    cases.push(duplicate_top_cli_across_stacks);

    let mut duplicate_name_across_stacks = cross_stack;
    let mut second_stack = duplicate_name_across_stacks["stacks"][0].clone();
    second_stack["cliId"] = json!("s2");
    second_stack["branches"][0]["cliId"] = json!("parallel-cli");
    duplicate_name_across_stacks["stacks"]
        .as_array_mut()
        .unwrap()
        .push(second_stack);
    cases.push(duplicate_name_across_stacks);

    let mut duplicate_change = status(&fixture.base, &fixture.first, &fixture.second);
    duplicate_change["stacks"][0]["branches"][1]["commits"][0]["changeId"] = json!(CHANGE_TWO);
    cases.push(duplicate_change);

    let mut malformed_ancestry = status(&fixture.base, &fixture.first, &fixture.second);
    malformed_ancestry["stacks"][0]["branches"][1]["commits"][0]["commitId"] =
        json!(fixture.second);
    cases.push(malformed_ancestry);

    let mut invalid_object = status(&fixture.base, &fixture.first, &fixture.second);
    invalid_object["stacks"][0]["branches"][0]["commits"][0]["commitId"] = json!("not-a-sha1");
    cases.push(invalid_object);

    let mut empty_branch = status(&fixture.base, &fixture.first, &fixture.second);
    empty_branch["stacks"][0]["branches"][0]["commits"] = json!([]);
    cases.push(empty_branch);

    let mut published = status(&fixture.base, &fixture.first, &fixture.second);
    published["stacks"][0]["branches"][0]["upstreamCommits"] = json!([base_commit(&fixture.base)]);
    cases.push(published);

    let mut assigned = status(&fixture.base, &fixture.first, &fixture.second);
    assigned["stacks"][0]["assignedChanges"] = json!([{"cliId": "assigned"}]);
    cases.push(assigned);

    let mut extended_branch = status(&fixture.base, &fixture.first, &fixture.second);
    extended_branch["stacks"][0]["branches"][0]["ci"] = json!({"status": "passed"});
    cases.push(extended_branch);

    for value in cases {
        fs::write(&fixture.status_path, serde_json::to_vec(&value).unwrap()).unwrap();
        let error = adapter
            .observe(&discovery, &fixture.repository_id)
            .unwrap_err();
        assert!(matches!(
            error,
            GitButlerProviderError::InvalidOutput { .. }
                | GitButlerProviderError::Unsupported { .. }
        ));
    }
}

#[test]
fn project_reconciliation_reports_rewrite_missing_new_and_conflict_evidence() {
    let fixture = Fixture::new();
    let adapter = fixture.adapter();
    let discovery = adapter
        .discover(&fixture.repository, fixture.repository_id.clone())
        .unwrap();
    let previous = adapter.observe(&discovery, &fixture.repository_id).unwrap();

    fs::write(fixture.repository.join("replacement.txt"), b"replacement\n").unwrap();
    command(
        Some(&fixture.repository),
        ["add", "--all"].map(OsString::from),
    );
    let tree = output(
        Some(&fixture.repository),
        ["write-tree"].map(OsString::from),
    );
    let replacement = command_with_input(
        &fixture.repository,
        [
            OsString::from("commit-tree"),
            OsString::from(&tree),
            OsString::from("-p"),
            OsString::from(&fixture.base),
        ],
        b"replacement\n",
    );
    let changed = one_change_status(&fixture.base, CHANGE_ONE, &replacement, true);
    fs::write(
        &fixture.status_path,
        serde_json::to_vec_pretty(&changed).unwrap(),
    )
    .unwrap();
    let reconciled = adapter
        .reconcile_project(&discovery, &fixture.repository_id, &previous)
        .unwrap();
    assert_eq!(
        reconciled.rewritten_provider_refs,
        [ProviderRef::new(CHANGE_ONE).unwrap()]
    );
    assert_eq!(
        reconciled.missing_provider_refs,
        [ProviderRef::new(CHANGE_TWO).unwrap()]
    );
    assert_eq!(reconciled.conflicts.len(), 1);
    assert_eq!(reconciled.conflicts[0].provider_ref().as_str(), CHANGE_ONE);
}

#[test]
fn guarded_landing_verifies_success_and_both_timeout_boundaries() {
    let fixture = Fixture::new();
    let adapter = fixture.adapter();
    let discovery = adapter
        .discover(&fixture.repository, fixture.repository_id.clone())
        .unwrap();
    let observation = adapter.observe(&discovery, &fixture.repository_id).unwrap();
    let candidate = adapter.candidate(&observation, "s1").unwrap();
    let plan = adapter
        .plan_local_landing(
            &discovery,
            &fixture.repository_id,
            &candidate,
            &fixture.base,
            &EffectOperationId::new("effect-gitbutler-land").unwrap(),
        )
        .unwrap();
    let landed = adapter
        .execute_local_landing(&discovery, &fixture.repository_id, &plan)
        .unwrap();
    assert!(matches!(
        landed,
        LandingReconciliation::ResultVerified(ref result)
            if result.result_revision == fixture.second
                && result.effect_operation_id == "effect-gitbutler-land"
    ));

    fixture.reset_target();
    let post_spawn_failure = fixture.adapter().with_post_spawn_failure();
    let recovered = post_spawn_failure
        .execute_local_landing(&discovery, &fixture.repository_id, &plan)
        .unwrap();
    assert!(matches!(
        recovered,
        LandingReconciliation::ResultVerified(_)
    ));

    fixture.reset_target();
    let timeout_after = fixture.adapter_with(Duration::from_millis(80), "after-update-timeout");
    let reconciled = timeout_after
        .execute_local_landing(&discovery, &fixture.repository_id, &plan)
        .unwrap();
    assert!(matches!(
        reconciled,
        LandingReconciliation::ResultVerified(_)
    ));

    fixture.reset_target();
    let timeout_before = fixture.adapter_with(Duration::from_millis(80), "before-update-timeout");
    let uncertain = timeout_before
        .execute_local_landing(&discovery, &fixture.repository_id, &plan)
        .unwrap();
    assert!(matches!(
        uncertain,
        LandingReconciliation::StillUncertain { ref observed_target, .. }
            if observed_target == &fixture.base
    ));

    command(
        Some(&fixture.repository),
        [
            OsString::from("update-ref"),
            OsString::from("refs/remotes/gb-local/main"),
            OsString::from(&fixture.second),
            OsString::from(&fixture.base),
        ],
    );
    command(
        Some(&fixture.repository),
        [
            OsString::from("update-ref"),
            OsString::from("refs/remotes/gb-local/main"),
            OsString::from(&fixture.base),
            OsString::from(&fixture.second),
        ],
    );
    assert!(matches!(
        adapter
            .reconcile_local_landing(&discovery, &fixture.repository_id, &plan)
            .unwrap(),
        LandingReconciliation::StillUncertain { .. }
    ));

    let diverged_adapter = fixture.adapter_with(Duration::from_secs(2), "diverged");
    let diverged = diverged_adapter
        .execute_local_landing(&discovery, &fixture.repository_id, &plan)
        .unwrap();
    assert!(matches!(
        diverged,
        LandingReconciliation::Diverged { ref observed_target, .. }
            if observed_target == &fixture.first
    ));
}

#[cfg(unix)]
#[test]
fn gitbutler_command_bounds_terminate_output_and_descendant_holders() {
    let root = tempfile::tempdir().unwrap();
    let script = root.path().join("bounded-but");
    let survivor = root.path().join("survivor");
    fs::write(
        &script,
        br#"#!/bin/sh
set -eu
case "$WEFT_BOUND_MODE" in
  output) while :; do printf '0123456789'; done ;;
  descendant) (sleep 1; : > "$WEFT_SURVIVOR") & wait ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let output_error = run(
        &script,
        None,
        "bounded-output",
        std::iter::empty::<&str>(),
        CommandPolicy {
            timeout: Duration::from_secs(2),
            max_output_bytes: 64,
            inject_post_spawn_failure: false,
        },
        &[(OsString::from("WEFT_BOUND_MODE"), OsString::from("output"))],
    )
    .unwrap_err();
    assert!(matches!(
        output_error,
        GitButlerProviderError::OutputLimit { .. }
    ));

    let timeout_error = run(
        &script,
        None,
        "bounded-descendant",
        std::iter::empty::<&str>(),
        CommandPolicy {
            timeout: Duration::from_millis(80),
            max_output_bytes: 1024,
            inject_post_spawn_failure: false,
        },
        &[
            (
                OsString::from("WEFT_BOUND_MODE"),
                OsString::from("descendant"),
            ),
            (
                OsString::from("WEFT_SURVIVOR"),
                survivor.as_os_str().to_owned(),
            ),
        ],
    )
    .unwrap_err();
    assert!(matches!(
        timeout_error,
        GitButlerProviderError::CommandTimedOut { .. }
    ));
    std::thread::sleep(Duration::from_millis(1_100));
    assert!(!survivor.exists());
}

#[test]
#[ignore = "requires the explicitly tested GitButler CLI 0.22.0"]
#[allow(clippy::too_many_lines)]
fn live_gitbutler_0_22_stack_export_and_local_landing() {
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join("repository");
    command(
        None,
        [
            OsString::from("init"),
            OsString::from("--quiet"),
            OsString::from("--initial-branch=main"),
            repository.as_os_str().to_owned(),
        ],
    );
    command(
        Some(&repository),
        ["config", "user.name", "Weft Live Test"].map(OsString::from),
    );
    command(
        Some(&repository),
        ["config", "user.email", "weft-live@test.invalid"].map(OsString::from),
    );
    command(
        Some(&repository),
        ["config", "commit.gpgSign", "false"].map(OsString::from),
    );
    fs::write(repository.join("base.txt"), b"base\n").unwrap();
    command(Some(&repository), ["add", "--all"].map(OsString::from));
    command(
        Some(&repository),
        ["commit", "--quiet", "-m", "base"].map(OsString::from),
    );
    let base = output(Some(&repository), ["rev-parse", "HEAD"].map(OsString::from));
    let xdg_data = root.path().join("xdg-data");
    let xdg_config = root.path().join("xdg-config");
    let xdg_cache = root.path().join("xdg-cache");
    fs::create_dir_all(&xdg_data).unwrap();
    fs::create_dir_all(&xdg_config).unwrap();
    fs::create_dir_all(&xdg_cache).unwrap();
    let environment = [
        (
            OsString::from("XDG_DATA_HOME"),
            xdg_data.as_os_str().to_owned(),
        ),
        (
            OsString::from("XDG_CONFIG_HOME"),
            xdg_config.as_os_str().to_owned(),
        ),
        (
            OsString::from("XDG_CACHE_HOME"),
            xdg_cache.as_os_str().to_owned(),
        ),
    ];
    assert_eq!(
        program_output("but", None, [OsString::from("--version")], &environment),
        "but 0.22.0"
    );
    program(
        "but",
        None,
        [
            OsString::from("-C"),
            repository.as_os_str().to_owned(),
            OsString::from("setup"),
            OsString::from("--init"),
        ],
        &environment,
    );
    fs::write(repository.join("one.txt"), b"one\n").unwrap();
    program(
        "but",
        None,
        [
            OsString::from("-C"),
            repository.as_os_str().to_owned(),
            OsString::from("commit"),
            OsString::from("-b"),
            OsString::from("lower"),
            OsString::from("-m"),
            OsString::from("one"),
        ],
        &environment,
    );
    program(
        "but",
        None,
        [
            OsString::from("-C"),
            repository.as_os_str().to_owned(),
            OsString::from("branch"),
            OsString::from("new"),
            OsString::from("upper"),
            OsString::from("--anchor"),
            OsString::from("lower"),
        ],
        &environment,
    );
    fs::write(repository.join("two.txt"), b"two\n").unwrap();
    program(
        "but",
        None,
        [
            OsString::from("-C"),
            repository.as_os_str().to_owned(),
            OsString::from("commit"),
            OsString::from("-b"),
            OsString::from("upper"),
            OsString::from("-m"),
            OsString::from("two"),
        ],
        &environment,
    );

    let adapter = GitButler::with_defaults().with_environment(environment.clone());
    let repository_id = RepositoryId::new("repo-live-gitbutler").unwrap();
    let discovery = adapter
        .discover(&repository, repository_id.clone())
        .unwrap();
    let observation = adapter.observe(&discovery, &repository_id).unwrap();
    assert_eq!(observation.merge_base(), base);
    assert_eq!(observation.stacks().len(), 1);
    let candidate = adapter
        .candidate(&observation, observation.stacks()[0].cli_id())
        .unwrap();
    assert_eq!(candidate.inputs().len(), 2);
    let artifacts = ArtifactStore::open(root.path().join("cas")).unwrap();
    let exported = adapter
        .export_canonical(
            &discovery,
            &repository_id,
            candidate.inputs()[0].provider_ref(),
            candidate.inputs()[0].commit_id(),
            &base,
            &artifacts,
        )
        .unwrap();
    assert_eq!(exported.captured().changed_paths(), ["one.txt"]);
    let plan = adapter
        .plan_local_landing(
            &discovery,
            &repository_id,
            &candidate,
            &base,
            &EffectOperationId::new("effect-live-gitbutler").unwrap(),
        )
        .unwrap();
    let result = adapter
        .execute_local_landing(&discovery, &repository_id, &plan)
        .unwrap();
    assert!(matches!(
        result,
        LandingReconciliation::ResultVerified(ref result)
            if result.result_revision == candidate.inputs()[1].commit_id()
    ));

    fs::write(repository.join("shared.txt"), b"branch version\n").unwrap();
    program(
        "but",
        None,
        [
            OsString::from("-C"),
            repository.as_os_str().to_owned(),
            OsString::from("commit"),
            OsString::from("-b"),
            OsString::from("conflict-change"),
            OsString::from("-m"),
            OsString::from("conflict change"),
        ],
        &environment,
    );
    let before_external = adapter.observe(&discovery, &repository_id).unwrap();
    let conflict_ref = before_external.stacks()[0].changes_base_to_tip()[0]
        .provider_ref()
        .clone();
    let recorded_target = output(
        Some(&repository),
        ["rev-parse", "refs/heads/main"].map(OsString::from),
    );
    let external_blob = command_with_input(
        &repository,
        ["hash-object", "-w", "--stdin"].map(OsString::from),
        b"external version\n",
    );
    let mut tree_input = Command::new("git")
        .args(["ls-tree", &recorded_target])
        .current_dir(&repository)
        .output()
        .unwrap()
        .stdout;
    tree_input.extend_from_slice(format!("100644 blob {external_blob}\tshared.txt\n").as_bytes());
    let external_tree =
        command_with_input(&repository, ["mktree"].map(OsString::from), &tree_input);
    let external_target = command_with_input(
        &repository,
        [
            OsString::from("commit-tree"),
            OsString::from(&external_tree),
            OsString::from("-p"),
            OsString::from(&recorded_target),
        ],
        b"external target\n",
    );
    command(
        Some(&repository),
        [
            OsString::from("update-ref"),
            OsString::from("refs/heads/main"),
            OsString::from(&external_target),
            OsString::from(&recorded_target),
        ],
    );
    program(
        "but",
        None,
        [
            OsString::from("-C"),
            repository.as_os_str().to_owned(),
            OsString::from("pull"),
        ],
        &environment,
    );
    let reconciled = adapter
        .reconcile_project(&discovery, &repository_id, &before_external)
        .unwrap();
    assert_eq!(reconciled.observed_target, external_target);
    assert_eq!(reconciled.conflicts.len(), 1);
    assert_eq!(reconciled.conflicts[0].provider_ref(), &conflict_ref);
}

fn status(base: &str, first: &str, second: &str) -> Value {
    json!({
        "uncommittedChanges": [],
        "stacks": [{
            "cliId": "s1",
            "assignedChanges": [],
            "branches": [
                branch("upper", "u1", &[commit(CHANGE_TWO, second, false)]),
                branch("lower", "l1", &[commit(CHANGE_ONE, first, false)])
            ]
        }],
        "mergeBase": base_commit(base),
        "upstreamState": {
            "behind": 0,
            "latestCommit": base_commit(base),
            "lastFetched": null
        }
    })
}

fn one_change_status(base: &str, change_id: &str, commit_id: &str, conflicted: bool) -> Value {
    json!({
        "uncommittedChanges": [],
        "stacks": [{
            "cliId": "s1",
            "assignedChanges": [],
            "branches": [branch("lower", "l1", &[commit(change_id, commit_id, conflicted)])]
        }],
        "mergeBase": base_commit(base),
        "upstreamState": {
            "behind": 0,
            "latestCommit": base_commit(base),
            "lastFetched": null
        }
    })
}

fn empty_status(base: &str) -> Value {
    json!({
        "uncommittedChanges": [],
        "stacks": [],
        "mergeBase": base_commit(base),
        "upstreamState": {
            "behind": 0,
            "latestCommit": base_commit(base),
            "lastFetched": null
        }
    })
}

fn branch(name: &str, cli_id: &str, commits: &[Value]) -> Value {
    json!({
        "cliId": cli_id,
        "name": name,
        "commits": commits,
        "upstreamCommits": [],
        "branchStatus": "completelyUnpushed",
        "reviewId": null,
        "ci": null
    })
}

fn commit(change_id: &str, commit_id: &str, conflicted: bool) -> Value {
    json!({
        "cliId": &change_id[..3],
        "changeId": change_id,
        "commitId": commit_id,
        "createdAt": "2026-08-26T00:00:00+00:00",
        "message": "fixture",
        "authorName": "Weft Test",
        "authorEmail": "weft@test.invalid",
        "conflicted": conflicted,
        "reviewId": null,
        "changes": null
    })
}

fn base_commit(commit_id: &str) -> Value {
    json!({
        "cliId": "",
        "commitId": commit_id,
        "createdAt": "2026-08-26T00:00:00+00:00",
        "message": "base",
        "authorName": "Weft Test",
        "authorEmail": "weft@test.invalid",
        "conflicted": null,
        "reviewId": null,
        "changes": null
    })
}

fn command<I>(directory: Option<&Path>, args: I)
where
    I: IntoIterator<Item = OsString>,
{
    let status = Command::new("git")
        .args(args)
        .current_dir(directory.unwrap_or_else(|| Path::new("/")))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .unwrap();
    assert!(status.success());
}

fn output<I>(directory: Option<&Path>, args: I) -> String
where
    I: IntoIterator<Item = OsString>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(directory.unwrap_or_else(|| Path::new("/")))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn command_with_input(
    repository: &Path,
    args: impl IntoIterator<Item = OsString>,
    input: &[u8],
) -> String {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("git")
        .args(args)
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn program<I>(binary: &str, directory: Option<&Path>, args: I, environment: &[(OsString, OsString)])
where
    I: IntoIterator<Item = OsString>,
{
    let status = Command::new(binary)
        .args(args)
        .current_dir(directory.unwrap_or_else(|| Path::new("/")))
        .envs(environment.iter().cloned())
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .unwrap();
    assert!(status.success());
}

fn program_output<I>(
    binary: &str,
    directory: Option<&Path>,
    args: I,
    environment: &[(OsString, OsString)],
) -> String
where
    I: IntoIterator<Item = OsString>,
{
    let output = Command::new(binary)
        .args(args)
        .current_dir(directory.unwrap_or_else(|| Path::new("/")))
        .envs(environment.iter().cloned())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
