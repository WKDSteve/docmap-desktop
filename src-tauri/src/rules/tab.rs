//! What the Rules tab shows and does, without a window: the catalog with the
//! trust of each ruleset written into it, the run with exactly the trusted
//! folders, and the steps of the trust dialog.
//!
//! Every function takes the engine as [`Routes`] and the store as a file, so
//! the tests run them against a canned engine and a folder of their own.
//!
//! The window never names a folder to trust on its own authority: a path it
//! sends is looked up among the rulesets the engine lists for the project, and
//! what is trusted is the hash the dialog showed (a ruleset that changed while
//! the dialog was open is refused, not trusted as it is now).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use super::engine::Routes;
use super::trust::{self, Digest, Entry, State, Store, Verdict};
use crate::apperr::AppError;

/// A rule file larger than this is not opened to show a predicate.
const MAX_SOURCE_FILE: u64 = 4 * 1024 * 1024;
/// A predicate is shown up to this many lines.
const MAX_SOURCE_LINES: usize = 400;

/// A list the engine may encode as `[]` or, when empty, as `{}`.
fn list(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn available(v: &Value) -> bool {
    v.get("available").and_then(Value::as_bool) == Some(true)
}

/// The real path of each rule file, resolved once per file.
#[derive(Default)]
struct RealFiles(HashMap<String, Option<PathBuf>>);

impl RealFiles {
    fn of(&mut self, file: &str) -> Option<&PathBuf> {
        self.0
            .entry(file.to_string())
            .or_insert_with(|| trust::real_path(Path::new(file)).ok())
            .as_ref()
    }
}

/// One rule with a `lua_predicate` check, as the trust dialog lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Predicate {
    pub id: String,
    pub title: String,
    pub file: String,
    pub line: u64,
}

/// The predicate rules of the catalog `v` whose file lies in the ruleset at `real`.
fn predicates_in(v: &Value, real: &Path, files: &mut RealFiles) -> Vec<Predicate> {
    let mut out = Vec::new();
    for rule in list(v.get("rules")) {
        if rule.get("check").and_then(Value::as_str) != Some("lua_predicate") {
            continue;
        }
        let Some(file) = rule.get("source_file").and_then(Value::as_str) else {
            continue;
        };
        if !files.of(file).is_some_and(|f| trust::within(f, real)) {
            continue;
        }
        out.push(Predicate {
            id: text_of(rule, "id"),
            title: text_of(rule, "title"),
            file: file.to_string(),
            line: rule.get("source_line").and_then(Value::as_u64).unwrap_or(0),
        });
    }
    out
}

fn text_of(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// The store, or why there is none (and then nothing is trusted).
fn open_store(store_file: &Path) -> (Option<Store>, Option<AppError>) {
    match Store::load(store_file) {
        Ok(store) => (Some(store), None),
        Err(e) => (None, Some(e)),
    }
}

/// `rules/catalog`, with this machine's trust written into every ruleset:
///
/// - `trusted` is this side's verdict (the engine was told about no folder);
/// - `trust` is `{ state, real, predicates, by, at, problem }`, `state` one of
///   `trusted`, `untrusted`, `changed`, `unreadable`;
/// - `trustStoreError` is set when the store could not be read. Then nothing
///   is trusted, and the window says so instead of showing "untrusted" as if
///   nobody had ever decided.
///
/// An answer that is not `available` goes through as it is: its `reason` is
/// what the tab shows.
pub fn catalog(routes: &dyn Routes, root: &Path, store_file: &Path) -> Result<Value, AppError> {
    let mut v = routes.catalog(root)?;
    if !available(&v) {
        return Ok(v);
    }
    let (store, store_error) = open_store(store_file);
    let mut files = RealFiles::default();
    let mut rulesets: Vec<Value> = list(v.get("rulesets")).to_vec();
    for ruleset in &mut rulesets {
        let path = text_of(ruleset, "path");
        let verdict = trust::verdict(store.as_ref(), Path::new(&path));
        let predicates = verdict
            .real
            .as_deref()
            .map(|real| predicates_in(&v, real, &mut files).len())
            .unwrap_or(0);
        let Some(obj) = ruleset.as_object_mut() else {
            continue;
        };
        obj.insert("trusted".into(), json!(verdict.state == State::Trusted));
        obj.insert("trust".into(), trust_json(&verdict, predicates));
    }
    let Some(obj) = v.as_object_mut() else {
        return Err(AppError::new("err.rules.shape"));
    };
    obj.insert("rulesets".into(), Value::Array(rulesets));
    if let Some(e) = store_error {
        obj.insert(
            "trustStoreError".into(),
            serde_json::to_value(&e).unwrap_or(Value::Null),
        );
    }
    Ok(v)
}

fn trust_json(verdict: &Verdict, predicates: usize) -> Value {
    json!({
        "state": verdict.state,
        "real": verdict.real.as_deref().map(crate::portable),
        "predicates": predicates,
        "by": verdict.entry.as_ref().map(|e| e.by.clone()),
        "at": verdict.entry.as_ref().map(|e| e.at),
        "problem": verdict.problem.as_ref().and_then(|e| serde_json::to_value(e).ok()),
    })
}

/// The paths of the rulesets a catalog answer lists, as the engine names them.
pub fn ruleset_paths(catalog: &Value) -> Vec<String> {
    list(catalog.get("rulesets"))
        .iter()
        .map(|r| text_of(r, "path"))
        .filter(|p| !p.is_empty())
        .collect()
}

/// Of `rulesets`, the real folders that are trusted right now: in the store,
/// and with the content that was trusted.
fn trusted_now(store_file: &Path, rulesets: &[String]) -> Vec<String> {
    let (store, _) = open_store(store_file);
    rulesets
        .iter()
        .map(|path| trust::verdict(store.as_ref(), Path::new(path)))
        .filter(|v| v.state == State::Trusted)
        .filter_map(|v| v.real)
        .map(|real| crate::portable(&real))
        .collect()
}

/// `rules/run` of `select`: the engine is told to trust exactly the rulesets
/// whose content still has the hash the user trusted, looked up now, not when
/// the list was drawn. An untrusted predicate is the engine's `error` row
/// ("predicate not trusted"), never an absent one.
pub fn run(
    routes: &dyn Routes,
    root: &Path,
    store_file: &Path,
    select: &str,
) -> Result<Value, AppError> {
    let listed = routes.catalog(root)?;
    if !available(&listed) {
        return Ok(listed);
    }
    run_known(routes, root, store_file, select, &ruleset_paths(&listed))
}

/// [`run`] without asking the engine for the catalog again: `rulesets` are
/// the paths an earlier catalog of this project listed. An engine call takes
/// seconds on Windows, and a run right after the list was drawn would double
/// that for nothing.
///
/// What is decided here is decided now all the same: the store is read and
/// every ruleset is hashed again, so a file changed since the list was drawn
/// is not trusted. A list that is out of date can only cost trust, never
/// grant it: a ruleset the project has gained since is not in `rulesets` and
/// so not named to the engine, and one it has lost is named to an engine that
/// no longer loads it.
pub fn run_known(
    routes: &dyn Routes,
    root: &Path,
    store_file: &Path,
    select: &str,
    rulesets: &[String],
) -> Result<Value, AppError> {
    routes.run(root, select, &trusted_now(store_file, rulesets))
}

/// What the trust dialog shows before anything is trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustPreview {
    /// The real folder: what would be trusted, under this name.
    pub path: String,
    pub sha256: String,
    pub files: u64,
    pub bytes: u64,
    pub state: State,
    pub predicates: Vec<Predicate>,
}

/// The ruleset of the project that `path` names, by its real path, with the
/// catalog it was found in. A path that is not one of the project's rulesets
/// is refused: the window cannot make this side hash or trust any folder.
fn ruleset_of(routes: &dyn Routes, root: &Path, path: &str) -> Result<(PathBuf, Value), AppError> {
    let v = routes.catalog(root)?;
    let not_one = || AppError::new("err.rules.notRuleset").with("path", path);
    if !available(&v) {
        return Err(not_one());
    }
    let wanted = trust::real_path(Path::new(path)).map_err(|_| not_one())?;
    for ruleset in list(v.get("rulesets")) {
        let Ok(real) = trust::real_path(Path::new(&text_of(ruleset, "path"))) else {
            continue;
        };
        if trust::within(&real, &wanted) && trust::within(&wanted, &real) {
            return Ok((real, v));
        }
    }
    Err(not_one())
}

/// What trusting the ruleset at `path` would mean: its real folder, its hash,
/// and the predicate rules that would run.
pub fn preview(
    routes: &dyn Routes,
    root: &Path,
    store_file: &Path,
    path: &str,
) -> Result<TrustPreview, AppError> {
    let (real, v) = ruleset_of(routes, root, path)?;
    let Digest {
        sha256,
        files,
        bytes,
    } = trust::digest(&real)?;
    let (store, _) = open_store(store_file);
    let state = match store.as_ref().and_then(|s| s.find(&real)) {
        Some(entry) if entry.sha256 == sha256 => State::Trusted,
        Some(_) => State::Changed,
        None => State::Untrusted,
    };
    Ok(TrustPreview {
        path: crate::portable(&real),
        sha256,
        files,
        bytes,
        state,
        predicates: predicates_in(&v, &real, &mut RealFiles::default()),
    })
}

/// Trust the ruleset at `path` as the dialog showed it: `sha256` is the hash
/// of that preview. A ruleset whose content is no longer that is refused. A
/// store that cannot be read is not overwritten.
pub fn grant(
    routes: &dyn Routes,
    root: &Path,
    store_file: &Path,
    path: &str,
    sha256: &str,
    by: &str,
    at: u64,
) -> Result<(), AppError> {
    let (real, v) = ruleset_of(routes, root, path)?;
    let now = trust::digest(&real)?;
    if now.sha256 != sha256 {
        return Err(AppError::new("err.rules.trustChanged").with("path", crate::portable(&real)));
    }
    let mut store = Store::load(store_file)?;
    store.grant(Entry {
        path: crate::portable(&real),
        sha256: now.sha256,
        predicates: predicates_in(&v, &real, &mut RealFiles::default()).len() as u64,
        by: by.to_string(),
        at,
    });
    store.save(store_file)
}

/// Take the trust of the ruleset at `path` back.
pub fn revoke(
    routes: &dyn Routes,
    root: &Path,
    store_file: &Path,
    path: &str,
) -> Result<(), AppError> {
    let (real, _) = ruleset_of(routes, root, path)?;
    let mut store = Store::load(store_file)?;
    if store.revoke(&real) {
        store.save(store_file)?;
    }
    Ok(())
}

/// The source of one predicate rule, for the dialog's "show the code".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PredicateSource {
    pub id: String,
    pub file: String,
    pub line: u64,
    pub text: String,
    /// The block was longer than is shown.
    pub truncated: bool,
}

/// The rule block of the predicate rule `rule`, read from its file: from the
/// line the engine names to the fence that closes the block.
pub fn predicate_source(
    routes: &dyn Routes,
    root: &Path,
    rule: &str,
) -> Result<PredicateSource, AppError> {
    let v = routes.catalog(root)?;
    let none = || AppError::new("err.rules.noPredicate").with("rule", rule);
    let found = list(v.get("rules"))
        .iter()
        .find(|r| {
            r.get("id").and_then(Value::as_str) == Some(rule)
                && r.get("check").and_then(Value::as_str) == Some("lua_predicate")
        })
        .ok_or_else(none)?;
    let file = text_of(found, "source_file");
    let line = found
        .get("source_line")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    // Only a file of one of the project's rulesets is opened, by its real path.
    let real = trust::real_path(Path::new(&file)).map_err(|_| none())?;
    let inside = list(v.get("rulesets")).iter().any(|r| {
        trust::real_path(Path::new(&text_of(r, "path")))
            .is_ok_and(|root| trust::within(&real, &root))
    });
    if !inside {
        return Err(none());
    }
    let size = std::fs::metadata(&real)
        .map_err(|e| crate::path_err("err.path.read", &real, e))?
        .len();
    if size > MAX_SOURCE_FILE {
        return Err(crate::path_err(
            "err.path.read",
            &real,
            "the file is too large to show",
        ));
    }
    let body =
        std::fs::read_to_string(&real).map_err(|e| crate::path_err("err.path.read", &real, e))?;
    let (text, truncated) = block_from(&body, line);
    Ok(PredicateSource {
        id: rule.to_string(),
        file,
        line,
        text,
        truncated,
    })
}

/// The fenced block that starts at `line` (1-based) of `body`: up to and
/// including the line that closes the fence, or [`MAX_SOURCE_LINES`] lines.
fn block_from(body: &str, line: u64) -> (String, bool) {
    let start = usize::try_from(line.saturating_sub(1)).unwrap_or(usize::MAX);
    let mut out: Vec<&str> = Vec::new();
    for (i, text) in body.lines().skip(start).enumerate() {
        if i >= MAX_SOURCE_LINES {
            return (out.join("\n"), true);
        }
        out.push(text);
        if i > 0 && text.trim_start().starts_with("```") {
            break;
        }
    }
    (out.join("\n"), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fresh_dir;
    use std::fs;
    use std::sync::Mutex;

    const RULES: &str = "# Rules\n\n#### `P-01` one\n\n```rule\nid = \"P-01\",\ncheck = {\n  type = \"lua_predicate\",\n  fn = function(root) return true end,\n},\n```\n\n#### `F-01` two\n\n```rule\nid = \"F-01\",\n```\n";

    /// An engine that lists one ruleset folder with one predicate rule and one
    /// manual rule, and remembers what a run was told to trust.
    struct Canned {
        ruleset: PathBuf,
        seen_trust: Mutex<Vec<Vec<String>>>,
    }

    impl Canned {
        fn new(name: &str) -> (Canned, PathBuf) {
            let base = fresh_dir(name);
            let ruleset = base.join("ruleset");
            fs::create_dir_all(&ruleset).unwrap();
            fs::write(ruleset.join("RULES.md"), RULES).unwrap();
            let store = base.join("config").join(trust::STORE_FILE);
            (
                Canned {
                    ruleset,
                    seen_trust: Mutex::new(Vec::new()),
                },
                store,
            )
        }

        fn file(&self) -> String {
            crate::portable(&self.ruleset.join("RULES.md"))
        }
    }

    impl Routes for Canned {
        fn catalog(&self, _root: &Path) -> Result<Value, AppError> {
            Ok(json!({
                "available": true,
                "rulesets": [{
                    "name": "../ruleset", "origin": "project",
                    "path": crate::portable(&self.ruleset), "trusted": false
                }],
                "rules": [
                    {"id": "F-01", "title": "two", "family": "FIX", "has_check": false,
                     "source_file": self.file(), "source_line": 15},
                    {"id": "P-01", "title": "one", "family": "PRD", "has_check": true,
                     "check": "lua_predicate", "source_file": self.file(), "source_line": 5}
                ],
                "counts": {"rules": 2, "automated": 1, "manual": 1, "predicates": 1},
                "gates": {}, "errors": {}, "warnings": {}
            }))
        }

        fn run(&self, _root: &Path, select: &str, trust: &[String]) -> Result<Value, AppError> {
            self.seen_trust.lock().unwrap().push(trust.to_vec());
            Ok(json!({"available": true, "selection": select, "results": []}))
        }
    }

    fn state(v: &Value) -> String {
        v["rulesets"][0]["trust"]["state"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn trust_it(engine: &Canned, store: &Path) {
        let path = crate::portable(&engine.ruleset);
        let shown = preview(engine, Path::new("."), store, &path).unwrap();
        grant(
            engine,
            Path::new("."),
            store,
            &path,
            &shown.sha256,
            "tester",
            7,
        )
        .unwrap();
    }

    #[test]
    fn a_ruleset_is_untrusted_until_somebody_says_yes_and_a_run_is_told_nothing() {
        let (engine, store) = Canned::new("docmap-rules-tab-untrusted");
        let v = catalog(&engine, Path::new("."), &store).unwrap();
        assert_eq!(state(&v), "untrusted");
        assert_eq!(v["rulesets"][0]["trusted"], json!(false));
        assert_eq!(v["rulesets"][0]["trust"]["predicates"], json!(1));
        assert!(v.get("trustStoreError").is_none());
        run(&engine, Path::new("."), &store, "PRD").unwrap();
        assert_eq!(
            *engine.seen_trust.lock().unwrap(),
            vec![Vec::<String>::new()]
        );
    }

    #[test]
    fn a_trusted_ruleset_is_named_to_the_run_by_its_real_folder() {
        let (engine, store) = Canned::new("docmap-rules-tab-trusted");
        trust_it(&engine, &store);
        let v = catalog(&engine, Path::new("."), &store).unwrap();
        assert_eq!(state(&v), "trusted");
        assert_eq!(v["rulesets"][0]["trusted"], json!(true));
        assert_eq!(v["rulesets"][0]["trust"]["by"], json!("tester"));
        run(&engine, Path::new("."), &store, "PRD").unwrap();
        let real = crate::portable(&trust::real_path(&engine.ruleset).unwrap());
        assert_eq!(*engine.seen_trust.lock().unwrap(), vec![vec![real]]);
    }

    #[test]
    fn a_change_after_the_yes_is_seen_by_the_next_run_not_only_by_the_next_list() {
        let (engine, store) = Canned::new("docmap-rules-tab-changed");
        trust_it(&engine, &store);
        fs::write(engine.ruleset.join("RULES.md"), format!("{RULES}\nmore\n")).unwrap();
        // no catalog call of the window in between: the run decides for itself
        run(&engine, Path::new("."), &store, "PRD").unwrap();
        assert_eq!(
            *engine.seen_trust.lock().unwrap(),
            vec![Vec::<String>::new()]
        );
        assert_eq!(
            state(&catalog(&engine, Path::new("."), &store).unwrap()),
            "changed"
        );
    }

    #[test]
    fn a_run_from_a_remembered_list_still_hashes_the_ruleset_as_it_is_now() {
        let (engine, store) = Canned::new("docmap-rules-tab-known");
        trust_it(&engine, &store);
        let listed = catalog(&engine, Path::new("."), &store).unwrap();
        let paths = ruleset_paths(&listed);
        assert_eq!(paths.len(), 1);
        run_known(&engine, Path::new("."), &store, "P", &paths).unwrap();
        let real = crate::portable(&trust::real_path(&engine.ruleset).unwrap());
        // the list is from before the change, the decision is not
        fs::write(
            engine.ruleset.join("RULES.md"),
            "changed after the list was drawn
",
        )
        .unwrap();
        run_known(&engine, Path::new("."), &store, "P", &paths).unwrap();
        // a path the project never listed is hashed like any other and found untrusted
        let stranger = fresh_dir("docmap-rules-tab-known-stranger");
        fs::write(
            stranger.join("RULES.md"),
            "x
",
        )
        .unwrap();
        run_known(
            &engine,
            Path::new("."),
            &store,
            "P",
            &[crate::portable(&stranger)],
        )
        .unwrap();
        assert_eq!(
            *engine.seen_trust.lock().unwrap(),
            vec![vec![real], Vec::new(), Vec::new()]
        );
    }

    #[test]
    fn a_store_that_does_not_parse_is_said_and_trusts_nothing_and_is_not_overwritten() {
        let (engine, store) = Canned::new("docmap-rules-tab-broken");
        trust_it(&engine, &store);
        fs::write(&store, "{ broken").unwrap();
        let v = catalog(&engine, Path::new("."), &store).unwrap();
        assert_eq!(v["trustStoreError"]["key"], json!("err.rules.trustStore"));
        assert_eq!(v["rulesets"][0]["trusted"], json!(false));
        run(&engine, Path::new("."), &store, "PRD").unwrap();
        assert_eq!(
            *engine.seen_trust.lock().unwrap(),
            vec![Vec::<String>::new()]
        );

        let path = crate::portable(&engine.ruleset);
        let shown = preview(&engine, Path::new("."), &store, &path).unwrap();
        let e = grant(
            &engine,
            Path::new("."),
            &store,
            &path,
            &shown.sha256,
            "t",
            1,
        )
        .unwrap_err();
        assert_eq!(e.key(), "err.rules.trustStore");
        assert_eq!(fs::read_to_string(&store).unwrap(), "{ broken");
    }

    #[test]
    fn what_is_trusted_is_what_the_dialog_showed() {
        let (engine, store) = Canned::new("docmap-rules-tab-stale");
        let path = crate::portable(&engine.ruleset);
        let shown = preview(&engine, Path::new("."), &store, &path).unwrap();
        assert_eq!(shown.state, State::Untrusted);
        assert_eq!(shown.files, 1);
        assert_eq!(shown.predicates.len(), 1);
        assert_eq!(shown.predicates[0].id, "P-01");
        fs::write(
            engine.ruleset.join("RULES.md"),
            "swapped while the dialog was open\n",
        )
        .unwrap();
        let e = grant(
            &engine,
            Path::new("."),
            &store,
            &path,
            &shown.sha256,
            "t",
            1,
        )
        .unwrap_err();
        assert_eq!(e.key(), "err.rules.trustChanged");
        assert!(!store.exists(), "nothing was written");
    }

    #[test]
    fn a_folder_that_is_not_a_ruleset_of_the_project_is_refused() {
        let (engine, store) = Canned::new("docmap-rules-tab-foreign");
        let foreign = fresh_dir("docmap-rules-tab-foreign-other");
        fs::write(foreign.join("RULES.md"), "x\n").unwrap();
        let path = crate::portable(&foreign);
        for e in [
            preview(&engine, Path::new("."), &store, &path).unwrap_err(),
            grant(&engine, Path::new("."), &store, &path, "00", "t", 1).unwrap_err(),
            revoke(&engine, Path::new("."), &store, &path).unwrap_err(),
            // a parent of the ruleset is not the ruleset
            preview(
                &engine,
                Path::new("."),
                &store,
                &crate::portable(engine.ruleset.parent().unwrap()),
            )
            .unwrap_err(),
        ] {
            assert_eq!(e.key(), "err.rules.notRuleset");
        }
    }

    #[test]
    fn revoking_makes_it_untrusted_again() {
        let (engine, store) = Canned::new("docmap-rules-tab-revoke");
        trust_it(&engine, &store);
        let path = crate::portable(&engine.ruleset);
        revoke(&engine, Path::new("."), &store, &path).unwrap();
        assert_eq!(
            state(&catalog(&engine, Path::new("."), &store).unwrap()),
            "untrusted"
        );
    }

    #[test]
    fn the_source_of_a_predicate_is_its_rule_block() {
        let (engine, _) = Canned::new("docmap-rules-tab-source");
        let src = predicate_source(&engine, Path::new("."), "P-01").unwrap();
        assert!(
            src.text.starts_with("```rule\nid = \"P-01\""),
            "{}",
            src.text
        );
        assert!(src.text.ends_with("},\n```"), "{}", src.text);
        assert!(!src.truncated);
        assert_eq!(src.line, 5);
        // a rule without a predicate has no code to show, and neither has an unknown one
        for id in ["F-01", "N-1"] {
            let e = predicate_source(&engine, Path::new("."), id).unwrap_err();
            assert_eq!(e.key(), "err.rules.noPredicate");
        }
    }

    #[test]
    fn a_block_without_an_end_is_cut_and_says_so() {
        let body = format!("```rule\n{}", "x\n".repeat(MAX_SOURCE_LINES + 50));
        let (text, truncated) = block_from(&body, 1);
        assert!(truncated);
        assert_eq!(text.lines().count(), MAX_SOURCE_LINES);
        assert_eq!(block_from("a\nb\n", 9), (String::new(), false));
    }

    #[test]
    fn an_answer_that_is_not_available_goes_through_untouched() {
        struct No;
        impl Routes for No {
            fn catalog(&self, _: &Path) -> Result<Value, AppError> {
                Ok(json!({"available": false, "reason": "no ruleset is configured"}))
            }
            fn run(&self, _: &Path, _: &str, _: &[String]) -> Result<Value, AppError> {
                panic!("a run without a catalog must not reach the engine")
            }
        }
        let store = fresh_dir("docmap-rules-tab-none").join(trust::STORE_FILE);
        let v = catalog(&No, Path::new("."), &store).unwrap();
        assert_eq!(v["reason"], json!("no ruleset is configured"));
        assert_eq!(run(&No, Path::new("."), &store, "X").unwrap(), v);
    }

    /// The trust list stays on this machine: nothing in `rules/` can reach a
    /// network, and the agent run, which does talk to ai-desktop, never reads
    /// the store.
    #[test]
    fn the_trust_store_is_out_of_reach_of_anything_that_sends() {
        let rules = [
            include_str!("mod.rs"),
            include_str!("trust.rs"),
            include_str!("engine.rs"),
            include_str!("commands.rs"),
        ];
        for src in rules {
            let code: String = src
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            for word in ["ureq", "agent::client", "TcpStream", "http://"] {
                assert!(!code.contains(word), "rules/ must not use {word}");
            }
        }
        let agent = [
            include_str!("../agent/mod.rs"),
            include_str!("../agent/client.rs"),
            include_str!("../agent/commands.rs"),
            include_str!("../agent/engine.rs"),
            include_str!("../agent/runner.rs"),
            include_str!("../agent/store.rs"),
        ];
        for src in agent {
            for word in ["rules::trust", "rules-trust", "STORE_FILE", "sha256\":"] {
                assert!(
                    !src.contains(word),
                    "agent/ must not touch the trust store ({word})"
                );
            }
        }
    }

    /// The bundled engine, when this checkout has a real one beside it (a
    /// developer's machine; CI stages an empty placeholder and has none).
    fn real_engine() -> Option<super::super::engine::CliRoutes> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
        let exe = fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("docmap-"))
                    && fs::metadata(p).is_ok_and(|m| m.len() > 0)
            })?;
        // No vault and no trust from the environment of whoever runs the tests.
        super::super::engine::CliRoutes::new(exe.to_string_lossy().into_owned(), None)
            .ok()
            .map(|e| e.with_env("RULES_VAULT", "").with_env("RULES_TRUST", ""))
    }

    /// A project with a ruleset beside it: one predicate that passes, one that
    /// asks for two gigabytes in a single call, and one grep that fails.
    fn real_project(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        // Under the build folder, not the temp folder: the standalone engine does not come back from a project
        // below `AppData\Local\Temp` on Windows (measured 2026-11-14, more than 60 s; under Neovim and on
        // another drive it answers at once). And as the app stores a root: no `\\?\` in front.
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("rules-fixtures")
            .join(name);
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let base = PathBuf::from(crate::portable(&fs::canonicalize(&base).unwrap()));
        let project = base.join("project");
        let ruleset = base.join("ruleset");
        fs::create_dir_all(project.join("lua")).unwrap();
        fs::create_dir_all(&ruleset).unwrap();
        fs::write(
            project.join(".rules.json"),
            "{ \"rulesets\": [\"../ruleset\"] }\n",
        )
        .unwrap();
        fs::write(project.join("lua/a.lua"), "local x = FORBIDDEN\n").unwrap();
        fs::write(
            ruleset.join("RULES.md"),
            concat!(
                "# Rules\n\n",
                "#### `P-01` a predicate that passes\n\n",
                "```rule\nid = \"P-01\",\nseverity = \"recommended\",\ncheck = {\n",
                "  type = \"lua_predicate\",\n  fn = function(root)\n    return true\n  end,\n},\n```\n\n",
                "#### `P-02` a predicate that takes all the memory\n\n",
                "```rule\nid = \"P-02\",\nseverity = \"recommended\",\ncheck = {\n",
                "  type = \"lua_predicate\",\n  fn = function(root)\n",
                "    local big = (\"x\"):rep(2e9)\n    return #big > 0\n  end,\n},\n```\n\n",
                "#### `Q-01` a word that must not appear\n\n",
                "```rule\nid = \"Q-01\",\nseverity = \"recommended\",\ncheck = {\n",
                "  type = \"grep\",\n  pattern = \"FORBIDDEN\",\n  include = \"%.lua$\",\n},\n```\n",
            ),
        )
        .unwrap();
        (
            project,
            ruleset,
            base.join("config").join(trust::STORE_FILE),
        )
    }

    fn statuses(v: &Value) -> HashMap<String, String> {
        list(v.get("results"))
            .iter()
            .map(|r| (text_of(r, "id"), text_of(r, "status")))
            .collect()
    }

    #[test]
    fn with_the_real_engine_a_predicate_runs_only_once_its_ruleset_is_trusted() {
        let Some(engine) = real_engine() else {
            eprintln!("skipped: no real engine in src-tauri/binaries");
            return;
        };
        let (project, ruleset, store) = real_project("docmap-rules-real-trust");
        let listed = catalog(&engine, &project, &store).unwrap();
        assert_eq!(listed["available"], json!(true), "{listed}");
        assert_eq!(state(&listed), "untrusted");
        assert_eq!(listed["rulesets"][0]["trust"]["predicates"], json!(2));

        // not trusted: the predicate is an error row that says so, the grep still runs
        let before = run(&engine, &project, &store, "P-01,Q-01").unwrap();
        let st = statuses(&before);
        assert_eq!(st["P-01"], "error", "{before}");
        assert_eq!(st["Q-01"], "fail", "{before}");
        assert!(
            before.to_string().contains("predicate not trusted"),
            "{before}"
        );

        let path = text_of(&listed["rulesets"][0], "path");
        let shown = preview(&engine, &project, &store, &path).unwrap();
        assert_eq!(shown.predicates.len(), 2);
        grant(&engine, &project, &store, &path, &shown.sha256, "t", 1).unwrap();
        let after = run(&engine, &project, &store, "P-01").unwrap();
        assert_eq!(statuses(&after)["P-01"], "pass", "{after}");

        // the code the dialog shows is the block of the rule
        let src = predicate_source(&engine, &project, "P-01").unwrap();
        assert!(src.text.contains("return true"), "{}", src.text);

        // a changed file: asked again, and until then the predicate does not run
        fs::write(ruleset.join("extra.md"), "more\n").unwrap();
        let changed = run(&engine, &project, &store, "P-01").unwrap();
        assert_eq!(statuses(&changed)["P-01"], "error", "{changed}");
        assert_eq!(
            state(&catalog(&engine, &project, &store).unwrap()),
            "changed"
        );
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn with_the_real_engine_a_predicate_that_takes_all_the_memory_ends_as_an_error() {
        let Some(engine) = real_engine() else {
            eprintln!("skipped: no real engine in src-tauri/binaries");
            return;
        };
        let (project, _, store) = real_project("docmap-rules-real-memory");
        let listed = catalog(&engine, &project, &store).unwrap();
        let path = text_of(&listed["rulesets"][0], "path");
        let shown = preview(&engine, &project, &store, &path).unwrap();
        grant(&engine, &project, &store, &path, &shown.sha256, "t", 1).unwrap();

        let started = std::time::Instant::now();
        // Either the engine reports the rule as an error (Lua could not get the memory), or
        // the engine itself was ended and the call says so: never a pass, never a hang.
        match run(&engine, &project, &store, "P-02") {
            Ok(v) => assert_eq!(statuses(&v)["P-02"], "error", "{v}"),
            Err(e) => assert!(
                ["err.rules.engine", "err.rules.timeout"].contains(&e.key()),
                "{e}"
            ),
        }
        assert!(started.elapsed() < super::super::engine::RUN_TIMEOUT);
    }
}
