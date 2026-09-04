//! Stable noninteractive process boundary for local Weft workflows.

mod command;
mod contract;
mod error;
mod parser;

use std::ffi::OsString;
use std::io::Write;

use command::execute;
use contract::{Format, write_failure, write_success};
use error::CliError;
use parser::Invocation;

/// Executes one CLI invocation against caller-owned output streams.
///
/// This is the same boundary used by the `weft` binary and compatibility tests.
#[must_use]
pub fn run(arguments: Vec<OsString>, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let parsed = Invocation::parse(arguments);
    let format = parsed
        .as_ref()
        .map_or_else(|_| Format::Human, Invocation::format);
    match parsed.and_then(execute) {
        Ok(success) => {
            if write_success(stdout, format, &success).is_err() {
                let fallback = CliError::local("failed to write command output");
                let _ = write_failure(stderr, Format::Human, success.command, &fallback);
                fallback.exit_code()
            } else {
                0
            }
        }
        Err(failure) => {
            let target: &mut dyn Write = if failure.format == Format::Json {
                stdout
            } else {
                stderr
            };
            let _ = write_failure(target, failure.format, failure.command, &failure.error);
            failure.error.exit_code()
        }
    }
}

#[cfg(test)]
mod tests;
