//! The engine (`docmap.nvim`'s standalone binary): finding it, its languages and grammars, and generating, checking
//! and serving a map.

use super::*;

/// What the view needs to know before it tries to show anything.
#[derive(Debug, Serialize)]
pub(crate) struct MapStatus {
    pub(crate) exists: bool,
    pub(crate) index_path: String,
    /// Present only when the map exists — the counts the sidebar shows so a
    /// project is identifiable without opening it.
    pub(crate) modules: Option<u64>,
    pub(crate) files: Option<u64>,
    pub(crate) namespaces: Option<u64>,
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
    pub(crate) schema: Option<u64>,
}

/// Does this project have a generated map, and what is in it?
///
/// Reads `module_map.json` rather than parsing the HTML: the JSON is the
/// artifact with a stated contract (`meta.counts`), the HTML is a rendering
/// of it. Reading the rendering to recover the data it was rendered from is
/// the kind of shortcut that breaks the first time the page changes.
#[tauri::command(async)]
pub(crate) fn map_status(app: tauri::AppHandle, map_dir: String) -> MapStatus {
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
pub(crate) fn scan_languages(
    root: String,
    map_dir: Option<String>,
) -> Result<languages::LanguageScan, AppError> {
    languages::scan(Path::new(&root), map_dir.as_deref().map(Path::new))
}

/// Look for the engine on `PATH`, the way a shell would.
///
/// Written out rather than shelling to `where`/`which`: one fewer subprocess,
/// and the answer is a path this program then has to hold anyway.
pub(crate) fn engine_on_path() -> Option<String> {
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
/// `externalBin` in `tauri.{windows,linux}.conf.json` names the file the
/// bundle carries; it is not a promise the file exists at run time — a
/// target without `externalBin` (macOS) has nothing there. So this is
/// checked the same way `engine_on_path` checks PATH candidates: resolve,
/// then verify, never assume.
///
/// Returns the path, resolved by this program itself: the bundle puts the
/// sidecar next to the executable under its plain name (`docmap`, plus
/// `.exe` on Windows; the target triple of `binaries/docmap-<triple>` is
/// only the name of the source file), and so does `tauri-build` for a dev
/// build (`target/debug/`). No plugin is needed to find a neighbour of the
/// executable, and the page gets no permission to start anything: the
/// engine is only ever started by this backend. Every caller downstream of
/// `EngineInfo.path` treats the result like a configured or PATH-found one.
pub(crate) fn engine_sidecar() -> Option<PathBuf> {
    // `tauri::utils::platform::current_exe` refuses a symlinked executable
    // path that could point the lookup at someone else's directory.
    let exe = tauri::utils::platform::current_exe().ok()?;
    sidecar_beside(&exe)
}

/// The sidecar next to `exe`, if the file is there.
///
/// A `cargo test` binary runs from `target/<profile>/deps/`, one level below
/// the directory the sidecar is staged in; that one step up is the only
/// special case.
pub(crate) fn sidecar_beside(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let dir = if dir.ends_with("deps") {
        dir.parent()?
    } else {
        dir
    };
    let name = if cfg!(windows) {
        "docmap.exe"
    } else {
        "docmap"
    };
    let candidate = dir.join(name);
    candidate.is_file().then_some(candidate)
}

/// A grammars directory: configured, or the bundled resource directory if
/// this build shipped one. Mirrors `engine_sidecar`'s own "resolve, then
/// verify" rule — `resource_dir()` resolving successfully says nothing
/// about whether `grammars/` inside it actually has anything in it (a
/// build that bundled the engine without grammars is a real, supported
/// case: parser-less fidelity, exactly like an unconfigured `grammars`
/// today).
pub(crate) fn resolve_grammars<R: tauri::Runtime>(
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
pub(crate) struct GrammarDir {
    /// The directory the engine is given, or `None` when neither a setting
    /// nor a bundled directory resolved.
    pub(crate) dir: Option<String>,
    /// True when it came from Settings rather than from the bundle -- the
    /// difference between "fix your setting" and "this build ships none".
    pub(crate) from_setting: bool,
    /// False when `dir` is set but is not there any more. A configured path
    /// that has been deleted is a different problem from an empty one, and
    /// the fix is different too.
    pub(crate) exists: bool,
    /// Base names, sorted, capped. Enough to see the pattern and to spot a
    /// typo; not a file browser.
    pub(crate) files: Vec<String>,
    /// How many were left out by the cap, so a long list says it is long
    /// rather than quietly looking short.
    pub(crate) more: usize,
}

/// Names listed before [`GrammarDir::more`] takes over.
pub(crate) const GRAMMAR_FILES_SHOWN: usize = 12;

#[tauri::command]
pub(crate) fn grammar_dir(app: tauri::AppHandle) -> Result<GrammarDir, AppError> {
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
pub(crate) struct EngineInfo {
    /// The configured path, one found on PATH, the bundled sidecar's
    /// resolved path, or none.
    pub(crate) path: Option<String>,
    /// True when it came from PATH rather than from a setting — worth showing,
    /// because it explains why it might disappear on another machine.
    pub(crate) from_path: bool,
    /// True when `path` is the bundled sidecar rather than something found
    /// on this machine — the case that needs no setup at all, and the
    /// sidebar should say so rather than looking identical to a lucky PATH
    /// find that could vanish on the next machine.
    pub(crate) bundled: bool,
    pub(crate) grammars: Option<String>,
}

#[tauri::command]
pub(crate) fn engine_info(app: tauri::AppHandle) -> Result<EngineInfo, AppError> {
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
    if let Some(sidecar) = engine_sidecar() {
        let path = portable(&sidecar);
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
pub(crate) struct EngineLanguage {
    pub(crate) name: String,
    pub(crate) grammar: Option<String>,
    pub(crate) grammar_loaded: Option<bool>,
    /// Whether this backend produces call edges at all.
    ///
    /// `None` from an engine that predates the field, and that is a third
    /// answer rather than a defaulted `false`: "this build has no call
    /// extraction for this language" and "this build cannot say" lead to
    /// different sentences, and only the first is worth putting in front of
    /// a reader looking at an empty panel.
    #[serde(default)]
    pub(crate) calls: Option<bool>,
}

/// What the engine says it can read.
///
/// `languages: None` is not an error and not an empty list -- it means this
/// engine predates the field. The frontend has to say "unknown", because
/// "no languages" and "an engine that cannot be asked" lead to opposite
/// advice: the first is broken, the second works fine and just cannot
/// explain itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EngineLanguages {
    pub(crate) languages: Option<Vec<EngineLanguage>>,
    /// The artifact schema this engine writes. `None` from a build that
    /// predates the field, which is a different fact from any particular
    /// number and is why it is not defaulted to one.
    pub(crate) schema: Option<u64>,
    /// Which build it is: commit, commit date, and whether the tree it was
    /// built from was clean. `None` for an engine run from a source
    /// checkout, which stamps nothing rather than claiming a provenance.
    pub(crate) build: Option<EngineBuild>,
    /// The `/api/*` routes the engine answers. `None` from a build that
    /// predates the field -- not an empty list, which would claim an engine
    /// that answers nothing.
    pub(crate) routes: Option<Vec<String>>,
    /// The contract this build of the app was checked against
    /// (`engine.lock`). Sent with every answer, including "cannot be asked",
    /// so the window compares the two in one place (`src/lib/contract.js`).
    pub(crate) contract: Option<contract::Contract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EngineBuild {
    pub(crate) commit: Option<String>,
    pub(crate) committed_at: Option<String>,
    /// `None` when git could not answer at all — which is not the same as
    /// a clean tree, and the difference is what makes the commit
    /// trustworthy.
    pub(crate) dirty: Option<bool>,
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
pub(crate) fn engine_languages(app: tauri::AppHandle) -> Result<EngineLanguages, AppError> {
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
pub(crate) fn set_engine(
    app: tauri::AppHandle,
    path: Option<String>,
) -> Result<EngineInfo, AppError> {
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
pub(crate) fn set_grammars(
    app: tauri::AppHandle,
    path: Option<String>,
) -> Result<EngineInfo, AppError> {
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
pub(crate) fn generate_command(
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
pub(crate) struct GenerateResult {
    pub(crate) ok: bool,
    pub(crate) code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
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
pub(crate) async fn generate(
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
pub(crate) struct CheckResult {
    /// Whether regenerating would produce byte-identical files.
    pub(crate) current: bool,
    pub(crate) code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
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
pub(crate) async fn check_map(
    app: tauri::AppHandle,
    root: String,
) -> Result<CheckResult, AppError> {
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
pub(crate) fn serve_project(app: tauri::AppHandle, id: String) -> Result<String, AppError> {
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

    // The window falls back to the asset protocol when the server cannot start.
    // Only what `map_file` lets through: the scope canonicalises a path, which would follow a link to another machine.
    if let Some(index) = map_file(
        Path::new(&project.root),
        Path::new(&project.map_dir),
        "index.html",
    ) {
        allow_asset(&app, &index);
    }
    let port = server::start(server::ServeConfig {
        root: project.root.clone(),
        map_dir: project.map_dir.clone(),
        engine: info.path,
        grammars: info.grammars,
    })?;
    Ok(server::url(port))
}
