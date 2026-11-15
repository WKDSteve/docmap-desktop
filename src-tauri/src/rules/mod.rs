//! The Rules tab: a project's rules, and the mechanical run of the ones that
//! have a check.
//!
//! `RULES_AGENT_CONCEPT.md` step P3. Four parts:
//!
//! - [`engine`] asks the standalone engine for `rules/catalog` and
//!   `rules/run`; the list and the run are `rules.nvim`'s, not rebuilt here;
//! - [`trust`] is the one thing this side decides: which rulesets may run
//!   their `lua_predicate` checks (D5), by real path and content hash;
//! - [`judged`] reads which rules a person decided about (verdicts, waivers),
//!   for the window only;
//! - [`tab`] puts the two together for the window, and [`commands`] are the
//!   Tauri commands.
//!
//! Nothing here talks to a network or to ai-desktop: a mechanical run stays on
//! this machine, and so does the list of what is trusted. The agent run over
//! the manual rules is `agent/`.

pub mod commands;
pub mod engine;
pub mod judged;
pub mod tab;
pub mod trust;
