//! What the run asks of the engine: `rules/plan` and `rules/validate`.
//!
//! Both are pure routes of the standalone engine (`docmap.nvim`
//! `core/rules_api.lua`, wiring `rules.nvim`'s `engine/agent`), so the prompt a
//! model receives and the check of every quote against the tree are the same
//! code in Neovim and here. Nothing is rebuilt on this side. [`Engine`] is the
//! seam: the real one starts the binary, the tests bring a canned one.
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
    /// object, never a verdict.
    fn validate(&self, root: &Path, rule: &str, answer: &str) -> Result<Value, AppError>;
}

/// The standalone `docmap` binary, one process per call.
pub struct CliEngine {
    program: String,
    grammars: Option<String>,
}

impl CliEngine {
    /// Checked before the first call, for the reason `server.rs`'s
    /// `engine_supports_api` gives: an engine from before `--api=` would
    /// *generate the map* on an unknown argument. `--capabilities` is safe on
    /// every version (an old one exits 2 before doing anything), and the run
    /// needs `api` and `rules-agent`.
    pub fn new(program: String, grammars: Option<String>) -> Result<CliEngine, AppError> {
        let engine = CliEngine { program, grammars };
        let mut cmd = engine.command();
        cmd.arg("--capabilities");
        let out = proc::run_with_timeout(&mut cmd, PROBE_TIMEOUT)
            .map_err(|e| e.into_app(&engine.program))?;
        let caps: Vec<String> = out
            .status
            .success()
            .then(|| serde_json::from_slice::<Value>(&out.stdout).ok())
            .flatten()
            .and_then(|v| v.get("capabilities").cloned())
            .and_then(|c| serde_json::from_value(c).ok())
            .unwrap_or_default();
        if !(caps.iter().any(|c| c == "api") && caps.iter().any(|c| c == "rules-agent")) {
            return Err(AppError::new("err.agent.engineOld").with("engine", &engine.program));
        }
        Ok(engine)
    }

    fn command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.program);
        if let Some(g) = &self.grammars {
            cmd.env("DOCMAP_TS_DIR", g);
        }
        crate::server::no_window(&mut cmd);
        cmd
    }

    fn call(&self, root: &Path, route: &str, flags: &[String]) -> Result<Value, AppError> {
        let mut cmd = self.command();
        cmd.arg(root).arg(format!("--api={route}")).args(flags);
        let out = proc::run_with_timeout(&mut cmd, CALL_TIMEOUT)
            .map_err(|e| e.into_app(&self.program))?;
        if !out.status.success() {
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

    fn validate(&self, root: &Path, rule: &str, answer: &str) -> Result<Value, AppError> {
        // A file, not stdin (the engine's own choice: a host that forgets to
        // close a pipe would leave it waiting for ever). Removed when done.
        let file = TempAnswer::write(answer)?;
        let v = self.call(
            root,
            "rules/validate",
            &[
                format!("--select={rule}"),
                format!("--answer={}", file.path().display()),
            ],
        )?;
        unavailable(&v)?;
        v.get("proposal")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| AppError::new("err.agent.engineShape"))
    }
}

/// An answer on disk for the length of one `rules/validate` call.
pub struct TempAnswer(std::path::PathBuf);

impl TempAnswer {
    pub fn write(text: &str) -> Result<TempAnswer, AppError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "docmap-answer-{}-{}-{}.txt",
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
    fn a_plan_of_the_wrong_shape_is_named_as_such() {
        let e = Plan::from_engine(&json!({"available": true, "batches": "nope"})).unwrap_err();
        assert_eq!(e.key(), "err.agent.engineShape");
    }

    #[test]
    fn the_answer_file_holds_the_text_and_goes_away() {
        let text = "{\"results\": []}\nline two";
        let file = TempAnswer::write(text).unwrap();
        let path = file.path().to_path_buf();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        drop(file);
        assert!(!path.exists());
    }
}
