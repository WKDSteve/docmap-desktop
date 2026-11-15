//! Every process this app starts for its own work, through `appkit_core::child`.
//!
//! The engine, a headless Neovim, `git clone` and `gh repo list` are children
//! of the app, and the helper of appkit is what makes that visible from
//! outside (`appkit/docs/PROCESSES.md`):
//!
//! - no console window, and its three streams are pipes of the app;
//! - it ends with the app: on Windows it is in a job object with
//!   `KILL_ON_JOB_CLOSE` (also when the app crashes or is killed), on Linux and
//!   macOS it leads a process group that is ended with the call;
//! - a time limit for the whole call and a size limit per output stream; when
//!   one is passed the child and everything it started are ended;
//! - an argv and an absolute program path, never a shell line;
//! - a line in the process log for the first call of each caller and program
//!   (`processes.log` in the app's log folder, see [`log_to`]).
//!
//! `Command::output()` used to wait forever, and nothing bound a child to the
//! app: a configuration that stops to ask something, a credential prompt
//! nobody can see or a hung binary kept a pool thread and a process for as long
//! as they liked, and outlived the window.
//!
//! The commands of this app are synchronous functions on a blocking thread; the
//! helper is async. [`run`] bridges the two (see [`block_on`]).

use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use appkit_core::child::{self, Environment, Leftovers, Start};

use crate::apperr::AppError;

/// The most a child may write to stdout by default: an engine answer of a large
/// project is a few megabytes, a report of a run a few kilobytes.
pub const MAX_STDOUT: usize = 64 * 1024 * 1024;
/// The most a child may write to stderr by default.
pub const MAX_STDERR: usize = 8 * 1024 * 1024;

/// Why [`run`] has no output to hand back. Not an [`AppError`] itself: which
/// sentence fits depends on the program (a Neovim that does not answer is most
/// likely waiting for input), so the caller picks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcError {
    /// It could not be started, was ended for writing too much, or a pipe
    /// failed; the reason, verbatim.
    Failed(String),
    /// It was ended after this many seconds without finishing.
    Timeout(u64),
    /// The person cancelled it ([`Cancel::cancel`]); it was ended with what it
    /// started.
    Cancelled,
}

impl ProcError {
    /// `err.proc.run`, `err.proc.timeout` or `err.proc.cancelled`, naming
    /// `program`.
    pub fn into_app(self, program: &str) -> AppError {
        match self {
            ProcError::Failed(e) => AppError::new("err.proc.run")
                .with("program", program)
                .detail(e),
            ProcError::Timeout(secs) => AppError::new("err.proc.timeout")
                .with("program", program)
                .with("secs", secs),
            ProcError::Cancelled => AppError::new("err.proc.cancelled").with("program", program),
        }
    }

    /// The same for a headless Neovim, whose silence asks a question of its
    /// own: does the configuration stop to wait for input?
    pub fn into_nvim(self, program: &str) -> AppError {
        match self {
            ProcError::Timeout(secs) => AppError::new("err.nvim.timeout")
                .with("program", program)
                .with("secs", secs),
            other => other.into_app(program),
        }
    }

    fn from_child(e: child::ProcError) -> ProcError {
        match e {
            child::ProcError::Timeout(limit) => ProcError::Timeout(limit.as_secs()),
            other => ProcError::Failed(other.to_string()),
        }
    }
}

/// What a finished child left: its exit code (`None` when a signal ended it)
/// and what it wrote.
#[derive(Debug, Clone, Default)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    /// Exit code 0.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// One process to start: the program, its argv, what changes in the app's
/// environment, and the limits. The shape of `std::process::Command` where the
/// call sites need it, so a test can read the arguments.
#[derive(Debug, Clone)]
pub struct Cmd {
    caller: &'static str,
    program: OsString,
    args: Vec<OsString>,
    set: Vec<(OsString, OsString)>,
    memory: Option<u64>,
    max_stdout: usize,
    max_stderr: usize,
}

impl Cmd {
    /// `caller` says in plain words who asks (`docmap-desktop generate`), for
    /// the log. `program` is a path, or a name that is looked up on `PATH`
    /// when the call starts (`git`, `gh`, `nvim`).
    pub fn new(caller: &'static str, program: impl AsRef<OsStr>) -> Cmd {
        Cmd {
            caller,
            program: program.as_ref().to_owned(),
            args: Vec::new(),
            set: Vec::new(),
            memory: None,
            max_stdout: MAX_STDOUT,
            max_stderr: MAX_STDERR,
        }
    }

    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Cmd {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Cmd
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_owned()));
        self
    }

    /// `name` set to `value` on top of the app's own environment.
    pub fn env(&mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Cmd {
        self.set
            .push((name.as_ref().to_owned(), value.as_ref().to_owned()));
        self
    }

    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Cmd
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        for (k, v) in vars {
            self.env(k, v);
        }
        self
    }

    /// The child may not hold more than `bytes` (`Start::memory`: Windows and
    /// Linux; macOS has no such limit).
    pub fn memory(&mut self, bytes: Option<u64>) -> &mut Cmd {
        self.memory = bytes;
        self
    }

    /// Other size limits for stdout and stderr than [`MAX_STDOUT`] and
    /// [`MAX_STDERR`].
    pub fn max_output(&mut self, stdout: usize, stderr: usize) -> &mut Cmd {
        self.max_stdout = stdout;
        self.max_stderr = stderr;
        self
    }

    #[cfg(test)]
    pub fn get_program(&self) -> &OsStr {
        &self.program
    }

    #[cfg(test)]
    pub fn get_args(&self) -> impl Iterator<Item = &OsStr> {
        self.args.iter().map(OsString::as_os_str)
    }

    #[cfg(test)]
    pub fn get_envs(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.set.iter().map(|(k, v)| (k.as_os_str(), v.as_os_str()))
    }

    fn start(&self) -> Result<Start, ProcError> {
        let program = resolve(Path::new(&self.program)).ok_or_else(|| {
            ProcError::Failed(format!(
                "{} was not found (neither a file nor a program on PATH)",
                Path::new(&self.program).display()
            ))
        })?;
        let mut start = Start::new(self.caller, program)
            .args(self.args.iter().cloned())
            .env(Environment::Inherited {
                set: self.set.clone(),
                remove: Vec::new(),
            });
        if let Some(bytes) = self.memory {
            start = start.memory(bytes);
        }
        Ok(start)
    }
}

/// The absolute path of `program`: itself when it is absolute, else the first
/// match on `PATH` (on Windows with the extensions of `PATHEXT` when it has
/// none). The helper starts nothing by a bare name, so the lookup the system
/// used to do happens here, once per call, visibly.
pub fn resolve(program: &Path) -> Option<PathBuf> {
    if program.is_absolute() {
        return program.is_file().then(|| program.to_path_buf());
    }
    // A relative path with a folder in it is not a name to look up.
    if program.components().count() != 1 {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    let extensions: Vec<OsString> = if cfg!(windows) && program.extension().is_none() {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(OsString::from)
            .collect()
    } else {
        vec![OsString::new()]
    };
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| {
            extensions.iter().map(move |ext| {
                let mut name = program.as_os_str().to_owned();
                name.push(ext);
                dir.join(name)
            })
        })
        .find(|candidate| candidate.is_file())
}

/// Run `cmd` to its end and collect its output, or end it (with everything it
/// started) after `limit`.
pub fn run(cmd: &Cmd, limit: Duration) -> Result<Output, ProcError> {
    run_cancellable(cmd, limit, None)
}

/// [`run`], and ended early when `cancel` is cancelled.
pub fn run_cancellable(
    cmd: &Cmd,
    limit: Duration,
    cancel: Option<&Cancel>,
) -> Result<Output, ProcError> {
    let start = cmd.start()?;
    let call = child::run(child::Run {
        start,
        stdin: Vec::new(),
        timeout: limit,
        max_stdout: cmd.max_stdout,
        max_stderr: cmd.max_stderr,
        leftovers: Leftovers::End,
    });
    let cancelled = cancel.map(Cancel::watch);
    let out = block_on(async move {
        match cancelled {
            // Dropping the call drops the child, and with it its job (Windows)
            // or its process group (Unix): the clone and its helpers end.
            Some(mut cancelled) => tokio::select! {
                out = call => out.map_err(ProcError::from_child),
                _ = cancelled.wait_for(|c| *c) => Err(ProcError::Cancelled),
            },
            None => call.await.map_err(ProcError::from_child),
        }
    })?;
    Ok(Output {
        code: out.code,
        stdout: out.stdout,
        stderr: out.stderr,
    })
}

/// A switch that ends a running call ([`run_cancellable`]) from elsewhere: the
/// person cancels an import while `git clone` runs.
#[derive(Debug, Clone)]
pub struct Cancel(Arc<tokio::sync::watch::Sender<bool>>);

impl Default for Cancel {
    fn default() -> Cancel {
        Cancel(Arc::new(tokio::sync::watch::channel(false).0))
    }
}

impl Cancel {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }

    fn watch(&self) -> tokio::sync::watch::Receiver<bool> {
        self.0.subscribe()
    }
}

/// Wait for `future` on a thread that is not async.
///
/// The commands of this app run on Tauri's blocking pool, the local server on
/// threads of its own, the tests on test threads. On a thread of a runtime
/// (the blocking pool, or a worker that a synchronous `async` command blocks
/// anyway) `block_in_place` hands the worker's tasks on and the runtime's own
/// handle waits; elsewhere Tauri's runtime does. Either way the child's pipes
/// and timers belong to a runtime that lives as long as the app.
fn block_on<F: Future>(future: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(move || handle.block_on(future)),
        Err(_) => tauri::async_runtime::block_on(future),
    }
}

static LOG: OnceLock<PathBuf> = OnceLock::new();
static LOG_LOCK: Mutex<()> = Mutex::new(());
/// The size at which the process log starts over.
const LOG_LIMIT: u64 = 1024 * 1024;

/// Send the helper's lines to `file` (the app's log folder): a release build
/// has no console, so stderr reaches nobody. Once, at start-up; the folder is
/// made when the first line is written.
pub fn log_to(file: PathBuf) {
    if LOG.set(file).is_ok() {
        child::set_log(write_log);
    }
}

fn write_log(line: &str) {
    use std::io::Write;
    let Some(file) = LOG.get() else { return };
    let _guard = LOG_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let too_big = std::fs::metadata(file).is_ok_and(|m| m.len() > LOG_LIMIT);
    let opened = if too_big {
        std::fs::File::create(file)
    } else {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
    };
    if let Ok(mut f) = opened {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "{secs} {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Not a test of its own: the child of the two memory tests below. Started
    /// as this test binary with `DOCMAP_TEST_HOLD_MB` set, it takes that much
    /// memory, touches every page and says so; without the variable it does
    /// nothing.
    #[test]
    fn child_holds_memory() {
        let Some(mb) = std::env::var("DOCMAP_TEST_HOLD_MB")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        else {
            return;
        };
        let mut held: Vec<Vec<u8>> = Vec::new();
        for _ in 0..mb {
            let mut block = vec![0u8; 1024 * 1024];
            for page in block.chunks_mut(4096) {
                page[0] = 1;
            }
            held.push(block);
        }
        println!("held {} MB", held.len());
    }

    #[cfg(any(windows, target_os = "linux"))]
    fn holding(mb: usize, memory: u64) -> Cmd {
        let mut cmd = Cmd::new("docmap-desktop test", std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "proc::tests::child_holds_memory",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("DOCMAP_TEST_HOLD_MB", mb.to_string())
        .memory(Some(memory));
        cmd
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn a_child_within_its_memory_limit_runs() {
        let out = run(&holding(8, 1024 * 1024 * 1024), Duration::from_secs(60)).unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.success(), "{text}");
        assert!(text.contains("held 8 MB"), "{text}");
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn a_child_that_takes_more_than_its_limit_does_not_get_it() {
        // 2 GB asked for under a limit of 256 MB: the allocation fails in the child (the Rust
        // runtime aborts on that), so it neither succeeds nor says it held the memory.
        let out = run(&holding(2048, 256 * 1024 * 1024), Duration::from_secs(60)).unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!out.success(), "{text}");
        assert!(!text.contains("held 2048 MB"), "{text}");
    }

    #[cfg(windows)]
    fn shell(line: &str) -> Cmd {
        let mut c = Cmd::new("docmap-desktop test", "cmd");
        c.args(["/c", line]);
        c
    }
    #[cfg(not(windows))]
    fn shell(line: &str) -> Cmd {
        let mut c = Cmd::new("docmap-desktop test", "sh");
        c.args(["-c", line]);
        c
    }

    #[test]
    fn a_finished_process_returns_its_output() {
        let out = run(&shell("echo hello"), Duration::from_secs(20)).unwrap();
        assert!(out.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));
    }

    #[test]
    fn a_process_that_does_not_finish_is_killed_and_reported() {
        #[cfg(windows)]
        let line = "ping -n 30 127.0.0.1 >nul";
        #[cfg(not(windows))]
        let line = "sleep 30";
        let started = Instant::now();
        let err = run(&shell(line), Duration::from_millis(1300)).unwrap_err();
        assert_eq!(err, ProcError::Timeout(1));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "must not wait for the child"
        );
    }

    #[test]
    fn a_process_that_writes_more_than_its_limit_is_ended() {
        let mut cmd = shell("echo 0123456789012345678901234567890123456789");
        cmd.max_output(16, 1024);
        let err = run(&cmd, Duration::from_secs(20)).unwrap_err();
        let ProcError::Failed(text) = &err else {
            panic!("{err:?}")
        };
        assert!(text.contains("stdout"), "{text}");
    }

    // The import's Cancel: the call comes back at once, and the child with
    // what it started is ended (the job, or the process group, goes with it).
    #[test]
    fn a_cancelled_call_returns_at_once() {
        #[cfg(windows)]
        let line = "ping -n 30 127.0.0.1 >nul";
        #[cfg(not(windows))]
        let line = "sleep 30";
        let cancel = Cancel::default();
        let later = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            later.cancel();
        });
        let started = Instant::now();
        let err =
            run_cancellable(&shell(line), Duration::from_secs(60), Some(&cancel)).unwrap_err();
        assert_eq!(err, ProcError::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(err.into_app("git").key(), "err.proc.cancelled");
    }

    // The same, from a thread of Tauri's blocking pool, where every command of
    // this app calls it: the bridge must neither panic nor deadlock there.
    #[test]
    fn a_call_from_the_blocking_pool_runs() {
        let out = tauri::async_runtime::block_on(tauri::async_runtime::spawn_blocking(|| {
            run(&shell("echo pool"), Duration::from_secs(20))
        }))
        .unwrap()
        .unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("pool"));
    }

    #[test]
    fn a_program_that_cannot_start_is_a_failure_with_the_reason() {
        let cmd = Cmd::new("docmap-desktop test", "docmap-desktop-no-such-program-x");
        let err = run(&cmd, Duration::from_secs(5)).unwrap_err();
        let ProcError::Failed(text) = &err else {
            panic!("{err:?}")
        };
        assert!(text.contains("not found"), "{text}");
        let app = err.into_app("x");
        assert_eq!(app.key(), "err.proc.run");
        assert_eq!(app.param("program"), Some("x"));
    }

    #[test]
    fn a_name_is_found_on_path_and_a_relative_path_is_not_looked_up() {
        #[cfg(windows)]
        let name = "cmd";
        #[cfg(not(windows))]
        let name = "sh";
        let found = resolve(Path::new(name)).expect("the shell is on PATH");
        assert!(
            found.is_absolute() && found.is_file(),
            "{}",
            found.display()
        );
        assert_eq!(resolve(Path::new("sub/cmd")), None);
        assert_eq!(resolve(&found), Some(found.clone()));
    }
}
