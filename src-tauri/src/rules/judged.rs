//! Which rules of a project a person has decided about: the verdicts in
//! `.rules-verdicts.json` and the waivers in `.rules-waivers.json`, for the
//! Rules tab to show in its Agent column and to leave out of the count of
//! open manual rules.
//!
//! **The engine reads both files, not this side.** `rules/judged` (docmap.nvim
//! `0fe645e`) answers with `rules.nvim`'s own readers, the files' only
//! definition: a waiver is expired by the engine's local day, the day
//! `rules/run` waives by, and a verdict is stale when a file of its evidence
//! changed in a commit after it was accepted. This module only puts that answer
//! into the shape the window reads (maps by rule id). A file that is there and
//! cannot be used is in `unreadable` (the count then subtracts nothing for it),
//! never taken as "no verdicts"; an entry without an id is left out.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::apperr::AppError;

/// One verdict, as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    /// `violation` or `compliant`.
    pub verdict: String,
    pub date: String,
    pub by: String,
    /// A file of its evidence changed in a commit after it was accepted;
    /// `None` when the engine could not tell (no git).
    pub stale: Option<bool>,
}

/// One waiver, as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Waiver {
    pub reason: String,
    pub expires: Option<String>,
    /// Past its last day: the rule counts as open again.
    pub expired: bool,
    /// Limited to some paths.
    pub partial: bool,
}

/// What the Rules tab knows of a project's decisions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Judged {
    pub verdicts: BTreeMap<String, Verdict>,
    pub waivers: BTreeMap<String, Waiver>,
    /// The files that are there and could not be read, by name.
    pub unreadable: Vec<String>,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn entries<'a>(answer: &'a Value, key: &str) -> Result<&'a [Value], AppError> {
    match answer.get(key) {
        Some(Value::Array(list)) => Ok(list),
        // an empty list may come as an empty object from a JSON encoder
        Some(Value::Object(o)) if o.is_empty() => Ok(&[]),
        _ => Err(AppError::new("err.rules.shape")),
    }
}

/// The engine's answer to `rules/judged`, as the window reads it. The engine's
/// refusal (`available: false`) is an error with its reason, never an empty
/// list: that would count every manual rule as open without saying why.
pub fn from_engine(answer: &Value) -> Result<Judged, AppError> {
    if answer.get("available").and_then(Value::as_bool) != Some(true) {
        let reason = text(answer, "reason").unwrap_or_else(|| "no reason given".to_string());
        return Err(AppError::new("err.rules.engine").detail(reason));
    }
    let mut out = Judged::default();
    for v in entries(answer, "verdicts")? {
        let (Some(id), Some(verdict)) = (text(v, "id"), text(v, "verdict")) else {
            continue;
        };
        out.verdicts.insert(
            id,
            Verdict {
                verdict,
                date: text(v, "date").unwrap_or_default(),
                by: text(v, "by").unwrap_or_default(),
                stale: v.get("stale").and_then(Value::as_bool),
            },
        );
    }
    for w in entries(answer, "waivers")? {
        let (Some(id), Some(reason)) = (text(w, "id"), text(w, "reason")) else {
            continue;
        };
        out.waivers.insert(
            id,
            Waiver {
                reason,
                expires: text(w, "expires"),
                expired: w.get("expired").and_then(Value::as_bool) == Some(true),
                partial: w.get("partial").and_then(Value::as_bool) == Some(true),
            },
        );
    }
    for u in entries(answer, "unreadable")? {
        if let Some(file) = text(u, "file") {
            out.unreadable.push(file);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_answer_of_the_engine_becomes_maps_by_rule_id() {
        let j = from_engine(&json!({
            "available": true, "today": "2026-11-16", "stale_checked": true,
            "verdicts": [
                {"id": "M-1", "verdict": "violation", "date": "2026-11-15", "by": "A Person", "evidence": 2, "stale": true},
                {"verdict": "compliant"}
            ],
            "waivers": [
                {"id": "M-3", "reason": "generated code", "expired": false, "partial": false},
                {"id": "M-4", "reason": "until the rewrite", "expires": "2026-11-14", "expired": true, "partial": false},
                {"id": "M-5", "reason": "f\u{fc}r \u{4e2d}", "expires": "2026-11-16", "paths": ["lua/gen"], "expired": false, "partial": true}
            ],
            "unreadable": []
        }))
        .unwrap();
        assert_eq!(j.verdicts.len(), 1, "an entry without an id is left out");
        assert_eq!(j.verdicts["M-1"].by, "A Person");
        assert_eq!(j.verdicts["M-1"].stale, Some(true));
        assert_eq!(j.waivers.len(), 3);
        assert!(!j.waivers["M-3"].expired);
        assert!(
            j.waivers["M-4"].expired,
            "the engine's word, not a second clock"
        );
        assert!(j.waivers["M-5"].partial);
        assert_eq!(j.waivers["M-5"].reason, "f\u{fc}r \u{4e2d}");
        assert!(j.unreadable.is_empty());
        // empty lists may arrive as empty objects
        let none = from_engine(
            &json!({"available": true, "verdicts": {}, "waivers": {}, "unreadable": {}}),
        );
        assert_eq!(none.unwrap(), Judged::default());
    }

    #[test]
    fn a_file_that_cannot_be_read_and_a_refusal_say_so_and_are_no_empty_list() {
        let j = from_engine(&json!({
            "available": true, "verdicts": [], "waivers": [],
            "unreadable": [{"file": ".rules-verdicts.json", "reason": "not JSON"}]
        }))
        .unwrap();
        assert_eq!(j.unreadable, vec![".rules-verdicts.json"]);
        let e = from_engine(&json!({"available": false, "reason": "no rules engine"})).unwrap_err();
        assert_eq!(e.key(), "err.rules.engine");
        assert_eq!(e.detail_text(), Some("no rules engine"));
        assert_eq!(
            from_engine(&json!({"available": true, "verdicts": 3}))
                .unwrap_err()
                .key(),
            "err.rules.shape"
        );
    }
}
