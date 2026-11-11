//! What a run leaves on disk, in the project: `.rules-proposals/`.
//!
//! ```text
//! .rules-proposals/
//!   cache/<key>.json        the raw answer of a request, by the request's key
//!   <run-id>/run.json       the record: title, notes, selection, destination,
//!                           batches, proposals, timestamps
//!   <run-id>/plan.json      the frozen plan (the requests, file contents included)
//! ```
//!
//! **Proposals are not committed** (decision 2 of 2026-09-21): the folder is
//! git-ignored, and the ignore entry is *offered* on first use ([`ignore_state`],
//! [`ignore_add`]) rather than written behind the user's back. They live in
//! the project so Neovim and the app see each other's.
//!
//! **The cache key** is `sha256(system, prompt, backend, model)` joined by NUL,
//! exactly what `rules.nvim`'s `engine/agent/cache` computes. The prompt holds
//! the rule text, the question and the numbered files, so an unchanged rule
//! against an unchanged tree is not asked twice, and any edit is a different
//! key. What is stored is the answer *as received*, never the result of
//! validation: the answer is checked against the tree again on every use, so a
//! cache can never turn an unchecked text into a proposal that looks checked.
//!
//! **Outdated.** A proposal remembers the hash of its rule's block in the
//! prompt. A later plan tells an edited rule (`rule`) from changed files
//! (`files`) and from a rule that is no longer planned (`gone`); see
//! [`outdated`].
//!
//! Every file is replaced atomically (`write_atomic`), so a reader, or a
//! restart after a crash, sees the old content or the new, never half.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::client::{Backend, Usage};
use super::engine::Plan;
use crate::apperr::AppError;
use crate::path_err;

/// The folder at the project root.
pub const DIR: &str = ".rules-proposals";
/// The line `.gitignore` is offered.
pub const IGNORE_ENTRY: &str = ".rules-proposals/";

/// Hex SHA-256 of `parts` joined by NUL, the form `rules.nvim` keys its cache in.
pub fn sha256_hex(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            h.update([0u8]);
        }
        h.update(p.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The key of one request to one destination.
pub fn request_key(system: &str, prompt: &str, backend: Backend, model: &str) -> String {
    sha256_hex(&[system, prompt, backend.name(), model])
}

/// Hash of each rule's block inside a batch prompt (`### ID (severity)`, the
/// question, the text), by id. A rule whose block cannot be found is left out,
/// and its proposal then compares by request key alone.
pub fn rule_hashes(prompt: &str, ids: &[String]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(head) = prompt.strip_prefix("Rules to check:\n\n") else {
        return out;
    };
    // The files start with `=== path ===`; a rule's own text is cut at the
    // first place a file block follows the `Files:` line.
    let section = head
        .find("\n\nFiles:\n\n=== ")
        .map(|end| &head[..end])
        .unwrap_or(head);
    let mut starts: Vec<(usize, &String)> = ids
        .iter()
        .filter_map(|id| {
            let marker = format!("### {id} (");
            let at = if section.starts_with(&marker) {
                Some(0)
            } else {
                section.find(&format!("\n\n{marker}")).map(|i| i + 2)
            };
            at.map(|i| (i, id))
        })
        .collect();
    starts.sort_by_key(|(i, _)| *i);
    for (n, (start, id)) in starts.iter().enumerate() {
        let end = starts.get(n + 1).map(|(i, _)| *i).unwrap_or(section.len());
        out.insert(
            (*id).clone(),
            sha256_hex(&[section[*start..end].trim_end()]),
        );
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Running,
    /// Stopped but resumable: the app quit, or crashed, while it ran.
    Paused,
    Cancelled,
    Done,
    /// Stopped by an error that would have hit every batch (see `AskError::is_fatal`).
    Failed,
}

impl RunState {
    pub fn is_open(self) -> bool {
        matches!(self, RunState::Running | RunState::Paused)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BatchState {
    Queued,
    Asking,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BatchRec {
    pub id: String,
    pub rules: Vec<String>,
    pub files: usize,
    pub bytes: u64,
    /// The plan's estimate, before the request; the exact count is `usage`.
    pub tokens: u64,
    pub key: String,
    pub state: BatchState,
    /// Answered from the cache: nothing was sent.
    pub from_cache: bool,
    pub started_ms: Option<u64>,
    pub finished_ms: Option<u64>,
    pub usage: Usage,
    /// The failure in the wire shape of an `AppError` (`key`, `params`, `detail`, `message`).
    pub error: Option<Value>,
}

impl Default for BatchRec {
    fn default() -> Self {
        BatchRec {
            id: String::new(),
            rules: Vec::new(),
            files: 0,
            bytes: 0,
            tokens: 0,
            key: String::new(),
            state: BatchState::Queued,
            from_cache: false,
            started_ms: None,
            finished_ms: None,
            usage: Usage::default(),
            error: None,
        }
    }
}

/// One rule's outcome. `proposal` is the engine's object, with `proposed`
/// forced to true and `verified` to false when it is stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProposalRec {
    pub rule_id: String,
    pub batch: String,
    pub key: String,
    pub rule_hash: Option<String>,
    /// `violation`, `compliant` or `unclear` (the engine's word), or `error`
    /// when the answer could not be checked at all.
    pub status: String,
    pub proposal: Value,
    pub error: Option<Value>,
    pub created_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Run {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub root: String,
    /// The selection as `rules/plan` got it: frozen, so a repeat uses the same.
    pub selection: String,
    /// Asked again even where the answer is cached, also after a resume.
    pub refresh: bool,
    pub backend: Backend,
    pub model: String,
    pub local: bool,
    pub state: RunState,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub finished_ms: Option<u64>,
    pub batches: Vec<BatchRec>,
    pub proposals: Vec<ProposalRec>,
    /// Rules the plan did not send, each with its reason.
    pub skipped: Vec<Value>,
    pub usage: Usage,
    pub error: Option<Value>,
}

impl Default for Run {
    fn default() -> Self {
        Run {
            id: String::new(),
            title: String::new(),
            notes: String::new(),
            root: String::new(),
            selection: String::new(),
            refresh: false,
            backend: Backend::Ollama,
            model: String::new(),
            local: true,
            state: RunState::Paused,
            created_ms: 0,
            updated_ms: 0,
            finished_ms: None,
            batches: Vec::new(),
            proposals: Vec::new(),
            skipped: Vec::new(),
            usage: Usage::default(),
            error: None,
        }
    }
}

impl Run {
    /// Put a proposal in, replacing an earlier one for the same rule.
    pub fn set_proposal(&mut self, p: ProposalRec) {
        self.proposals.retain(|q| q.rule_id != p.rule_id);
        self.proposals.push(p);
    }
}

/// A run for a list: no batches, no proposals.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub root: String,
    pub title: String,
    pub selection: String,
    pub backend: Backend,
    pub model: String,
    pub local: bool,
    pub state: RunState,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub finished_ms: Option<u64>,
    pub batches_total: usize,
    pub batches_done: usize,
    pub proposals: usize,
    pub usage: Usage,
}

impl Summary {
    pub fn of(run: &Run) -> Summary {
        Summary {
            id: run.id.clone(),
            root: run.root.clone(),
            title: run.title.clone(),
            selection: run.selection.clone(),
            backend: run.backend,
            model: run.model.clone(),
            local: run.local,
            state: run.state,
            created_ms: run.created_ms,
            updated_ms: run.updated_ms,
            finished_ms: run.finished_ms,
            batches_total: run.batches.len(),
            batches_done: run
                .batches
                .iter()
                .filter(|b| matches!(b.state, BatchState::Done | BatchState::Failed))
                .count(),
            proposals: run.proposals.len(),
            usage: run.usage,
        }
    }
}

/// An answer kept by its request's key. Same fields as `rules.nvim`'s entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CacheEntry {
    pub text: String,
    pub provider: String,
    pub model: Option<String>,
    pub date: String,
}

/// Whether a stored proposal still matches what the rules and the tree say now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Freshness {
    /// Same rule, same request.
    Current,
    /// The rule's text (or question) was edited since.
    Rule,
    /// The rule is the same, the files it looked at are not.
    Files,
    /// The rule is not in the plan any more (deleted, or no longer sent).
    Gone,
}

/// Each proposal of `run` against a fresh `plan` of the same selection.
///
/// The request key is recomputed for the run's own destination, so a model
/// change does not show as an edit. Where the rule's block could not be
/// separated out of a prompt, a different key can only be called `files`.
pub fn outdated(run: &Run, plan: &Plan) -> BTreeMap<String, Freshness> {
    let mut now: HashMap<&str, (String, Option<String>)> = HashMap::new();
    for b in &plan.batches {
        let ids: Vec<String> = b.rules.iter().map(|r| r.id.clone()).collect();
        let key = request_key(&b.system, &b.prompt, run.backend, &run.model);
        let hashes = rule_hashes(&b.prompt, &ids);
        for r in &b.rules {
            now.insert(&r.id, (key.clone(), hashes.get(&r.id).cloned()));
        }
    }
    run.proposals
        .iter()
        .map(|p| {
            let state = match now.get(p.rule_id.as_str()) {
                None => Freshness::Gone,
                Some((key, hash)) => {
                    if *key == p.key {
                        Freshness::Current
                    } else if p.rule_hash.is_some() && *hash != p.rule_hash {
                        Freshness::Rule
                    } else {
                        Freshness::Files
                    }
                }
            };
            (p.rule_id.clone(), state)
        })
        .collect()
}

/// A run id is made of letters, digits and `-`: it becomes a folder name, and
/// one that came over IPC is not trusted to stay where it started.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A fresh run id. Unique within a process by a counter, across processes by
/// the pid and the clock.
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "r{}-{}-{}",
        super::now_ms(),
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

/// The proposals folder of one project.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: &Path) -> Store {
        Store {
            root: root.to_path_buf(),
        }
    }

    fn base(&self) -> PathBuf {
        self.root.join(DIR)
    }

    /// The folder, made if needed, and checked to stay inside the project: a
    /// link that leads elsewhere would put the project's file contents there.
    fn ensure_base(&self) -> Result<PathBuf, AppError> {
        let base = self.base();
        fs::create_dir_all(&base).map_err(|e| path_err("err.path.create", &base, e))?;
        crate::resolve_inside(&self.root, DIR)?;
        Ok(base)
    }

    /// A folder below the base, made if needed, and checked like the base: a link at `cache` or at a run's folder
    /// would lead just as far out.
    fn ensure_below(&self, name: &str) -> Result<PathBuf, AppError> {
        self.ensure_base()?;
        let dir = self.base().join(name);
        fs::create_dir_all(&dir).map_err(|e| path_err("err.path.create", &dir, e))?;
        crate::resolve_inside(&self.root, &format!("{DIR}/{name}"))?;
        Ok(dir)
    }

    /// Hold the run `id` for this thread until the file is dropped: `None` when another process has it. An app that
    /// is started twice must not see the other's live run as cut off, or ask its batches a second time.
    pub fn hold(&self, id: &str) -> Result<Option<fs::File>, AppError> {
        if !valid_id(id) {
            return Err(AppError::new("err.agent.noRun").with("id", id));
        }
        let path = self.ensure_below(id)?.join("run.lock");
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| path_err("err.path.write", &path, e))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(e)) => Err(path_err("err.path.write", &path, e)),
        }
    }

    /// Whether a thread holds the run `id` right now, here or in another process.
    pub fn held(&self, id: &str) -> bool {
        let Ok(dir) = self.run_dir(id) else {
            return false;
        };
        let Ok(file) = fs::OpenOptions::new()
            .write(true)
            .open(dir.join("run.lock"))
        else {
            return false;
        };
        matches!(file.try_lock(), Err(fs::TryLockError::WouldBlock))
    }

    fn run_dir(&self, id: &str) -> Result<PathBuf, AppError> {
        if !valid_id(id) {
            return Err(AppError::new("err.agent.noRun").with("id", id));
        }
        Ok(self.base().join(id))
    }

    fn write_json(&self, path: &Path, value: &impl Serialize) -> Result<(), AppError> {
        let body = serde_json::to_string_pretty(value)
            .map_err(|e| AppError::new("err.workspace.serialise").detail(e))?;
        crate::write_atomic(path, body).map_err(|e| path_err("err.path.write", path, e))
    }

    fn read_json<T: for<'a> Deserialize<'a>>(&self, path: &Path) -> Result<T, AppError> {
        let body = fs::read_to_string(path).map_err(|e| path_err("err.path.read", path, e))?;
        serde_json::from_str(&body).map_err(|e| path_err("err.agent.runUnreadable", path, e))
    }

    pub fn save(&self, run: &Run) -> Result<(), AppError> {
        self.run_dir(&run.id)?;
        let dir = self.ensure_below(&run.id)?;
        self.write_json(&dir.join("run.json"), run)
    }

    pub fn save_plan(&self, id: &str, plan: &Plan) -> Result<(), AppError> {
        self.run_dir(id)?;
        let dir = self.ensure_below(id)?;
        self.write_json(&dir.join("plan.json"), plan)
    }

    pub fn load(&self, id: &str) -> Result<Run, AppError> {
        let path = self.run_dir(id)?.join("run.json");
        if !path.is_file() {
            return Err(AppError::new("err.agent.noRun").with("id", id));
        }
        self.read_json(&path)
    }

    pub fn load_plan(&self, id: &str) -> Result<Plan, AppError> {
        self.read_json(&self.run_dir(id)?.join("plan.json"))
    }

    /// Every readable run, newest first. A folder that does not read as a run
    /// (an old format, a half-deleted one) is left out, not an error.
    pub fn list(&self) -> Vec<Run> {
        let mut runs: Vec<Run> = fs::read_dir(self.base())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                (name != "cache").then(|| self.load(&name).ok()).flatten()
            })
            .collect();
        runs.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(b.id.cmp(&a.id)));
        runs
    }

    fn cache_path(&self, key: &str) -> Option<PathBuf> {
        (!key.is_empty() && key.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| self.base().join("cache").join(format!("{key}.json")))
    }

    /// The answer kept under `key`; anything unreadable is a miss.
    pub fn cache_get(&self, key: &str) -> Option<CacheEntry> {
        let body = fs::read_to_string(self.cache_path(key)?).ok()?;
        serde_json::from_str(&body).ok()
    }

    /// Keep an answer. Losing the write loses only the saving, not the answer.
    pub fn cache_put(&self, key: &str, entry: &CacheEntry) {
        let Some(path) = self.cache_path(key) else {
            return;
        };
        if self.ensure_below("cache").is_err() {
            return;
        }
        let _ = self.write_json(&path, entry);
    }
}

/// Whether `.rules-proposals/` is already ignored by the project's `.gitignore`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IgnoreState {
    pub ignored: bool,
    pub entry: &'static str,
    /// The project has a `.git` entry, so an ignore line means something.
    pub git: bool,
}

fn is_ignore_line(line: &str) -> bool {
    matches!(
        line.trim(),
        ".rules-proposals" | ".rules-proposals/" | "/.rules-proposals" | "/.rules-proposals/"
    )
}

pub fn ignore_state(root: &Path) -> IgnoreState {
    let ignored = fs::read_to_string(root.join(".gitignore"))
        .map(|t| t.lines().any(is_ignore_line))
        .unwrap_or(false);
    IgnoreState {
        ignored,
        entry: IGNORE_ENTRY,
        git: root.join(".git").exists(),
    }
}

/// Append the ignore line to `.gitignore` (made if missing). Only ever called
/// for a yes: the run itself writes nothing outside `.rules-proposals/`.
pub fn ignore_add(root: &Path) -> Result<IgnoreState, AppError> {
    if ignore_state(root).ignored {
        return Ok(ignore_state(root));
    }
    let path = root.join(".gitignore");
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(AppError::new("err.path.notInside").with("path", ".gitignore"));
    }
    let mut text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(path_err("err.path.read", &path, e)),
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(IGNORE_ENTRY);
    text.push('\n');
    crate::write_atomic(&path, text).map_err(|e| path_err("err.path.write", &path, e))?;
    Ok(ignore_state(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::engine::{PlanBatch, PlanRule};
    use crate::testutil::fresh_dir;

    #[test]
    fn the_key_is_the_sha256_the_neovim_cache_computes() {
        // sha256("S\0P\0ollama\0m"), computed independently of this code.
        assert_eq!(
            request_key("S", "P", Backend::Ollama, "m"),
            sha256_hex(&["S", "P", "ollama", "m"])
        );
        assert_eq!(
            sha256_hex(&["abc"]),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Every part decides the key.
        let k = request_key("S", "P", Backend::Ollama, "m");
        for other in [
            request_key("S2", "P", Backend::Ollama, "m"),
            request_key("S", "P2", Backend::Ollama, "m"),
            request_key("S", "P", Backend::Openai, "m"),
            request_key("S", "P", Backend::Ollama, "m2"),
        ] {
            assert_ne!(k, other);
        }
        // And the NUL keeps ("a","bc") apart from ("ab","c").
        assert_ne!(sha256_hex(&["a", "bc"]), sha256_hex(&["ab", "c"]));
    }

    fn prompt(rule_a: &str, rule_b: &str, file: &str) -> String {
        format!(
            "Rules to check:\n\n### A-1 (error)\nQuestion: q\n{rule_a}\n\n### B-2 (warn)\nQuestion: q\n{rule_b}\n\nFiles:\n\n=== a.lua ===\n1: {file}"
        )
    }

    #[test]
    fn each_rule_block_is_hashed_on_its_own() {
        let ids = vec!["A-1".to_string(), "B-2".to_string()];
        let base = rule_hashes(&prompt("text a", "text b", "x"), &ids);
        assert_eq!(base.len(), 2);
        let edit_a = rule_hashes(&prompt("text a EDITED", "text b", "x"), &ids);
        assert_ne!(base["A-1"], edit_a["A-1"]);
        assert_eq!(base["B-2"], edit_a["B-2"]);
        let edit_file = rule_hashes(&prompt("text a", "text b", "y"), &ids);
        assert_eq!(base, edit_file, "a file change is not a rule change");
        // Not a prompt of the known shape: nothing to compare by.
        assert!(rule_hashes("something else", &ids).is_empty());
    }

    fn plan_with(prompt: String) -> Plan {
        Plan {
            batches: vec![PlanBatch {
                id: "b1".into(),
                system: "SYS".into(),
                prompt,
                rules: ["A-1", "B-2"]
                    .iter()
                    .map(|id| PlanRule {
                        id: id.to_string(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn run_for(plan: &Plan) -> Run {
        let b = &plan.batches[0];
        let ids: Vec<String> = b.rules.iter().map(|r| r.id.clone()).collect();
        let hashes = rule_hashes(&b.prompt, &ids);
        let key = request_key(&b.system, &b.prompt, Backend::Ollama, "m");
        Run {
            model: "m".into(),
            proposals: ids
                .iter()
                .map(|id| ProposalRec {
                    rule_id: id.clone(),
                    key: key.clone(),
                    rule_hash: hashes.get(id).cloned(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn an_edited_rule_is_outdated_and_changed_files_are_told_apart() {
        let then = plan_with(prompt("text a", "text b", "x"));
        let run = run_for(&then);

        let same = outdated(&run, &then);
        assert!(same.values().all(|f| *f == Freshness::Current));

        let edited = outdated(&run, &plan_with(prompt("text a EDITED", "text b", "x")));
        assert_eq!(edited["A-1"], Freshness::Rule);
        // B-2's block is unchanged, but it shares the request, whose bytes changed.
        assert_eq!(edited["B-2"], Freshness::Files);

        let files = outdated(&run, &plan_with(prompt("text a", "text b", "y")));
        assert_eq!(files["A-1"], Freshness::Files);
        assert_eq!(files["B-2"], Freshness::Files);

        let mut gone = plan_with(prompt("text a", "text b", "x"));
        gone.batches[0].rules.truncate(1);
        let gone = outdated(&run, &gone);
        assert_eq!(gone["B-2"], Freshness::Gone);
    }

    #[test]
    fn only_plain_ids_become_folder_names() {
        assert!(valid_id("r1730-12-0"));
        for bad in ["", "..", "a/b", "a\\b", "a b", "a.b", &"x".repeat(81)] {
            assert!(!valid_id(bad), "{bad}");
        }
        assert!(valid_id(&new_id()));
        assert_ne!(new_id(), new_id());
        let s = Store::new(Path::new("."));
        assert_eq!(s.load("../x").unwrap_err().key(), "err.agent.noRun");
    }

    /// The cost of the record of a large run, written as the runner writes it: three saves per batch
    /// (asking, paid, done), eight rules per batch, and every answer kept as 10 KB of raw text in the
    /// error of each rule (an answer that is no JSON). Not a check, a measurement:
    /// `cargo test --release -- --ignored --nocapture measure_the_record_of_a_large_run`.
    #[test]
    #[ignore = "a measurement, minutes of disk writes"]
    fn measure_the_record_of_a_large_run() {
        let dir = fresh_dir("docmap-agent-store-measure");
        let s = Store::new(&dir);
        let raw = "x".repeat(10 * 1024);
        let mut run = Run {
            id: "big".into(),
            ..Default::default()
        };
        let (batches, rules) = (200, 8);
        for b in 0..batches {
            run.batches.push(BatchRec {
                id: format!("b{b}"),
                ..Default::default()
            });
        }
        let started = std::time::Instant::now();
        let mut written = 0u64;
        let mut save = |run: &Run| {
            s.save(run).unwrap();
            written += fs::metadata(dir.join(DIR).join("big").join("run.json"))
                .unwrap()
                .len();
        };
        for b in 0..batches {
            run.batches[b].state = BatchState::Asking;
            save(&run);
            save(&run);
            for r in 0..rules {
                run.set_proposal(ProposalRec {
                    rule_id: format!("R-{b}-{r}"),
                    batch: format!("b{b}"),
                    status: "error".into(),
                    error: Some(
                        serde_json::json!({ "key": "err.agent.answerShape", "detail": raw }),
                    ),
                    ..Default::default()
                });
            }
            run.batches[b].state = BatchState::Done;
            save(&run);
        }
        let elapsed = started.elapsed();
        let size = fs::metadata(dir.join(DIR).join("big").join("run.json"))
            .unwrap()
            .len();
        let listed = std::time::Instant::now();
        assert_eq!(s.list().len(), 1);
        eprintln!(
            "MEASURE batches={batches} rules={rules} final={:.1} MB written={:.2} GB in {:.1} s; list {:.0} ms",
            size as f64 / 1e6,
            written as f64 / 1e9,
            elapsed.as_secs_f64(),
            listed.elapsed().as_secs_f64() * 1e3
        );
    }

    #[test]
    fn a_run_survives_a_round_trip_and_lists_newest_first() {
        let dir = fresh_dir("docmap-agent-store-roundtrip");
        let s = Store::new(&dir);
        let mut old = Run {
            id: "r1".into(),
            title: "old".into(),
            created_ms: 1,
            ..Default::default()
        };
        old.batches.push(BatchRec {
            id: "b4".into(),
            ..Default::default()
        });
        let new = Run {
            id: "r2".into(),
            title: "new".into(),
            created_ms: 2,
            ..Default::default()
        };
        s.save(&old).unwrap();
        s.save(&new).unwrap();
        assert_eq!(s.load("r1").unwrap(), old);
        let titles: Vec<_> = s.list().iter().map(|r| r.title.clone()).collect();
        assert_eq!(titles, ["new", "old"]);
        // A folder that is not a run, and the cache folder, are not runs.
        fs::create_dir_all(dir.join(DIR).join("junk")).unwrap();
        fs::write(dir.join(DIR).join("junk").join("run.json"), "{ not json").unwrap();
        s.cache_put(
            &"ab".repeat(32),
            &CacheEntry {
                text: "t".into(),
                provider: "ollama".into(),
                model: None,
                date: "2026-01-01".into(),
            },
        );
        assert_eq!(s.list().len(), 2);
        assert_eq!(s.load("nope").unwrap_err().key(), "err.agent.noRun");
    }

    #[test]
    fn the_cache_returns_what_was_stored_and_nothing_for_a_bad_key() {
        let dir = fresh_dir("docmap-agent-store-cache");
        let s = Store::new(&dir);
        let key = request_key("S", "P", Backend::Ollama, "m");
        assert_eq!(s.cache_get(&key), None);
        let entry = CacheEntry {
            text: "the raw answer".into(),
            provider: "ollama".into(),
            model: Some("m".into()),
            date: "2026-11-02".into(),
        };
        s.cache_put(&key, &entry);
        assert_eq!(s.cache_get(&key), Some(entry.clone()));
        // A key that is not hex never reaches the filesystem.
        s.cache_put("../escape", &entry);
        assert_eq!(s.cache_get("../escape"), None);
        assert!(!dir.join("escape.json").exists());
        // A damaged entry is a miss.
        fs::write(s.cache_path(&key).unwrap(), "{ cut").unwrap();
        assert_eq!(s.cache_get(&key), None);
    }

    #[test]
    fn the_ignore_entry_is_offered_and_added_only_on_request() {
        let dir = fresh_dir("docmap-agent-store-ignore");
        let st = ignore_state(&dir);
        assert!(!st.ignored);
        assert_eq!(st.entry, ".rules-proposals/");

        fs::write(dir.join(".gitignore"), "target\n").unwrap();
        assert!(!ignore_state(&dir).ignored);
        // Saving a run writes nothing outside the proposals folder.
        Store::new(&dir)
            .save(&Run {
                id: "r1".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(".gitignore")).unwrap(),
            "target\n"
        );

        assert!(ignore_add(&dir).unwrap().ignored);
        assert_eq!(
            fs::read_to_string(dir.join(".gitignore")).unwrap(),
            "target\n.rules-proposals/\n"
        );
        // Again: no second line.
        ignore_add(&dir).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(".gitignore"))
                .unwrap()
                .matches("rules-proposals")
                .count(),
            1
        );

        // No trailing newline, and no file at all.
        fs::write(dir.join(".gitignore"), "a").unwrap();
        ignore_add(&dir).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(".gitignore")).unwrap(),
            "a\n.rules-proposals/\n"
        );
        fs::remove_file(dir.join(".gitignore")).unwrap();
        ignore_add(&dir).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(".gitignore")).unwrap(),
            ".rules-proposals/\n"
        );
        for line in [
            ".rules-proposals",
            "/.rules-proposals/",
            "  .rules-proposals/  ",
        ] {
            assert!(is_ignore_line(line), "{line}");
        }
        assert!(!is_ignore_line("# .rules-proposals/"));
    }

    /// A link at `at` to `to`; false where this system does not let the test make one.
    fn link_dir(to: &Path, at: &Path) -> bool {
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(at)
                .arg(to)
                .output()
                .is_ok_and(|o| o.status.success())
        }
        #[cfg(not(windows))]
        {
            std::os::unix::fs::symlink(to, at).is_ok()
        }
    }

    #[test]
    fn a_link_below_the_base_leads_nothing_out_of_the_project() {
        let root = fresh_dir("docmap-agent-links");
        let outside = fresh_dir("docmap-agent-links-out");
        let store = Store::new(&root);
        fs::create_dir_all(root.join(DIR)).unwrap();
        if !link_dir(&outside, &root.join(DIR).join("cache")) {
            return;
        }
        store.cache_put(
            "abc",
            &CacheEntry {
                text: "t".into(),
                provider: "p".into(),
                model: None,
                date: "d".into(),
            },
        );
        assert_eq!(
            fs::read_dir(&outside).unwrap().count(),
            0,
            "no answer outside"
        );

        assert!(link_dir(&outside, &root.join(DIR).join("r1")));
        let run = Run {
            id: "r1".into(),
            ..Default::default()
        };
        assert!(store.save(&run).is_err());
        assert_eq!(
            fs::read_dir(&outside).unwrap().count(),
            0,
            "no record outside"
        );
    }

    #[test]
    fn a_gitignore_that_is_a_link_is_not_read_into_the_project() {
        let root = fresh_dir("docmap-agent-ignore-link");
        let outside = fresh_dir("docmap-agent-ignore-out");
        let secret = outside.join("secret");
        fs::write(&secret, "SECRET\n").unwrap();
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&secret, root.join(".gitignore")).is_ok();
        #[cfg(not(windows))]
        let made = std::os::unix::fs::symlink(&secret, root.join(".gitignore")).is_ok();
        if !made {
            return;
        }
        assert!(ignore_add(&root).is_err());
        assert_eq!(fs::read_to_string(&secret).unwrap(), "SECRET\n");
    }

    #[test]
    fn a_run_is_held_by_one_at_a_time() {
        let root = fresh_dir("docmap-agent-hold");
        let store = Store::new(&root);
        assert!(!store.held("r1"));
        let first = store.hold("r1").unwrap().unwrap();
        assert!(store.held("r1"));
        assert!(store.hold("r1").unwrap().is_none());
        drop(first);
        assert!(!store.held("r1"));
        assert!(store.hold("r1").unwrap().is_some());
    }
}
