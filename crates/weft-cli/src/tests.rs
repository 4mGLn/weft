use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use weft_artifact::{ArtifactStore, CanonicalTreeDelta};
use weft_domain::{BaseState, FileMode, PathOperation, RepositoryId, TreeDelta};

use super::run;

struct ResultOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

#[test]
fn json_contract_is_single_object_noninteractive_and_restart_safe() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let init = invoke(&state, ["init"]);
    assert_eq!(init.code, 0);
    assert!(init.stderr.is_empty());
    let init_json = parse_one(&init.stdout);
    assert_eq!(init_json["schema"], "weft.cli.v1");
    assert_eq!(init_json["ok"], true);
    assert_eq!(init_json["command"], "init");
    assert_eq!(init_json["data"]["metadata_schema_version"], 7);

    let created = invoke(
        &state,
        [
            "change",
            "create",
            "--change-id",
            "change-1",
            "--operation-id",
            "op-create-1",
            "--actor",
            "agent-1",
            "--at",
            "1000",
        ],
    );
    assert_eq!(created.code, 0, "{}", created.stderr);
    assert_eq!(parse_one(&created.stdout)["data"]["change_id"], "change-1");

    let replay = invoke(
        &state,
        [
            "change",
            "create",
            "--change-id",
            "change-1",
            "--operation-id",
            "op-create-1",
            "--actor",
            "agent-1",
            "--at",
            "1000",
        ],
    );
    assert_eq!(replay.code, 0);

    let shown = invoke(&state, ["change", "show", "--change-id", "change-1"]);
    assert_eq!(shown.code, 0);
    let shown = parse_one(&shown.stdout);
    assert_eq!(shown["data"]["head_revision_id"], Value::Null);
    assert_eq!(shown["data"]["revisions"], serde_json::json!([]));

    let history = invoke(&state, ["change", "history", "--change-id", "change-1"]);
    assert_eq!(history.code, 0);
    assert_eq!(
        parse_one(&history.stdout)["data"]["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn revision_append_requires_exact_artifact_head_and_operation_intent() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    assert_eq!(invoke(&state, ["init"]).code, 0);
    assert_eq!(
        invoke(
            &state,
            [
                "change",
                "create",
                "--change-id",
                "change-1",
                "--operation-id",
                "op-create",
                "--actor",
                "agent",
                "--at",
                "1000",
            ],
        )
        .code,
        0
    );
    let artifact = store_artifact(&state, "repo-1", "base-1");
    let append_arguments = [
        "revision",
        "append",
        "--change-id",
        "change-1",
        "--revision-id",
        "revision-1",
        "--expected-head",
        "none",
        "--repository-id",
        "repo-1",
        "--base-object",
        "base-1",
        "--artifact-digest",
        artifact.as_str(),
        "--operation-id",
        "op-revision-1",
        "--actor",
        "agent",
        "--at",
        "1100",
    ];
    let appended = invoke(&state, append_arguments);
    assert_eq!(appended.code, 0, "{}", appended.stderr);
    assert_eq!(
        parse_one(&appended.stdout)["data"]["head_revision_id"],
        "revision-1"
    );
    assert_eq!(invoke(&state, append_arguments).code, 0);

    let stale_result = invoke(
        &state,
        [
            "revision",
            "append",
            "--change-id",
            "change-1",
            "--revision-id",
            "revision-2",
            "--expected-head",
            "none",
            "--repository-id",
            "repo-1",
            "--base-object",
            "base-1",
            "--artifact-digest",
            artifact.as_str(),
            "--operation-id",
            "op-revision-2",
            "--actor",
            "agent",
            "--at",
            "1200",
        ],
    );
    assert_eq!(stale_result.code, 4);
    let stale_json = parse_one(&stale_result.stdout);
    assert_eq!(stale_json["error"]["code"], "conflict");
    assert_eq!(stale_json["error"]["retryable"], true);
}

#[test]
fn invalid_usage_and_operation_conflicts_have_stable_exits_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let uninitialized = invoke(&state, ["change", "show", "--change-id", "missing"]);
    assert_eq!(uninitialized.code, 2);
    assert_eq!(parse_one(&uninitialized.stdout)["error"]["code"], "usage");

    assert_eq!(invoke(&state, ["init"]).code, 0);
    let unknown = invoke(
        &state,
        [
            "change",
            "create",
            "--change-id",
            "change-1",
            "--operation-id",
            "op-1",
            "--actor",
            "agent",
            "--at",
            "1000",
            "--future",
            "value",
        ],
    );
    assert_eq!(unknown.code, 2);
    assert_eq!(
        invoke(&state, ["change", "show", "--change-id", "change-1"]).code,
        3
    );

    assert_eq!(
        invoke(
            &state,
            [
                "change",
                "create",
                "--change-id",
                "change-1",
                "--operation-id",
                "shared-op",
                "--actor",
                "agent",
                "--at",
                "1000",
            ],
        )
        .code,
        0
    );
    let conflict = invoke(
        &state,
        [
            "change",
            "create",
            "--change-id",
            "change-2",
            "--operation-id",
            "shared-op",
            "--actor",
            "agent",
            "--at",
            "1000",
        ],
    );
    assert_eq!(conflict.code, 4);
    assert_eq!(parse_one(&conflict.stdout)["error"]["code"], "conflict");
}

#[test]
fn human_mode_uses_stdout_for_success_and_stderr_for_failure() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        vec![
            OsString::from("--state-dir"),
            state.as_os_str().to_owned(),
            OsString::from("init"),
        ],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0);
    assert!(
        String::from_utf8(stdout)
            .unwrap()
            .starts_with("initialized Weft state")
    );
    assert!(stderr.is_empty());

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(vec![OsString::from("unknown")], &mut stdout, &mut stderr);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(
        String::from_utf8(stderr)
            .unwrap()
            .starts_with("error[usage]:")
    );
}

#[test]
fn short_version_and_verbose_flags_preserve_machine_output() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(vec![OsString::from("-V")], &mut stdout, &mut stderr);
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8(stdout).unwrap(), "weft 0.2.0\n");
    assert!(stderr.is_empty());

    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        vec![
            OsString::from("--format"),
            OsString::from("json"),
            OsString::from("-v"),
            OsString::from("--state-dir"),
            state.as_os_str().to_owned(),
            OsString::from("init"),
        ],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0);
    assert_eq!(
        parse_one(&String::from_utf8(stdout).unwrap())["command"],
        "init"
    );
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        format!(
            "weft: command=init format=json state-dir={}\n",
            state.display()
        )
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn setup_wires_project_context_idempotently_and_denies_malformed_markers() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let state = root.path().join("state");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("AGENTS.md"), "# Existing project rules\n").unwrap();

    let setup = invoke_os(
        &state,
        vec![
            OsString::from("setup"),
            OsString::from("--project-dir"),
            project.as_os_str().to_owned(),
            OsString::from("--runtime"),
            OsString::from("codex,claude-code,gemini-cli,paseo"),
        ],
    );
    assert_eq!(setup.code, 0, "{}", setup.stderr);
    let setup = parse_one(&setup.stdout);
    assert_eq!(setup["command"], "setup");
    assert_eq!(setup["data"]["bridge_schema"], "weft.runtime-bridge.v1");
    assert_eq!(setup["data"]["initialized"], true);
    assert_eq!(setup["data"]["runtimes"].as_array().unwrap().len(), 4);

    let agents_path = project.join("AGENTS.md");
    let agents = fs::read_to_string(&agents_path).unwrap();
    assert!(agents.starts_with("# Existing project rules\n\n"));
    assert!(agents.contains("<!-- weft:runtime-wiring:start -->"));
    assert!(agents.contains("<!-- weft:runtime-wiring:end -->"));
    assert!(project.join("CLAUDE.md").is_file());
    assert!(project.join("GEMINI.md").is_file());
    let project_bridge_path = project.join(".weft/runtime-bridge.json");
    let bridge = fs::read(&project_bridge_path).unwrap();
    assert!(!state.join("runtime-bridge.json").exists());
    assert!(agents.contains(".weft/runtime-bridge.json"));
    assert!(agents.contains("--state-dir <configured-state-dir>"));

    let repeated = invoke_os(
        &state,
        vec![
            OsString::from("setup"),
            OsString::from("--project-dir"),
            project.as_os_str().to_owned(),
            OsString::from("--runtime"),
            OsString::from("codex,claude-code,gemini-cli,paseo"),
        ],
    );
    assert_eq!(repeated.code, 0, "{}", repeated.stderr);
    assert_eq!(fs::read_to_string(&agents_path).unwrap(), agents);
    assert_eq!(fs::read(&project_bridge_path).unwrap(), bridge);

    let doctor = invoke_os(
        &state,
        vec![
            OsString::from("doctor"),
            OsString::from("--project-dir"),
            project.as_os_str().to_owned(),
        ],
    );
    assert_eq!(doctor.code, 0, "{}", doctor.stderr);
    let doctor = parse_one(&doctor.stdout);
    assert_eq!(doctor["command"], "doctor");
    assert_eq!(doctor["data"]["initialized"], true);
    assert!(
        doctor["data"]["problems"]
            .as_array()
            .unwrap()
            .iter()
            .all(|problem| !problem.as_str().unwrap().contains("bridge is missing"))
    );

    fs::remove_file(&project_bridge_path).unwrap();
    let missing_project_bridge = invoke_os(
        &state,
        vec![
            OsString::from("doctor"),
            OsString::from("--project-dir"),
            project.as_os_str().to_owned(),
        ],
    );
    assert_eq!(missing_project_bridge.code, 0);
    let missing_project_bridge = parse_one(&missing_project_bridge.stdout);
    assert_eq!(missing_project_bridge["data"]["healthy"], false);
    assert!(
        missing_project_bridge["data"]["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|problem| problem
                .as_str()
                .unwrap()
                .contains("runtime bridge is missing"))
    );
    fs::write(&project_bridge_path, &bridge).unwrap();

    fs::write(
        &agents_path,
        "# Existing project rules\n\n<!-- weft:runtime-wiring:start -->\nmissing\n<!-- weft:runtime-wiring:end -->\n",
    )
    .unwrap();
    let stale_instruction = invoke_os(
        &state,
        vec![
            OsString::from("doctor"),
            OsString::from("--project-dir"),
            project.as_os_str().to_owned(),
        ],
    );
    assert_eq!(stale_instruction.code, 0);
    assert_eq!(
        parse_one(&stale_instruction.stdout)["data"]["healthy"],
        false
    );
    fs::write(&agents_path, &agents).unwrap();

    fs::write(
        project.join("GEMINI.md"),
        "<!-- weft:runtime-wiring:start -->\n",
    )
    .unwrap();
    let malformed = invoke_os(
        &state,
        vec![
            OsString::from("setup"),
            OsString::from("--project-dir"),
            project.as_os_str().to_owned(),
            OsString::from("--runtime"),
            OsString::from("codex,gemini-cli"),
        ],
    );
    assert_eq!(malformed.code, 7);
    assert_eq!(fs::read_to_string(&agents_path).unwrap(), agents);
    assert_eq!(fs::read(&project_bridge_path).unwrap(), bridge);
}

#[test]
fn setup_preflight_keeps_new_state_absent_when_instruction_markers_are_malformed() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let state = root.path().join("state");
    fs::create_dir(&project).unwrap();
    fs::write(
        project.join("GEMINI.md"),
        "<!-- weft:runtime-wiring:start -->\n",
    )
    .unwrap();

    let result = invoke_os(
        &state,
        vec![
            OsString::from("setup"),
            OsString::from("--project-dir"),
            project.as_os_str().to_owned(),
            OsString::from("--runtime"),
            OsString::from("gemini-cli"),
        ],
    );
    assert_eq!(result.code, 7);
    assert!(!state.exists());
}

#[test]
#[allow(clippy::too_many_lines)]
fn assignments_are_durable_and_terminal_release_requires_confirmation_and_exact_version() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    initialize_change(&state);

    let created = invoke(
        &state,
        [
            "assignment",
            "create",
            "--assignment-id",
            "assignment-1",
            "--change-id",
            "change-1",
            "--subject-kind",
            "agent",
            "--subject-id",
            "agent-2",
            "--role",
            "implementer",
            "--operation-id",
            "op-assign",
            "--actor",
            "agent-1",
            "--at",
            "1100",
        ],
    );
    assert_eq!(created.code, 0, "{}", created.stderr);
    let created = parse_one(&created.stdout);
    assert_eq!(created["data"]["version"], 1);
    assert_eq!(created["data"]["active"], true);

    let listed = invoke(&state, ["assignment", "list", "--change-id", "change-1"]);
    assert_eq!(listed.code, 0);
    assert_eq!(
        parse_one(&listed.stdout)["data"]["assignments"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let release_without_confirmation = invoke(
        &state,
        [
            "assignment",
            "release",
            "--assignment-id",
            "assignment-1",
            "--expected-version",
            "1",
            "--operation-id",
            "op-release",
            "--actor",
            "agent-1",
            "--at",
            "1200",
        ],
    );
    assert_eq!(release_without_confirmation.code, 2);

    let released = invoke(
        &state,
        [
            "assignment",
            "release",
            "--assignment-id",
            "assignment-1",
            "--expected-version",
            "1",
            "--operation-id",
            "op-release",
            "--actor",
            "agent-1",
            "--at",
            "1200",
            "--yes",
        ],
    );
    assert_eq!(released.code, 0, "{}", released.stderr);
    let released = parse_one(&released.stdout);
    assert_eq!(released["data"]["version"], 2);
    assert_eq!(released["data"]["active"], false);

    let stale_release = invoke(
        &state,
        [
            "assignment",
            "release",
            "--assignment-id",
            "assignment-1",
            "--expected-version",
            "1",
            "--operation-id",
            "op-release-stale",
            "--actor",
            "agent-1",
            "--at",
            "1300",
            "--yes",
        ],
    );
    assert_eq!(stale_release.code, 4);
    assert_eq!(
        parse_one(&stale_release.stdout)["error"]["code"],
        "conflict"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn leases_reject_competitors_then_support_expiry_reclaim_renewal_and_release() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    initialize_change(&state);

    let acquired = invoke(
        &state,
        [
            "lease",
            "acquire",
            "--lease-id",
            "lease-1",
            "--change-id",
            "change-1",
            "--operation",
            "integrate",
            "--holder-kind",
            "agent",
            "--holder-id",
            "agent-1",
            "--expected-version",
            "0",
            "--expires-at",
            "2000",
            "--operation-id",
            "op-lease-1",
            "--actor",
            "agent-1",
            "--at",
            "1100",
        ],
    );
    assert_eq!(acquired.code, 0, "{}", acquired.stderr);
    assert_eq!(parse_one(&acquired.stdout)["data"]["version"], 1);

    let held = invoke(
        &state,
        [
            "lease",
            "acquire",
            "--lease-id",
            "lease-held",
            "--change-id",
            "change-1",
            "--operation",
            "integrate",
            "--holder-kind",
            "agent",
            "--holder-id",
            "agent-2",
            "--expected-version",
            "1",
            "--expires-at",
            "2500",
            "--operation-id",
            "op-lease-held",
            "--actor",
            "agent-2",
            "--at",
            "1500",
        ],
    );
    assert_eq!(held.code, 4);
    assert_eq!(parse_one(&held.stdout)["error"]["retryable"], true);

    let reclaimed = invoke(
        &state,
        [
            "lease",
            "acquire",
            "--lease-id",
            "lease-2",
            "--change-id",
            "change-1",
            "--operation",
            "integrate",
            "--holder-kind",
            "agent",
            "--holder-id",
            "agent-2",
            "--expected-version",
            "1",
            "--expires-at",
            "3000",
            "--operation-id",
            "op-lease-2",
            "--actor",
            "agent-2",
            "--at",
            "2000",
        ],
    );
    assert_eq!(reclaimed.code, 0, "{}", reclaimed.stderr);
    let reclaimed = parse_one(&reclaimed.stdout);
    assert_eq!(reclaimed["data"]["version"], 2);
    assert_eq!(reclaimed["data"]["predecessor_lease_id"], "lease-1");

    let renewed = invoke(
        &state,
        [
            "lease",
            "renew",
            "--lease-id",
            "lease-2",
            "--expected-version",
            "2",
            "--expires-at",
            "3500",
            "--operation-id",
            "op-renew",
            "--actor",
            "agent-2",
            "--at",
            "2100",
        ],
    );
    assert_eq!(renewed.code, 0, "{}", renewed.stderr);
    assert_eq!(parse_one(&renewed.stdout)["data"]["version"], 3);

    let released = invoke(
        &state,
        [
            "lease",
            "release",
            "--lease-id",
            "lease-2",
            "--expected-version",
            "3",
            "--operation-id",
            "op-lease-release",
            "--actor",
            "agent-2",
            "--at",
            "2200",
            "--yes",
        ],
    );
    assert_eq!(released.code, 0, "{}", released.stderr);
    assert_eq!(parse_one(&released.stdout)["data"]["version"], 4);

    let shown = invoke(
        &state,
        [
            "lease",
            "show",
            "--change-id",
            "change-1",
            "--operation",
            "integrate",
        ],
    );
    assert_eq!(shown.code, 0);
    assert_eq!(parse_one(&shown.stdout)["data"]["lease"], Value::Null);
}

#[test]
#[allow(clippy::too_many_lines)]
fn exact_dependencies_stacks_and_candidates_report_durable_freshness() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    assert_eq!(invoke(&state, ["init"]).code, 0);
    create_change(&state, "upstream", "op-upstream", 1000);
    create_change(&state, "downstream", "op-downstream", 1001);
    let artifact = store_artifact(&state, "repo-1", "base-1");
    append_revision(
        &state,
        "upstream",
        "upstream-r1",
        "none",
        &artifact,
        "op-upstream-r1",
        1100,
    );
    append_revision(
        &state,
        "downstream",
        "downstream-r1",
        "none",
        &artifact,
        "op-downstream-r1",
        1101,
    );

    let dependency = invoke(
        &state,
        [
            "dependency",
            "create",
            "--dependency-id",
            "dependency-1",
            "--downstream-change-id",
            "downstream",
            "--upstream-change-id",
            "upstream",
            "--downstream-revision-id",
            "downstream-r1",
            "--upstream-revision-id",
            "upstream-r1",
            "--operation-id",
            "op-dependency",
            "--actor",
            "agent-1",
            "--at",
            "1200",
        ],
    );
    assert_eq!(dependency.code, 0, "{}", dependency.stderr);

    let stack = invoke(
        &state,
        [
            "stack",
            "create",
            "--stack-id",
            "stack-1",
            "--policy",
            "predecessor_dependencies",
            "--changes",
            "upstream,downstream",
            "--operation-id",
            "op-stack",
            "--actor",
            "agent-1",
            "--at",
            "1201",
        ],
    );
    assert_eq!(stack.code, 0, "{}", stack.stderr);
    assert_eq!(parse_one(&stack.stdout)["data"]["version"], 1);

    let candidate = invoke(
        &state,
        [
            "candidate",
            "create",
            "--candidate-id",
            "candidate-1",
            "--repository-id",
            "repo-1",
            "--target-object",
            "base-1",
            "--stack-id",
            "stack-1",
            "--expected-stack-version",
            "1",
            "--operation-id",
            "op-candidate",
            "--actor",
            "agent-1",
            "--at",
            "1300",
        ],
    );
    assert_eq!(candidate.code, 0, "{}", candidate.stderr);
    let candidate = parse_one(&candidate.stdout);
    assert_eq!(candidate["data"]["inputs"][0]["change_id"], "upstream");
    assert_eq!(candidate["data"]["inputs"][1]["change_id"], "downstream");
    assert_eq!(
        candidate["data"]["requirements"].as_array().unwrap().len(),
        2
    );

    append_revision(
        &state,
        "downstream",
        "downstream-r2",
        "downstream-r1",
        &artifact,
        "op-downstream-r2",
        1400,
    );
    let dependencies = invoke(&state, ["dependency", "list", "--change-id", "downstream"]);
    assert_eq!(dependencies.code, 0);
    assert_eq!(
        parse_one(&dependencies.stdout)["data"]["dependencies"][0]["freshness"],
        "downstream_advanced"
    );
    let freshness = invoke(
        &state,
        ["candidate", "freshness", "--candidate-id", "candidate-1"],
    );
    assert_eq!(freshness.code, 0);
    let freshness = parse_one(&freshness.stdout);
    assert_eq!(freshness["data"]["current"], false);
    assert_eq!(freshness["data"]["advanced_inputs"][0], "downstream");
}

#[test]
#[allow(clippy::too_many_lines)]
fn materialization_review_and_validation_stay_pinned_to_exact_revision() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    assert_eq!(invoke(&state, ["init"]).code, 0);
    create_change(&state, "change-1", "op-create", 1000);
    let artifact = store_artifact(&state, "repo-1", "base-1");
    append_revision(
        &state,
        "change-1",
        "revision-1",
        "none",
        &artifact,
        "op-revision-1",
        1100,
    );

    let materialization = invoke(
        &state,
        [
            "materialization",
            "create",
            "--materialization-id",
            "materialization-1",
            "--change-id",
            "change-1",
            "--revision-id",
            "revision-1",
            "--workspace-id",
            "workspace-1",
            "--provider-id",
            "native-git",
            "--provider-ref",
            "refs/weft/change-1",
            "--provider-evidence",
            "observed:clean",
            "--operation-id",
            "op-materialize",
            "--actor",
            "agent-1",
            "--at",
            "1200",
        ],
    );
    assert_eq!(materialization.code, 0, "{}", materialization.stderr);
    assert_eq!(parse_one(&materialization.stdout)["data"]["state"], "clean");

    let dirty = invoke(
        &state,
        [
            "materialization",
            "transition",
            "--materialization-id",
            "materialization-1",
            "--expected-version",
            "1",
            "--state",
            "dirty",
            "--provider-ref",
            "refs/weft/change-1",
            "--provider-evidence",
            "observed:dirty",
            "--operation-id",
            "op-dirty",
            "--actor",
            "agent-1",
            "--at",
            "1250",
        ],
    );
    assert_eq!(dirty.code, 0, "{}", dirty.stderr);
    assert_eq!(parse_one(&dirty.stdout)["data"]["version"], 2);

    let unconfirmed_release = invoke(
        &state,
        [
            "materialization",
            "transition",
            "--materialization-id",
            "materialization-1",
            "--expected-version",
            "2",
            "--state",
            "released",
            "--provider-ref",
            "refs/weft/change-1",
            "--provider-evidence",
            "observed:released",
            "--operation-id",
            "op-release",
            "--actor",
            "agent-1",
            "--at",
            "1300",
        ],
    );
    assert_eq!(unconfirmed_release.code, 2);

    let review = invoke(
        &state,
        [
            "review",
            "request",
            "--review-request-id",
            "review-1",
            "--reviewers",
            "reviewer-1",
            "--change-id",
            "change-1",
            "--revision-id",
            "revision-1",
            "--operation-id",
            "op-review",
            "--actor",
            "agent-1",
            "--at",
            "1300",
        ],
    );
    assert_eq!(review.code, 0, "{}", review.stderr);
    assert_eq!(
        parse_one(&review.stdout)["data"]["freshness"]["current"],
        true
    );

    let submission = invoke(
        &state,
        [
            "review",
            "submit",
            "--review-submission-id",
            "submission-1",
            "--review-request-id",
            "review-1",
            "--outcome",
            "approved",
            "--comments",
            "exact revision reviewed",
            "--operation-id",
            "op-submission",
            "--actor",
            "reviewer-1",
            "--at",
            "1400",
        ],
    );
    assert_eq!(submission.code, 0, "{}", submission.stderr);
    assert_eq!(parse_one(&submission.stdout)["data"]["outcome"], "approved");

    let validation = invoke(
        &state,
        [
            "validation",
            "record",
            "--validation-result-id",
            "validation-1",
            "--validation-type",
            "test",
            "--environment",
            "local",
            "--outcome",
            "passed",
            "--execution-id",
            "execution-1",
            "--scope",
            "exact_target",
            "--change-id",
            "change-1",
            "--revision-id",
            "revision-1",
            "--operation-id",
            "op-validation",
            "--actor",
            "validator-1",
            "--at",
            "1401",
        ],
    );
    assert_eq!(validation.code, 0, "{}", validation.stderr);
    assert_eq!(
        parse_one(&validation.stdout)["data"]["freshness"]["current"],
        true
    );

    append_revision(
        &state,
        "change-1",
        "revision-2",
        "revision-1",
        &artifact,
        "op-revision-2",
        1500,
    );
    let review = invoke(
        &state,
        ["review", "show", "--review-request-id", "review-1"],
    );
    assert_eq!(review.code, 0);
    assert_eq!(
        parse_one(&review.stdout)["data"]["freshness"]["status"],
        "revision_advanced"
    );
    let validation = invoke(
        &state,
        [
            "validation",
            "show",
            "--validation-result-id",
            "validation-1",
        ],
    );
    assert_eq!(validation.code, 0);
    assert_eq!(
        parse_one(&validation.stdout)["data"]["freshness"]["current"],
        false
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn integration_uncertainty_requires_durable_reconciliation_before_success() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    assert_eq!(invoke(&state, ["init"]).code, 0);
    create_change(&state, "change-1", "op-create", 1000);
    let artifact = store_artifact(&state, "repo-1", "base-1");
    append_revision(
        &state,
        "change-1",
        "revision-1",
        "none",
        &artifact,
        "op-revision",
        1100,
    );
    let candidate = invoke(
        &state,
        [
            "candidate",
            "create",
            "--candidate-id",
            "candidate-1",
            "--repository-id",
            "repo-1",
            "--target-object",
            "base-1",
            "--changes",
            "change-1",
            "--operation-id",
            "op-candidate",
            "--actor",
            "agent-1",
            "--at",
            "1200",
        ],
    );
    assert_eq!(candidate.code, 0, "{}", candidate.stderr);

    let planned = invoke(
        &state,
        [
            "integration",
            "plan",
            "--integration-id",
            "integration-1",
            "--candidate-id",
            "candidate-1",
            "--target-ref",
            "refs/heads/main",
            "--expected-target",
            "base-1",
            "--provider-id",
            "test-provider",
            "--strategy",
            "squash",
            "--effect-operation-id",
            "effect-1",
            "--policy-evidence",
            "policy:allowed",
            "--capability-evidence",
            "native-git:guarded-ref-update",
            "--observed-target",
            "base-1",
            "--observation-evidence",
            "target:planned",
            "--operation-id",
            "op-plan",
            "--actor",
            "agent-1",
            "--at",
            "1300",
        ],
    );
    assert_eq!(planned.code, 0, "{}", planned.stderr);
    assert_eq!(parse_one(&planned.stdout)["data"]["state"], "planned");

    let started = invoke(
        &state,
        [
            "integration",
            "start",
            "--integration-id",
            "integration-1",
            "--expected-version",
            "1",
            "--lease-id",
            "execution-lease-1",
            "--holder-kind",
            "agent",
            "--holder-id",
            "agent-1",
            "--expires-at",
            "1600",
            "--target-ref",
            "refs/heads/main",
            "--observed-target",
            "base-1",
            "--observation-evidence",
            "target:start-cas",
            "--operation-id",
            "op-start",
            "--actor",
            "agent-1",
            "--at",
            "1400",
        ],
    );
    assert_eq!(started.code, 0, "{}", started.stderr);
    assert_eq!(parse_one(&started.stdout)["data"]["state"], "running");

    let uncertain = invoke(
        &state,
        [
            "integration",
            "uncertain",
            "--integration-id",
            "integration-1",
            "--expected-version",
            "2",
            "--reconciliation-id",
            "reconciliation-1",
            "--lease-id",
            "execution-lease-1",
            "--holder-kind",
            "agent",
            "--holder-id",
            "agent-1",
            "--target-ref",
            "refs/heads/main",
            "--observed-target",
            "provider-unknown",
            "--observation-evidence",
            "provider:timeout",
            "--operation-id",
            "op-uncertain",
            "--actor",
            "agent-1",
            "--at",
            "1500",
        ],
    );
    assert_eq!(uncertain.code, 0, "{}", uncertain.stderr);
    assert_eq!(parse_one(&uncertain.stdout)["data"]["state"], "reconciling");

    let reconciled = invoke(
        &state,
        [
            "integration",
            "reconcile",
            "--integration-id",
            "integration-1",
            "--expected-version",
            "3",
            "--reconciliation-id",
            "reconciliation-2",
            "--outcome",
            "result_verified",
            "--target-ref",
            "refs/heads/main",
            "--observed-target",
            "result-1",
            "--observation-evidence",
            "provider:result-verified",
            "--operation-id",
            "op-reconcile",
            "--actor",
            "agent-1",
            "--at",
            "1501",
        ],
    );
    assert_eq!(reconciled.code, 0, "{}", reconciled.stderr);

    let succeeded = invoke(
        &state,
        [
            "integration",
            "succeed",
            "--integration-id",
            "integration-1",
            "--expected-version",
            "4",
            "--receipt-id",
            "receipt-1",
            "--target-ref",
            "refs/heads/main",
            "--observed-target",
            "result-1",
            "--observation-evidence",
            "provider:result-verified",
            "--operation-id",
            "op-succeed",
            "--actor",
            "agent-1",
            "--at",
            "1502",
        ],
    );
    assert_eq!(succeeded.code, 0, "{}", succeeded.stderr);
    let succeeded = parse_one(&succeeded.stdout);
    assert_eq!(succeeded["data"]["attempt"]["state"], "succeeded");
    assert_eq!(
        succeeded["data"]["receipt"]["effect_operation_id"],
        "effect-1"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn native_git_discovery_inspection_and_capture_create_exact_durable_revision() {
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join("repository");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "--initial-branch=main"]);
    git(&repository, &["config", "user.name", "Weft Test"]);
    git(
        &repository,
        &["config", "user.email", "weft@example.invalid"],
    );
    git(&repository, &["config", "commit.gpgSign", "false"]);
    fs::write(repository.join("file.txt"), "base\n").unwrap();
    git(&repository, &["add", "file.txt"]);
    git(&repository, &["commit", "-m", "base"]);
    let base = git(&repository, &["rev-parse", "HEAD"]);
    git(&repository, &["branch", "integration-target", &base]);
    fs::write(repository.join("file.txt"), "changed\n").unwrap();
    git(&repository, &["commit", "-am", "change"]);
    let revision = git(&repository, &["rev-parse", "HEAD"]);

    let state = root.path().join("state");
    assert_eq!(invoke(&state, ["init"]).code, 0);
    create_change(&state, "change-1", "op-create", 1000);
    let repository_path = repository.to_string_lossy();
    let discovery = invoke(
        &state,
        ["native-git", "discover", "--repository", &repository_path],
    );
    assert_eq!(discovery.code, 0, "{}", discovery.stderr);
    let discovery = parse_one(&discovery.stdout);
    assert_eq!(discovery["data"]["object_format"], "sha1");
    assert_eq!(discovery["data"]["capabilities"][0]["supported"], true);

    let inspected = invoke(
        &state,
        [
            "native-git",
            "inspect",
            "--repository",
            &repository_path,
            "--revision",
            &revision,
        ],
    );
    assert_eq!(inspected.code, 0, "{}", inspected.stderr);
    assert_eq!(parse_one(&inspected.stdout)["data"]["commit_id"], revision);

    let captured = invoke(
        &state,
        [
            "native-git",
            "capture",
            "--repository",
            &repository_path,
            "--repository-id",
            "repo-1",
            "--base-revision",
            &base,
            "--provider-revision",
            &revision,
            "--change-id",
            "change-1",
            "--revision-id",
            "revision-1",
            "--expected-head",
            "none",
            "--operation-id",
            "op-capture",
            "--actor",
            "agent-1",
            "--at",
            "1100",
        ],
    );
    assert_eq!(captured.code, 0, "{}", captured.stderr);
    let captured = parse_one(&captured.stdout);
    assert_eq!(captured["data"]["provider"]["commit_id"], revision);
    assert_eq!(captured["data"]["change"]["head_revision_id"], "revision-1");
    assert!(
        captured["data"]["change"]["revisions"][0]["artifact"]["manifest_digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );

    let worktree = root.path().join("materialized");
    let worktree_path = worktree.to_string_lossy();
    let materialized = invoke(
        &state,
        [
            "native-git",
            "materialize",
            "--repository",
            &repository_path,
            "--provider-revision",
            &revision,
            "--change-id",
            "change-1",
            "--revision-id",
            "revision-1",
            "--destination",
            &worktree_path,
            "--materialization-id",
            "materialization-1",
            "--workspace-id",
            "workspace-1",
            "--operation-id",
            "op-materialize",
            "--actor",
            "agent-1",
            "--at",
            "1200",
        ],
    );
    assert_eq!(materialized.code, 0, "{}", materialized.stderr);
    assert_eq!(
        parse_one(&materialized.stdout)["data"]["materialization"]["state"],
        "clean"
    );

    fs::write(worktree.join("file.txt"), "locally changed\n").unwrap();
    let observed = invoke(
        &state,
        [
            "native-git",
            "observe-materialization",
            "--repository",
            &repository_path,
            "--worktree",
            &worktree_path,
            "--provider-revision",
            &revision,
            "--materialization-id",
            "materialization-1",
            "--expected-version",
            "1",
            "--operation-id",
            "op-observe",
            "--actor",
            "agent-1",
            "--at",
            "1300",
        ],
    );
    assert_eq!(observed.code, 0, "{}", observed.stderr);
    assert_eq!(parse_one(&observed.stdout)["data"]["state"], "dirty");

    git(&worktree, &["reset", "--hard", &base]);
    let released = invoke(
        &state,
        [
            "native-git",
            "release-materialization",
            "--repository",
            &repository_path,
            "--worktree",
            &worktree_path,
            "--materialization-id",
            "materialization-1",
            "--expected-version",
            "2",
            "--operation-id",
            "op-release",
            "--actor",
            "agent-1",
            "--at",
            "1400",
            "--yes",
        ],
    );
    assert_eq!(released.code, 0, "{}", released.stderr);
    assert_eq!(parse_one(&released.stdout)["data"]["state"], "released");
    assert!(!worktree.exists());

    let candidate = invoke(
        &state,
        [
            "candidate",
            "create",
            "--candidate-id",
            "candidate-1",
            "--repository-id",
            "repo-1",
            "--target-object",
            &base,
            "--changes",
            "change-1",
            "--operation-id",
            "op-candidate",
            "--actor",
            "agent-1",
            "--at",
            "1500",
        ],
    );
    assert_eq!(candidate.code, 0, "{}", candidate.stderr);
    let planning_scratch = root.path().join("integration-planning-scratch");
    let planning_scratch_path = planning_scratch.to_string_lossy();
    let planned = invoke(
        &state,
        [
            "integration",
            "plan",
            "--integration-id",
            "integration-1",
            "--candidate-id",
            "candidate-1",
            "--target-ref",
            "refs/heads/integration-target",
            "--expected-target",
            &base,
            "--provider-id",
            "native-git",
            "--strategy",
            "squash",
            "--effect-operation-id",
            "effect-1",
            "--policy-evidence",
            "policy:allowed",
            "--repository",
            &repository_path,
            "--provider-revisions",
            &revision,
            "--scratch",
            &planning_scratch_path,
            "--observed-target",
            &base,
            "--observation-evidence",
            "target:planned",
            "--operation-id",
            "op-plan",
            "--actor",
            "agent-1",
            "--at",
            "1600",
        ],
    );
    assert_eq!(planned.code, 0, "{}", planned.stderr);
    let other_repository = root.path().join("other-repository");
    git(
        &repository,
        &["clone", ".", other_repository.to_string_lossy().as_ref()],
    );
    let other_repository_path = other_repository.to_string_lossy();
    let other_scratch = root.path().join("other-integration-scratch");
    let other_scratch_path = other_scratch.to_string_lossy();
    let rejected_other_clone = invoke(
        &state,
        [
            "native-git",
            "execute-integration",
            "--repository",
            &other_repository_path,
            "--integration-id",
            "integration-1",
            "--expected-version",
            "1",
            "--scratch",
            &other_scratch_path,
            "--lease-id",
            "wrong-clone-lease",
            "--holder-kind",
            "agent",
            "--holder-id",
            "agent-1",
            "--expires-at",
            "1800",
            "--receipt-id",
            "wrong-clone-receipt",
            "--reconciliation-id",
            "wrong-clone-reconciliation",
            "--start-operation-id",
            "op-wrong-clone-start",
            "--finish-operation-id",
            "op-wrong-clone-finish",
            "--actor",
            "agent-1",
            "--at",
            "1700",
        ],
    );
    assert_eq!(
        rejected_other_clone.code, 7,
        "{}",
        rejected_other_clone.stderr
    );
    assert_eq!(git(&repository, &["rev-parse", "integration-target"]), base);
    let scratch = root.path().join("integration-scratch");
    let scratch_path = scratch.to_string_lossy();
    let executed = invoke(
        &state,
        [
            "native-git",
            "execute-integration",
            "--repository",
            &repository_path,
            "--integration-id",
            "integration-1",
            "--expected-version",
            "1",
            "--scratch",
            &scratch_path,
            "--lease-id",
            "execution-lease-1",
            "--holder-kind",
            "agent",
            "--holder-id",
            "agent-1",
            "--expires-at",
            "1800",
            "--receipt-id",
            "receipt-1",
            "--reconciliation-id",
            "reconciliation-on-error",
            "--start-operation-id",
            "op-execute-start",
            "--finish-operation-id",
            "op-execute-finish",
            "--actor",
            "agent-1",
            "--at",
            "1700",
        ],
    );
    assert_eq!(executed.code, 0, "{}", executed.stderr);
    let executed = parse_one(&executed.stdout);
    assert_eq!(executed["data"]["attempt"]["state"], "succeeded");
    let landed = git(&repository, &["rev-parse", "integration-target"]);
    assert_eq!(executed["data"]["receipt"]["result_revision"], landed);
}

fn invoke<const N: usize>(state: &Path, arguments: [&str; N]) -> ResultOutput {
    let mut values = vec![
        OsString::from("--format"),
        OsString::from("json"),
        OsString::from("--state-dir"),
        state.as_os_str().to_owned(),
    ];
    values.extend(arguments.into_iter().map(OsString::from));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(values, &mut stdout, &mut stderr);
    ResultOutput {
        code,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
    }
}

fn invoke_os(state: &Path, mut arguments: Vec<OsString>) -> ResultOutput {
    let mut values = vec![
        OsString::from("--format"),
        OsString::from("json"),
        OsString::from("--state-dir"),
        state.as_os_str().to_owned(),
    ];
    values.append(&mut arguments);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(values, &mut stdout, &mut stderr);
    ResultOutput {
        code,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
    }
}

fn parse_one(value: &str) -> Value {
    assert_eq!(
        value.lines().count(),
        1,
        "expected one JSON line: {value:?}"
    );
    serde_json::from_str(value).unwrap()
}

fn initialize_change(state: &Path) {
    assert_eq!(invoke(state, ["init"]).code, 0);
    let created = invoke(
        state,
        [
            "change",
            "create",
            "--change-id",
            "change-1",
            "--operation-id",
            "op-create",
            "--actor",
            "agent-1",
            "--at",
            "1000",
        ],
    );
    assert_eq!(created.code, 0, "{}", created.stderr);
}

fn create_change(state: &Path, change_id: &str, operation_id: &str, at: i64) {
    let at = at.to_string();
    let created = invoke(
        state,
        [
            "change",
            "create",
            "--change-id",
            change_id,
            "--operation-id",
            operation_id,
            "--actor",
            "agent-1",
            "--at",
            &at,
        ],
    );
    assert_eq!(created.code, 0, "{}", created.stderr);
}

#[allow(clippy::too_many_arguments)]
fn append_revision(
    state: &Path,
    change_id: &str,
    revision_id: &str,
    expected_head: &str,
    artifact: &str,
    operation_id: &str,
    at: i64,
) {
    let at = at.to_string();
    let appended = invoke(
        state,
        [
            "revision",
            "append",
            "--change-id",
            change_id,
            "--revision-id",
            revision_id,
            "--expected-head",
            expected_head,
            "--repository-id",
            "repo-1",
            "--base-object",
            "base-1",
            "--artifact-digest",
            artifact,
            "--operation-id",
            operation_id,
            "--actor",
            "agent-1",
            "--at",
            &at,
        ],
    );
    assert_eq!(appended.code, 0, "{}", appended.stderr);
}

fn store_artifact(state: &Path, repository_id: &str, base_object: &str) -> String {
    let store = ArtifactStore::open(state.join("artifacts")).unwrap();
    let blob = store.store_blob(b"content\n").unwrap();
    let manifest = CanonicalTreeDelta::new(
        BaseState::new(RepositoryId::new(repository_id).unwrap(), base_object).unwrap(),
        TreeDelta::new(vec![PathOperation::Upsert {
            path: "file.txt".to_owned(),
            mode: FileMode::Regular,
            blob_digest: blob.as_str().to_owned(),
        }])
        .unwrap(),
    );
    store
        .store_manifest(&manifest)
        .unwrap()
        .manifest_digest()
        .to_owned()
}

fn git(repository: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
