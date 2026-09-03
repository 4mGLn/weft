//! Stable, noninteractive local command-line interface for Weft.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use weft_domain::{Assignment, AssignmentId, ChangeId, ContentStore, RevisionId, SqliteRepository};
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
