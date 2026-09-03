use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::GitProviderError;

const POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Clone, Copy, Debug)]
pub(crate) struct CommandPolicy {
    pub(crate) timeout: Duration,
    pub(crate) max_output_bytes: usize,
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

pub(crate) fn run_git<I, S>(
    git: &Path,
    directory: Option<&Path>,
    operation: &'static str,
    args: I,
    input: Option<&[u8]>,
    policy: CommandPolicy,
) -> Result<CommandOutput, GitProviderError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<OsString> = args
        .into_iter()
        .map(|value| value.as_ref().to_os_string())
        .collect();
    let deadline = Instant::now() + policy.timeout;
    let mut child = spawn_git(git, directory, &args, input.is_some(), deadline, operation)?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| GitProviderError::InvalidOutput {
            operation,
            reason: "stdout pipe unavailable".to_owned(),
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| GitProviderError::InvalidOutput {
            operation,
            reason: "stderr pipe unavailable".to_owned(),
        })?;
    let stop = Arc::new(AtomicBool::new(false));
    let output_exceeded = Arc::new(AtomicBool::new(false));
    let stdout_reader = capture(
        stdout,
        policy.max_output_bytes,
        Arc::clone(&stop),
        Arc::clone(&output_exceeded),
    );
    let stderr_reader = capture(
        stderr,
        policy.max_output_bytes,
        Arc::clone(&stop),
        Arc::clone(&output_exceeded),
    );

    let mut input_writer = if let Some(input) = input {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| GitProviderError::InvalidOutput {
                operation,
                reason: "stdin pipe unavailable".to_owned(),
            })?;
        let input = input.to_vec();
        Some(thread::spawn(move || stdin.write_all(&input)))
    } else {
        None
    };

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if output_exceeded.load(Ordering::Relaxed) {
            terminate(&mut child);
            stop.store(true, Ordering::Relaxed);
            join_input_writer(input_writer.take());
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(GitProviderError::OutputLimit { operation });
        }
        if Instant::now() >= deadline {
            terminate(&mut child);
            stop.store(true, Ordering::Relaxed);
            join_input_writer(input_writer.take());
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(GitProviderError::CommandTimedOut { operation });
        }
        thread::sleep(POLL_INTERVAL);
    };
    finish_input_writer(input_writer.take(), operation)?;
    let stdout = stdout_reader
        .join()
        .map_err(|_| GitProviderError::InvalidOutput {
            operation,
            reason: "stdout reader panicked".to_owned(),
        })?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| GitProviderError::InvalidOutput {
            operation,
            reason: "stderr reader panicked".to_owned(),
        })?;
    if stdout.exceeded || stderr.exceeded {
        return Err(GitProviderError::OutputLimit { operation });
    }
    Ok(CommandOutput {
        status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

fn finish_input_writer(
    writer: Option<thread::JoinHandle<std::io::Result<()>>>,
    operation: &'static str,
) -> Result<(), GitProviderError> {
    if let Some(writer) = writer {
        writer
            .join()
            .map_err(|_| GitProviderError::InvalidOutput {
                operation,
                reason: "stdin writer panicked".to_owned(),
            })??;
    }
    Ok(())
}

fn join_input_writer(writer: Option<thread::JoinHandle<std::io::Result<()>>>) {
    if let Some(writer) = writer {
        let _ = writer.join();
    }
}

fn spawn_git(
    git: &Path,
    directory: Option<&Path>,
    args: &[OsString],
    pipe_input: bool,
    deadline: Instant,
    operation: &'static str,
) -> Result<Child, GitProviderError> {
    let mut command = Command::new(git);
    command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .stdin(if pipe_input {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    set_process_group(&mut command);
    loop {
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(error) if error.kind() == ErrorKind::ExecutableFileBusy => {
                if Instant::now() >= deadline {
                    return Err(GitProviderError::CommandTimedOut { operation });
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(error) => return Err(error.into()),
        }
    }
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
