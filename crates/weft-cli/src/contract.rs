use std::io::{self, Write};

use serde::Serialize;
use serde_json::Value;

use crate::error::CliError;

pub(crate) const SCHEMA: &str = "weft.cli.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    Human,
    Json,
}

impl Format {
    pub(crate) fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "human" => Ok(Self::Human),
            "json" => Ok(Self::Json),
            _ => Err(CliError::usage("--format must be `human` or `json`")),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Success {
    pub(crate) command: &'static str,
    pub(crate) data: Value,
    pub(crate) human: String,
}

#[derive(Serialize)]
struct SuccessEnvelope<'a> {
    schema: &'static str,
    ok: bool,
    command: &'a str,
    data: &'a Value,
}

#[derive(Serialize)]
struct FailureEnvelope<'a> {
    schema: &'static str,
    ok: bool,
    command: &'a str,
    error: ErrorView<'a>,
}

#[derive(Serialize)]
struct ErrorView<'a> {
    code: &'static str,
    message: &'a str,
    retryable: bool,
}

pub(crate) fn write_success(
    writer: &mut dyn Write,
    format: Format,
    success: &Success,
) -> io::Result<()> {
    match format {
        Format::Human => writeln!(writer, "{}", success.human),
        Format::Json => {
            serde_json::to_writer(
                &mut *writer,
                &SuccessEnvelope {
                    schema: SCHEMA,
                    ok: true,
                    command: success.command,
                    data: &success.data,
                },
            )?;
            writeln!(writer)
        }
    }
}

pub(crate) fn write_failure(
    writer: &mut dyn Write,
    format: Format,
    command: &str,
    error: &CliError,
) -> io::Result<()> {
    match format {
        Format::Human => writeln!(writer, "error[{}]: {}", error.code(), error.message()),
        Format::Json => {
            serde_json::to_writer(
                &mut *writer,
                &FailureEnvelope {
                    schema: SCHEMA,
                    ok: false,
                    command,
                    error: ErrorView {
                        code: error.code(),
                        message: error.message(),
                        retryable: error.retryable(),
                    },
                },
            )?;
            writeln!(writer)
        }
    }
}
