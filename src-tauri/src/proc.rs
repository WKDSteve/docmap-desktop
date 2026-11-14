//! Running a child process with a time limit.
//!
//! `Command::output()` waits forever. For the processes this app starts on a
//! button -- a headless Neovim that loads the user's whole configuration, a
//! `git clone`, the engine -- "forever" is a real outcome: a configuration that
//! stops to ask something, a credential prompt nobody can see, a hung binary.
//! The button then never comes back and a pool thread is lost with it.

use std::io::Read;
use std::time::{Duration, Instant};

use crate::apperr::AppError;

/// Why [`run_with_timeout`] has no output to hand back. Not an [`AppError`]
/// itself: which sentence fits depends on the program (a Neovim that does not
/// answer is most likely waiting for input), so the caller picks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcError {
    /// It could not be started or waited for; the OS error, verbatim.
    Failed(String),
    /// It was killed after this many seconds without finishing.
    Timeout(u64),
}

impl ProcError {
    /// `err.proc.run` or `err.proc.timeout`, naming `program`.
    pub fn into_app(self, program: &str) -> AppError {
        match self {
            ProcError::Failed(e) => AppError::new("err.proc.run")
                .with("program", program)
                .detail(e),
            ProcError::Timeout(secs) => AppError::new("err.proc.timeout")
                .with("program", program)
                .with("secs", secs),
        }
    }

    /// The same for a headless Neovim, whose silence asks a question of its
    /// own: does the configuration stop to wait for input?
    pub fn into_nvim(self, program: &str) -> AppError {
        match self {
            ProcError::Timeout(secs) => AppError::new("err.nvim.timeout")
                .with("program", program)
                .with("secs", secs),
            failed => failed.into_app(program),
        }
    }
}

/// Run `cmd` to completion and collect its output, or kill it after `limit`.
///
/// Only the child this function started is killed, by its handle. The output
/// is read on separate threads so a full pipe cannot stall the child while this
/// one waits, and after a kill the readers get a short grace period rather
/// than being joined: a grandchild that inherited the pipe would otherwise keep
/// them (and this call) alive.
pub fn run_with_timeout(
    cmd: &mut std::process::Command,
    limit: Duration,
) -> Result<std::process::Output, ProcError> {
    run_limited(cmd, limit, None)
}

/// [`run_with_timeout`], and with `memory` the child may not hold more than
/// that many bytes.
///
/// For a child that runs somebody else's code: the engine evaluating the
/// `lua_predicate` checks of a ruleset. Its sandbox counts instructions, not
/// bytes, so one call can ask for gigabytes; with the limit that allocation
/// fails inside the child (Lua reports "not enough memory", the rule is an
/// `error` row) or the child dies, and neither takes the machine with it.
///
/// What the limit is, per platform:
///
/// - **Windows**: a job object with a memory limit for the whole job, so a
///   grandchild counts too, and `KILL_ON_JOB_CLOSE`: nothing of the job
///   outlives this call. The child is assigned right after it starts; the few
///   milliseconds before that are not covered.
/// - **Linux**: `RLIMIT_AS`, set in the child before it becomes the program.
///   It bounds address space, which is more than what is used; the limit has
///   to leave room for that.
/// - **macOS and the rest**: none. The kernel there does not enforce an
///   address-space limit a process sets for itself, so only the time limit
///   holds; saying otherwise would be a promise nothing keeps.
///
/// A limit that cannot be set is a failure, not a run without one.
pub fn run_limited(
    cmd: &mut std::process::Command,
    limit: Duration,
    memory: Option<u64>,
) -> Result<std::process::Output, ProcError> {
    use std::process::Stdio;
    use std::sync::mpsc;

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "linux")]
    if let Some(bytes) = memory {
        limit::before_exec(cmd, bytes);
    }
    let mut child = cmd.spawn().map_err(|e| ProcError::Failed(e.to_string()))?;
    #[cfg(windows)]
    let _job = match memory {
        Some(bytes) => match limit::Job::around(&child, bytes) {
            Ok(job) => Some(job),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProcError::Failed(e));
            }
        },
        None => None,
    };
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = memory;

    fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    }
    let out_rx = drain(child.stdout.take());
    let err_rx = drain(child.stderr.take());

    let deadline = Instant::now() + limit;
    // `None` covers both ways out of the loop without a status: the deadline
    // passed, or `try_wait` itself errored. Either way the child is killed
    // before this function returns -- a bare `?` on the error would have
    // left it running, unread and un-reaped, for as long as it lives.
    let mut wait_err: Option<String> = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                wait_err = Some(e.to_string());
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };

    let grace = Duration::from_millis(500);
    let stdout = out_rx.recv_timeout(grace).unwrap_or_default();
    let stderr = err_rx.recv_timeout(grace).unwrap_or_default();
    match (status, wait_err) {
        (Some(status), _) => Ok(std::process::Output {
            status,
            stdout,
            stderr,
        }),
        (None, Some(e)) => Err(ProcError::Failed(e)),
        (None, None) => Err(ProcError::Timeout(limit.as_secs())),
    }
}

/// The memory limit of [`run_limited`], Windows: a job object.
#[cfg(windows)]
mod limit {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    type Handle = *mut c_void;

    const EXTENDED_LIMIT_INFORMATION: u32 = 9;
    const LIMIT_JOB_MEMORY: u32 = 0x0000_0200;
    const LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

    #[repr(C)]
    #[derive(Default)]
    struct BasicLimits {
        per_process_user_time: i64,
        per_job_user_time: i64,
        limit_flags: u32,
        minimum_working_set: usize,
        maximum_working_set: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct IoCounters([u64; 6]);

    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory: usize,
        peak_job_memory: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: u32, info: *const c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    /// A job that holds one child. Closing it (the drop) ends whatever of the
    /// job is still running.
    pub struct Job(Handle);

    impl Job {
        /// A job with a memory limit of `bytes`, and `child` in it.
        pub fn around(child: &std::process::Child, bytes: u64) -> Result<Job, String> {
            let last = || std::io::Error::last_os_error().to_string();
            // SAFETY: no attributes and no name are valid arguments; the handle is checked below.
            let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
            if handle.is_null() {
                return Err(last());
            }
            let job = Job(handle);
            let mut info = ExtendedLimits::default();
            info.basic.limit_flags = LIMIT_JOB_MEMORY | LIMIT_KILL_ON_JOB_CLOSE;
            info.job_memory_limit = usize::try_from(bytes).unwrap_or(usize::MAX);
            let len =
                u32::try_from(std::mem::size_of::<ExtendedLimits>()).map_err(|e| e.to_string())?;
            // SAFETY: `info` is the structure class 9 asks for, alive for the call, `len` its size.
            let set = unsafe {
                SetInformationJobObject(
                    job.0,
                    EXTENDED_LIMIT_INFORMATION,
                    (&info as *const ExtendedLimits).cast(),
                    len,
                )
            };
            if set == 0 {
                return Err(last());
            }
            // SAFETY: both handles are open: the job above, the child for as long as `child` lives.
            if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle().cast()) } == 0 {
                return Err(last());
            }
            Ok(job)
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle came from `CreateJobObjectW` and is closed nowhere else.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

/// The memory limit of [`run_limited`], Linux: `RLIMIT_AS` in the child.
#[cfg(target_os = "linux")]
mod limit {
    use std::os::unix::process::CommandExt;

    const RLIMIT_AS: i32 = 9;

    #[repr(C)]
    struct RLimit {
        cur: u64,
        max: u64,
    }

    extern "C" {
        fn setrlimit(resource: i32, limit: *const RLimit) -> i32;
    }

    /// Have the child lower its own address-space limit to `bytes` before it
    /// becomes the program; a child that cannot does not start.
    pub fn before_exec(cmd: &mut std::process::Command, bytes: u64) {
        // SAFETY: the closure runs between fork and exec and only calls `setrlimit`, which is
        // async-signal-safe; it allocates nothing and touches no lock.
        unsafe {
            cmd.pre_exec(move || {
                let limit = RLimit {
                    cur: bytes,
                    max: bytes,
                };
                if setrlimit(RLIMIT_AS, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn holding(mb: usize) -> std::process::Command {
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "proc::tests::child_holds_memory",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("DOCMAP_TEST_HOLD_MB", mb.to_string());
        cmd
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn a_child_within_its_memory_limit_runs() {
        let out = run_limited(
            &mut holding(8),
            Duration::from_secs(60),
            Some(1024 * 1024 * 1024),
        )
        .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{text}");
        assert!(text.contains("held 8 MB"), "{text}");
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn a_child_that_takes_more_than_its_limit_does_not_get_it() {
        // 2 GB asked for under a limit of 256 MB: the allocation fails in the child (the Rust
        // runtime aborts on that), so it neither succeeds nor says it held the memory.
        let out = run_limited(
            &mut holding(2048),
            Duration::from_secs(60),
            Some(256 * 1024 * 1024),
        )
        .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!out.status.success(), "{text}");
        assert!(!text.contains("held 2048 MB"), "{text}");
    }

    #[cfg(windows)]
    fn shell(line: &str) -> std::process::Command {
        let mut c = std::process::Command::new("cmd");
        c.args(["/c", line]);
        c
    }
    #[cfg(not(windows))]
    fn shell(line: &str) -> std::process::Command {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", line]);
        c
    }

    #[test]
    fn a_finished_process_returns_its_output() {
        let out = run_with_timeout(&mut shell("echo hello"), Duration::from_secs(20)).unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));
    }

    #[test]
    fn a_process_that_does_not_finish_is_killed_and_reported() {
        #[cfg(windows)]
        let line = "ping -n 30 127.0.0.1 >nul";
        #[cfg(not(windows))]
        let line = "sleep 30";
        let started = Instant::now();
        let err = run_with_timeout(&mut shell(line), Duration::from_millis(1300)).unwrap_err();
        assert_eq!(err, ProcError::Timeout(1));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "must not wait for the child"
        );
    }

    #[test]
    fn a_program_that_cannot_start_is_a_failure_with_the_os_error() {
        let mut cmd = std::process::Command::new("docmap-desktop-no-such-program-x");
        let err = run_with_timeout(&mut cmd, Duration::from_secs(5)).unwrap_err();
        let ProcError::Failed(text) = &err else {
            panic!("{err:?}")
        };
        assert!(!text.is_empty());
        let app = err.into_app("x");
        assert_eq!(app.key(), "err.proc.run");
        assert_eq!(app.param("program"), Some("x"));
    }

    #[test]
    fn a_silent_neovim_is_asked_whether_it_waits_for_input() {
        let e = ProcError::Timeout(60).into_nvim("nvim");
        assert_eq!(e.key(), "err.nvim.timeout");
        assert_eq!(e.param("secs"), Some("60"));
        let e = ProcError::Timeout(60).into_app("git");
        assert_eq!(e.key(), "err.proc.timeout");
        let e = ProcError::Failed("nope".into()).into_nvim("nvim");
        assert_eq!(e.key(), "err.proc.run");
        assert_eq!(e.detail_text(), Some("nope"));
    }
}
