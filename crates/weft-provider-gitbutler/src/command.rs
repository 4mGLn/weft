use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::GitButlerProviderError;

const POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Clone, Copy, Debug)]
pub(crate) struct CommandPolicy {
    pub(crate) timeout: Duration,
    pub(crate) max_output_bytes: usize,
    #[cfg(test)]
    pub(crate) inject_post_spawn_failure: bool,
}

#[derive(Debug)]
pub(crate) struct CommandOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

#[derive(Debug)]
struct Captured {
    bytes: Vec<u8>,
    exceeded: bool,
}

pub(crate) fn run<I, S>(
    binary: &Path,
    directory: Option<&Path>,
    operation: &'static str,
    args: I,
    policy: CommandPolicy,
    environment: &[(OsString, OsString)],
) -> Result<CommandOutput, GitButlerProviderError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<OsString> = args
        .into_iter()
        .map(|value| value.as_ref().to_os_string())
        .collect();
    let mut child = spawn(binary, directory, &args, environment)?;
    #[cfg(test)]
    if policy.inject_post_spawn_failure && operation == "land-local-stack" {
        thread::sleep(Duration::from_millis(50));
        terminate(&mut child);
        return Err(GitButlerProviderError::InvalidOutput {
            operation,
            reason: "injected post-spawn collection failure".to_owned(),
        });
    }
    let deadline = Instant::now() + policy.timeout;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| GitButlerProviderError::InvalidOutput {
            operation,
            reason: "stdout pipe unavailable".to_owned(),
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| GitButlerProviderError::InvalidOutput {
            operation,
            reason: "stderr pipe unavailable".to_owned(),
        })?;
    let stop = Arc::new(AtomicBool::new(false));
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout_reader = capture(
        stdout,
        policy.max_output_bytes,
        Arc::clone(&stop),
        Arc::clone(&exceeded),
    );
    let stderr_reader = capture(
        stderr,
        policy.max_output_bytes,
        Arc::clone(&stop),
        Arc::clone(&exceeded),
    );

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if exceeded.load(Ordering::Relaxed) {
            terminate(&mut child);
            stop.store(true, Ordering::Relaxed);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(GitButlerProviderError::OutputLimit { operation });
        }
        if Instant::now() >= deadline {
            terminate(&mut child);
            stop.store(true, Ordering::Relaxed);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(GitButlerProviderError::CommandTimedOut { operation });
        }
        thread::sleep(POLL_INTERVAL);
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| GitButlerProviderError::InvalidOutput {
            operation,
            reason: "stdout reader panicked".to_owned(),
        })?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| GitButlerProviderError::InvalidOutput {
            operation,
            reason: "stderr reader panicked".to_owned(),
        })?;
    if stdout.exceeded || stderr.exceeded {
        return Err(GitButlerProviderError::OutputLimit { operation });
    }
    Ok(CommandOutput {
        status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

fn spawn(
    binary: &Path,
    directory: Option<&Path>,
    args: &[OsString],
    environment: &[(OsString, OsString)],
) -> Result<Child, GitButlerProviderError> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .envs(environment.iter().cloned())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    set_process_group(&mut command);
    Ok(command.spawn()?)
}

#[cfg(unix)]
fn set_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn set_process_group(_command: &mut Command) {}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = format!("-{}", child.id());
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &process_group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn capture(
    mut reader: impl Read + Send + 'static,
    limit: usize,
    stop: Arc<AtomicBool>,
    output_exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<Captured> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut exceeded = false;
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    let available = limit.saturating_sub(bytes.len());
                    let retained = count.min(available);
                    bytes.extend_from_slice(&buffer[..retained]);
                    exceeded |= retained < count;
                    if exceeded {
                        output_exceeded.store(true, Ordering::Relaxed);
                    }
                }
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
        }
        Captured { bytes, exceeded }
    })
}
