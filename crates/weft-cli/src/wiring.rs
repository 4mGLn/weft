use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::CliError;

const BRIDGE_FILE: &str = "runtime-bridge.json";
const BRIDGE_SCHEMA: &str = "weft.runtime-bridge.v1";
const MANAGED_START: &str = "<!-- weft:runtime-wiring:start -->";
const MANAGED_END: &str = "<!-- weft:runtime-wiring:end -->";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Runtime {
    Codex,
    ClaudeCode,
    GeminiCli,
    Paseo,
    Omc,
    Omg,
    Omx,
}

impl Runtime {
    const ALL: [Self; 7] = [
        Self::Codex,
        Self::ClaudeCode,
        Self::GeminiCli,
        Self::Paseo,
        Self::Omc,
        Self::Omg,
        Self::Omx,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
            Self::GeminiCli => "gemini-cli",
            Self::Paseo => "paseo",
            Self::Omc => "omc",
            Self::Omg => "omg",
            Self::Omx => "omx",
        }
    }

    const fn executable(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude",
            Self::GeminiCli => "gemini",
            Self::Paseo => "paseo",
            Self::Omc => "omc",
            Self::Omg => "omg",
            Self::Omx => "omx",
        }
    }

    const fn instruction_file(self) -> Option<&'static str> {
        match self {
            Self::Codex => Some("AGENTS.md"),
            Self::ClaudeCode => Some("CLAUDE.md"),
            Self::GeminiCli => Some("GEMINI.md"),
            Self::Paseo | Self::Omc | Self::Omg | Self::Omx => None,
        }
    }

    const fn integration(self) -> &'static str {
        match self.instruction_file() {
            Some(_) => "project-instructions",
            None => "runtime-bridge",
        }
    }

    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "codex" => Ok(Self::Codex),
            "claude-code" => Ok(Self::ClaudeCode),
            "gemini-cli" => Ok(Self::GeminiCli),
            "paseo" => Ok(Self::Paseo),
            "omc" => Ok(Self::Omc),
            "omg" => Ok(Self::Omg),
            "omx" => Ok(Self::Omx),
            _ => Err(CliError::usage(format!(
                "--runtime accepts auto, all, codex, claude-code, gemini-cli, paseo, omc, omg, or omx; found `{value}`"
            ))),
        }
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RuntimeBridge {
    schema: String,
    project_dir: String,
    state_dir: String,
    protocol: String,
    runtimes: Vec<RuntimeEntry>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RuntimeEntry {
    name: String,
    executable: String,
    detected: bool,
    integration: String,
    instruction_file: Option<String>,
}

pub(crate) fn setup(
    state_dir: &Path,
    project_dir: &Path,
    runtime_spec: &str,
) -> Result<Value, CliError> {
    let project_dir = canonical_directory(project_dir, "--project-dir")?;
    let state_dir = canonical_directory(state_dir, "--state-dir")?;
    let runtimes = select_runtimes(runtime_spec)?;
    let path_entries = path_entries();
    let bridge = RuntimeBridge {
        schema: BRIDGE_SCHEMA.to_owned(),
        project_dir: display_path(&project_dir),
        state_dir: display_path(&state_dir),
        protocol: "weft.cli.v1".to_owned(),
        runtimes: runtimes
            .iter()
            .map(|runtime| RuntimeEntry {
                name: runtime.name().to_owned(),
                executable: runtime.executable().to_owned(),
                detected: executable_in_paths(runtime.executable(), &path_entries),
                integration: runtime.integration().to_owned(),
                instruction_file: runtime.instruction_file().map(ToOwned::to_owned),
            })
            .collect(),
    };
    let writes = instruction_writes(&project_dir, &runtimes)?;
    for (path, content) in writes {
        write_atomically(&path, &content)?;
    }
    let bridge_bytes = serde_json::to_vec_pretty(&bridge)
        .map_err(|_| CliError::local("failed to encode runtime bridge"))?;
    let state_bridge_path = state_dir.join(BRIDGE_FILE);
    write_atomically_bytes(&state_bridge_path, &bridge_bytes)?;
    let project_bridge_path = project_dir.join(".weft").join(BRIDGE_FILE);
    if project_bridge_path != state_bridge_path {
        write_atomically_bytes(&project_bridge_path, &bridge_bytes)?;
    }
    Ok(bridge_view(&bridge, true, &[]))
}

pub(crate) fn preflight(project_dir: &Path, runtime_spec: &str) -> Result<(), CliError> {
    let project_dir = canonical_directory(project_dir, "--project-dir")?;
    let runtimes = select_runtimes(runtime_spec)?;
    let _ = instruction_writes(&project_dir, &runtimes)?;
    Ok(())
}

pub(crate) fn doctor(state_dir: &Path, project_dir: &Path) -> Result<Value, CliError> {
    let project_dir = canonical_directory(project_dir, "--project-dir")?;
    let initialized = state_dir.is_dir()
        && sqlite_database(&state_dir.join("metadata.sqlite3"))
        && state_dir.join("artifacts").is_dir();
    let bridge_path = state_dir.join(BRIDGE_FILE);
    let mut problems = Vec::new();
    let bridge = if bridge_path.is_file() {
        match fs::read(&bridge_path)
            .map_err(|_| CliError::local("failed to read runtime bridge"))
            .and_then(|bytes| {
                serde_json::from_slice::<RuntimeBridge>(&bytes)
                    .map_err(|_| CliError::integrity("runtime bridge is not valid JSON"))
            }) {
            Ok(bridge) if bridge.schema == BRIDGE_SCHEMA => Some(bridge),
            Ok(_) => {
                problems.push("runtime bridge has an unsupported schema".to_owned());
                None
            }
            Err(error) => {
                problems.push(error.message().to_owned());
                None
            }
        }
    } else {
        problems.push("runtime bridge is missing; run `weft setup`".to_owned());
        None
    };
    if !initialized {
        problems.push("Weft state is not initialized".to_owned());
    }
    if let Some(bridge) = &bridge {
        project_bridge_problems(&project_dir, bridge, &mut problems);
    }
    let mut value = bridge.map_or_else(
        || {
            json!({
                "schema": BRIDGE_SCHEMA,
                "project_dir": display_path(&project_dir),
                "state_dir": display_path(state_dir),
                "protocol": "weft.cli.v1",
                "runtimes": []
            })
        },
        |bridge| {
            runtime_instruction_problems(&project_dir, &bridge, &mut problems);
            bridge_view(&bridge, initialized, &[])
        },
    );
    value["healthy"] = Value::Bool(initialized && problems.is_empty());
    value["problems"] = json!(problems);
    Ok(value)
}

fn select_runtimes(value: &str) -> Result<Vec<Runtime>, CliError> {
    match value {
        "auto" => {
            let paths = path_entries();
            Ok(Runtime::ALL
                .into_iter()
                .filter(|runtime| executable_in_paths(runtime.executable(), &paths))
                .collect())
        }
        "all" => Ok(Runtime::ALL.into()),
        _ => {
            let mut runtimes = Vec::new();
            for value in value.split(',') {
                if value.is_empty() {
                    return Err(CliError::usage("--runtime must not contain an empty entry"));
                }
                let runtime = Runtime::parse(value)?;
                if runtimes.contains(&runtime) {
                    return Err(CliError::usage(format!("duplicate runtime `{value}`")));
                }
                runtimes.push(runtime);
            }
            Ok(runtimes)
        }
    }
}

fn instruction_writes(
    project_dir: &Path,
    runtimes: &[Runtime],
) -> Result<Vec<(PathBuf, String)>, CliError> {
    let mut writes = Vec::new();
    for instruction_file in runtimes
        .iter()
        .filter_map(|runtime| runtime.instruction_file())
    {
        let path = project_dir.join(instruction_file);
        let existing = if path.exists() {
            fs::read_to_string(&path)
                .map_err(|_| CliError::local(format!("failed to read {}", path.display())))?
        } else {
            String::new()
        };
        let content = managed_instruction(&existing)?;
        if content != existing {
            writes.push((path, content));
        }
    }
    Ok(writes)
}

fn managed_instruction(existing: &str) -> Result<String, CliError> {
    let start_positions = positions(existing, MANAGED_START);
    let end_positions = positions(existing, MANAGED_END);
    match (start_positions.as_slice(), end_positions.as_slice()) {
        ([], []) => Ok(format_instruction_append(existing)),
        ([start], [end]) if start < end => {
            let end = end + MANAGED_END.len();
            Ok(format!(
                "{}{}{}",
                &existing[..*start],
                managed_block(),
                &existing[end..]
            ))
        }
        _ => Err(CliError::integrity(
            "Weft runtime-wiring markers are malformed; repair them before running setup",
        )),
    }
}

fn positions(text: &str, needle: &str) -> Vec<usize> {
    text.match_indices(needle).map(|(index, _)| index).collect()
}

fn format_instruction_append(existing: &str) -> String {
    if existing.is_empty() {
        format!("{}\n", managed_block())
    } else if existing.ends_with('\n') {
        format!("{existing}\n{}\n", managed_block())
    } else {
        format!("{existing}\n\n{}\n", managed_block())
    }
}

fn managed_block() -> String {
    format!(
        "{MANAGED_START}\n\
# Weft agent coordination\n\
This project uses Weft as its shared durable coordination layer for concurrent\n\
agents. Use `weft --format json` for coordination operations; a Change, exact\n\
revision, Assignment, and Lease are durable authority, while a prompt, session,\n\
branch, or workspace is replaceable evidence.\n\n\
Before mutating shared work, inspect Weft state and acquire the required durable\n\
Assignment/Lease. Checkpoint canonical progress before handoff or session exit.\n\
Never infer success from a runtime or provider result alone; record/reconcile\n\
uncertain provider mutations through Weft. The external runtime or orchestrator\n\
still launches and supervises agents—Weft does not schedule them.\n\n\
Read `.weft/runtime-bridge.json` to find the configured `state_dir`, then pass it\n\
to Weft as `--state-dir`. Run `weft --state-dir <configured-state-dir> doctor`\n\
to inspect local wiring.\n\
{MANAGED_END}"
    )
}

fn runtime_instruction_problems(
    project_dir: &Path,
    bridge: &RuntimeBridge,
    problems: &mut Vec<String>,
) {
    let paths = path_entries();
    for runtime in &bridge.runtimes {
        if !executable_in_paths(&runtime.executable, &paths) {
            problems.push(format!(
                "{} is unavailable on PATH for runtime {}",
                runtime.executable, runtime.name
            ));
        }
        if let Some(file) = &runtime.instruction_file {
            let path = project_dir.join(file);
            match fs::read_to_string(&path) {
                Ok(content)
                    if managed_instruction(&content).is_ok() && content.contains(MANAGED_START) => {
                }
                Ok(_) => problems.push(format!(
                    "{} is missing a valid Weft managed block",
                    path.display()
                )),
                Err(_) => problems.push(format!("{} is unavailable", path.display())),
            }
        }
    }
}

fn project_bridge_problems(project_dir: &Path, bridge: &RuntimeBridge, problems: &mut Vec<String>) {
    let path = project_dir.join(".weft").join(BRIDGE_FILE);
    match fs::read(&path)
        .map_err(|_| CliError::local("failed to read project runtime bridge"))
        .and_then(|bytes| {
            serde_json::from_slice::<RuntimeBridge>(&bytes)
                .map_err(|_| CliError::integrity("project runtime bridge is not valid JSON"))
        }) {
        Ok(project_bridge) if project_bridge == *bridge => {}
        Ok(_) => problems.push(format!(
            "{} does not match the configured runtime bridge",
            path.display()
        )),
        Err(_) if !path.is_file() => {
            problems.push(format!("{} is missing; run `weft setup`", path.display()));
        }
        Err(error) => problems.push(format!("{}: {}", path.display(), error.message())),
    }
}

fn bridge_view(bridge: &RuntimeBridge, initialized: bool, problems: &[String]) -> Value {
    json!({
        "bridge_schema": bridge.schema,
        "project_dir": bridge.project_dir,
        "state_dir": bridge.state_dir,
        "protocol": bridge.protocol,
        "initialized": initialized,
        "runtimes": bridge.runtimes.iter().map(|runtime| json!({
            "name": runtime.name,
            "executable": runtime.executable,
            "detected": runtime.detected,
            "integration": runtime.integration,
            "instruction_file": runtime.instruction_file
        })).collect::<Vec<_>>(),
        "problems": problems
    })
}

fn canonical_directory(path: &Path, option: &str) -> Result<PathBuf, CliError> {
    if !path.is_dir() {
        return Err(CliError::usage(format!(
            "{option} must name an existing directory"
        )));
    }
    path.canonicalize()
        .map_err(|_| CliError::local(format!("failed to resolve {option}")))
}

fn path_entries() -> Vec<PathBuf> {
    env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default()
}

fn executable_in_paths(executable: &str, paths: &[PathBuf]) -> bool {
    paths.iter().any(|path| {
        let candidate = path.join(format!("{executable}{}", env::consts::EXE_SUFFIX));
        candidate.is_file() && executable_file(&candidate)
    })
}

#[cfg(unix)]
fn executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable_file(_path: &Path) -> bool {
    true
}

fn sqlite_database(path: &Path) -> bool {
    let mut header = [0_u8; 16];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok_and(|()| header == *b"SQLite format 3\0")
}

fn write_atomically(path: &Path, content: &str) -> Result<(), CliError> {
    write_atomically_bytes(path, content.as_bytes())
}

fn write_atomically_bytes(path: &Path, content: &[u8]) -> Result<(), CliError> {
    let parent = path
        .parent()
        .ok_or_else(|| CliError::local("runtime wiring path has no parent directory"))?;
    fs::create_dir_all(parent)
        .map_err(|_| CliError::local(format!("failed to create {}", parent.display())))?;
    let name = path
        .file_name()
        .ok_or_else(|| CliError::local("runtime wiring path has no file name"))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.weft-tmp-{}", std::process::id()));
    fs::write(&temporary, content)
        .map_err(|_| CliError::local(format!("failed to write {}", temporary.display())))?;
    fs::rename(&temporary, path)
        .map_err(|_| CliError::local(format!("failed to publish {}", path.display())))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{
        BRIDGE_FILE, BRIDGE_SCHEMA, Runtime, RuntimeBridge, RuntimeEntry, doctor,
        executable_in_paths, managed_instruction, select_runtimes,
    };

    #[test]
    fn managed_instruction_preserves_user_content_and_is_idempotent() {
        let first = managed_instruction("# Project rules\n").unwrap();
        assert!(first.starts_with("# Project rules\n\n"));
        assert_eq!(managed_instruction(&first).unwrap(), first);
    }

    #[test]
    fn managed_instruction_rejects_unpaired_markers() {
        let error = managed_instruction("<!-- weft:runtime-wiring:start -->").unwrap_err();
        assert_eq!(error.code(), "integrity");
    }

    #[test]
    fn runtime_selection_and_detection_are_explicit() {
        assert_eq!(
            select_runtimes("codex,paseo").unwrap(),
            vec![Runtime::Codex, Runtime::Paseo]
        );
        assert!(select_runtimes("codex,codex").is_err());
        assert!(select_runtimes("unknown").is_err());
        assert!(!executable_in_paths(
            "never-present",
            &[PathBuf::from("/definitely/missing")]
        ));
    }

    #[cfg(unix)]
    #[test]
    fn runtime_detection_rejects_non_executable_regular_files() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let candidate = root.path().join("codex");
        fs::write(&candidate, "not executable").unwrap();
        fs::set_permissions(&candidate, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!executable_in_paths("codex", &[root.path().to_owned()]));
        fs::set_permissions(&candidate, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(executable_in_paths("codex", &[root.path().to_owned()]));
    }

    #[test]
    fn doctor_reports_unavailable_runtime_and_invalid_state_without_mutating() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let state = root.path().join("state");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&state).unwrap();
        fs::create_dir(state.join("artifacts")).unwrap();
        fs::write(state.join("metadata.sqlite3"), b"not sqlite").unwrap();
        let bridge = RuntimeBridge {
            schema: BRIDGE_SCHEMA.to_owned(),
            project_dir: project.display().to_string(),
            state_dir: state.display().to_string(),
            protocol: "weft.cli.v1".to_owned(),
            runtimes: vec![RuntimeEntry {
                name: "test".to_owned(),
                executable: "weft-test-runtime-never-present".to_owned(),
                detected: false,
                integration: "runtime-bridge".to_owned(),
                instruction_file: None,
            }],
        };
        fs::write(
            state.join(BRIDGE_FILE),
            serde_json::to_vec(&bridge).unwrap(),
        )
        .unwrap();

        let report = doctor(&state, &project).unwrap();
        assert_eq!(report["healthy"], false);
        assert!(
            report["problems"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value.as_str().unwrap().contains("unavailable on PATH"))
        );
        assert_eq!(
            fs::read(state.join("metadata.sqlite3")).unwrap(),
            b"not sqlite"
        );
    }
}
