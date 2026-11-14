//! The one error shape that crosses the IPC boundary.
//!
//! # The decision
//!
//! Every command the window invokes fails with an [`AppError`], never with a
//! `String`. On the wire it is
//!
//! ```json
//! { "key": "err.path.notDirectory",
//!   "params": { "path": "C:/x" },
//!   "detail": "Access is denied. (os error 5)",
//!   "message": "C:/x is not a folder" }
//! ```
//!
//! - `key` is a stable catalog key (`src/lib/i18n.js`), and the window renders
//!   `t(key)` with `{name}` filled from `params`. The sentence is the
//!   interface's, so it is translated.
//! - `params` are named facts the sentence carries: paths, names, numbers. They
//!   are the subject and are never translated (rule 1 of the catalog).
//! - `detail` is somebody else's text, verbatim and untranslated: the OS error,
//!   `git`'s or `gh`'s stderr, the engine's or Neovim's own report. The window
//!   shows it after the sentence, separated by ` — `.
//! - `message` is the English sentence built from [`ENGLISH`], with the detail
//!   attached the same way. It is what the window falls back to (visibly
//!   marked) for a key its catalog does not know, what a log line shows, and
//!   what `Display` prints. Never a raw key.
//!
//! [`ENGLISH`] is the list of keys. `src/lib/errors.test.js` reads it out of
//! this file and holds it against the catalog both ways, sentence for
//! sentence, so a key renamed or reworded on one side is a failing test; the
//! test module below holds it against every key the Rust source uses.
//!
//! # Two namespaces, two catalogs
//!
//! `err.*` keys are the window's: they live in this app's catalog. `api.*` keys
//! are the answers of the page host (`server.rs`) to the generated page's
//! `/api/*` requests. That page has its own catalog (docmap.nvim's) and
//! reads the English sentence from the `error` field of the body; the key,
//! params and detail ride along in the same shape so its catalog can take them
//! up, but they are never put into this app's catalog, and the window never
//! renders one.

use serde::ser::{Serialize, SerializeMap, SerializeStruct, Serializer};
use std::fmt;

/// The English sentence of every key. The catalog's `en` entry must be the
/// same text, character for character (`src/lib/errors.test.js`).
///
/// One entry per line, a plain string with no quote or backslash in it: the
/// test reads this table with a regular expression. Backticks quote a value.
pub const ENGLISH: &[(&str, &str)] = &[
    // Projects and the files this app keeps.
    ("err.project.unknown", "No such project: {id}"),
    ("err.config.noDir", "This system names no folder for the app's settings"),
    ("err.workspace.unreadable", "{path} is not readable as a workspace"),
    ("err.workspace.listUnreadable", "{path} is not readable as a project list"),
    ("err.workspace.serialise", "The settings could not be put into words for saving"),
    ("err.workspace.exists", "A workspace named {name} already exists"),
    ("err.workspace.last", "The last workspace cannot be deleted"),
    ("err.lock", "An earlier failure left the app unable to save safely. Restart it"),
    ("err.task", "A background task of the app failed"),
    // Files and folders.
    ("err.path.notDirectory", "{path} is not a folder"),
    ("err.path.notFile", "{path} is not a file"),
    ("err.path.resolve", "Cannot resolve {path}"),
    ("err.path.read", "Cannot read {path}"),
    ("err.path.write", "Cannot write {path}"),
    ("err.path.create", "Cannot create {path}"),
    ("err.path.delete", "Cannot delete {path}"),
    ("err.path.rename", "Cannot rename {from} to {to}"),
    // A path inside a project (`resolve_inside`).
    ("err.path.nul", "A path cannot contain a NUL character"),
    ("err.path.tooLong", "That path is too long"),
    ("err.path.notInside", "{path} is not a path inside the project"),
    ("err.path.outside", "{path} resolves outside the project"),
    ("err.path.missing", "{path} is not in this project"),
    ("err.path.network", "{path} goes through a link to another machine"),
    ("err.path.tooManyLinks", "{path} goes through too many links (or a link loop, or a very deep path)"),
    ("err.path.unsafeLink", "{path} goes through a link that cannot be followed safely"),
    // The map directory.
    ("err.outDir.unsafe", "The map directory must be a folder inside the project, not `{value}` (only letters, digits, `-`, `_` and `.` in names, no `.` or `..` component, no absolute path; on Windows a name must not end in a dot)"),
    ("err.outDir.stored", "Project settings: the map directory must be a folder inside the project, not `{value}` (only letters, digits, `-`, `_` and `.` in names, no `.` or `..` component, no absolute path; on Windows a name must not end in a dot). Change it there"),
    ("err.outDir.link", "{dir} is a link, so the map is neither written to nor read from where it leads. Replace it with a plain folder"),
    ("err.outDir.fileLink", "{file} is a link that leaves the project, goes to another machine or cannot be checked (its target is not there), so the map is neither written to nor read from where it leads. Replace it with a plain file"),
    ("err.map.notJson", "{path} is not JSON"),
    // Importing from a URL.
    ("err.url.empty", "Cannot import this URL: it is empty or too long"),
    ("err.url.space", "Cannot import this URL: it contains a space or a control character"),
    ("err.url.dash", "Cannot import this URL: it starts with `-`, which git would read as an option"),
    ("err.url.helper", "Cannot import this URL: it names a remote helper (`::`), which is not supported"),
    ("err.url.scheme", "Cannot import this URL: it must start with https://, http://, ssh://, git:// or look like user@host:path"),
    ("err.url.noName", "No plain folder name can be made from this URL"),
    ("err.url.slotsTaken", "Every folder for this clone ({base} and its overflow) is taken"),
    ("err.url.outsideCache", "The folder for this clone would lie outside the cache"),
    ("err.git.cloneFailed", "git clone failed"),
    // Running other programs.
    ("err.proc.run", "Could not run {program}"),
    ("err.proc.timeout", "{program} gave no answer within {secs} s"),
    ("err.open.failed", "Could not open {target}"),
    ("err.open.reveal", "Could not show {path}"),
    // The engine.
    ("err.engine.none", "No docmap engine configured. It is docmap.nvim's standalone binary — put it on PATH, or point at it in the sidebar"),
    ("err.engine.notJson", "{program} answered --capabilities with something that is not JSON"),
    // Neovim.
    ("err.nvim.none", "No nvim binary configured. Put it on PATH, or point at it in Settings"),
    ("err.nvim.noConfig", "No Neovim config folder configured, and none found at the default location"),
    ("err.nvim.noCache", "Neovim could not be asked for its cache folder"),
    ("err.nvim.timeout", "{program} gave no answer within {secs} s — does the Neovim configuration wait for input?"),
    ("err.nvim.noExport", "The Neovim config in {dir} has no plugins.personal.core.export module, so there is no plugin list to read. Add the config itself with the Folder tab"),
    ("err.nvim.exit", "nvim exited with code {code}"),
    ("err.nvim.notJson", "nvim's output is not the JSON list this import reads"),
    ("err.telemetry.failed", "Neovim could not switch telemetry for {namespace}"),
    // GitHub traffic.
    ("err.traffic.emptyAnswer", "Neovim answered with an empty path"),
    ("err.traffic.noPlugin", "github_stats.nvim could not be loaded in Neovim"),
    ("err.traffic.noAnswer", "Neovim did not answer"),
    ("err.traffic.notFolder", "Not a folder"),
    ("err.traffic.tooLarge", "{file} is larger than {mib} MiB"),
    ("err.traffic.unreadable", "{file} could not be read"),
    ("err.traffic.notJson", "{file} is not JSON"),
    ("err.traffic.digestNotJson", "The digest is not JSON"),
    ("err.traffic.noSchema", "The digest has no schema field"),
    ("err.traffic.shape", "The digest does not have the expected shape"),
    ("err.traffic.schema", "Schema {schema}, this app understands up to {known}"),
    ("err.traffic.rootNewer", "root.json is newer than this app understands"),
    ("err.traffic.digestDirIgnored", "root.json's digest_dir ({dir}) is not an absolute local path, so it is ignored"),
    ("err.traffic.noDigests", "root.json points to a folder with no digests"),
    ("err.traffic.nothing", "No root.json and no digest folder"),
    // The rules agent: ai-desktop, the engine's plan, and the run record.
    ("err.agent.unreachable", "ai-desktop does not answer at {url}. Is it running, and is this the port it listens on?"),
    ("err.agent.origin", "ai-desktop at {url} refused the request as cross-origin (403). The request must come from the app, not from a page"),
    ("err.agent.rejected", "ai-desktop did not accept the request (status {status})"),
    ("err.agent.backend", "ai-desktop's backend failed (status {status})"),
    ("err.agent.timeout", "ai-desktop gave no answer within {secs} s"),
    ("err.agent.malformed", "ai-desktop's answer is not the JSON that /ask promises"),
    ("err.agent.stopped", "The request was stopped"),
    ("err.agent.url", "The ai-desktop address must be http://localhost, http://127.0.0.1 or http://[::1] with a port, not `{url}`"),
    ("err.agent.model", "A model is required: its name decides which backend answers"),
    ("err.agent.localOnly", "{model} would go to {backend}, a cloud backend, and this project allows local models only. Add `{backend}` to agent.backends in .rules.json to allow it"),
    ("err.agent.notAllowed", "{model} would go to {backend}, which this project does not allow. agent.backends in .rules.json allows: {allowed}"),
    ("err.agent.title", "A run needs a title, to be found again later"),
    ("err.agent.selection", "Nothing is selected, or the selection is too long. Name rule ids, families or gate:<name>"),
    ("err.agent.nothing", "Nothing to ask: no rule of the selection can be sent ({skipped} not sent)"),
    ("err.agent.engine", "The docmap engine could not plan or check the rules"),
    ("err.agent.engineOld", "{engine} is a docmap engine without the rules agent. Rebuild it from docmap.nvim, or point at a newer one"),
    ("err.agent.engineShape", "The docmap engine answered the rules agent with something unexpected"),
    ("err.agent.answerUnreadable", "The docmap engine could not read the model's answer, so no rule of the batch was checked. The batch stays to do: resume the run"),
    ("err.agent.noRun", "No such run: {id}"),
    ("err.agent.running", "Run {id} is already working"),
    ("err.agent.notResumable", "Run {id} cannot be resumed: it was cancelled or is finished"),
    ("err.agent.planChanged", "This run cannot be resumed: {file} is no longer sent. Start a new run"),
    ("err.agent.runUnreadable", "{path} is not readable as a run record"),
    // Everything else the window asks for.
    ("err.search.mode", "Unknown search mode: {mode}"),
    ("err.editor.noFile", "No file was named"),
    ("err.editor.empty", "The editor command is empty"),
    ("err.docs.page", "No such documentation page: {page}"),
    ("err.docs.query", "Type a word to search the documentation for"),
    ("err.feedback.repo", "No such repository: {repo}"),
    ("err.feedback.topic", "No such feedback topic: {topic}"),
    ("err.window.none", "The main window is gone"),
    ("err.window.failed", "The window refused the change"),
    ("err.menu.labels", "The menu cannot be built: labels are missing for {keys}"),
    ("err.menu.build", "The menu cannot be built"),
    ("err.server.start", "The map could not be served on this machine"),
    // The page host's `/api/*` answers. The generated page's catalog, not this
    // app's: see the module header.
    ("api.method", "only GET is served"),
    ("api.badRequest", "not a request this server understands"),
    ("api.foreign", "only the page of this server may ask"),
    ("api.sha", "not a commit hash"),
    ("api.route", "no such endpoint"),
    ("api.path", "bad path"),
    ("api.notFound", "not found"),
    ("api.engine.none", "no docmap engine configured"),
    ("api.engine.old", "{engine} is an older docmap engine without --api support. Rebuild it from docmap.nvim, or point at a newer one."),
    ("api.engine.run", "could not run {engine}"),
    ("api.engine.exit", "engine exited {code}"),
    ("api.engine.notJson", "engine did not answer {route} with JSON"),
];

/// What a command fails with. See the module header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    key: &'static str,
    params: Vec<(&'static str, String)>,
    detail: Option<String>,
}

impl AppError {
    /// An error with this key and nothing filled in yet.
    pub fn new(key: &'static str) -> Self {
        debug_assert!(
            ENGLISH.iter().any(|(k, _)| *k == key),
            "{key} is not in apperr::ENGLISH"
        );
        AppError {
            key,
            params: Vec::new(),
            detail: None,
        }
    }

    /// Fill `{name}` in the sentence with `value`, verbatim.
    pub fn with(mut self, name: &'static str, value: impl fmt::Display) -> Self {
        self.params.push((name, value.to_string()));
        self
    }

    /// Attach somebody else's text (an OS error, a program's stderr). Blank
    /// text is no detail.
    pub fn detail(mut self, text: impl fmt::Display) -> Self {
        let text = text.to_string().trim().to_string();
        self.detail = (!text.is_empty()).then_some(text);
        self
    }

    /// `err.project.unknown`, the one nearly every command can fail with.
    pub fn no_project(id: &str) -> Self {
        AppError::new("err.project.unknown").with("id", id)
    }

    /// `err.path.notDirectory`.
    pub fn not_dir(path: impl fmt::Display) -> Self {
        AppError::new("err.path.notDirectory").with("path", path)
    }

    /// A failed join of a blocking task: a panic in it, or the runtime going
    /// away. The task's own words are the detail.
    pub fn task(e: impl fmt::Display) -> Self {
        AppError::new("err.task").detail(e)
    }

    // Read by the tests of every module; the app itself only sends the error whole.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn key(&self) -> &'static str {
        self.key
    }

    // Read by the tests of every module; the app itself only sends the error whole.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    // Read by the tests of every module; the app itself only sends the error whole.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn detail_text(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// The English sentence, params filled, detail attached after ` — `.
    pub fn english(&self) -> String {
        let template = ENGLISH
            .iter()
            .find(|(k, _)| *k == self.key)
            .map(|(_, s)| *s)
            .unwrap_or(self.key);
        let mut text = fill(template, &self.params);
        if let Some(d) = &self.detail {
            text.push_str(" — ");
            text.push_str(d);
        }
        text
    }

    /// For the tests that were written against the English text.
    #[cfg(test)]
    pub fn contains(&self, needle: &str) -> bool {
        self.english().contains(needle)
    }

    /// The body of a `/api/*` failure: the English sentence under `error`,
    /// which is what the generated page reads, and the same key, params and
    /// detail as a command's error beside it.
    pub fn api_body(&self) -> String {
        let mut body = serde_json::json!({
            "error": self.english(),
            "key": self.key,
            "params": self.params_map(),
        });
        if let Some(d) = &self.detail {
            body["detail"] = serde_json::Value::String(d.clone());
        }
        body.to_string()
    }

    fn params_map(&self) -> serde_json::Map<String, serde_json::Value> {
        self.params
            .iter()
            .map(|(k, v)| ((*k).to_string(), serde_json::Value::String(v.clone())))
            .collect()
    }
}

/// Every `{name}` in `template` replaced by its value, in one pass: a value
/// is the subject (a path may well contain `{x}`) and is never read for
/// placeholders itself. A placeholder nobody filled stays as it is, which is
/// visible rather than silently empty.
fn fill(template: &str, params: &[(&'static str, String)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| {
            let name = &after[..close];
            params
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (v, close))
        });
        match value {
            Some((v, close)) => {
                out.push_str(v);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.english())
    }
}

impl std::error::Error for AppError {}

struct Params<'a>(&'a [(&'static str, String)]);

impl Serialize for Params<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("AppError", 4)?;
        st.serialize_field("key", self.key)?;
        st.serialize_field("params", &Params(&self.params))?;
        st.serialize_field("detail", &self.detail)?;
        st.serialize_field("message", &self.english())?;
        st.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Rust source file of the app, by name. `include_str!` rather than
    /// a directory walk, so a test binary run from anywhere reads the same
    /// files; a module added later has to be listed here, which the count
    /// check in `every_module_is_scanned` asks for.
    const SOURCES: &[(&str, &str)] = &[
        // The rules agent is a folder of modules.
        ("agent/mod.rs", include_str!("agent/mod.rs")),
        ("agent/client.rs", include_str!("agent/client.rs")),
        ("agent/commands.rs", include_str!("agent/commands.rs")),
        ("agent/engine.rs", include_str!("agent/engine.rs")),
        ("agent/fake.rs", include_str!("agent/fake.rs")),
        ("agent/runner.rs", include_str!("agent/runner.rs")),
        ("agent/store.rs", include_str!("agent/store.rs")),
        ("apperr.rs", include_str!("apperr.rs")),
        ("contract.rs", include_str!("contract.rs")),
        ("deps.rs", include_str!("deps.rs")),
        ("engine.rs", include_str!("engine.rs")),
        ("feedback.rs", include_str!("feedback.rs")),
        ("filetree.rs", include_str!("filetree.rs")),
        ("freshness.rs", include_str!("freshness.rs")),
        ("gitcfg.rs", include_str!("gitcfg.rs")),
        ("github.rs", include_str!("github.rs")),
        ("icon.rs", include_str!("icon.rs")),
        ("import.rs", include_str!("import.rs")),
        ("languages.rs", include_str!("languages.rs")),
        ("main.rs", include_str!("main.rs")),
        ("menu.rs", include_str!("menu.rs")),
        ("nvim.rs", include_str!("nvim.rs")),
        ("paths.rs", include_str!("paths.rs")),
        ("proc.rs", include_str!("proc.rs")),
        ("safe_read.rs", include_str!("safe_read.rs")),
        ("search.rs", include_str!("search.rs")),
        ("server.rs", include_str!("server.rs")),
        ("shellopen.rs", include_str!("shellopen.rs")),
        ("stats.rs", include_str!("stats.rs")),
        ("telemetry.rs", include_str!("telemetry.rs")),
        ("testutil.rs", include_str!("testutil.rs")),
        ("traffic.rs", include_str!("traffic.rs")),
        ("traffic_cmds.rs", include_str!("traffic_cmds.rs")),
        ("views.rs", include_str!("views.rs")),
        ("workspace.rs", include_str!("workspace.rs")),
    ];

    /// What a key is made of: the catalog's own convention
    /// (`menu.file.open_browser`) plus digits. `errors.test.js` reads the
    /// table with this same class, so the two readers see the same keys.
    fn key_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '.' || c == '_'
    }

    #[test]
    fn keys_used_reads_keys_with_digits_underscores_and_dashes() {
        let src = r#"Err(AppError::new("err.open_url.failed").with("url", u));
            Err(AppError::new("err.utf8.invalid"));
            Err(AppError::new("err.x-y"));
            Err(AppError::new("err.path.read"))"#;
        assert_eq!(
            keys_used("x.rs", src),
            [
                "err.open_url.failed",
                "err.utf8.invalid",
                "err.x-y",
                "err.path.read"
            ]
        );
    }

    /// The `"err.…"` and `"api.…"` literals of one source. Of this file only
    /// the constructors count: the table is what is checked, and these tests
    /// are not a use.
    fn keys_used(name: &str, src: &str) -> Vec<String> {
        let src = if name == "apperr.rs" {
            let start = src.find("\n];\n").expect("the table ends with `];`");
            let end = src
                .find("\n#[cfg(test)]\nmod tests")
                .expect("the tests are last");
            &src[start..end]
        } else {
            src
        };
        let mut out = Vec::new();
        for prefix in ["\"err.", "\"api."] {
            let mut rest = src;
            while let Some(at) = rest.find(prefix) {
                let after = &rest[at + 1..];
                // Wider than a key may be (see `key_char`): a key with a
                // `-` in it must be read whole and then fail the table
                // lookup, not be cut short and skipped.
                let len = after
                    .find(|c: char| !(key_char(c) || c == '-'))
                    .unwrap_or(after.len());
                if after[len..].starts_with('"') {
                    out.push(after[..len].to_string());
                }
                rest = &after[len..];
            }
        }
        out
    }

    #[test]
    fn every_module_is_scanned() {
        // Folders too (`agent/`): a module in one has to be listed as well.
        fn count(dir: &std::path::Path) -> usize {
            std::fs::read_dir(dir)
                .unwrap()
                .flatten()
                .map(|e| {
                    let p = e.path();
                    if p.is_dir() {
                        count(&p)
                    } else {
                        usize::from(p.extension().is_some_and(|x| x == "rs"))
                    }
                })
                .sum()
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        assert_eq!(
            count(&dir),
            SOURCES.len(),
            "a new module must be added to SOURCES"
        );
    }

    #[test]
    fn every_key_the_source_uses_has_an_english_sentence() {
        for (name, src) in SOURCES {
            for key in keys_used(name, src) {
                assert!(
                    ENGLISH.iter().any(|(k, _)| *k == key),
                    "{name} uses {key}, which apperr::ENGLISH does not have"
                );
            }
        }
    }

    #[test]
    fn every_sentence_is_used_somewhere() {
        let used: Vec<String> = SOURCES
            .iter()
            .flat_map(|(name, src)| keys_used(name, src))
            .collect();
        for (key, _) in ENGLISH {
            assert!(
                used.iter().any(|u| u == key),
                "{key} is in apperr::ENGLISH but no source uses it"
            );
        }
    }

    #[test]
    fn page_host_keys_stay_in_the_page_host() {
        for (name, src) in SOURCES {
            if *name == "server.rs" {
                continue;
            }
            for key in keys_used(name, src) {
                assert!(
                    !key.starts_with("api."),
                    "{name} uses {key}: api.* keys are the page's, only server.rs answers it"
                );
            }
        }
    }

    #[test]
    fn the_table_is_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for (key, sentence) in ENGLISH {
            assert!(seen.insert(*key), "{key} is listed twice");
            assert!(
                key.starts_with("err.") || key.starts_with("api."),
                "{key}: keys are err.* (the window) or api.* (the page)"
            );
            assert!(
                key.chars().all(key_char),
                "{key}: a key is letters, digits, `.` and `_` (what both test readers parse)"
            );
            assert!(
                !sentence.contains('"') && !sentence.contains('\\'),
                "{key}: the JS test reads this table with a regex"
            );
            assert!(!sentence.is_empty(), "{key} has no sentence");
        }
        assert!(
            ENGLISH.len() >= 60,
            "the table lost entries: {}",
            ENGLISH.len()
        );
    }

    #[test]
    fn the_english_sentence_fills_params_and_attaches_the_detail() {
        let e = AppError::new("err.path.read")
            .with("path", "C:/x {path}")
            .detail("  Access is denied. (os error 5)\n");
        assert_eq!(
            e.english(),
            "Cannot read C:/x {path} — Access is denied. (os error 5)"
        );
        assert_eq!(e.to_string(), e.english());
        assert_eq!(e.param("path"), Some("C:/x {path}"));
        assert_eq!(e.detail_text(), Some("Access is denied. (os error 5)"));
        // A value is the subject, never read for placeholders: a folder named
        // `a{to}` stays `a{to}` even though `{to}` is filled after it.
        let e = AppError::new("err.path.rename")
            .with("from", "a{to}")
            .with("to", "b");
        assert_eq!(e.english(), "Cannot rename a{to} to b");
        // Blank detail is no detail.
        let e = AppError::new("err.lock").detail("   ");
        assert_eq!(e.detail_text(), None);
        assert!(!e.english().contains('—'));
    }

    #[test]
    fn the_wire_shape_is_key_params_detail_and_message() {
        let e = AppError::new("err.proc.timeout")
            .with("program", "nvim")
            .with("secs", 60);
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "key": "err.proc.timeout",
                "params": { "program": "nvim", "secs": "60" },
                "detail": null,
                "message": "nvim gave no answer within 60 s",
            })
        );
    }

    #[test]
    fn an_api_body_keeps_the_error_field_the_page_reads() {
        let e = AppError::new("api.engine.exit")
            .with("code", 2)
            .detail("boom");
        let v: serde_json::Value = serde_json::from_str(&e.api_body()).unwrap();
        assert_eq!(v["error"], "engine exited 2 — boom");
        assert_eq!(v["key"], "api.engine.exit");
        assert_eq!(v["params"]["code"], "2");
        assert_eq!(v["detail"], "boom");
    }
}
