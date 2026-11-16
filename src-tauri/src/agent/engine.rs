//! What the run asks of the engine: `rules/plan` and `rules/validate`, and
//! the two routes that write what a person decided: `rules/accept` and
//! `rules/waive`.
//!
//! The first two are pure routes of the standalone engine (`docmap.nvim`
//! `core/rules_api.lua`, wiring `rules.nvim`'s `engine/agent`), so the prompt a
//! model receives and the check of every quote against the tree are the same
//! code in Neovim and here. Nothing is rebuilt on this side. [`Engine`] is the
//! seam: the real one starts the binary, the tests bring a canned one.
//!
//! The writers are `rules.nvim`'s own (`rules.engine.verdicts.accept` and the
//! `waive` operation of `rules.ops`), run by the engine: this side never writes
//! `.rules-verdicts.json` or `.rules-waivers.json` itself. [`Judge`] is their
//! seam, apart from [`Engine`] because an engine that plans need not write.
//!
//! The engine's answers are read leniently where its JSON encoder is: an empty
//! Lua table may arrive as `{}` instead of `[]`, so every list accepts both.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::apperr::AppError;
use crate::proc;

/// `--capabilities` answers in milliseconds, or the binary is hung.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// A plan reads the rulesets and every file of a scope; a validation re-plans
/// one rule. Generous, bounded.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// A list that the engine may encode as `[]` or, when empty, as `{}`.
fn lenient<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: for<'a> Deserialize<'a>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::Null => Ok(Vec::new()),
        Value::Object(o) if o.is_empty() => Ok(Vec::new()),
        other => serde_json::from_value(other).map_err(serde::de::Error::custom),
    }
}

/// One rule of a batch, as `rules/plan` lists it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanRule {
    pub id: String,
    pub severity: Option<String>,
    pub title: Option<String>,
    /// `rule` or `family`: where its scope came from.
    pub source: Option<String>,
}

/// One request of the plan: what to ask, about which files and rules.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanBatch {
    /// The engine's name of the batch (`b1`): a word, not a number.
    pub id: String,
    pub system: String,
    pub prompt: String,
    #[serde(deserialize_with = "lenient")]
    pub files: Vec<String>,
    pub bytes: u64,
    pub tokens: u64,
    #[serde(deserialize_with = "lenient")]
    pub rules: Vec<PlanRule>,
}

/// The project's allow-list, which the host enforces.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentCfg {
    #[serde(deserialize_with = "lenient")]
    pub backends: Vec<String>,
    #[serde(deserialize_with = "lenient")]
    pub models: Vec<String>,
}

/// `rules/plan`, the parts a run needs. The rest of the answer (denied paths,
/// limits, rules with a check, warnings) is kept verbatim in `extra` for the
/// window that previews the run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Plan {
    #[serde(deserialize_with = "lenient")]
    pub batches: Vec<PlanBatch>,
    /// Rules that are not sent, each with its reason.
    #[serde(deserialize_with = "lenient")]
    pub skipped: Vec<Value>,
    pub totals: Value,
    pub agent: AgentCfg,
    pub extra: Value,
}

impl Plan {
    /// The plan of an `available` answer, or the engine's reason it is not.
    pub fn from_engine(v: &Value) -> Result<Plan, AppError> {
        unavailable(v)?;
        let mut plan: Plan = serde_json::from_value(v.clone())
            .map_err(|e| AppError::new("err.agent.engineShape").detail(e))?;
        let mut extra = serde_json::Map::new();
        for k in [
            "denied",
            "limits",
            "with_check",
            "unknown",
            "warnings",
            "errors",
        ] {
            if let Some(x) = v.get(k) {
                extra.insert(k.to_string(), x.clone());
            }
        }
        plan.extra = Value::Object(extra);
        Ok(plan)
    }
}

/// `{available = false, reason}` is the engine saying no, not a crash.
fn unavailable(v: &Value) -> Result<(), AppError> {
    if v.get("available").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    let reason = v
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("the engine gave no reason");
    Err(AppError::new("err.agent.engine").detail(reason))
}

/// The engine as the run uses it.
pub trait Engine: Send + Sync {
    /// `rules/plan` for `select` (rule ids, families, `gate:<name>`, comma
    /// separated), over the project at `root`.
    fn plan(&self, root: &Path, select: &str) -> Result<Plan, AppError>;

    /// `rules/validate` of one rule's raw `answer`: the engine's `proposal`
    /// object, never a verdict. `scratch` is the folder of the run inside the
    /// project, for the answer file (as for [`Judge::accept`]).
    fn validate(
        &self,
        root: &Path,
        rule: &str,
        answer: &str,
        scratch: &Path,
    ) -> Result<Value, AppError>;

    /// `rules/attach`: whether the file `path` (relative to `root`) may go into
    /// a request, and its text as a request carries it. The planner's decision
    /// (deny list, links, size), never this side's. An engine without the
    /// route says so.
    fn attach(&self, _root: &Path, _path: &str) -> Result<Value, AppError> {
        Err(AppError::new("err.agent.engineNoAttach"))
    }
}

/// The engine's two writers, as the run view uses them. Each call is one
/// person's decision about one rule; there is no call for several.
pub trait Judge: Send + Sync {
    /// `rules/accept`: `proposal` (the object `rules/validate` gave, as it was
    /// stored) becomes the verdict of `rule`, accepted by `by`. The engine's
    /// answer on success; its refusal as `err.agent.refused` with the reason.
    /// `scratch` is a folder of the project's own for the proposal file.
    fn accept(
        &self,
        root: &Path,
        rule: &str,
        proposal: &Value,
        by: &str,
        note: Option<&str>,
        scratch: &Path,
    ) -> Result<Value, AppError>;

    /// `rules/waive`: a waiver of `rule` with `reason`, until `expires`
    /// (`YYYY-MM-DD`), limited to `paths` when there are any. `scratch` as for
    /// [`Judge::accept`], for the file with the texts.
    fn waive(
        &self,
        root: &Path,
        rule: &str,
        reason: &str,
        expires: Option<&str>,
        paths: &[String],
        scratch: &Path,
    ) -> Result<Value, AppError>;
}

/// The engine binary as a process to start (`proc`: bound to the app, no
/// console window), its grammars named. Shared with the Rules tab's own calls
/// (`rules/engine.rs`), the map server and Generate.
pub(crate) fn command(caller: &'static str, program: &str, grammars: Option<&str>) -> proc::Cmd {
    let mut cmd = proc::Cmd::new(caller, program);
    if let Some(g) = grammars {
        cmd.env("DOCMAP_TS_DIR", g);
    }
    cmd
}

/// What makes an answer of `--capabilities` stale: another engine file at the
/// same path (size, modification time), or a change in the grammars folder
/// (a grammar added or removed changes which languages the engine has).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    engine: Option<(u64, std::time::SystemTime)>,
    grammars: Option<std::time::SystemTime>,
}

impl Stamp {
    fn of(program: &str, grammars: Option<&str>) -> Stamp {
        let engine = proc::resolve(Path::new(program))
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| Some((m.len(), m.modified().ok()?)));
        let grammars = grammars
            .and_then(|g| std::fs::metadata(g).ok())
            .and_then(|m| m.modified().ok());
        Stamp { engine, grammars }
    }
}

type Answers = std::collections::HashMap<(String, Option<String>), (Stamp, Option<Vec<u8>>)>;

/// The answer of `--capabilities` of `program` (stdout of an exit 0; `None`
/// when the engine cannot be asked: one from before exits 2 without doing
/// anything), asked once per engine path and grammars folder and remembered
/// until the engine file or the folder changes.
///
/// It used to be asked before every request of the map page, two processes per
/// request; the answer only changes with the binary. A failure (the engine is
/// missing, hangs, exits with anything but 0 or 2) is not remembered: the
/// next call asks again.
pub(crate) fn capabilities_answer(
    program: &str,
    grammars: Option<&str>,
) -> Result<Option<Vec<u8>>, AppError> {
    static ANSWERS: std::sync::OnceLock<std::sync::Mutex<Answers>> = std::sync::OnceLock::new();
    let answers = ANSWERS.get_or_init(Default::default);
    let key = (program.to_string(), grammars.map(str::to_string));
    let stamp = Stamp::of(program, grammars);
    {
        let known = answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((seen, answer)) = known.get(&key) {
            if *seen == stamp && stamp.engine.is_some() {
                return Ok(answer.clone());
            }
        }
    }
    let mut cmd = command("docmap-desktop engine capabilities", program, grammars);
    cmd.arg("--capabilities").max_output(1024 * 1024, 64 * 1024);
    let out = proc::run(&cmd, PROBE_TIMEOUT).map_err(|e| e.into_app(program))?;
    let answer = out.success().then_some(out.stdout);
    // Only a real answer and the documented refusal of an old engine (exit 2)
    // are facts about the binary; a one-off crash or a lock by the virus
    // scanner is asked again next time instead of meaning "old engine" until
    // the app restarts.
    if answer.is_some() || out.code == Some(2) {
        answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, (stamp, answer.clone()));
    }
    Ok(answer)
}

/// What `--capabilities` of the engine lists; nothing when it does not answer
/// the question (an engine from before it exits 2 without doing anything).
pub(crate) fn capabilities(program: &str, grammars: Option<&str>) -> Result<Vec<String>, AppError> {
    Ok(capabilities_answer(program, grammars)?
        .and_then(|stdout| serde_json::from_slice::<Value>(&stdout).ok())
        .and_then(|v| v.get("capabilities").cloned())
        .and_then(|c| serde_json::from_value(c).ok())
        .unwrap_or_default())
}

/// The `/api/*` routes the engine lists in `--capabilities`; nothing when it
/// does not answer the question.
pub(crate) fn routes(program: &str, grammars: Option<&str>) -> Result<Vec<String>, AppError> {
    Ok(capabilities_answer(program, grammars)?
        .and_then(|stdout| serde_json::from_slice::<Value>(&stdout).ok())
        .and_then(|v| v.get("routes").cloned())
        .and_then(|r| serde_json::from_value(r).ok())
        .unwrap_or_default())
}

/// The route that came with `--input=<file>` (docmap.nvim `0fe645e`): an
/// engine that lists it takes the texts of `rules/accept` and `rules/waive`
/// from a UTF-8 file, and there is no capability of its own for that.
pub(crate) const JUDGED_ROUTE: &str = "rules/judged";

/// The standalone `docmap` binary, one process per call.
pub struct CliEngine {
    program: String,
    grammars: Option<String>,
    /// `rules-attach` is among its capabilities (an engine from before the
    /// chat has the agent functions without it).
    attach: bool,
}

impl CliEngine {
    /// Checked before the first call, for the reason `server.rs`'s
    /// `engine_supports_api` gives: an engine from before `--api=` would
    /// *generate the map* on an unknown argument. `--capabilities` is safe on
    /// every version (an old one exits 2 before doing anything), and the run
    /// needs `api` and `rules-agent`.
    pub fn new(program: String, grammars: Option<String>) -> Result<CliEngine, AppError> {
        let caps = capabilities(&program, grammars.as_deref())?;
        if !(caps.iter().any(|c| c == "api") && caps.iter().any(|c| c == "rules-agent")) {
            return Err(AppError::new("err.agent.engineOld").with("engine", &program));
        }
        let attach = caps.iter().any(|c| c == "rules-attach");
        Ok(CliEngine {
            program,
            grammars,
            attach,
        })
    }

    fn command(&self) -> proc::Cmd {
        command(
            "docmap-desktop rules agent",
            &self.program,
            self.grammars.as_deref(),
        )
    }

    fn call(&self, root: &Path, route: &str, flags: &[String]) -> Result<Value, AppError> {
        let mut cmd = self.command();
        cmd.arg(root).arg(format!("--api={route}")).args(flags);
        let out = proc::run(&cmd, CALL_TIMEOUT).map_err(|e| e.into_app(&self.program))?;
        if !out.success() {
            return Err(
                AppError::new("err.agent.engine").detail(String::from_utf8_lossy(&out.stderr))
            );
        }
        serde_json::from_slice(&out.stdout).map_err(|_| {
            AppError::new("err.agent.engine").detail("the engine did not answer with JSON")
        })
    }
}

impl Engine for CliEngine {
    fn plan(&self, root: &Path, select: &str) -> Result<Plan, AppError> {
        Plan::from_engine(&self.call(root, "rules/plan", &[format!("--select={select}")])?)
    }

    fn validate(
        &self,
        root: &Path,
        rule: &str,
        answer: &str,
        scratch: &Path,
    ) -> Result<Value, AppError> {
        // A file, not stdin (the engine's own choice: a host that forgets to
        // close a pipe would leave it waiting for ever), in the run's own
        // folder rather than the temp folder (`TempAnswer::write_in`). Removed
        // when done.
        let file = TempAnswer::write_in(scratch, "answer", answer)?;
        let v = self
            .call(
                root,
                "rules/validate",
                &[
                    format!("--select={rule}"),
                    format!("--answer={}", answer_arg(file.path()).display()),
                ],
            )
            .and_then(|v| unavailable(&v).map(|()| v))
            .map_err(answer_unreadable)?;
        v.get("proposal")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| AppError::new("err.agent.engineShape"))
    }

    fn attach(&self, root: &Path, path: &str) -> Result<Value, AppError> {
        if !self.attach {
            return Err(AppError::new("err.agent.engineNoAttach").with("engine", &self.program));
        }
        attach_arg(path)?;
        let v = self.call(root, "rules/attach", &[format!("--select={path}")])?;
        unavailable(&v)?;
        Ok(v)
    }
}

/// A path the engine is handed on its command line: on Windows it reads that
/// line through the ANSI code page (`answer_arg`), so a path that would arrive
/// changed is refused here rather than asked about under another name.
fn attach_arg(path: &str) -> Result<(), AppError> {
    if path.trim().is_empty() || path.chars().any(char::is_control) {
        return Err(AppError::new("err.agent.attachPath").with("path", path));
    }
    if cfg!(windows) && !path.is_ascii() {
        return Err(AppError::new("err.agent.ascii").with("text", path));
    }
    Ok(())
}

/// The standalone binary as the writer of verdicts and waivers.
pub struct CliJudge(CliEngine);

impl CliJudge {
    /// Checked like [`CliEngine::new`], and for `rules-write` on top: an engine
    /// from before the two routes cannot write, and the person should learn
    /// that in words before deciding, not from an engine's error.
    pub fn new(program: String, grammars: Option<String>) -> Result<CliJudge, AppError> {
        let caps = capabilities(&program, grammars.as_deref())?;
        let has = |c: &str| caps.iter().any(|x| x == c);
        // `--input=` is how the texts a person typed reach the engine unchanged
        // ([`input_file`]); an engine without it would get them through the
        // ANSI code page on Windows.
        let input = routes(&program, grammars.as_deref())?
            .iter()
            .any(|r| r == JUDGED_ROUTE);
        if !(has("api") && has("rules") && has("rules-agent") && has("rules-write") && input) {
            return Err(AppError::new("err.agent.engineNoWrite").with("engine", &program));
        }
        Ok(CliJudge(CliEngine {
            program,
            grammars,
            attach: has("rules-attach"),
        }))
    }
}

impl Judge for CliJudge {
    fn accept(
        &self,
        root: &Path,
        rule: &str,
        proposal: &Value,
        by: &str,
        note: Option<&str>,
        scratch: &Path,
    ) -> Result<Value, AppError> {
        let text = serde_json::to_string(proposal)
            .map_err(|e| AppError::new("err.workspace.serialise").detail(e))?;
        // A file, as `--answer=` is, in the run's own folder (`TempAnswer::write_in`). Removed when done.
        let file = TempAnswer::write_in(scratch, "proposal", &text)?;
        let mut texts = serde_json::Map::new();
        texts.insert("by".into(), Value::from(by));
        if let Some(note) = note {
            texts.insert("note".into(), Value::from(note));
        }
        let input = input_file(scratch, texts)?;
        let flags = vec![
            format!("--select={rule}"),
            format!("--proposal={}", answer_arg(file.path()).display()),
            format!("--input={}", answer_arg(input.path()).display()),
        ];
        written(self.0.call(root, "rules/accept", &flags)?)
    }

    fn waive(
        &self,
        root: &Path,
        rule: &str,
        reason: &str,
        expires: Option<&str>,
        paths: &[String],
        scratch: &Path,
    ) -> Result<Value, AppError> {
        let mut texts = serde_json::Map::new();
        texts.insert("reason".into(), Value::from(reason));
        if let Some(day) = expires {
            texts.insert("expires".into(), Value::from(day));
        }
        if !paths.is_empty() {
            texts.insert("paths".into(), Value::from(paths.to_vec()));
        }
        let input = input_file(scratch, texts)?;
        let flags = vec![
            format!("--select={rule}"),
            format!("--input={}", answer_arg(input.path()).display()),
        ];
        written(self.0.call(root, "rules/waive", &flags)?)
    }
}

/// The texts a person typed (`by`, `note`, `reason`, `expires`, `paths`) as
/// the UTF-8 JSON file of `--input=`, in the run's own folder like the
/// proposal file, removed when done. On the command line the standalone
/// engine reads them through the ANSI code page on Windows: measured
/// 2026-11-15, a reason with `ü` was refused as "a control character" and
/// one with `中` was written as `?`. Through the file they arrive as typed.
fn input_file(
    scratch: &Path,
    texts: serde_json::Map<String, Value>,
) -> Result<TempAnswer, AppError> {
    let text = serde_json::to_string(&Value::Object(texts))
        .map_err(|e| AppError::new("err.workspace.serialise").detail(e))?;
    TempAnswer::write_in(scratch, "input", &text)
}

/// A writer's answer: as it is when the engine wrote (or found the same entry
/// there already), else its refusal, after which the file is as it was.
fn written(v: Value) -> Result<Value, AppError> {
    if v.get("available").and_then(Value::as_bool) == Some(true) {
        return Ok(v);
    }
    let reason = v
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("the engine gave no reason");
    Err(AppError::new("err.agent.refused").detail(reason))
}

/// The path the engine is told for the answer file.
///
/// The bundled engine reads its arguments through the ANSI code page on Windows, so a temp folder with a character
/// outside it (a user name with `中`) made every answer unreadable. Such a path goes in its 8.3 form, which is plain
/// ASCII (`C:\Users\5E2D~1\...`). Where there is none (8.3 names switched off on the drive) it stays as it is, and
/// the engine's "cannot be read" stops the run ([`answer_unreadable`]). A relative name beside a working directory
/// does not help: the engine changes its directory while it walks the project and finds its way back through the
/// same code page.
pub(crate) fn answer_arg(path: &Path) -> std::path::PathBuf {
    if path.to_str().is_some_and(str::is_ascii) {
        return path.to_path_buf();
    }
    short_ascii(path).unwrap_or_else(|| path.to_path_buf())
}

/// The 8.3 form of an existing `path`, when it is plain ASCII.
#[cfg(windows)]
fn short_ascii(path: &Path) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    #[link(name = "kernel32")]
    extern "system" {
        fn GetShortPathNameW(long: *const u16, short: *mut u16, len: u32) -> u32;
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    let cap = u32::try_from(buf.len()).ok()?;
    // SAFETY: `wide` is NUL-terminated and lives through the call; `buf` holds `cap` u16s, and the call writes at
    // most that many (a buffer too small gets nothing written and the size it needs returned instead).
    let n = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), cap) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let short = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]));
    short.to_str().is_some_and(str::is_ascii).then_some(short)
}

#[cfg(not(windows))]
fn short_ascii(_path: &Path) -> Option<std::path::PathBuf> {
    None
}

/// An engine that could not read the answer it was handed did not check the rule: that is the hand-over failing, not
/// a finding, and the run must not count the batch as done.
fn answer_unreadable(e: AppError) -> AppError {
    let unread = e
        .detail_text()
        .is_some_and(|d| d.contains("the answer file cannot be read"));
    if e.key() == "err.agent.engine" && unread {
        let detail = e.detail_text().unwrap_or_default().to_string();
        return AppError::new("err.agent.answerUnreadable").detail(detail);
    }
    e
}

/// An answer or a proposal on disk for the length of one engine call.
pub struct TempAnswer(std::path::PathBuf);

impl TempAnswer {
    /// The file in `dir`, always the folder of its run inside the project,
    /// never the temp folder: the standalone engine has
    /// been seen not to come back from paths below `AppData\Local\Temp` on
    /// Windows (`rules/tab.rs` measured it for a project there), and neither a
    /// run nor a decision a person waits for should depend on whether a file
    /// there is read.
    pub fn write_in(dir: &Path, stem: &str, text: &str) -> Result<TempAnswer, AppError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let path = dir.join(format!(
            "docmap-{stem}-{}-{}-{}.txt",
            std::process::id(),
            super::now_ms(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, text).map_err(|e| crate::path_err("err.path.write", &path, e))?;
        Ok(TempAnswer(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempAnswer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An engine that counts how often it is asked: every start appends a
    /// line to `count.txt` next to it, and it answers `--capabilities`.
    fn counting_engine(dir: &Path, version: &str) -> String {
        #[cfg(windows)]
        let (name, body) = (
            "engine.cmd",
            format!(
                "@echo off\r\necho x>>\"%~dp0count.txt\"\r\necho {{\"capabilities\":[\"api\"],\"v\":\"{version}\"}}\r\n"
            ),
        );
        #[cfg(not(windows))]
        let (name, body) = (
            "engine.sh",
            format!(
                "#!/bin/sh\necho x >> \"$(dirname \"$0\")/count.txt\"\necho '{{\"capabilities\":[\"api\"],\"v\":\"{version}\"}}'\n"
            ),
        );
        let file = dir.join(name);
        std::fs::write(&file, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        file.to_string_lossy().into_owned()
    }

    fn asked(dir: &Path) -> usize {
        std::fs::read_to_string(dir.join("count.txt"))
            .map(|t| t.lines().count())
            .unwrap_or(0)
    }

    // The map page asked `--capabilities` before every request: two processes
    // per request. Now once per engine, and again only when the file changes.
    #[test]
    fn capabilities_are_asked_once_per_engine_file() {
        let dir = std::env::temp_dir().join(format!("docmap-caps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let engine = counting_engine(&dir, "1");

        assert_eq!(capabilities(&engine, None).unwrap(), ["api"]);
        assert_eq!(capabilities(&engine, None).unwrap(), ["api"]);
        assert!(capabilities_answer(&engine, None).unwrap().is_some());
        assert_eq!(asked(&dir), 1, "asked again for the same engine");

        // Another engine at the same path (a different size): asked anew.
        counting_engine(&dir, "22");
        assert_eq!(capabilities(&engine, None).unwrap(), ["api"]);
        assert_eq!(asked(&dir), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An engine that fails its first start (exit 1) and answers from the
    /// second on: `fails.txt` next to it marks that the failure happened.
    fn flaky_engine(dir: &Path) -> String {
        #[cfg(windows)]
        let (name, body) = (
            "flaky.cmd",
            "@echo off
if not exist \"%~dp0fails.txt\" (
  echo x>\"%~dp0fails.txt\"
  exit /b 1
)
echo {\"capabilities\":[\"api\"]}
",
        );
        #[cfg(not(windows))]
        let (name, body) = (
            "flaky.sh",
            "#!/bin/sh
d=\"$(dirname \"$0\")\"
if [ ! -e \"$d/fails.txt\" ]; then echo x > \"$d/fails.txt\"; exit 1; fi
echo '{\"capabilities\":[\"api\"]}'
",
        );
        let file = dir.join(name);
        std::fs::write(&file, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        file.to_string_lossy().into_owned()
    }

    // A one-off crash (exit 1) was remembered as "old engine" until the app
    // restarted; only exit 0 and the old engine's exit 2 are remembered.
    #[test]
    fn a_one_off_failure_of_capabilities_is_asked_again() {
        let dir = std::env::temp_dir().join(format!("docmap-caps-flaky-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let engine = flaky_engine(&dir);

        assert!(capabilities(&engine, None).unwrap().is_empty());
        assert_eq!(capabilities(&engine, None).unwrap(), ["api"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plan_is_read_with_empty_lists_in_either_encoding() {
        let v = json!({
            "available": true,
            "batches": [{
                "id": "b3", "system": "S", "prompt": "P", "files": {}, "bytes": 10, "tokens": 5,
                "rules": [{"id": "A-1", "severity": "error", "title": "t", "source": "rule", "truncated": null}],
                "unreadable": {}
            }],
            "skipped": {},
            "denied": {},
            "totals": {"requests": 1},
            "agent": {"backends": {}, "models": ["qwen"]},
            "warnings": []
        });
        let p = Plan::from_engine(&v).unwrap();
        assert_eq!(p.batches.len(), 1);
        assert_eq!(p.batches[0].id, "b3");
        assert!(p.batches[0].files.is_empty());
        assert_eq!(p.batches[0].rules[0].id, "A-1");
        assert!(p.skipped.is_empty());
        assert!(p.agent.backends.is_empty());
        assert_eq!(p.agent.models, vec!["qwen"]);
        assert_eq!(p.extra["denied"], json!({}));
    }

    #[test]
    fn an_engine_that_says_no_gives_its_reason() {
        let e = Plan::from_engine(
            &json!({"available": false, "reason": "the selection matches no rule"}),
        )
        .unwrap_err();
        assert_eq!(e.key(), "err.agent.engine");
        assert_eq!(e.detail_text(), Some("the selection matches no rule"));
        let e = Plan::from_engine(&json!({"batches": []})).unwrap_err();
        assert_eq!(e.key(), "err.agent.engine");
    }

    #[test]
    fn a_plain_path_goes_as_it_is() {
        let file = TempAnswer::write_in(&std::env::temp_dir(), "answer", "{}").unwrap();
        if file.path().to_str().is_some_and(str::is_ascii) {
            assert_eq!(answer_arg(file.path()), file.path());
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_path_outside_the_code_page_goes_in_its_short_ascii_form() {
        let dir = std::env::temp_dir().join(format!("docmap-中-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("docmap-answer-1.txt");
        std::fs::write(&file, "{}").unwrap();
        let arg = answer_arg(&file);
        let text = std::fs::read_to_string(&arg);
        let short = short_ascii(&file);
        let _ = std::fs::remove_dir_all(&dir);
        match short {
            // 8.3 names are on (the default on the system drive): ASCII, and it is the same file
            Some(_) => {
                assert!(arg.to_str().is_some_and(str::is_ascii), "{}", arg.display());
                assert_eq!(text.unwrap(), "{}");
            }
            // switched off on this drive: the path stays, the engine's refusal stops the run
            None => assert_eq!(arg, file),
        }
    }

    #[test]
    fn an_answer_the_engine_cannot_read_is_its_own_error() {
        let e = answer_unreadable(
            AppError::new("err.agent.engine").detail("the answer file cannot be read: x.txt"),
        );
        assert_eq!(e.key(), "err.agent.answerUnreadable");
        assert_eq!(
            e.detail_text(),
            Some("the answer file cannot be read: x.txt")
        );
        let other = answer_unreadable(AppError::new("err.agent.engine").detail("no rule A-9"));
        assert_eq!(other.key(), "err.agent.engine");
    }

    #[test]
    fn a_writer_that_says_no_is_a_refusal_with_its_reason() {
        let ok = json!({"available": true, "written": true, "id": "M-1"});
        assert_eq!(written(ok.clone()).unwrap(), ok);
        let e = written(
            json!({"available": false, "reason": "waived already", "code": "invalid_argument"}),
        )
        .unwrap_err();
        assert_eq!(e.key(), "err.agent.refused");
        assert_eq!(e.detail_text(), Some("waived already"));
        let e = written(json!({"written": true})).unwrap_err();
        assert_eq!(e.detail_text(), Some("the engine gave no reason"));
    }

    #[test]
    fn a_plan_of_the_wrong_shape_is_named_as_such() {
        let e = Plan::from_engine(&json!({"available": true, "batches": "nope"})).unwrap_err();
        assert_eq!(e.key(), "err.agent.engineShape");
    }

    #[test]
    fn the_answer_file_holds_the_text_and_goes_away() {
        let text = "{\"results\": []}\nline two";
        let file = TempAnswer::write_in(&std::env::temp_dir(), "answer", text).unwrap();
        let path = file.path().to_path_buf();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        drop(file);
        assert!(!path.exists());
    }
}
