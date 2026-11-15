//! Which rules of a project a person has decided about: the verdicts in
//! `.rules-verdicts.json` and the waivers in `.rules-waivers.json`, for the
//! Rules tab to show in its Agent column and to leave out of the count of
//! open manual rules.
//!
//! **A reader for the window, not a second definition of either file.** The
//! engine answers no route with them (`rules/run` reports a waived manual rule
//! as `manual`), and they are written only by `rules.nvim`'s writers, run by the
//! engine (`agent/decide.rs`). So this reads the few fields it shows and judges
//! nothing a run depends on: an entry it does not understand is left out, and a
//! file it cannot read at all is reported as such (the count then subtracts
//! nothing for it), never taken as "no verdicts". Whether a waiver has expired
//! is compared with today's date in UTC; `rules.nvim` uses the local day, so for
//! a few hours around midnight the two can disagree about the last day.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

/// The verdicts file at the project root (`rules.engine.verdicts.FILE`).
pub const VERDICTS: &str = ".rules-verdicts.json";
/// The waivers file at the project root (`rules.engine.waivers.FILE`).
pub const WAIVERS: &str = ".rules-waivers.json";
/// Far above what the writers allow (1000 verdicts, 500 waivers).
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// One verdict, as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    /// `violation` or `compliant`.
    pub verdict: String,
    pub date: String,
    pub by: String,
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

/// The JSON object in `root/name`: `Ok(None)` when there is no file, `Err`
/// when there is one that cannot be used (a link, too large, not an object).
fn object(root: &Path, name: &str) -> Result<Option<serde_json::Map<String, Value>>, ()> {
    let path = root.join(name);
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err(());
    }
    let text = fs::read_to_string(&path).map_err(|_| ())?;
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(o)) => Ok(Some(o)),
        _ => Err(()),
    }
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// The decisions of the project at `root`, judged against `today` (`YYYY-MM-DD`).
pub fn read(root: &Path, today: &str) -> Judged {
    let mut out = Judged::default();
    match object(root, VERDICTS) {
        Ok(Some(o)) => {
            for (id, entry) in o {
                let verdict = text(&entry, "verdict");
                if let Some(verdict) = verdict.filter(|v| v == "violation" || v == "compliant") {
                    out.verdicts.insert(
                        id,
                        Verdict {
                            verdict,
                            date: text(&entry, "date").unwrap_or_default(),
                            by: text(&entry, "by").unwrap_or_default(),
                        },
                    );
                }
            }
        }
        Ok(None) => {}
        Err(()) => out.unreadable.push(VERDICTS.to_string()),
    }
    match object(root, WAIVERS) {
        Ok(Some(o)) => {
            for (id, entry) in o {
                // `"ID": "reason"` and `"ID": { reason, expires?, paths? }`
                let waiver = match &entry {
                    Value::String(reason) => Some(Waiver {
                        reason: reason.clone(),
                        expires: None,
                        expired: false,
                        partial: false,
                    }),
                    Value::Object(_) => text(&entry, "reason").map(|reason| {
                        let expires = text(&entry, "expires");
                        Waiver {
                            reason,
                            // ISO dates compare as text
                            expired: expires.as_deref().is_some_and(|d| d < today),
                            expires,
                            partial: entry
                                .get("paths")
                                .and_then(Value::as_array)
                                .is_some_and(|p| !p.is_empty()),
                        }
                    }),
                    _ => None,
                };
                if let Some(w) = waiver {
                    out.waivers.insert(id, w);
                }
            }
        }
        Ok(None) => {}
        Err(()) => out.unreadable.push(WAIVERS.to_string()),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fresh_dir;

    #[test]
    fn verdicts_and_waivers_are_read_with_the_expired_ones_marked() {
        let root = fresh_dir("docmap-rules-judged");
        assert_eq!(read(&root, "2026-11-15"), Judged::default());
        fs::write(
            root.join(VERDICTS),
            r#"{ "M-1": { "verdict": "violation", "date": "2026-11-15", "by": "A Person", "evidence": [] },
                 "M-2": { "verdict": "maybe" } }"#,
        )
        .unwrap();
        fs::write(
            root.join(WAIVERS),
            r#"{ "M-3": "generated code",
                 "M-4": { "reason": "until the rewrite", "expires": "2026-11-14" },
                 "M-5": { "reason": "today is the last day", "expires": "2026-11-15", "paths": ["lua/gen"] },
                 "M-6": 7 }"#,
        )
        .unwrap();
        let j = read(&root, "2026-11-15");
        assert_eq!(
            j.verdicts.len(),
            1,
            "an entry that is no verdict is left out"
        );
        assert_eq!(j.verdicts["M-1"].by, "A Person");
        assert_eq!(j.waivers.len(), 3);
        assert!(!j.waivers["M-3"].expired);
        assert!(j.waivers["M-4"].expired);
        assert!(!j.waivers["M-5"].expired, "valid through its last day");
        assert!(j.waivers["M-5"].partial);
        assert!(j.unreadable.is_empty());
    }

    #[test]
    fn a_file_that_cannot_be_read_says_so_and_is_no_empty_list() {
        let root = fresh_dir("docmap-rules-judged-broken");
        fs::write(root.join(VERDICTS), "{ cut").unwrap();
        fs::write(root.join(WAIVERS), "[]").unwrap();
        let j = read(&root, "2026-11-15");
        assert_eq!(j.unreadable, vec![VERDICTS, WAIVERS]);
        assert!(j.verdicts.is_empty() && j.waivers.is_empty());
    }
}
