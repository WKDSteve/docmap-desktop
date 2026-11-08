//! Neovim: where it is, the config folder, the import of projects from the config, the telemetry folder.

use super::*;

/// Look for `nvim` on `PATH`, the same way `engine_on_path` looks for the
/// docmap engine.
pub(crate) fn nvim_on_path() -> Option<String> {
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
pub(crate) fn default_nvim_config_dir() -> Option<String> {
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
pub(crate) struct NvimInfo {
    /// The configured `nvim` binary, or one found on PATH, or none.
    pub(crate) path: Option<String>,
    pub(crate) from_path: bool,
    /// The configured Neovim config directory, or the platform default if
    /// that default actually exists on disk, or none.
    pub(crate) config_dir: Option<String>,
    pub(crate) config_dir_from_default: bool,
}

#[tauri::command]
pub(crate) fn nvim_info(app: tauri::AppHandle) -> Result<NvimInfo, AppError> {
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
pub(crate) fn set_nvim_path(
    app: tauri::AppHandle,
    path: Option<String>,
) -> Result<NvimInfo, AppError> {
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
pub(crate) fn set_nvim_config_dir(
    app: tauri::AppHandle,
    path: Option<String>,
) -> Result<NvimInfo, AppError> {
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
pub(crate) struct NvimProjectEntry {
    pub(crate) name: String,
    #[allow(dead_code)]
    pub(crate) repo: String,
    pub(crate) dir: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ImportResult {
    /// How many entries the import tried: the config directory itself plus
    /// every entry the Neovim export returned, before dedup.
    pub(crate) found: usize,
    /// Newly added projects only — not ones that were already present.
    pub(crate) added: Vec<Project>,
    pub(crate) already_present: usize,
    /// Every entry that failed to add: its name and why.
    pub(crate) errors: Vec<ImportFailure>,
}

/// One entry of an import that could not be added. The error is an
/// [`AppError`] like any command's, so a window that lists these can say why
/// in its own language.
#[derive(Debug, Serialize)]
pub(crate) struct ImportFailure {
    pub(crate) name: String,
    pub(crate) error: AppError,
}

/// The Lua chunk `import_from_nvim_config` runs inside a fully started,
/// headless Neovim: the JSON of `plugins.personal.core.export.projects()`
/// (the nvim config's own interface, see its `lua/plugins/personal/README.md`)
/// on stdout, one line.
///
/// It used to be a file in the config, `scripts/docmap_projects.lua`; that file
/// was deleted in a cleanup there and the import failed on every machine with
/// a missing-script error the reader never connected to the tab. The chunk is
/// four lines, so it lives here, where nothing outside this repository can
/// remove it. `plugins.personal.export` is the module's older path and is
/// still tried. Exit 3 means neither module exists (a config that is not this
/// one), exit 1 that the module reported an error; both say why on stderr.
///
/// Single quotes only: the chunk is one argument of `-c`, and on Windows a
/// double quote inside it would have to survive two layers of quoting.
pub(crate) const NVIM_EXPORT_LUA: &str = "lua \
local ok, m = pcall(require, 'plugins.personal.core.export') \
if not ok then ok, m = pcall(require, 'plugins.personal.export') end \
if not ok then io.stderr:write('docmap-desktop: no plugins.personal.core.export in this config\n') os.exit(3) end \
local p, e = m.projects() \
if e then io.stderr:write('docmap-desktop: ' .. tostring(e) .. '\n') os.exit(1) end \
io.stdout:write(vim.json.encode(p) .. '\n') os.exit(0)";

/// The `nvim` command line of the import. `-c`, not `-l`: measured, not
/// assumed - `-l` is a bare Lua-script runner that skips `init.lua` and
/// lazy.nvim's bootstrap entirely, so it cannot see this config's real,
/// fully-resolved plugin policy. The `qa` after it never runs (the chunk
/// exits), it is there so a chunk that somehow returns does not leave a
/// headless Neovim waiting for input until the timeout.
pub(crate) fn nvim_export_command(nvim: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(nvim);
    cmd.args(["--headless", "-c", NVIM_EXPORT_LUA, "-c", "qa"]);
    cmd
}

/// The export's stdout as entries. The chunk writes the JSON as one line and
/// exits, so it is the last non-empty line; anything a plugin printed during
/// startup comes before it and is skipped. Not "from the first `[`": a banner
/// like `[lazy] 3 updates` starts with one too.
pub(crate) fn parse_nvim_export(stdout: &str) -> Result<Vec<NvimProjectEntry>, AppError> {
    let json = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    serde_json::from_str(json)
        .map_err(|e| AppError::new("err.nvim.notJson").detail(format!("{e}\n{stdout}")))
}

/// Add the config directory itself and then every entry, counting each
/// outcome. The config comes first: it is the project the tab is named after,
/// and reading "the Neovim config" and finding everything but the config in
/// the list was the second half of why the tab looked like it did nothing.
pub(crate) fn import_entries(
    ws: &mut Workspace,
    config_dir: Option<&str>,
    entries: &[NvimProjectEntry],
) -> ImportResult {
    let mut added = Vec::new();
    let mut already_present = 0usize;
    let mut errors = Vec::new();
    let config = config_dir.map(|dir| NvimProjectEntry {
        name: Path::new(dir)
            .file_name()
            .map_or_else(|| dir.to_string(), |n| n.to_string_lossy().to_string()),
        repo: String::new(),
        dir: dir.to_string(),
    });
    let all: Vec<&NvimProjectEntry> = config.iter().chain(entries.iter()).collect();
    for entry in &all {
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
    ImportResult {
        found: all.len(),
        added,
        already_present,
        errors,
    }
}

/// Import the Neovim config itself and every enabled, locally-checked-out
/// personal plugin it declares.
///
/// Delegates the plugin list entirely to the config's own
/// `plugins.personal.core.export.projects()` (see [`NVIM_EXPORT_LUA`]) - this
/// program never parses `source.lua`'s policy table itself, which would be
/// brittle in a way neither side should have to maintain.
///
/// `async` + `spawn_blocking`, same reasoning as `generate`: a real Neovim
/// startup takes real time, and a command that blocks freezes the window
/// it was invoked from.
#[tauri::command]
pub(crate) async fn import_from_nvim_config(
    app: tauri::AppHandle,
) -> Result<ImportResult, AppError> {
    let info = nvim_info(app.clone())?;
    let nvim_path = info.path.ok_or_else(|| AppError::new("err.nvim.none"))?;
    let config_dir = info
        .config_dir
        .ok_or_else(|| AppError::new("err.nvim.noConfig"))?;

    let out = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = nvim_export_command(&nvim_path);
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

    if out.status.code() == Some(3) {
        return Err(AppError::new("err.nvim.noExport")
            .with("dir", &config_dir)
            .detail(String::from_utf8_lossy(&out.stderr)));
    }
    if !out.status.success() {
        return Err(AppError::new("err.nvim.exit")
            .with("code", out.status.code().unwrap_or(-1))
            .detail(String::from_utf8_lossy(&out.stderr)));
    }

    let found = parse_nvim_export(&String::from_utf8_lossy(&out.stdout))?;
    with_workspace(&app, |ws| Ok(import_entries(ws, Some(&config_dir), &found)))
}

/// Neovim's own cache root, asked of Neovim.
///
/// Not guessed from the platform: `stdpath("cache")` answers differently
/// under `XDG_CACHE_HOME`, and a hardcoded guess would read an empty
/// directory and report "no telemetry" for a machine full of it. One
/// subprocess, cached for the window's lifetime — the answer cannot change
/// while Neovim's environment does not.
pub(crate) static CACHE_ROOT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

pub(crate) fn nvim_cache_root(app: &tauri::AppHandle) -> Option<String> {
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
pub(crate) fn telemetry_info(
    app: tauri::AppHandle,
    namespace: String,
) -> Result<telemetry::Info, AppError> {
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
pub(crate) async fn set_telemetry(
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

/// Read or set the editor command template.
#[tauri::command]
pub(crate) fn editor_command(
    app: tauri::AppHandle,
    set: Option<String>,
) -> Result<Option<String>, AppError> {
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
