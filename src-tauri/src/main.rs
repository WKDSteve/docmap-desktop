// docmap-desktop — a project list and a window, in front of maps that
// something else generated.
//
// The whole backend is deliberately small. This program does not analyse
// anything and does not render anything: `docmap.nvim`'s standalone
// binary produces the map, and the map is a self-contained HTML page that
// renders itself. What is left for Rust is the part neither can do — remember
// which projects exist, and answer where a project's map lives on disk.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod apperr;
mod contract;
mod deps;
mod feedback;
mod filetree;
mod freshness;
mod github;
mod icon;
mod languages;
mod menu;
mod proc;
mod safe_read;
mod search;
mod server;
mod stats;
mod telemetry;
#[cfg(test)]
mod testutil;
mod traffic;

/// How long a button waits for a headless Neovim that loads the user's whole
/// configuration (import from the config, the telemetry folder question).
const NVIM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// `git clone --depth 1` of a repository the user pasted a URL for.
const CLONE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// `docmap --capabilities`: it answers in milliseconds, or it is hung.
const ENGINE_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use apperr::AppError;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_shell::ShellExt;

/// An [`AppError`] about a file or folder: `key`'s sentence names `path`, and
/// the OS error is the detail.
fn path_err(key: &'static str, path: &Path, e: impl std::fmt::Display) -> AppError {
    AppError::new(key).with("path", path.display()).detail(e)
}

/// An [`AppError`] about a project-relative path, as it was given.
fn path_key(key: &'static str, rel: &str) -> AppError {
    AppError::new(key).with("path", rel)
}

/// A path as this program shows it and passes it around: forward slashes,
/// and no `\?\`.
///
/// Windows hands back extended-length paths from `canonicalize()` and from
/// `resource_dir()`, and they are correct, unreadable, and leak into every
/// label, tooltip and error message. `add_project` has stripped the prefix
/// since it was written; twelve other conversions did not, and the one that
/// showed up was About reporting
/// `grammars: //?/C:/Program Files/docmap-desktop/grammars` — the prefix
/// half-eaten by the slash replacement that follows it.
///
/// **Measured before being called a bug:** the engine loads all four
/// grammars from `//?/C:/tools/docmap-grammars` exactly as it does from the
/// clean path, checked against a deliberately wrong path to prove the probe
/// discriminates. So this is cosmetic — which is a reason to fix it once,
/// centrally, rather than a reason to leave it.
pub(crate) fn portable(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/")
}

/// One entry in the sidebar.
///
/// `map_dir` is stored rather than derived so a project whose map lives
/// somewhere other than `docs/map` is representable later without a
/// migration. `id` is the absolute root path: two projects are the same
/// project exactly when they are the same directory, which is a truth the
/// filesystem already owns and this program should not invent a second answer
/// for.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Project {
    id: String,
    name: String,
    root: String,
    map_dir: String,
    /// Repository-relative paths the engine is told not to read.
    ///
    /// Per project rather than per machine, unlike everything in Settings:
    /// "my engine lives here" is a fact about this computer, "this
    /// repository vendors a copy of something" is a fact about the
    /// repository, and putting the second one in the machine's settings
    /// would apply one project's answer to all of them.
    ///
    /// `#[serde(default)]` on both of these fields, so a workspace file
    /// written before per-project settings existed loads unchanged. An empty
    /// list is the same as no list and needs no migration.
    #[serde(default)]
    exclude: Vec<String>,
    /// Which language backends the engine may use for this project, by their
    /// registered names, or `None` for all of them.
    ///
    /// `None` and `Some(vec![])` are deliberately **the same answer** here,
    /// matching the engine: an empty selection is a reader who has not
    /// chosen, not a reader asking for an empty map. The dialog can
    /// therefore un-tick everything without producing a project that reports
    /// nothing.
    #[serde(default)]
    languages: Option<Vec<String>>,
    /// Repository-relative directory the engine writes its artifacts to —
    /// `opts.out_dir`, passed as `--out-dir=`.
    ///
    /// `None` means the engine's own default, `docs/map`. Stored *relative*
    /// while `map_dir` above is absolute, and the two are kept in step by
    /// `project_flags_set`: everything in this program that opens a map
    /// wants the absolute path, and everything that talks to the engine
    /// wants the relative one, so storing one and deriving the other on
    /// every use would just move the conversion to a dozen call sites.
    ///
    /// The `Project` comment above says `map_dir` was stored "so a project
    /// whose map lives somewhere other than `docs/map` is representable
    /// later without a migration". This is that later: the field was always
    /// representable and never settable, because nothing wrote anything but
    /// `docs/map` into it and `--out-dir` was never passed at all.
    #[serde(default)]
    out_dir: Option<String>,
    /// Directory or directories the engine scans, relative to the root —
    /// `opts.source`, passed as `--source=`. Several are comma-separated.
    ///
    /// `None` leaves detection alone, which is right for almost every tree.
    /// It exists for the ones where detection is a wager the reader cannot
    /// correct: a repository with `lua/` beside `src/`, or sources under
    /// `packages/*`. `exclude` and `languages` were already correctable and
    /// this was not, which was an arbitrary place to stop.
    #[serde(default)]
    source: Option<String>,
    /// Base URL source links in the generated page are built from —
    /// `opts.repo_url`, passed as `--repo-url=`.
    ///
    /// Without it the page renders with no link from any module or function
    /// to its own source, silently. That was the visible difference between
    /// a map generated here and the same map generated by the same engine in
    /// CI, and nothing in the window explained it.
    #[serde(default)]
    repo_url: Option<String>,
    /// Branch those source links point at — `opts.branch`, `--branch=`.
    /// `None` means the engine's default, `main`.
    #[serde(default)]
    branch: Option<String>,
    /// Generate with `--full` (LuaLS enrichment) without being asked each
    /// time.
    ///
    /// A per-project default rather than a machine-wide one because the
    /// answer is a property of the tree: a Lua repository with
    /// `lua-language-server` installed wants it always, a TypeScript one
    /// gains nothing from it ever. **Generate full** stays as a command, so
    /// this only decides what the plain **Generate** does — including inside
    /// "generate all", which never offered the choice at all.
    #[serde(default)]
    full: bool,
    /// The project was opted out of GitHub traffic in this app.
    ///
    /// Per project and in this app rather than in `github_stats.nvim`, which
    /// has no notion of "the explorer": a private repository is one a person
    /// tracks in the plugin and still does not want on screen or in a
    /// screenshot. `true` means nothing is read, resolved or shown for the
    /// project (`traffic::Status::Disabled`). `#[serde(default)]`, so a
    /// workspace file from before this existed loads with it off.
    #[serde(default)]
    traffic_hidden: bool,
}

/// One project's engine settings, as the engine's flags want them.
///
/// Looked up from the workspace by `root` rather than passed in by the
/// caller, and that is the point: `generate`, `check_map` and the
/// generate-all loop all have to honour these, and a parameter is something
/// each of them can forget. The project is the owner of the setting, so the
/// lookup belongs on this side of the boundary.
///
/// A root that is not in the workspace is not an error — it is the ordinary
/// answer for a tree nobody has added — and yields the defaults, which is
/// exactly the behaviour before any of this existed.
///
/// **One struct rather than the tuple this used to return.** Two settings fit
/// in a tuple; six do not, and every call site would have to spell the order
/// right — which is the shape of bug that compiles fine and passes `branch`
/// where `repo_url` belongs, forever.
#[derive(Debug, Default, Clone)]
struct ProjectFlags {
    exclude: Vec<String>,
    languages: Option<Vec<String>>,
    out_dir: Option<String>,
    source: Option<String>,
    repo_url: Option<String>,
    branch: Option<String>,
    full: bool,
}

/// Refuse to run the engine on a project whose output directory is a link.
///
/// The engine writes `index.html`, `module_map.json` and `overview.md` into
/// that directory (and `--check` reads the committed ones from it), following
/// whatever the directory is. With `docs/map` checked out as a link, "no map
/// yet" - which is what the rest of this program now says about it - would
/// send an automatic *Generate* through the link into a directory, or to a
/// share, of the repository's choosing. The directory is the project's
/// `out_dir`, or `docs/map` when none is set.
///
/// What a `.docmap.json` in the repository says about `out_dir` is the
/// engine's to read, and so is refusing to write through a link there.
///
/// The project's own `out_dir` is vetted first, exactly as it is passed to the
/// engine. The dialog no longer takes a value the engine refuses, but one saved
/// before that (`../maps`, `docs/my map`) or put into `workspace.json` by hand
/// is still passed on. Generate and Check say so here, in the dialog's words,
/// instead of leaving it to the engine's message, which Check cannot tell from
/// a stale map.
fn refuse_linked_output(root: &str, flags: &ProjectFlags) -> Result<(), AppError> {
    if let Some(dir) = flags.out_dir.as_deref().filter(|dir| !dir.is_empty()) {
        if !is_safe_out_dir(dir, cfg!(windows)) {
            return Err(AppError::new("err.outDir.stored").with("value", dir));
        }
    }
    let root = Path::new(root);
    let rel = flags
        .out_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("docs/map");
    let dir = root.join(rel);
    if !map_dir_is_plain(root, &dir) {
        return Err(AppError::new("err.outDir.link").with("dir", rel));
    }
    // The files the engine writes are just as much the repository's: a link
    // that leaves the project (or reaches another machine) is refused, one that
    // stays inside it is not - `map_file` is the one that tells them apart. A
    // link whose target is not there cannot be told apart from one that leaves
    // (the engine would create the target wherever the link says), so it is
    // refused too.
    for name in OUTPUT_FILES {
        let is_link =
            fs::symlink_metadata(dir.join(name)).is_ok_and(|m| m.file_type().is_symlink());
        if is_link && map_file(root, &dir, name).is_none() {
            return Err(AppError::new("err.outDir.fileLink").with("file", format!("{rel}/{name}")));
        }
    }
    Ok(())
}

/// The files the engine writes into the output directory: the map
/// (`index.html`, `module_map.json`, `overview.md`) and the badge a
/// `.docmap.json` can ask for (`coverage.svg`). Keep in step with what the
/// engine writes (docmap.nvim, `lua/docmap/init.lua`).
const OUTPUT_FILES: [&str; 4] = [
    "index.html",
    "module_map.json",
    "overview.md",
    "coverage.svg",
];

fn project_flags(app: &tauri::AppHandle, root: &str) -> ProjectFlags {
    let normalised = root.replace('\\', "/");
    match read_workspace(app) {
        Ok(ws) => ws
            .projects
            .iter()
            .find(|p| p.root.replace('\\', "/") == normalised)
            .map(|p| ProjectFlags {
                exclude: p.exclude.clone(),
                languages: p.languages.clone(),
                out_dir: p.out_dir.clone(),
                source: p.source.clone(),
                repo_url: p.repo_url.clone(),
                branch: p.branch.clone(),
                full: p.full,
            })
            .unwrap_or_default(),
        Err(_) => ProjectFlags::default(),
    }
}

/// Add this project's flags to a command, if it asked for any.
///
/// Nothing is passed when nothing was chosen, rather than an empty
/// `--languages=`: the engine reads an empty list as "all", so both spellings
/// work, and the one that adds no argument keeps the command line a reader
/// sees in a bug report equal to the one they would have typed. Every option
/// below follows that rule.
///
/// `--full` is **not** here, deliberately. It is a property of the
/// *invocation* rather than of the scope, and `generate_full` passes it
/// explicitly — a project with `full` set would otherwise pass it twice.
///
/// **These beat anything the repository states about itself.** The engine
/// now reads a `.docmap.json` at the project root, and a flag wins over that
/// file by design: the person holding the window is answering for this
/// machine, and the file is answering for everyone. That order is the
/// engine's, not this program's — see `config/file.lua` there.
fn apply_flags(cmd: &mut std::process::Command, flags: &ProjectFlags) {
    for path in &flags.exclude {
        if !path.trim().is_empty() {
            cmd.arg(format!("--exclude={}", path.trim()));
        }
    }
    if let Some(names) = &flags.languages {
        if !names.is_empty() {
            cmd.arg(format!("--languages={}", names.join(",")));
        }
    }
    // Passed as stored: `project_flags_set` already normalised every one of
    // these on the way in, so cleaning them again here would be a second
    // place for the two to disagree about what the stored value means.
    for (flag, value) in [
        ("--out-dir", &flags.out_dir),
        ("--source", &flags.source),
        ("--repo-url", &flags.repo_url),
        ("--branch", &flags.branch),
    ] {
        if let Some(v) = value {
            if !v.is_empty() {
                cmd.arg(format!("{flag}={v}"));
            }
        }
    }
}

/// Settings live beside the project list rather than in a second file: there
/// is one workspace, and splitting it would mean two things to keep in step.
///
/// `engine` is a path to `docmap.nvim`'s standalone binary. Not
/// bundled yet, and that is the open question this slice deliberately does
/// not answer — shipping it per platform is the better experience and the
/// larger release problem. Until then: found on `PATH`, or pointed at.
///
/// `grammars` is optional and decides fidelity, not success. With a
/// directory of compiled tree-sitter grammars the engine produces
/// function-level data; without one it still produces a complete module
/// tree and says so. Passing it through as `DOCMAP_TS_DIR` is the whole
/// integration.
///
/// `nvim_path`/`nvim_config_dir` back the spec-import feature: not every
/// machine has the same Neovim config in the same place, so both are
/// configurable the same way `engine`/`grammars` are, rather than assumed.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Workspace {
    projects: Vec<Project>,
    #[serde(default)]
    engine: Option<String>,
    #[serde(default)]
    grammars: Option<String>,
    #[serde(default)]
    nvim_path: Option<String>,
    #[serde(default)]
    nvim_config_dir: Option<String>,
    /// A folder the user chose as the home of `github_stats.nvim`'s traffic
    /// digest — either the plugin's data folder (has `root.json`), a
    /// `digest_dir`, or the `digest/` folder itself. First step of the
    /// discovery chain in `traffic.rs`; `None` falls through to the rest.
    #[serde(default)]
    traffic_dir: Option<String>,
    /// What Neovim answered when asked where the digest is (the "Ask Neovim"
    /// button), kept like a chosen folder: one process on a button, never on
    /// a render. Third step of the chain.
    #[serde(default)]
    traffic_asked_dir: Option<String>,
    /// How to open a source file in an editor, as a command template.
    ///
    /// `{file}` and `{line}` are substituted; anything else is passed
    /// through. `None` means "whatever the desktop opens this file with",
    /// which is a real answer rather than a missing setting — it is what
    /// double-clicking the file in a file manager would do.
    #[serde(default)]
    editor: Option<String>,
    /// Where ai-desktop listens, for the rules agent (`agent/`). `None` is its
    /// default port; only an address on this machine is ever stored.
    #[serde(default)]
    ai_desktop_url: Option<String>,
    /// Which named workspace is loaded.
    ///
    /// Lives beside the settings rather than inside a workspace, because
    /// "which set of projects am I looking at" is a property of this
    /// machine — the same reasoning that keeps theme and engine paths out
    /// of a workspace. `None` on a file written before workspaces existed.
    #[serde(default)]
    active: Option<String>,
}

/// The name of the workspace a fresh install starts in.
///
/// Named rather than empty so the dashboard has something to show and the
/// file on disk has something to be called. It is renameable like any other.
const DEFAULT_WORKSPACE: &str = "Default";

/// A workspace name as a filename.
///
/// A name is typed by a person and becomes a path, which is the shape of
/// every directory-traversal bug ever written. Everything outside this set
/// becomes `_`; a name that reduces to nothing falls back to the default
/// rather than writing to the directory itself.
fn workspace_file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // No leading-dot guard: a dot is not in the allowed set, so it has
    // already become `_` above. One was written here first and removed for
    // suggesting a protection that nothing needs.
    let trimmed = safe.trim().to_string();
    if trimmed.is_empty() {
        DEFAULT_WORKSPACE.to_string()
    } else {
        trimmed
    }
}

fn workspaces_dir(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::new("err.config.noDir").detail(e))?
        .join("workspaces");
    fs::create_dir_all(&dir).map_err(|e| path_err("err.path.create", &dir, e))?;
    Ok(dir)
}

fn workspace_projects_path(app: &tauri::AppHandle, name: &str) -> Result<PathBuf, AppError> {
    Ok(workspaces_dir(app)?.join(format!("{}.json", workspace_file_name(name))))
}

/// Every workspace that exists, by name, sorted.
///
/// Read from the directory rather than from an index kept beside it: an
/// index is a second thing to keep in step, and the answer to "which
/// workspaces are there" is already spelled by the files.
pub fn workspace_names(app: &tauri::AppHandle) -> Result<Vec<String>, AppError> {
    let dir = workspaces_dir(app)?;
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    if names.is_empty() {
        names.push(DEFAULT_WORKSPACE.to_string());
    }
    names.sort_by_key(|n| n.to_lowercase());
    Ok(names)
}

/// Where the project list lives.
///
/// `app_config_dir` rather than a file beside the executable: an installed
/// app has no business writing into Program Files, and a portable copy on a
/// USB stick should still find the same list on the same machine.
fn workspace_path(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::new("err.config.noDir").detail(e))?;
    fs::create_dir_all(&dir).map_err(|e| path_err("err.path.create", &dir, e))?;
    Ok(dir.join("workspace.json"))
}

/// Write a file so that a reader sees the old content or the new one, never
/// half of it.
///
/// `fs::write` truncates first. A command on another thread that read the
/// workspace in that moment saw an empty or partial list and failed on it -
/// or, where it treats a failed read as "nothing known", acted on that.
fn write_atomic(path: &Path, body: impl AsRef<[u8]>) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, body)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// The settings file, without the project list attached.
fn read_settings(app: &tauri::AppHandle) -> Result<Workspace, AppError> {
    let path = workspace_path(app)?;
    match fs::read_to_string(&path) {
        Ok(body) => {
            serde_json::from_str(&body).map_err(|e| path_err("err.workspace.unreadable", &path, e))
        }
        // A missing file is the first-run case, not an error. An empty
        // workspace is exactly what a first run should see.
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Workspace::default()),
        Err(e) => Err(path_err("err.path.read", &path, e)),
    }
}

/// The settings, with the active workspace's projects attached.
///
/// **Every command above and below this still sees one `Workspace`.** The
/// split — settings in one file, each project list in its own — is entirely
/// inside this function and its writer, which is why adding workspaces
/// changed no command that operates on projects.
///
/// Migration happens here and is not asked about: a file written before
/// workspaces existed carries its projects inline, and they become the
/// first workspace. A feature whose first act is losing somebody's project
/// list is not a feature.
fn read_workspace(app: &tauri::AppHandle) -> Result<Workspace, AppError> {
    let mut ws = read_settings(app)?;
    let name = ws
        .active
        .clone()
        .unwrap_or_else(|| DEFAULT_WORKSPACE.to_string());
    let path = workspace_projects_path(app, &name)?;

    if path.is_file() {
        let body = fs::read_to_string(&path).map_err(|e| path_err("err.path.read", &path, e))?;
        ws.projects = serde_json::from_str(&body)
            .map_err(|e| path_err("err.workspace.listUnreadable", &path, e))?;
    } else if !ws.projects.is_empty() {
        // Pre-workspace file with projects inline. Written out under the
        // active name so the next read finds it where it now belongs.
        let body = serde_json::to_string_pretty(&ws.projects)
            .map_err(|e| AppError::new("err.workspace.serialise").detail(e))?;
        write_atomic(&path, body).map_err(|e| path_err("err.path.write", &path, e))?;
    }

    ws.active = Some(name);
    Ok(ws)
}

fn write_workspace(app: &tauri::AppHandle, ws: &Workspace) -> Result<(), AppError> {
    let name = ws
        .active
        .clone()
        .unwrap_or_else(|| DEFAULT_WORKSPACE.to_string());

    // The project list, in its own file. Written first: if the settings
    // write fails afterwards the projects are still saved, which is the
    // right way round for the half nobody can retype.
    let list_path = workspace_projects_path(app, &name)?;
    let list = serde_json::to_string_pretty(&ws.projects)
        .map_err(|e| AppError::new("err.workspace.serialise").detail(e))?;
    write_atomic(&list_path, list).map_err(|e| path_err("err.path.write", &list_path, e))?;

    // The settings, with the list left out — it would be a stale second
    // copy the moment anything is added to the real one.
    let mut settings = Workspace {
        projects: Vec::new(),
        active: Some(name),
        ..Default::default()
    };
    settings.engine = ws.engine.clone();
    settings.grammars = ws.grammars.clone();
    settings.nvim_path = ws.nvim_path.clone();
    settings.nvim_config_dir = ws.nvim_config_dir.clone();
    settings.traffic_dir = ws.traffic_dir.clone();
    settings.traffic_asked_dir = ws.traffic_asked_dir.clone();
    settings.editor = ws.editor.clone();
    settings.ai_desktop_url = ws.ai_desktop_url.clone();

    let path = workspace_path(app)?;
    let body = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppError::new("err.workspace.serialise").detail(e))?;
    write_atomic(&path, body).map_err(|e| path_err("err.path.write", &path, e))
}

/// Guards every workspace.json read-modify-write cycle. Two commands that
/// each read, mutate, and write independently (e.g. "Add project" clicked
/// while "Import from URL" is still cloning) would otherwise race and one
/// silently drop the other's change.
static WORKSPACE_LOCK: Mutex<()> = Mutex::new(());

/// Read the workspace, let `f` mutate it, write it back -- one lock held for
/// the whole cycle. Centralizes the read-modify-write shape every mutating
/// command below used to repeat by hand, and is what actually closes the
/// race: the lock, not the shared function.
fn with_workspace<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&mut Workspace) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WORKSPACE_LOCK
        .lock()
        .map_err(|_| AppError::new("err.lock"))?;
    let mut ws = read_workspace(app)?;
    let result = f(&mut ws)?;
    write_workspace(app, &ws)?;
    Ok(result)
}

#[tauri::command]
fn list_projects(app: tauri::AppHandle) -> Result<Vec<Project>, AppError> {
    Ok(read_workspace(&app)?.projects)
}

/// Resolve and add one directory to an in-memory workspace, without touching
/// disk itself — `add_project` wraps this in a single `with_workspace` for
/// the ordinary one-at-a-time case, and `import_many` loops it inside *one*
/// `with_workspace` instead of locking, reading and writing the workspace
/// file once per directory.
///
/// Returns the added `Project`, or `None` when the canonical path was
/// already present — adding the same directory twice is a no-op rather than
/// an error, the same reasoning `add_project`'s doc comment states. Does
/// **not** sort `ws.projects` or require a map to exist yet; the caller sorts
/// once after its own batch, and reporting "no map here" in the view is more
/// useful than refusing to add the project.
fn add_one(ws: &mut Workspace, root: &str) -> Result<Option<Project>, AppError> {
    add_one_named(ws, root, None)
}

/// [`add_one`] with the sidebar name given rather than taken from the folder:
/// an import from a URL names the project after the repository, not after
/// its clone folder (see [`imported_name`]). `None` is the folder name.
fn add_one_named(
    ws: &mut Workspace,
    root: &str,
    name: Option<String>,
) -> Result<Option<Project>, AppError> {
    let root_path = Path::new(root);
    if !root_path.is_dir() {
        return Err(AppError::not_dir(root));
    }
    let canonical = portable(&fs::canonicalize(root_path).map_err(|e| {
        AppError::new("err.path.resolve")
            .with("path", root)
            .detail(e)
    })?);

    if ws.projects.iter().any(|p| p.id == canonical) {
        return Ok(None);
    }

    let name = name.unwrap_or_else(|| {
        root_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| canonical.clone())
    });

    let project = Project {
        id: canonical.clone(),
        name,
        root: canonical.clone(),
        map_dir: format!("{canonical}/docs/map"),
        exclude: Vec::new(),
        languages: None,
        // Every engine setting starts unset, meaning "whatever the engine
        // decides" — which includes whatever the repository states in its
        // own `.docmap.json`. A project added here therefore behaves exactly
        // as `docmap <root>` would on a command line, and the dialog is
        // where somebody departs from that on purpose.
        out_dir: None,
        source: None,
        repo_url: None,
        branch: None,
        full: false,
        traffic_hidden: false,
    };
    ws.projects.push(project.clone());
    Ok(Some(project))
}

/// Add a directory to the workspace. See `add_one` for what actually happens;
/// this is the single-directory case wrapped in its own lock-read-write.
#[tauri::command]
fn add_project(app: tauri::AppHandle, root: String) -> Result<Vec<Project>, AppError> {
    with_workspace(&app, |ws| {
        add_one(ws, &root)?;
        ws.projects.sort_by_key(|p| p.name.to_lowercase());
        Ok(ws.projects.clone())
    })
}

/// One candidate directory found under a folder someone pointed at, before
/// it becomes a `Project`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubRepo {
    name: String,
    path: String,
    /// Has a `.git` entry — a plain clone or a worktree, either is a file or
    /// a directory named `.git`. Informational only: a directory without one
    /// is still offered, because not everything worth mapping is a checkout.
    is_git: bool,
    already_added: bool,
}

/// What picking a single folder in the add dialog needs to know before it
/// decides between "add this one" and "list what is inside it".
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderScan {
    /// The folder itself has a `.git` entry, so it is a project on its own
    /// rather than a container of several.
    is_git: bool,
    /// Immediate subdirectories, alphabetical. Hidden ones (leading `.`) are
    /// left out — `.git` itself included — the same as everything else in
    /// this program's own directory listings.
    subrepos: Vec<SubRepo>,
}

/// Immediate, non-hidden subdirectories of `root` — name, canonical path,
/// and whether a `.git` entry sits in it — alphabetical by name.
///
/// Pure and `AppHandle`-free on purpose: the one thing here worth getting
/// wrong is the filesystem walk, and that is what a test can hold a tempdir
/// up to without building a mock app around it, the same split `languages.rs`
/// and `filetree.rs` already make.
///
/// A directory that vanishes or resolves to nowhere between listing and
/// canonicalising is skipped rather than aborting the whole scan — a stale
/// symlink two directories down should not blank out the other thirty-one.
fn list_subdirs(root: &Path) -> Result<Vec<(String, String, bool)>, AppError> {
    let mut out = Vec::new();
    // Verbatim, so that every path built from it below is looked at literally:
    // on the typed folder Win32 rewrites a name (`evil.` is `evil`, `NUL` is a
    // device) before the file system sees it, and a link's target that names
    // one was looked at as something other than what the kernel then follows.
    let root = fs::canonicalize(root).map_err(|e| path_err("err.path.read", root, e))?;
    let root = root.as_path();
    let entries = fs::read_dir(root).map_err(|e| path_err("err.path.read", root, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| path_err("err.path.read", root, e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || languages::win32_rewrites_name(&entry.file_name()) {
            continue;
        }
        // The folder picked is somebody's repository, so its entries are
        // repository content. `file_type` does not follow a link; `is_dir` and
        // `canonicalize` below do, and a link to `\\host\share` made them
        // connect there (a 20 second stall of the window and an NTLM
        // negotiation) before the dialog had shown anything. A link is
        // followed only where it provably stays on this machine - a folder of
        // plugins with a junction to another drive is still a folder of plugins.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_symlink() {
            if !link_stays_local(root, &path) {
                continue;
            }
        } else if !file_type.is_dir() {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        let canonical = match fs::canonicalize(&path) {
            Ok(c) => portable(&c),
            Err(_) => continue,
        };
        // Looked at, not followed: see `languages::is_nested_checkout`.
        let is_git = languages::has_git_entry(&path);
        out.push((name, canonical, is_git));
    }
    out.sort_by_key(|entry| entry.0.to_lowercase());
    Ok(out)
}

/// Look at one folder without adding anything: is it a repository itself, or
/// does it hold several — `$REPOS_DIR` with three dozen plugins in it, say.
///
/// Read-only, so the dialog can call it the moment a folder is picked and
/// decide what to show next, the same way `list_github_repos` is opt-in but
/// free of side effects.
///
/// `async` so that a slow disk (a share, a folder of thousands of entries)
/// stalls a worker and not the window.
#[tauri::command(async)]
fn inspect_folder(app: tauri::AppHandle, root: String) -> Result<FolderScan, AppError> {
    let root_path = Path::new(&root);
    if !root_path.is_dir() {
        return Err(AppError::not_dir(&root));
    }

    // Looked at, not followed: see `languages::is_nested_checkout`.
    let is_git = languages::has_git_entry(root_path);
    // A repository is added as itself and the dialog never shows its
    // subdirectories, so they are not listed.
    if is_git {
        return Ok(FolderScan {
            is_git,
            subrepos: Vec::new(),
        });
    }

    let existing_ids: std::collections::HashSet<String> = read_workspace(&app)?
        .projects
        .iter()
        .map(|p| p.id.clone())
        .collect();

    let subrepos = list_subdirs(root_path)?
        .into_iter()
        .map(|(name, path, is_git)| SubRepo {
            already_added: existing_ids.contains(&path),
            name,
            path,
            is_git,
        })
        .collect();

    Ok(FolderScan { is_git, subrepos })
}

/// Add every one of the given directories, the same way `add_project` adds
/// one — same rules, same idempotency — but through a single lock-read-write
/// of the workspace file rather than one per directory. A folder of thirty
/// plugins used to mean thirty full read-modify-write cycles of
/// `workspace.json`, each behind the same mutex; `add_one` lets this batch
/// share one.
///
/// One bad entry does not fail the rest: each `add_one` result is collected
/// on its own, the same isolation `import_from_nvim_config`'s loop already
/// gives its own batch.
#[tauri::command]
fn import_many(app: tauri::AppHandle, roots: Vec<String>) -> Result<ImportResult, AppError> {
    let mut added = Vec::new();
    let mut already_present = 0usize;
    let mut errors = Vec::new();

    with_workspace(&app, |ws| {
        for root in &roots {
            match add_one(ws, root) {
                Ok(Some(project)) => added.push(project),
                Ok(None) => already_present += 1,
                Err(error) => errors.push(ImportFailure {
                    name: root.clone(),
                    error,
                }),
            }
        }
        ws.projects.sort_by_key(|p| p.name.to_lowercase());
        Ok(())
    })?;

    Ok(ImportResult {
        found: roots.len(),
        added,
        already_present,
        errors,
    })
}

/// One project's settings, for the dialog to render.
///
/// Its own command rather than reading it off the `Project` the frontend
/// already holds: `list_projects` is called on every workspace change and
/// its result is passed around widely, and a dialog that edited a copy of
/// that object would be one stale list away from writing back settings the
/// user changed in another window.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectSettings {
    exclude: Vec<String>,
    languages: Option<Vec<String>>,
    out_dir: Option<String>,
    source: Option<String>,
    repo_url: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    full: bool,
}

#[tauri::command]
fn project_scope_get(app: tauri::AppHandle, id: String) -> Result<ProjectSettings, AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;
    Ok(ProjectSettings {
        exclude: project.exclude.clone(),
        languages: project.languages.clone(),
        out_dir: project.out_dir.clone(),
        source: project.source.clone(),
        repo_url: project.repo_url.clone(),
        branch: project.branch.clone(),
        full: project.full,
    })
}

/// A repository-relative path as the engine wants it, or `None`.
///
/// **Normalised here, not in the dialog.** A path typed by a person arrives
/// with backslashes on Windows, a leading `./`, a trailing slash, or
/// surrounding whitespace, and the engine matches on an exact
/// forward-slashed repository-relative path. Doing it on this side means
/// every future caller gets it right, and means the stored value is the one
/// that will actually be passed — so a reader comparing the dialog with a
/// bug report sees the same string twice.
///
/// An empty result is `None` rather than `Some("")`: the two would reach the
/// engine as "default" and as `--out-dir=`, and only one of those is a thing
/// anybody meant.
fn rel_path(raw: Option<String>) -> Option<String> {
    let cleaned = raw?
        .trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Would the engine take `dir` as its map directory?
///
/// The rule of docmap.nvim's `core/safe_out_dir.lua` (the engine refuses
/// any other `--out-dir`, "not a safe relative path"), and of the one name
/// check its `core/safe_fs.lua` adds on Windows. Keep them in step: this is
/// what keeps the dialog from taking a value that could only fail on the next
/// Generate. Every name must be made of ASCII letters, digits, `-`, `_` and
/// `.`, and must not be `.` or `..`; a leading separator makes the path
/// absolute, and a drive (`C:x`) holds a `:`, which no name may. Empty names
/// are collapsed (`docs//map`), but one name has to be left. Windows drops a
/// trailing dot before the file system sees a name, so there `map.` is
/// refused; a trailing space is outside the whitelist already.
///
/// `windows` is a parameter so that both answers can be tested anywhere.
fn is_safe_out_dir(dir: &str, windows: bool) -> bool {
    let dir = dir.replace('\\', "/");
    if dir.starts_with('/') {
        return false;
    }
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_');
    let mut names = dir.split('/').filter(|name| !name.is_empty()).peekable();
    names.peek().is_some()
        && names.all(|name| {
            name != "."
                && name != ".."
                && name.chars().all(plain)
                && !(windows && name.ends_with('.'))
        })
}

/// Why `value` is not a map directory, in the words the dialog shows.
fn map_dir_refusal(value: &str) -> AppError {
    AppError::new("err.outDir.unsafe").with("value", value)
}

/// The Map directory setting, or why it cannot be one.
///
/// What the engine refuses is refused here ([`is_safe_out_dir`]), on the value
/// that will be stored and passed to it: `  ./out/ ` is stored as `out`, which
/// is fine. The one thing checked on the raw text is a leading `/`, because
/// [`rel_path`] trims it, which would turn `/maps` into the project-relative
/// `maps` and hide that an absolute path was meant.
fn map_dir_setting(raw: Option<String>) -> Result<Option<String>, AppError> {
    let typed = raw.as_deref().unwrap_or("").trim().replace('\\', "/");
    let dir = rel_path(raw);
    let unsafe_dir = dir
        .as_deref()
        .is_some_and(|dir| !is_safe_out_dir(dir, cfg!(windows)));
    if typed.starts_with('/') || unsafe_dir {
        return Err(map_dir_refusal(&typed));
    }
    Ok(dir)
}

/// A free-text setting, trimmed, with empty meaning "unset".
fn text(raw: Option<String>) -> Option<String> {
    let cleaned = raw?.trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// A base URL for source links.
///
/// Its own helper rather than `rel_path`, and the difference is not
/// cosmetic: a URL's `//` after the scheme and its leading `/` on a
/// host-relative path are not separators to be collapsed, and `rel_path`
/// would take both. Only the trailing slash goes, because the engine
/// concatenates a path onto this and two slashes in the middle of a link is
/// the one artefact a reader actually sees.
fn base_url(raw: Option<String>) -> Option<String> {
    let cleaned = text(raw)?.trim_end_matches('/').to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Write one project's settings.
///
/// An empty `languages` list is stored as `None`: the two mean the same
/// thing to the engine, and keeping one spelling means the dialog can
/// un-tick everything without inventing a third state.
///
/// **`map_dir` is rewritten from `out_dir` here.** It is the absolute path
/// every reader in this program opens the map through — the picker, the
/// freshness walk, the dependency scan — and it was previously written once,
/// at add time, and never again. A project pointed at a different `out_dir`
/// with a stale `map_dir` would generate into one directory and read from
/// another, which is the most confusing failure this dialog could produce:
/// everything succeeds and the window shows the old map.
// One argument per field of the settings dialog: Tauri maps the named
// arguments of a command to the keys of the object `src/main.js` sends, so
// folding them into a struct would change that wire format for no gain.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn project_scope_set(
    app: tauri::AppHandle,
    id: String,
    exclude: Vec<String>,
    languages: Option<Vec<String>>,
    out_dir: Option<String>,
    source: Option<String>,
    repo_url: Option<String>,
    branch: Option<String>,
    full: Option<bool>,
) -> Result<ProjectSettings, AppError> {
    let cleaned: Vec<String> = exclude
        .iter()
        .cloned()
        .filter_map(|p| rel_path(Some(p)))
        .collect();
    let langs = languages.filter(|l| !l.is_empty());
    let out = map_dir_setting(out_dir)?;
    // Several sources are comma-separated, so this is cleaned segment by
    // segment rather than as one path — `--source=lua, src/` has to reach
    // the engine as `lua,src`, and `rel_path` over the whole string would
    // only trim its two ends.
    let src = text(source).map(|s| {
        s.split(',')
            .filter_map(|part| rel_path(Some(part.to_string())))
            .collect::<Vec<_>>()
            .join(",")
    });
    let src = src.filter(|s| !s.is_empty());
    let url = base_url(repo_url);
    let br = text(branch);
    let want_full = full.unwrap_or(false);

    with_workspace(&app, |ws| {
        let project = ws
            .projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::no_project(&id))?;
        project.exclude = cleaned.clone();
        project.languages = langs.clone();
        project.out_dir = out.clone();
        project.source = src.clone();
        project.repo_url = url.clone();
        project.branch = br.clone();
        project.full = want_full;
        // Its origin may be what the edit is about; the remembered answer is
        // cheap to ask for again.
        traffic::forget(&project.root);
        project.map_dir = format!(
            "{}/{}",
            project.root.replace('\\', "/").trim_end_matches('/'),
            out.clone().unwrap_or_else(|| "docs/map".to_string())
        );
        Ok(ProjectSettings {
            exclude: cleaned.clone(),
            languages: langs.clone(),
            out_dir: out.clone(),
            source: src.clone(),
            repo_url: url.clone(),
            branch: br.clone(),
            full: want_full,
        })
    })
}

#[tauri::command]
fn remove_project(app: tauri::AppHandle, id: String) -> Result<Vec<Project>, AppError> {
    with_workspace(&app, |ws| {
        if let Some(p) = ws.projects.iter().find(|p| p.id == id) {
            traffic::forget(&p.root);
        }
        ws.projects.retain(|p| p.id != id);
        Ok(ws.projects.clone())
    })
}

/// Where cloned repositories land. A subdirectory of the same app-config
/// directory `workspace.json` lives in, not the system temp dir: a clone
/// this app made is something to keep and reopen, not scratch space.
fn repos_cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::new("err.config.noDir").detail(e))?
        .join("repos");
    fs::create_dir_all(&dir).map_err(|e| path_err("err.path.create", &dir, e))?;
    Ok(dir)
}

/// The transports a pasted URL may use. `GIT_ALLOW_PROTOCOL` makes git refuse
/// every other one (`ext::` runs a command, `file:` reads the local disk), so
/// a URL that slips past [`clone_url_problem`] still cannot reach them.
const CLONE_PROTOCOLS: &str = "https:http:ssh:git";

/// Why `url` may not be handed to `git clone`, or `None` when it may.
///
/// The URL is user input that becomes an argument of another program, so the
/// shape is decided here and not left to git: a URL that starts with `-` is
/// read by git as an option (`--upload-pack=…` runs a command), and a
/// `<transport>::<address>` URL selects a remote helper. What stays allowed is
/// what a user pastes from a hosting page: `https://`, `http://`, `ssh://`,
/// `git://` and the scp form `user@host:path`.
fn clone_url_problem(url: &str) -> Option<&'static str> {
    if url.is_empty() || url.len() > 2048 {
        return Some("err.url.empty");
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Some("err.url.space");
    }
    if url.starts_with('-') {
        return Some("err.url.dash");
    }
    // git reads `<name>::<address>` as a remote helper only when the URL starts
    // with scheme characters and then `::`; an IPv6 literal (`https://[::1]/r`)
    // has `::` elsewhere and is a plain address.
    let scheme_len = url
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'))
        .count();
    if url[scheme_len..].starts_with("::") {
        return Some("err.url.helper");
    }
    let lower = url.to_ascii_lowercase();
    let known_scheme = ["https://", "http://", "ssh://", "git://"]
        .iter()
        .any(|s| lower.starts_with(s));
    if known_scheme {
        return None;
    }
    // scp form: `user@host:path`. The part before the first ':' must not
    // contain '/' (that would be a relative path), and there must be a path.
    if let Some((host, path)) = url.split_once(':') {
        if !host.is_empty() && !path.is_empty() && !host.contains('/') && host.contains('@') {
            return None;
        }
    }
    Some("err.url.scheme")
}

/// The directory name a clone of `url` should land in: the URL's last path
/// segment, minus a trailing `.git`.
///
/// The name becomes a path under the cache directory, so only a plain name
/// passes: letters, digits, `.`, `_` and `-`, never `.` or `..`, no separator
/// (a backslash is one on Windows), no `:` (a drive or a stream on Windows).
/// Whether `url` is actually clonable stays git's decision.
fn repo_dir_name(url: &str) -> Option<String> {
    let trimmed = url.trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    let plain = !name.is_empty()
        && name.len() <= 100
        && name != "."
        && name != ".."
        && !name.ends_with('.')
        && !is_windows_device_name(name)
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    plain.then(|| name.to_string())
}

/// Hosts whose owner and repository names are case-insensitive: the same
/// repository is reached as `github.com/Owner/Name` and `github.com/owner/name`.
/// Elsewhere the path keeps its case, because a plain git server serves files
/// from a file system that may well tell `Foo` from `foo`.
const CASE_INSENSITIVE_HOSTS: [&str; 4] =
    ["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"];

/// The part of `url` two imports must share to be the same repository, as
/// `https://<host>[:<port>]/<path>`.
///
/// One repository is offered in many spellings, and each must find the one
/// clone: `https://`, `http://`, `ssh://`, `git://` and the scp form
/// `user@host:path` all name the same host and path, so the scheme and any
/// user or password are left out. The host is compared in lower case, the
/// path too on [`CASE_INSENSITIVE_HOSTS`]. A port stays only where it is not
/// the scheme's default; an ssh port never does, since ssh runs on a port of
/// its own beside the web one. A trailing `/` and `.git` (in any case) go.
/// `https://github.com/a/foo` is its own key, so clones made before this
/// normalisation keep their folder names.
fn clone_url_key(url: &str) -> String {
    let (scheme, rest) = match url.find("://") {
        Some(i) => (url[..i].to_ascii_lowercase(), &url[i + 3..]),
        None => (String::from("scp"), url),
    };
    let (authority, path) = if scheme == "scp" {
        // `user@host:path`, the host possibly a bracketed IPv6 address.
        let host_start = rest.find('@').map_or(0, |i| i + 1);
        let colon = if rest[host_start..].starts_with('[') {
            rest[host_start..].find(']').and_then(|end| {
                rest[host_start + end..]
                    .find(':')
                    .map(|c| host_start + end + c)
            })
        } else {
            rest[host_start..].find(':').map(|c| host_start + c)
        };
        match colon {
            Some(c) => (&rest[..c], &rest[c + 1..]),
            None => (rest, ""),
        }
    } else {
        rest.split_once('/').unwrap_or((rest, ""))
    };
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = if host_port.starts_with('[') {
        match host_port.find(']') {
            Some(end) => (&host_port[..=end], host_port[end + 1..].strip_prefix(':')),
            None => (host_port, None),
        }
    } else {
        match host_port.rsplit_once(':') {
            Some((h, p)) if p.bytes().all(|b| b.is_ascii_digit()) => (h, Some(p)),
            _ => (host_port, None),
        }
    };
    let host = host.to_ascii_lowercase();
    let port = match (scheme.as_str(), port) {
        (_, None | Some("")) | ("ssh" | "scp", _) => String::new(),
        ("https", Some("443")) | ("http", Some("80")) | ("git", Some("9418")) => String::new(),
        (_, Some(p)) => format!(":{p}"),
    };
    let path = path.trim_matches('/');
    let path = match path.len().checked_sub(4) {
        Some(cut) if path.is_char_boundary(cut) && path[cut..].eq_ignore_ascii_case(".git") => {
            &path[..cut]
        }
        _ => path,
    };
    let path = path.trim_end_matches('/');
    let path = if CASE_INSENSITIVE_HOSTS.contains(&host.as_str()) {
        path.to_ascii_lowercase()
    } else {
        path.to_owned()
    };
    format!("https://{host}{port}/{path}")
}

/// The folder a new clone of `url` lands in: [`repo_dir_name`] of the
/// [`clone_url_key`] plus `-` and eight hex digits of a hash of that key, e.g.
/// `foo-1a2b3c4d`. Every spelling of one repository gets the same name.
///
/// A hash of the whole URL rather than the owner: the owner alone does not
/// tell `github.com/a/foo` from `gitlab.com/a/foo`, and an owner (or a host, a
/// port, a group path) can hold characters a plain folder name may not, so it
/// would need its own escaping and could still collide after it. The hash
/// covers every part of the URL and only adds `[0-9a-f-]` to a name that is
/// already plain, so the result is plain too. FNV-1a, written out, because the
/// name must stay the same across builds (`DefaultHasher` makes no such
/// promise); 32 bits is plenty for the handful of same-named repositories one
/// user imports, and short enough to keep the name readable. It is not enough
/// to trust: two URLs can share a hash (FNV is not keyed, so a colliding URL
/// can even be searched for), which is why [`clone_target`] checks the
/// `origin` of a folder of this name before it takes it.
fn clone_dir_name(url: &str) -> Option<String> {
    let key = clone_url_key(url);
    let name = repo_dir_name(&key)?;
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    Some(format!("{name}-{:08x}", hash >> 32))
}

/// Where an import of `url` goes, decided before anything is cloned.
#[derive(Debug, PartialEq, Eq)]
enum CloneTarget {
    /// This URL is already cloned here: add it, clone nothing.
    Existing(PathBuf),
    /// Nothing of this URL is in the cache yet: clone into this folder.
    Clone(PathBuf),
}

/// The clone of `url` under `cache`, or the folder a new one goes into.
///
/// A folder is taken only when its `origin` is this repository (compared by
/// [`clone_url_key`]), whatever its name. The current name
/// ([`clone_dir_name`]) and its overflow slots `<name>-2` … `<name>-16` are
/// looked at first: a hash shared with another URL puts that URL's clone in
/// the first slot, and this one goes into the next free one instead of
/// opening the other repository. Then the old names, the plain
/// [`repo_dir_name`] every clone got before the hash was added (as typed, and
/// as the key spells it): such a folder may just as well hold another
/// repository of the same name, which is the mix-up the hash is there to end.
/// Old clones are found in place rather than moved to the new name, because
/// the workspace already lists them by their path, and a move would leave that
/// entry pointing at nothing. Every slot is looked at, not only those up to
/// the first free one, so a slot emptied by hand does not hide a later clone.
fn clone_target(cache: &Path, url: &str) -> Result<CloneTarget, AppError> {
    let base = clone_dir_name(url).ok_or_else(|| AppError::new("err.url.noName"))?;
    let key = clone_url_key(url);
    let holds_this = |dir: &Path| origin_url(dir).is_some_and(|o| clone_url_key(&o) == key);
    let mut free = None;
    for slot in 1..=CLONE_DIR_SLOTS {
        let dir = match slot {
            1 => cache.join(&base),
            n => cache.join(format!("{base}-{n}")),
        };
        if dir.is_dir() {
            if holds_this(&dir) {
                return Ok(CloneTarget::Existing(dir));
            }
        } else if free.is_none() && !dir.exists() {
            free = Some(dir);
        }
    }
    let mut old_names = vec![repo_dir_name(url), repo_dir_name(&key)];
    old_names.dedup();
    for old in old_names.into_iter().flatten().map(|n| cache.join(n)) {
        if holds_this(&old) {
            return Ok(CloneTarget::Existing(old));
        }
    }
    free.map(CloneTarget::Clone)
        .ok_or_else(|| AppError::new("err.url.slotsTaken").with("base", &base))
}

/// How many folders [`clone_target`] tries for one hash: the name itself and
/// 15 overflow slots, far more than chance or a deliberate collision fills.
const CLONE_DIR_SLOTS: u32 = 16;

/// The sidebar name of a project imported from `url`: the repository name as
/// the URL spells it (`foo`, not the clone folder `foo-c2edc6b0`), or
/// `owner/foo` when a project of that name is already listed, so two
/// repositories of one name can be told apart. `None` leaves the folder name.
fn imported_name(url: &str, projects: &[Project]) -> Option<String> {
    let name = repo_dir_name(url)?;
    if !projects.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
        return Some(name);
    }
    let mut parts = url.trim_end_matches('/').rsplitn(3, ['/', ':']);
    let _last = parts.next();
    let owner = parts
        .next()
        .map(|o| o.rsplit('@').next().unwrap_or(o))
        .filter(|o| !o.is_empty());
    Some(match owner {
        Some(owner) => format!("{owner}/{name}"),
        None => name,
    })
}

/// The `origin` URL in the git config of the clone at `dir`, if it has one.
///
/// `git config --file` reads that one file and nothing else: no repository is
/// opened, so neither its hooks nor a `safe.directory` check play a part. The
/// path goes in [`portable`] form: git for Windows cannot open a `\\?\` path,
/// which is what a canonical path is there.
fn origin_url(dir: &Path) -> Option<String> {
    let config = dir.join(".git").join("config");
    if !config.is_file() {
        return None;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.arg("config")
        .arg("--file")
        .arg(portable(&config))
        .args(["--get", "remote.origin.url"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!url.is_empty()).then_some(url)
}

/// `CON`, `NUL`, `COM1` … : names Windows opens as a device, with or without an
/// extension, in any case. A clone folder of that name is no folder.
fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|p| {
            stem.strip_prefix(p)
                .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']))
        })
}

/// Add the clone of `url` at `dir` as a project named after the repository
/// ([`imported_name`]), not after its clone folder. One lock, so the name is
/// chosen against the list it goes into.
fn add_imported(app: &tauri::AppHandle, dir: &Path, url: &str) -> Result<Vec<Project>, AppError> {
    with_workspace(app, |ws| {
        let name = imported_name(url, &ws.projects);
        add_one_named(ws, &portable(dir), name)?;
        ws.projects.sort_by_key(|p| p.name.to_lowercase());
        Ok(ws.projects.clone())
    })
}

/// Clone a repository into the cache directory and add it as a project.
///
/// `git clone` via a plain subprocess, the same way `generate` and
/// `import_from_nvim_config` shell out to their own subprocess: this
/// program does not speak git itself, and does not touch credentials
/// either — whatever authentication the URL needs (an HTTPS credential
/// helper, an SSH agent) is exactly what a user's own `git clone` in a
/// terminal would use, unchanged. A failure surfaces git's own stderr
/// rather than a guess at what went wrong.
///
/// `--depth 1`: this app only ever reads a project's current working tree
/// (map generation, `add_project`), never its history, and a shallow clone
/// is materially smaller — the "Größe" failure mode this feature was kept
/// separate from folder-adding specifically to isolate.
///
/// Importing a URL that is already cloned is a no-op, same reasoning as
/// `add_project`'s own duplicate check: re-entering a URL that was already
/// imported should not be an error. Which folder is "already cloned" is
/// [`clone_target`]'s decision; another repository of the same name gets a
/// folder of its own.
#[tauri::command]
async fn import_from_url(app: tauri::AppHandle, url: String) -> Result<Vec<Project>, AppError> {
    let url = url.trim().to_owned();
    if let Some(problem) = clone_url_problem(&url) {
        return Err(AppError::new(problem));
    }
    let cache = repos_cache_dir(&app)?;
    let (cache_for_lookup, url_for_lookup) = (cache.clone(), url.clone());
    let target = tauri::async_runtime::spawn_blocking(move || {
        clone_target(&cache_for_lookup, &url_for_lookup)
    })
    .await
    .map_err(AppError::task)??;
    let dest = match &target {
        CloneTarget::Existing(dir) | CloneTarget::Clone(dir) => dir.clone(),
    };
    // The name is plain, so this holds; checked anyway because the next lines
    // add a directory as a project and, on failure, delete one.
    if dest.parent() != Some(cache.as_path()) {
        return Err(AppError::new("err.url.outsideCache"));
    }

    if let CloneTarget::Existing(dir) = target {
        return add_imported(&app, &dir, &url);
    }

    let dest_str = portable(&dest);
    let url_owned = url.clone();
    let dest_for_cleanup = dest.clone();
    let out = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = std::process::Command::new("git");
        // `--` ends the options: nothing after it is read as one, whatever
        // the URL or the destination looks like.
        cmd.args(["clone", "--depth", "1", "--", &url_owned, &dest_str]);
        // No credential prompt, ever: there is no terminal to answer it on,
        // and a prompt nobody can see is a clone that never ends.
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        cmd.env("GIT_ALLOW_PROTOCOL", CLONE_PROTOCOLS);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        proc::run_with_timeout(&mut cmd, CLONE_TIMEOUT).map_err(|e| e.into_app("git"))
    })
    .await
    .map_err(AppError::task)??;

    if !out.status.success() {
        // A partial clone would otherwise permanently occupy `dest`, so
        // every retry after a failure would hit "directory already exists"
        // instead of trying again.
        let _ = fs::remove_dir_all(&dest_for_cleanup);
        return Err(
            AppError::new("err.git.cloneFailed").detail(String::from_utf8_lossy(&out.stderr))
        );
    }

    add_imported(&app, &dest, &url)
}

/// What the view needs to know before it tries to show anything.
#[derive(Debug, Serialize)]
struct MapStatus {
    exists: bool,
    index_path: String,
    /// Present only when the map exists — the counts the sidebar shows so a
    /// project is identifiable without opening it.
    modules: Option<u64>,
    files: Option<u64>,
    namespaces: Option<u64>,
    /// The artifact schema this map was written with.
    ///
    /// Compared against the schema the *engine* reports, this answers a
    /// question nothing could answer before: whether a map predates the
    /// engine installed now. Not academic — a page-side feature fixed in
    /// the engine looks broken until the map is regenerated, because the
    /// page is baked at generation time, and an evening went into exactly
    /// that confusion.
    ///
    /// `None` for a map written before the field existed, which is itself
    /// the answer: older than any engine that reports one.
    schema: Option<u64>,
}

/// Does this project have a generated map, and what is in it?
///
/// Reads `module_map.json` rather than parsing the HTML: the JSON is the
/// artifact with a stated contract (`meta.counts`), the HTML is a rendering
/// of it. Reading the rendering to recover the data it was rendered from is
/// the kind of shortcut that breaks the first time the page changes.
#[tauri::command(async)]
fn map_status(app: tauri::AppHandle, map_dir: String) -> MapStatus {
    let index = format!("{map_dir}/index.html");
    // The map directory and the files in it are repository content: see
    // `map_file`. Looked up by its path because the page asks by directory;
    // every caller passes the directory of a listed project, so one that
    // matches none (a spelling that has since changed) has nothing to check
    // against and reads as no map - never as one to read unchecked.
    let project = read_workspace(&app)
        .ok()
        .and_then(|ws| ws.projects.into_iter().find(|p| p.map_dir == map_dir));
    let (index_file, json_file) = match &project {
        Some(p) => {
            let (root, dir) = (Path::new(&p.root), Path::new(&p.map_dir));
            (
                map_file(root, dir, "index.html"),
                map_file(root, dir, "module_map.json"),
            )
        }
        None => (None, None),
    };
    let exists = index_file.is_some_and(|f| f.is_file());
    let mut modules = None;
    let mut files = None;
    let mut namespaces = None;
    let mut schema = None;

    if let Some(json_file) = json_file.filter(|_| exists) {
        if let Some(body) = safe_read::read_text(&json_file, safe_read::MAP_JSON_MAX, false) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                let counts = &v["meta"]["counts"];
                modules = counts["module"].as_u64();
                files = counts["file"].as_u64();
                namespaces = counts["namespace"].as_u64();
                schema = v["meta"]["schema"].as_u64();
            }
        }
    }

    MapStatus {
        exists,
        index_path: index,
        modules,
        files,
        namespaces,
        schema,
    }
}

/// The signed-in user's GitHub repositories, for the URL tab's picker.
///
/// Delegated to `gh` rather than the API on purpose — see `github.rs` for
/// why this program must not hold a credential. Never called when the dialog
/// opens: listing repositories is a network call against someone's account,
/// and a dialog that makes one just for being looked at is doing something
/// the reader did not ask for.
#[tauri::command]
async fn list_github_repos() -> github::RepoList {
    // On a blocking task for the same reason `generate` is: this shells out
    // and waits on the network, and a window that stops repainting while it
    // does looks broken.
    tauri::async_runtime::spawn_blocking(|| github::list(200))
        .await
        // The runtime's own words about the task, like `gh`'s on any other
        // failure: the catalog's sentence for `failed` says what went wrong.
        .unwrap_or_else(|e| github::RepoList {
            repos: vec![],
            problem: Some(github::ListProblem::Failed),
            message: Some(e.to_string()),
        })
}

/// What languages a directory is written in, counted from file extensions.
///
/// Deliberately answerable without the engine, without a grammar and without
/// a generated map: this is what the sidebar shows for a project that has
/// never been generated, and what the folder picker shows *before* the first
/// generate. See `languages.rs` for why it counts rather than parses, and
/// why it does not decide which of the languages it names can actually be
/// read -- that is the engine's answer, joined in the frontend.
///
/// `map_dir` is optional because the caller does not always have one: a
/// folder being previewed is not a `Project` yet.
#[tauri::command(async)]
fn scan_languages(
    root: String,
    map_dir: Option<String>,
) -> Result<languages::LanguageScan, AppError> {
    languages::scan(Path::new(&root), map_dir.as_deref().map(Path::new))
}

/// Look for the engine on `PATH`, the way a shell would.
///
/// Written out rather than shelling to `where`/`which`: one fewer subprocess,
/// and the answer is a path this program then has to hold anyway.
fn engine_on_path() -> Option<String> {
    let names: &[&str] = if cfg!(windows) {
        &["docmap.exe", "docmap"]
    } else {
        &["docmap"]
    };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(portable(&candidate));
            }
        }
    }
    None
}

/// The engine bundled as a Tauri sidecar (#7, `HANDOVER.md`), if this
/// build actually shipped one for the platform it is running on.
///
/// `externalBin` in `tauri.conf.json` declares the *name* `docmap` is
/// allowed to resolve as a sidecar; it is not a promise the file exists —
/// a dev build, or a target this session's own CI never staged a binary
/// for, both leave `sidecar()` resolving to a path with nothing there. So
/// this is checked the same way `engine_on_path` checks PATH candidates:
/// resolve, then verify, never assume.
///
/// Returns the already-configured `std::process::Command` (via
/// `tauri_plugin_shell`'s own `From<shell::Command> for
/// std::process::Command`) rather than just a path, so every existing
/// caller downstream of `EngineInfo.path` — `generate`, `serve_project`,
/// `server.rs`'s subprocess calls — needs no changes at all: a sidecar
/// resolves to a real, existing, absolute path exactly like a configured
/// or PATH-found one, and `Command::new(&that_path)` behaves identically
/// either way.
///
/// Generic over `R: tauri::Runtime` rather than the concrete app alias,
/// specifically so `cargo test` can exercise it against
/// `tauri::test::mock_app()` — a real `AppHandle`, real path resolution,
/// no window, which is the only way this environment can verify sidecar
/// resolution at all: there is no way to see a native window's sidebar
/// from here.
fn engine_sidecar<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<std::process::Command> {
    let cmd: std::process::Command = app.shell().sidecar("docmap").ok()?.into();
    if Path::new(cmd.get_program()).is_file() {
        Some(cmd)
    } else {
        None
    }
}

/// A grammars directory: configured, or the bundled resource directory if
/// this build shipped one. Mirrors `engine_sidecar`'s own "resolve, then
/// verify" rule — `resource_dir()` resolving successfully says nothing
/// about whether `grammars/` inside it actually has anything in it (a
/// build that bundled the engine without grammars is a real, supported
/// case: parser-less fidelity, exactly like an unconfigured `grammars`
/// today).
fn resolve_grammars<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    configured: Option<String>,
) -> Option<String> {
    if let Some(g) = configured {
        if Path::new(&g).is_dir() {
            return Some(g);
        }
    }
    let dir = app.path().resource_dir().ok()?.join("grammars");
    if dir.is_dir() {
        Some(portable(&dir))
    } else {
        None
    }
}

/// What is actually in the grammars directory, so a missing grammar can be
/// diagnosed rather than only reported.
///
/// The engine already says *which backends* have no grammar; that is a
/// verdict, and the next question it provokes is "so what do I put where".
/// Answering it needs two facts this side owns: the directory this app
/// resolves and passes as `DOCMAP_TS_DIR`, and what that directory holds.
///
/// **File names, not a rule.** This deliberately reports the directory's
/// own contents rather than computing which paths the engine would probe:
/// the resolution order lives in `docmap.nvim`'s
/// `standalone/treesitter.lua` (`$DOCMAP_TS_<LANG>` first, then
/// `$DOCMAP_TS_DIR/<lang>.{so,dll,dylib}`), and a second implementation of
/// it here would be a rule that can disagree with the engine's while
/// looking authoritative.
#[derive(Debug, Serialize)]
struct GrammarDir {
    /// The directory the engine is given, or `None` when neither a setting
    /// nor a bundled directory resolved.
    dir: Option<String>,
    /// True when it came from Settings rather than from the bundle -- the
    /// difference between "fix your setting" and "this build ships none".
    from_setting: bool,
    /// False when `dir` is set but is not there any more. A configured path
    /// that has been deleted is a different problem from an empty one, and
    /// the fix is different too.
    exists: bool,
    /// Base names, sorted, capped. Enough to see the pattern and to spot a
    /// typo; not a file browser.
    files: Vec<String>,
    /// How many were left out by the cap, so a long list says it is long
    /// rather than quietly looking short.
    more: usize,
}

/// Names listed before [`GrammarDir::more`] takes over.
const GRAMMAR_FILES_SHOWN: usize = 12;

#[tauri::command]
fn grammar_dir(app: tauri::AppHandle) -> Result<GrammarDir, AppError> {
    let ws = read_workspace(&app)?;
    let from_setting = ws
        .grammars
        .as_deref()
        .map(|g| Path::new(g).is_dir())
        .unwrap_or(false);
    let dir = resolve_grammars(&app, ws.grammars);

    let Some(d) = dir else {
        return Ok(GrammarDir {
            dir: None,
            from_setting: false,
            exists: false,
            files: Vec::new(),
            more: 0,
        });
    };

    let path = Path::new(&d);
    if !path.is_dir() {
        return Ok(GrammarDir {
            dir: Some(d),
            from_setting,
            exists: false,
            files: Vec::new(),
            more: 0,
        });
    }

    let mut names: Vec<String> = match std::fs::read_dir(path) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        // Unreadable is not empty, and saying "holds nothing" about a
        // directory that refused to be read would send somebody looking for
        // a missing file that is sitting right there.
        Err(e) => return Err(AppError::new("err.path.read").with("path", &d).detail(e)),
    };
    names.sort();

    let more = names.len().saturating_sub(GRAMMAR_FILES_SHOWN);
    names.truncate(GRAMMAR_FILES_SHOWN);

    Ok(GrammarDir {
        dir: Some(d),
        from_setting,
        exists: true,
        files: names,
        more,
    })
}

#[derive(Debug, Serialize)]
struct EngineInfo {
    /// The configured path, one found on PATH, the bundled sidecar's
    /// resolved path, or none.
    path: Option<String>,
    /// True when it came from PATH rather than from a setting — worth showing,
    /// because it explains why it might disappear on another machine.
    from_path: bool,
    /// True when `path` is the bundled sidecar rather than something found
    /// on this machine — the case that needs no setup at all, and the
    /// sidebar should say so rather than looking identical to a lucky PATH
    /// find that could vanish on the next machine.
    bundled: bool,
    grammars: Option<String>,
}

#[tauri::command]
fn engine_info(app: tauri::AppHandle) -> Result<EngineInfo, AppError> {
    let ws = read_workspace(&app)?;
    let grammars = resolve_grammars(&app, ws.grammars);

    if let Some(p) = ws.engine.clone() {
        if Path::new(&p).is_file() {
            return Ok(EngineInfo {
                path: Some(p),
                from_path: false,
                bundled: false,
                grammars,
            });
        }
        // A configured path that no longer exists is worse than none: it
        // would fail at generation time with a confusing OS error. Fall
        // through to detection and let the caller see `from_path`.
    }

    // **The bundled engine beats a stray PATH find, and that order changed on
    // 2026-08-20 after it bit.** PATH used to win, on the reasoning that
    // somebody who put an engine there meant it. Measured on the author's own
    // machine right after shipping v0.2.0: the app used
    // the engine on PATH from two days earlier — four languages, older
    // schema — while its own installed sidecar next to the exe read
    // twenty-three. Nothing said so; the project-settings dialog simply
    // offered four checkboxes and was *correct* to, because it asks the
    // engine.
    //
    // The sidecar is the version this build was tested against and installed
    // beside itself. A binary on PATH is somebody's leftover as often as it
    // is their intention, and this program cannot tell which — nor which is
    // newer, since "newer file" and "newer engine" are not the same claim.
    //
    // **A configured path still beats both**, and that is the whole escape
    // hatch: someone who genuinely wants their own build points at it in
    // Settings, which is an act of intent rather than a coincidence of
    // environment.
    if let Some(cmd) = engine_sidecar(&app) {
        let path = portable(Path::new(cmd.get_program()));
        return Ok(EngineInfo {
            path: Some(path),
            from_path: false,
            bundled: true,
            grammars,
        });
    }

    if let Some(p) = engine_on_path() {
        return Ok(EngineInfo {
            path: Some(p),
            from_path: true,
            bundled: false,
            grammars,
        });
    }

    Ok(EngineInfo {
        path: None,
        from_path: true,
        bundled: false,
        grammars,
    })
}

/// One language backend the engine reports.
///
/// `grammar_loaded` is deliberately three-valued and stays that way across
/// the IPC boundary: `Some(true)` full fidelity, `Some(false)` a backend
/// that wants a grammar and could not load one, `None` a backend needing no
/// parser at all. Collapsing the last two would report a healthy backend as
/// broken -- see `lang_registry.report()` on the engine side, which is where
/// this distinction is defined.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineLanguage {
    name: String,
    grammar: Option<String>,
    grammar_loaded: Option<bool>,
    /// Whether this backend produces call edges at all.
    ///
    /// `None` from an engine that predates the field, and that is a third
    /// answer rather than a defaulted `false`: "this build has no call
    /// extraction for this language" and "this build cannot say" lead to
    /// different sentences, and only the first is worth putting in front of
    /// a reader looking at an empty panel.
    #[serde(default)]
    calls: Option<bool>,
}

/// What the engine says it can read.
///
/// `languages: None` is not an error and not an empty list -- it means this
/// engine predates the field. The frontend has to say "unknown", because
/// "no languages" and "an engine that cannot be asked" lead to opposite
/// advice: the first is broken, the second works fine and just cannot
/// explain itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineLanguages {
    languages: Option<Vec<EngineLanguage>>,
    /// The artifact schema this engine writes. `None` from a build that
    /// predates the field, which is a different fact from any particular
    /// number and is why it is not defaulted to one.
    schema: Option<u64>,
    /// Which build it is: commit, commit date, and whether the tree it was
    /// built from was clean. `None` for an engine run from a source
    /// checkout, which stamps nothing rather than claiming a provenance.
    build: Option<EngineBuild>,
    /// The `/api/*` routes the engine answers. `None` from a build that
    /// predates the field -- not an empty list, which would claim an engine
    /// that answers nothing.
    routes: Option<Vec<String>>,
    /// The contract this build of the app was checked against
    /// (`engine.lock`). Sent with every answer, including "cannot be asked",
    /// so the window compares the two in one place (`src/lib/contract.js`).
    contract: Option<contract::Contract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineBuild {
    commit: Option<String>,
    committed_at: Option<String>,
    /// `None` when git could not answer at all — which is not the same as
    /// a clean tree, and the difference is what makes the commit
    /// trustworthy.
    dirty: Option<bool>,
}

/// Ask the engine which language backends it has, and whether each found its
/// grammar.
///
/// A separate command rather than a field on `engine_info` on purpose:
/// `engine_info` is called on every render and touches no subprocess, and
/// putting a process spawn behind it would put one in a hot path for an
/// answer that changes only when the engine or the grammars directory
/// changes.
///
/// `DOCMAP_TS_DIR` is passed exactly as `generate` passes it. Without it the
/// probe would answer for an environment the real run never happens in, and
/// report every grammar missing on a machine where generation works --
/// which is worse than not asking, because it would be confidently wrong.
///
/// `--capabilities` is safe against every engine version for the reason
/// `server.rs`'s `engine_supports_api` documents at length: it sits in the
/// root-argument position, so an older binary rejects it with exit 2 before
/// doing any work, rather than treating it as a path and generating a map.
#[tauri::command]
fn engine_languages(app: tauri::AppHandle) -> Result<EngineLanguages, AppError> {
    let info = engine_info(app)?;
    let engine = info.path.ok_or_else(|| AppError::new("err.engine.none"))?;

    let mut cmd = std::process::Command::new(&engine);
    cmd.arg("--capabilities");
    if let Some(g) = info.grammars {
        cmd.env("DOCMAP_TS_DIR", g);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let out =
        proc::run_with_timeout(&mut cmd, ENGINE_PROBE_TIMEOUT).map_err(|e| e.into_app(&engine))?;
    if !out.status.success() {
        // An older engine exits non-zero here. Not an error to show: it is
        // the "cannot be asked" case, which the frontend renders as unknown.
        return Ok(EngineLanguages {
            languages: None,
            schema: None,
            build: None,
            routes: None,
            contract: contract::pinned(),
        });
    }

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| {
        AppError::new("err.engine.notJson")
            .with("program", &engine)
            .detail(e)
    })?;

    // Absent field and unparseable field are both "cannot be asked". A
    // partially-shaped answer is not worth guessing at -- the engine that
    // emits this field emits it whole.
    let languages = value
        .get("languages")
        .and_then(|l| serde_json::from_value::<Vec<EngineLanguage>>(l.clone()).ok());

    Ok(EngineLanguages {
        languages,
        schema: value.get("schema").and_then(|v| v.as_u64()),
        build: value
            .get("build")
            .and_then(|b| serde_json::from_value::<EngineBuild>(b.clone()).ok()),
        routes: value
            .get("routes")
            .and_then(|r| serde_json::from_value::<Vec<String>>(r.clone()).ok()),
        contract: contract::pinned(),
    })
}

#[tauri::command]
fn set_engine(app: tauri::AppHandle, path: Option<String>) -> Result<EngineInfo, AppError> {
    if let Some(ref p) = path {
        if !Path::new(p).is_file() {
            return Err(AppError::new("err.path.notFile").with("path", p));
        }
    }
    with_workspace(&app, |ws| {
        ws.engine = path;
        Ok(())
    })?;
    engine_info(app)
}

#[tauri::command]
fn set_grammars(app: tauri::AppHandle, path: Option<String>) -> Result<EngineInfo, AppError> {
    if let Some(ref p) = path {
        if !Path::new(p).is_dir() {
            return Err(AppError::not_dir(p));
        }
    }
    with_workspace(&app, |ws| {
        ws.grammars = path;
        Ok(())
    })?;
    engine_info(app)
}

/// Look for `nvim` on `PATH`, the same way `engine_on_path` looks for the
/// docmap engine.
fn nvim_on_path() -> Option<String> {
    let names: &[&str] = if cfg!(windows) {
        &["nvim.exe", "nvim"]
    } else {
        &["nvim"]
    };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(portable(&candidate));
            }
        }
    }
    None
}

/// The one place Neovim itself would look by default on this OS — not a
/// search, just the conventional location, so a first run usually needs no
/// manual configuration at all. Only returned when it actually exists;
/// a configured path always wins over this guess.
fn default_nvim_config_dir() -> Option<String> {
    let guess = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| Path::new(&d).join("nvim"))
    } else {
        std::env::var_os("HOME").map(|d| Path::new(&d).join(".config/nvim"))
    }?;
    if guess.is_dir() {
        Some(portable(&guess))
    } else {
        None
    }
}

#[derive(Debug, Serialize)]
struct NvimInfo {
    /// The configured `nvim` binary, or one found on PATH, or none.
    path: Option<String>,
    from_path: bool,
    /// The configured Neovim config directory, or the platform default if
    /// that default actually exists on disk, or none.
    config_dir: Option<String>,
    config_dir_from_default: bool,
}

#[tauri::command]
fn nvim_info(app: tauri::AppHandle) -> Result<NvimInfo, AppError> {
    let ws = read_workspace(&app)?;

    let (path, from_path) = match ws.nvim_path.clone() {
        Some(p) if Path::new(&p).is_file() => (Some(p), false),
        // A configured path that no longer exists falls back to detection,
        // same reasoning as `engine_info`.
        _ => (nvim_on_path(), true),
    };

    let (config_dir, config_dir_from_default) = match ws.nvim_config_dir.clone() {
        Some(p) if Path::new(&p).is_dir() => (Some(p), false),
        _ => (default_nvim_config_dir(), true),
    };

    Ok(NvimInfo {
        path,
        from_path,
        config_dir,
        config_dir_from_default,
    })
}

#[tauri::command]
fn set_nvim_path(app: tauri::AppHandle, path: Option<String>) -> Result<NvimInfo, AppError> {
    if let Some(ref p) = path {
        if !Path::new(p).is_file() {
            return Err(AppError::new("err.path.notFile").with("path", p));
        }
    }
    with_workspace(&app, |ws| {
        ws.nvim_path = path;
        Ok(())
    })?;
    nvim_info(app)
}

#[tauri::command]
fn set_nvim_config_dir(app: tauri::AppHandle, path: Option<String>) -> Result<NvimInfo, AppError> {
    if let Some(ref p) = path {
        if !Path::new(p).is_dir() {
            return Err(AppError::not_dir(p));
        }
    }
    with_workspace(&app, |ws| {
        ws.nvim_config_dir = path;
        Ok(())
    })?;
    nvim_info(app)
}

/// One entry in `scripts/docmap_projects.lua`'s own JSON contract (nvim
/// config repo) — see that script's header for the full shape guarantee.
#[derive(Debug, Deserialize)]
struct NvimProjectEntry {
    name: String,
    #[allow(dead_code)]
    repo: String,
    dir: String,
}

#[derive(Debug, Serialize)]
struct ImportResult {
    /// How many entries the Neovim export returned, before dedup.
    found: usize,
    /// Newly added projects only — not ones that were already present.
    added: Vec<Project>,
    already_present: usize,
    /// Every entry that failed to add: its name and why.
    errors: Vec<ImportFailure>,
}

/// One entry of an import that could not be added. The error is an
/// [`AppError`] like any command's, so a window that lists these can say why
/// in its own language.
#[derive(Debug, Serialize)]
struct ImportFailure {
    name: String,
    error: AppError,
}

/// Import every enabled, locally-checked-out personal plugin from the
/// user's Neovim configuration.
///
/// Delegates entirely to `plugins.personal.export.projects()` (nvim config,
/// `lua/plugins/personal/export.lua`) via the headless entry point
/// `scripts/docmap_projects.lua` — this program never parses
/// `source.lua`'s own policy table itself, which would be brittle in a way
/// neither side should have to maintain. Must invoke with `-c "luafile
/// ..." -c "qa"`, not `-l`: measured, not assumed — `-l` is a bare
/// Lua-script runner that skips `init.lua` and lazy.nvim's bootstrap
/// entirely, so it cannot see this config's real, fully-resolved plugin
/// policy. See that script's own header for the full story.
///
/// `async` + `spawn_blocking`, same reasoning as `generate`: a real Neovim
/// startup takes real time, and a command that blocks freezes the window
/// it was invoked from.
#[tauri::command]
async fn import_from_nvim_config(app: tauri::AppHandle) -> Result<ImportResult, AppError> {
    let info = nvim_info(app.clone())?;
    let nvim_path = info.path.ok_or_else(|| AppError::new("err.nvim.none"))?;
    let config_dir = info
        .config_dir
        .ok_or_else(|| AppError::new("err.nvim.noConfig"))?;

    // Absolute path to the script rather than a relative one plus a
    // matching cwd: one fewer thing that can silently point at the wrong
    // directory.
    let script = format!("{config_dir}/scripts/docmap_projects.lua");
    if !Path::new(&script).is_file() {
        return Err(AppError::new("err.nvim.noScript").with("script", &script));
    }

    let out = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&nvim_path);
        cmd.args(["--headless", "-c", &format!("luafile {script}"), "-c", "qa"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        proc::run_with_timeout(&mut cmd, NVIM_TIMEOUT).map_err(|e| e.into_nvim(&nvim_path))
    })
    .await
    .map_err(AppError::task)??;

    if !out.status.success() {
        return Err(AppError::new("err.nvim.exit")
            .with("code", out.status.code().unwrap_or(-1))
            .detail(String::from_utf8_lossy(&out.stderr)));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let found: Vec<NvimProjectEntry> = serde_json::from_str(stdout.trim())
        .map_err(|e| AppError::new("err.nvim.notJson").detail(format!("{e}\n{stdout}")))?;

    let mut added = Vec::new();
    let mut already_present = 0usize;
    let mut errors = Vec::new();

    with_workspace(&app, |ws| {
        for entry in &found {
            match add_one(ws, &entry.dir) {
                Ok(Some(project)) => added.push(project),
                Ok(None) => already_present += 1,
                Err(error) => errors.push(ImportFailure {
                    name: entry.name.clone(),
                    error,
                }),
            }
        }
        ws.projects.sort_by_key(|p| p.name.to_lowercase());
        Ok(())
    })?;

    Ok(ImportResult {
        found: found.len(),
        added,
        already_present,
        errors,
    })
}

/// The engine command line of one **Generate**.
///
/// Its own function so a test can read the arguments, and for one reason in
/// particular: **`--lenient` is what makes the exit code mean "the map was
/// written".** Without it the engine exits 1 whenever the *tree* carries
/// error-severity drift findings (a Lua file without `---@module`, say) -
/// after it has written the map, because the findings are a verdict on the
/// source, not a failure of the run. A repository like that (a large vault
/// with a few dozen such findings) was reported as "Failed" with a map sitting
/// on disk. A real
/// failure (no such directory, an unsafe `out_dir`, a crashed backend) still
/// exits 1 with `--lenient`, so the flag removes exactly the false alarm.
/// `check_map` passes it for the same reason; the findings themselves still
/// arrive on stderr, where this program shows them (grouped and capped).
fn generate_command(
    engine: &str,
    root: &str,
    full: bool,
    flags: &ProjectFlags,
) -> std::process::Command {
    let mut cmd = std::process::Command::new(engine);
    // Just the root: the engine detects `source` itself, verified against
    // three unrelated repositories. Passing a guess would be worse than
    // letting it look.
    cmd.arg(root);
    // `--full` adds `lua-language-server --doc` enrichment: the
    // `@class`/`@alias` detail behind the Types panel. It needs the
    // tool on PATH and says `lua-language-server not found on PATH`
    // when it is missing, rather than quietly producing a thinner map.
    // The caller has to put that sentence somewhere the reader is
    // looking - see `WORKPLAN.md` §2 on why that obligation is
    // this side's rather than the menu's.
    if full {
        cmd.arg("--full");
    }
    cmd.arg("--lenient");
    // This project's own settings, not the machine's: see
    // `Project::exclude` for why they live where they do.
    apply_flags(&mut cmd, flags);
    cmd
}

#[derive(Debug, Serialize)]
struct GenerateResult {
    ok: bool,
    code: i32,
    stdout: String,
    stderr: String,
}

/// Run the engine over one project.
///
/// `async` plus `spawn_blocking`, not a plain synchronous command: this takes
/// seconds on a large tree, and a Tauri command that blocks freezes the
/// window it was invoked from. A viewer that stops repainting while it works
/// looks broken in exactly the way the work is meant to prevent.
///
/// Output is returned whole rather than streamed. The engine's own report is
/// a dozen lines at the end, not a running log, so streaming would add a
/// channel and a subscription for something that arrives at once anyway.
#[tauri::command]
async fn generate(
    app: tauri::AppHandle,
    root: String,
    full: bool,
) -> Result<GenerateResult, AppError> {
    // Read before `engine_info` consumes the handle, and before the move
    // into the blocking task: both need `app`, and the flags are a workspace
    // read rather than a process launch, so it costs nothing to do first.
    let flags = project_flags(&app, &root);
    refuse_linked_output(&root, &flags)?;
    // The project's own `full` is a *default*, not a ceiling: **Generate
    // full** passes `true` and must stay able to, while plain **Generate**
    // on a project that asked for enrichment gets it without being asked
    // again. `||` rather than a branch, because there is no third answer.
    let full = full || flags.full;
    let info = engine_info(app)?;
    let engine = info.path.ok_or_else(|| AppError::new("err.engine.none"))?;

    tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = generate_command(&engine, &root, full, &flags);
        if let Some(g) = info.grammars {
            cmd.env("DOCMAP_TS_DIR", g);
        }
        #[cfg(windows)]
        {
            // Without this a console window flashes up on every generation.
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let out = cmd.output().map_err(|e| {
            AppError::new("err.proc.run")
                .with("program", &engine)
                .detail(e)
        })?;
        Ok(GenerateResult {
            ok: out.status.success(),
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        })
    })
    .await
    .map_err(AppError::task)?
}

#[derive(Debug, Serialize)]
struct CheckResult {
    /// Whether regenerating would produce byte-identical files.
    current: bool,
    code: i32,
    stdout: String,
    stderr: String,
}

/// Ask the engine whether the map would come out different.
///
/// **The difference from the staleness mark, which is the whole reason this
/// exists.** `freshness.rs` compares modification times: it answers "something
/// was touched since this was written", and a file saved without an edit in it
/// counts. This runs the analysis and compares the *output* byte for byte, so
/// it answers the question the mark only approximates — and it is the one
/// answer that settles it.
///
/// **`--lenient` is not a softening, it is what makes the exit code readable.**
/// Without it the engine exits 1 for two unrelated reasons — the map is stale,
/// *or* the map is current but carries error-severity drift findings — and a
/// caller with one bit cannot tell those apart. Sniffing the prose on stderr
/// for "Module map is stale" would work today and break the first time that
/// sentence is reworded. With `--lenient` the exit code means staleness and
/// nothing else, except `2`: the engine refused or could not run and so has no
/// verdict (the frontend reads it as "not checked", not as "stale"). The findings
/// still arrive in the output, which is where this app shows the engine's own
/// report: its lines as they are, the findings grouped per check and capped.
///
/// Writes nothing. That is why it is its own command rather than a flag on
/// `generate`: the two have opposite guarantees, and a boolean that decided
/// whether a function writes to somebody's repository is the kind of argument
/// that eventually gets passed the wrong way round.
#[tauri::command]
async fn check_map(app: tauri::AppHandle, root: String) -> Result<CheckResult, AppError> {
    // **The same settings as `generate`, and it has to be.** This command
    // answers "would regenerating change anything", and regenerating means
    // regenerating *with this project's settings*. Asking without them would
    // compare the committed map against a map nobody would ever write, and
    // report a stale project every time somebody excluded a directory.
    //
    // `--full` is the one it deliberately does not mirror: enrichment adds
    // detail to the map, so a `--check` run with it would compare against
    // something the committed artifact may legitimately not contain. That
    // asymmetry predates these flags and is unchanged by them.
    let flags = project_flags(&app, &root);
    refuse_linked_output(&root, &flags)?;
    let info = engine_info(app)?;
    let engine = info.path.ok_or_else(|| AppError::new("err.engine.none"))?;

    tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&engine);
        cmd.arg(&root);
        cmd.arg("--check");
        cmd.arg("--lenient");
        apply_flags(&mut cmd, &flags);
        if let Some(g) = info.grammars {
            cmd.env("DOCMAP_TS_DIR", g);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let out = cmd.output().map_err(|e| {
            AppError::new("err.proc.run")
                .with("program", &engine)
                .detail(e)
        })?;
        Ok(CheckResult {
            current: out.status.success(),
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        })
    })
    .await
    .map_err(AppError::task)?
}

/// Serve one project's map over HTTP and return the URL to point the iframe
/// at.
///
/// Replaces `convertFileSrc` for the view. The page is identical either way;
/// what changes is that `/api/*` now has something on the other end, so the
/// Telemetry and Loaded panels can show real data instead of advising a
/// `:DocMap serve` that would not have helped. See `server.rs`'s own header
/// for why the asset protocol made that failure look like a working server.
#[tauri::command]
fn serve_project(app: tauri::AppHandle, id: String) -> Result<String, AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;

    // Resolved per call rather than captured once at startup: the engine can
    // be located, or grammars pointed at, after the window is already open,
    // and a server holding the values from before would answer with a
    // fidelity the sidebar says has since changed.
    let info = engine_info(app.clone())?;

    let port = server::start(server::ServeConfig {
        root: project.root.clone(),
        map_dir: project.map_dir.clone(),
        engine: info.path,
        grammars: info.grammars,
    })?;
    Ok(server::url(port))
}

// =====================================================================
// GitHub traffic (`github_stats.nvim`'s digest)
//
// Read-only, and nothing here talks to GitHub or handles a token. The logic
// lives in `traffic.rs` and is testable without an app; what is here is the
// part that needs one: the workspace lookup, the settings, and the one
// subprocess ("Ask Neovim"). See `docs/FEATURES/TRAFFIC.md`.
// =====================================================================

/// The discovery chain's starting points, from the saved settings.
fn traffic_sources(ws: &Workspace) -> traffic::Sources {
    traffic::Sources {
        explicit: ws.traffic_dir.clone(),
        default_dir: traffic::default_data_dir(),
        asked: ws.traffic_asked_dir.clone(),
    }
}

/// What a project needs to resolve its repository: root, an already-known
/// `repo_url`, and whether it was opted out.
type TrafficProject = (String, Option<String>, bool);

/// [`TrafficProject`] of a project, or `None` for an id nobody added.
fn traffic_project(ws: &Workspace, id: &str) -> Option<TrafficProject> {
    ws.projects
        .iter()
        .find(|p| p.id == id)
        .map(|p| (p.root.clone(), p.repo_url.clone(), p.traffic_hidden))
}

/// The repository of a project, unless it was opted out — an opted-out project
/// resolves nothing, not even its remote, so no process is spawned for it.
fn traffic_repo(root: &str, repo_url: Option<&str>, hidden: bool) -> Option<String> {
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
async fn traffic_info(app: tauri::AppHandle, id: String) -> Result<traffic::Info, AppError> {
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
async fn traffic_detail(
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
async fn traffic_list(
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
fn traffic_settings(app: tauri::AppHandle) -> Result<traffic::Survey, AppError> {
    let ws = read_workspace(&app)?;
    Ok(traffic::survey(&traffic_sources(&ws)))
}

/// Choose (or clear) the folder the digest is read from.
///
/// The folder is read by this process only. It is **not** added to a Tauri fs
/// scope (`capabilities/default.json`): the webview asks for numbers through
/// the commands above and never gets a path it could read on its own.
#[tauri::command]
fn traffic_set_dir(
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
async fn traffic_ask_neovim(app: tauri::AppHandle) -> Result<traffic::Survey, AppError> {
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
fn traffic_set_hidden(app: tauri::AppHandle, id: String, hidden: bool) -> Result<(), AppError> {
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
fn traffic_refresh() {
    traffic::forget_all();
}

/// Write text the page handed over to a path the reader chose.
///
/// The bytes come from the map page through its inbound channel, and the
/// path from a save dialog — so nothing is written anywhere nobody named.
/// The page's own `<a download>` still exists for a browser; inside this
/// window it would land wherever the webview decided, which is the reason
/// this route exists at all.
#[tauri::command]
fn save_text(path: String, contents: String) -> Result<(), AppError> {
    fs::write(&path, contents).map_err(|e| {
        AppError::new("err.path.write")
            .with("path", &path)
            .detail(e)
    })
}

/// Neovim's own cache root, asked of Neovim.
///
/// Not guessed from the platform: `stdpath("cache")` answers differently
/// under `XDG_CACHE_HOME`, and a hardcoded guess would read an empty
/// directory and report "no telemetry" for a machine full of it. One
/// subprocess, cached for the window's lifetime — the answer cannot change
/// while Neovim's environment does not.
static CACHE_ROOT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

fn nvim_cache_root(app: &tauri::AppHandle) -> Option<String> {
    CACHE_ROOT
        .get_or_init(|| {
            let info = nvim_info(app.clone()).ok()?;
            let nvim = info.path?;
            let mut cmd = std::process::Command::new(&nvim);
            cmd.args([
                "--headless",
                "-c",
                "lua io.write(vim.fn.stdpath('cache'))",
                "-c",
                "qa",
            ]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let out = cmd.output().ok()?;
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if path.is_empty() {
                None
            } else {
                Some(path.replace('\\', "/"))
            }
        })
        .clone()
}

/// What is known about a project's telemetry.
///
/// The namespace is the project's name, because a telemetry namespace is a
/// plugin name — see `telemetry.rs` on why that is reported rather than
/// assumed to line up.
#[tauri::command]
fn telemetry_info(app: tauri::AppHandle, namespace: String) -> Result<telemetry::Info, AppError> {
    let root = nvim_cache_root(&app).ok_or_else(|| AppError::new("err.nvim.noCache"))?;
    Ok(telemetry::info(&root, &namespace))
}

/// Switch collection on or off for a namespace, persistently.
///
/// Through `:RATelemetry enable|disable` rather than by writing the control
/// file: the plugin owns that file's format, and a second writer of it is a
/// second thing to keep in step with a format that is not ours.
///
/// Takes effect from the **next** Neovim session — the flag is what
/// `inst.start()` consults, and nothing in this process is running the
/// plugin. The caller has to say so.
#[tauri::command]
async fn set_telemetry(
    app: tauri::AppHandle,
    namespace: String,
    enabled: bool,
) -> Result<(), AppError> {
    let info = nvim_info(app)?;
    let nvim = info.path.ok_or_else(|| AppError::new("err.nvim.none"))?;
    let verb = if enabled { "enable" } else { "disable" };
    let arg = format!("RATelemetry {verb} {namespace}");
    let failed = AppError::new("err.telemetry.failed").with("namespace", &namespace);

    let out = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&nvim);
        cmd.args(["--headless", "-c", &arg, "-c", "qa"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        proc::run_with_timeout(&mut cmd, NVIM_TIMEOUT).map_err(|e| e.into_nvim(&nvim))
    })
    .await
    .map_err(AppError::task)??;

    if out.status.success() {
        Ok(())
    } else {
        // The plugin's own message, verbatim: it knows why better than a
        // sentence written here could guess.
        Err(failed.detail(String::from_utf8_lossy(&out.stderr)))
    }
}

/// `rel` resolved inside `root`, or an error — checked **before** the
/// filesystem is touched.
///
/// `root` must already be canonical. `Path::join` replaces the base when the
/// right-hand side is absolute, so a `rel` such as `\\host\share\x` becomes
/// that UNC path; `fs::canonicalize` then opens it, which on Windows makes the
/// machine connect to that host with the user's credentials (an NTLM hash
/// leak) and blocks for as long as the connection takes — and only after that
/// did the old `starts_with(root)` check say no. A path that arrives from a
/// map page, a search hit or a text box is therefore refused on its *shape*
/// first: no drive or UNC prefix, no leading separator. `..` is allowed to
/// pass here because the containment check below catches anything that really
/// leaves the root, and `a/../b` inside it is a legitimate path.
///
/// An empty `rel` is the root itself, which is what a folder search over the
/// whole project asks for.
pub(crate) fn resolve_inside(root: &Path, rel: &str) -> Result<PathBuf, AppError> {
    let mut steps = WALK_STEPS;
    resolve_inside_budgeted(root, rel, &mut steps)
}

/// [`resolve_inside`] with the work of the link walk (one `lstat` per component
/// looked at, the re-walk after every link hop included) drawn from `steps`.
///
/// The caps on `rel` do not bound what a *link* costs: its target is walked
/// again from the link's directory, and a target can name thousands of real
/// levels. A caller that resolves many paths on behalf of one request (the icon
/// lookup tries some seventy fixed names and up to 256 manifest entries) shares
/// one budget, so the request is bounded rather than each path. Spent, the walk
/// answers "too many links".
pub(crate) fn resolve_inside_budgeted(
    root: &Path,
    rel: &str,
    steps: &mut u32,
) -> Result<PathBuf, AppError> {
    #[cfg(not(windows))]
    let _ = &steps;
    if rel.contains('\0') {
        return Err(AppError::new("err.path.nul"));
    }
    // No path the OS accepts is longer, and `rel` can come out of a repository's
    // manifest at up to a megabyte.
    if rel.len() > MAX_REL_LEN {
        return Err(AppError::new("err.path.tooLong"));
    }
    let relative = Path::new(rel);
    if relative
        .components()
        .any(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
    {
        return Err(path_key("err.path.notInside", rel));
    }
    // Collapsed lexically, once, and *this* is the path that is examined and
    // then opened. `Path::join` on a verbatim (`\\?\`) root drops `.` and `..`
    // before the OS sees the path, and Win32 does the same to any other: an
    // examination that walked the components as written (`missing/../l`)
    // stopped at the first one that does not exist and never looked at `l`,
    // which is what `canonicalize` then opened.
    //
    // Built relative to nothing and joined to the root once: `push` onto a
    // verbatim (`\\?\`) path rebuilds the whole buffer, so pushing each name
    // onto the root made the loop quadratic - a manifest `src` of half a
    // million `a/` kept a thread busy for over an hour.
    let mut collapsed = PathBuf::new();
    let mut depth = 0usize;
    for part in relative.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if depth == 0 {
                    return Err(path_key("err.path.outside", rel));
                }
                collapsed.pop();
                depth -= 1;
            }
            Component::Normal(name) => {
                // `push` re-parses its argument: `C:l.png` has a drive prefix,
                // so it *replaces* everything pushed so far, and what is then
                // examined is a drive-relative path against the process's
                // current directory, not a path under the root.
                if reparsed_by_push(name) {
                    return Err(path_key("err.path.notInside", rel));
                }
                // The root is a canonical, verbatim path, so `evil.` is looked
                // at as itself here; handed on without the prefix (as the
                // icon path is, to the asset protocol, and the editor's) Win32
                // reads it as `evil` - through a link, if there is one beside it.
                if crate::languages::win32_rewrites_name(name) {
                    return Err(path_key("err.path.notInside", rel));
                }
                collapsed.push(name);
                depth += 1;
                // Looking at each prefix of an existing path costs a walk of
                // the whole prefix, so a path of thousands of real levels is
                // quadratic work in the kernel. No project has one.
                if depth > MAX_REL_COMPONENTS {
                    return Err(AppError::new("err.path.tooLong"));
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(path_key("err.path.notInside", rel));
            }
        }
    }
    let clean = if depth == 0 {
        root.to_path_buf()
    } else {
        root.join(&collapsed)
    };
    // Whatever else went wrong above, nothing past this point looks at a path
    // that is not under the root.
    if !clean.starts_with(root) {
        return Err(path_key("err.path.outside", rel));
    }
    #[cfg(windows)]
    match links_to_network(&clean, root.components().count(), &mut 16, steps) {
        LinkWalk::Local => {}
        LinkWalk::Network => return Err(path_key("err.path.network", rel)),
        LinkWalk::TooDeep => return Err(path_key("err.path.tooManyLinks", rel)),
        LinkWalk::Unusable => return Err(path_key("err.path.unsafeLink", rel)),
    }
    let target = fs::canonicalize(&clean).map_err(|_| path_key("err.path.missing", rel))?;
    if !target.starts_with(root) {
        return Err(path_key("err.path.outside", rel));
    }
    // What is handed on is the result, and a link can have led it through a
    // name Win32 reads as another (`a/l -> evil./f`): the check on `rel` does not
    // see that one. The root is the user's own and is left out of it.
    if target.strip_prefix(root).is_ok_and(|inside| {
        inside
            .components()
            .any(|c| matches!(c, Component::Normal(n) if crate::languages::win32_rewrites_name(n)))
    }) {
        return Err(path_key("err.path.notInside", rel));
    }
    Ok(target)
}

/// Would `PathBuf::push(name)` treat this one component as more than a name?
///
/// On Windows `C:x` is a drive-relative path and replaces the buffer; `a:b` is
/// also an NTFS stream spelling, and no file name contains a colon there. On
/// other systems a component is always just a name.
fn reparsed_by_push(name: &std::ffi::OsStr) -> bool {
    (cfg!(windows) && name.to_string_lossy().contains(':'))
        || Path::new(name).components().next() != Some(Component::Normal(name))
}

/// Is the map directory a plain directory on the way from the project root?
///
/// `docs/map` lies inside the project, so it is repository content: checked
/// out as a link it makes every reader of "the map" read somewhere else — a
/// share (`map_freshness` stats it for every project at start-up), the
/// repository root (`.env` next to a served `index.html`), a home directory.
/// No component between the root and the map directory may be a link, and none
/// is followed to find out (`symlink_metadata` does not, and reports a Windows
/// junction as a link too).
///
/// A map directory that is not under the root (`../maps`, saved in Project
/// settings before they refused it) is not repository content and is left
/// alone here; `..` in it is walked as the OS does, and only the part inside
/// the root is inspected. The engine itself refuses such a value on write and
/// on `--check`, and so does [`refuse_linked_output`] before Generate and
/// Check, so this only governs what the app reads.
///
/// A component that is not there yet is fine - there is nothing to read - but
/// the walk goes on past it, because a later `..` can lead back to one that is
/// (`dist/../docs/map`). Any other failure to look is a no.
///
/// This vets the *directory*. A file inside it can be a link as well: read it
/// through [`map_file`].
pub(crate) fn map_dir_is_plain(root: &Path, map_dir: &Path) -> bool {
    let Ok(rel) = map_dir.strip_prefix(root) else {
        return true;
    };
    let Ok(canon_root) = fs::canonicalize(root) else {
        return false;
    };
    let mut walked = canon_root.clone();
    for part in rel.components() {
        match part {
            Component::Normal(name) => {
                if reparsed_by_push(name) {
                    return false;
                }
                // Win32 drops a trailing dot or space before the file system
                // sees the name, so `map.` is `map` - which the verbatim path
                // walked here would report as missing.
                if languages::win32_rewrites_name(name) {
                    return false;
                }
                walked.push(name);
            }
            Component::CurDir => continue,
            Component::ParentDir => {
                walked.pop();
                continue;
            }
            _ => return false,
        }
        if !walked.starts_with(&canon_root) {
            continue;
        }
        match fs::symlink_metadata(&walked) {
            Ok(meta) if meta.file_type().is_symlink() => return false,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return false,
        }
    }
    true
}

/// Where to read `name` - a file directly in the map directory - or `None`
/// when it must not be read: the directory is not plain, or the file is not
/// there, or it is a link that leaves the project or leads to another machine.
///
/// [`map_dir_is_plain`] covers the directory; a repository can just as well
/// commit `docs/map/module_map.json` as a link to `\\host\share\x`, and the
/// first `stat`, `is_file` or `canonicalize` of it would connect to that host
/// (a stall of 20 seconds and an NTLM negotiation) before any check on the
/// *result* could say no. Under the project root the file is therefore
/// resolved with [`resolve_inside`], which walks its links without following
/// them; a link that stays in the project still works. A map directory outside
/// the root is the user's own choice and is taken as it is.
pub(crate) fn map_file(root: &Path, map_dir: &Path, name: &str) -> Option<PathBuf> {
    if !map_dir_is_plain(root, map_dir) {
        return None;
    }
    let Ok(rel) = map_dir.strip_prefix(root) else {
        return Some(map_dir.join(name));
    };
    // Collapsed the way `map_dir_is_plain` walks it, on the canonical root: a
    // `..` that climbs out puts the directory outside the project, but one that
    // comes back in (`../<project>/docs/map`) is still the project's own.
    let canon_root = fs::canonicalize(root).ok()?;
    let mut walked = canon_root.clone();
    for part in rel.components() {
        match part {
            Component::Normal(n) => walked.push(n),
            Component::ParentDir => {
                walked.pop();
            }
            _ => {}
        }
    }
    let Ok(inside) = walked.strip_prefix(&canon_root) else {
        return Some(map_dir.join(name));
    };
    let mut rel_file = portable(inside);
    if !rel_file.is_empty() {
        rel_file.push('/');
    }
    rel_file.push_str(name);
    resolve_inside(&canon_root, &rel_file).ok()
}

/// The longest relative path [`resolve_inside`] looks at, in bytes and in
/// components.
const MAX_REL_LEN: usize = 32 * 1024;
const MAX_REL_COMPONENTS: usize = 256;

/// What one [`resolve_inside`] may spend on its link walk: components looked
/// at, over the whole chain of links. A real path needs a few dozen.
///
/// A step is not a constant cost: each look makes the kernel walk the whole
/// prefix, so what the budget bounds is about N^2/2 component lookups. 512
/// keeps the worst case under a second.
const WALK_STEPS: u32 = 512;
/// The same for one entry of the folder picker.
const PICKER_WALK_STEPS: u32 = 256;
// The longest relative path must fit in it with room for the links on the way.
const _: () = assert!(WALK_STEPS as usize >= MAX_REL_COMPONENTS + 64);
const _: () = assert!(PICKER_WALK_STEPS <= WALK_STEPS);

/// Does the link at `path` - an entry below `root`, which is not inspected -
/// stay on this machine? Always so off Windows, where a link cannot name a
/// share.
fn link_stays_local(root: &Path, path: &Path) -> bool {
    #[cfg(windows)]
    {
        let mut steps = PICKER_WALK_STEPS;
        matches!(
            links_to_network(path, root.components().count(), &mut 16, &mut steps),
            LinkWalk::Local
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (root, path);
        true
    }
}

/// What a walk along a path found out about the links on it.
#[cfg(windows)]
#[derive(Debug, PartialEq, Eq)]
enum LinkWalk {
    /// No link on the path leads off this machine.
    Local,
    /// A link leads to another machine (a share, a device namespace).
    Network,
    /// More links than any real path has, or a loop.
    TooDeep,
    /// A link whose target could not be read, or a component `push` would
    /// misread: not followed, because nothing can be said about it.
    Unusable,
}

/// Does `path` — walked from its start, component by component — go through a
/// symlink or junction whose target is on another machine?
///
/// The shape check refuses a UNC *string*; a link checked out inside the
/// project that points at one passes it, and `canonicalize` would connect to
/// the host before the containment check said no. So every component is
/// inspected with `symlink_metadata`, which does not follow, and only after
/// everything before it has been shown not to be such a link — nothing is
/// followed until it has been vetted.
///
/// **A link target is local only if it provably is.** `read_link` hands back
/// every NT-style target as a verbatim path — `\\.\GLOBALROOT\Device\Mup\host`
/// reads as `\\?\GLOBALROOT\...` and `\\.\pipe\x` as `\\?\pipe\x` — so a list of
/// the *remote* spellings (UNC, device namespace) misses the ones that reach a
/// share through the kernel's own redirector. A drive (`C:`, `\\?\C:`) or a
/// volume mount point (`\\?\Volume{...}`) is local; any other prefix is not.
///
/// When a component *is* a link, what remains of the path continues from the
/// link's target, and that whole path is walked the same way (a link whose
/// target passes through another link is the case a single look at the last
/// component misses). `budget` is shared across the chain. The first `skip`
/// components are not inspected — the canonical project root has no links.
#[cfg(windows)]
fn links_to_network(path: &Path, skip: usize, budget: &mut u8, steps: &mut u32) -> LinkWalk {
    use std::path::Prefix;

    let parts: Vec<Component> = path.components().collect();
    let mut walked = PathBuf::new();
    for (i, part) in parts.iter().enumerate() {
        match part {
            Component::CurDir => continue,
            Component::ParentDir => {
                walked.pop();
                continue;
            }
            // A drive or share prefix and the root are only built up, never
            // statted on their own: `\\?\C:` is not a path `symlink_metadata`
            // answers for, and an early "not there" here ended the walk before
            // it reached a single real component.
            Component::Prefix(_) | Component::RootDir => {
                walked.push(part.as_os_str());
                continue;
            }
            Component::Normal(name) => {
                if reparsed_by_push(name) {
                    return LinkWalk::Unusable;
                }
                walked.push(name);
            }
        }
        if i < skip {
            continue;
        }
        // A name Win32 reads as another one (`evil.` as `evil`): where it leads
        // depends on whoever opens the path, not on this walk. A name that
        // reaches here beyond the root comes from the path asked for or from a
        // link's target; both are the repository's.
        if matches!(part, Component::Normal(n) if crate::languages::win32_rewrites_name(n)) {
            return LinkWalk::Unusable;
        }
        // Each look costs the kernel a walk of the whole prefix, so a path of
        // thousands of real levels - or a link to one - is quadratic work.
        if *steps == 0 {
            return LinkWalk::TooDeep;
        }
        *steps -= 1;
        // Not there: `canonicalize` will say so, and nothing past it can be
        // reached.
        let Ok(meta) = fs::symlink_metadata(&walked) else {
            return LinkWalk::Local;
        };
        if !meta.file_type().is_symlink() {
            continue;
        }
        if *budget == 0 {
            return LinkWalk::TooDeep;
        }
        *budget -= 1;
        let Ok(target) = fs::read_link(&walked) else {
            return LinkWalk::Unusable;
        };
        if let Some(Component::Prefix(p)) = target.components().next() {
            let local = match p.kind() {
                Prefix::Disk(_) | Prefix::VerbatimDisk(_) => true,
                Prefix::Verbatim(name) => name
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("volume{"),
                _ => false,
            };
            if !local {
                return LinkWalk::Network;
            }
            // `C:x` names a drive and is relative to that drive's current
            // directory, which is the process's, not the link's.
            if !target.has_root() {
                return LinkWalk::Unusable;
            }
        } else if target.has_root() {
            // A root and no drive (`\foo`, or a raw NT path such as
            // `\Device\Mup\host\share\x` written into the reparse data by a
            // tool that does not go through the Win32 API): where it leads
            // depends on the kernel, not on this walk, and the redirector is
            // reachable by that spelling. Not local unless it provably is.
            return LinkWalk::Unusable;
        }
        // A relative target continues from the link's directory, and what it
        // shares with that directory has just been looked at and is plain: it
        // is not looked at again (each hop restarted from the drive, which made
        // a chain of links at the bottom of a deep tree cost the depth once per
        // hop). An absolute one can lead anywhere and is walked from its start.
        //
        // What is shared is counted, not assumed to be the whole directory:
        // `join` onto a verbatim path folds a `..` away, so `a\l -> ..\x` gives
        // `root\x`, no longer than `root\a`, and `x` - never looked at - would
        // sit inside a prefix counted as vetted.
        let (base, vetted) = if target.is_absolute() {
            (target, 0)
        } else {
            let parent = walked.parent().unwrap_or(&walked);
            let base = parent.join(target);
            let shared = parent
                .components()
                .zip(base.components())
                .take_while(|(a, b)| a == b)
                .count();
            (base, shared)
        };
        let rest: PathBuf = parts[i + 1..].iter().collect();
        return links_to_network(&base.join(rest), vetted, budget, steps);
    }
    LinkWalk::Local
}

/// A path as the desktop's own tools want it: the platform's separators and
/// no `\\?\` verbatim prefix.
///
/// [`portable`] is for display and for the editor template's `{file}`;
/// Explorer does not parse a forward-slash path — it opens the Documents
/// folder instead, measured on Windows 11 — so what is handed to it is this.
fn native_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
    }
    #[cfg(not(windows))]
    {
        text.into_owned()
    }
}

/// A stored forward-slash path (a project root) in the platform's separators.
fn native_str(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

/// Extensions whose "open" is to show the text, on every desktop this runs on.
///
/// A short list on purpose, and **not** the language table: `.js` is run by
/// Windows Script Host, `.py`/`.rb`/`.pl` by an installed interpreter, `.ps1`,
/// `.bat`, `.cmd`, `.sh`, `.command` by the shell, `.html`/`.svg` by a browser
/// with `file://` rights. What is opened for the reader comes from search hits,
/// a map's own file list and a message the embedded page can post — all of it
/// text from a repository somebody else wrote — so "open" must not mean "run".
const INERT_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mdx", "txt", "rst", "adoc", "json", "jsonc", "toml", "yaml", "yml", "log",
    "ini", "cfg", "conf", "lock", "lua", "vim", "rs", "c", "h", "cc", "cpp", "cxx", "hpp", "go",
    "java", "kt", "swift", "dart", "zig", "css", "scss", "less", "sql", "ex", "exs", "erl", "hs",
    "ml", "scala", "clj", "el",
];

/// Whether a file starts like something the system would execute: a shebang,
/// or the magic number of an ELF, Mach-O, fat Mach-O or PE binary.
#[cfg(unix)]
fn looks_like_program(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 4];
    let Ok(mut file) = fs::File::open(path) else {
        // Unreadable and executable: not worth the benefit of the doubt.
        return true;
    };
    let n = file.read(&mut head).unwrap_or(0);
    let head = &head[..n];
    head.starts_with(b"#!")
        || head.starts_with(b"\x7fELF")
        || head.starts_with(b"MZ")
        || matches!(
            head,
            [0xCF, 0xFA, 0xED, 0xFE]
                | [0xCE, 0xFA, 0xED, 0xFE]
                | [0xFE, 0xED, 0xFA, 0xCE]
                | [0xFE, 0xED, 0xFA, 0xCF]
                | [0xCA, 0xFE, 0xBA, 0xBE]
        )
}

/// Whether handing `path` to the desktop's file association is safe.
fn is_inert_document(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Marked executable *and* looks like a program: a script or a binary
        // is a program whatever it is called. The bit alone is not evidence —
        // FAT, exFAT and NTFS volumes, and some network shares, report every
        // file as executable, and refusing all of them would make "open" show
        // the file manager for every README on such a drive.
        if meta.permissions().mode() & 0o111 != 0 && looks_like_program(path) {
            return false;
        }
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| INERT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Show `native` in the desktop's file manager instead of opening it.
fn reveal_in_file_manager(native: &str) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("explorer");
        // `raw_arg`: Explorer wants `/select,"<path>"` with the quotes around
        // the path only; a whole-argument quote (what `arg` would add for a
        // path with a space) makes it ignore the selection.
        c.raw_arg(format!("/select,\"{native}\""));
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg("-R").arg(native);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        // `xdg-open` has no "select": the folder it is in is the nearest thing.
        let folder = Path::new(native)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| native.to_string());
        let mut c = std::process::Command::new("xdg-open");
        c.arg(folder);
        c
    };

    cmd.spawn().map(|_| ()).map_err(|e| {
        AppError::new("err.open.reveal")
            .with("path", native)
            .detail(e)
    })
}

/// Open a source file in an editor, at a line where one is known.
///
/// The path arrives repo-relative from the map page, because that is what
/// the artifact stores; it is resolved against the project it belongs to
/// here rather than there. The page has no idea where the repository sits
/// on this machine, and should not.
///
/// **The resolved path must stay inside the project.** The message comes
/// from a document this app embeds but does not author — a map generated
/// by an older engine, or one someone else produced — and `../../` in a
/// path is the difference between opening a file and opening any file.
#[tauri::command(async)]
fn open_in_editor(
    app: tauri::AppHandle,
    id: String,
    path: String,
    line: Option<u64>,
) -> Result<(), AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;

    let root = fs::canonicalize(&project.root).map_err(|e| {
        AppError::new("err.path.resolve")
            .with("path", &project.root)
            .detail(e)
    })?;
    if path.trim().is_empty() {
        return Err(AppError::new("err.editor.noFile"));
    }
    let target = resolve_inside(&root, &path)?;
    let file = portable(&target);

    let template = match ws.editor.as_deref() {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        // No template configured: hand it to the desktop — but only a file
        // whose "open" is to show it. An executable, a script or an archive
        // is shown in the file manager instead of being run.
        _ => {
            let native = native_path(&target);
            return if is_inert_document(&target) {
                open_externally(&native)
            } else {
                reveal_in_file_manager(&native)
            };
        }
    };

    // Split before substituting, so a path with a space in it stays one
    // argument rather than becoming two — `C:/Program Files/...` is the
    // normal case on this platform, not the exotic one.
    let mut parts = template.split_whitespace().map(|p| {
        p.replace("{file}", &file)
            .replace("{line}", &line.unwrap_or(1).to_string())
    });
    let program = parts
        .next()
        .ok_or_else(|| AppError::new("err.editor.empty"))?;
    let args: Vec<String> = parts.collect();

    let mut cmd = std::process::Command::new(&program);
    cmd.args(&args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().map(|_| ()).map_err(|e| {
        AppError::new("err.proc.run")
            .with("program", &program)
            .detail(e)
    })
}

/// Read or set the editor command template.
#[tauri::command]
fn editor_command(app: tauri::AppHandle, set: Option<String>) -> Result<Option<String>, AppError> {
    if let Some(value) = set {
        let trimmed = value.trim().to_string();
        return with_workspace(&app, |ws| {
            ws.editor = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.clone())
            };
            Ok(ws.editor.clone())
        });
    }
    Ok(read_workspace(&app)?.editor)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceEntry {
    name: String,
    projects: usize,
    active: bool,
}

/// Every workspace, with how many projects each holds.
///
/// The count is what makes the dashboard worth looking at — a list of
/// names says nothing about which one you meant.
#[tauri::command]
fn list_workspaces(app: tauri::AppHandle) -> Result<Vec<WorkspaceEntry>, AppError> {
    let active = read_settings(&app)?
        .active
        .unwrap_or_else(|| DEFAULT_WORKSPACE.to_string());
    // Reading the active one through `read_workspace` first performs the
    // migration, so a first run after an update lists the projects that
    // were inline rather than an empty Default.
    let _ = read_workspace(&app)?;

    let mut out = Vec::new();
    for name in workspace_names(&app)? {
        let path = workspace_projects_path(&app, &name)?;
        let projects = fs::read_to_string(&path)
            .ok()
            .and_then(|b| serde_json::from_str::<Vec<Project>>(&b).ok())
            .map(|p| p.len())
            .unwrap_or(0);
        out.push(WorkspaceEntry {
            active: name == active,
            name,
            projects,
        });
    }
    Ok(out)
}

/// Switch to a workspace, creating it if it does not exist yet.
///
/// Creating on switch rather than refusing: the two are the same act from
/// the dashboard's point of view, and a "create" that then needs a
/// "switch" is two clicks for one intention.
#[tauri::command]
fn switch_workspace(app: tauri::AppHandle, name: String) -> Result<Vec<Project>, AppError> {
    let clean = workspace_file_name(&name);
    let _guard = WORKSPACE_LOCK
        .lock()
        .map_err(|_| AppError::new("err.lock"))?;

    // The current list is already on disk under its own name; only the
    // pointer moves.
    let mut settings = read_settings(&app)?;
    settings.active = Some(clean.clone());
    settings.projects = Vec::new();
    let path = workspace_path(&app)?;
    let body = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppError::new("err.workspace.serialise").detail(e))?;
    write_atomic(&path, body).map_err(|e| path_err("err.path.write", &path, e))?;

    // A workspace that has never been written yet is an empty one, and an
    // empty file now is what makes it appear in the list.
    let list_path = workspace_projects_path(&app, &clean)?;
    if !list_path.is_file() {
        fs::write(&list_path, "[]").map_err(|e| path_err("err.path.write", &list_path, e))?;
    }
    drop(_guard);
    Ok(read_workspace(&app)?.projects)
}

/// Rename a workspace, moving its project list with it.
#[tauri::command]
fn rename_workspace(app: tauri::AppHandle, from: String, to: String) -> Result<(), AppError> {
    let from_clean = workspace_file_name(&from);
    let to_clean = workspace_file_name(&to);
    if from_clean == to_clean {
        return Ok(());
    }
    let src = workspace_projects_path(&app, &from_clean)?;
    let dst = workspace_projects_path(&app, &to_clean)?;
    if dst.is_file() {
        return Err(AppError::new("err.workspace.exists").with("name", &to_clean));
    }
    fs::rename(&src, &dst).map_err(|e| {
        AppError::new("err.path.rename")
            .with("from", src.display())
            .with("to", dst.display())
            .detail(e)
    })?;

    let mut settings = read_settings(&app)?;
    if settings.active.as_deref() == Some(from_clean.as_str()) {
        settings.active = Some(to_clean);
    }
    settings.projects = Vec::new();
    let path = workspace_path(&app)?;
    let body = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppError::new("err.workspace.serialise").detail(e))?;
    write_atomic(&path, body).map_err(|e| path_err("err.path.write", &path, e))
}

/// Delete a workspace.
///
/// Removes a list of paths, never a repository — the same promise
/// `remove_project` makes, one scale up, and the wording in the UI has to
/// carry it.
///
/// The last one cannot be deleted: an app with no workspace has nowhere to
/// put the next project, and "delete everything" is not what somebody
/// pressing delete on one row meant.
#[tauri::command]
fn delete_workspace(app: tauri::AppHandle, name: String) -> Result<Vec<WorkspaceEntry>, AppError> {
    let clean = workspace_file_name(&name);
    if workspace_names(&app)?.len() <= 1 {
        return Err(AppError::new("err.workspace.last"));
    }
    let path = workspace_projects_path(&app, &clean)?;
    fs::remove_file(&path).map_err(|e| path_err("err.path.delete", &path, e))?;

    let mut settings = read_settings(&app)?;
    if settings.active.as_deref() == Some(clean.as_str()) {
        // Deleting what you are standing in has to land somewhere, and the
        // first remaining one is the only choice that needs no guessing.
        settings.active = workspace_names(&app)?.into_iter().next();
    }
    settings.projects = Vec::new();
    let settings_path = workspace_path(&app)?;
    let body = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppError::new("err.workspace.serialise").detail(e))?;
    write_atomic(&settings_path, body)
        .map_err(|e| path_err("err.path.write", &settings_path, e))?;

    list_workspaces(app)
}

/// One directory of a project, as it is on disk.
///
/// One level per call: a repository is tens of thousands of files and a
/// reader opens perhaps a dozen directories. See `filetree.rs` on why this
/// is read live here rather than collected into the artifact.
#[tauri::command(async)]
fn file_tree(
    app: tauri::AppHandle,
    id: String,
    sub: String,
) -> Result<filetree::Listing, AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;
    filetree::list(Path::new(&project.root), &sub)
}

/// A project's own icon, if it ships one by any convention worth
/// following. `None` for most repositories, which is correct rather than a
/// failure — see `icon.rs` on why nothing is shown instead of a
/// placeholder.
#[tauri::command(async)]
fn project_icon(app: tauri::AppHandle, id: String) -> Result<Option<String>, AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;
    Ok(icon::find(Path::new(&project.root)).map(|p| portable(&p)))
}

/// Whether a project's map has fallen behind its sources.
///
/// Cheap by construction — modification times, not a regeneration — so the
/// answer is "something was touched since", never "the map is wrong". See
/// `freshness.rs` on why the wording of the mark follows from that.
#[tauri::command(async)]
fn map_freshness(app: tauri::AppHandle, id: String) -> Result<freshness::Freshness, AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;
    freshness::check(Path::new(&project.root), Path::new(&project.map_dir))
}

/// The files modified after the project's map was written — what the
/// "map outdated" mark is made of. At most `limit` of them, newest first,
/// plus the true total. See `freshness::changed_since_map`.
#[tauri::command(async)]
fn map_changes(
    app: tauri::AppHandle,
    id: String,
    limit: usize,
) -> Result<freshness::Changes, AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;
    freshness::changed_since_map(
        Path::new(&project.root),
        Path::new(&project.map_dir),
        limit.min(500),
    )
}

/// What the project is made of: files and lines per language, and how the
/// lines split into code, comments, documentation and data. Walks and reads
/// every file, so it is asked for, never shown by default. See `stats.rs`.
#[tauri::command]
async fn project_stats(app: tauri::AppHandle, id: String) -> Result<stats::Stats, AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?
        .clone();
    // Off the main thread: opening forty thousand files is not something the
    // window should wait for in place.
    tauri::async_runtime::spawn_blocking(move || {
        stats::collect(Path::new(&project.root), Path::new(&project.map_dir))
    })
    .await
    .map_err(AppError::task)?
}

/// A folder search: text in files (`mode` `"text"`) or file names (`"files"`).
///
/// `sub` is the folder to search, relative to the project root; empty is the
/// whole project. It is resolved and checked to stay inside the root, the
/// same rule `open_in_editor` applies to the file it opens: a search is a
/// read of the disk, and a path that came from a text box is not trusted to
/// stay where it started.
#[tauri::command]
async fn project_search(
    app: tauri::AppHandle,
    id: String,
    sub: String,
    query: String,
    mode: String,
    case_sensitive: bool,
) -> Result<search::Results, AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?
        .clone();
    let mode = search::Mode::parse(&mode)
        .ok_or_else(|| AppError::new("err.search.mode").with("mode", &mode))?;

    let root = fs::canonicalize(&project.root).map_err(|e| {
        AppError::new("err.path.resolve")
            .with("path", &project.root)
            .detail(e)
    })?;
    let scope = resolve_inside(&root, sub.trim_matches('/'))?;
    // The map directory, canonicalised the same way, so the comparison the
    // walk makes is between paths of one shape.
    // Not even canonicalised when it is a link: that would follow it.
    let map_dir = if map_dir_is_plain(Path::new(&project.root), Path::new(&project.map_dir)) {
        fs::canonicalize(&project.map_dir).unwrap_or_else(|_| PathBuf::from(&project.map_dir))
    } else {
        PathBuf::new()
    };

    tauri::async_runtime::spawn_blocking(move || {
        search::run(&root, &scope, &map_dir, &query, mode, case_sensitive, 300)
    })
    .await
    .map_err(AppError::task)?
}

/// Search what the map shows — see `search::view`.
#[tauri::command(async)]
fn view_search(
    app: tauri::AppHandle,
    id: String,
    query: String,
) -> Result<search::ViewResults, AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;
    // The map file itself, not just its directory: see `map_file`.
    let Some(map_json) = map_file(
        Path::new(&project.root),
        Path::new(&project.map_dir),
        "module_map.json",
    ) else {
        return Ok(search::ViewResults::default());
    };
    search::view(&map_json, &query, 200)
}

/// Which projects in this workspace depend on which others.
///
/// The whole workspace in one call rather than one per project: the answer
/// is a graph, and a graph assembled from N independent answers would be N
/// reads of the same module index. On the author's tree that index is 1 820
/// names built from 30 artifacts, and it is the same index for every row of
/// the result.
///
/// Everything it needs is on disk, so it needs no engine and works for a
/// workspace whose engine is not configured at all — the artifact really is
/// the extension point, and this is the first consumer of it that is not the
/// engine itself.
#[tauri::command(async)]
fn workspace_deps(app: tauri::AppHandle) -> Result<deps::Deps, AppError> {
    let ws = read_workspace(&app)?;
    let list: Vec<(String, Option<PathBuf>)> = ws
        .projects
        .iter()
        .map(|p| {
            // `None` - a linked map directory or file, or no map - is listed as
            // unread rather than silently missing: see `map_file`.
            (
                p.id.clone(),
                map_file(Path::new(&p.root), Path::new(&p.map_dir), "module_map.json"),
            )
        })
        .collect();
    Ok(deps::resolve(&list))
}

/// Hand a path or URL to whatever the desktop opens it with.
///
/// Written here rather than reached for through a plugin because it is
/// three lines per platform and the alternative is another capability
/// surface for the webview. Nothing from the page reaches this: both
/// callers pass a path this process produced — but "produced by this
/// process" still means a project's own folder name, which is not this
/// program's to constrain (`Foo & Bar`, say, is a real folder name nobody
/// would call hostile).
/// `explorer <target>`, not `cmd /c start "" <target>`. `cmd.exe` parses its
/// *whole command line* for `&`, `|`, `^`, `<`, `>` before `start` ever sees
/// an argument — confirmed by building a Rust CLI that called exactly the
/// previous line with a path containing `&` and watching the part after it
/// run as a second command. That is not this process's own quoting failing
/// (`Command` quotes each argument correctly for a normal argv-parsing
/// program); it is that `cmd.exe`'s *own* parser does not read its command
/// line that way, no matter how correctly the caller quoted it — the
/// documented reason Rust's std singles out `cmd.exe`/batch files as unsafe
/// targets for argument passing. `explorer.exe` is an ordinary Win32 program
/// (`CommandLineToArgvW` parsing, the same convention `Command` quotes for)
/// and still honours the default browser for a URL and the default handler
/// for a file, same as `start` did. Pulled out of `open_externally` so the
/// one-argument shape is a thing a test can check without opening a window.
#[cfg(target_os = "windows")]
fn windows_open_command(target: &str) -> std::process::Command {
    let mut c = std::process::Command::new("explorer");
    c.arg(target);
    c
}

fn open_externally(target: &str) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    let mut cmd = windows_open_command(target);
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(target);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target);
        c
    };

    cmd.spawn().map(|_| ()).map_err(|e| {
        AppError::new("err.open.failed")
            .with("target", target)
            .detail(e)
    })
}

/// Open a project's map in the system browser.
///
/// Through the same local server the embedded view uses, not as a `file://`
/// URL: the generated page fetches its own sibling files, and a browser
/// opening it off disk applies a different origin policy than the one it
/// was tested under. Same page, same server, second window.
#[tauri::command]
fn open_map_in_browser(app: tauri::AppHandle, id: String) -> Result<String, AppError> {
    let url = serve_project(app, id)?;
    open_externally(&url)?;
    Ok(url)
}

/// Open one of this project's own documentation pages.
///
/// An allowlist of two, not a URL parameter. The webview asks for a *page*
/// by name and this decides what that means — so nothing the page can say
/// turns into a browser opening an arbitrary address, which is what a
/// generic `open_url` command would be.
#[tauri::command]
fn open_docs(page: String) -> Result<(), AppError> {
    const BASE: &str = "https://github.com/WKDSteve/docmap-desktop/blob/main/docs/";
    let target = match page.as_str() {
        "usage" => format!("{BASE}USAGE.md"),
        // The single most common confusion this app produces: that the
        // engine is a separate program this is a window onto.
        "engine" => format!("{BASE}USAGE.md#the-engine-indicator"),
        other => return Err(AppError::new("err.docs.page").with("page", other)),
    };
    open_externally(&target)
}

/// Show a project's root in the desktop's file manager.
///
/// Answers "where is this actually" without a path to copy out of a
/// tooltip. The repository root, not the map directory: the map is an
/// output, and the question is about the project.
#[tauri::command]
fn reveal_project(app: tauri::AppHandle, id: String) -> Result<(), AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;
    open_externally(&native_str(&project.root))
}

/// Show the folder this app keeps its settings and workspaces in.
///
/// The thing a bug report needs and nobody can find: `workspace.json`, the
/// workspace lists beside it, and whatever went wrong in them. Opened
/// rather than printed — a path in a dialog is a path somebody has to
/// retype.
#[tauri::command]
fn reveal_settings(app: tauri::AppHandle) -> Result<(), AppError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::new("err.config.noDir").detail(e))?;
    fs::create_dir_all(&dir).map_err(|e| path_err("err.path.create", &dir, e))?;
    open_externally(&dir.to_string_lossy())
}

/// Open a prefilled report on GitHub.
///
/// Opens, never posts — see `feedback.rs` for why. The reader lands on
/// GitHub's own form with the text already in it, reads it, and presses
/// Submit as themselves.
///
/// `repo` is a choice between two names, not a string that reaches a URL:
/// feedback about the window and feedback about the engine belong in
/// different trackers, and the reader is the only one who knows which.
#[tauri::command]
fn open_feedback(
    repo: String,
    topic: String,
    title: String,
    body: String,
) -> Result<String, AppError> {
    let repo = match repo.as_str() {
        "desktop" => "docmap-desktop",
        "engine" => "docmap.nvim",
        other => return Err(AppError::new("err.feedback.repo").with("repo", other)),
    };
    let url = feedback::url(repo, &topic, &title, &body)?;
    open_externally(&url)?;
    Ok(url)
}

/// What the feedback form offers to attach: the versions a report is
/// useless without, and nothing else.
///
/// Shown in the dialog before anything is opened, and again on GitHub's
/// own form before anything is submitted — so it is never sent anywhere
/// the writer has not read it first.
#[tauri::command]
fn about_info(app: tauri::AppHandle) -> AboutInfo {
    let engine = engine_info(app).ok();
    AboutInfo {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        engine_path: engine.as_ref().and_then(|e| e.path.clone()),
        grammars: engine.and_then(|e| e.grammars),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AboutInfo {
    app_version: String,
    os: String,
    arch: String,
    engine_path: Option<String>,
    grammars: Option<String>,
}

/// Name the window after what it is showing.
///
/// The title bar and the sidebar heading both read `docmap`, one directly
/// above the other, which says the same thing twice and answers nothing.
/// The sidebar keeps the application's name — that is what a heading in an
/// application is for — and the title bar takes the subject, which is what
/// a title bar is for, and what the taskbar and the window switcher show.
///
/// A command rather than the frontend calling `setTitle` itself: the
/// `core:window:default` permission set grants `allow-title`, which reads,
/// and not `allow-set-title`. Widening the webview's own permissions for
/// this would be a larger grant than the one thing needed.
#[tauri::command]
fn set_window_title(app: tauri::AppHandle, title: String) -> Result<(), AppError> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::new("err.window.none"))?;
    window
        .set_title(&title)
        .map_err(|e| AppError::new("err.window.failed").detail(e))
}

/// Set the window's zoom factor.
///
/// On the webview rather than in CSS: the map is an iframe served from
/// `127.0.0.1` while the shell is on `tauri://`, so a page-level transform
/// stops at the frame boundary and would scale everything *except* the
/// dense page the reader wanted bigger.
///
/// Clamped here rather than in the frontend because this is the side that
/// has to survive being asked for zero.
#[tauri::command]
fn set_zoom(app: tauri::AppHandle, factor: f64) -> Result<f64, AppError> {
    let factor = factor.clamp(0.5, 3.0);
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::new("err.window.none"))?;
    window
        .set_zoom(factor)
        .map_err(|e| AppError::new("err.window.failed").detail(e))?;
    Ok(factor)
}

/// Install (or replace) the window menu with the frontend's labels.
///
/// Called once after i18n initialises and again on every language change
/// and every selection change. Replacing the whole menu rather than
/// mutating items is what Tauri supports for a label change, and the menu
/// is nine items — the cost is not worth a second code path.
#[tauri::command]
fn set_menu(
    app: tauri::AppHandle,
    labels: HashMap<String, String>,
    has_project: bool,
    state: menu::ViewState,
) -> Result<(), AppError> {
    let built = menu::build(&app, &labels, has_project, &state)?;
    app.set_menu(built)
        .map_err(|e| AppError::new("err.menu.build").detail(e))?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Registered once. `set_menu` replaces the menu on every
            // language and selection change, and a handler registered per
            // rebuild would fire once per rebuild it outlived.
            menu::forward_clicks(app.handle());
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        // Every command here fails with an `apperr::AppError` and nothing
        // else: a catalog key, named params and the untranslated detail (the
        // OS error, git's, gh's or Neovim's own words), plus the English
        // sentence as a marked fallback. The window renders it with
        // `errorText` (`src/lib/errors.js`) in its own language; a command
        // never returns a sentence baked in Rust. The keys are listed once,
        // in `apperr::ENGLISH`, and `src/lib/errors.test.js` holds that list
        // against the catalog both ways. The page host's `/api/*` answers
        // (`server.rs`) use the same shape under `api.*` keys, which belong to
        // the generated page's catalog, not to this window's.
        // The runs of the rules agent live here, not in a window: closing the
        // window does not stop one, quitting the app pauses it (see below).
        .manage(agent::runner::Runner::default())
        .invoke_handler(tauri::generate_handler![
            list_projects,
            add_project,
            inspect_folder,
            import_many,
            remove_project,
            project_scope_get,
            project_scope_set,
            import_from_url,
            map_status,
            workspace_deps,
            scan_languages,
            list_github_repos,
            engine_info,
            engine_languages,
            grammar_dir,
            set_engine,
            set_grammars,
            generate,
            check_map,
            nvim_info,
            set_nvim_path,
            set_nvim_config_dir,
            import_from_nvim_config,
            serve_project,
            set_menu,
            open_map_in_browser,
            reveal_project,
            reveal_settings,
            open_docs,
            set_zoom,
            set_window_title,
            open_feedback,
            about_info,
            map_freshness,
            map_changes,
            project_stats,
            project_search,
            view_search,
            project_icon,
            open_in_editor,
            file_tree,
            list_workspaces,
            switch_workspace,
            rename_workspace,
            delete_workspace,
            editor_command,
            telemetry_info,
            set_telemetry,
            traffic_info,
            traffic_detail,
            traffic_list,
            traffic_settings,
            traffic_set_dir,
            traffic_ask_neovim,
            traffic_set_hidden,
            traffic_refresh,
            save_text,
            agent::commands::agent_url,
            agent::commands::agent_health,
            agent::commands::agent_preview,
            agent::commands::agent_start,
            agent::commands::agent_resume,
            agent::commands::agent_cancel,
            agent::commands::agent_list,
            agent::commands::agent_status,
            agent::commands::agent_running,
            agent::commands::agent_freshness,
            agent::commands::agent_ignore
        ])
        .build(tauri::generate_context!())
        .expect("docmap-desktop failed to start")
        .run(|app, event| {
            // A run that outlives the app would spend money with nobody
            // watching: on exit every live run is written as paused instead.
            if let tauri::RunEvent::Exit = event {
                app.state::<agent::runner::Runner>()
                    .pause_all(std::time::Duration::from_secs(3));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_that_git_would_read_as_an_option_or_a_helper_is_refused() {
        for bad in [
            "--upload-pack=calc",
            "-u calc",
            "-cprotocol.ext.allow=always",
            "ext::sh -c calc",
            "ext::calc",
            "fd::3",
            "file:///C:/secret",
            "/etc/passwd",
            "C:\\Users\\x\\repo",
            "../other",
            "https://host/a b",
            "https://host/a\nb",
            "",
        ] {
            assert!(clone_url_problem(bad).is_some(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn the_urls_a_user_pastes_from_a_hosting_page_are_accepted() {
        for good in [
            "https://github.com/WKDSteve/docmap-desktop",
            "https://github.com/WKDSteve/docmap-desktop.git",
            "HTTPS://example.org/a/b.git",
            "http://intranet/a/b",
            "ssh://git@host:2222/a/b.git",
            "git://host/a/b.git",
            "git@github.com:WKDSteve/docmap-desktop.git",
            "https://[::1]/a/b.git",
            "ssh://git@[2001:db8::1]/a/b.git",
        ] {
            assert_eq!(clone_url_problem(good), None, "{good:?} must pass");
        }
    }

    #[test]
    fn the_folder_of_a_clone_is_a_plain_name_and_never_a_way_out() {
        assert_eq!(
            repo_dir_name("https://github.com/o/docmap-desktop.git").as_deref(),
            Some("docmap-desktop")
        );
        assert_eq!(
            repo_dir_name("git@github.com:o/my_repo.v2/").as_deref(),
            Some("my_repo.v2")
        );
        for bad in [
            "https://host/a/..",
            "https://host/a/../",
            "https://host/a/.",
            "https://host/a/..git",
            "https://host/x\\..",
            "https://host/C:",
            "https://host/",
            "https://host/a/b?x=1",
            "https://host/a/b%2f..",
        ] {
            let name = repo_dir_name(bad);
            assert!(
                name.as_deref().is_none_or(|n| n != ".." && n != "."),
                "{bad:?} gave {name:?}"
            );
            if let Some(n) = name {
                assert!(!n.contains(['/', '\\', ':']), "{bad:?} gave {n:?}");
            }
        }
        for device in [
            "https://host/o/con",
            "https://host/o/NUL.git",
            "https://host/o/com1",
            "https://host/o/lpt9.txt",
            "https://host/o/aux.v2",
            "https://host/o/name.",
        ] {
            assert_eq!(repo_dir_name(device), None, "{device:?} is no folder name");
        }
        assert_eq!(
            repo_dir_name("https://host/o/com10").as_deref(),
            Some("com10"),
            "only COM1..COM9 are devices"
        );
        assert_eq!(repo_dir_name("https://host/a/.."), None);
        assert_eq!(repo_dir_name("https://host/x\\.."), None);
        assert_eq!(repo_dir_name("https://host/a/b?x=1"), None);
    }

    /// A folder that looks like a clone of `url`: `.git/config` with that
    /// `origin`, written by git itself so the file is in git's own format.
    fn fake_clone(dir: &Path, url: &str) {
        fs::create_dir_all(dir.join(".git")).unwrap();
        let ok = std::process::Command::new("git")
            .arg("config")
            .arg("--file")
            .arg(portable(&dir.join(".git").join("config")))
            .args(["remote.origin.url", url])
            .status()
            .unwrap()
            .success();
        assert!(ok, "git config could not write {}", dir.display());
    }

    #[test]
    fn the_folder_of_a_clone_tells_two_repositories_of_one_name_apart() {
        let a = clone_dir_name("https://github.com/a/foo").unwrap();
        let b = clone_dir_name("https://github.com/b/foo").unwrap();
        let other_host = clone_dir_name("https://gitlab.com/a/foo").unwrap();
        assert_ne!(a, b);
        assert_ne!(a, other_host);
        for name in [&a, &b, &other_host] {
            assert!(name.starts_with("foo-"), "{name:?}");
            // Still a plain name: what repo_dir_name lets through, plus [0-9a-f-].
            assert!(
                name.len() == "foo-".len() + 8
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')),
                "{name:?}"
            );
        }
        // One repository, however the hosting page spells its URL.
        for same in [
            "https://github.com/a/foo/",
            "https://github.com/a/foo.git",
            "https://github.com/a/foo.git/",
        ] {
            assert_eq!(clone_dir_name(same).as_ref(), Some(&a), "{same:?}");
        }
        // Fixed across builds: an old clone must keep its name.
        assert_eq!(
            clone_dir_name("https://github.com/a/foo").as_deref(),
            Some("foo-c2edc6b0")
        );
        assert_eq!(clone_dir_name("https://host/o/.."), None);
        assert_eq!(clone_dir_name("https://host/o/con"), None);
    }

    #[test]
    fn importing_the_same_url_twice_finds_the_first_clone() {
        let cache = crate::testutil::fresh_dir("clone-target-same-url");
        let url = "https://github.com/a/foo";
        let CloneTarget::Clone(first) = clone_target(&cache, url).unwrap() else {
            panic!("an empty cache has nothing to reuse");
        };
        assert_eq!(first, cache.join("foo-c2edc6b0"));
        fake_clone(&first, url);
        for again in [url, "https://github.com/a/foo.git"] {
            assert_eq!(
                clone_target(&cache, again).unwrap(),
                CloneTarget::Existing(first.clone()),
                "{again:?}"
            );
        }
    }

    #[test]
    fn two_urls_with_the_same_last_part_become_two_clones() {
        let cache = crate::testutil::fresh_dir("clone-target-same-name");
        let a = "https://github.com/a/foo";
        let b = "https://github.com/b/foo";
        let CloneTarget::Clone(dir_a) = clone_target(&cache, a).unwrap() else {
            panic!("an empty cache has nothing to reuse");
        };
        fake_clone(&dir_a, a);
        let CloneTarget::Clone(dir_b) = clone_target(&cache, b).unwrap() else {
            panic!("b/foo must not open the clone of a/foo");
        };
        assert_ne!(dir_a, dir_b);
        assert_eq!(dir_b.parent(), Some(cache.as_path()));
        fake_clone(&dir_b, b);
        assert_eq!(
            clone_target(&cache, a).unwrap(),
            CloneTarget::Existing(dir_a)
        );
        assert_eq!(
            clone_target(&cache, b).unwrap(),
            CloneTarget::Existing(dir_b)
        );
    }

    #[test]
    fn a_clone_under_the_old_plain_name_is_found_only_for_its_own_url() {
        let cache = crate::testutil::fresh_dir("clone-target-old-name");
        let a = "https://github.com/a/foo.git";
        let old = cache.join("foo");
        fake_clone(&old, a);
        // Its own URL, with or without `.git`: the old folder, no new clone.
        for same in [a, "https://github.com/a/foo"] {
            assert_eq!(
                clone_target(&cache, same).unwrap(),
                CloneTarget::Existing(old.clone()),
                "{same:?}"
            );
        }
        // Another repository named foo: a folder of its own, not a/foo's.
        assert_eq!(
            clone_target(&cache, "https://github.com/b/foo").unwrap(),
            CloneTarget::Clone(cache.join(clone_dir_name("https://github.com/b/foo").unwrap()))
        );
        // A folder of that name that is no clone at all is not taken either.
        let stray = cache.join("bar");
        fs::create_dir_all(&stray).unwrap();
        assert!(matches!(
            clone_target(&cache, "https://github.com/a/bar").unwrap(),
            CloneTarget::Clone(dir) if dir != stray
        ));
    }

    #[test]
    fn every_spelling_of_one_repository_gets_one_clone_folder() {
        // What a hosting page, the GitHub picker or a pasted URL may say for
        // github.com/a/foo: one repository, one folder, no second clone.
        for same in [
            "https://github.com/a/foo",
            "https://github.com/a/foo.GIT",
            "https://github.com/A/Foo",
            "HTTPS://GitHub.com/a/foo/",
            "https://github.com:443/a/foo",
            "http://github.com/a/foo",
            "https://user:token@github.com/a/foo.git",
            "git://github.com/a/foo",
            "ssh://git@github.com/a/foo",
            "ssh://git@github.com:22/a/foo.git",
            "git@github.com:a/foo.git",
            "git@github.com:/a/foo",
        ] {
            assert_eq!(
                clone_dir_name(same).as_deref(),
                Some("foo-c2edc6b0"),
                "{same:?}"
            );
        }
        // Still told apart: another owner, another host, another port, and a
        // path whose case a plain git server may well care about.
        let differ = |a: &str, b: &str| {
            assert_ne!(clone_url_key(a), clone_url_key(b), "{a:?} vs {b:?}");
        };
        differ("https://github.com/a/foo", "https://github.com/b/foo");
        differ("https://github.com/a/foo", "https://gitlab.com/a/foo");
        differ("https://example.org/A/Foo", "https://example.org/a/foo");
        differ(
            "https://example.org:8443/a/foo",
            "https://example.org/a/foo",
        );
        // A bracketed IPv6 host, in the scp form as well.
        for same in ["git@[::1]:a/foo.git", "ssh://git@[::1]:2222/a/foo"] {
            assert_eq!(
                clone_url_key(same),
                clone_url_key("https://[::1]/a/foo"),
                "{same:?}"
            );
        }
    }

    #[test]
    fn a_clone_made_from_another_spelling_of_the_url_is_found() {
        let cache = crate::testutil::fresh_dir("clone-target-spelling");
        // Cloned over ssh, imported again from the picker's https URL.
        let new = cache.join("foo-c2edc6b0");
        fake_clone(&new, "git@github.com:a/foo.git");
        assert_eq!(
            clone_target(&cache, "https://github.com/A/Foo").unwrap(),
            CloneTarget::Existing(new)
        );
        // An old plain-name clone made over ssh, imported again over https.
        let old = cache.join("bar");
        fake_clone(&old, "git@github.com:a/bar.git");
        assert_eq!(
            clone_target(&cache, "https://github.com/a/bar").unwrap(),
            CloneTarget::Existing(old)
        );
    }

    #[test]
    fn a_hash_shared_with_another_url_does_not_open_that_urls_clone() {
        // Two real URLs whose 32-bit hashes collide.
        let a = "https://github.com/user54873/foo";
        let b = "https://github.com/user101577/foo";
        assert_eq!(clone_dir_name(a).as_deref(), Some("foo-354dbd88"));
        assert_eq!(clone_dir_name(b), clone_dir_name(a));

        let cache = crate::testutil::fresh_dir("clone-target-collision");
        let base = cache.join("foo-354dbd88");
        assert_eq!(
            clone_target(&cache, a).unwrap(),
            CloneTarget::Clone(base.clone())
        );
        fake_clone(&base, a);
        let CloneTarget::Clone(dir_b) = clone_target(&cache, b).unwrap() else {
            panic!("b must not open the clone of a, which shares its hash");
        };
        assert_eq!(dir_b, cache.join("foo-354dbd88-2"));
        fake_clone(&dir_b, b);
        assert_eq!(
            clone_target(&cache, a).unwrap(),
            CloneTarget::Existing(base.clone())
        );
        assert_eq!(
            clone_target(&cache, b).unwrap(),
            CloneTarget::Existing(dir_b.clone())
        );
        // The first slot emptied by hand: b's clone is still found, not
        // cloned again into the hole.
        fs::remove_dir_all(&base).unwrap();
        assert_eq!(
            clone_target(&cache, b).unwrap(),
            CloneTarget::Existing(dir_b)
        );
        // A folder of the hashed name that is no clone is not taken.
        fs::create_dir_all(&base).unwrap();
        assert_eq!(
            clone_target(&cache, a).unwrap(),
            CloneTarget::Clone(cache.join("foo-354dbd88-3"))
        );
    }

    #[test]
    fn an_imported_project_is_named_after_the_repository_not_its_clone_folder() {
        let cache = crate::testutil::fresh_dir("imported-name");
        let mut ws = Workspace::default();
        let mut import = |url: &str| {
            let dir = cache.join(clone_dir_name(url).unwrap());
            fs::create_dir_all(&dir).unwrap();
            let name = imported_name(url, &ws.projects);
            add_one_named(&mut ws, &portable(&dir), name)
                .unwrap()
                .unwrap()
                .name
        };
        // The name the URL gives, in the case it gives it (the picker's URL).
        assert_eq!(import("https://github.com/Owner/Docmap"), "Docmap");
        assert_eq!(import("https://github.com/a/foo"), "foo");
        // A second repository of one name: told apart by its owner.
        assert_eq!(import("https://github.com/b/foo.git"), "b/foo");
        assert_eq!(import("git@gitlab.com:c/foo.git"), "c/foo");
        // A folder added by hand keeps its folder name.
        let plain = cache.join("by-hand");
        fs::create_dir_all(&plain).unwrap();
        let added = add_one(&mut ws, &portable(&plain)).unwrap().unwrap();
        assert_eq!(added.name, "by-hand");
    }

    // `open_externally`'s own `windows_open_command` is not spawned here (it
    // opens a real window on this machine); what is checked is the `Command`
    // it builds. A regression here means `target` is no longer passed as one
    // argv entry to a non-shell program -- the exact class of change that
    // reintroduces the `&`-splits-the-command-line defect `cmd /c start` had.
    #[cfg(target_os = "windows")]
    #[test]
    fn open_externally_passes_the_target_as_one_argument_to_explorer_not_cmd() {
        let target = r"C:\repos\Foo & Bar\file.txt";
        let cmd = windows_open_command(target);

        assert_eq!(cmd.get_program(), "explorer");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec![std::ffi::OsStr::new(target)]);
    }

    #[test]
    fn a_map_directory_setting_must_stay_inside_the_project() {
        let ok = |v: &str| map_dir_setting(Some(v.into()));
        assert_eq!(ok("docs/map"), Ok(Some("docs/map".into())));
        assert_eq!(ok("  ./out/ "), Ok(Some("out".into())));
        assert_eq!(map_dir_setting(None), Ok(None));
        assert_eq!(ok("   "), Ok(None));
        for bad in [
            "../maps",
            "docs/../../maps",
            "..",
            r"..\maps",
            "/maps",
            r"\maps",
            "C:/maps",
            "C:maps",
            // Only a drive once the `./` is gone, which is what gets stored.
            "./C:/maps",
            r".\C:\maps",
        ] {
            assert!(ok(bad).is_err(), "{bad} must be refused");
        }
        // A name that merely contains dots is fine.
        assert_eq!(ok("docs/map..v2"), Ok(Some("docs/map..v2".into())));
    }

    #[test]
    fn a_map_directory_setting_is_refused_where_the_engine_would_refuse_it() {
        let ok = |v: &str| map_dir_setting(Some(v.into()));
        // The engine's `safe_out_dir` allows ASCII letters, digits, `-`, `_` and
        // `.` in a name, and no `.` or `..` name: the rest fails at the next
        // Generate, with a message that reads as if the map were at fault.
        for bad in [
            "docs/my map",
            "docs/./map",
            "docs/map/.",
            ".",
            "./.",
            "dokumentation/\u{fc}bersicht",
            "~/maps",
            "docs/map:x",
            "1:x",
            "docs/ma\np",
            "docs/ma\0p",
        ] {
            let err = ok(bad).unwrap_err();
            assert_eq!(err.key(), "err.outDir.unsafe", "{bad}: {err}");
            assert!(err.contains("only letters, digits"), "{bad}: {err}");
        }
        // What it takes stays accepted: dots inside a name, a name that starts
        // with one, empty names (the engine collapses them), the `./` and the
        // surrounding space that `rel_path` strips before the engine sees it.
        for (typed, stored) in [
            ("docs/map", "docs/map"),
            ("docs/map..v2", "docs/map..v2"),
            ("docs//map", "docs//map"),
            (".github/map", ".github/map"),
            ("Docs/Map_v2-1.0", "Docs/Map_v2-1.0"),
            ("  ./out/ ", "out"),
            (r"docs\map", "docs/map"),
        ] {
            assert_eq!(ok(typed), Ok(Some(stored.into())), "{typed}");
        }
        // The refusal names the value as typed.
        assert!(ok("docs/my map").unwrap_err().contains("`docs/my map`"));
    }

    #[test]
    fn a_name_ending_in_a_dot_is_refused_where_windows_would_rewrite_it() {
        // Win32 reads `map.` as `map`, and the engine's `safe_fs` refuses such a
        // name there and nowhere else.
        for dir in ["docs/map.", "docs/map./x", "docs/..."] {
            assert!(is_safe_out_dir(dir, false), "{dir}");
            assert!(!is_safe_out_dir(dir, true), "{dir} on Windows");
        }
        for dir in ["docs/map", "docs/map..v2", ".github/map"] {
            assert!(is_safe_out_dir(dir, false), "{dir}");
            assert!(is_safe_out_dir(dir, true), "{dir} on Windows");
        }
        // Nothing left once the empty names are collapsed.
        assert!(!is_safe_out_dir("", false));
        assert!(!is_safe_out_dir("//", false));
        // The engine folds backslashes first, so a stored `docs\map` is `docs/map`
        // and a stored `\maps` is absolute.
        assert!(is_safe_out_dir(r"docs\map", false));
        assert!(!is_safe_out_dir(r"\maps", false));
    }

    #[test]
    fn a_path_is_refused_on_its_shape_before_the_disk_is_touched() {
        let root = fs::canonicalize(std::env::temp_dir()).unwrap();
        // Absolute, drive-lettered and rooted forms all escape `join`.
        // By the *message*: a refusal after the disk had been touched ends
        // in `Err` too (the UNC form would only make the test slow while
        // Windows tried to reach the host), and that is the bug.
        let shape = |rel: &str| resolve_inside(&root, rel).unwrap_err();
        let absolute = root.to_string_lossy().to_string();
        assert!(shape(&absolute).contains("not a path inside the project"));
        #[cfg(windows)]
        {
            assert!(shape(r"\\192.0.2.1\share\x.lua").contains("not a path inside the project"));
            assert!(shape("//192.0.2.1/share/x.lua").contains("not a path inside the project"));
            assert!(shape(r"C:foo").contains("not a path inside the project"));
            assert!(shape(r"\foo").contains("not a path inside the project"));
        }
        #[cfg(unix)]
        assert!(shape("/etc/passwd").contains("not a path inside the project"));
        assert!(shape("a\0b").contains("NUL"));
    }

    #[test]
    fn a_path_inside_the_root_resolves_and_one_that_leaves_it_does_not() {
        let root = fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = root.join("docmap-resolve-inside");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/a.txt"), "x").unwrap();
        let dir = fs::canonicalize(&dir).unwrap();

        assert_eq!(
            resolve_inside(&dir, "sub/a.txt").unwrap(),
            dir.join("sub/a.txt")
        );
        assert_eq!(
            resolve_inside(&dir, "sub/../sub/a.txt").unwrap(),
            dir.join("sub/a.txt")
        );
        assert_eq!(resolve_inside(&dir, "").unwrap(), dir);
        assert!(resolve_inside(&dir, "..").is_err(), "the parent is outside");
        assert!(resolve_inside(&dir, "nope.txt").is_err());
    }

    #[test]
    fn only_documents_whose_open_is_to_show_them_go_to_the_desktop() {
        let dir = std::env::temp_dir().join("docmap-inert");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, inert) in [
            ("a.md", true),
            ("a.TXT", true),
            ("a.lua", true),
            ("a.js", false),
            ("a.bat", false),
            ("a.ps1", false),
            ("a.py", false),
            ("a.html", false),
            ("a.svg", false),
            ("a.exe", false),
            ("a.command", false),
            ("noextension", false),
        ] {
            let path = dir.join(name);
            fs::write(&path, "x").unwrap();
            assert_eq!(is_inert_document(&path), inert, "{name}");
        }
        assert!(!is_inert_document(&dir), "a folder is shown, not opened");
        assert!(!is_inert_document(&dir.join("missing.md")));
    }

    #[cfg(unix)]
    #[test]
    fn the_execute_bit_alone_does_not_make_a_document_a_program() {
        // A FAT or exFAT volume reports every file as 0755.
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join("docmap-inert-fat");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("README.md");
        fs::write(&path, "# readme\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_inert_document(&path));
    }

    #[cfg(windows)]
    #[test]
    fn a_link_to_another_machine_is_refused_before_it_is_followed() {
        let root = fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = root.join("docmap-network-link");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dir = fs::canonicalize(&dir).unwrap();
        // Creating a symlink needs a privilege on Windows; without it there
        // is nothing to test here.
        let target = format!(r"\\{}\share\x.md", crate::testutil::unc_host());
        if std::os::windows::fs::symlink_file(&target, dir.join("l.md")).is_err() {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        let started = std::time::Instant::now();
        // As written, behind a component that does not exist, and behind `.`:
        // all of them are the same path once collapsed, and all are refused.
        for rel in ["l.md", "missing/../l.md", "./l.md", "a/../l.md"] {
            let err = resolve_inside(&dir, rel).unwrap_err();
            assert!(err.contains("link to another machine"), "{rel}: {err}");
        }
        assert!(
            started.elapsed().as_secs() < 3,
            "it must not wait for the host"
        );
    }

    use crate::testutil::{dir_link, file_link, fresh_dir};

    #[test]
    fn a_map_directory_that_is_a_link_is_not_plain() {
        let root = fresh_dir("docmap-plain-map");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("real")).unwrap();
        assert!(
            map_dir_is_plain(&root, &root.join("docs/map")),
            "not there yet is fine"
        );
        fs::create_dir_all(root.join("docs/map")).unwrap();
        assert!(map_dir_is_plain(&root, &root.join("docs/map")));
        fs::remove_dir_all(root.join("docs/map")).unwrap();
        assert!(dir_link(&root.join("docs").join("map"), &root.join("real")));
        assert!(!map_dir_is_plain(&root, &root.join("docs/map")));
        // The link may be higher up, too.
        fs::remove_dir_all(root.join("docs")).unwrap();
        assert!(dir_link(&root.join("docs"), &root.join("real")));
        assert!(!map_dir_is_plain(&root, &root.join("docs/map")));
    }

    #[test]
    fn a_map_directory_outside_the_project_is_the_users_own_choice() {
        let outer = fresh_dir("docmap-outside-map");
        let project = outer.join("proj");
        fs::create_dir_all(project.join("docs")).unwrap();
        fs::create_dir_all(outer.join("maps")).unwrap();
        // Built the way the Map directory field stores it: root and map
        // directory as `portable` strings with the `..` kept, not as joined
        // `Path`s (joining onto a `\\?\` path drops the `..` and the case
        // below would then never reach the code that handles it).
        let root_str = portable(&project);
        let root = PathBuf::from(&root_str);
        let dir = |tail: &str| PathBuf::from(format!("{root_str}/{tail}"));
        assert!(map_dir_is_plain(&root, &dir("../maps")));
        assert!(map_dir_is_plain(&root, &dir("../elsewhere/maps")));
        // Even one that is a link: that link is the user's own.
        assert!(dir_link(&outer.join("linked"), &outer.join("maps")));
        assert!(map_dir_is_plain(&root, &dir("../linked")));

        // Back into the project by way of `..` it is the project's directory
        // again, and a link there is repository content.
        assert!(dir_link(
            &project.join("docs").join("map"),
            &outer.join("maps")
        ));
        assert!(!map_dir_is_plain(&root, &dir("../proj/docs/map")));
        // A component that is not there does not end the walk: a later `..`
        // leads back to one that is.
        assert!(!map_dir_is_plain(&root, &dir("dist/../docs/map")));
        assert!(!map_dir_is_plain(&root, &dir("docs/real/../map")));
    }

    #[cfg(windows)]
    #[test]
    fn a_map_directory_spelled_so_win32_reads_it_differently_is_refused() {
        let root = fresh_dir("docmap-spelling");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("real")).unwrap();
        assert!(dir_link(&root.join("docs").join("map"), &root.join("real")));
        let root_str = portable(&root);
        let root = PathBuf::from(&root_str);
        let dir = |tail: &str| PathBuf::from(format!("{root_str}/{tail}"));
        assert!(!map_dir_is_plain(&root, &dir("docs/map")), "the control");
        // Win32 drops the trailing dot before the file system sees the name.
        assert!(!map_dir_is_plain(&root, &dir("docs/map.")));
        // A drive-like component is not a name.
        assert!(!map_dir_is_plain(&root, &dir("docs/C:x")));
    }

    #[test]
    fn a_map_file_is_read_where_it_is_unless_it_is_a_link_that_leaves() {
        let root = fresh_dir("docmap-map-file");
        let map = root.join("docs").join("map");
        fs::create_dir_all(&map).unwrap();
        fs::write(map.join("module_map.json"), "{}").unwrap();
        assert_eq!(
            map_file(&root, &map, "module_map.json").unwrap(),
            map.join("module_map.json")
        );
        assert!(map_file(&root, &map, "index.html").is_none(), "not there");

        // The directory a junction makes is not plain, and so nothing in it.
        let linked = root.join("docs2");
        fs::create_dir_all(&linked).unwrap();
        assert!(dir_link(&linked.join("map"), &map));
        assert!(map_file(&root, &linked.join("map"), "module_map.json").is_none());

        // A link to a file elsewhere in the project still works; one that
        // leaves the project does not.
        let outside = root.with_file_name("docmap-map-file-outside.json");
        fs::write(&outside, "{}").unwrap();
        fs::write(root.join("inside.json"), "{}").unwrap();
        if !file_link(&map.join("in.json"), &root.join("inside.json")) {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        assert!(file_link(&map.join("out.json"), &outside));
        assert_eq!(
            map_file(&root, &map, "in.json").unwrap(),
            root.join("inside.json")
        );
        assert!(map_file(&root, &map, "out.json").is_none());
    }

    #[test]
    fn a_map_directory_outside_the_project_is_read_as_it_is() {
        let outer = fresh_dir("docmap-map-file-outside");
        let project = outer.join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(outer.join("maps")).unwrap();
        fs::write(outer.join("maps").join("module_map.json"), "{}").unwrap();
        let root_str = portable(&project);
        let root = PathBuf::from(&root_str);
        let map = PathBuf::from(format!("{root_str}/../maps"));
        assert_eq!(
            map_file(&root, &map, "module_map.json").unwrap(),
            map.join("module_map.json")
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_map_file_that_links_to_another_machine_is_not_followed() {
        // Each name its own host, and a new one every run: Windows remembers a
        // host that did not answer, so a second look at the same one returns at
        // once and proves nothing.
        let root = fresh_dir("docmap-map-file-unc");
        let map = root.join("docs").join("map");
        fs::create_dir_all(&map).unwrap();
        let names = [
            ("module_map.json", crate::testutil::unc_host()),
            ("index.html", crate::testutil::unc_host()),
            ("leak", crate::testutil::unc_host()),
        ];
        for (name, host) in &names {
            let target = format!(r"\\{host}\share\{name}");
            if !file_link(&map.join(name), Path::new(&target)) {
                eprintln!("SKIP: no privilege to create symlinks");
                return;
            }
        }
        let started = std::time::Instant::now();
        for (name, host) in &names {
            assert!(map_file(&root, &map, name).is_none(), "{name} at {host}");
        }
        assert!(
            started.elapsed().as_secs() < 3,
            "it must not wait for a host"
        );
    }

    #[test]
    fn a_path_of_thousands_of_components_is_answered_at_once() {
        let dir = fresh_dir("docmap-long-rel");
        let started = std::time::Instant::now();
        // Too many components, and too many bytes: both refused before any
        // component is looked at (each prefix of an existing path costs a walk
        // of the whole prefix, and a manifest `src` can be a megabyte).
        for rel in [
            "a/".repeat(MAX_REL_COMPONENTS + 1),
            "a/".repeat(15_000),
            "a/".repeat(MAX_REL_LEN),
        ] {
            let err = resolve_inside(&dir, &rel).unwrap_err();
            assert!(err.contains("too long"), "{} bytes: {err}", rel.len());
        }
        // At the limit it is an ordinary missing path.
        let err = resolve_inside(&dir, &"a/".repeat(MAX_REL_COMPONENTS)).unwrap_err();
        assert!(err.contains("not in this project"), "{err}");
        // Many components that cancel out are many cheap steps, not depth.
        assert_eq!(resolve_inside(&dir, &"a/../".repeat(6_000)).unwrap(), dir);
        assert!(
            started.elapsed().as_millis() < 500,
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_map_directory_that_comes_back_into_the_project_is_still_the_projects() {
        // `../<project>/docs/map`: the `..` climbs out and the name comes
        // back, which is the project's own directory - and a link in it is
        // repository content like in any other spelling.
        let outer = fresh_dir("docmap-reenter");
        let project = outer.join("proj");
        let map = project.join("docs").join("map");
        fs::create_dir_all(&map).unwrap();
        let outside = outer.join("secret.json");
        fs::write(&outside, "{}").unwrap();
        fs::write(map.join("module_map.json"), "{}").unwrap();
        let root_str = portable(&project);
        let root = PathBuf::from(&root_str);
        let reentry = PathBuf::from(format!("{root_str}/../proj/docs/map"));
        let plain = PathBuf::from(format!("{root_str}/docs/map"));
        assert!(map_file(&root, &reentry, "module_map.json").is_some());
        fs::remove_file(map.join("module_map.json")).unwrap();
        if !file_link(&map.join("module_map.json"), &outside) {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        assert!(map_file(&root, &plain, "module_map.json").is_none());
        assert!(map_file(&root, &reentry, "module_map.json").is_none());
    }

    #[test]
    fn a_linked_output_file_that_leaves_the_project_is_refused_too() {
        // The names are spelled out here and not taken from the list under
        // test: dropping one from it (or never adding one the engine starts to
        // write) has to fail this.
        assert_eq!(
            OUTPUT_FILES,
            [
                "index.html",
                "module_map.json",
                "overview.md",
                "coverage.svg"
            ]
        );
        let root = fresh_dir("docmap-refuse-file");
        let map = root.join("docs").join("map");
        fs::create_dir_all(&map).unwrap();
        fs::write(root.join("inside.json"), "{}").unwrap();
        let outside = root.with_file_name("docmap-refuse-file-outside.json");
        fs::write(&outside, "{}").unwrap();
        let root_str = portable(&root);
        let flags = ProjectFlags::default();
        assert!(refuse_linked_output(&root_str, &flags).is_ok());
        if !file_link(&map.join("probe.json"), &root.join("inside.json")) {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        fs::remove_file(map.join("probe.json")).unwrap();
        for name in [
            "index.html",
            "module_map.json",
            "overview.md",
            "coverage.svg",
        ] {
            // A link that stays inside the project is fine ...
            assert!(file_link(&map.join(name), &root.join("inside.json")));
            assert!(
                refuse_linked_output(&root_str, &flags).is_ok(),
                "{name}: a link that stays inside the project is fine"
            );
            fs::remove_file(map.join(name)).unwrap();
            // ... one that leaves it is not ...
            assert!(file_link(&map.join(name), &outside));
            let err = refuse_linked_output(&root_str, &flags).unwrap_err();
            assert!(err.contains(&format!("docs/map/{name} is a link")), "{err}");
            fs::remove_file(map.join(name)).unwrap();
            // ... and neither is one whose target is not there, which the
            // engine would create wherever the link says.
            assert!(file_link(&map.join(name), &root.join("not-there.md")));
            let err = refuse_linked_output(&root_str, &flags).unwrap_err();
            assert!(err.contains(&format!("docs/map/{name} is a link")), "{err}");
            fs::remove_file(map.join(name)).unwrap();
        }
        assert!(refuse_linked_output(&root_str, &flags).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn the_work_of_the_link_walk_is_bounded_whatever_a_link_points_at() {
        // The caps on `rel` do not reach a link target: a link can name
        // thousands of real levels, and each look at one costs the kernel the
        // walk of its whole prefix.
        let dir = fresh_dir("docmap-deep-target");
        let deep = "a/".repeat(120);
        fs::create_dir_all(dir.join(&deep)).unwrap();
        fs::write(dir.join(&deep).join("f.png"), "x").unwrap();
        let target = format!("{}f.png", deep.replace('/', "\\"));
        if std::os::windows::fs::symlink_file(&target, dir.join("l")).is_err() {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        // A budget of the real size is spent by a tree of 1 100 levels (4 s to
        // build); a small one shows the same with 120.
        let mut steps = 100;
        let err = resolve_inside_budgeted(&dir, "l", &mut steps).unwrap_err();
        assert!(err.contains("too many links"), "{err}");
        assert_eq!(steps, 0);
        // With enough it is an ordinary path.
        let mut steps = 200;
        assert_eq!(
            resolve_inside_budgeted(&dir, "l", &mut steps).unwrap(),
            dir.join(&deep).join("f.png")
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_chain_of_links_at_the_bottom_of_a_deep_tree_does_not_walk_the_tree_once_per_hop() {
        // 15 hops below 200 levels: walked from the top each time that is
        // 3 000 looks; continued from the link's directory it is about 230.
        let dir = fresh_dir("docmap-deep-chain");
        let deep = "a/".repeat(200);
        fs::create_dir_all(dir.join(&deep)).unwrap();
        fs::write(dir.join(&deep).join("f.png"), "x").unwrap();
        let bottom = dir.join(&deep);
        let mut made = true;
        for i in 1..=15 {
            let to = if i == 15 {
                "f.png".to_string()
            } else {
                format!("x{}", i + 1)
            };
            made &= std::os::windows::fs::symlink_file(&to, bottom.join(format!("x{i}"))).is_ok();
        }
        if !made {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        let resolved = resolve_inside(&dir, &format!("{deep}x1")).unwrap();
        assert_eq!(resolved, bottom.join("f.png"));
    }

    #[cfg(windows)]
    #[test]
    fn a_link_target_that_climbs_with_dot_dot_is_still_looked_at() {
        // `join` onto a verbatim path folds a `..` away, so `a\l -> ..\x`
        // continues as `root\x`: no longer than `root\a`, and `x` - a link to a
        // share - sat inside a prefix counted as already looked at.
        let dir = fresh_dir("docmap-dotdot-target");
        fs::create_dir_all(dir.join("a").join("b")).unwrap();
        fs::create_dir_all(dir.join("public")).unwrap();
        let share = format!(r"\\{}\share", crate::testutil::unc_host());
        if std::os::windows::fs::symlink_dir(&share, dir.join("x")).is_err() {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        for (link, target) in [
            ("a/l", r"..\x"),
            ("a/b/l", r"..\..\x"),
            ("public/favicon.ico", r"..\x"),
            ("a/m", r".."),
        ] {
            assert!(std::os::windows::fs::symlink_dir(target, dir.join(link)).is_ok());
        }
        let started = std::time::Instant::now();
        // `a/m/x` goes through a link to the parent and then a name.
        for rel in ["a/l", "a/b/l", "public/favicon.ico", "a/m/x"] {
            let err = resolve_inside(&dir, rel).unwrap_err();
            assert!(err.contains("link to another machine"), "{rel}: {err}");
        }
        assert!(
            started.elapsed().as_secs() < 3,
            "it must not wait for a host"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_link_whose_target_names_a_rewritten_name_is_not_followed() {
        // `a/evil.` is a real directory (made through the verbatim root), `a/evil`
        // beside it a link to a share, and `a/l -> evil.\f.txt` names the first.
        // The walk looked at `evil.` literally; whoever is handed the result
        // without its prefix opens `evil`.
        let dir = fresh_dir("docmap-rewritten-target");
        fs::create_dir_all(dir.join("a").join("evil.")).unwrap();
        fs::write(dir.join("a").join("evil.").join("f.txt"), "x").unwrap();
        let share = format!(r"\\{}\share", crate::testutil::unc_host());
        if std::os::windows::fs::symlink_dir(&share, dir.join("a").join("evil")).is_err()
            || std::os::windows::fs::symlink_file(r"evil.\f.txt", dir.join("a").join("l")).is_err()
        {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        let started = std::time::Instant::now();
        let err = resolve_inside(&dir, "a/l").unwrap_err();
        assert!(err.contains("cannot be followed safely"), "{err}");
        assert!(
            started.elapsed().as_secs() < 3,
            "it must not wait for a host"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_folder_picker_does_not_follow_a_link_that_names_a_rewritten_name() {
        // The picked folder is typed, so Win32 would have read `evil.` as
        // `evil` while the kernel follows the link's target literally.
        let root = fresh_dir("docmap-picker-rewritten");
        fs::create_dir_all(root.join("loc.")).unwrap();
        fs::create_dir_all(root.join("plain")).unwrap();
        let share = format!(r"\\{}\share", crate::testutil::unc_host());
        let share2 = format!(r"\\{}\share", crate::testutil::unc_host());
        // `NUL` is a device to Win32 whatever directory it is in: a link of
        // that name is only a link to a path that is looked at literally.
        if std::os::windows::fs::symlink_dir(&share, root.join("evil.")).is_err()
            || std::os::windows::fs::symlink_dir("evil.", root.join("sub")).is_err()
            || std::os::windows::fs::symlink_dir("loc.", root.join("sub2")).is_err()
            || std::os::windows::fs::symlink_dir(&share2, root.join("NUL")).is_err()
            || std::os::windows::fs::symlink_dir("NUL", root.join("sub3")).is_err()
        {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        let stored = PathBuf::from(portable(&root));
        let started = std::time::Instant::now();
        let found = list_subdirs(&stored).unwrap();
        let names: Vec<_> = found.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(
            names,
            ["plain"],
            "neither link, nor the folders of those names"
        );
        assert!(
            started.elapsed().as_secs() < 3,
            "it must not wait for a host"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_link_target_with_a_root_and_no_drive_is_not_followed() {
        // `\Device\Mup\<host>\...` is how the kernel's own redirector is
        // spelled; written raw into the reparse data (an archive or image
        // restore can) it is not relative to anything. Through the Win32 API
        // it is stored relative to the link's drive, which is the same shape
        // as far as `read_link` can tell: rooted, no prefix.
        let dir = fresh_dir("docmap-rooted-target");
        let target = format!(r"\Device\Mup\{}\share\x", crate::testutil::unc_host());
        if std::os::windows::fs::symlink_file(&target, dir.join("g")).is_err() {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        let err = resolve_inside(&dir, "g").unwrap_err();
        assert!(err.contains("cannot be followed safely"), "{err}");
    }

    #[test]
    fn the_engine_is_not_run_on_a_project_whose_output_is_a_link() {
        let root = fresh_dir("docmap-refuse");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("real")).unwrap();
        let root_str = portable(&root);
        let flags = ProjectFlags::default();
        assert!(
            refuse_linked_output(&root_str, &flags).is_ok(),
            "no map yet is fine"
        );
        assert!(dir_link(&root.join("docs").join("map"), &root.join("real")));
        let err = refuse_linked_output(&root_str, &flags).unwrap_err();
        assert!(err.contains("docs/map is a link"), "{err}");
        // The project's own choice of directory is the one that is checked.
        let own = ProjectFlags {
            out_dir: Some("out".into()),
            ..Default::default()
        };
        assert!(refuse_linked_output(&root_str, &own).is_ok());
    }

    #[test]
    fn a_stored_map_directory_the_engine_refuses_is_reported_instead_of_run() {
        let root = fresh_dir("docmap-refuse-stored");
        let root_str = portable(&root);
        let stored = |dir: &str| ProjectFlags {
            out_dir: Some(dir.into()),
            ..Default::default()
        };
        // Saved before the dialog refused it, or put into `workspace.json` by
        // hand: passed on to the engine as it is, so it is turned back here.
        // A blank one is not the default either (`--out-dir=  ` is passed).
        for dir in [
            "../maps",
            "/maps",
            "C:/maps",
            "docs/my map",
            "docs/./map",
            "./out",
            "  ",
        ] {
            let err = refuse_linked_output(&root_str, &stored(dir)).unwrap_err();
            assert_eq!(err.key(), "err.outDir.stored", "{dir}: {err}");
            assert_eq!(err.param("value"), Some(dir), "{dir}: {err}");
            assert!(err.contains(&format!("`{dir}`")), "{dir}: {err}");
            assert!(err.english().ends_with("Change it there"), "{dir}: {err}");
        }
        for dir in ["out", "docs/map", "docs//map", ""] {
            assert!(
                refuse_linked_output(&root_str, &stored(dir)).is_ok(),
                "{dir}"
            );
        }
    }

    #[test]
    fn a_file_is_replaced_whole_and_leaves_no_temporary_behind() {
        use std::io::Read;
        let dir = fresh_dir("docmap-atomic");
        let path = dir.join("workspace.json");
        write_atomic(&path, "one-one-one").unwrap();
        // A reader that has the file open when it is replaced keeps the old
        // file whole: the replacement is a new file swapped in, not the old one
        // truncated and written again (which is what `fs::write` does, and what
        // lets a concurrent read see half a list).
        let mut held = fs::File::open(&path).unwrap();
        write_atomic(&path, "two").unwrap();
        let mut old = String::new();
        held.read_to_string(&mut old).unwrap();
        assert_eq!(old, "one-one-one");
        drop(held);
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, ["workspace.json"]);
    }

    #[cfg(windows)]
    #[test]
    fn a_folder_listing_does_not_follow_a_link_to_another_machine() {
        let root = fresh_dir("docmap-subdirs-unc");
        fs::create_dir_all(root.join("real").join("sub")).unwrap();
        // A link as an entry, and a `.git` that is a link both in the listed
        // folder and one level down: looked at, not followed. A host each, and
        // new ones every run, as above.
        let share = || format!(r"\\{}\share", crate::testutil::unc_host());
        let links = [
            root.join("vendor"),
            root.join("real").join(".git"),
            root.join("real").join("sub").join(".git"),
        ];
        for link in &links {
            if std::os::windows::fs::symlink_dir(share(), link).is_err() {
                eprintln!("SKIP: no privilege to create symlinks");
                return;
            }
        }
        let started = std::time::Instant::now();
        let found = list_subdirs(&root).unwrap();
        let names: Vec<_> = found.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(names, ["real"], "the link to a share is not a folder");
        assert!(found[0].2, "real/ has a .git entry, a link or not");
        // One level down the walks ask the same question of every directory.
        assert!(crate::languages::is_nested_checkout(
            &root.join("real").join("sub")
        ));
        assert!(
            started.elapsed().as_secs() < 3,
            "it must not wait for a host"
        );
    }

    #[test]
    fn a_git_entry_counts_whatever_it_is_and_is_never_followed() {
        // No host and no privilege needed: a link whose target is gone is
        // enough to tell a look from a follow (`exists()` says no).
        let root = fresh_dir("docmap-git-entry");
        let gone = root.join("gone");
        fs::create_dir_all(&gone).unwrap();
        fs::create_dir_all(root.join("dangling")).unwrap();
        fs::create_dir_all(root.join("dir").join(".git")).unwrap();
        fs::create_dir_all(root.join("file")).unwrap();
        fs::write(root.join("file").join(".git"), "gitdir: ../x").unwrap();
        fs::create_dir_all(root.join("none")).unwrap();
        assert!(dir_link(&root.join("dangling").join(".git"), &gone));
        fs::remove_dir_all(&gone).unwrap();

        let found = list_subdirs(&root).unwrap();
        let seen: Vec<_> = found.iter().map(|f| (f.0.as_str(), f.2)).collect();
        assert_eq!(
            seen,
            [
                ("dangling", true),
                ("dir", true),
                ("file", true),
                ("none", false)
            ]
        );
        assert!(crate::languages::is_nested_checkout(&root.join("dangling")));
        assert!(!crate::languages::is_nested_checkout(&root.join("none")));
    }

    #[cfg(windows)]
    #[test]
    fn a_name_win32_rewrites_is_neither_listed_nor_entered() {
        // `evil.` is a real directory (made through the verbatim root, which
        // keeps its dot) beside a junction `evil`. Opened by a path without
        // the verbatim prefix - which is what a stored project root is -
        // Win32 reads `evil.` as `evil`, and everything done to the entry
        // goes through the junction.
        let root = fresh_dir("docmap-rewritten-name");
        let elsewhere = fresh_dir("docmap-rewritten-name-target");
        fs::create_dir_all(root.join("evil.")).unwrap();
        fs::create_dir_all(root.join("plain")).unwrap();
        assert!(dir_link(&root.join("evil"), &elsewhere));
        let stored = PathBuf::from(portable(&root));

        let found = list_subdirs(&stored).unwrap();
        let names: Vec<_> = found.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(
            names,
            ["evil", "plain"],
            "only the junction by its own name"
        );
        assert!(crate::languages::is_nested_checkout(&stored.join("evil.")));
        assert!(!crate::languages::is_nested_checkout(&stored.join("plain")));
    }

    #[test]
    fn a_link_to_a_folder_on_this_machine_is_still_listed() {
        // A folder of plugins with a junction to another drive is still a
        // folder of plugins.
        let root = fresh_dir("docmap-subdirs-local");
        let elsewhere = fresh_dir("docmap-subdirs-local-target");
        fs::create_dir_all(elsewhere.join(".git")).unwrap();
        fs::create_dir_all(root.join("plain")).unwrap();
        assert!(dir_link(&root.join("linked"), &elsewhere));
        let found = list_subdirs(&root).unwrap();
        let names: Vec<_> = found.iter().map(|f| (f.0.as_str(), f.2)).collect();
        assert_eq!(names, [("linked", true), ("plain", false)]);
    }

    #[cfg(windows)]
    #[test]
    fn a_component_that_would_replace_the_path_is_refused_on_its_shape() {
        // `x/C:f.txt` used to reach `push("C:f.txt")`, which replaces the whole
        // buffer, and the link walk then looked at nothing.
        let dir = fresh_dir("docmap-colon");
        let shape = |rel: &str| resolve_inside(&dir, rel).unwrap_err();
        for rel in [
            "x/C:f.txt",
            "./C:f.txt",
            "a/../C:f.txt",
            "docs/c:",
            "a/C:..",
            "a.txt:stream",
            // Win32 reads these as `evil`, `x` and `b`.
            "evil./icon.png",
            "x./",
            "a/b /c",
            "icon.png.",
        ] {
            assert!(
                shape(rel).contains("not a path inside the project"),
                "{rel}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_link_to_the_kernels_unc_redirector_is_refused_whatever_it_is_spelled() {
        // `read_link` returns these as verbatim paths, so they are not UNC or
        // device-namespace prefixes to Rust: they have to be refused for not
        // being a drive.
        let dir = fresh_dir("docmap-nt-namespace");
        let targets = [
            r"\\?\GLOBALROOT\Device\Mup",
            r"\\.\GLOBALROOT\Device\Mup",
            r"\\?\Global\UNC\h.invalid\share",
            r"\\.\pipe\x",
        ];
        for (i, target) in targets.iter().enumerate() {
            if std::os::windows::fs::symlink_dir(target, dir.join(format!("g{i}"))).is_err() {
                eprintln!("SKIP: no privilege to create symlinks");
                return;
            }
        }
        let started = std::time::Instant::now();
        for (i, target) in targets.iter().enumerate() {
            let err = resolve_inside(&dir, &format!("g{i}/x")).unwrap_err();
            assert!(err.contains("link to another machine"), "{target}: {err}");
        }
        assert!(
            started.elapsed().as_secs() < 3,
            "it must not wait for a host"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_link_loop_and_a_local_link_are_told_apart_from_a_remote_one() {
        let dir = fresh_dir("docmap-loop");
        fs::create_dir_all(dir.join("real")).unwrap();
        fs::write(dir.join("real/f.txt"), "x").unwrap();
        // A junction to a directory inside the project resolves.
        if !dir_link(&dir.join("d"), &dir.join("real")) {
            eprintln!("SKIP: could not create a directory link");
            return;
        }
        assert_eq!(
            resolve_inside(&dir, "d/f.txt").unwrap(),
            dir.join("real/f.txt")
        );
        // A loop is refused, and not blamed on another machine.
        if std::os::windows::fs::symlink_dir("b", dir.join("a")).is_err()
            || std::os::windows::fs::symlink_dir("a", dir.join("b")).is_err()
        {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        let err = resolve_inside(&dir, "a/x").unwrap_err();
        assert!(err.contains("too many links"), "{err}");
        assert!(!err.contains("another machine"), "{err}");
    }

    #[cfg(windows)]
    #[test]
    fn a_link_that_leads_through_another_link_to_another_machine_is_refused() {
        // `l.png -> d\i.png` and `d -> \\host\share`: the second link is in the
        // *middle* of the first one's target, where a look at the last
        // component alone does not see it.
        let root = fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = root.join("docmap-network-chain");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dir = fs::canonicalize(&dir).unwrap();
        if std::os::windows::fs::symlink_dir(r"\\192.0.2.1\share", dir.join("d")).is_err() {
            return;
        }
        if std::os::windows::fs::symlink_file(r"d\i.png", dir.join("l.png")).is_err() {
            return;
        }
        let started = std::time::Instant::now();
        let err = resolve_inside(&dir, "l.png").unwrap_err();
        assert!(err.contains("link to another machine"), "{err}");
        assert!(
            started.elapsed().as_secs() < 3,
            "it must not wait for the host"
        );
    }

    #[test]
    fn dot_dot_is_collapsed_before_anything_is_opened() {
        let root = fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = root.join("docmap-collapse");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a")).unwrap();
        fs::write(dir.join("a/f.txt"), "x").unwrap();
        let dir = fs::canonicalize(&dir).unwrap();
        // A component that does not exist before a `..` is fine: it is gone
        // once collapsed.
        assert_eq!(
            resolve_inside(&dir, "nope/../a/f.txt").unwrap(),
            dir.join("a/f.txt")
        );
        assert_eq!(
            resolve_inside(&dir, "a/./f.txt").unwrap(),
            dir.join("a/f.txt")
        );
        // Leaving the root is refused by what it says, not by a later check.
        assert!(resolve_inside(&dir, "a/../..")
            .unwrap_err()
            .contains("outside"));
        assert!(resolve_inside(&dir, "../x")
            .unwrap_err()
            .contains("outside"));
    }

    #[cfg(unix)]
    #[test]
    fn an_executable_is_never_inert_whatever_it_is_called() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join("docmap-inert-exec");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run.md");
        fs::write(&path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!is_inert_document(&path));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn what_explorer_is_given_has_backslashes_and_no_verbatim_prefix() {
        assert_eq!(
            native_path(Path::new(r"\\?\C:\repos\x\a.lua")),
            r"C:\repos\x\a.lua"
        );
        assert_eq!(
            native_path(Path::new(r"\\?\UNC\host\share\a.lua")),
            r"\\host\share\a.lua"
        );
        assert_eq!(native_str("E:/repos/x"), r"E:\repos\x");
        assert!(!native_str("E:/repos/x").contains('/'));
    }

    // The one thing this environment cannot verify by eye: whether the
    // bundled sidecar and grammars actually resolve to real files once
    // built, the way a running app's own sidebar would show. `mock_app()`
    // gives a real `AppHandle` (built from this crate's own real
    // `tauri.conf.json`, not a fake one -- `generate_context!()` here reads
    // the same file `main()` does) with no window, which is enough to
    // exercise the exact resolution code the sidebar calls into.
    //
    // Requires the sidecar/grammars to actually be staged under
    // `target/debug/` before running -- true after any `cargo build`/
    // `cargo check` once `src-tauri/binaries/docmap-<target-triple>.exe`
    // and `src-tauri/resources/grammars/` exist, which `HANDOVER.md`
    // #7 explains how to populate. Not run in CI for that reason -- same
    // posture `docmap.nvim`'s own `check_treesitter.lua` states for
    // needing a real grammar on hand.
    // **Not on macOS, and not because the behaviour differs there.**
    // `generate_context!()` expands `embed_plist`, which defines the
    // symbol `_EMBED_INFO_PLIST`; `main()` already expands it once, and a
    // second expansion in the same test binary is a duplicate-symbol link
    // error by construction. Windows and Linux have no plist to embed and
    // link fine, which is why this only ever failed on one of the three.
    //
    // Gated rather than rewritten against `mock_context(noop_assets())`:
    // the whole point of this test is that it reads *this crate's real*
    // `tauri.conf.json`, and a fake context would leave it asserting
    // against a configuration nobody ships.
    fn args_of(cmd: &std::process::Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    // A vault with findings was shown as "Failed" with its map on disk: the
    // engine exits 1 after writing when the tree has error-severity drift
    // findings, unless it is told `--lenient`. A map is "generated" when the
    // engine wrote it, so a Generate has to ask for that exit code - and still
    // pass everything else. (That `generate` really builds its command line
    // here is `src/lib/wiring.test.js`'s job.)
    #[test]
    fn generate_asks_for_the_exit_code_that_means_the_map_was_written() {
        let plain = generate_command(
            "docmap",
            "E:/repos/example-vault",
            false,
            &ProjectFlags::default(),
        );
        assert_eq!(args_of(&plain), ["E:/repos/example-vault", "--lenient"]);

        let flags = ProjectFlags {
            exclude: vec!["vendor".into()],
            ..ProjectFlags::default()
        };
        let full = args_of(&generate_command("docmap", "/r", true, &flags));
        assert_eq!(full, ["/r", "--full", "--lenient", "--exclude=vendor"]);
    }

    // The per-project engine settings normalise on the way *in*, so that
    // `apply_flags` can pass what was stored without cleaning it a second
    // time. That split only holds if the cleaning is actually total, which
    // is what these assert: everything below is a spelling a person
    // genuinely types into the dialog.
    #[test]
    fn a_typed_path_is_normalised_to_what_the_engine_matches_on() {
        assert_eq!(
            rel_path(Some("  docs\\map  ".into())),
            Some("docs/map".into())
        );
        assert_eq!(
            rel_path(Some("./docs/map/".into())),
            Some("docs/map".into())
        );
        assert_eq!(rel_path(Some("/docs/map".into())), Some("docs/map".into()));
    }

    // Empty is "unset", never `Some("")`. The two would reach the engine as
    // "use your default" and as a literal `--out-dir=`, and only one of
    // those is something a person meant by clearing a field.
    #[test]
    fn an_emptied_field_is_unset_rather_than_empty() {
        assert_eq!(rel_path(Some("   ".into())), None);
        assert_eq!(rel_path(Some("./".into())), None);
        assert_eq!(rel_path(None), None);
        assert_eq!(text(Some("  ".into())), None);
        assert_eq!(text(None), None);
    }

    // A URL keeps the `//` after its scheme and loses only the trailing
    // slash — the engine concatenates a path onto this, and `.../repo//lua`
    // is the one artefact a reader actually clicks. The assertion exists to
    // stop somebody tidying `base_url` and `rel_path` into one helper: the
    // path one collapses every run of slashes, which would eat the scheme.
    #[test]
    fn a_repo_url_keeps_its_scheme_and_loses_its_trailing_slash() {
        assert_eq!(
            base_url(Some("  https://github.com/u/r/  ".into())),
            Some("https://github.com/u/r".into())
        );
        assert_eq!(base_url(Some("   ".into())), None);
        assert_eq!(base_url(Some("///".into())), None);
    }

    #[test]
    fn a_windows_extended_path_does_not_leak_its_prefix() {
        // Straight from a screenshot of the installed v0.1.0's About box:
        // `grammars: //?/C:/Program Files/docmap-desktop/grammars`. The
        // prefix survived `resource_dir()` and was then half-eaten by the
        // slash replacement that followed it.
        //
        // Cosmetic rather than functional -- the engine loads all four
        // grammars from the mangled path exactly as from the clean one,
        // checked against a deliberately wrong path to prove the probe
        // discriminates -- which is a reason to fix it in one place rather
        // than a reason to leave it in thirteen.
        let ugly = std::path::PathBuf::from(r"\\?\C:\Program Files\docmap-desktop\grammars");
        assert_eq!(portable(&ugly), "C:/Program Files/docmap-desktop/grammars");

        // An ordinary path is only slash-normalised, and a path that never
        // had the prefix must not lose leading characters to the trim.
        assert_eq!(
            portable(std::path::Path::new(r"C:\tools\docmap-grammars")),
            "C:/tools/docmap-grammars"
        );
        assert_eq!(
            portable(std::path::Path::new("/home/x/grammars")),
            "/home/x/grammars"
        );
    }

    #[test]
    fn a_workspace_name_cannot_escape_its_directory() {
        // A name is typed by a person and becomes a filename, which is the
        // shape of every directory-traversal bug ever written.
        assert_eq!(workspace_file_name("../../etc/passwd"), "______etc_passwd");
        assert_eq!(workspace_file_name("work/side"), "work_side");
        assert_eq!(workspace_file_name(r"C:\evil"), "C__evil");
    }

    #[test]
    fn a_name_that_reduces_to_nothing_falls_back() {
        // An empty filename would write to the directory itself.
        // `...` is *not* one of these: every dot has already become `_`,
        // so it is a safe if ugly name rather than a fallback case.
        assert_eq!(workspace_file_name("..."), "___");
        assert_eq!(workspace_file_name("   "), DEFAULT_WORKSPACE);
        assert_eq!(workspace_file_name(""), DEFAULT_WORKSPACE);
    }

    /// A scratch directory under the OS temp dir, cleared first so a
    /// previous run's leftovers cannot pass this one.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("docmap-subdirs-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_git_checkout_is_flagged_and_a_plain_folder_is_not() {
        let root = scratch("mixed");
        fs::create_dir_all(root.join("lib.nvim/.git")).unwrap();
        fs::create_dir_all(root.join("notes")).unwrap();

        let found = list_subdirs(&root).unwrap();
        assert_eq!(found.len(), 2);
        let lib = found.iter().find(|(n, ..)| n == "lib.nvim").unwrap();
        assert!(lib.2, "a .git entry makes it a repository");
        let notes = found.iter().find(|(n, ..)| n == "notes").unwrap();
        assert!(!notes.2, "no .git entry, no claim of being one");
    }

    #[test]
    fn hidden_directories_and_plain_files_are_left_out() {
        let root = scratch("hidden-and-files");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::create_dir_all(root.join("real")).unwrap();
        fs::write(root.join("README.md"), "not a directory").unwrap();

        let found = list_subdirs(&root).unwrap();
        assert_eq!(
            found.iter().map(|(n, ..)| n.as_str()).collect::<Vec<_>>(),
            vec!["real"]
        );
    }

    #[test]
    fn the_order_is_alphabetical_and_case_insensitive() {
        let root = scratch("order");
        for name in ["Zeta", "alpha", "Beta"] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        let found = list_subdirs(&root).unwrap();
        assert_eq!(
            found.iter().map(|(n, ..)| n.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "Beta", "Zeta"]
        );
    }

    // `add_one` is the core `import_many` batches inside a single
    // `with_workspace` instead of one lock-read-write per directory — these
    // exercise it directly, against a plain in-memory `Workspace`, the same
    // split from `AppHandle` that makes `list_subdirs` testable above.

    #[test]
    fn add_one_adds_a_new_directory_and_returns_it() {
        let root = scratch("add-one-new");
        let mut ws = Workspace::default();

        let added = add_one(&mut ws, &root.to_string_lossy()).unwrap();
        assert!(
            added.is_some(),
            "a directory not yet in the workspace is added"
        );
        assert_eq!(ws.projects.len(), 1);
        assert_eq!(ws.projects[0].id, added.unwrap().id);
    }

    #[test]
    fn add_one_is_idempotent_within_one_batch() {
        // The case `import_many` exists for: the same path offered twice in
        // one call -- once because it was genuinely picked twice, and once
        // because two names in a checklist canonicalise to the same
        // directory -- must not become two rows.
        let root = scratch("add-one-twice");
        let mut ws = Workspace::default();
        let path = root.to_string_lossy().to_string();

        let first = add_one(&mut ws, &path).unwrap();
        let second = add_one(&mut ws, &path).unwrap();

        assert!(first.is_some(), "the first call adds it");
        assert!(second.is_none(), "the second call reports nothing new");
        assert_eq!(ws.projects.len(), 1, "still exactly one project");
    }

    #[test]
    fn add_one_refuses_a_path_that_is_not_a_directory() {
        let root = scratch("add-one-not-a-dir");
        let file = root.join("plain.txt");
        fs::write(&file, "not a directory").unwrap();
        let mut ws = Workspace::default();

        let err = add_one(&mut ws, &file.to_string_lossy()).unwrap_err();
        assert_eq!(err.key(), "err.path.notDirectory");
        assert!(ws.projects.is_empty(), "a refused root adds nothing");
    }

    #[test]
    fn add_one_over_a_batch_matches_what_import_many_reports() {
        // The exact loop `import_many` runs, against a `Workspace` built in
        // memory instead of through `with_workspace` -- proving the batching
        // refactor preserves `import_from_nvim_config`'s per-entry isolation:
        // one bad root does not stop the rest, and repeats are counted, not
        // duplicated.
        let good_a = scratch("batch-a");
        let good_b = scratch("batch-b");
        let missing = scratch("batch-missing");
        fs::remove_dir_all(&missing).unwrap(); // exists() is false, not a directory

        let roots = vec![
            good_a.to_string_lossy().to_string(),
            good_b.to_string_lossy().to_string(),
            good_a.to_string_lossy().to_string(), // repeated on purpose
            missing.to_string_lossy().to_string(),
        ];

        let mut ws = Workspace::default();
        let mut added = 0;
        let mut already_present = 0;
        let mut errors = 0;
        for root in &roots {
            match add_one(&mut ws, root) {
                Ok(Some(_)) => added += 1,
                Ok(None) => already_present += 1,
                Err(_) => errors += 1,
            }
        }

        assert_eq!(added, 2, "the two distinct directories");
        assert_eq!(already_present, 1, "the repeat of the first");
        assert_eq!(errors, 1, "the directory that does not exist");
        assert_eq!(ws.projects.len(), 2, "no duplicate rows from the repeat");
    }

    #[test]
    fn ordinary_names_survive_intact() {
        // The sanitiser must not mangle what people will actually type,
        // or every workspace ends up called something with underscores in.
        assert_eq!(workspace_file_name("Work"), "Work");
        assert_eq!(workspace_file_name("nvim plugins"), "nvim plugins");
        assert_eq!(workspace_file_name("client-2026"), "client-2026");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn bundled_sidecar_and_grammars_resolve_to_real_files() {
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_shell::init())
            .build(tauri::generate_context!())
            .expect("failed to build mock app from this crate's real tauri.conf.json");
        let handle = app.handle();

        // `engine_sidecar` resolves correctly even run from here:
        // `tauri_plugin_shell`'s own path arithmetic special-cases
        // `cargo test`'s `target/debug/deps/` binaries, going up one level
        // to land back in `target/debug/` where the sidecar is actually
        // staged.
        let sidecar = engine_sidecar(handle);
        assert!(
            sidecar.is_some(),
            "expected src-tauri/binaries/docmap-<this platform's target triple> to resolve \
             as a real sidecar file -- see README.md, 'The engine'"
        );
        let program = sidecar.unwrap().get_program().to_os_string();
        assert!(
            Path::new(&program).is_file(),
            "resolved sidecar path {program:?} is not a real file"
        );

        // `resolve_grammars` goes through `app.path().resource_dir()`
        // instead, which has no equivalent `deps/`-awareness -- measured
        // directly (`docmap-desktop.exe --debug-resolve-and-exit`, a
        // one-off probe, not kept): the real compiled app resolves both
        // correctly from `target/debug/`, and only a `cargo test` binary
        // itself (running from `target/debug/deps/`) sees the offset. So
        // this recomputes the same "if I'm in `deps/`, my sibling files
        // are one level up" adjustment tauri_plugin_shell already applies
        // for the sidecar, purely to give this assertion the same real
        // exe-relative directory a genuine run would use -- not a second,
        // separately-trusted implementation of `resolve_grammars` itself.
        let exe_dir = std::env::current_exe()
            .expect("current_exe")
            .parent()
            .unwrap()
            .to_path_buf();
        let real_app_dir = if exe_dir.ends_with("deps") {
            exe_dir.parent().unwrap().to_path_buf()
        } else {
            exe_dir
        };
        let dir = real_app_dir.join("grammars");
        assert!(
            dir.is_dir(),
            "expected {dir:?} (src-tauri/resources/grammars/, staged next to the built exe) \
             to exist -- see README.md, 'The engine'"
        );
        let has_a_grammar = fs::read_dir(&dir)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        assert!(has_a_grammar, "{dir:?} exists but has nothing in it");

        // And that `resolve_grammars` agrees once pointed at a `configured`
        // override -- the code path an actual `set_grammars` call takes,
        // fully exercisable without the `deps/` detour at all.
        let via_configured = resolve_grammars(handle, Some(portable(&dir)));
        assert!(
            via_configured.is_some(),
            "resolve_grammars did not accept a real, existing configured dir"
        );
    }
}
