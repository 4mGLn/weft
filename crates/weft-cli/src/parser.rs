use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::contract::Format;
use crate::error::CliError;

#[derive(Debug)]
pub(crate) struct Invocation {
    format: Format,
    state_dir: PathBuf,
    verbose: bool,
    command: Command,
}

impl Invocation {
    pub(crate) fn parse(arguments: Vec<OsString>) -> Result<Self, Failure> {
        parse(arguments).map_err(|error| Failure {
            format: error.format,
            command: error.command,
            error: error.error,
        })
    }

    pub(crate) const fn format(&self) -> Format {
        self.format
    }

    pub(crate) const fn command(&self) -> &Command {
        &self.command
    }

    pub(crate) const fn verbose(&self) -> bool {
        self.verbose
    }

    pub(crate) fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub(crate) fn into_parts(self) -> (Format, PathBuf, Command) {
        (self.format, self.state_dir, self.command)
    }
}

#[derive(Debug)]
pub(crate) enum Command {
    Help,
    Version,
    Init,
    Setup(Options),
    Doctor(Options),
    ChangeCreate(Options),
    ChangeShow(Options),
    ChangeHistory(Options),
    RevisionAppend(Options),
    AssignmentCreate(Options),
    AssignmentList(Options),
    AssignmentRelease(Options),
    LeaseAcquire(Options),
    LeaseShow(Options),
    LeaseRenew(Options),
    LeaseRelease(Options),
    RelationshipCreate(Options),
    RelationshipList(Options),
    RelationshipRemove(Options),
    DependencyCreate(Options),
    DependencyList(Options),
    DependencyRepin(Options),
    DependencyRemove(Options),
    StackCreate(Options),
    StackShow(Options),
    StackReplace(Options),
    CandidateCreate(Options),
    CandidateShow(Options),
    CandidateFreshness(Options),
    MaterializationCreate(Options),
    MaterializationShow(Options),
    MaterializationList(Options),
    MaterializationTransition(Options),
    ReviewRequest(Options),
    ReviewShow(Options),
    ReviewSubmit(Options),
    ReviewSubmissions(Options),
    ValidationRecord(Options),
    ValidationShow(Options),
    IntegrationPlan(Options),
    IntegrationShow(Options),
    IntegrationStart(Options),
    IntegrationRenew(Options),
    IntegrationUncertain(Options),
    IntegrationReconcile(Options),
    IntegrationConflict(Options),
    IntegrationSucceed(Options),
    IntegrationFinish(Options),
    IntegrationAbort(Options),
    IntegrationSupersede(Options),
    NativeGitDiscover(Options),
    NativeGitInspect(Options),
    NativeGitCapture(Options),
    NativeGitMaterialize(Options),
    NativeGitObserveMaterialization(Options),
    NativeGitReleaseMaterialization(Options),
    NativeGitExecuteIntegration(Options),
    NativeGitReconcileIntegration(Options),
    GitButlerDiscover(Options),
}

impl Command {
    pub(crate) const fn name(&self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::Version => "version",
            Self::Init => "init",
            Self::Setup(_) => "setup",
            Self::Doctor(_) => "doctor",
            Self::ChangeCreate(_) => "change.create",
            Self::ChangeShow(_) => "change.show",
            Self::ChangeHistory(_) => "change.history",
            Self::RevisionAppend(_) => "revision.append",
            Self::AssignmentCreate(_) => "assignment.create",
            Self::AssignmentList(_) => "assignment.list",
            Self::AssignmentRelease(_) => "assignment.release",
            Self::LeaseAcquire(_) => "lease.acquire",
            Self::LeaseShow(_) => "lease.show",
            Self::LeaseRenew(_) => "lease.renew",
            Self::LeaseRelease(_) => "lease.release",
            Self::RelationshipCreate(_) => "relationship.create",
            Self::RelationshipList(_) => "relationship.list",
            Self::RelationshipRemove(_) => "relationship.remove",
            Self::DependencyCreate(_) => "dependency.create",
            Self::DependencyList(_) => "dependency.list",
            Self::DependencyRepin(_) => "dependency.repin",
            Self::DependencyRemove(_) => "dependency.remove",
            Self::StackCreate(_) => "stack.create",
            Self::StackShow(_) => "stack.show",
            Self::StackReplace(_) => "stack.replace",
            Self::CandidateCreate(_) => "candidate.create",
            Self::CandidateShow(_) => "candidate.show",
            Self::CandidateFreshness(_) => "candidate.freshness",
            Self::MaterializationCreate(_) => "materialization.create",
            Self::MaterializationShow(_) => "materialization.show",
            Self::MaterializationList(_) => "materialization.list",
            Self::MaterializationTransition(_) => "materialization.transition",
            Self::ReviewRequest(_) => "review.request",
            Self::ReviewShow(_) => "review.show",
            Self::ReviewSubmit(_) => "review.submit",
            Self::ReviewSubmissions(_) => "review.submissions",
            Self::ValidationRecord(_) => "validation.record",
            Self::ValidationShow(_) => "validation.show",
            Self::IntegrationPlan(_) => "integration.plan",
            Self::IntegrationShow(_) => "integration.show",
            Self::IntegrationStart(_) => "integration.start",
            Self::IntegrationRenew(_) => "integration.renew",
            Self::IntegrationUncertain(_) => "integration.uncertain",
            Self::IntegrationReconcile(_) => "integration.reconcile",
            Self::IntegrationConflict(_) => "integration.conflict",
            Self::IntegrationSucceed(_) => "integration.succeed",
            Self::IntegrationFinish(_) => "integration.finish",
            Self::IntegrationAbort(_) => "integration.abort",
            Self::IntegrationSupersede(_) => "integration.supersede",
            Self::NativeGitDiscover(_) => "native-git.discover",
            Self::NativeGitInspect(_) => "native-git.inspect",
            Self::NativeGitCapture(_) => "native-git.capture",
            Self::NativeGitMaterialize(_) => "native-git.materialize",
            Self::NativeGitObserveMaterialization(_) => "native-git.observe-materialization",
            Self::NativeGitReleaseMaterialization(_) => "native-git.release-materialization",
            Self::NativeGitExecuteIntegration(_) => "native-git.execute-integration",
            Self::NativeGitReconcileIntegration(_) => "native-git.reconcile-integration",
            Self::GitButlerDiscover(_) => "gitbutler.discover",
        }
    }
}

#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) format: Format,
    pub(crate) command: &'static str,
    pub(crate) error: CliError,
}

#[derive(Debug)]
struct ParseError {
    format: Format,
    command: &'static str,
    error: CliError,
}

#[derive(Debug)]
pub(crate) struct Options {
    values: BTreeMap<String, String>,
    yes: bool,
}

impl Options {
    fn parse(values: &[String], command: &'static str, format: Format) -> Result<Self, ParseError> {
        let mut parsed = BTreeMap::new();
        let mut yes = false;
        let mut index = 0;
        while index < values.len() {
            let key = &values[index];
            if key == "--yes" {
                if yes {
                    return Err(parse_error(format, command, "duplicate option --yes"));
                }
                yes = true;
                index += 1;
                continue;
            }
            if !key.starts_with("--") {
                return Err(parse_error(
                    format,
                    command,
                    format!("unexpected positional argument `{key}`"),
                ));
            }
            let value = values.get(index + 1).ok_or_else(|| {
                parse_error(format, command, format!("option {key} requires a value"))
            })?;
            if value.starts_with("--") {
                return Err(parse_error(
                    format,
                    command,
                    format!("option {key} requires a value"),
                ));
            }
            let name = key.trim_start_matches("--").to_owned();
            if parsed.insert(name, value.clone()).is_some() {
                return Err(parse_error(
                    format,
                    command,
                    format!("duplicate option {key}"),
                ));
            }
            index += 2;
        }
        Ok(Self {
            values: parsed,
            yes,
        })
    }

    pub(crate) fn required(&mut self, name: &str) -> Result<String, CliError> {
        self.values
            .remove(name)
            .ok_or_else(|| CliError::usage(format!("missing required option --{name}")))
    }

    pub(crate) fn optional(&mut self, name: &str) -> Option<String> {
        self.values.remove(name)
    }

    pub(crate) fn finish(self) -> Result<(), CliError> {
        if let Some(name) = self.values.keys().next() {
            Err(CliError::usage(format!("unknown option --{name}")))
        } else if self.yes {
            Err(CliError::usage("--yes is not valid for this command"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn require_yes(&mut self) -> Result<(), CliError> {
        if !self.yes {
            return Err(CliError::usage(
                "this terminal transition requires explicit --yes",
            ));
        }
        self.yes = false;
        Ok(())
    }
}

fn parse(arguments: Vec<OsString>) -> Result<Invocation, ParseError> {
    let arguments = arguments
        .into_iter()
        .map(|value| {
            value
                .into_string()
                .map_err(|_| parse_error(Format::Human, "unknown", "arguments must be valid UTF-8"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut format = Format::Human;
    let mut state_dir = PathBuf::from(".weft");
    let mut verbose = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--format" => {
                let value = arguments.get(index + 1).ok_or_else(|| {
                    parse_error(format, "unknown", "option --format requires a value")
                })?;
                format = Format::parse(value).map_err(|error| ParseError {
                    format,
                    command: "unknown",
                    error,
                })?;
                index += 2;
            }
            "--state-dir" => {
                let value = arguments.get(index + 1).ok_or_else(|| {
                    parse_error(format, "unknown", "option --state-dir requires a value")
                })?;
                if value.is_empty() {
                    return Err(parse_error(
                        format,
                        "unknown",
                        "--state-dir cannot be empty",
                    ));
                }
                state_dir = PathBuf::from(value);
                index += 2;
            }
            "-v" | "--verbose" => {
                enable_verbose(&mut verbose, format)?;
                index += 1;
            }
            "--help" => {
                index += 1;
                if index != arguments.len() {
                    return Err(parse_error(format, "help", "--help accepts no arguments"));
                }
                return Ok(Invocation {
                    format,
                    state_dir,
                    verbose,
                    command: Command::Help,
                });
            }
            "-V" | "--version" => {
                index += 1;
                if index != arguments.len() {
                    return Err(parse_error(
                        format,
                        "version",
                        "--version accepts no arguments",
                    ));
                }
                return Ok(Invocation {
                    format,
                    state_dir,
                    verbose,
                    command: Command::Version,
                });
            }
            value if value.starts_with("--") => {
                return Err(parse_error(
                    format,
                    "unknown",
                    format!("unknown global option {value}"),
                ));
            }
            _ => break,
        }
    }
    if index == arguments.len() {
        return Ok(Invocation {
            format,
            state_dir,
            verbose,
            command: Command::Help,
        });
    }
    let command = parse_command(&arguments, index, format)?;
    Ok(Invocation {
        format,
        state_dir,
        verbose,
        command,
    })
}

fn enable_verbose(verbose: &mut bool, format: Format) -> Result<(), ParseError> {
    if *verbose {
        return Err(parse_error(
            format,
            "unknown",
            "duplicate global option --verbose",
        ));
    }
    *verbose = true;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn parse_command(
    arguments: &[String],
    index: usize,
    format: Format,
) -> Result<Command, ParseError> {
    let noun = &arguments[index];
    let verb = arguments.get(index + 1).map(String::as_str);
    Ok(match (noun.as_str(), verb) {
        ("init", None) => Command::Init,
        ("setup", _) => Command::Setup(Options::parse(&arguments[index + 1..], "setup", format)?),
        ("doctor", _) => {
            Command::Doctor(Options::parse(&arguments[index + 1..], "doctor", format)?)
        }
        ("change", Some("create")) => Command::ChangeCreate(Options::parse(
            &arguments[index + 2..],
            "change.create",
            format,
        )?),
        ("change", Some("show")) => Command::ChangeShow(Options::parse(
            &arguments[index + 2..],
            "change.show",
            format,
        )?),
        ("change", Some("history")) => Command::ChangeHistory(Options::parse(
            &arguments[index + 2..],
            "change.history",
            format,
        )?),
        ("revision", Some("append")) => Command::RevisionAppend(Options::parse(
            &arguments[index + 2..],
            "revision.append",
            format,
        )?),
        ("assignment", Some("create")) => Command::AssignmentCreate(Options::parse(
            &arguments[index + 2..],
            "assignment.create",
            format,
        )?),
        ("assignment", Some("list")) => Command::AssignmentList(Options::parse(
            &arguments[index + 2..],
            "assignment.list",
            format,
        )?),
        ("assignment", Some("release")) => Command::AssignmentRelease(Options::parse(
            &arguments[index + 2..],
            "assignment.release",
            format,
        )?),
        ("lease", Some("acquire")) => Command::LeaseAcquire(Options::parse(
            &arguments[index + 2..],
            "lease.acquire",
            format,
        )?),
        ("lease", Some("show")) => Command::LeaseShow(Options::parse(
            &arguments[index + 2..],
            "lease.show",
            format,
        )?),
        ("lease", Some("renew")) => Command::LeaseRenew(Options::parse(
            &arguments[index + 2..],
            "lease.renew",
            format,
        )?),
        ("lease", Some("release")) => Command::LeaseRelease(Options::parse(
            &arguments[index + 2..],
            "lease.release",
            format,
        )?),
        ("relationship", Some("create")) => Command::RelationshipCreate(Options::parse(
            &arguments[index + 2..],
            "relationship.create",
            format,
        )?),
        ("relationship", Some("list")) => Command::RelationshipList(Options::parse(
            &arguments[index + 2..],
            "relationship.list",
            format,
        )?),
        ("relationship", Some("remove")) => Command::RelationshipRemove(Options::parse(
            &arguments[index + 2..],
            "relationship.remove",
            format,
        )?),
        ("dependency", Some("create")) => Command::DependencyCreate(Options::parse(
            &arguments[index + 2..],
            "dependency.create",
            format,
        )?),
        ("dependency", Some("list")) => Command::DependencyList(Options::parse(
            &arguments[index + 2..],
            "dependency.list",
            format,
        )?),
        ("dependency", Some("repin")) => Command::DependencyRepin(Options::parse(
            &arguments[index + 2..],
            "dependency.repin",
            format,
        )?),
        ("dependency", Some("remove")) => Command::DependencyRemove(Options::parse(
            &arguments[index + 2..],
            "dependency.remove",
            format,
        )?),
        ("stack", Some("create")) => Command::StackCreate(Options::parse(
            &arguments[index + 2..],
            "stack.create",
            format,
        )?),
        ("stack", Some("show")) => Command::StackShow(Options::parse(
            &arguments[index + 2..],
            "stack.show",
            format,
        )?),
        ("stack", Some("replace")) => Command::StackReplace(Options::parse(
            &arguments[index + 2..],
            "stack.replace",
            format,
        )?),
        ("candidate", Some("create")) => Command::CandidateCreate(Options::parse(
            &arguments[index + 2..],
            "candidate.create",
            format,
        )?),
        ("candidate", Some("show")) => Command::CandidateShow(Options::parse(
            &arguments[index + 2..],
            "candidate.show",
            format,
        )?),
        ("candidate", Some("freshness")) => Command::CandidateFreshness(Options::parse(
            &arguments[index + 2..],
            "candidate.freshness",
            format,
        )?),
        ("materialization", Some("create")) => Command::MaterializationCreate(Options::parse(
            &arguments[index + 2..],
            "materialization.create",
            format,
        )?),
        ("materialization", Some("show")) => Command::MaterializationShow(Options::parse(
            &arguments[index + 2..],
            "materialization.show",
            format,
        )?),
        ("materialization", Some("list")) => Command::MaterializationList(Options::parse(
            &arguments[index + 2..],
            "materialization.list",
            format,
        )?),
        ("materialization", Some("transition")) => {
            Command::MaterializationTransition(Options::parse(
                &arguments[index + 2..],
                "materialization.transition",
                format,
            )?)
        }
        ("review", Some("request")) => Command::ReviewRequest(Options::parse(
            &arguments[index + 2..],
            "review.request",
            format,
        )?),
        ("review", Some("show")) => Command::ReviewShow(Options::parse(
            &arguments[index + 2..],
            "review.show",
            format,
        )?),
        ("review", Some("submit")) => Command::ReviewSubmit(Options::parse(
            &arguments[index + 2..],
            "review.submit",
            format,
        )?),
        ("review", Some("submissions")) => Command::ReviewSubmissions(Options::parse(
            &arguments[index + 2..],
            "review.submissions",
            format,
        )?),
        ("validation", Some("record")) => Command::ValidationRecord(Options::parse(
            &arguments[index + 2..],
            "validation.record",
            format,
        )?),
        ("validation", Some("show")) => Command::ValidationShow(Options::parse(
            &arguments[index + 2..],
            "validation.show",
            format,
        )?),
        ("integration", Some("plan")) => Command::IntegrationPlan(Options::parse(
            &arguments[index + 2..],
            "integration.plan",
            format,
        )?),
        ("integration", Some("show")) => Command::IntegrationShow(Options::parse(
            &arguments[index + 2..],
            "integration.show",
            format,
        )?),
        ("integration", Some("start")) => Command::IntegrationStart(Options::parse(
            &arguments[index + 2..],
            "integration.start",
            format,
        )?),
        ("integration", Some("renew")) => Command::IntegrationRenew(Options::parse(
            &arguments[index + 2..],
            "integration.renew",
            format,
        )?),
        ("integration", Some("uncertain")) => Command::IntegrationUncertain(Options::parse(
            &arguments[index + 2..],
            "integration.uncertain",
            format,
        )?),
        ("integration", Some("reconcile")) => Command::IntegrationReconcile(Options::parse(
            &arguments[index + 2..],
            "integration.reconcile",
            format,
        )?),
        ("integration", Some("conflict")) => Command::IntegrationConflict(Options::parse(
            &arguments[index + 2..],
            "integration.conflict",
            format,
        )?),
        ("integration", Some("succeed")) => Command::IntegrationSucceed(Options::parse(
            &arguments[index + 2..],
            "integration.succeed",
            format,
        )?),
        ("integration", Some("finish")) => Command::IntegrationFinish(Options::parse(
            &arguments[index + 2..],
            "integration.finish",
            format,
        )?),
        ("integration", Some("abort")) => Command::IntegrationAbort(Options::parse(
            &arguments[index + 2..],
            "integration.abort",
            format,
        )?),
        ("integration", Some("supersede")) => Command::IntegrationSupersede(Options::parse(
            &arguments[index + 2..],
            "integration.supersede",
            format,
        )?),
        ("native-git", Some("discover")) => Command::NativeGitDiscover(Options::parse(
            &arguments[index + 2..],
            "native-git.discover",
            format,
        )?),
        ("native-git", Some("inspect")) => Command::NativeGitInspect(Options::parse(
            &arguments[index + 2..],
            "native-git.inspect",
            format,
        )?),
        ("native-git", Some("capture")) => Command::NativeGitCapture(Options::parse(
            &arguments[index + 2..],
            "native-git.capture",
            format,
        )?),
        ("native-git", Some("materialize")) => Command::NativeGitMaterialize(Options::parse(
            &arguments[index + 2..],
            "native-git.materialize",
            format,
        )?),
        ("native-git", Some("observe-materialization")) => {
            Command::NativeGitObserveMaterialization(Options::parse(
                &arguments[index + 2..],
                "native-git.observe-materialization",
                format,
            )?)
        }
        ("native-git", Some("release-materialization")) => {
            Command::NativeGitReleaseMaterialization(Options::parse(
                &arguments[index + 2..],
                "native-git.release-materialization",
                format,
            )?)
        }
        ("native-git", Some("execute-integration")) => {
            Command::NativeGitExecuteIntegration(Options::parse(
                &arguments[index + 2..],
                "native-git.execute-integration",
                format,
            )?)
        }
        ("native-git", Some("reconcile-integration")) => {
            Command::NativeGitReconcileIntegration(Options::parse(
                &arguments[index + 2..],
                "native-git.reconcile-integration",
                format,
            )?)
        }
        ("gitbutler", Some("discover")) => Command::GitButlerDiscover(Options::parse(
            &arguments[index + 2..],
            "gitbutler.discover",
            format,
        )?),
        ("init", Some(_)) => {
            return Err(parse_error(format, "init", "init accepts no subcommand"));
        }
        _ => {
            return Err(parse_error(
                format,
                "unknown",
                format!("unknown command `{}`", arguments[index..].join(" ")),
            ));
        }
    })
}

fn parse_error(format: Format, command: &'static str, message: impl Into<String>) -> ParseError {
    ParseError {
        format,
        command,
        error: CliError::usage(message),
    }
}
