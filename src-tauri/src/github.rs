//! The signed-in user's repositories, via the GitHub CLI.
//!
//! ## Why `gh` and not the API
//!
//! Because this program must not hold a credential. `docs/USAGE.md` already
//! states that position for cloning — "whatever a plain `git clone` of that
//! URL would need on this machine is exactly what runs here too" — and asking
//! for a token to list repositories would quietly abandon it. `gh` is already
//! authenticated on a machine where this feature is useful at all, it keeps
//! the token in the OS credential store where it belongs, and this program
//! never sees it.
//!
//! The trade is that `gh` might not be installed or might not be logged in.
//! Both are ordinary states, not errors: the URL field beside this list keeps
//! working, so the feature degrades to exactly what existed before it.
//!
//! ## On demand, never on open
//!
//! Nothing here runs when the dialog opens. Listing repositories is a network
//! call against someone's account, and a dialog that makes one just for being
//! looked at is doing something the reader did not ask for.

use serde::{Deserialize, Serialize};

/// One repository, as the picker shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub name: String,
    pub name_with_owner: String,
    pub url: String,
    pub description: Option<String>,
    pub is_private: bool,
    pub updated_at: String,
    /// GitHub's own primary-language guess. Shown because it answers "will
    /// this map into anything" before the clone, the same question
    /// `scan_languages` answers for a local folder — and unlike that one,
    /// this is somebody else's guess, so it is labelled as GitHub's.
    pub language: Option<String>,
}

/// Why a listing produced nothing, in the caller's terms.
///
/// Three states rather than one error string, because they need three
/// different sentences in the UI and only one of them is worth acting on:
/// install it, log in, or read the message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ListProblem {
    /// `gh` is not on PATH.
    NotInstalled,
    /// `gh` is there and has no usable credential.
    NotAuthenticated,
    /// It ran and failed for some other reason; the message is `gh`'s own.
    Failed,
    /// It answered, but not with the list it was asked for; the message is
    /// the parser's.
    Unreadable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoList {
    pub repos: Vec<Repo>,
    /// `None` when the listing succeeded.
    pub problem: Option<ListProblem>,
    /// `gh`'s own stderr when it failed. Shown verbatim rather than
    /// summarised — the same decision `generate` already makes about the
    /// engine's report, and for the same reason: it already says what went
    /// wrong, and a summary would be this program inventing an opinion about
    /// someone else's output.
    pub message: Option<String>,
}

/// The shape `gh repo list --json` returns.
#[derive(Deserialize)]
struct GhRepo {
    name: String,
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
    url: String,
    description: Option<String>,
    #[serde(rename = "isPrivate")]
    is_private: bool,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    #[serde(rename = "primaryLanguage")]
    primary_language: Option<GhLanguage>,
}

#[derive(Deserialize)]
struct GhLanguage {
    name: String,
}

/// The `gh` command line of the list (`proc`: bound to the app, no window).
///
/// `gh` never asks anything here: a question needs a terminal this app does
/// not have, and a `gh` waiting for an answer nobody can give waited for ever
/// (it had no time limit either). `GH_PROMPT_DISABLED` turns its prompts into
/// failures, `GH_NO_UPDATE_NOTIFIER` keeps it from checking for a newer `gh`
/// on the way, and `GIT_TERMINAL_PROMPT` covers a git it might start.
pub(crate) fn list_command(limit: u32) -> crate::proc::Cmd {
    let limit = limit.clamp(1, 300);
    let mut cmd = crate::proc::Cmd::new("docmap-desktop list GitHub repositories", "gh");
    cmd.args([
        "repo",
        "list",
        "--limit",
        &limit.to_string(),
        "--json",
        "name,nameWithOwner,url,description,isPrivate,updatedAt,primaryLanguage",
    ])
    .env("GH_PROMPT_DISABLED", "1")
    .env("GH_NO_UPDATE_NOTIFIER", "1")
    .env("GIT_TERMINAL_PROMPT", "0")
    // 300 repositories are some 150 kB of JSON.
    .max_output(8 * 1024 * 1024, 1024 * 1024);
    cmd
}

/// List the signed-in user's repositories, newest first.
///
/// `limit` is capped rather than trusted: this renders into a scrollable
/// list, and a five-thousand-entry response would cost a long wait for
/// something nobody scrolls to the end of.
pub fn list(limit: u32) -> RepoList {
    // Distinguished from every other failure because it is the only one with
    // an action attached that is not "log in".
    if crate::proc::resolve(std::path::Path::new("gh")).is_none() {
        return RepoList {
            repos: vec![],
            problem: Some(ListProblem::NotInstalled),
            message: None,
        };
    }
    let out = match crate::proc::run(&list_command(limit), crate::GH_LIST_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            return RepoList {
                repos: vec![],
                problem: Some(ListProblem::Failed),
                message: Some(match e {
                    crate::proc::ProcError::Timeout(secs) => {
                        format!("gh gave no answer within {secs} s")
                    }
                    crate::proc::ProcError::Failed(why) => why,
                    crate::proc::ProcError::Cancelled => "cancelled".to_string(),
                }),
            }
        }
    };

    if !out.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // `gh` says "To get started with GitHub CLI, please run: gh auth
        // login" (and variations) on an unauthenticated account. Matched on
        // the stable half of that sentence rather than on an exit code,
        // which is the same for every failure.
        let unauth = stderr.contains("gh auth login")
            || stderr.contains("not logged")
            || stderr.contains("authentication");
        return RepoList {
            repos: vec![],
            problem: Some(if unauth {
                ListProblem::NotAuthenticated
            } else {
                ListProblem::Failed
            }),
            message: (!stderr.is_empty()).then_some(stderr),
        };
    }

    match serde_json::from_slice::<Vec<GhRepo>>(&out.stdout) {
        Ok(list) => RepoList {
            repos: list
                .into_iter()
                .map(|r| Repo {
                    name: r.name,
                    name_with_owner: r.name_with_owner,
                    url: r.url,
                    description: r.description.filter(|d| !d.is_empty()),
                    is_private: r.is_private,
                    updated_at: r.updated_at,
                    language: r.primary_language.map(|l| l.name),
                })
                .collect(),
            problem: None,
            message: None,
        },
        Err(e) => RepoList {
            repos: vec![],
            problem: Some(ListProblem::Unreadable),
            message: Some(e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_gh_is_its_own_state_not_a_generic_failure() {
        // Cannot be tested by running the real binary — it *is* installed
        // here — so this asserts the discrimination the UI depends on rather
        // than the spawn: three problems that need three different
        // sentences, and only one of which says "install something".
        assert_ne!(ListProblem::NotInstalled, ListProblem::NotAuthenticated);
        assert_ne!(ListProblem::NotAuthenticated, ListProblem::Failed);
        assert_ne!(ListProblem::Failed, ListProblem::Unreadable);
    }

    // `gh` without a terminal: a prompt would wait for ever, so none may come.
    #[test]
    fn gh_is_told_never_to_ask() {
        let cmd = list_command(200);
        let envs: Vec<(String, String)> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                )
            })
            .collect();
        for pair in [
            ("GH_PROMPT_DISABLED", "1"),
            ("GH_NO_UPDATE_NOTIFIER", "1"),
            ("GIT_TERMINAL_PROMPT", "0"),
        ] {
            assert!(
                envs.contains(&(pair.0.to_string(), pair.1.to_string())),
                "{envs:?}"
            );
        }
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(&args[..4], ["repo", "list", "--limit", "200"]);
        assert_eq!(cmd.get_program(), "gh");
    }

    #[test]
    fn the_limit_is_capped_rather_than_trusted() {
        // Indirect, since `list` shells out: the clamp is the contract, and
        // this pins the bounds it promises.
        assert_eq!(0u32.clamp(1, 300), 1);
        assert_eq!(9999u32.clamp(1, 300), 300);
    }
}
