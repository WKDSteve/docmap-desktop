//! What the Rules tab asks of the engine: `rules/catalog`, `rules/run` and
//! `rules/judged`.
//!
//! Both are routes of the standalone engine (`docmap.nvim`
//! `core/rules_api.lua`, which wires `rules.nvim`), so the list of rules and
//! the mechanical run are the same code as `:Rules check` in Neovim. Nothing
//! of a ruleset is read or judged on this side, with one exception that is
//! this side's job: which rulesets may run their predicates (`trust.rs`), told
//! to the engine as `--trust=<folder>`.
//!
//! A run is the one engine call that executes code from a ruleset, so it is
//! the one with a memory limit next to the time limit (`proc::Cmd::memory`).
//! [`Routes`] is the seam: the real one starts the binary, the tests bring a
//! canned one.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::apperr::AppError;
use crate::proc::{self, ProcError};

/// A catalog reads the rulesets; no code of them runs. As generous as a run: on Windows the engine
/// resolves every path through `cmd.exe`, a few seconds on a quiet machine and more than a minute
/// on one that is busy building (measured 2026-11-14, 100 % CPU: over 60 s for seven rules).
const CATALOG_TIMEOUT: Duration = Duration::from_secs(120);
/// A run greps a project and runs the trusted predicates. A predicate in a
/// loop ends here, as an error the rows show, not as a window that waits.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(120);
/// What the engine may hold during a run. It reads one file at a time and its
/// answer is a few megabytes at most; a predicate that asks for more than
/// this is the runaway the limit is for.
pub const RUN_MEMORY: u64 = 1024 * 1024 * 1024;

/// The engine as the Rules tab uses it.
pub trait Routes: Send + Sync {
    /// `rules/catalog` of the project at `root`, the engine's answer as it is.
    fn catalog(&self, root: &Path) -> Result<Value, AppError>;

    /// `rules/run` of `select` (rule ids, families, `gate:<name>`, comma
    /// separated); the predicates of the folders in `trust` may run.
    fn run(&self, root: &Path, select: &str, trust: &[String]) -> Result<Value, AppError>;
}

/// The standalone `docmap` binary, one process per call.
pub struct CliRoutes {
    program: String,
    grammars: Option<String>,
    /// Variables set for the engine on top of the app's own environment. Empty in the app: the
    /// vault a user names in `RULES_VAULT` is theirs to name. A test empties that variable, so
    /// its fixture is the only ruleset there is.
    env: Vec<(String, String)>,
    /// `rules/judged` is among its routes (an engine from before it has the
    /// rules without it).
    judged: bool,
}

impl CliRoutes {
    /// Checked before the first call, for the reason `agent/engine.rs` gives:
    /// an engine from before `--api=` would generate the map on an unknown
    /// argument. The tab needs `api` and `rules`.
    pub fn new(program: String, grammars: Option<String>) -> Result<CliRoutes, AppError> {
        let caps = crate::agent::engine::capabilities(&program, grammars.as_deref())?;
        if !(caps.iter().any(|c| c == "api") && caps.iter().any(|c| c == "rules")) {
            return Err(AppError::new("err.rules.engineOld").with("engine", &program));
        }
        let judged = crate::agent::engine::routes(&program, grammars.as_deref())?
            .iter()
            .any(|r| r == crate::agent::engine::JUDGED_ROUTE);
        Ok(CliRoutes {
            program,
            grammars,
            env: Vec::new(),
            judged,
        })
    }

    /// The same engine with `name` set to `value` for every call.
    #[cfg(test)]
    pub fn with_env(mut self, name: &str, value: &str) -> CliRoutes {
        self.env.push((name.to_string(), value.to_string()));
        self
    }

    fn call(
        &self,
        root: &Path,
        route: &str,
        flags: &[String],
        limit: Duration,
        memory: Option<u64>,
    ) -> Result<Value, AppError> {
        let mut cmd = crate::agent::engine::command(
            "docmap-desktop rules tab",
            &self.program,
            self.grammars.as_deref(),
        );
        cmd.envs(self.env.iter().map(|(k, v)| (k, v)));
        cmd.arg(root)
            .arg(format!("--api={route}"))
            .args(flags)
            .memory(memory);
        let out = proc::run(&cmd, limit).map_err(|e| match e {
            ProcError::Timeout(secs) => AppError::new("err.rules.timeout").with("secs", secs),
            failed => failed.into_app(&self.program),
        })?;
        if !out.success() {
            // Killed by the memory limit, or an engine that failed: either way it said nothing
            // usable, and what it wrote last is the only account of why.
            return Err(AppError::new("err.rules.engine").detail(last_lines(&out.stderr)));
        }
        serde_json::from_slice(&out.stdout).map_err(|_| {
            AppError::new("err.rules.engine").detail("the engine did not answer with JSON")
        })
    }
}

impl CliRoutes {
    /// `rules/judged` of the project at `root`: what a person decided (the
    /// verdicts and waivers), read by `rules.nvim`'s own readers, the engine's
    /// answer as it is. It runs no ruleset code; `git log` decides whether a
    /// verdict is stale, so it gets the catalog's time.
    pub fn judged(&self, root: &Path) -> Result<Value, AppError> {
        if !self.judged {
            return Err(AppError::new("err.rules.engineNoJudged").with("engine", &self.program));
        }
        self.call(
            root,
            crate::agent::engine::JUDGED_ROUTE,
            &[],
            CATALOG_TIMEOUT,
            None,
        )
    }
}

/// The end of what a failed engine wrote, short enough for a row.
fn last_lines(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    if text.is_empty() {
        return "the engine stopped without a message (out of memory?)".to_string();
    }
    let start = text
        .char_indices()
        .rev()
        .nth(599)
        .map(|(i, _)| i)
        .unwrap_or(0);
    text[start..].to_string()
}

impl Routes for CliRoutes {
    fn catalog(&self, root: &Path) -> Result<Value, AppError> {
        self.call(root, "rules/catalog", &[], CATALOG_TIMEOUT, None)
    }

    fn run(&self, root: &Path, select: &str, trust: &[String]) -> Result<Value, AppError> {
        let mut flags = vec![format!("--select={select}")];
        for folder in trust {
            // the engine reads its arguments through the ANSI code page on Windows
            let arg = crate::agent::engine::answer_arg(Path::new(folder));
            flags.push(format!("--trust={}", arg.display()));
        }
        self.call(root, "rules/run", &flags, RUN_TIMEOUT, Some(RUN_MEMORY))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silent_failure_says_so_and_a_long_one_keeps_its_end() {
        assert!(last_lines(b"  \n").contains("without a message"));
        assert_eq!(last_lines(b"not enough memory\n"), "not enough memory");
        let long = format!("{}END", "x".repeat(5000));
        let cut = last_lines(long.as_bytes());
        assert_eq!(cut.chars().count(), 600);
        assert!(cut.ends_with("END"));
    }
}
