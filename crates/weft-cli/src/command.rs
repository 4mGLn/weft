use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use weft_artifact::ArtifactStore;
use weft_domain::{
    ActorId, ArtifactRef, Assignment, AssignmentId, AssignmentRole, BaseState, CandidateId, Change,
    ChangeId, CompositionCandidate, CoordinationVersion, Dependency, DependencyFreshness,
    DependencyId, DependencyPins, EffectOperationId, ExactTarget, ExecutionLease, ExecutionLeaseId,
    GatePolicyEvidence, IntegrationAttempt, IntegrationBinding, IntegrationCapabilityEvidence,
    IntegrationConflictId, IntegrationEvidence, IntegrationGate, IntegrationId, IntegrationIntent,
    IntegrationMethod, IntegrationReceiptId, IntegrationState, IntegrationStrategy,
    IntegrationTarget, IntegrationVersion, Lease, LeaseId, LeaseOperation, LeaseScope,
    Materialization, MaterializationId, MaterializationPlacement, MaterializationState,
    MaterializationVersion, NewRevision, ProviderEvidence, ProviderId, ProviderObservation,
    ProviderRef, ReconciliationId, ReconciliationOutcome, Relationship, RelationshipEndpoints,
    RelationshipId, RelationshipKind, RelationshipVersion, RepositoryId, ResolvedRequirementSource,
    ReviewOutcome, ReviewRequest, ReviewRequestId, ReviewSubmission, ReviewSubmissionId,
    RevisionId, Stack, StackDefinition, StackId, StackPolicy, StackVersion, Subject, SubjectId,
    SubjectKind, TargetObservation, TargetRef, TargetRevision, UnixMillis, ValidationEnvironment,
    ValidationExecutionId, ValidationObservation, ValidationOutcome, ValidationResult,
    ValidationResultId, ValidationScope, ValidationType, WorkspaceId,
};
use weft_provider_git::{
    CandidateComposition, CapturedRevision, GitCapability, IntegrationPlan, NativeGit,
    ReconciliationResult,
};
use weft_provider_gitbutler::{GitButler, GitButlerCapability};
use weft_storage_sqlite::{
    CandidateFreshness, CandidateSelection, ConflictReport, ExactTargetFreshness, LeaseRenewal,
    MutationContext, ReconciliationRecord, ReconciliationStart, SqliteStore, SuccessVerification,
};

use crate::contract::Success;
use crate::error::CliError;
use crate::parser::{Command, Failure, Invocation, Options};
use crate::wiring;

const HELP: &str = "weft — exact local Change coordination\n\n\
Usage:\n  weft [--format human|json] [--state-dir PATH] [-v|--verbose] <command>\n  weft [-V|--version]\n\n\
Commands:\n  init\n  setup [--project-dir PATH] [--runtime auto|all|NAME,...]\n  doctor [--project-dir PATH]\n  change create|show|history ...\n  revision append ...\n\
  assignment create|list|release ...\n  lease acquire|show|renew|release ...\n\
  relationship create|list|remove ...\n  dependency create|list|repin|remove ...\n\
  stack create|show|replace ...\n  candidate create|show|freshness ...\n\
  materialization create|show|list|transition ...\n\
  review request|show|submit|submissions ...\n  validation record|show ...\n\
  integration plan|show|start|renew|uncertain|reconcile|conflict|succeed|finish|abort|supersede ...\n\
  native-git discover|inspect|capture|materialize|observe-materialization|release-materialization ...\n\
  native-git execute-integration|reconcile-integration ...\n  gitbutler discover ...\n";

struct State {
    artifacts: ArtifactStore,
    store: SqliteStore,
}

pub(crate) fn execute(invocation: Invocation) -> Result<Success, Failure> {
    let command_name = invocation.command().name();
    let format = invocation.format();
    execute_inner(invocation).map_err(|error| Failure {
        format,
        command: command_name,
        error,
    })
}

fn execute_inner(invocation: Invocation) -> Result<Success, CliError> {
    let (_format, state_dir, command) = invocation.into_parts();
    match command {
        Command::Help => Ok(help()),
        Command::Version => Ok(Success {
            command: "version",
            data: json!({"version": env!("CARGO_PKG_VERSION"), "schema": "weft.cli.v1"}),
            human: format!("weft {}", env!("CARGO_PKG_VERSION")),
        }),
        Command::Init => init(&state_dir),
        Command::Setup(options) => setup(&state_dir, options),
        Command::Doctor(options) => doctor(&state_dir, options),
        Command::ChangeCreate(options) => change_create(&state_dir, options),
        Command::ChangeShow(options) => change_show(&state_dir, options),
        Command::ChangeHistory(options) => change_history(&state_dir, options),
        Command::RevisionAppend(options) => revision_append(&state_dir, options),
        Command::AssignmentCreate(options) => assignment_create(&state_dir, options),
        Command::AssignmentList(options) => assignment_list(&state_dir, options),
        Command::AssignmentRelease(options) => assignment_release(&state_dir, options),
        Command::LeaseAcquire(options) => lease_acquire(&state_dir, options),
        Command::LeaseShow(options) => lease_show(&state_dir, options),
        Command::LeaseRenew(options) => lease_renew(&state_dir, options),
        Command::LeaseRelease(options) => lease_release(&state_dir, options),
        Command::RelationshipCreate(options) => relationship_create(&state_dir, options),
        Command::RelationshipList(options) => relationship_list(&state_dir, options),
        Command::RelationshipRemove(options) => relationship_remove(&state_dir, options),
        Command::DependencyCreate(options) => dependency_create(&state_dir, options),
        Command::DependencyList(options) => dependency_list(&state_dir, options),
        Command::DependencyRepin(options) => dependency_repin(&state_dir, options),
        Command::DependencyRemove(options) => dependency_remove(&state_dir, options),
        Command::StackCreate(options) => stack_create(&state_dir, options),
        Command::StackShow(options) => stack_show(&state_dir, options),
        Command::StackReplace(options) => stack_replace(&state_dir, options),
        Command::CandidateCreate(options) => candidate_create(&state_dir, options),
        Command::CandidateShow(options) => candidate_show(&state_dir, options),
        Command::CandidateFreshness(options) => candidate_freshness(&state_dir, options),
        Command::MaterializationCreate(options) => materialization_create(&state_dir, options),
        Command::MaterializationShow(options) => materialization_show(&state_dir, options),
        Command::MaterializationList(options) => materialization_list(&state_dir, options),
        Command::MaterializationTransition(options) => {
            materialization_transition(&state_dir, options)
        }
        Command::ReviewRequest(options) => review_request(&state_dir, options),
        Command::ReviewShow(options) => review_show(&state_dir, options),
        Command::ReviewSubmit(options) => review_submit(&state_dir, options),
        Command::ReviewSubmissions(options) => review_submissions(&state_dir, options),
        Command::ValidationRecord(options) => validation_record(&state_dir, options),
        Command::ValidationShow(options) => validation_show(&state_dir, options),
        Command::IntegrationPlan(options) => integration_plan(&state_dir, options),
        Command::IntegrationShow(options) => integration_show(&state_dir, options),
        Command::IntegrationStart(options) => integration_start(&state_dir, options),
        Command::IntegrationRenew(options) => integration_renew(&state_dir, options),
        Command::IntegrationUncertain(options) => integration_uncertain(&state_dir, options),
        Command::IntegrationReconcile(options) => integration_reconcile(&state_dir, options),
        Command::IntegrationConflict(options) => integration_conflict(&state_dir, options),
        Command::IntegrationSucceed(options) => integration_succeed(&state_dir, options),
        Command::IntegrationFinish(options) => integration_finish(&state_dir, options),
        Command::IntegrationAbort(options) => integration_abort(&state_dir, options),
        Command::IntegrationSupersede(options) => integration_supersede(&state_dir, options),
        Command::NativeGitDiscover(options) => native_git_discover(options),
        Command::NativeGitInspect(options) => native_git_inspect(options),
        Command::NativeGitCapture(options) => native_git_capture(&state_dir, options),
        Command::NativeGitMaterialize(options) => native_git_materialize(&state_dir, options),
        Command::NativeGitObserveMaterialization(options) => {
            native_git_observe_materialization(&state_dir, options)
        }
        Command::NativeGitReleaseMaterialization(options) => {
            native_git_release_materialization(&state_dir, options)
        }
        Command::NativeGitExecuteIntegration(options) => {
            native_git_execute_integration(&state_dir, options)
        }
        Command::NativeGitReconcileIntegration(options) => {
            native_git_reconcile_integration(&state_dir, options)
        }
        Command::GitButlerDiscover(options) => gitbutler_discover(options),
    }
}

fn help() -> Success {
    Success {
        command: "help",
        data: json!({
            "usage": "weft [--format human|json] [--state-dir PATH] [-v|--verbose] <command>",
            "commands": [
                "init", "setup", "doctor", "change.create", "change.show", "change.history", "revision.append",
                "assignment.create", "assignment.list", "assignment.release",
                "lease.acquire", "lease.show", "lease.renew", "lease.release",
                "relationship.create", "relationship.list", "relationship.remove",
                "dependency.create", "dependency.list", "dependency.repin", "dependency.remove",
                "stack.create", "stack.show", "stack.replace",
                "candidate.create", "candidate.show", "candidate.freshness",
                "materialization.create", "materialization.show", "materialization.list",
                "materialization.transition", "review.request", "review.show", "review.submit",
                "review.submissions", "validation.record", "validation.show",
                "integration.plan", "integration.show", "integration.start", "integration.renew",
                "integration.uncertain", "integration.reconcile", "integration.conflict",
                "integration.succeed", "integration.finish", "integration.abort",
                "integration.supersede", "native-git.discover", "native-git.inspect",
                "native-git.capture", "native-git.materialize",
                "native-git.observe-materialization", "native-git.release-materialization",
                "native-git.execute-integration", "native-git.reconcile-integration",
                "gitbutler.discover"
            ]
        }),
        human: HELP.to_owned(),
    }
}

fn init(state_dir: &Path) -> Result<Success, CliError> {
    if state_dir.exists() && !state_dir.is_dir() {
        return Err(CliError::usage("--state-dir exists and is not a directory"));
    }
    fs::create_dir_all(state_dir)
        .map_err(|_| CliError::local("failed to create the Weft state directory"))?;
    let artifact_path = state_dir.join("artifacts");
    ArtifactStore::open(&artifact_path)
        .map_err(|_| CliError::local("failed to initialize canonical artifact storage"))?;
    let database_path = state_dir.join("metadata.sqlite3");
    SqliteStore::open(&database_path)?;
    Ok(Success {
        command: "init",
        data: json!({
            "state_dir": display_path(state_dir),
            "database": display_path(&database_path),
            "artifacts": display_path(&artifact_path),
            "metadata_schema_version": SqliteStore::schema_version()
        }),
        human: format!("initialized Weft state at {}", state_dir.display()),
    })
}

fn setup(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let project_dir = project_dir(&mut options)?;
    let runtime = options
        .optional("runtime")
        .unwrap_or_else(|| "auto".to_owned());
    options.finish()?;
    wiring::preflight(&project_dir, &runtime)?;
    init(state_dir)?;
    let data = wiring::setup(state_dir, &project_dir, &runtime)?;
    let configured = data["runtimes"].as_array().map_or(0, Vec::len);
    Ok(Success {
        command: "setup",
        data,
        human: format!("initialized Weft and configured {configured} runtime bridge entries"),
    })
}

fn doctor(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let project_dir = project_dir(&mut options)?;
    options.finish()?;
    let data = wiring::doctor(state_dir, &project_dir)?;
    let healthy = data["healthy"].as_bool().unwrap_or(false);
    Ok(Success {
        command: "doctor",
        data,
        human: if healthy {
            "Weft runtime wiring is healthy".to_owned()
        } else {
            "Weft runtime wiring needs attention; use --format json for details".to_owned()
        },
    })
}

fn project_dir(options: &mut Options) -> Result<PathBuf, CliError> {
    match options.optional("project-dir") {
        Some(path) => Ok(PathBuf::from(path)),
        None => std::env::current_dir()
            .map_err(|_| CliError::local("failed to determine the current project directory")),
    }
}

fn change_create(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    state.store.create_change(&change_id, &context)?;
    let change = state.store.load_change(&state.artifacts, &change_id)?;
    Ok(Success {
        command: "change.create",
        data: change_view(&change),
        human: format!("created Change {}", change_id.as_str()),
    })
}

fn change_show(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let change = state.store.load_change(&state.artifacts, &change_id)?;
    Ok(Success {
        command: "change.show",
        data: change_view(&change),
        human: format_change(&change),
    })
}

fn change_history(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    state.store.load_change(&state.artifacts, &change_id)?;
    let events = state.store.audit_events(&change_id)?;
    let values = events
        .iter()
        .map(|event| {
            json!({
                "event_id": event.event_id,
                "event_kind": event.event_kind,
                "change_id": event.change_id.as_str(),
                "revision_id": event.revision_id.as_ref().map(RevisionId::as_str),
                "expected_head_revision_id": event.expected_head_revision_id.as_ref().map(RevisionId::as_str),
                "resulting_head_revision_id": event.resulting_head_revision_id.as_ref().map(RevisionId::as_str),
                "operation_id": event.operation_id,
                "actor": event.actor.as_str(),
                "occurred_at_unix_ms": event.occurred_at.value()
            })
        })
        .collect::<Vec<_>>();
    Ok(Success {
        command: "change.history",
        data: json!({"change_id": change_id.as_str(), "events": values}),
        human: format!("{} audit event(s) for {}", events.len(), change_id.as_str()),
    })
}

fn revision_append(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    let revision_id =
        RevisionId::new(options.required("revision-id")?).map_err(CliError::from_input)?;
    let expected = options.required("expected-head")?;
    let expected_head = if expected == "none" {
        None
    } else {
        Some(RevisionId::new(expected).map_err(CliError::from_input)?)
    };
    let repository_id =
        RepositoryId::new(options.required("repository-id")?).map_err(CliError::from_input)?;
    let base = BaseState::new(repository_id, options.required("base-object")?)
        .map_err(CliError::from_input)?;
    let artifact = ArtifactRef::tree_delta_v1(options.required("artifact-digest")?)
        .map_err(CliError::from_input)?;
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    options.finish()?;
    let revision = NewRevision::new(revision_id, base, artifact, at, actor);
    let mut state = open_state(state_dir)?;
    state.store.append_revision(
        &state.artifacts,
        &change_id,
        expected_head.as_ref(),
        &revision,
        &context,
    )?;
    let change = state.store.load_change(&state.artifacts, &change_id)?;
    Ok(Success {
        command: "revision.append",
        data: change_view(&change),
        human: format!(
            "appended Revision {} to Change {}",
            revision.revision_id().as_str(),
            change_id.as_str()
        ),
    })
}

fn assignment_create(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let assignment_id =
        AssignmentId::new(options.required("assignment-id")?).map_err(CliError::from_input)?;
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    let assignee = subject(&mut options, "subject-kind", "subject-id")?;
    let role = AssignmentRole::parse(&options.required("role")?).map_err(CliError::from_input)?;
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    options.finish()?;
    let assignment = Assignment::new(assignment_id, change_id, assignee, role, at, actor);
    let mut state = open_state(state_dir)?;
    state.store.create_assignment(&assignment, &context)?;
    let assignments = state.store.assignments(assignment.change_id())?;
    let stored = assignments
        .iter()
        .find(|candidate| candidate.id() == assignment.id())
        .ok_or_else(|| CliError::integrity("created assignment could not be read back"))?;
    Ok(Success {
        command: "assignment.create",
        data: assignment_view(stored),
        human: format!("created Assignment {}", stored.id().as_str()),
    })
}

fn assignment_list(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    state.store.load_change(&state.artifacts, &change_id)?;
    let assignments = state.store.assignments(&change_id)?;
    Ok(Success {
        command: "assignment.list",
        data: json!({
            "change_id": change_id.as_str(),
            "assignments": assignments.iter().map(assignment_view).collect::<Vec<_>>()
        }),
        human: format!(
            "{} assignment(s) for {}",
            assignments.len(),
            change_id.as_str()
        ),
    })
}

fn assignment_release(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let assignment_id =
        AssignmentId::new(options.required("assignment-id")?).map_err(CliError::from_input)?;
    let expected_version = coordination_version(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.require_yes()?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let assignment = state
        .store
        .release_assignment(&assignment_id, expected_version, &context)?;
    Ok(Success {
        command: "assignment.release",
        data: assignment_view(&assignment),
        human: format!("released Assignment {}", assignment.id().as_str()),
    })
}

fn lease_acquire(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let lease_id = LeaseId::new(options.required("lease-id")?).map_err(CliError::from_input)?;
    let scope = lease_scope(&mut options)?;
    let holder = subject(&mut options, "holder-kind", "holder-id")?;
    let expected_version = coordination_version(&mut options)?;
    let expires_at = unix_millis(&mut options, "expires-at")?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let lease = state.store.acquire_lease(
        &lease_id,
        &scope,
        &holder,
        expected_version,
        expires_at,
        &context,
    )?;
    Ok(Success {
        command: "lease.acquire",
        data: lease_view(&lease),
        human: format!("acquired Lease {}", lease.id().as_str()),
    })
}

fn lease_show(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let scope = lease_scope(&mut options)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    state
        .store
        .load_change(&state.artifacts, scope.change_id())?;
    let lease = state.store.current_lease(&scope)?;
    Ok(Success {
        command: "lease.show",
        data: json!({"scope": lease_scope_view(&scope), "lease": lease.as_ref().map(lease_view)}),
        human: lease.as_ref().map_or_else(
            || format!("no current Lease for {}", scope.operation().as_str()),
            |value| format!("current Lease {}", value.id().as_str()),
        ),
    })
}

fn lease_renew(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let lease_id = LeaseId::new(options.required("lease-id")?).map_err(CliError::from_input)?;
    let expected_version = coordination_version(&mut options)?;
    let expires_at = unix_millis(&mut options, "expires-at")?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let lease = state
        .store
        .renew_lease(&lease_id, expected_version, expires_at, &context)?;
    Ok(Success {
        command: "lease.renew",
        data: lease_view(&lease),
        human: format!("renewed Lease {}", lease.id().as_str()),
    })
}

fn lease_release(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let lease_id = LeaseId::new(options.required("lease-id")?).map_err(CliError::from_input)?;
    let expected_version = coordination_version(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.require_yes()?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let lease = state
        .store
        .release_lease(&lease_id, expected_version, &context)?;
    Ok(Success {
        command: "lease.release",
        data: lease_view(&lease),
        human: format!("released Lease {}", lease.id().as_str()),
    })
}

fn relationship_create(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        RelationshipId::new(options.required("relationship-id")?).map_err(CliError::from_input)?;
    let kind = RelationshipKind::parse(&options.required("kind")?).map_err(CliError::from_input)?;
    let endpoints = RelationshipEndpoints::new(
        ChangeId::new(options.required("left-change-id")?).map_err(CliError::from_input)?,
        ChangeId::new(options.required("right-change-id")?).map_err(CliError::from_input)?,
    )
    .map_err(CliError::from_input)?;
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    options.finish()?;
    let relationship = Relationship::new(id, kind, endpoints, at, actor);
    let mut state = open_state(state_dir)?;
    state.store.create_relationship(&relationship, &context)?;
    let stored = state.store.relationship(relationship.id())?;
    Ok(Success {
        command: "relationship.create",
        data: relationship_view(&stored),
        human: format!("created Relationship {}", stored.id().as_str()),
    })
}

fn relationship_list(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let values = state.store.relationships(&change_id)?;
    Ok(Success {
        command: "relationship.list",
        data: json!({
            "change_id": change_id.as_str(),
            "relationships": values.iter().map(relationship_view).collect::<Vec<_>>()
        }),
        human: format!(
            "{} relationship(s) for {}",
            values.len(),
            change_id.as_str()
        ),
    })
}

fn relationship_remove(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        RelationshipId::new(options.required("relationship-id")?).map_err(CliError::from_input)?;
    let expected = relationship_version(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.require_yes()?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let relationship = state.store.remove_relationship(&id, expected, &context)?;
    Ok(Success {
        command: "relationship.remove",
        data: relationship_view(&relationship),
        human: format!("removed Relationship {}", relationship.id().as_str()),
    })
}

fn dependency_create(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = DependencyId::new(options.required("dependency-id")?).map_err(CliError::from_input)?;
    let downstream =
        ChangeId::new(options.required("downstream-change-id")?).map_err(CliError::from_input)?;
    let upstream =
        ChangeId::new(options.required("upstream-change-id")?).map_err(CliError::from_input)?;
    let pins = dependency_pins(&mut options)?;
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    options.finish()?;
    let dependency =
        Dependency::new(id, downstream, upstream, pins, at, actor).map_err(CliError::from_input)?;
    let mut state = open_state(state_dir)?;
    state
        .store
        .create_dependency(&state.artifacts, &dependency, &context)?;
    let stored = state.store.dependency(&state.artifacts, dependency.id())?;
    Ok(Success {
        command: "dependency.create",
        data: dependency_view(&stored, None),
        human: format!("created Dependency {}", stored.id().as_str()),
    })
}

fn dependency_list(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let dependencies = state.store.dependencies(&state.artifacts, &change_id)?;
    let values = dependencies
        .iter()
        .map(|dependency| {
            state
                .store
                .dependency_freshness(&state.artifacts, dependency.id())
                .map(|freshness| dependency_view(dependency, Some(freshness)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Success {
        command: "dependency.list",
        data: json!({"change_id": change_id.as_str(), "dependencies": values}),
        human: format!(
            "{} dependency/dependencies for {}",
            dependencies.len(),
            change_id.as_str()
        ),
    })
}

fn dependency_repin(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = DependencyId::new(options.required("dependency-id")?).map_err(CliError::from_input)?;
    let expected = relationship_version(&mut options)?;
    let pins = dependency_pins(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let dependency =
        state
            .store
            .repin_dependency(&state.artifacts, &id, expected, pins, &context)?;
    let freshness = state
        .store
        .dependency_freshness(&state.artifacts, dependency.id())?;
    Ok(Success {
        command: "dependency.repin",
        data: dependency_view(&dependency, Some(freshness)),
        human: format!("repinned Dependency {}", dependency.id().as_str()),
    })
}

fn dependency_remove(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = DependencyId::new(options.required("dependency-id")?).map_err(CliError::from_input)?;
    let expected = relationship_version(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.require_yes()?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let dependency = state
        .store
        .remove_dependency(&state.artifacts, &id, expected, &context)?;
    Ok(Success {
        command: "dependency.remove",
        data: dependency_view(&dependency, Some(DependencyFreshness::Removed)),
        human: format!("removed Dependency {}", dependency.id().as_str()),
    })
}

fn stack_create(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = StackId::new(options.required("stack-id")?).map_err(CliError::from_input)?;
    let definition = stack_definition(&mut options)?;
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    options.finish()?;
    let stack = Stack::new(id, definition, at, actor);
    let mut state = open_state(state_dir)?;
    state.store.create_stack(&stack, &context)?;
    let stored = state.store.stack(stack.id())?;
    Ok(Success {
        command: "stack.create",
        data: stack_view(&stored),
        human: format!("created Stack {}", stored.id().as_str()),
    })
}

fn stack_show(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = StackId::new(options.required("stack-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let stack = state.store.stack(&id)?;
    Ok(Success {
        command: "stack.show",
        data: stack_view(&stack),
        human: format!(
            "Stack {} at version {}",
            stack.id().as_str(),
            stack.version().value()
        ),
    })
}

fn stack_replace(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = StackId::new(options.required("stack-id")?).map_err(CliError::from_input)?;
    let expected = stack_version(&mut options)?;
    let definition = stack_definition(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let stack = state
        .store
        .replace_stack(&id, expected, definition, &context)?;
    Ok(Success {
        command: "stack.replace",
        data: stack_view(&stack),
        human: format!("replaced Stack {}", stack.id().as_str()),
    })
}

fn candidate_create(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = CandidateId::new(options.required("candidate-id")?).map_err(CliError::from_input)?;
    let target_base = BaseState::new(
        RepositoryId::new(options.required("repository-id")?).map_err(CliError::from_input)?,
        options.required("target-object")?,
    )
    .map_err(CliError::from_input)?;
    let selection = candidate_selection(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let candidate =
        state
            .store
            .create_candidate(&state.artifacts, id, target_base, &selection, &context)?;
    Ok(Success {
        command: "candidate.create",
        data: candidate_view(&candidate, None),
        human: format!("created CompositionCandidate {}", candidate.id().as_str()),
    })
}

fn candidate_show(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = CandidateId::new(options.required("candidate-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let candidate = state.store.candidate(&state.artifacts, &id)?;
    let freshness = state.store.candidate_freshness(&state.artifacts, &id)?;
    Ok(Success {
        command: "candidate.show",
        data: candidate_view(&candidate, Some(&freshness)),
        human: format!("CompositionCandidate {}", candidate.id().as_str()),
    })
}

fn candidate_freshness(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = CandidateId::new(options.required("candidate-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let freshness = state.store.candidate_freshness(&state.artifacts, &id)?;
    Ok(Success {
        command: "candidate.freshness",
        data: candidate_freshness_view(&freshness),
        human: format!(
            "CompositionCandidate {} is {}",
            id.as_str(),
            if freshness.is_current() {
                "current"
            } else {
                "stale"
            }
        ),
    })
}

fn materialization_create(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = MaterializationId::new(options.required("materialization-id")?)
        .map_err(CliError::from_input)?;
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    let revision_id =
        RevisionId::new(options.required("revision-id")?).map_err(CliError::from_input)?;
    let placement = MaterializationPlacement::new(
        WorkspaceId::new(options.required("workspace-id")?).map_err(CliError::from_input)?,
        ProviderId::new(options.required("provider-id")?).map_err(CliError::from_input)?,
        ProviderRef::new(options.required("provider-ref")?).map_err(CliError::from_input)?,
    );
    let evidence = ProviderEvidence::new(options.required("provider-evidence")?)
        .map_err(CliError::from_input)?;
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    options.finish()?;
    let materialization = Materialization::new(id, change_id, revision_id, placement, at, actor);
    let mut state = open_state(state_dir)?;
    state
        .store
        .create_materialization(&state.artifacts, &materialization, &evidence, &context)?;
    let stored = state
        .store
        .materialization(&state.artifacts, materialization.id())?;
    Ok(Success {
        command: "materialization.create",
        data: materialization_view(&stored),
        human: format!("created Materialization {}", stored.id().as_str()),
    })
}

fn materialization_show(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = MaterializationId::new(options.required("materialization-id")?)
        .map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let materialization = state.store.materialization(&state.artifacts, &id)?;
    Ok(Success {
        command: "materialization.show",
        data: materialization_view(&materialization),
        human: format!("Materialization {}", materialization.id().as_str()),
    })
}

fn materialization_list(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let values = state.store.materializations(&state.artifacts, &change_id)?;
    Ok(Success {
        command: "materialization.list",
        data: json!({
            "change_id": change_id.as_str(),
            "materializations": values.iter().map(materialization_view).collect::<Vec<_>>()
        }),
        human: format!(
            "{} materialization(s) for {}",
            values.len(),
            change_id.as_str()
        ),
    })
}

fn materialization_transition(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = MaterializationId::new(options.required("materialization-id")?)
        .map_err(CliError::from_input)?;
    let expected = materialization_version(&mut options)?;
    let observed_state =
        MaterializationState::parse(&options.required("state")?).map_err(CliError::from_input)?;
    let observation = ProviderObservation::new(
        observed_state,
        ProviderRef::new(options.required("provider-ref")?).map_err(CliError::from_input)?,
        ProviderEvidence::new(options.required("provider-evidence")?)
            .map_err(CliError::from_input)?,
    );
    let context = mutation_context(&mut options)?;
    if observed_state.is_terminal() {
        options.require_yes()?;
    }
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let materialization = state.store.transition_materialization(
        &state.artifacts,
        &id,
        expected,
        observation,
        &context,
    )?;
    Ok(Success {
        command: "materialization.transition",
        data: materialization_view(&materialization),
        human: format!(
            "transitioned Materialization {} to {}",
            materialization.id().as_str(),
            materialization.state().as_str()
        ),
    })
}

fn review_request(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = ReviewRequestId::new(options.required("review-request-id")?)
        .map_err(CliError::from_input)?;
    let reviewers = parse_actors(&options.required("reviewers")?)?;
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    let mut state = open_state(state_dir)?;
    let target = exact_target(&state, &mut options)?;
    options.finish()?;
    let request =
        ReviewRequest::new(id, target, actor, reviewers, at).map_err(CliError::from_input)?;
    state
        .store
        .create_review_request(&state.artifacts, &request, &context)?;
    let stored = state.store.review_request(&state.artifacts, request.id())?;
    let freshness = state
        .store
        .review_request_freshness(&state.artifacts, stored.id())?;
    Ok(Success {
        command: "review.request",
        data: review_request_view(&stored, &freshness),
        human: format!("created ReviewRequest {}", stored.id().as_str()),
    })
}

fn review_show(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = ReviewRequestId::new(options.required("review-request-id")?)
        .map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let request = state.store.review_request(&state.artifacts, &id)?;
    let freshness = state
        .store
        .review_request_freshness(&state.artifacts, &id)?;
    Ok(Success {
        command: "review.show",
        data: review_request_view(&request, &freshness),
        human: format!("ReviewRequest {}", request.id().as_str()),
    })
}

fn review_submit(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = ReviewSubmissionId::new(options.required("review-submission-id")?)
        .map_err(CliError::from_input)?;
    let request_id = ReviewRequestId::new(options.required("review-request-id")?)
        .map_err(CliError::from_input)?;
    let outcome =
        ReviewOutcome::parse(&options.required("outcome")?).map_err(CliError::from_input)?;
    let comments = options.optional("comments");
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let request = state.store.review_request(&state.artifacts, &request_id)?;
    let submission = ReviewSubmission::new(id, &request, actor, outcome, comments, at)
        .map_err(CliError::from_input)?;
    state
        .store
        .create_review_submission(&state.artifacts, &submission, &context)?;
    let stored = state
        .store
        .review_submission(&state.artifacts, submission.id())?;
    Ok(Success {
        command: "review.submit",
        data: review_submission_view(&stored),
        human: format!("recorded ReviewSubmission {}", stored.id().as_str()),
    })
}

fn review_submissions(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = ReviewRequestId::new(options.required("review-request-id")?)
        .map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let submissions = state.store.review_submissions(&state.artifacts, &id)?;
    Ok(Success {
        command: "review.submissions",
        data: json!({
            "review_request_id": id.as_str(),
            "submissions": submissions.iter().map(review_submission_view).collect::<Vec<_>>()
        }),
        human: format!("{} submission(s) for {}", submissions.len(), id.as_str()),
    })
}

fn validation_record(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = ValidationResultId::new(options.required("validation-result-id")?)
        .map_err(CliError::from_input)?;
    let observation = validation_observation(&mut options)?;
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    let mut state = open_state(state_dir)?;
    let target = exact_target(&state, &mut options)?;
    options.finish()?;
    let result = ValidationResult::new(id, target, observation, actor, at);
    state
        .store
        .create_validation_result(&state.artifacts, &result, &context)?;
    let stored = state
        .store
        .validation_result(&state.artifacts, result.id())?;
    let freshness = state
        .store
        .validation_result_freshness(&state.artifacts, stored.id())?;
    Ok(Success {
        command: "validation.record",
        data: validation_result_view(&stored, &freshness),
        human: format!("recorded ValidationResult {}", stored.id().as_str()),
    })
}

fn validation_show(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id = ValidationResultId::new(options.required("validation-result-id")?)
        .map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let result = state.store.validation_result(&state.artifacts, &id)?;
    let freshness = state
        .store
        .validation_result_freshness(&state.artifacts, &id)?;
    Ok(Success {
        command: "validation.show",
        data: validation_result_view(&result, &freshness),
        human: format!("ValidationResult {}", result.id().as_str()),
    })
}

#[allow(clippy::too_many_lines)]
fn integration_plan(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let candidate_id =
        CandidateId::new(options.required("candidate-id")?).map_err(CliError::from_input)?;
    let target_ref =
        TargetRef::new(options.required("target-ref")?).map_err(CliError::from_input)?;
    let expected =
        TargetRevision::new(options.required("expected-target")?).map_err(CliError::from_input)?;
    let provider_id =
        ProviderId::new(options.required("provider-id")?).map_err(CliError::from_input)?;
    let strategy =
        IntegrationStrategy::new(options.required("strategy")?).map_err(CliError::from_input)?;
    let effect = EffectOperationId::new(options.required("effect-operation-id")?)
        .map_err(CliError::from_input)?;
    let policy = GatePolicyEvidence::new(options.required("policy-evidence")?)
        .map_err(CliError::from_input)?;
    let requested_capability = options.optional("capability-evidence");
    let requested_observed_target = options.optional("observed-target");
    let requested_observation_evidence = options.optional("observation-evidence");
    let reviews = parse_optional_review_ids(options.optional("reviews"))?;
    let validations = parse_optional_validation_ids(options.optional("validations"))?;
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    let mut state = open_state(state_dir)?;
    let candidate = state.store.candidate(&state.artifacts, &candidate_id)?;
    let (capability, observation) = if provider_id.as_str() == "native-git" {
        let repository = options.required("repository")?;
        let provider_revisions = options.required("provider-revisions")?;
        let scratch = options.required("scratch")?;
        options.finish()?;
        let (provider, composition) = compose_live_native_candidate(
            &state,
            Path::new(&repository),
            candidate.inputs(),
            candidate.target_base().repository_id(),
            &provider_revisions,
            Path::new(&scratch),
        )?;
        let discovery = provider.discover(&repository)?;
        let provider_plan = provider.plan_integration(
            Path::new(&repository),
            candidate.target_base().repository_id(),
            target_ref.as_str(),
            expected.as_str(),
            &composition,
            &effect,
        )?;
        let observed = provider.observe_target(Path::new(&repository), target_ref.as_str())?;
        verify_requested_observation(
            requested_observed_target.as_deref(),
            requested_observation_evidence.as_deref(),
            &observed,
        )?;
        let _ = requested_capability;
        (
            IntegrationCapabilityEvidence::new(native_git_plan_evidence(
                &discovery.provider_locator_evidence,
                provider_plan.candidate_tree(),
            ))
            .map_err(CliError::from_input)?,
            observed,
        )
    } else {
        options.finish()?;
        let capability = IntegrationCapabilityEvidence::new(
            requested_capability
                .ok_or_else(|| CliError::usage("missing required option --capability-evidence"))?,
        )
        .map_err(CliError::from_input)?;
        let observation = TargetObservation::new(
            target_ref.clone(),
            TargetRevision::new(
                requested_observed_target
                    .ok_or_else(|| CliError::usage("missing required option --observed-target"))?,
            )
            .map_err(CliError::from_input)?,
            IntegrationEvidence::new(requested_observation_evidence.ok_or_else(|| {
                CliError::usage("missing required option --observation-evidence")
            })?)
            .map_err(CliError::from_input)?,
        );
        (capability, observation)
    };
    let binding = IntegrationBinding::new(
        candidate.id().clone(),
        candidate.content_digest().as_str(),
        candidate.inputs().to_vec(),
    )
    .map_err(CliError::from_input)?;
    let attempt = IntegrationAttempt::plan(
        id,
        IntegrationIntent::new(
            binding,
            IntegrationTarget::new(
                candidate.target_base().repository_id().clone(),
                target_ref,
                expected,
            ),
            IntegrationMethod::new(provider_id, strategy, effect),
        ),
        IntegrationGate::new(policy, capability, reviews, validations, observation),
        at,
        actor,
    )
    .map_err(CliError::from_input)?;
    state
        .store
        .create_integration_attempt(&state.artifacts, &attempt, &context)?;
    let stored = state
        .store
        .integration_attempt(&state.artifacts, attempt.id())?;
    Ok(integration_success("integration.plan", &stored, "planned"))
}

fn integration_show(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let state = open_state(state_dir)?;
    let attempt = state.store.integration_attempt(&state.artifacts, &id)?;
    Ok(integration_success("integration.show", &attempt, "loaded"))
}

fn integration_start(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected = integration_version(&mut options)?;
    let lease_id =
        ExecutionLeaseId::new(options.required("lease-id")?).map_err(CliError::from_input)?;
    let holder = subject(&mut options, "holder-kind", "holder-id")?;
    let expires_at = unix_millis(&mut options, "expires-at")?;
    let observation = target_observation(&mut options)?;
    let (context, _actor, at) = mutation_context_parts(&mut options)?;
    options.finish()?;
    let lease =
        ExecutionLease::new(lease_id, holder, at, expires_at).map_err(CliError::from_input)?;
    let mut state = open_state(state_dir)?;
    let attempt = state.store.start_integration(
        &state.artifacts,
        &id,
        expected,
        lease,
        &observation,
        &context,
    )?;
    Ok(integration_success(
        "integration.start",
        &attempt,
        "started",
    ))
}

fn integration_renew(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected = integration_version(&mut options)?;
    let lease_id =
        ExecutionLeaseId::new(options.required("lease-id")?).map_err(CliError::from_input)?;
    let holder = subject(&mut options, "holder-kind", "holder-id")?;
    let expires_at = unix_millis(&mut options, "expires-at")?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let attempt = state.store.renew_integration_lease(
        &state.artifacts,
        &id,
        &LeaseRenewal {
            expected_version: expected,
            lease_id,
            holder,
            expires_at,
        },
        &context,
    )?;
    Ok(integration_success(
        "integration.renew",
        &attempt,
        "renewed",
    ))
}

fn integration_uncertain(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected = integration_version(&mut options)?;
    let reconciliation_id = ReconciliationId::new(options.required("reconciliation-id")?)
        .map_err(CliError::from_input)?;
    let authority = optional_authority(&mut options)?;
    let observation = target_observation(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let (attempt, _) = state.store.enter_integration_reconciliation(
        &state.artifacts,
        &id,
        &ReconciliationStart {
            expected_version: expected,
            reconciliation_id,
            authority,
            observation,
        },
        &context,
    )?;
    Ok(integration_success(
        "integration.uncertain",
        &attempt,
        "entered reconciliation",
    ))
}

fn integration_reconcile(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected = integration_version(&mut options)?;
    let reconciliation_id = ReconciliationId::new(options.required("reconciliation-id")?)
        .map_err(CliError::from_input)?;
    let outcome = ReconciliationOutcome::parse(&options.required("outcome")?)
        .map_err(CliError::from_input)?;
    let observation = target_observation(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let (attempt, _) = state.store.reconcile_integration(
        &state.artifacts,
        &id,
        &ReconciliationRecord {
            expected_version: expected,
            reconciliation_id,
            outcome,
            observation,
        },
        &context,
    )?;
    Ok(integration_success(
        "integration.reconcile",
        &attempt,
        "reconciled",
    ))
}

fn integration_conflict(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected = integration_version(&mut options)?;
    let conflict_id = IntegrationConflictId::new(options.required("conflict-id")?)
        .map_err(CliError::from_input)?;
    let authority = optional_authority(&mut options)?;
    let provider_state = IntegrationEvidence::new(options.required("provider-state")?)
        .map_err(CliError::from_input)?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let (attempt, _) = state.store.conflict_integration(
        &state.artifacts,
        &id,
        &ConflictReport {
            expected_version: expected,
            conflict_id,
            authority,
            provider_state,
        },
        &context,
    )?;
    Ok(integration_success(
        "integration.conflict",
        &attempt,
        "conflicted",
    ))
}

fn integration_succeed(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected = integration_version(&mut options)?;
    let receipt_id =
        IntegrationReceiptId::new(options.required("receipt-id")?).map_err(CliError::from_input)?;
    let authority = optional_authority(&mut options)?;
    let observation = target_observation(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let (attempt, receipt) = state.store.succeed_integration(
        &state.artifacts,
        &id,
        &SuccessVerification {
            expected_version: expected,
            receipt_id,
            authority,
            observation,
        },
        &context,
    )?;
    Ok(Success {
        command: "integration.succeed",
        data: json!({
            "attempt": integration_view(&attempt),
            "receipt": {
                "receipt_id": receipt.id().as_str(),
                "effect_operation_id": receipt.effect_operation_id().as_str(),
                "result_revision": receipt.result_revision().as_str(),
                "evidence": receipt.verification_evidence().as_str()
            }
        }),
        human: format!("succeeded IntegrationAttempt {}", attempt.id().as_str()),
    })
}

fn integration_finish(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected = integration_version(&mut options)?;
    let state_value =
        IntegrationState::parse(&options.required("state")?).map_err(CliError::from_input)?;
    if !matches!(
        state_value,
        IntegrationState::Failed | IntegrationState::Aborted
    ) {
        return Err(CliError::usage("--state must be failed or aborted"));
    }
    let context = mutation_context(&mut options)?;
    options.require_yes()?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let attempt = state.store.finish_integration_no_effect(
        &state.artifacts,
        &id,
        expected,
        state_value,
        &context,
    )?;
    Ok(integration_success(
        "integration.finish",
        &attempt,
        "finished",
    ))
}

fn integration_abort(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected = integration_version(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.require_yes()?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let attempt =
        state
            .store
            .abort_planned_integration(&state.artifacts, &id, expected, &context)?;
    Ok(integration_success(
        "integration.abort",
        &attempt,
        "aborted",
    ))
}

fn integration_supersede(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected = integration_version(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.require_yes()?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let attempt =
        state
            .store
            .supersede_diverged_integration(&state.artifacts, &id, expected, &context)?;
    Ok(integration_success(
        "integration.supersede",
        &attempt,
        "superseded",
    ))
}

fn native_git_discover(mut options: Options) -> Result<Success, CliError> {
    let repository = options.required("repository")?;
    options.finish()?;
    let discovery = NativeGit::with_defaults().discover(&repository)?;
    let capabilities = [
        (
            "exact_revision_inspection",
            GitCapability::ExactRevisionInspection,
        ),
        ("canonical_capture", GitCapability::CanonicalCapture),
        ("detached_worktrees", GitCapability::DetachedWorktrees),
        ("candidate_composition", GitCapability::CandidateComposition),
        ("guarded_ref_update", GitCapability::GuardedRefUpdate),
        ("conflict_capture", GitCapability::ConflictCapture),
        ("reconciliation", GitCapability::Reconciliation),
    ];
    Ok(Success {
        command: "native-git.discover",
        data: json!({
            "worktree_root": display_path(&discovery.worktree_root),
            "common_git_directory": display_path(&discovery.common_git_directory),
            "provider_locator_evidence": discovery.provider_locator_evidence,
            "object_format": discovery.object_format,
            "git_version": discovery.git_version,
            "capabilities": capabilities.into_iter().map(|(name, capability)| json!({
                "name": name, "supported": discovery.capabilities.supports(capability)
            })).collect::<Vec<_>>()
        }),
        human: format!(
            "discovered Native Git repository at {}",
            discovery.worktree_root.display()
        ),
    })
}

fn native_git_inspect(mut options: Options) -> Result<Success, CliError> {
    let repository = options.required("repository")?;
    let revision = options.required("revision")?;
    options.finish()?;
    let observation =
        NativeGit::with_defaults().inspect_revision(Path::new(&repository), &revision)?;
    Ok(Success {
        command: "native-git.inspect",
        data: json!({
            "commit_id": observation.commit_id(),
            "tree_id": observation.tree_id(),
            "evidence": observation.evidence()
        }),
        human: format!("inspected Native Git commit {}", observation.commit_id()),
    })
}

fn native_git_capture(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let repository = options.required("repository")?;
    let repository_id =
        RepositoryId::new(options.required("repository-id")?).map_err(CliError::from_input)?;
    let base_revision = options.required("base-revision")?;
    let provider_revision = options.required("provider-revision")?;
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    let revision_id =
        RevisionId::new(options.required("revision-id")?).map_err(CliError::from_input)?;
    let expected_value = options.required("expected-head")?;
    let expected_head = if expected_value == "none" {
        None
    } else {
        Some(RevisionId::new(expected_value).map_err(CliError::from_input)?)
    };
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let captured = NativeGit::with_defaults().capture_revision(
        Path::new(&repository),
        repository_id,
        &base_revision,
        &provider_revision,
        &state.artifacts,
    )?;
    let manifest = state
        .artifacts
        .load_manifest(captured.artifact_ref())
        .map_err(|_| CliError::integrity("captured canonical artifact could not be read back"))?;
    let revision = NewRevision::new(
        revision_id,
        manifest.base().clone(),
        captured.artifact_ref().clone(),
        at,
        actor,
    );
    state.store.append_revision(
        &state.artifacts,
        &change_id,
        expected_head.as_ref(),
        &revision,
        &context,
    )?;
    let change = state.store.load_change(&state.artifacts, &change_id)?;
    Ok(Success {
        command: "native-git.capture",
        data: json!({
            "change": change_view(&change),
            "provider": {
                "commit_id": captured.observation().commit_id(),
                "tree_id": captured.observation().tree_id(),
                "evidence": captured.observation().evidence(),
                "changed_paths": captured.changed_paths()
            }
        }),
        human: format!(
            "captured Native Git revision {}",
            captured.observation().commit_id()
        ),
    })
}

fn native_git_materialize(state_dir: &Path, mut options: Options) -> Result<Success, CliError> {
    let repository = options.required("repository")?;
    let provider_revision = options.required("provider-revision")?;
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    let revision_id =
        RevisionId::new(options.required("revision-id")?).map_err(CliError::from_input)?;
    let destination = options.required("destination")?;
    let materialization_id = MaterializationId::new(options.required("materialization-id")?)
        .map_err(CliError::from_input)?;
    let workspace_id =
        WorkspaceId::new(options.required("workspace-id")?).map_err(CliError::from_input)?;
    let (context, actor, at) = mutation_context_parts(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let provider = NativeGit::with_defaults();
    let (captured, repository_id) = recapture_exact_revision(
        &state,
        &provider,
        Path::new(&repository),
        &change_id,
        &revision_id,
        &provider_revision,
    )?;
    let result = provider.materialize(
        Path::new(&repository),
        &repository_id,
        &captured,
        &state.artifacts,
        Path::new(&destination),
    )?;
    let materialization = Materialization::new(
        materialization_id,
        change_id,
        revision_id,
        MaterializationPlacement::new(
            workspace_id,
            ProviderId::new("native-git").map_err(CliError::from_input)?,
            ProviderRef::new(result.provider_ref.clone()).map_err(CliError::from_input)?,
        ),
        at,
        actor,
    );
    let evidence = ProviderEvidence::new(result.evidence.clone()).map_err(CliError::from_input)?;
    state
        .store
        .create_materialization(&state.artifacts, &materialization, &evidence, &context)?;
    Ok(Success {
        command: "native-git.materialize",
        data: json!({
            "materialization": materialization_view(&materialization),
            "provider": {
                "path": display_path(&result.path),
                "base_commit": result.base_commit,
                "resulting_tree": result.resulting_tree,
                "evidence": result.evidence
            }
        }),
        human: format!("materialized exact revision at {}", result.path.display()),
    })
}

fn native_git_observe_materialization(
    state_dir: &Path,
    mut options: Options,
) -> Result<Success, CliError> {
    let repository = options.required("repository")?;
    let worktree = options.required("worktree")?;
    let provider_revision = options.required("provider-revision")?;
    let materialization_id = MaterializationId::new(options.required("materialization-id")?)
        .map_err(CliError::from_input)?;
    let expected = materialization_version(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let materialization = state
        .store
        .materialization(&state.artifacts, &materialization_id)?;
    let provider = NativeGit::with_defaults();
    let (captured, _) = recapture_exact_revision(
        &state,
        &provider,
        Path::new(&repository),
        materialization.change_id(),
        materialization.revision_id(),
        &provider_revision,
    )?;
    let manifest = state
        .artifacts
        .load_manifest(captured.artifact_ref())
        .map_err(|_| CliError::integrity("materialization artifact could not be read"))?;
    let observation = provider.observe_materialization(
        Path::new(&worktree),
        manifest.base().object_id(),
        captured.observation().tree_id(),
    )?;
    let updated = state.store.transition_materialization(
        &state.artifacts,
        &materialization_id,
        expected,
        observation,
        &context,
    )?;
    Ok(Success {
        command: "native-git.observe-materialization",
        data: materialization_view(&updated),
        human: format!(
            "observed Materialization {} as {}",
            updated.id().as_str(),
            updated.state().as_str()
        ),
    })
}

fn native_git_release_materialization(
    state_dir: &Path,
    mut options: Options,
) -> Result<Success, CliError> {
    let repository = options.required("repository")?;
    let worktree = options.required("worktree")?;
    let materialization_id = MaterializationId::new(options.required("materialization-id")?)
        .map_err(CliError::from_input)?;
    let expected = materialization_version(&mut options)?;
    let context = mutation_context(&mut options)?;
    options.require_yes()?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let materialization = state
        .store
        .materialization(&state.artifacts, &materialization_id)?;
    NativeGit::with_defaults()
        .release_materialization(Path::new(&repository), Path::new(&worktree))?;
    let observation = ProviderObservation::new(
        MaterializationState::Released,
        materialization.provider_ref().clone(),
        ProviderEvidence::new("native-git:worktree-release-verified")
            .map_err(CliError::from_input)?,
    );
    let updated = state.store.transition_materialization(
        &state.artifacts,
        &materialization_id,
        expected,
        observation,
        &context,
    )?;
    Ok(Success {
        command: "native-git.release-materialization",
        data: materialization_view(&updated),
        human: format!("released Materialization {}", updated.id().as_str()),
    })
}

fn recapture_exact_revision(
    state: &State,
    provider: &NativeGit,
    repository: &Path,
    change_id: &ChangeId,
    revision_id: &RevisionId,
    provider_revision: &str,
) -> Result<(CapturedRevision, RepositoryId), CliError> {
    let change = state.store.load_change(&state.artifacts, change_id)?;
    let revision = change
        .revisions()
        .iter()
        .find(|value| value.revision_id() == revision_id)
        .ok_or_else(|| {
            CliError::usage(format!(
                "Revision {} does not belong to Change {}",
                revision_id.as_str(),
                change_id.as_str()
            ))
        })?;
    let repository_id = revision.base().repository_id().clone();
    let captured = provider.capture_revision(
        repository,
        repository_id.clone(),
        revision.base().object_id(),
        provider_revision,
        &state.artifacts,
    )?;
    if captured.artifact_ref() != revision.artifact() {
        return Err(CliError::integrity(
            "provider revision content does not match the exact durable revision",
        ));
    }
    Ok((captured, repository_id))
}

#[allow(clippy::too_many_lines)]
fn native_git_execute_integration(
    state_dir: &Path,
    mut options: Options,
) -> Result<Success, CliError> {
    let repository = options.required("repository")?;
    let integration_id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected_version = integration_version(&mut options)?;
    let scratch = options.required("scratch")?;
    let lease_id =
        ExecutionLeaseId::new(options.required("lease-id")?).map_err(CliError::from_input)?;
    let holder = subject(&mut options, "holder-kind", "holder-id")?;
    let expires_at = unix_millis(&mut options, "expires-at")?;
    let receipt_id =
        IntegrationReceiptId::new(options.required("receipt-id")?).map_err(CliError::from_input)?;
    let reconciliation_id = ReconciliationId::new(options.required("reconciliation-id")?)
        .map_err(CliError::from_input)?;
    let start_operation = options.required("start-operation-id")?;
    let finish_operation = options.required("finish-operation-id")?;
    let actor = ActorId::new(options.required("actor")?).map_err(CliError::from_input)?;
    let at = unix_millis(&mut options, "at")?;
    options.finish()?;
    let start_context = MutationContext::new(start_operation, actor.clone(), at)?;
    let finish_context = MutationContext::new(finish_operation, actor, at)?;
    let mut state = open_state(state_dir)?;
    let attempt = state
        .store
        .integration_attempt(&state.artifacts, &integration_id)?;
    let (provider, plan) = prepare_native_git_plan(
        &state,
        Path::new(&repository),
        &attempt,
        Path::new(&scratch),
    )?;
    let observation = provider.observe_target(
        Path::new(&repository),
        attempt.intent().target().target_ref().as_str(),
    )?;
    let lease = ExecutionLease::new(lease_id.clone(), holder.clone(), at, expires_at)
        .map_err(CliError::from_input)?;
    let running = state.store.start_integration(
        &state.artifacts,
        &integration_id,
        expected_version,
        lease,
        &observation,
        &start_context,
    )?;
    match provider.execute_integration(
        Path::new(&repository),
        attempt.intent().target().repository_id(),
        &plan,
    ) {
        Ok(result) => {
            let verified = TargetObservation::new(
                attempt.intent().target().target_ref().clone(),
                TargetRevision::new(result.result_revision.clone())
                    .map_err(CliError::from_input)?,
                IntegrationEvidence::new(result.evidence.clone()).map_err(CliError::from_input)?,
            );
            let (succeeded, receipt) = state.store.succeed_integration(
                &state.artifacts,
                &integration_id,
                &SuccessVerification {
                    expected_version: running.version(),
                    receipt_id,
                    authority: Some((lease_id, holder)),
                    observation: verified,
                },
                &finish_context,
            )?;
            Ok(Success {
                command: "native-git.execute-integration",
                data: json!({
                    "attempt": integration_view(&succeeded),
                    "receipt": {
                        "receipt_id": receipt.id().as_str(),
                        "result_revision": receipt.result_revision().as_str(),
                        "effect_operation_id": receipt.effect_operation_id().as_str(),
                        "evidence": receipt.verification_evidence().as_str()
                    },
                    "provider": {
                        "prior_target": result.prior_target,
                        "result_revision": result.result_revision,
                        "result_tree": result.result_tree,
                        "evidence": result.evidence
                    }
                }),
                human: format!(
                    "executed Native Git IntegrationAttempt {}",
                    integration_id.as_str()
                ),
            })
        }
        Err(error) => {
            let uncertain_observation = provider
                .observe_target(
                    Path::new(&repository),
                    attempt.intent().target().target_ref().as_str(),
                )
                .unwrap_or_else(|_| {
                    TargetObservation::new(
                        attempt.intent().target().target_ref().clone(),
                        attempt.intent().target().expected_revision().clone(),
                        IntegrationEvidence::new(
                            "native-git:post-execution-observation-unavailable",
                        )
                        .expect("static evidence is valid"),
                    )
                });
            state.store.enter_integration_reconciliation(
                &state.artifacts,
                &integration_id,
                &ReconciliationStart {
                    expected_version: running.version(),
                    reconciliation_id,
                    authority: Some((lease_id, holder)),
                    observation: uncertain_observation,
                },
                &finish_context,
            )?;
            Err(CliError::from(error))
        }
    }
}

fn native_git_reconcile_integration(
    state_dir: &Path,
    mut options: Options,
) -> Result<Success, CliError> {
    let repository = options.required("repository")?;
    let integration_id =
        IntegrationId::new(options.required("integration-id")?).map_err(CliError::from_input)?;
    let expected_version = integration_version(&mut options)?;
    let scratch = options.required("scratch")?;
    let reconciliation_id = ReconciliationId::new(options.required("reconciliation-id")?)
        .map_err(CliError::from_input)?;
    let result_hint = options.optional("result-hint");
    let context = mutation_context(&mut options)?;
    options.finish()?;
    let mut state = open_state(state_dir)?;
    let attempt = state
        .store
        .integration_attempt(&state.artifacts, &integration_id)?;
    let (provider, plan) = prepare_native_git_plan(
        &state,
        Path::new(&repository),
        &attempt,
        Path::new(&scratch),
    )?;
    let result = provider.reconcile_integration(
        Path::new(&repository),
        attempt.intent().target().repository_id(),
        &plan,
        result_hint.as_deref(),
    )?;
    let (outcome, observed_revision, evidence) = match &result {
        ReconciliationResult::ResultVerified(value) => (
            ReconciliationOutcome::ResultVerified,
            value.result_revision.as_str(),
            value.evidence.as_str(),
        ),
        ReconciliationResult::Diverged {
            observed_target,
            evidence,
        } => (
            ReconciliationOutcome::Diverged,
            observed_target.as_str(),
            evidence.as_str(),
        ),
        ReconciliationResult::StillUncertain { evidence } => (
            ReconciliationOutcome::StillUncertain,
            attempt.intent().target().expected_revision().as_str(),
            evidence.as_str(),
        ),
    };
    let observation = TargetObservation::new(
        attempt.intent().target().target_ref().clone(),
        TargetRevision::new(observed_revision).map_err(CliError::from_input)?,
        IntegrationEvidence::new(evidence).map_err(CliError::from_input)?,
    );
    let (updated, _) = state.store.reconcile_integration(
        &state.artifacts,
        &integration_id,
        &ReconciliationRecord {
            expected_version,
            reconciliation_id,
            outcome,
            observation,
        },
        &context,
    )?;
    Ok(Success {
        command: "native-git.reconcile-integration",
        data: json!({
            "attempt": integration_view(&updated),
            "provider_outcome": outcome.as_str()
        }),
        human: format!(
            "reconciled Native Git IntegrationAttempt {} as {}",
            integration_id.as_str(),
            outcome.as_str()
        ),
    })
}

fn compose_live_native_candidate(
    state: &State,
    repository: &Path,
    inputs: &[weft_domain::CandidateInput],
    repository_id: &RepositoryId,
    provider_revisions: &str,
    scratch: &Path,
) -> Result<(NativeGit, CandidateComposition), CliError> {
    let revisions = provider_revisions.split(',').collect::<Vec<_>>();
    if revisions.len() != inputs.len() {
        return Err(CliError::usage(
            "--provider-revisions must align one-for-one with ordered candidate inputs",
        ));
    }
    let provider = NativeGit::with_defaults();
    let mut captured = Vec::with_capacity(inputs.len());
    for (input, provider_revision) in inputs.iter().zip(revisions) {
        let (value, observed_repository_id) = recapture_exact_revision(
            state,
            &provider,
            repository,
            input.change_id(),
            input.revision_id(),
            provider_revision,
        )?;
        if &observed_repository_id != repository_id {
            return Err(CliError::integrity(
                "candidate input repository differs from integration target",
            ));
        }
        captured.push(value);
    }
    let composition = provider.compose_candidate(
        repository,
        repository_id,
        &captured,
        &state.artifacts,
        scratch,
    )?;
    Ok((provider, composition))
}

fn native_git_plan_evidence(locator_evidence: &str, candidate_tree: &str) -> String {
    format!("native-git-plan-v1;{candidate_tree};{locator_evidence}")
}

fn parse_native_git_plan_evidence(value: &str) -> Result<(&str, &str), CliError> {
    value
        .strip_prefix("native-git-plan-v1;")
        .and_then(|rest| rest.split_once(';'))
        .ok_or_else(|| CliError::integrity("Native Git durable plan evidence is malformed"))
}

fn verify_requested_observation(
    requested_target: Option<&str>,
    _requested_evidence: Option<&str>,
    observed: &TargetObservation,
) -> Result<(), CliError> {
    if requested_target.is_some_and(|value| value != observed.observed_revision().as_str()) {
        return Err(CliError::usage(
            "--observed-target differs from the provider observation",
        ));
    }
    Ok(())
}

fn prepare_native_git_plan(
    state: &State,
    repository: &Path,
    attempt: &IntegrationAttempt,
    scratch: &Path,
) -> Result<(NativeGit, IntegrationPlan), CliError> {
    if attempt.intent().method().provider_id().as_str() != "native-git"
        || attempt.intent().method().strategy().as_str() != "squash"
    {
        return Err(CliError::usage(
            "Native Git execution requires provider native-git and strategy squash",
        ));
    }
    let (candidate_tree, locator_evidence) =
        parse_native_git_plan_evidence(attempt.gate().capability_evidence().as_str())?;
    let provider = NativeGit::with_defaults();
    let mut artifacts = Vec::new();
    for input in attempt.intent().binding().ordered_inputs() {
        let change = state
            .store
            .load_change(&state.artifacts, input.change_id())?;
        let revision = change
            .revisions()
            .iter()
            .find(|value| value.revision_id() == input.revision_id())
            .ok_or_else(|| CliError::integrity("integration input revision is unavailable"))?;
        artifacts.push(revision.artifact().clone());
    }
    provider.reconstruct_candidate(
        repository,
        attempt.intent().target().repository_id(),
        attempt.intent().target().expected_revision().as_str(),
        &artifacts,
        candidate_tree,
        &state.artifacts,
        scratch,
    )?;
    let target = attempt.intent().target();
    let method = attempt.intent().method();
    let plan = provider.rehydrate_integration_plan(
        repository,
        target.repository_id(),
        locator_evidence,
        target.target_ref().as_str(),
        target.expected_revision().as_str(),
        candidate_tree,
        method.effect_operation_id(),
    )?;
    Ok((provider, plan))
}

fn gitbutler_discover(mut options: Options) -> Result<Success, CliError> {
    let repository = options.required("repository")?;
    let repository_id =
        RepositoryId::new(options.required("repository-id")?).map_err(CliError::from_input)?;
    options.finish()?;
    let discovery = GitButler::with_defaults().discover(&repository, repository_id)?;
    let capabilities = [
        ("status_inspection", GitButlerCapability::StatusInspection),
        (
            "parallel_materializations",
            GitButlerCapability::ParallelMaterializations,
        ),
        ("stack_mapping", GitButlerCapability::StackMapping),
        ("canonical_export", GitButlerCapability::CanonicalExport),
        ("conflict_mapping", GitButlerCapability::ConflictMapping),
        (
            "external_state_reconciliation",
            GitButlerCapability::ExternalStateReconciliation,
        ),
        (
            "guarded_local_fast_forward_landing",
            GitButlerCapability::GuardedLocalFastForwardLanding,
        ),
        ("canonical_import", GitButlerCapability::CanonicalImport),
        ("provider_reconnect", GitButlerCapability::ProviderReconnect),
        ("remote_landing", GitButlerCapability::RemoteLanding),
    ];
    Ok(Success {
        command: "gitbutler.discover",
        data: json!({
            "repository_id": discovery.repository_id().as_str(),
            "worktree_root": display_path(discovery.worktree_root()),
            "common_git_directory": display_path(discovery.common_git_directory()),
            "version": discovery.version(),
            "target_ref": discovery.target_ref(),
            "local_target": discovery.local_target(),
            "evidence": discovery.evidence(),
            "capabilities": capabilities.into_iter().map(|(name, capability)| json!({
                "name": name, "supported": discovery.capabilities().supports(capability)
            })).collect::<Vec<_>>()
        }),
        human: format!(
            "discovered GitButler {} at {}",
            discovery.version(),
            discovery.worktree_root().display()
        ),
    })
}

fn mutation_context(options: &mut Options) -> Result<MutationContext, CliError> {
    mutation_context_parts(options).map(|(context, _actor, _at)| context)
}

fn mutation_context_parts(
    options: &mut Options,
) -> Result<(MutationContext, ActorId, UnixMillis), CliError> {
    let operation_id = options.required("operation-id")?;
    let actor = ActorId::new(options.required("actor")?).map_err(CliError::from_input)?;
    let at = unix_millis(options, "at")?;
    let context = MutationContext::new(operation_id, actor.clone(), at)?;
    Ok((context, actor, at))
}

fn unix_millis(options: &mut Options, name: &str) -> Result<UnixMillis, CliError> {
    let value = options.required(name)?.parse::<i64>().map_err(|_| {
        CliError::usage(format!(
            "--{name} must be a non-negative Unix millisecond integer"
        ))
    })?;
    UnixMillis::new(value).map_err(CliError::from_input)
}

fn coordination_version(options: &mut Options) -> Result<CoordinationVersion, CliError> {
    let value = options
        .required("expected-version")?
        .parse::<i64>()
        .map_err(|_| CliError::usage("--expected-version must be a non-negative integer"))?;
    CoordinationVersion::new(value).map_err(CliError::from_input)
}

fn subject(options: &mut Options, kind_name: &str, id_name: &str) -> Result<Subject, CliError> {
    let kind = SubjectKind::parse(&options.required(kind_name)?).map_err(CliError::from_input)?;
    let id = SubjectId::new(options.required(id_name)?).map_err(CliError::from_input)?;
    Ok(Subject::new(kind, id))
}

fn lease_scope(options: &mut Options) -> Result<LeaseScope, CliError> {
    let change_id = ChangeId::new(options.required("change-id")?).map_err(CliError::from_input)?;
    let operation =
        LeaseOperation::new(options.required("operation")?).map_err(CliError::from_input)?;
    Ok(LeaseScope::new(change_id, operation))
}

fn relationship_version(options: &mut Options) -> Result<RelationshipVersion, CliError> {
    let value = options
        .required("expected-version")?
        .parse::<i64>()
        .map_err(|_| CliError::usage("--expected-version must be a non-negative integer"))?;
    RelationshipVersion::new(value).map_err(CliError::from_input)
}

fn stack_version(options: &mut Options) -> Result<StackVersion, CliError> {
    let value = options
        .required("expected-version")?
        .parse::<i64>()
        .map_err(|_| CliError::usage("--expected-version must be a non-negative integer"))?;
    StackVersion::new(value).map_err(CliError::from_input)
}

fn dependency_pins(options: &mut Options) -> Result<DependencyPins, CliError> {
    Ok(DependencyPins::new(
        RevisionId::new(options.required("downstream-revision-id")?)
            .map_err(CliError::from_input)?,
        RevisionId::new(options.required("upstream-revision-id")?).map_err(CliError::from_input)?,
    ))
}

fn stack_definition(options: &mut Options) -> Result<StackDefinition, CliError> {
    let policy = StackPolicy::parse(&options.required("policy")?).map_err(CliError::from_input)?;
    let changes = parse_change_ids(&options.required("changes")?)?;
    StackDefinition::from_changes(policy, changes).map_err(CliError::from_input)
}

fn candidate_selection(options: &mut Options) -> Result<CandidateSelection, CliError> {
    match (options.optional("changes"), options.optional("stack-id")) {
        (Some(changes), None) => Ok(CandidateSelection::Changes(parse_change_ids(&changes)?)),
        (None, Some(stack_id)) => {
            let expected = options
                .required("expected-stack-version")?
                .parse::<i64>()
                .map_err(|_| {
                    CliError::usage("--expected-stack-version must be a non-negative integer")
                })?;
            Ok(CandidateSelection::Stack {
                stack_id: StackId::new(stack_id).map_err(CliError::from_input)?,
                expected_version: StackVersion::new(expected).map_err(CliError::from_input)?,
            })
        }
        (Some(_), Some(_)) => Err(CliError::usage(
            "candidate selection accepts exactly one of --changes or --stack-id",
        )),
        (None, None) => Err(CliError::usage(
            "candidate selection requires --changes or --stack-id",
        )),
    }
}

fn parse_change_ids(value: &str) -> Result<Vec<ChangeId>, CliError> {
    if value.is_empty() {
        return Err(CliError::usage(
            "--changes must contain at least one Change ID",
        ));
    }
    value
        .split(',')
        .map(|part| ChangeId::new(part).map_err(CliError::from_input))
        .collect()
}

fn materialization_version(options: &mut Options) -> Result<MaterializationVersion, CliError> {
    let value = options
        .required("expected-version")?
        .parse::<i64>()
        .map_err(|_| CliError::usage("--expected-version must be a non-negative integer"))?;
    MaterializationVersion::new(value).map_err(CliError::from_input)
}

fn parse_actors(value: &str) -> Result<Vec<ActorId>, CliError> {
    if value.is_empty() {
        return Err(CliError::usage(
            "--reviewers must contain at least one actor ID",
        ));
    }
    value
        .split(',')
        .map(|part| ActorId::new(part).map_err(CliError::from_input))
        .collect()
}

fn exact_target(state: &State, options: &mut Options) -> Result<ExactTarget, CliError> {
    match (
        options.optional("change-id"),
        options.optional("candidate-id"),
    ) {
        (Some(change), None) => {
            let revision = options.required("revision-id")?;
            state
                .store
                .revision_target(
                    &state.artifacts,
                    &ChangeId::new(change).map_err(CliError::from_input)?,
                    &RevisionId::new(revision).map_err(CliError::from_input)?,
                )
                .map_err(CliError::from)
        }
        (None, Some(candidate)) => state
            .store
            .candidate_target(
                &state.artifacts,
                &CandidateId::new(candidate).map_err(CliError::from_input)?,
            )
            .map_err(CliError::from),
        (Some(_), Some(_)) => Err(CliError::usage(
            "exact target accepts --change-id with --revision-id or --candidate-id, not both",
        )),
        (None, None) => Err(CliError::usage(
            "exact target requires --change-id with --revision-id or --candidate-id",
        )),
    }
}

fn validation_observation(options: &mut Options) -> Result<ValidationObservation, CliError> {
    let validation_type =
        ValidationType::new(options.required("validation-type")?).map_err(CliError::from_input)?;
    let environment = ValidationEnvironment::new(options.required("environment")?)
        .map_err(CliError::from_input)?;
    let outcome =
        ValidationOutcome::parse(&options.required("outcome")?).map_err(CliError::from_input)?;
    let execution = ValidationExecutionId::new(options.required("execution-id")?)
        .map_err(CliError::from_input)?;
    let scope = match options.required("scope")?.as_str() {
        "exact_target" => ValidationScope::ExactTarget,
        "declared_reusable" => ValidationScope::declared_reusable(
            options.required("reusable-scope")?,
            options.required("scope-rationale")?,
        )
        .map_err(CliError::from_input)?,
        value => {
            return Err(CliError::usage(format!(
                "invalid --scope `{value}`; expected exact_target or declared_reusable"
            )));
        }
    };
    Ok(ValidationObservation::new(
        validation_type,
        environment,
        outcome,
        execution,
        scope,
    ))
}

fn integration_version(options: &mut Options) -> Result<IntegrationVersion, CliError> {
    let value = options
        .required("expected-version")?
        .parse::<i64>()
        .map_err(|_| CliError::usage("--expected-version must be a non-negative integer"))?;
    IntegrationVersion::new(value).map_err(CliError::from_input)
}

fn target_observation(options: &mut Options) -> Result<TargetObservation, CliError> {
    Ok(TargetObservation::new(
        TargetRef::new(options.required("target-ref")?).map_err(CliError::from_input)?,
        TargetRevision::new(options.required("observed-target")?).map_err(CliError::from_input)?,
        IntegrationEvidence::new(options.required("observation-evidence")?)
            .map_err(CliError::from_input)?,
    ))
}

fn optional_authority(
    options: &mut Options,
) -> Result<Option<(ExecutionLeaseId, Subject)>, CliError> {
    let Some(lease_id) = options.optional("lease-id") else {
        return Ok(None);
    };
    Ok(Some((
        ExecutionLeaseId::new(lease_id).map_err(CliError::from_input)?,
        subject(options, "holder-kind", "holder-id")?,
    )))
}

fn parse_optional_review_ids(value: Option<String>) -> Result<Vec<ReviewSubmissionId>, CliError> {
    value.map_or_else(
        || Ok(Vec::new()),
        |list| {
            list.split(',')
                .map(|part| ReviewSubmissionId::new(part).map_err(CliError::from_input))
                .collect()
        },
    )
}

fn parse_optional_validation_ids(
    value: Option<String>,
) -> Result<Vec<ValidationResultId>, CliError> {
    value.map_or_else(
        || Ok(Vec::new()),
        |list| {
            list.split(',')
                .map(|part| ValidationResultId::new(part).map_err(CliError::from_input))
                .collect()
        },
    )
}

fn open_state(state_dir: &Path) -> Result<State, CliError> {
    let database = state_dir.join("metadata.sqlite3");
    let artifacts = state_dir.join("artifacts");
    if !state_dir.is_dir() || !database.is_file() || !artifacts.is_dir() {
        return Err(CliError::usage(format!(
            "Weft state is not initialized at {}; run `weft --state-dir {} init`",
            state_dir.display(),
            state_dir.display()
        )));
    }
    Ok(State {
        artifacts: ArtifactStore::open(artifacts)
            .map_err(|_| CliError::integrity("canonical artifact storage could not be opened"))?,
        store: SqliteStore::open(database)?,
    })
}

fn change_view(change: &Change) -> Value {
    let revisions = change
        .revisions()
        .iter()
        .map(|revision| {
            json!({
                "revision_id": revision.revision_id().as_str(),
                "parent_revision_id": revision.parent_revision_id().map(RevisionId::as_str),
                "repository_id": revision.base().repository_id().as_str(),
                "base_object": revision.base().object_id(),
                "artifact": {
                    "version": revision.artifact().version(),
                    "manifest_digest": revision.artifact().manifest_digest()
                },
                "created_at_unix_ms": revision.created_at().value(),
                "created_by": revision.created_by().as_str()
            })
        })
        .collect::<Vec<_>>();
    json!({
        "change_id": change.id().as_str(),
        "head_revision_id": change.head().map(RevisionId::as_str),
        "revisions": revisions
    })
}

fn assignment_view(assignment: &Assignment) -> Value {
    json!({
        "assignment_id": assignment.id().as_str(),
        "change_id": assignment.change_id().as_str(),
        "subject": {
            "kind": assignment.subject().kind().as_str(),
            "id": assignment.subject().id().as_str()
        },
        "role": assignment.role().as_str(),
        "assigned_at_unix_ms": assignment.assigned_at().value(),
        "assigned_by": assignment.assigned_by().as_str(),
        "version": assignment.version().value(),
        "released_at_unix_ms": assignment.released_at().map(UnixMillis::value),
        "released_by": assignment.released_by().map(ActorId::as_str),
        "active": assignment.is_active()
    })
}

fn lease_view(lease: &Lease) -> Value {
    json!({
        "lease_id": lease.id().as_str(),
        "scope": lease_scope_view(lease.scope()),
        "holder": {
            "kind": lease.holder().kind().as_str(),
            "id": lease.holder().id().as_str()
        },
        "predecessor_lease_id": lease.predecessor().map(LeaseId::as_str),
        "acquired_at_unix_ms": lease.acquired_at().value(),
        "expires_at_unix_ms": lease.expires_at().value(),
        "version": lease.version().value(),
        "released_at_unix_ms": lease.released_at().map(UnixMillis::value)
    })
}

fn lease_scope_view(scope: &LeaseScope) -> Value {
    json!({
        "change_id": scope.change_id().as_str(),
        "operation": scope.operation().as_str()
    })
}

fn relationship_view(relationship: &Relationship) -> Value {
    json!({
        "relationship_id": relationship.id().as_str(),
        "kind": relationship.kind().as_str(),
        "first_change_id": relationship.endpoints().first().as_str(),
        "second_change_id": relationship.endpoints().second().as_str(),
        "version": relationship.version().value(),
        "created_at_unix_ms": relationship.created_at().value(),
        "created_by": relationship.created_by().as_str(),
        "removed_at_unix_ms": relationship.removed_at().map(UnixMillis::value),
        "removed_by": relationship.removed_by().map(ActorId::as_str),
        "active": relationship.is_active()
    })
}

fn dependency_view(dependency: &Dependency, freshness: Option<DependencyFreshness>) -> Value {
    json!({
        "dependency_id": dependency.id().as_str(),
        "downstream_change_id": dependency.downstream_change_id().as_str(),
        "upstream_change_id": dependency.upstream_change_id().as_str(),
        "pins": {
            "downstream_revision_id": dependency.pins().downstream_revision_id().as_str(),
            "upstream_revision_id": dependency.pins().upstream_revision_id().as_str()
        },
        "version": dependency.version().value(),
        "created_at_unix_ms": dependency.created_at().value(),
        "created_by": dependency.created_by().as_str(),
        "updated_at_unix_ms": dependency.updated_at().value(),
        "updated_by": dependency.updated_by().as_str(),
        "removed_at_unix_ms": dependency.removed_at().map(UnixMillis::value),
        "removed_by": dependency.removed_by().map(ActorId::as_str),
        "active": dependency.is_active(),
        "freshness": freshness.map(dependency_freshness_name)
    })
}

const fn dependency_freshness_name(freshness: DependencyFreshness) -> &'static str {
    match freshness {
        DependencyFreshness::Current => "current",
        DependencyFreshness::DownstreamAdvanced => "downstream_advanced",
        DependencyFreshness::UpstreamAdvanced => "upstream_advanced",
        DependencyFreshness::BothAdvanced => "both_advanced",
        DependencyFreshness::Removed => "removed",
    }
}

fn stack_view(stack: &Stack) -> Value {
    json!({
        "stack_id": stack.id().as_str(),
        "policy": stack.definition().policy().as_str(),
        "members": stack.definition().members().iter().map(|member| json!({
            "change_id": member.change_id().as_str(),
            "predecessor_change_id": member.predecessor_change_id().map(ChangeId::as_str)
        })).collect::<Vec<_>>(),
        "version": stack.version().value(),
        "created_at_unix_ms": stack.created_at().value(),
        "created_by": stack.created_by().as_str(),
        "updated_at_unix_ms": stack.updated_at().value(),
        "updated_by": stack.updated_by().as_str()
    })
}

fn candidate_view(
    candidate: &CompositionCandidate,
    freshness: Option<&CandidateFreshness>,
) -> Value {
    let requirements = candidate
        .requirements()
        .iter()
        .map(|requirement| {
            let source = match requirement.source() {
                ResolvedRequirementSource::Dependency {
                    dependency_id,
                    version,
                } => json!({
                    "kind": "dependency", "dependency_id": dependency_id.as_str(),
                    "version": version.value()
                }),
                ResolvedRequirementSource::StackPredecessor {
                    stack_id,
                    version,
                    downstream_position,
                } => json!({
                    "kind": "stack_predecessor", "stack_id": stack_id.as_str(),
                    "version": version.value(), "downstream_position": downstream_position
                }),
            };
            json!({
                "source": source,
                "downstream": candidate_input_view(requirement.downstream()),
                "upstream": candidate_input_view(requirement.upstream())
            })
        })
        .collect::<Vec<_>>();
    json!({
        "candidate_id": candidate.id().as_str(),
        "target_base": {
            "repository_id": candidate.target_base().repository_id().as_str(),
            "object_id": candidate.target_base().object_id()
        },
        "stack": candidate.stack().map(|stack| json!({
            "stack_id": stack.stack_id().as_str(), "version": stack.version().value(),
            "policy": stack.policy().as_str()
        })),
        "inputs": candidate.inputs().iter().map(candidate_input_view).collect::<Vec<_>>(),
        "requirements": requirements,
        "content_digest": candidate.content_digest().as_str(),
        "created_at_unix_ms": candidate.created_at().value(),
        "created_by": candidate.created_by().as_str(),
        "freshness": freshness.map(candidate_freshness_view)
    })
}

fn candidate_input_view(input: &weft_domain::CandidateInput) -> Value {
    json!({
        "change_id": input.change_id().as_str(),
        "revision_id": input.revision_id().as_str()
    })
}

fn candidate_freshness_view(freshness: &CandidateFreshness) -> Value {
    json!({
        "current": freshness.is_current(),
        "advanced_inputs": freshness.advanced_inputs.iter().map(ChangeId::as_str).collect::<Vec<_>>(),
        "changed_dependencies": freshness.changed_dependencies.iter().map(DependencyId::as_str).collect::<Vec<_>>(),
        "stack_changed": freshness.stack_changed
    })
}

fn materialization_view(materialization: &Materialization) -> Value {
    json!({
        "materialization_id": materialization.id().as_str(),
        "change_id": materialization.change_id().as_str(),
        "revision_id": materialization.revision_id().as_str(),
        "workspace_id": materialization.workspace_id().as_str(),
        "provider_id": materialization.provider_id().as_str(),
        "provider_ref": materialization.provider_ref().as_str(),
        "state": materialization.state().as_str(),
        "version": materialization.version().value(),
        "created_at_unix_ms": materialization.created_at().value(),
        "created_by": materialization.created_by().as_str(),
        "state_changed_at_unix_ms": materialization.state_changed_at().value(),
        "released_at_unix_ms": materialization.released_at().map(UnixMillis::value)
    })
}

fn exact_target_view(target: &ExactTarget) -> Value {
    json!({
        "kind": target.kind(),
        "change_id": target.change_id().map(ChangeId::as_str),
        "revision_id": target.revision_id().map(RevisionId::as_str),
        "candidate_id": target.candidate_id().map(CandidateId::as_str),
        "repository_id": target.context().repository_id().as_str(),
        "context_object_id": target.context().object_id(),
        "content_digest": target.content_digest()
    })
}

fn target_freshness_view(freshness: &ExactTargetFreshness) -> Value {
    match freshness {
        ExactTargetFreshness::Current => json!({"status": "current", "current": true}),
        ExactTargetFreshness::RevisionAdvanced => {
            json!({"status": "revision_advanced", "current": false})
        }
        ExactTargetFreshness::CandidateStale(details) => json!({
            "status": "candidate_stale", "current": false,
            "candidate": candidate_freshness_view(details)
        }),
    }
}

fn review_request_view(request: &ReviewRequest, freshness: &ExactTargetFreshness) -> Value {
    json!({
        "review_request_id": request.id().as_str(),
        "target": exact_target_view(request.target()),
        "requested_by": request.requested_by().as_str(),
        "reviewers": request.reviewers().iter().map(ActorId::as_str).collect::<Vec<_>>(),
        "reuse_policy": request.reuse_policy().as_str(),
        "created_at_unix_ms": request.created_at().value(),
        "freshness": target_freshness_view(freshness)
    })
}

fn review_submission_view(submission: &ReviewSubmission) -> Value {
    json!({
        "review_submission_id": submission.id().as_str(),
        "review_request_id": submission.request_id().as_str(),
        "target": exact_target_view(submission.target()),
        "reviewer": submission.reviewer().as_str(),
        "outcome": submission.outcome().as_str(),
        "comments": submission.comments(),
        "submitted_at_unix_ms": submission.submitted_at().value()
    })
}

fn validation_result_view(result: &ValidationResult, freshness: &ExactTargetFreshness) -> Value {
    let (reusable_scope, scope_rationale) = result
        .scope()
        .declaration()
        .map_or((None, None), |(scope, rationale)| {
            (Some(scope), Some(rationale))
        });
    json!({
        "validation_result_id": result.id().as_str(),
        "target": exact_target_view(result.target()),
        "validation_type": result.validation_type().as_str(),
        "environment": result.environment().as_str(),
        "outcome": result.outcome().as_str(),
        "execution_id": result.execution_id().as_str(),
        "scope": result.scope().as_str(),
        "reusable_scope": reusable_scope,
        "scope_rationale": scope_rationale,
        "validated_by": result.validated_by().as_str(),
        "validated_at_unix_ms": result.validated_at().value(),
        "freshness": target_freshness_view(freshness)
    })
}

fn integration_success(
    command: &'static str,
    attempt: &IntegrationAttempt,
    action: &str,
) -> Success {
    Success {
        command,
        data: integration_view(attempt),
        human: format!("{action} IntegrationAttempt {}", attempt.id().as_str()),
    }
}

fn integration_view(attempt: &IntegrationAttempt) -> Value {
    let intent = attempt.intent();
    let target = intent.target();
    let method = intent.method();
    let gate = attempt.gate();
    json!({
        "integration_id": attempt.id().as_str(),
        "candidate_id": intent.binding().candidate_id().as_str(),
        "candidate_digest": intent.binding().candidate_digest(),
        "ordered_inputs": intent.binding().ordered_inputs().iter().map(candidate_input_view).collect::<Vec<_>>(),
        "target": {
            "repository_id": target.repository_id().as_str(),
            "target_ref": target.target_ref().as_str(),
            "expected_revision": target.expected_revision().as_str()
        },
        "method": {
            "provider_id": method.provider_id().as_str(),
            "strategy": method.strategy().as_str(),
            "effect_operation_id": method.effect_operation_id().as_str()
        },
        "gate": {
            "policy_evidence": gate.policy_evidence().as_str(),
            "capability_evidence": gate.capability_evidence().as_str(),
            "review_refs": gate.review_refs().iter().map(ReviewSubmissionId::as_str).collect::<Vec<_>>(),
            "validation_refs": gate.validation_refs().iter().map(ValidationResultId::as_str).collect::<Vec<_>>(),
            "target_observation": target_observation_view(gate.target_observation())
        },
        "state": attempt.state().as_str(),
        "version": attempt.version().value(),
        "created_at_unix_ms": attempt.created_at().value(),
        "created_by": attempt.created_by().as_str(),
        "updated_at_unix_ms": attempt.updated_at().value(),
        "updated_by": attempt.updated_by().as_str(),
        "started_at_unix_ms": attempt.started_at().map(UnixMillis::value),
        "finished_at_unix_ms": attempt.finished_at().map(UnixMillis::value),
        "result_revision": attempt.result_revision().map(TargetRevision::as_str),
        "lease": attempt.lease().map(|lease| json!({
            "lease_id": lease.id().as_str(),
            "holder": {"kind": lease.holder().kind().as_str(), "id": lease.holder().id().as_str()},
            "acquired_at_unix_ms": lease.acquired_at().value(),
            "expires_at_unix_ms": lease.expires_at().value(),
            "version": lease.version().value()
        })),
        "latest_reconciliation": attempt.latest_reconciliation().map(|value| json!({
            "reconciliation_id": value.id().as_str(),
            "outcome": value.outcome().as_str(),
            "target": target_observation_view(value.target()),
            "actor": value.actor().as_str(),
            "observed_at_unix_ms": value.observed_at().value()
        }))
    })
}

fn target_observation_view(observation: &TargetObservation) -> Value {
    json!({
        "target_ref": observation.target_ref().as_str(),
        "observed_revision": observation.observed_revision().as_str(),
        "evidence": observation.evidence().as_str()
    })
}

fn format_change(change: &Change) -> String {
    format!(
        "Change {}\nhead: {}\nrevisions: {}",
        change.id().as_str(),
        change.head().map_or("none", RevisionId::as_str),
        change.revisions().len()
    )
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
