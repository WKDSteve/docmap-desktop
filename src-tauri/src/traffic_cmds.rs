//! The commands of the GitHub traffic view (`github_stats.nvim`'s digest)
//!
//! Read-only, and nothing here talks to GitHub or handles a token. The logic
//! lives in `traffic.rs` and is testable without an app; what is here is the
//! part that needs one: the workspace lookup, the settings, and the one
//! subprocess ("Ask Neovim"). See `docs/FEATURES/TRAFFIC.md`.

use super::*;

/// The discovery chain's starting points, from the saved settings.
pub(crate) fn traffic_sources(ws: &Workspace) -> traffic::Sources {
    traffic::Sources {
        explicit: ws.traffic_dir.clone(),
        default_dir: traffic::default_data_dir(),
        asked: ws.traffic_asked_dir.clone(),
    }
}

/// What a project needs to resolve its repository: root, an already-known
/// `repo_url`, and whether it was opted out.
pub(crate) type TrafficProject = (String, Option<String>, bool);

/// [`TrafficProject`] of a project, or `None` for an id nobody added.
pub(crate) fn traffic_project(ws: &Workspace, id: &str) -> Option<TrafficProject> {
    ws.projects
        .iter()
        .find(|p| p.id == id)
        .map(|p| (p.root.clone(), p.repo_url.clone(), p.traffic_hidden))
}

/// The repository of a project, unless it was opted out — an opted-out project
/// resolves nothing, not even its remote, so no process is spawned for it.
pub(crate) fn traffic_repo(root: &str, repo_url: Option<&str>, hidden: bool) -> Option<String> {
    if hidden {
        None
    } else {
        traffic::repo_of(root, repo_url)
    }
}

/// One project's traffic, or the reason there is none.
///
/// `async` + `spawn_blocking`: the first ask for a project may run
/// `git remote get-url origin`, and a command that blocks freezes the window
/// it was invoked from. Every later ask is a cache hit.
#[tauri::command]
pub(crate) async fn traffic_info(
    app: tauri::AppHandle,
    id: String,
) -> Result<traffic::Info, AppError> {
    let ws = read_workspace(&app)?;
    let sources = traffic_sources(&ws);
    let (root, repo_url, hidden) =
        traffic_project(&ws, &id).ok_or_else(|| AppError::no_project(&id))?;

    tauri::async_runtime::spawn_blocking(move || {
        let repo = traffic_repo(&root, repo_url.as_deref(), hidden);
        traffic::info_for(repo.as_deref(), hidden, &sources)
    })
    .await
    .map_err(AppError::task)
}

/// The whole digest of one project, for the detail dialog: the daily series,
/// referrers and paths. `None` unless there is a readable one.
///
/// Each `paths` entry's `project_path` is resolved here, against this
/// project's own root — the one piece `traffic::detail` cannot do on its
/// own, since it only knows where the *digest* lives, never where the
/// repository it describes was checked out.
#[tauri::command]
pub(crate) async fn traffic_detail(
    app: tauri::AppHandle,
    id: String,
) -> Result<Option<traffic::Digest>, AppError> {
    let ws = read_workspace(&app)?;
    let sources = traffic_sources(&ws);
    let (root, repo_url, hidden) =
        traffic_project(&ws, &id).ok_or_else(|| AppError::no_project(&id))?;

    tauri::async_runtime::spawn_blocking(move || {
        let repo = traffic_repo(&root, repo_url.as_deref(), hidden);
        let mut digest = traffic::detail(repo.as_deref(), hidden, &sources);
        if let Some(d) = digest.as_mut() {
            let digest_repo = d.repo.clone();
            if let Some(paths) = d.paths.as_mut() {
                let project_root = Path::new(&root);
                for item in paths.iter_mut() {
                    item.project_path =
                        traffic::resolve_page_path(&item.path, &digest_repo, project_root);
                }
            }
        }
        digest
    })
    .await
    .map_err(AppError::task)
}

/// Every listed project's traffic numbers in one call — what the sort order
/// and the list column read. One async read for the whole list, never one
/// call per row.
#[tauri::command]
pub(crate) async fn traffic_list(
    app: tauri::AppHandle,
    ids: Vec<String>,
) -> Result<Vec<traffic::ListEntry>, AppError> {
    let ws = read_workspace(&app)?;
    let sources = traffic_sources(&ws);
    let wanted: Vec<(String, TrafficProject)> = ids
        .into_iter()
        .filter_map(|id| traffic_project(&ws, &id).map(|p| (id, p)))
        .collect();

    tauri::async_runtime::spawn_blocking(move || {
        // The discovery chain once for the whole list, not once per project.
        let located = traffic::locate(&sources).0;
        wanted
            .into_iter()
            .map(|(id, (root, repo_url, hidden))| {
                let repo = traffic_repo(&root, repo_url.as_deref(), hidden);
                let info = traffic::info_with(repo.as_deref(), hidden, located.as_ref());
                traffic::list_entry(&id, &info)
            })
            .collect()
    })
    .await
    .map_err(AppError::task)
}

/// What the Settings panel shows: the chosen folder, the answer Neovim gave,
/// and what the discovery chain found (or why not).
#[tauri::command]
pub(crate) fn traffic_settings(app: tauri::AppHandle) -> Result<traffic::Survey, AppError> {
    let ws = read_workspace(&app)?;
    Ok(traffic::survey(&traffic_sources(&ws)))
}

/// Choose (or clear) the folder the digest is read from.
///
/// The folder is read by this process only. It is **not** added to a Tauri fs
/// scope (`capabilities/default.json`): the webview asks for numbers through
/// the commands above and never gets a path it could read on its own.
#[tauri::command]
pub(crate) fn traffic_set_dir(
    app: tauri::AppHandle,
    path: Option<String>,
) -> Result<traffic::Survey, AppError> {
    let path = text(path);
    if let Some(ref p) = path {
        if !Path::new(p).is_dir() {
            return Err(AppError::not_dir(p));
        }
    }
    with_workspace(&app, |ws| {
        ws.traffic_dir = path.clone();
        Ok(())
    })?;
    traffic::forget_all();
    traffic_settings(app)
}

/// Ask Neovim where `github_stats.nvim` keeps its digest, and remember the
/// answer like a chosen folder.
///
/// One `nvim --headless` process, only on the button and never on a render —
/// modelled on `import_from_nvim_config`, including `spawn_blocking`. It asks
/// the *loaded plugin* rather than reading the path out of the user's
/// installation spec, which is Lua code (`opts` may be a function).
#[tauri::command]
pub(crate) async fn traffic_ask_neovim(app: tauri::AppHandle) -> Result<traffic::Survey, AppError> {
    let info = nvim_info(app.clone())?;
    let nvim = info.path.ok_or_else(|| AppError::new("err.nvim.none"))?;

    let out = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&nvim);
        cmd.args(["--headless", "-c", traffic::ASK_LUA, "-c", "qa"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        proc::run_with_timeout(&mut cmd, traffic::ASK_TIMEOUT).map_err(|e| e.into_nvim(&nvim))
    })
    .await
    .map_err(AppError::task)??;

    // `io.write` reaches stdout; a config's own startup noise may share it,
    // which is why the answer sits between markers.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let dir = traffic::parse_asked(&stdout, &String::from_utf8_lossy(&out.stderr))?;

    with_workspace(&app, |ws| {
        ws.traffic_asked_dir = Some(dir.clone());
        Ok(())
    })?;
    traffic::forget_all();
    traffic_settings(app)
}

/// Opt one project in or out of showing GitHub traffic.
///
/// Off means nothing is read, resolved or shown for it — including not the
/// `git` process that would find its remote.
#[tauri::command]
pub(crate) fn traffic_set_hidden(
    app: tauri::AppHandle,
    id: String,
    hidden: bool,
) -> Result<(), AppError> {
    with_workspace(&app, |ws| {
        let project = ws
            .projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::no_project(&id))?;
        project.traffic_hidden = hidden;
        Ok(())
    })
}

/// Forget every remembered origin and parsed digest, so the next look reads
/// disk and `git` again. What "Look again" calls.
#[tauri::command]
pub(crate) fn traffic_refresh() {
    traffic::forget_all();
}
