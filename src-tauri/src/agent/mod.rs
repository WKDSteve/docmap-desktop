//! The run of an agent over a project's manual rules -- the part that has no
//! window.
//!
//! `RULES_AGENT_CONCEPT.md` (D2, D6, "The run") splits the work three ways and
//! this module is the third:
//!
//! - the **engine** (`docmap rules/plan`, `rules/validate`) decides what is
//!   asked and whether an answer holds up; those are pure functions shared with
//!   Neovim, so the prompt and the evidence check live in one place and are not
//!   rewritten here ([`engine`]);
//! - **ai-desktop** answers the question; the webview cannot call it (it
//!   refuses any request with a foreign `Origin`, which a browser always
//!   sends), so a small blocking client does ([`client`]);
//! - this side **owns the run**: batches in order, an answer cache, cancel,
//!   a record kept in the project so a restart finds it again, and the
//!   commands a window will call ([`runner`], [`store`], [`commands`]).
//!
//! What it never does: turn an answer into a verdict. Every answer goes
//! through `rules/validate`, what comes out is a *proposal*
//! (`proposed = true`, `verified = false`, forced again on the way to disk),
//! and nothing here writes `.rules-verdicts.json` or `.rules-waivers.json`;
//! accepting is a person's act and the next task's.

pub mod client;
pub mod commands;
pub mod engine;
pub mod runner;
pub mod store;

#[cfg(test)]
mod fake;

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Why a run was asked to stop. A cancel ends it; a pause keeps it resumable
/// (that is what the app does on quit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopKind {
    Cancel,
    Pause,
}

/// The flag a running job and the request in flight both look at. One word, so
/// the first request to stop wins and a later one cannot turn a cancel into a
/// pause.
#[derive(Debug, Default)]
pub struct Stop(AtomicU8);

impl Stop {
    pub fn request(&self, kind: StopKind) {
        let v = match kind {
            StopKind::Cancel => 1,
            StopKind::Pause => 2,
        };
        let _ = self
            .0
            .compare_exchange(0, v, Ordering::SeqCst, Ordering::SeqCst);
    }

    pub fn requested(&self) -> Option<StopKind> {
        match self.0.load(Ordering::SeqCst) {
            1 => Some(StopKind::Cancel),
            2 => Some(StopKind::Pause),
            _ => None,
        }
    }
}

/// Milliseconds since the epoch. The record keeps these rather than a
/// formatted date: the window formats them in its own language.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `YYYY-MM-DD` (UTC) of a time in milliseconds, the form the Neovim side's
/// answer cache dates its entries in. Days to a civil date by Howard Hinnant's
/// algorithm, so no date crate is pulled in for one line of text.
pub fn date_of(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_is_the_utc_day_of_the_millisecond() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(86_399_999), "1970-01-01");
        assert_eq!(date_of(86_400_000), "1970-01-02");
        // 2024-02-29, a leap day, and the turn of a year.
        assert_eq!(date_of(1_709_164_800_000), "2024-02-29");
        assert_eq!(date_of(1_735_689_599_000), "2024-12-31");
        assert_eq!(date_of(1_735_689_600_000), "2025-01-01");
    }

    #[test]
    fn the_first_request_to_stop_wins() {
        let s = Stop::default();
        assert_eq!(s.requested(), None);
        s.request(StopKind::Cancel);
        s.request(StopKind::Pause);
        assert_eq!(s.requested(), Some(StopKind::Cancel));
        let s = Stop::default();
        s.request(StopKind::Pause);
        s.request(StopKind::Cancel);
        assert_eq!(s.requested(), Some(StopKind::Pause));
    }
}
