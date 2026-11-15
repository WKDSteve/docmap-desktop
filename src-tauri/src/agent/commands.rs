//! The Tauri commands of the run: what the run dialog and the run view call,
//! and the three decisions a person takes about one proposal (`decide.rs`).
//!
//! A project is named by its `id` (its root path) as everywhere else; the
//! commands never take a path. The blocking ones (an engine process, a
//! request) run on Tauri's pool (`command(async)`), not on the main thread.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use super::chat::{self, ChatArgs, ChatEvent, ChatPreview, ChatReply, Chats};
use super::client::{self, Client, Health};
use super::decide::{self, WaiveArgs};
use super::engine::{CliEngine, CliJudge, Engine};
use super::runner::{self, Preview, Runner, Running, StartArgs};
use super::store::{self, Freshness, IgnoreState, Run, Summary};
use crate::apperr::AppError;

/// The root of the project `id`, which must be one of the workspace's.
fn project_root(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, AppError> {
    let ws = crate::read_workspace(app)?;
    let p = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(id))?;
    Ok(PathBuf::from(&p.root))
}

/// The root of a run: the one the live run knows, because it is listed in every workspace and spends money whichever
/// is open; else that of its project.
fn root_of_run(
    app: &tauri::AppHandle,
    runner: &Runner,
    id: &str,
    run_id: &str,
) -> Result<PathBuf, AppError> {
    match runner.live_root(run_id) {
        Some(root) => Ok(root),
        None => project_root(app, id),
    }
}

/// The engine the app is set up with, checked to speak `rules/plan` and
/// `rules/validate`.
fn engine(app: &tauri::AppHandle) -> Result<Arc<dyn Engine>, AppError> {
    let info = crate::engine_info(app.clone())?;
    let path = info.path.ok_or_else(|| AppError::new("err.engine.none"))?;
    Ok(Arc::new(CliEngine::new(path, info.grammars)?))
}

fn client(app: &tauri::AppHandle) -> Result<Client, AppError> {
    let url = crate::read_workspace(app)?
        .ai_desktop_url
        .unwrap_or_else(|| client::DEFAULT_URL.to_string());
    Client::new(&url)
}

/// Read, or set, the address ai-desktop is reached at. An empty value goes
/// back to the default. Only an address on this machine is accepted (see
/// `client::check_url`); the stored value is the normalised one.
#[tauri::command(async)]
pub fn agent_url(app: tauri::AppHandle, set: Option<String>) -> Result<String, AppError> {
    if let Some(value) = set {
        let value = value.trim().to_string();
        let checked = if value.is_empty() {
            None
        } else {
            Some(client::check_url(&value)?)
        };
        return crate::with_workspace(&app, |ws| {
            ws.ai_desktop_url = checked.clone();
            Ok(ws
                .ai_desktop_url
                .clone()
                .unwrap_or_else(|| client::DEFAULT_URL.to_string()))
        });
    }
    Ok(crate::read_workspace(&app)?
        .ai_desktop_url
        .unwrap_or_else(|| client::DEFAULT_URL.to_string()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHealth {
    pub url: String,
    #[serde(flatten)]
    pub health: Health,
}

/// `GET /health`: which backends ai-desktop has configured, for the consent
/// summary. Presence only, never a key.
#[tauri::command(async)]
pub fn agent_health(app: tauri::AppHandle) -> Result<AgentHealth, AppError> {
    let client = client(&app)?;
    let health = client.health().map_err(|e| e.into_app(client.base()))?;
    Ok(AgentHealth {
        url: client.base().to_string(),
        health,
    })
}

/// What a run would send and where, without sending or writing anything.
#[tauri::command(async)]
pub fn agent_preview(
    app: tauri::AppHandle,
    id: String,
    selection: String,
    model: String,
) -> Result<Preview, AppError> {
    let root = project_root(&app, &id)?;
    runner::preview(&*engine(&app)?, &root, &selection, &model)
}

/// Start a run: `args` is the dialog's form (title, notes, selection, model,
/// and `refresh` to ask again where the answer is cached).
#[tauri::command(async)]
pub fn agent_start(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    args: StartArgs,
) -> Result<Run, AppError> {
    let root = project_root(&app, &id)?;
    runner.start(&root, args, engine(&app)?, client(&app)?)
}

#[tauri::command(async)]
pub fn agent_resume(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    run_id: String,
) -> Result<Run, AppError> {
    let root = project_root(&app, &id)?;
    runner.resume(&root, &run_id, engine(&app)?, client(&app)?)
}

#[tauri::command(async)]
pub fn agent_cancel(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    run_id: String,
) -> Result<Run, AppError> {
    let root = root_of_run(&app, &runner, &id, &run_id)?;
    runner.cancel(&root, &run_id)
}

/// Pause one run; it stays resumable (`agent_resume`).
#[tauri::command(async)]
pub fn agent_pause(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    run_id: String,
) -> Result<Run, AppError> {
    let root = root_of_run(&app, &runner, &id, &run_id)?;
    runner.pause(&root, &run_id)
}

/// The runs of a project, newest first.
#[tauri::command(async)]
pub fn agent_list(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
) -> Result<Vec<Summary>, AppError> {
    Ok(runner.list(&project_root(&app, &id)?))
}

/// One run in full: batches, proposals, timestamps.
#[tauri::command(async)]
pub fn agent_status(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    run_id: String,
) -> Result<Run, AppError> {
    runner.status(&root_of_run(&app, &runner, &id, &run_id)?, &run_id)
}

/// Every run that is working right now, in any project.
#[tauri::command]
pub fn agent_running(runner: State<'_, Runner>) -> Vec<Running> {
    runner.running()
}

/// Which proposals of a run no longer match the rules or the tree.
#[tauri::command(async)]
pub fn agent_freshness(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    run_id: String,
) -> Result<std::collections::BTreeMap<String, Freshness>, AppError> {
    let root = project_root(&app, &id)?;
    runner.freshness(&root, &run_id, &*engine(&app)?)
}

/// The engine the app is set up with, checked to write verdicts and waivers.
fn judge(app: &tauri::AppHandle) -> Result<CliJudge, AppError> {
    let info = crate::engine_info(app.clone())?;
    let path = info.path.ok_or_else(|| AppError::new("err.engine.none"))?;
    CliJudge::new(path, info.grammars)
}

/// Accept the proposal about `rule` of run `run_id` as the rule's verdict:
/// written by the engine into `.rules-verdicts.json`, and only while the
/// proposal still matches the rule and the tree. Answers the run as it is then.
#[tauri::command(async)]
pub fn agent_accept(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    run_id: String,
    rule: String,
    by: String,
    note: Option<String>,
) -> Result<Run, AppError> {
    let root = project_root(&app, &id)?;
    let judge = judge(&app)?;
    decide::accept(
        &root,
        &run_id,
        &rule,
        &by,
        note.as_deref(),
        &*engine(&app)?,
        &judge,
    )?;
    runner.status(&root, &run_id)
}

/// Waive `rule`, the rule of a proposal of run `run_id`: written by the engine
/// into `.rules-waivers.json`.
#[tauri::command(async)]
pub fn agent_waive(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    run_id: String,
    rule: String,
    args: WaiveArgs,
) -> Result<Run, AppError> {
    let root = project_root(&app, &id)?;
    decide::waive(&root, &run_id, &rule, &args, &judge(&app)?)?;
    runner.status(&root, &run_id)
}

/// Reject the proposal about `rule`: the run remembers it, nothing else is written.
#[tauri::command(async)]
pub fn agent_reject(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    run_id: String,
    rule: String,
    by: String,
) -> Result<Run, AppError> {
    let root = project_root(&app, &id)?;
    decide::reject(&root, &run_id, &rule, &by)?;
    runner.status(&root, &run_id)
}

/// The name to offer for "decided by": git's `user.name` for the project, else
/// the account's name. Only a suggestion; the person can write another.
#[tauri::command(async)]
pub fn agent_person(app: tauri::AppHandle, id: String) -> Result<String, AppError> {
    let root = project_root(&app, &id)?;
    Ok(crate::gitcfg::user_name(&root)
        .or_else(|| std::env::var("USERNAME").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_default())
}

/// Whether `.rules-proposals/` is git-ignored, and with `add` the offer taken:
/// the line is appended to the project's `.gitignore`.
#[tauri::command(async)]
pub fn agent_ignore(app: tauri::AppHandle, id: String, add: bool) -> Result<IgnoreState, AppError> {
    let root = project_root(&app, &id)?;
    if add {
        store::ignore_add(&root)
    } else {
        Ok(store::ignore_state(&root))
    }
}

// ---- the chat of the run view (`chat.rs`) ---------------------------------

/// What the next turn of a conversation would send and where, without sending
/// anything: the window shows it before the person sends (no request ever
/// leaves without being shown first).
#[tauri::command(async)]
pub fn agent_chat_preview(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    args: ChatArgs,
) -> Result<ChatPreview, AppError> {
    let root = root_of_run(&app, &runner, &id, &args.run_id)?;
    Ok(chat::build(&*engine(&app)?, &root, &args, false)?.preview)
}

/// Send one turn: the person's last message with the rule, its files, the
/// attached files and the conversation so far. The answer streams into
/// `on_event` as it comes; the reply is the whole of it. `chat` is the window's
/// id for the conversation, which [`agent_chat_cancel`] names.
#[tauri::command(async)]
pub fn agent_chat_send(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    chats: State<'_, Chats>,
    id: String,
    chat: String,
    args: ChatArgs,
    on_event: tauri::ipc::Channel<ChatEvent>,
) -> Result<ChatReply, AppError> {
    let root = root_of_run(&app, &runner, &id, &args.run_id)?;
    let engine = engine(&app)?;
    let client = client(&app)?;
    let stop = chats.begin(&chat)?;
    let out = chat::send(&*engine, &client, &root, &args, &stop, &mut |ev| {
        // A window that went away does not stop the turn: the cancel does.
        let _ = on_event.send(ev);
    });
    chats.end(&chat);
    out
}

/// Stop the turn of conversation `chat` in flight. What came in before stays
/// in the window; the turn ends with `err.agent.chatStopped`.
#[tauri::command]
pub fn agent_chat_cancel(chats: State<'_, Chats>, chat: String) -> bool {
    chats.cancel(&chat)
}

/// Make one answer of the chat about `rule` a proposal of run `run_id`: checked
/// by `rules/validate`, stored as a proposal that waits for a person. Never a
/// verdict. Answers the run as it is then.
#[tauri::command(async)]
pub fn agent_chat_propose(
    app: tauri::AppHandle,
    runner: State<'_, Runner>,
    id: String,
    run_id: String,
    rule: String,
    text: String,
) -> Result<Run, AppError> {
    let root = project_root(&app, &id)?;
    chat::propose(&*engine(&app)?, &root, &run_id, &rule, &text)?;
    runner.status(&root, &run_id)
}
