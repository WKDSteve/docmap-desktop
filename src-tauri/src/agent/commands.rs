//! The Tauri commands of the run: what a window calls. No window is opened
//! here; the run dialog and the run window are the next task's.
//!
//! A project is named by its `id` (its root path) as everywhere else; the
//! commands never take a path. The blocking ones (an engine process, a
//! request) run on Tauri's pool (`command(async)`), not on the main thread.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use super::client::{self, Client, Health};
use super::engine::{CliEngine, Engine};
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
