//! Stable, noninteractive local command-line interface for Weft.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use weft_domain::{
    Assignment, AssignmentId, AuditContext, CandidateId, CandidateInput, ChangeId,
    ChangeRelationKind, ConflictId, ContentStore, Dependency, IntegrationAttempt, IntegrationId,
    IntegrationReceiptId, MaterializationId, MaterializationState, OperationId, ReconciliationId,
    RepositoryId, ReviewOutcome, ReviewRequest, ReviewRequestId, ReviewSubmission,
    ReviewSubmissionId, RevisionId, SqliteRepository, StackId, Target, ValidationResult,
    ValidationResultId, ValidationStatus, WorkspaceId,
};
use weft_native_git::NativeGitRepository;

const SCHEMA_VERSION: u8 = 1;

fn main() -> ExitCode {
    match execute(env::args().skip(1).collect()) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "{{\"schemaVersion\":{SCHEMA_VERSION},\"error\":{{\"code\":\"{}\",\"message\":\"{}\"}}}}",
                error.code(),
                escape(error.message())
            );
            ExitCode::from(error.exit_code())
        }
    }
}

#[derive(Debug)]
struct CliError {
    code: &'static str,
    message: String,
    exit_code: u8,
}
impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: "usage",
            message: message.into(),
            exit_code: 2,
        }
    }
    fn domain(message: impl Into<String>) -> Self {
        Self {
            code: "domain",
            message: message.into(),
            exit_code: 3,
        }
    }
    fn code(&self) -> &'static str {
        self.code
    }
    fn message(&self) -> &str {
        &self.message
    }
    const fn exit_code(&self) -> u8 {
        self.exit_code
    }
}

#[allow(clippy::too_many_lines)]
fn execute(mut arguments: Vec<String>) -> Result<String, CliError> {
    let state = take_option(&mut arguments, "--state")
        .map_or_else(|| PathBuf::from(".weft"), PathBuf::from);
    let json = take_flag(&mut arguments, "--json");
    if !json {
        return Err(CliError::usage("--json is required in CLI schema v1"));
    }
    let store = ContentStore::open(state.join("content"))
        .map_err(|error| CliError::domain(error.to_string()))?;
    let mut repository = SqliteRepository::open(state.join("weft.sqlite"), store)
        .map_err(|error| CliError::domain(error.to_string()))?;
    if arguments.len() >= 3 && arguments[0] == "change" && arguments[1] == "revise" {
        let change = arguments.remove(2);
        arguments.drain(0..2);
        return revise(&mut repository, &change, &mut arguments);
    }
    if arguments.len() >= 3 && arguments[0] == "change" && arguments[1] == "assign" {
        let change = arguments.remove(2);
        arguments.drain(0..2);
        return assign(&mut repository, &change, &mut arguments);
    }
    if arguments.len() >= 3 && arguments[0] == "change" && arguments[1] == "acquire" {
        let change = arguments.remove(2);
        arguments.drain(0..2);
        return acquire(&mut repository, &change, &mut arguments);
    }
    if arguments.len() == 4 && arguments[0] == "dependency" && arguments[1] == "add" {
        return add_dependency(&mut repository, &arguments[2], &arguments[3]);
    }
    if arguments.len() >= 4 && arguments[0] == "stack" && arguments[1] == "create" {
        return create_stack(&mut repository, &arguments[2], &arguments[3..]);
    }
    if arguments.len() >= 5 && arguments[0] == "stack" && arguments[1] == "revise" {
        return revise_stack(
            &mut repository,
            &arguments[2],
            &arguments[3],
            &arguments[4..],
        );
    }
    if arguments.len() >= 4 && arguments[0] == "candidate" && arguments[1] == "create" {
        return create_candidate(&mut repository, &arguments[2], &arguments[3..]);
    }
    if arguments.len() >= 3 && arguments[0] == "materialization" && arguments[1] == "create" {
        let id = arguments.remove(2);
        arguments.drain(0..2);
        return create_materialization(&mut repository, &id, &mut arguments);
    }
    if arguments.len() >= 3 && arguments[0] == "materialization" && arguments[1] == "transition" {
        let id = arguments.remove(2);
        arguments.drain(0..2);
        return transition_materialization(&mut repository, &id, &mut arguments);
    }
    if arguments.len() >= 3 && arguments[0] == "validation" && arguments[1] == "record" {
        let id = arguments.remove(2);
        arguments.drain(0..2);
        return record_validation(&mut repository, &id, &mut arguments);
    }
    if arguments.len() >= 3 && arguments[0] == "review" && arguments[1] == "request" {
        let id = arguments.remove(2);
        arguments.drain(0..2);
        return request_review(&mut repository, &id, &mut arguments);
    }
    if arguments.len() >= 3 && arguments[0] == "review" && arguments[1] == "submit" {
        let id = arguments.remove(2);
        arguments.drain(0..2);
        return submit_review(&mut repository, &id, &mut arguments);
    }
    if arguments.len() >= 3 && arguments[0] == "integrate" && arguments[1] == "plan" {
        let id = arguments.remove(2);
        arguments.drain(0..2);
        return plan_integration(&mut repository, &id, &mut arguments);
    }
    if arguments.len() >= 3 && arguments[0] == "integrate" && arguments[1] == "run" {
        let id = arguments.remove(2);
        arguments.drain(0..2);
        return run_integration(&mut repository, &id, &mut arguments);
    }
    if arguments.len() >= 3 && arguments[0] == "reconcile" && arguments[1] == "integration" {
        let id = arguments.remove(2);
        arguments.drain(0..2);
        return reconcile_integration(&mut repository, &id, &mut arguments);
    }
    if arguments.len() == 2 && arguments[0] == "history" {
        return history(&repository, &arguments[1]);
    }
    if arguments.len() >= 4 && arguments[0] == "relation" && arguments[1] == "add" {
        return add_relation(
            &mut repository,
            &arguments[2],
            &arguments[3],
            &arguments[4..],
        );
    }
    match arguments.as_slice() {
        [command] if command == "status" => Ok(format!(
            "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"status\",\"stateDirectory\":\"{}\"}}",
            escape(&state.display().to_string())
        )),
        [group, command, id] if group == "change" && command == "create" => {
            let id = ChangeId::new(id).map_err(|error| CliError::usage(error.to_string()))?;
            repository
                .create_change(id.clone())
                .map_err(|error| CliError::domain(error.to_string()))?;
            Ok(format!(
                "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"change\",\"changeId\":\"{}\",\"headRevisionId\":null}}",
                escape(id.as_str())
            ))
        }
        [group, command, id] if group == "change" && command == "show" => {
            let id = ChangeId::new(id).map_err(|error| CliError::usage(error.to_string()))?;
            let change = repository
                .load_change(&id)
                .map_err(|error| CliError::domain(error.to_string()))?;
            let head = change.head().map_or("null".to_owned(), |head| {
                format!("\"{}\"", escape(head.as_str()))
            });
            Ok(format!(
                "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"change\",\"changeId\":\"{}\",\"headRevisionId\":{head}}}",
                escape(change.id().as_str())
            ))
        }
        _ => Err(CliError::usage(
            "expected `status`, `change create <id>`, or `change show <id>`",
        )),
    }
}

fn add_relation(
    repository: &mut SqliteRepository,
    source: &str,
    target: &str,
    arguments: &[String],
) -> Result<String, CliError> {
    let mut arguments = arguments.to_vec();
    let kind = required_option(&mut arguments, "--kind")?;
    let actor = required_option(&mut arguments, "--actor")?;
    let at = required_i64(&mut arguments, "--at")?;
    if !arguments.is_empty() {
        return Err(CliError::usage("unexpected relation add arguments"));
    }
    let kind = match kind.as_str() {
        "task-decomposition" => ChangeRelationKind::TaskDecomposition,
        "related-to" => ChangeRelationKind::RelatedTo,
        _ => {
            return Err(CliError::usage(
                "relation kind must be task-decomposition or related-to",
            ));
        }
    };
    let audit = AuditContext::new(actor, at).map_err(|error| CliError::usage(error.to_string()))?;
    let source = ChangeId::new(source).map_err(|error| CliError::usage(error.to_string()))?;
    let target = ChangeId::new(target).map_err(|error| CliError::usage(error.to_string()))?;
    repository
        .add_change_relation(&source, &target, kind, &audit)
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"relation\",\"sourceChangeId\":\"{}\",\"targetChangeId\":\"{}\"}}",
        escape(source.as_str()),
        escape(target.as_str())
    ))
}

fn reconcile_integration(
    repository: &mut SqliteRepository,
    id: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let path = required_option(arguments, "--repository")?;
    let expected = required_option(arguments, "--expected-result")?;
    let receipt = required_option(arguments, "--receipt-id")?;
    let reconciliation = required_option(arguments, "--reconciliation-id")?;
    let actor = required_option(arguments, "--actor")?;
    let at = required_i64(arguments, "--at")?;
    if !arguments.is_empty() {
        return Err(CliError::usage(
            "unexpected reconcile integration arguments",
        ));
    }
    let attempt = repository
        .load_integration_attempt(
            &IntegrationId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
        )
        .map_err(|error| CliError::domain(error.to_string()))?;
    if attempt.provider() != "native-git" {
        return Err(CliError::usage(
            "reconcile integration currently supports provider native-git only",
        ));
    }
    let provider =
        NativeGitRepository::discover(path).map_err(|error| CliError::domain(error.to_string()))?;
    let audit = AuditContext::new(actor, at).map_err(|error| CliError::usage(error.to_string()))?;
    let result = provider
        .reconcile_integration(
            repository,
            &attempt,
            &expected,
            IntegrationReceiptId::new(receipt)
                .map_err(|error| CliError::usage(error.to_string()))?,
            ReconciliationId::new(reconciliation)
                .map_err(|error| CliError::usage(error.to_string()))?,
            &audit,
        )
        .map_err(|error| CliError::domain(error.to_string()))?;
    match result {
        weft_native_git::NativeGitReconciliation::Confirmed { result_commit } => Ok(format!(
            "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"reconciliation\",\"integrationId\":\"{}\",\"resolved\":true,\"resultCommit\":\"{}\"}}",
            escape(id),
            escape(&result_commit)
        )),
        weft_native_git::NativeGitReconciliation::Diverged { observed_target } => Ok(format!(
            "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"reconciliation\",\"integrationId\":\"{}\",\"resolved\":false,\"observedTarget\":\"{}\"}}",
            escape(id),
            escape(&observed_target)
        )),
    }
}

fn history(repository: &SqliteRepository, change: &str) -> Result<String, CliError> {
    let change = ChangeId::new(change).map_err(|error| CliError::usage(error.to_string()))?;
    let events = repository
        .audit_events(&change)
        .map_err(|error| CliError::domain(error.to_string()))?;
    let events = events
        .iter()
        .map(|event| {
            format!(
                "{{\"eventId\":{},\"kind\":\"{}\",\"detail\":\"{}\"}}",
                event.event_id(),
                escape(event.kind()),
                escape(event.detail())
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"history\",\"changeId\":\"{}\",\"events\":[{events}]}}",
        escape(change.as_str())
    ))
}

fn plan_integration(
    repository: &mut SqliteRepository,
    id: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let candidate = required_option(arguments, "--candidate")?;
    let repository_id = required_option(arguments, "--repository-id")?;
    let target_ref = required_option(arguments, "--target-ref")?;
    let expected_target = required_option(arguments, "--expected-target")?;
    let provider = required_option(arguments, "--provider")?;
    let strategy = required_option(arguments, "--strategy")?;
    let operation = required_option(arguments, "--operation-id")?;
    let actor = required_option(arguments, "--actor")?;
    let at = required_i64(arguments, "--at")?;
    if !arguments.is_empty() {
        return Err(CliError::usage("unexpected integrate plan arguments"));
    }
    let attempt = IntegrationAttempt::new(
        IntegrationId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
        RepositoryId::new(repository_id).map_err(|error| CliError::usage(error.to_string()))?,
        CandidateId::new(candidate).map_err(|error| CliError::usage(error.to_string()))?,
        target_ref,
        expected_target,
        provider,
        strategy,
        OperationId::new(operation).map_err(|error| CliError::usage(error.to_string()))?,
        actor.clone(),
    )
    .map_err(|error| CliError::usage(error.to_string()))?;
    let audit = AuditContext::new(actor, at).map_err(|error| CliError::usage(error.to_string()))?;
    let attempt = repository
        .plan_integration(&attempt, &audit)
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"integration\",\"integrationId\":\"{}\",\"candidateId\":\"{}\",\"state\":\"planned\",\"operationId\":\"{}\"}}",
        escape(attempt.id().as_str()),
        escape(attempt.candidate_id().as_str()),
        escape(attempt.operation_id().as_str())
    ))
}

fn run_integration(
    repository: &mut SqliteRepository,
    id: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let path = required_option(arguments, "--repository")?;
    let destination = required_option(arguments, "--destination")?;
    let receipt = required_option(arguments, "--receipt-id")?;
    let conflict = required_option(arguments, "--conflict-id")?;
    let reconciliation = required_option(arguments, "--reconciliation-id")?;
    let now = required_i64(arguments, "--now")?;
    if !take_flag(arguments, "--yes") || !arguments.is_empty() {
        return Err(CliError::usage(
            "integrate run requires --yes and no unexpected arguments",
        ));
    }
    let attempt = repository
        .load_integration_attempt(
            &IntegrationId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
        )
        .map_err(|error| CliError::domain(error.to_string()))?;
    if attempt.provider() != "native-git" {
        return Err(CliError::usage(
            "integrate run currently supports provider native-git only",
        ));
    }
    let provider =
        NativeGitRepository::discover(path).map_err(|error| CliError::domain(error.to_string()))?;
    let audit = AuditContext::new(attempt.actor(), now)
        .map_err(|error| CliError::usage(error.to_string()))?;
    let receipt = provider
        .execute_integration(
            repository,
            &attempt,
            IntegrationReceiptId::new(receipt)
                .map_err(|error| CliError::usage(error.to_string()))?,
            ConflictId::new(conflict).map_err(|error| CliError::usage(error.to_string()))?,
            ReconciliationId::new(reconciliation)
                .map_err(|error| CliError::usage(error.to_string()))?,
            &audit,
            now,
            destination,
        )
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"integrationReceipt\",\"integrationId\":\"{}\",\"priorTarget\":\"{}\",\"resultCommit\":\"{}\"}}",
        escape(id),
        escape(receipt.prior_target()),
        escape(receipt.result_commit())
    ))
}

fn request_review(
    repository: &mut SqliteRepository,
    id: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let target = parse_target(&required_option(arguments, "--target")?)?;
    let requester = required_option(arguments, "--requester")?;
    let reviewers = required_option(arguments, "--reviewers")?;
    let at = required_i64(arguments, "--at")?;
    if !arguments.is_empty() {
        return Err(CliError::usage("unexpected review request arguments"));
    }
    let request = ReviewRequest::new(
        ReviewRequestId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
        target.clone(),
        requester,
        reviewers,
        at,
    )
    .map_err(|error| CliError::usage(error.to_string()))?;
    repository
        .create_review_request(&request)
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"reviewRequest\",\"reviewRequestId\":\"{}\",\"target\":\"{}\"}}",
        escape(id),
        escape(&target_name(&target))
    ))
}
fn submit_review(
    repository: &mut SqliteRepository,
    id: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let request = required_option(arguments, "--request")?;
    let reviewer = required_option(arguments, "--reviewer")?;
    let outcome = parse_review_outcome(&required_option(arguments, "--outcome")?)?;
    let comments = required_option(arguments, "--comments")?;
    let at = required_i64(arguments, "--at")?;
    if !arguments.is_empty() {
        return Err(CliError::usage("unexpected review submit arguments"));
    }
    let submission = ReviewSubmission::new(
        ReviewSubmissionId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
        ReviewRequestId::new(request).map_err(|error| CliError::usage(error.to_string()))?,
        reviewer,
        outcome,
        comments,
        at,
    )
    .map_err(|error| CliError::usage(error.to_string()))?;
    repository
        .submit_review(&submission)
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"reviewSubmission\",\"reviewSubmissionId\":\"{}\",\"outcome\":\"{}\"}}",
        escape(id),
        review_outcome_name(outcome)
    ))
}
fn parse_review_outcome(value: &str) -> Result<ReviewOutcome, CliError> {
    match value {
        "approved" => Ok(ReviewOutcome::Approved),
        "changes-requested" => Ok(ReviewOutcome::ChangesRequested),
        "rejected" => Ok(ReviewOutcome::Rejected),
        "blocked" => Ok(ReviewOutcome::Blocked),
        _ => Err(CliError::usage("invalid review outcome")),
    }
}
fn review_outcome_name(value: ReviewOutcome) -> &'static str {
    match value {
        ReviewOutcome::Approved => "approved",
        ReviewOutcome::ChangesRequested => "changes-requested",
        ReviewOutcome::Rejected => "rejected",
        ReviewOutcome::Blocked => "blocked",
    }
}

fn record_validation(
    repository: &mut SqliteRepository,
    id: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let target = parse_target(&required_option(arguments, "--target")?)?;
    let kind = required_option(arguments, "--kind")?;
    let environment = required_option(arguments, "--environment")?;
    let status = parse_validation_status(&required_option(arguments, "--status")?)?;
    let execution = required_option(arguments, "--execution")?;
    let at = required_i64(arguments, "--at")?;
    if !arguments.is_empty() {
        return Err(CliError::usage("unexpected validation record arguments"));
    }
    let result = ValidationResult::new(
        ValidationResultId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
        target.clone(),
        kind,
        environment,
        status,
        execution,
        at,
    )
    .map_err(|error| CliError::usage(error.to_string()))?;
    repository
        .record_validation(&result)
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"validation\",\"validationId\":\"{}\",\"target\":\"{}\",\"status\":\"{}\"}}",
        escape(id),
        escape(&target_name(&target)),
        validation_status_name(status)
    ))
}
fn parse_target(value: &str) -> Result<Target, CliError> {
    let (kind, id) = value
        .split_once(':')
        .ok_or_else(|| CliError::usage("target must be revision:<id> or candidate:<id>"))?;
    match kind {
        "revision" => Ok(Target::Revision(
            RevisionId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
        )),
        "candidate" => Ok(Target::Candidate(
            CandidateId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
        )),
        _ => Err(CliError::usage(
            "target must be revision:<id> or candidate:<id>",
        )),
    }
}
fn target_name(value: &Target) -> String {
    match value {
        Target::Revision(id) => format!("revision:{}", id.as_str()),
        Target::Candidate(id) => format!("candidate:{}", id.as_str()),
    }
}
fn parse_validation_status(value: &str) -> Result<ValidationStatus, CliError> {
    match value {
        "passed" => Ok(ValidationStatus::Passed),
        "failed" => Ok(ValidationStatus::Failed),
        "blocked" => Ok(ValidationStatus::Blocked),
        _ => Err(CliError::usage(
            "validation status must be passed, failed, or blocked",
        )),
    }
}
fn validation_status_name(value: ValidationStatus) -> &'static str {
    match value {
        ValidationStatus::Passed => "passed",
        ValidationStatus::Failed => "failed",
        ValidationStatus::Blocked => "blocked",
    }
}

fn create_materialization(
    repository: &mut SqliteRepository,
    id: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let revision = required_option(arguments, "--revision")?;
    let workspace = required_option(arguments, "--workspace")?;
    let provider = required_option(arguments, "--provider")?;
    let provider_ref = required_option(arguments, "--provider-ref")?;
    let actor = required_option(arguments, "--actor")?;
    let at = required_i64(arguments, "--at")?;
    if !arguments.is_empty() {
        return Err(CliError::usage(
            "unexpected materialization create arguments",
        ));
    }
    let audit = AuditContext::new(actor, at).map_err(|error| CliError::usage(error.to_string()))?;
    let materialization = repository
        .create_materialization(
            MaterializationId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
            RevisionId::new(revision).map_err(|error| CliError::usage(error.to_string()))?,
            WorkspaceId::new(workspace).map_err(|error| CliError::usage(error.to_string()))?,
            provider,
            provider_ref,
            &audit,
        )
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(materialization_json(&materialization))
}
fn transition_materialization(
    repository: &mut SqliteRepository,
    id: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let expected = parse_materialization_state(&required_option(arguments, "--expected-state")?)?;
    let next = parse_materialization_state(&required_option(arguments, "--next-state")?)?;
    let actor = required_option(arguments, "--actor")?;
    let at = required_i64(arguments, "--at")?;
    if !arguments.is_empty() {
        return Err(CliError::usage(
            "unexpected materialization transition arguments",
        ));
    }
    let audit = AuditContext::new(actor, at).map_err(|error| CliError::usage(error.to_string()))?;
    let materialization = repository
        .transition_materialization(
            &MaterializationId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
            expected,
            next,
            &audit,
        )
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(materialization_json(&materialization))
}
fn parse_materialization_state(value: &str) -> Result<MaterializationState, CliError> {
    match value {
        "clean" => Ok(MaterializationState::Clean),
        "dirty" => Ok(MaterializationState::Dirty),
        "diverged" => Ok(MaterializationState::Diverged),
        "suspended" => Ok(MaterializationState::Suspended),
        "released" => Ok(MaterializationState::Released),
        "invalidated" => Ok(MaterializationState::Invalidated),
        _ => Err(CliError::usage("invalid materialization state")),
    }
}
fn materialization_json(value: &weft_domain::Materialization) -> String {
    format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"materialization\",\"materializationId\":\"{}\",\"revisionId\":\"{}\",\"workspaceId\":\"{}\",\"provider\":\"{}\",\"providerRef\":\"{}\",\"state\":\"{}\"}}",
        escape(value.materialization_id().as_str()),
        escape(value.revision_id().as_str()),
        escape(value.workspace_id().as_str()),
        escape(value.provider()),
        escape(value.provider_ref()),
        materialization_state_name(value.state())
    )
}
fn materialization_state_name(value: MaterializationState) -> &'static str {
    match value {
        MaterializationState::Clean => "clean",
        MaterializationState::Dirty => "dirty",
        MaterializationState::Diverged => "diverged",
        MaterializationState::Suspended => "suspended",
        MaterializationState::Released => "released",
        MaterializationState::Invalidated => "invalidated",
    }
}

fn create_stack(
    repository: &mut SqliteRepository,
    id: &str,
    changes: &[String],
) -> Result<String, CliError> {
    let stack = repository
        .create_stack(
            StackId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
            parse_changes(changes)?,
        )
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(stack_json(&stack))
}
fn revise_stack(
    repository: &mut SqliteRepository,
    id: &str,
    expected: &str,
    changes: &[String],
) -> Result<String, CliError> {
    let expected = expected
        .parse()
        .map_err(|_| CliError::usage("stack version must be an integer"))?;
    let stack = repository
        .revise_stack(
            StackId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
            expected,
            parse_changes(changes)?,
        )
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(stack_json(&stack))
}
fn parse_changes(values: &[String]) -> Result<Vec<ChangeId>, CliError> {
    values
        .iter()
        .map(|value| ChangeId::new(value).map_err(|error| CliError::usage(error.to_string())))
        .collect()
}
fn stack_json(stack: &weft_domain::StackVersion) -> String {
    let changes = stack
        .changes()
        .iter()
        .map(|change| format!("\"{}\"", escape(change.as_str())))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"stack\",\"stackId\":\"{}\",\"version\":{},\"changeIds\":[{changes}]}}",
        escape(stack.stack_id().as_str()),
        stack.version()
    )
}
fn create_candidate(
    repository: &mut SqliteRepository,
    id: &str,
    inputs: &[String],
) -> Result<String, CliError> {
    let inputs = inputs
        .iter()
        .map(|value| parse_candidate_input(value))
        .collect::<Result<Vec<_>, _>>()?;
    let first = inputs
        .first()
        .ok_or_else(|| CliError::usage("candidate requires at least one change@revision input"))?;
    let base = repository
        .load_artifact_for_revision(first.revision_id())
        .map_err(|error| CliError::domain(error.to_string()))?
        .base()
        .clone();
    let candidate = repository
        .create_candidate(
            CandidateId::new(id).map_err(|error| CliError::usage(error.to_string()))?,
            base,
            inputs,
        )
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"candidate\",\"candidateId\":\"{}\",\"contentDigest\":\"{}\"}}",
        escape(candidate.candidate_id().as_str()),
        escape(candidate.content_digest())
    ))
}
fn parse_candidate_input(value: &str) -> Result<CandidateInput, CliError> {
    let (change, revision) = value
        .split_once('@')
        .ok_or_else(|| CliError::usage("candidate input must be <change-id>@<revision-id>"))?;
    Ok(CandidateInput::new(
        ChangeId::new(change).map_err(|error| CliError::usage(error.to_string()))?,
        RevisionId::new(revision).map_err(|error| CliError::usage(error.to_string()))?,
    ))
}

fn add_dependency(
    repository: &mut SqliteRepository,
    upstream: &str,
    downstream: &str,
) -> Result<String, CliError> {
    let (upstream_change, upstream_revision) = upstream
        .split_once('@')
        .ok_or_else(|| CliError::usage("upstream must be <change-id>@<revision-id>"))?;
    let dependency = Dependency::new(
        ChangeId::new(upstream_change).map_err(|error| CliError::usage(error.to_string()))?,
        RevisionId::new(upstream_revision).map_err(|error| CliError::usage(error.to_string()))?,
        ChangeId::new(downstream).map_err(|error| CliError::usage(error.to_string()))?,
    );
    repository
        .add_dependency(&dependency)
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"dependency\",\"upstreamChangeId\":\"{}\",\"upstreamRevisionId\":\"{}\",\"downstreamChangeId\":\"{}\"}}",
        escape(dependency.upstream_change_id().as_str()),
        escape(dependency.upstream_revision_id().as_str()),
        escape(dependency.downstream_change_id().as_str())
    ))
}

fn assign(
    repository: &mut SqliteRepository,
    change: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let assignment = required_option(arguments, "--assignment")?;
    let subject = required_option(arguments, "--subject")?;
    let role = required_option(arguments, "--role")?;
    let actor = required_option(arguments, "--actor")?;
    let at = required_i64(arguments, "--at")?;
    if !arguments.is_empty() {
        return Err(CliError::usage("unexpected change assign arguments"));
    }
    let change = ChangeId::new(change).map_err(|error| CliError::usage(error.to_string()))?;
    let assignment = Assignment::new(
        AssignmentId::new(assignment).map_err(|error| CliError::usage(error.to_string()))?,
        change.clone(),
        subject,
        role,
        actor,
        at,
    )
    .map_err(|error| CliError::usage(error.to_string()))?;
    repository
        .record_assignment(&assignment)
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"assignment\",\"assignmentId\":\"{}\",\"changeId\":\"{}\"}}",
        escape(assignment.assignment_id().as_str()),
        escape(change.as_str())
    ))
}

fn acquire(
    repository: &mut SqliteRepository,
    change: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let operation = required_option(arguments, "--operation")?;
    let holder = required_option(arguments, "--holder")?;
    let now = required_i64(arguments, "--now")?;
    let expires = required_i64(arguments, "--expires")?;
    if !arguments.is_empty() {
        return Err(CliError::usage("unexpected change acquire arguments"));
    }
    let change = ChangeId::new(change).map_err(|error| CliError::usage(error.to_string()))?;
    let lease = repository
        .acquire_lease(&change, operation, holder, now, expires)
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"lease\",\"changeId\":\"{}\",\"operation\":\"{}\",\"holder\":\"{}\",\"expiresAtUnixMs\":{}}}",
        escape(lease.change_id().as_str()),
        escape(lease.operation()),
        escape(lease.holder()),
        lease.expires_at_unix_ms()
    ))
}

fn revise(
    repository: &mut SqliteRepository,
    change: &str,
    arguments: &mut Vec<String>,
) -> Result<String, CliError> {
    let path = take_option(arguments, "--repository")
        .ok_or_else(|| CliError::usage("change revise requires --repository <path>"))?;
    let base = take_option(arguments, "--base")
        .ok_or_else(|| CliError::usage("change revise requires --base <commit>"))?;
    let revision = take_option(arguments, "--revision")
        .ok_or_else(|| CliError::usage("change revise requires --revision <revision-id>"))?;
    let expected = take_option(arguments, "--expected-head").ok_or_else(|| {
        CliError::usage("change revise requires --expected-head <revision-id|none>")
    })?;
    if !arguments.is_empty() {
        return Err(CliError::usage("unexpected change revise arguments"));
    }
    let change = ChangeId::new(change).map_err(|error| CliError::usage(error.to_string()))?;
    let revision = RevisionId::new(revision).map_err(|error| CliError::usage(error.to_string()))?;
    let expected = if expected == "none" {
        None
    } else {
        Some(RevisionId::new(expected).map_err(|error| CliError::usage(error.to_string()))?)
    };
    let provider =
        NativeGitRepository::discover(path).map_err(|error| CliError::domain(error.to_string()))?;
    let artifact = provider
        .capture_revision(&base, "HEAD", repository.content_store())
        .map_err(|error| CliError::domain(error.to_string()))?;
    repository
        .append_revision(&change, expected.as_ref(), revision.clone(), &artifact)
        .map_err(|error| CliError::domain(error.to_string()))?;
    Ok(format!(
        "{{\"schemaVersion\":{SCHEMA_VERSION},\"kind\":\"revision\",\"changeId\":\"{}\",\"revisionId\":\"{}\",\"artifactDigest\":\"{}\"}}",
        escape(change.as_str()),
        escape(revision.as_str()),
        escape(artifact.digest())
    ))
}

fn take_flag(arguments: &mut Vec<String>, name: &str) -> bool {
    arguments
        .iter()
        .position(|argument| argument == name)
        .is_some_and(|index| {
            arguments.remove(index);
            true
        })
}
fn take_option(arguments: &mut Vec<String>, name: &str) -> Option<String> {
    let index = arguments.iter().position(|argument| argument == name)?;
    if index + 1 == arguments.len() {
        return None;
    }
    arguments.remove(index);
    Some(arguments.remove(index))
}
fn required_option(arguments: &mut Vec<String>, name: &str) -> Result<String, CliError> {
    take_option(arguments, name).ok_or_else(|| CliError::usage(format!("missing {name}")))
}
fn required_i64(arguments: &mut Vec<String>, name: &str) -> Result<i64, CliError> {
    required_option(arguments, name)?
        .parse()
        .map_err(|_| CliError::usage(format!("{name} must be a signed 64-bit integer")))
}
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn state_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("weft-cli-contract-{nonce}"))
    }

    #[test]
    fn change_json_contract_persists_and_classifies_errors() {
        let state = state_directory();
        let state = state.to_str().unwrap().to_owned();
        let status = execute(vec![
            "--state".to_owned(),
            state.clone(),
            "status".to_owned(),
            "--json".to_owned(),
        ])
        .unwrap();
        assert_eq!(
            status,
            format!("{{\"schemaVersion\":1,\"kind\":\"status\",\"stateDirectory\":\"{state}\"}}")
        );
        let created = execute(vec![
            "--state".to_owned(),
            state.clone(),
            "change".to_owned(),
            "create".to_owned(),
            "change-1".to_owned(),
            "--json".to_owned(),
        ])
        .unwrap();
        assert_eq!(
            created,
            "{\"schemaVersion\":1,\"kind\":\"change\",\"changeId\":\"change-1\",\"headRevisionId\":null}"
        );
        let shown = execute(vec![
            "--state".to_owned(),
            state.clone(),
            "change".to_owned(),
            "show".to_owned(),
            "change-1".to_owned(),
            "--json".to_owned(),
        ])
        .unwrap();
        assert_eq!(shown, created);
        let missing = execute(vec![
            "--state".to_owned(),
            state.clone(),
            "change".to_owned(),
            "show".to_owned(),
            "missing".to_owned(),
            "--json".to_owned(),
        ])
        .unwrap_err();
        assert_eq!(missing.code(), "domain");
        assert_eq!(missing.exit_code(), 3);
        let usage = execute(vec!["status".to_owned()]).unwrap_err();
        assert_eq!(usage.code(), "usage");
        assert_eq!(usage.exit_code(), 2);
        std::fs::remove_dir_all(state).unwrap();
    }
}
