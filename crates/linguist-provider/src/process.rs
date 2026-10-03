//! Bounded local helper processes (OCR, speech synthesis).
//!
//! No shell is used, the environment is reduced to a fixed minimum, and each
//! child runs in its own process group with an address-space limit. Timeouts or
//! excess output kill the whole group; output is never truncated.
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

/// Diagnostic stderr is drained so the child cannot block, but never persisted.
const STDERR_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    SpawnFailed,
    Timeout,
    Failed { exit_code: Option<i32> },
    OutputLimit,
}

pub struct ProcessLimits {
    pub deadline: Instant,
    pub output: usize,
    pub memory: u64,
}

/// No shell is involved; the environment is reduced to a fixed minimum.
pub fn command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C.UTF-8")
        .env("OMP_THREAD_LIMIT", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Run a child under the shared deadline, memory and output bounds. Timeout or
/// excess output kills the whole process group; output is never truncated.
pub fn run(command: Command, limits: &ProcessLimits) -> Result<Vec<u8>, ProcessError> {
    run_with_input(command, None, limits)
}

/// As [`run`], writing `input` to the child's stdin (untrusted text never
/// becomes an argument) and then closing it.
pub fn run_with_input(
    mut command: Command,
    input: Option<Vec<u8>>,
    limits: &ProcessLimits,
) -> Result<Vec<u8>, ProcessError> {
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    if Instant::now() >= limits.deadline {
        return Err(ProcessError::Timeout);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let memory = limits.memory;
        // SAFETY: only async-signal-safe libc calls run between fork and exec.
        unsafe {
            command.pre_exec(move || {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let limit = libc::rlimit {
                    rlim_cur: memory,
                    rlim_max: memory,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command.spawn().map_err(|_| ProcessError::SpawnFailed)?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        // A child that exits early closes the pipe; that is not a write error here.
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(&input);
        });
    }
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout = drain(
        child.stdout.take().unwrap(),
        limits.output,
        Some(exceeded.clone()),
    );
    let stderr = drain(child.stderr.take().unwrap(), STDERR_LIMIT, None);
    let status = loop {
        if exceeded.load(Ordering::SeqCst) {
            kill(&mut child);
            let _ = (stdout.join(), stderr.join());
            return Err(ProcessError::OutputLimit);
        }
        if Instant::now() >= limits.deadline {
            kill(&mut child);
            let _ = (stdout.join(), stderr.join());
            return Err(ProcessError::Timeout);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                kill(&mut child);
                return Err(ProcessError::Failed { exit_code: None });
            }
        }
    };
    // A surviving grandchild could hold the pipe open; the group is killed either way.
    kill_group(&child);
    let output = stdout.join().unwrap_or_default();
    let _ = stderr.join();
    if exceeded.load(Ordering::SeqCst) {
        return Err(ProcessError::OutputLimit);
    }
    if !status.success() {
        return Err(ProcessError::Failed {
            exit_code: status.code(),
        });
    }
    Ok(output)
}

fn drain(
    mut pipe: impl Read + Send + 'static,
    limit: usize,
    exceeded: Option<Arc<AtomicBool>>,
) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) if kept.len() + n > limit => {
                    if let Some(flag) = &exceeded {
                        flag.store(true, Ordering::SeqCst);
                        break;
                    }
                }
                Ok(n) => kept.extend_from_slice(&chunk[..n]),
            }
        }
        kept
    })
}

fn kill_group(child: &Child) {
    #[cfg(unix)]
    // SAFETY: the child leads its own process group created in pre_exec.
    unsafe {
        libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
    }
}

fn kill(child: &mut Child) {
    kill_group(child);
    let _ = child.kill();
    let _ = child.wait();
}

pub fn hash_file(path: &Path, limit: u64) -> std::io::Result<(String, u64)> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let bytes = std::io::copy(&mut (&mut file).take(limit.saturating_add(1)), &mut hasher)?;
    if bytes > limit {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    Ok((hex(&hasher.finalize()), bytes))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

/// Private derivative directory removed on drop, including after failures.
pub struct TempDir(pub PathBuf);
impl TempDir {
    /// Create a private 0700 directory below `parent` (created privately if absent).
    pub fn create_in(parent: &Path) -> std::io::Result<Self> {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.recursive(true).create(parent)?;
        let path = parent.join(format!("helper-{}", uuid::Uuid::new_v4()));
        builder.recursive(false).create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
