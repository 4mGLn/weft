//! Stable, noninteractive local command-line interface for Weft.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use weft_domain::{ChangeId, ContentStore, SqliteRepository};

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
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
