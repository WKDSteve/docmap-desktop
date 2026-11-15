//! A person's decision about one proposal of a run: accept it, waive the rule,
//! or reject it.
//!
//! This is the step `RULES_AGENT_CONCEPT.md` keeps for a person (D1): an agent
//! proposes, a person decides, one proposal at a time. There is no call here
//! for several, and nothing in the app makes one.
//!
//! - **Accept** writes the proposal as the rule's verdict into
//!   `.rules-verdicts.json`, through the engine's `rules/accept`, and only for a
//!   proposal that still matches the rule and the tree: a fresh plan of the
//!   run's selection must give the request it was answered for
//!   ([`store::outdated`]). An `unclear` or failed one is not an answer and is
//!   not offered.
//! - **Waive** writes a waiver of the rule into `.rules-waivers.json` through
//!   `rules/waive`. A waiver is about the rule, not about what the model said,
//!   so an outdated proposal can still be waived.
//! - **Reject** writes nothing outside the run.
//!
//! The engine's refusal (`err.agent.refused`, its reason as the detail) leaves
//! its file as it was and is no decision: the proposal stays open. Every
//! decision the engine took, and every rejection, goes into the run's
//! `decisions.json`; a proposal with a decision is not decided again.
//!
//! The person (`by`) is what the host was told by the person, never anything
//! from the proposal.

use std::path::Path;
use std::sync::Mutex;

use serde_json::Value;

use super::engine::{Engine, Judge};
use super::now_ms;
use super::store::{self, Decision, DecisionKind, Freshness, ProposalRec, Run, Store};
use crate::apperr::AppError;

/// One decision at a time in this app: read the decisions, ask the engine,
/// write them. The engine's own writers guard their files across processes.
static DECIDE: Mutex<()> = Mutex::new(());

/// The lock of [`DECIDE`], for another step that changes a proposal (the chat's
/// `propose`): a decision never reads a proposal that is being replaced.
pub(super) fn one_at_a_time() -> Result<std::sync::MutexGuard<'static, ()>, AppError> {
    DECIDE.lock().map_err(|_| AppError::new("err.lock"))
}

/// The longest name the engine takes for `by` (`rules.engine.verdicts`).
const MAX_BY: usize = 100;
/// The longest note or reason (`rules.engine.waivers.MAX_REASON_LEN`).
const MAX_TEXT: usize = 500;
/// Paths a waiver may be limited to (`rules.engine.waivers.MAX_PATHS`).
const MAX_PATHS: usize = 50;

/// One line of text the engine will take: trimmed, not empty, no control
/// character (a line break would be a second line), at most `max` characters.
fn one_line(value: &str, max: usize, key: &'static str) -> Result<String, AppError> {
    let v = value.trim();
    if v.is_empty() || v.chars().count() > max || v.chars().any(char::is_control) {
        return Err(AppError::new(key));
    }
    reaches_the_engine(v)?;
    Ok(v.to_string())
}

/// Whether `text` arrives at the engine as it was written. On Windows the
/// standalone engine reads its command line through the ANSI code page
/// (`answer_arg` says the same of paths): measured 2026-11-15, a reason with
/// `ü` was refused as "a control character" and one with `中` was written into
/// `.rules-waivers.json` as `?`. A text that would be changed on its way is
/// refused here, before anything is written, until the engine takes these
/// texts another way.
fn reaches_the_engine(text: &str) -> Result<(), AppError> {
    if cfg!(windows) && !text.is_ascii() {
        return Err(AppError::new("err.agent.ascii").with("text", text));
    }
    Ok(())
}

/// An optional line: empty is none.
fn optional_line(
    value: Option<&str>,
    max: usize,
    key: &'static str,
) -> Result<Option<String>, AppError> {
    match value.map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => one_line(v, max, key).map(Some),
    }
}

/// `YYYY-MM-DD`, or none. Whether it is a day of the calendar is the engine's
/// question (it refuses `2026-02-30` with a reason); this only keeps out what
/// is not a date at all.
fn day(value: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let b = v.as_bytes();
    let shape = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shape {
        return Err(AppError::new("err.agent.expires").with("value", v));
    }
    Ok(Some(v.to_string()))
}

/// Paths of a waiver: relative, one per entry. The engine is handed them
/// comma-joined, so a comma cannot be part of one.
fn paths(values: &[String]) -> Result<Vec<String>, AppError> {
    let out: Vec<String> = values
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if out.len() > MAX_PATHS
        || out
            .iter()
            .any(|p| p.contains(',') || p.chars().any(char::is_control))
    {
        return Err(AppError::new("err.agent.paths"));
    }
    for p in &out {
        reaches_the_engine(p)?;
    }
    Ok(out)
}

/// The run, its proposal about `rule`, and the decisions read strictly: a
/// decision is never written over a file that could not be read.
fn open(store: &Store, run_id: &str, rule: &str) -> Result<(Run, ProposalRec), AppError> {
    let mut run = store.load(run_id)?;
    run.decisions = store.load_decisions(run_id)?;
    let rec = run
        .proposals
        .iter()
        .find(|p| p.rule_id == rule)
        .cloned()
        .ok_or_else(|| AppError::new("err.agent.noProposal").with("rule", rule))?;
    if run.decisions.contains_key(rule) {
        return Err(AppError::new("err.agent.decided").with("rule", rule));
    }
    Ok((run, rec))
}

fn remember(store: &Store, run: &mut Run, rule: &str, d: Decision) -> Result<Decision, AppError> {
    run.decisions.insert(rule.to_string(), d.clone());
    store.save_decisions(&run.id, &run.decisions)?;
    Ok(d)
}

fn text_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Accept the proposal about `rule` of run `run_id` as the rule's verdict.
pub fn accept(
    root: &Path,
    run_id: &str,
    rule: &str,
    by: &str,
    note: Option<&str>,
    engine: &dyn Engine,
    judge: &dyn Judge,
) -> Result<Decision, AppError> {
    let by = one_line(by, MAX_BY, "err.agent.by")?;
    let note = optional_line(note, MAX_TEXT, "err.agent.note")?;
    let _one = DECIDE.lock().map_err(|_| AppError::new("err.lock"))?;
    let store = Store::new(root);
    let (mut run, rec) = open(&store, run_id, rule)?;
    if !matches!(rec.status.as_str(), "violation" | "compliant") {
        return Err(AppError::new("err.agent.notAnswer").with("rule", rule));
    }
    // What the person read must be what the rule and the tree say now.
    let plan = engine.plan(root, &run.selection)?;
    match store::outdated(&run, &plan).get(rule) {
        Some(Freshness::Current) => {}
        Some(Freshness::Rule) => {
            return Err(AppError::new("err.agent.staleRule").with("rule", rule))
        }
        Some(Freshness::Gone) | None => {
            return Err(AppError::new("err.agent.staleGone").with("rule", rule))
        }
        Some(Freshness::Files) => {
            return Err(AppError::new("err.agent.staleFiles").with("rule", rule))
        }
    }
    let scratch = store.scratch(run_id)?;
    let answer = judge.accept(root, rule, &rec.proposal, &by, note.as_deref(), &scratch)?;
    let verdict = answer.get("verdict").cloned().unwrap_or(Value::Null);
    remember(
        &store,
        &mut run,
        rule,
        Decision {
            kind: DecisionKind::Accepted,
            by,
            at_ms: now_ms(),
            verdict: text_of(&verdict, "verdict").or_else(|| Some(rec.status.clone())),
            note,
            file: text_of(&answer, "file"),
            ..Default::default()
        },
    )
}

/// What the person enters to waive a rule.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WaiveArgs {
    pub by: String,
    pub reason: String,
    pub expires: Option<String>,
    pub paths: Vec<String>,
}

/// Waive `rule`, the rule of a proposal of run `run_id`.
pub fn waive(
    root: &Path,
    run_id: &str,
    rule: &str,
    args: &WaiveArgs,
    judge: &dyn Judge,
) -> Result<Decision, AppError> {
    let by = one_line(&args.by, MAX_BY, "err.agent.by")?;
    let reason = one_line(&args.reason, MAX_TEXT, "err.agent.reason")?;
    let expires = day(args.expires.as_deref())?;
    let paths = paths(&args.paths)?;
    let _one = DECIDE.lock().map_err(|_| AppError::new("err.lock"))?;
    let store = Store::new(root);
    let (mut run, _) = open(&store, run_id, rule)?;
    let answer = judge.waive(root, rule, &reason, expires.as_deref(), &paths)?;
    let waiver = answer.get("waiver").cloned().unwrap_or(Value::Null);
    let written_paths = waiver
        .get("paths")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or(paths);
    remember(
        &store,
        &mut run,
        rule,
        Decision {
            kind: DecisionKind::Waived,
            by,
            at_ms: now_ms(),
            reason: text_of(&waiver, "reason").or(Some(reason)),
            expires: text_of(&waiver, "expires").or(expires),
            paths: written_paths,
            file: text_of(&answer, "file"),
            ..Default::default()
        },
    )
}

/// Reject the proposal about `rule`: nothing is written but the run's memory of it.
pub fn reject(root: &Path, run_id: &str, rule: &str, by: &str) -> Result<Decision, AppError> {
    let by = one_line(by, MAX_BY, "err.agent.by")?;
    let _one = DECIDE.lock().map_err(|_| AppError::new("err.lock"))?;
    let store = Store::new(root);
    let (mut run, _) = open(&store, run_id, rule)?;
    remember(
        &store,
        &mut run,
        rule,
        Decision {
            kind: DecisionKind::Rejected,
            by,
            at_ms: now_ms(),
            ..Default::default()
        },
    )
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::agent::client::Backend;
    use crate::agent::engine::{Plan, PlanBatch, PlanRule};
    use crate::agent::store::{request_key, rule_hashes};
    use crate::testutil::fresh_dir;
    use serde_json::json;
    use std::fs;
    use std::sync::Mutex as StdMutex;

    fn prompt(rule_a: &str, file: &str) -> String {
        format!(
            "Rules to check:\n\n### A-1 (error)\nQuestion: q\n{rule_a}\n\n### B-2 (warn)\nQuestion: q\nb\n\nFiles:\n\n=== a.lua ===\n1: {file}"
        )
    }

    fn plan_of(prompt: String) -> Plan {
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

    /// A stored run over `plan` with a proposal per rule: `A-1` a violation,
    /// `B-2` unclear.
    fn seed(name: &str, plan: &Plan) -> std::path::PathBuf {
        let root = fresh_dir(name);
        let b = &plan.batches[0];
        let ids: Vec<String> = b.rules.iter().map(|r| r.id.clone()).collect();
        let hashes = rule_hashes(&b.prompt, &ids);
        let key = request_key(&b.system, &b.prompt, Backend::Ollama, "m");
        let rec = |id: &str, status: &str| ProposalRec {
            rule_id: id.into(),
            batch: "b1".into(),
            key: key.clone(),
            rule_hash: hashes.get(id).cloned(),
            status: status.into(),
            proposal: json!({"rule_id": id, "status": status, "proposed": true, "verified": false}),
            ..Default::default()
        };
        Store::new(&root)
            .save(&Run {
                id: "r1".into(),
                selection: "A-1,B-2".into(),
                model: "m".into(),
                state: store::RunState::Done,
                proposals: vec![rec("A-1", "violation"), rec("B-2", "unclear")],
                ..Default::default()
            })
            .unwrap();
        root
    }

    struct PlanAs(Plan);
    impl Engine for PlanAs {
        fn plan(&self, _: &Path, _: &str) -> Result<Plan, AppError> {
            Ok(self.0.clone())
        }
        fn validate(&self, _: &Path, _: &str, _: &str, _: &Path) -> Result<Value, AppError> {
            unreachable!("a decision validates nothing")
        }
    }

    /// What `accept` was handed: rule, proposal, person, note, and whether the proposal file was in the run's folder.
    type Accepted = (String, Value, String, Option<String>, bool);
    /// What `waive` was handed: rule, reason, last day, paths.
    type Waived = (String, String, Option<String>, Vec<String>);

    /// The writers, recording what they were handed; `refuse` answers as the
    /// engine does when it says no.
    #[derive(Default)]
    struct Writer {
        accepted: StdMutex<Vec<Accepted>>,
        waived: StdMutex<Vec<Waived>>,
        refuse: Option<&'static str>,
    }
    impl Judge for Writer {
        fn accept(
            &self,
            root: &Path,
            rule: &str,
            proposal: &Value,
            by: &str,
            note: Option<&str>,
            scratch: &Path,
        ) -> Result<Value, AppError> {
            let inside = scratch.starts_with(root.join(store::DIR));
            self.accepted.lock().unwrap().push((
                rule.into(),
                proposal.clone(),
                by.into(),
                note.map(str::to_string),
                inside,
            ));
            if let Some(why) = self.refuse {
                return Err(AppError::new("err.agent.refused").detail(why));
            }
            Ok(
                json!({"available": true, "written": true, "file": ".rules-verdicts.json", "id": rule,
                "verdict": {"verdict": proposal["status"], "by": by, "date": "2026-11-15"}}),
            )
        }
        fn waive(
            &self,
            _: &Path,
            rule: &str,
            reason: &str,
            expires: Option<&str>,
            paths: &[String],
        ) -> Result<Value, AppError> {
            self.waived.lock().unwrap().push((
                rule.into(),
                reason.into(),
                expires.map(str::to_string),
                paths.to_vec(),
            ));
            if let Some(why) = self.refuse {
                return Err(AppError::new("err.agent.refused").detail(why));
            }
            Ok(
                json!({"available": true, "written": true, "outcome": "waived", "file": ".rules-waivers.json",
                "id": rule, "waiver": {"reason": reason, "expires": expires}}),
            )
        }
    }

    #[test]
    fn an_accepted_proposal_is_written_by_the_engine_once_and_remembered() {
        let plan = plan_of(prompt("a", "x"));
        let root = seed("docmap-decide-accept", &plan);
        let w = Writer::default();
        let d = accept(
            &root,
            "r1",
            "A-1",
            "  A Person ",
            Some("looked at it"),
            &PlanAs(plan.clone()),
            &w,
        )
        .unwrap();
        assert_eq!(d.kind, DecisionKind::Accepted);
        assert_eq!(d.by, "A Person");
        assert_eq!(d.verdict.as_deref(), Some("violation"));
        assert_eq!(d.file.as_deref(), Some(".rules-verdicts.json"));
        let seen = w.accepted.lock().unwrap().clone();
        assert_eq!(seen.len(), 1);
        let (rule, proposal, by, note, inside) = &seen[0];
        assert_eq!(rule, "A-1");
        assert_eq!(proposal["status"], json!("violation"));
        assert_eq!(by, "A Person");
        assert_eq!(note.as_deref(), Some("looked at it"));
        assert!(
            inside,
            "the proposal file goes into the run's folder, not the temp folder"
        );

        // the run remembers, run.json does not carry it, and a second decision is refused
        let run = Store::new(&root).load("r1").unwrap();
        assert_eq!(run.decisions["A-1"].kind, DecisionKind::Accepted);
        let raw = fs::read_to_string(root.join(store::DIR).join("r1").join("run.json")).unwrap();
        assert!(!raw.contains("decisions"), "{raw}");
        let again = accept(&root, "r1", "A-1", "B", None, &PlanAs(plan.clone()), &w).unwrap_err();
        assert_eq!(again.key(), "err.agent.decided");
        assert_eq!(
            reject(&root, "r1", "A-1", "B").unwrap_err().key(),
            "err.agent.decided"
        );
        assert_eq!(w.accepted.lock().unwrap().len(), 1);
        // nothing of the kind is written by this side
        assert!(!root.join(".rules-verdicts.json").exists());
        assert!(!root.join(".rules-waivers.json").exists());
    }

    #[test]
    fn an_outdated_or_unclear_proposal_is_not_accepted_and_the_engine_is_not_asked() {
        let then = plan_of(prompt("a", "x"));
        let root = seed("docmap-decide-stale", &then);
        let w = Writer::default();
        let rule = accept(
            &root,
            "r1",
            "A-1",
            "P",
            None,
            &PlanAs(plan_of(prompt("a EDITED", "x"))),
            &w,
        )
        .unwrap_err();
        assert_eq!(rule.key(), "err.agent.staleRule");
        let files = accept(
            &root,
            "r1",
            "A-1",
            "P",
            None,
            &PlanAs(plan_of(prompt("a", "y"))),
            &w,
        )
        .unwrap_err();
        assert_eq!(files.key(), "err.agent.staleFiles");
        let mut gone = then.clone();
        gone.batches[0].rules.retain(|r| r.id != "A-1");
        let gone = accept(&root, "r1", "A-1", "P", None, &PlanAs(gone), &w).unwrap_err();
        assert_eq!(gone.key(), "err.agent.staleGone");
        let unclear = accept(&root, "r1", "B-2", "P", None, &PlanAs(then.clone()), &w).unwrap_err();
        assert_eq!(unclear.key(), "err.agent.notAnswer");
        let none = accept(&root, "r1", "C-3", "P", None, &PlanAs(then.clone()), &w).unwrap_err();
        assert_eq!(none.key(), "err.agent.noProposal");
        assert!(w.accepted.lock().unwrap().is_empty());
        assert!(Store::new(&root).load("r1").unwrap().decisions.is_empty());
        // an outdated proposal can still be waived: a waiver is about the rule
        let args = WaiveArgs {
            by: "P".into(),
            reason: "legacy code, replaced next quarter".into(),
            ..Default::default()
        };
        assert_eq!(
            waive(&root, "r1", "A-1", &args, &w).unwrap().kind,
            DecisionKind::Waived
        );
    }

    #[test]
    fn a_refusal_of_the_engine_is_no_decision() {
        let plan = plan_of(prompt("a", "x"));
        let root = seed("docmap-decide-refused", &plan);
        let w = Writer {
            refuse: Some("the verdicts file cannot be used"),
            ..Default::default()
        };
        let e = accept(&root, "r1", "A-1", "P", None, &PlanAs(plan), &w).unwrap_err();
        assert_eq!(e.key(), "err.agent.refused");
        assert_eq!(e.detail_text(), Some("the verdicts file cannot be used"));
        let args = WaiveArgs {
            by: "P".into(),
            reason: "r".into(),
            ..Default::default()
        };
        assert_eq!(
            waive(&root, "r1", "A-1", &args, &w).unwrap_err().key(),
            "err.agent.refused"
        );
        assert!(Store::new(&root).load("r1").unwrap().decisions.is_empty());
        assert!(!root
            .join(store::DIR)
            .join("r1")
            .join("decisions.json")
            .exists());
    }

    #[test]
    fn a_waiver_and_a_rejection_are_remembered_and_only_the_waiver_reaches_the_engine() {
        let plan = plan_of(prompt("a", "x"));
        let root = seed("docmap-decide-waive", &plan);
        let w = Writer::default();
        let args = WaiveArgs {
            by: "P".into(),
            reason: " generated code ".into(),
            expires: Some("2027-01-31".into()),
            paths: vec!["lua/gen".into(), " ".into(), "docs/*.md".into()],
        };
        let d = waive(&root, "r1", "A-1", &args, &w).unwrap();
        assert_eq!(d.kind, DecisionKind::Waived);
        assert_eq!(d.reason.as_deref(), Some("generated code"));
        assert_eq!(d.expires.as_deref(), Some("2027-01-31"));
        assert_eq!(d.paths, vec!["lua/gen", "docs/*.md"]);
        assert_eq!(
            w.waived.lock().unwrap()[0],
            (
                "A-1".to_string(),
                "generated code".to_string(),
                Some("2027-01-31".to_string()),
                vec!["lua/gen".to_string(), "docs/*.md".to_string()]
            )
        );
        let r = reject(&root, "r1", "B-2", "P").unwrap();
        assert_eq!(r.kind, DecisionKind::Rejected);
        assert_eq!(r.file, None);
        assert_eq!(w.waived.lock().unwrap().len(), 1);
        assert!(w.accepted.lock().unwrap().is_empty());
        let run = Store::new(&root).load("r1").unwrap();
        assert_eq!(run.decisions.len(), 2);
        assert_eq!(run.decisions["B-2"].kind, DecisionKind::Rejected);
    }

    #[test]
    fn what_the_person_enters_is_checked_before_anything_runs() {
        let plan = plan_of(prompt("a", "x"));
        let root = seed("docmap-decide-input", &plan);
        let w = Writer::default();
        for by in ["", "   ", "two\nlines", &"x".repeat(101)] {
            let e = accept(&root, "r1", "A-1", by, None, &PlanAs(plan.clone()), &w).unwrap_err();
            assert_eq!(e.key(), "err.agent.by", "{by:?}");
        }
        let e = accept(
            &root,
            "r1",
            "A-1",
            "P",
            Some("a\nb"),
            &PlanAs(plan.clone()),
            &w,
        )
        .unwrap_err();
        assert_eq!(e.key(), "err.agent.note");
        let base = WaiveArgs {
            by: "P".into(),
            reason: "r".into(),
            ..Default::default()
        };
        let no_reason = WaiveArgs {
            reason: " ".into(),
            ..base.clone()
        };
        assert_eq!(
            waive(&root, "r1", "A-1", &no_reason, &w).unwrap_err().key(),
            "err.agent.reason"
        );
        for bad in ["31.01.2027", "2027-1-31", "tomorrow"] {
            let a = WaiveArgs {
                expires: Some(bad.into()),
                ..base.clone()
            };
            assert_eq!(
                waive(&root, "r1", "A-1", &a, &w).unwrap_err().key(),
                "err.agent.expires"
            );
        }
        let comma = WaiveArgs {
            paths: vec!["a,b".into()],
            ..base.clone()
        };
        assert_eq!(
            waive(&root, "r1", "A-1", &comma, &w).unwrap_err().key(),
            "err.agent.paths"
        );
        assert!(w.waived.lock().unwrap().is_empty() && w.accepted.lock().unwrap().is_empty());
        // an empty expiry is none
        let none = WaiveArgs {
            expires: Some("  ".into()),
            ..base
        };
        assert_eq!(waive(&root, "r1", "A-1", &none, &w).unwrap().expires, None);
    }

    #[cfg(windows)]
    #[test]
    fn a_text_the_engine_would_change_on_windows_is_refused_before_it_is_handed_over() {
        let plan = plan_of(prompt("a", "x"));
        let root = seed("docmap-decide-ascii", &plan);
        let w = Writer::default();
        let e = accept(
            &root,
            "r1",
            "A-1",
            "M\u{fc}ller",
            None,
            &PlanAs(plan.clone()),
            &w,
        )
        .unwrap_err();
        assert_eq!(e.key(), "err.agent.ascii");
        let e = accept(&root, "r1", "A-1", "P", Some("f\u{fc}r"), &PlanAs(plan), &w).unwrap_err();
        assert_eq!(e.key(), "err.agent.ascii");
        for args in [
            WaiveArgs {
                by: "P".into(),
                reason: "x \u{4e2d}".into(),
                ..Default::default()
            },
            WaiveArgs {
                by: "P".into(),
                reason: "r".into(),
                paths: vec!["lua/\u{e4}.lua".into()],
                ..Default::default()
            },
        ] {
            assert_eq!(
                waive(&root, "r1", "A-1", &args, &w).unwrap_err().key(),
                "err.agent.ascii"
            );
        }
        assert!(w.accepted.lock().unwrap().is_empty() && w.waived.lock().unwrap().is_empty());
    }

    #[test]
    fn a_damaged_decisions_file_is_not_written_over() {
        let plan = plan_of(prompt("a", "x"));
        let root = seed("docmap-decide-damaged", &plan);
        let file = root.join(store::DIR).join("r1").join("decisions.json");
        fs::write(&file, "{ cut").unwrap();
        // the run still shows
        assert!(Store::new(&root).load("r1").unwrap().decisions.is_empty());
        let e = reject(&root, "r1", "A-1", "P").unwrap_err();
        assert_eq!(e.key(), "err.agent.runUnreadable");
        assert_eq!(fs::read_to_string(&file).unwrap(), "{ cut");
    }

    /// The bundled engine of this checkout, when there is a real one (a developer's machine; CI stages an empty
    /// placeholder), as planner and as writer. `None` also for an engine without `rules-write`.
    pub(in crate::agent) fn real_engine() -> Option<(
        crate::agent::engine::CliEngine,
        crate::agent::engine::CliJudge,
    )> {
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
        let exe = exe.to_string_lossy().into_owned();
        let engine = crate::agent::engine::CliEngine::new(exe.clone(), None).ok()?;
        let judge = crate::agent::engine::CliJudge::new(exe, None).ok()?;
        Some((engine, judge))
    }

    /// A project with a ruleset beside it: two manual rules with a scope, one with a check. Under the build
    /// folder, not the temp folder, for the reason `rules/tab.rs` gives.
    pub(in crate::agent) fn real_project(name: &str) -> std::path::PathBuf {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("rules-fixtures")
            .join(name);
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let base = std::path::PathBuf::from(crate::portable(&fs::canonicalize(&base).unwrap()));
        let project = base.join("project");
        let ruleset = base.join("ruleset");
        fs::create_dir_all(project.join("lua")).unwrap();
        fs::create_dir_all(&ruleset).unwrap();
        fs::write(
            project.join(".rules.json"),
            concat!(
                "{ \"rulesets\": [\"../ruleset\"],\n",
                "  \"agent\": { \"scopes\": { \"M\": { \"include\": [\"lua/*.lua\"] } },\n",
                "             \"backends\": [\"ollama\"] } }\n"
            ),
        )
        .unwrap();
        fs::write(
            project.join("lua/a.lua"),
            "local M = {}\nM.word = \"FORBIDDEN\"\nreturn M\n",
        )
        .unwrap();
        fs::write(
            ruleset.join("RULES.md"),
            concat!(
                "# Rules\n\n",
                "#### `M-01` a module is named after what it does\n\n",
                "```rule\nid = \"M-01\",\nseverity = \"recommended\",\n```\n\nRead it.\n\n",
                "#### `M-02` a boolean is named as a question\n\n",
                "```rule\nid = \"M-02\",\nseverity = \"recommended\",\n```\n\nRead it.\n\n",
                "#### `Q-01` a word that must not appear\n\n",
                "```rule\nid = \"Q-01\",\nseverity = \"recommended\",\n",
                "check = { type = \"grep\", pattern = \"FORBIDDEN\", include = \"%.lua$\" },\n```\n",
            ),
        )
        .unwrap();
        project
    }

    /// A finished run over `M-01,M-02` of `project`, made as the runner makes one: the engine's plan, a canned
    /// answer through `rules/validate`, the request key for the run's destination.
    pub(in crate::agent) fn real_run(project: &Path, engine: &dyn Engine) -> Run {
        let plan = engine.plan(project, "M-01,M-02").unwrap();
        assert_eq!(plan.batches.len(), 1, "{plan:?}");
        let b = &plan.batches[0];
        let ids: Vec<String> = b.rules.iter().map(|r| r.id.clone()).collect();
        let hashes = rule_hashes(&b.prompt, &ids);
        let key = request_key(&b.system, &b.prompt, Backend::Ollama, "m");
        let answer = json!({"results": [
            {"rule": "M-01", "verdict": "compliant", "checked": ["lua/a.lua"]},
            {"rule": "M-02", "verdict": "violation", "checked": ["lua/a.lua"],
             "findings": [{"file": "lua/a.lua", "line": 2, "quote": "M.word = \"FORBIDDEN\"", "why": "no question"}]}
        ]})
        .to_string();
        let proposals = ids
            .iter()
            .map(|id| {
                let p = engine
                    .validate(
                        project,
                        id,
                        &answer,
                        &Store::new(project).scratch("r1").unwrap(),
                    )
                    .unwrap();
                ProposalRec {
                    rule_id: id.clone(),
                    batch: b.id.clone(),
                    key: key.clone(),
                    rule_hash: hashes.get(id).cloned(),
                    status: p["status"].as_str().unwrap().to_string(),
                    proposal: p,
                    ..Default::default()
                }
            })
            .collect();
        let run = Run {
            id: "r1".into(),
            selection: "M-01,M-02".into(),
            model: "m".into(),
            state: store::RunState::Done,
            proposals,
            ..Default::default()
        };
        Store::new(project).save(&run).unwrap();
        run
    }

    #[test]
    fn with_the_real_engine_a_decision_is_written_by_rules_nvim_and_an_outdated_one_is_refused() {
        let Some((engine, judge)) = real_engine() else {
            eprintln!("skipped: no real engine with rules-write in src-tauri/binaries");
            return;
        };
        let project = real_project("docmap-decide-real");
        let run = real_run(&project, &engine);
        assert_eq!(
            run.proposals[1].status, "violation",
            "{:?}",
            run.proposals[1]
        );

        // accepted: the engine wrote the verdict, with the person and the evidence that held up
        let d = accept(
            &project,
            "r1",
            "M-02",
            "A Person",
            Some("looked"),
            &engine,
            &judge,
        )
        .unwrap();
        assert_eq!(d.verdict.as_deref(), Some("violation"));
        let verdicts: Value = serde_json::from_str(
            &fs::read_to_string(project.join(".rules-verdicts.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(verdicts["M-02"]["by"], json!("A Person"));
        assert_eq!(verdicts["M-02"]["verdict"], json!("violation"));
        assert_eq!(verdicts["M-02"]["evidence"][0]["file"], json!("lua/a.lua"));
        // no proposal file is left behind
        let left: Vec<_> = fs::read_dir(project.join(store::DIR).join("r1"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("docmap-proposal-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");

        // a file in scope changes: the other proposal is outdated and the engine is not asked
        fs::write(project.join("lua/a.lua"), "local M = {}\nreturn M\n").unwrap();
        let e = accept(&project, "r1", "M-01", "A Person", None, &engine, &judge).unwrap_err();
        assert_eq!(e.key(), "err.agent.staleFiles");
        assert!(!fs::read_to_string(project.join(".rules-verdicts.json"))
            .unwrap()
            .contains("M-01"));

        // but the rule can be waived, through the engine; a second waiver of it is the engine's refusal
        let args = WaiveArgs {
            by: "A Person".into(),
            reason: "the module is replaced next month".into(),
            expires: Some("2099-12-31".into()),
            paths: vec!["lua/a.lua".into()],
        };
        let w = waive(&project, "r1", "M-01", &args, &judge).unwrap();
        assert_eq!(w.file.as_deref(), Some(".rules-waivers.json"));
        let waivers: Value =
            serde_json::from_str(&fs::read_to_string(project.join(".rules-waivers.json")).unwrap())
                .unwrap();
        assert_eq!(
            waivers["M-01"]["reason"],
            json!("the module is replaced next month")
        );
        assert_eq!(waivers["M-01"]["expires"], json!("2099-12-31"));
        let again = judge
            .waive(&project, "M-01", "again", None, &[])
            .unwrap_err();
        assert_eq!(again.key(), "err.agent.refused");
        assert!(
            again
                .detail_text()
                .is_some_and(|d| d.contains("waived already")),
            "{again}"
        );

        // and what the Rules tab reads of the two files agrees with what the engine wrote
        let judged = crate::rules::judged::read(&project, "2026-11-15");
        assert_eq!(judged.verdicts["M-02"].by, "A Person");
        assert!(judged.waivers.contains_key("M-01") && !judged.waivers["M-01"].expired);
        assert!(judged.unreadable.is_empty());

        // a rule with a check is not the writer's: refused, and the file is as it was
        let before = fs::read_to_string(project.join(".rules-verdicts.json")).unwrap();
        let refused = judge
            .accept(
                &project,
                "Q-01",
                &json!({"rule_id": "Q-01", "status": "violation", "proposed": true, "verified": false}),
                "A Person",
                None,
                &Store::new(&project).scratch("r1").unwrap(),
            )
            .unwrap_err();
        assert_eq!(refused.key(), "err.agent.refused");
        assert_eq!(
            fs::read_to_string(project.join(".rules-verdicts.json")).unwrap(),
            before
        );
    }
}
