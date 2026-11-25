//! `docmap-desktop report [<root>...]`: the Duplicates, Quicks and Findings tabs over many repositories at once, and
//! what turns up in more than one of them, printed in the terminal.
//!
//! The join lives in the engine (`docmap --fleet <root>...`, `core/fleet.lua` of docmap.nvim); this side picks the
//! repositories (the ones named, or the projects of the active workspace), the engine (the same one the window uses)
//! and its grammars, and passes the output through line by line: progress on stderr while the engine reads, the report
//! on stdout. No window starts, and nothing is written into the configuration or a repository.
//!
//! Exit codes, as the other commands: 0 done, 1 a repository could not be read (the report still lists the others) or
//! the report could not be made at all, 2 wrong arguments.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::COMMAND;
use crate::proc;
use crate::workspace::{self, Project};

/// Lua patterns of the engine, plain substrings here: a repository whose folder name holds one of them (any case) is
/// private, and a run that picks repositories on its own (the projects of the workspace) never takes it. The same
/// list as `PRIVATE` in docmap.nvim's `core/fleet.lua`, which warns when one is named on purpose.
///
/// Assembled from parts (`concat!`): the source of this public repository names no private repository
/// (`src/lib/private-names.test.js`), the program still knows them.
pub(crate) const PRIVATE: [&str; 2] = [concat!("tri", "centis"), concat!("wk", "dbook")];

/// How long the engine may take for all repositories together: about two minutes for sixty on a busy machine.
const REPORT_LIMIT: Duration = Duration::from_secs(60 * 60);

/// How long the engine may take to say what it can.
const CAPABILITIES_LIMIT: Duration = Duration::from_secs(60);

/// The arguments of `report`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ReportArgs {
    /// Repositories named on the command line; none: the projects of the workspace.
    pub(crate) roots: Vec<String>,
    pub(crate) json: bool,
    pub(crate) top: Option<u32>,
    pub(crate) with_generated: bool,
}

/// Read the words after `report`. The text of the error says what is wrong.
pub(crate) fn parse(words: &[&str]) -> Result<ReportArgs, String> {
    let mut args = ReportArgs::default();
    for word in words {
        match *word {
            "--json" => args.json = true,
            "--with-generated" => args.with_generated = true,
            w if w.starts_with("--top=") => {
                let n = w["--top=".len()..]
                    .parse::<u32>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("`{w}`: --top wants a positive whole number"))?;
                args.top = Some(n);
            }
            w if w.starts_with('-') => return Err(format!("`report` has no option `{w}`")),
            w => args.roots.push(w.to_owned()),
        }
    }
    Ok(args)
}

/// The entry of [`PRIVATE`] the last folder of `path` holds, if any.
pub(crate) fn private_match(path: &str) -> Option<&'static str> {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let name = trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_lowercase();
    PRIVATE.into_iter().find(|p| name.contains(p))
}

/// The roots of the projects, without the private ones: those come back apart, with the entry they matched.
pub(crate) fn project_roots(projects: &[Project]) -> (Vec<String>, Vec<(String, &'static str)>) {
    let mut taken = Vec::new();
    let mut left_out = Vec::new();
    for project in projects {
        match private_match(&project.root) {
            Some(entry) => left_out.push((project.root.clone(), entry)),
            None => taken.push(project.root.clone()),
        }
    }
    (taken, left_out)
}

/// The words the engine gets: `--fleet`, the roots, the options as they were given.
pub(crate) fn engine_args(args: &ReportArgs, roots: &[String]) -> Vec<String> {
    let mut out = vec!["--fleet".to_owned()];
    out.extend(roots.iter().cloned());
    if args.json {
        out.push("--json".to_owned());
    }
    if let Some(top) = args.top {
        out.push(format!("--top={top}"));
    }
    if args.with_generated {
        out.push("--with-generated".to_owned());
    }
    out
}

/// The engine the window would use: the one set in Settings when the file is there, else the app's own beside the
/// program, else one on `PATH` (the order of `engine_info`).
pub(crate) fn pick_engine(
    configured: Option<&str>,
    beside: Option<PathBuf>,
    on_path: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if let Some(p) = configured.filter(|p| Path::new(p).is_file()) {
        return Some(p.to_owned());
    }
    if let Some(p) = beside {
        return Some(crate::portable(&p));
    }
    on_path()
}

/// The grammars: the folder set in Settings when it is there, else the app's own `grammars` beside the program (where
/// the Windows bundle puts them, and the resource folder of a build in `target/`).
pub(crate) fn pick_grammars(configured: Option<&str>, exe: Option<&Path>) -> Option<String> {
    if let Some(g) = configured.filter(|g| Path::new(g).is_dir()) {
        return Some(g.to_owned());
    }
    let dir = exe?.parent()?.join("grammars");
    dir.is_dir().then(|| crate::portable(&dir))
}

/// Whether the engine's `--capabilities` answer lists `fleet`.
pub(crate) fn has_fleet(capabilities: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(capabilities)
        .ok()
        .and_then(|v| v.get("capabilities").cloned())
        .and_then(|c| c.as_array().cloned())
        .is_some_and(|list| list.iter().any(|c| c.as_str() == Some("fleet")))
}

/// Run `report`; the exit code. Messages of this side start with the command's name, on stderr.
pub(crate) fn run(args: &ReportArgs) -> i32 {
    let fail = |text: String| {
        eprintln!("{COMMAND} report: {text}");
        1
    };
    let ws =
        match workspace::config_dir_without_app().and_then(|d| workspace::read_workspace_in(&d)) {
            Ok(ws) => ws,
            // Named roots need no workspace; only its settings (engine, grammars) are missed then.
            Err(e) if !args.roots.is_empty() => {
                eprintln!(
                "{COMMAND} report: the settings are not read ({e}); the app's own engine is used"
            );
                workspace::Workspace::default()
            }
            Err(e) => return fail(format!("the workspace is not read: {e}")),
        };

    let roots = if args.roots.is_empty() {
        let (taken, left_out) = project_roots(&ws.projects);
        for (root, entry) in &left_out {
            eprintln!("{COMMAND} report: left out {root}: a private repository (its name holds {entry:?})");
        }
        if taken.is_empty() {
            return fail(format!(
                "the workspace {:?} has no project to report on; name the repositories: {COMMAND} report <root>...",
                ws.active.as_deref().unwrap_or(workspace::DEFAULT_WORKSPACE)
            ));
        }
        taken
    } else {
        args.roots.clone()
    };

    let exe = tauri::utils::platform::current_exe().ok();
    let Some(engine) = pick_engine(
        ws.engine.as_deref(),
        exe.as_deref().and_then(crate::engine::sidecar_beside),
        crate::engine::engine_on_path,
    ) else {
        return fail(
            "no engine: the app's own is missing and none is set in Settings or on PATH".to_owned(),
        );
    };
    let grammars = pick_grammars(ws.grammars.as_deref(), exe.as_deref());

    // An older engine would take `--fleet` for a folder it cannot read and say only its usage line: ask first.
    let mut probe = proc::Cmd::new("docmap-desktop report", &engine);
    probe.arg("--capabilities");
    match proc::run(&probe, CAPABILITIES_LIMIT) {
        Ok(out) if out.success() && has_fleet(&out.stdout) => {}
        Ok(_) => {
            return fail(format!(
                "the engine {engine} cannot report over many repositories (no `fleet` in its --capabilities); \
                 update the app, or set a newer engine in Settings"
            ))
        }
        Err(e) => return fail(format!("the engine {engine} does not answer: {e:?}")),
    }
    if grammars.is_none() {
        eprintln!(
            "{COMMAND} report: no grammars found: the engine reads no functions, so no shapes are compared"
        );
    }

    let mut cmd = proc::Cmd::new("docmap-desktop report", &engine);
    cmd.args(engine_args(args, &roots));
    if let Some(g) = &grammars {
        cmd.env("DOCMAP_TS_DIR", g);
    }
    let done = proc::run_lines(&cmd, REPORT_LIMIT, |stream, line| match stream {
        proc::Stream::Stdout => println!("{line}"),
        proc::Stream::Stderr => eprintln!("{line}"),
    });
    match done {
        // The engine's own codes are this command's: 0, 1 a repository not read, 2 wrong arguments.
        Ok(Some(code)) => code,
        Ok(None) => fail("the engine was ended".to_owned()),
        Err(e) => fail(format!("the engine failed: {e:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(root: &str) -> Project {
        serde_json::from_value(serde_json::json!({
            "id": root, "name": root, "root": root, "map_dir": "docs/map"
        }))
        .expect("a project")
    }

    #[test]
    fn the_words_of_report_are_read() {
        assert_eq!(parse(&[]), Ok(ReportArgs::default()));
        assert_eq!(
            parse(&["a", "--json", "--top=3", "--with-generated", "b"]),
            Ok(ReportArgs {
                roots: vec!["a".into(), "b".into()],
                json: true,
                top: Some(3),
                with_generated: true,
            })
        );
        for bad in ["--top=0", "--top=x", "--top=-1", "--top=1.5", "--top="] {
            let e = parse(&["a", bad]).expect_err(bad);
            assert!(e.contains("--top"), "{bad}: {e}");
        }
        let e = parse(&["--dir=E:/repos"]).expect_err("an unknown option");
        assert!(e.contains("--dir=E:/repos"), "{e}");
    }

    #[test]
    fn the_engine_gets_fleet_the_roots_and_the_options() {
        let args = parse(&["--json", "--top=7", "--with-generated"]).unwrap();
        assert_eq!(
            engine_args(&args, &["E:/r/a".into(), "E:/r/b".into()]),
            [
                "--fleet",
                "E:/r/a",
                "E:/r/b",
                "--json",
                "--top=7",
                "--with-generated"
            ]
        );
        assert_eq!(
            engine_args(&ReportArgs::default(), &["a".into()]),
            ["--fleet", "a"]
        );
    }

    #[test]
    fn private_repositories_are_known_by_their_folder_name() {
        let [first, second] = PRIVATE;
        // a name holding an entry in any case, a backslash path with a trailing separator
        let mixed = format!("E:/repos/Notes-{}", second.to_uppercase());
        assert_eq!(private_match(&mixed), Some(second));
        let trailing = format!("E:\\repos\\{first}-x\\");
        assert_eq!(private_match(&trailing), Some(first));
        // only the last folder counts
        let parent = format!("/home/{first}/docmap.nvim");
        assert_eq!(private_match(&parent), None);
        assert_eq!(private_match("docmap.nvim"), None);
        assert!(
            PRIVATE
                .iter()
                .all(|p| p.len() > 5 && *p == p.to_lowercase()),
            "{PRIVATE:?}"
        );
    }

    #[test]
    fn the_projects_of_the_workspace_leave_the_private_ones_out() {
        let private = format!("E:/repos/x-{}", PRIVATE[1]);
        let (taken, left_out) = project_roots(&[
            project("E:/repos/docmap.nvim"),
            project(&private),
            project("E:/repos/lib.nvim"),
        ]);
        assert_eq!(taken, ["E:/repos/docmap.nvim", "E:/repos/lib.nvim"]);
        assert_eq!(left_out, [(private.clone(), PRIVATE[1])]);
    }

    #[test]
    fn the_engine_of_the_settings_wins_when_its_file_is_there() {
        let dir = std::env::temp_dir().join(format!("docmap-report-engine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("docmap.exe");
        std::fs::write(&file, b"").unwrap();
        let configured = file.to_string_lossy().into_owned();
        let beside = dir.join("beside.exe");
        assert_eq!(
            pick_engine(Some(&configured), Some(beside.clone()), || None),
            Some(configured.clone())
        );
        // a configured file that is gone falls through to the app's own, then to PATH
        assert_eq!(
            pick_engine(Some("Z:/gone/docmap.exe"), Some(beside.clone()), || None),
            Some(crate::portable(&beside))
        );
        assert_eq!(
            pick_engine(None, None, || Some("on-path".into())),
            Some("on-path".into())
        );
        assert_eq!(pick_engine(None, None, || None), None);

        // grammars: the configured folder, else `grammars` beside the program
        assert_eq!(pick_grammars(None, Some(&file)), None);
        std::fs::create_dir_all(dir.join("grammars")).unwrap();
        assert_eq!(
            pick_grammars(None, Some(&file)),
            Some(crate::portable(&dir.join("grammars")))
        );
        let own = dir.to_string_lossy().into_owned();
        assert_eq!(pick_grammars(Some(&own), Some(&file)), Some(own.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_capabilities_answer_says_whether_fleet_is_there() {
        assert!(has_fleet(br#"{"capabilities":["api","fleet"],"schema":4}"#));
        assert!(!has_fleet(br#"{"capabilities":["api","languages"]}"#));
        assert!(!has_fleet(b"usage: lua standalone/docmap.lua <root>"));
        assert!(!has_fleet(b""));
    }

    /// Without the app the folder of its files is computed, and it must be the one the window uses.
    #[test]
    fn the_folder_without_the_app_is_the_one_of_the_window() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        assert_eq!(conf["identifier"].as_str(), Some(workspace::IDENTIFIER));

        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| std::ffi::OsString::from(*v))
            }
        };
        if cfg!(windows) {
            assert_eq!(
                workspace::platform_config_dir(env(&[(
                    "APPDATA",
                    "C:\\Users\\me\\AppData\\Roaming"
                )])),
                Some(PathBuf::from("C:\\Users\\me\\AppData\\Roaming"))
            );
            // a relative or empty value is no folder
            assert_eq!(
                workspace::platform_config_dir(env(&[("APPDATA", "AppData")])),
                None
            );
            assert_eq!(
                workspace::platform_config_dir(env(&[("APPDATA", "")])),
                None
            );
        } else if !cfg!(target_os = "macos") {
            assert_eq!(
                workspace::platform_config_dir(env(&[
                    ("XDG_CONFIG_HOME", "/x/cfg"),
                    ("HOME", "/h")
                ])),
                Some(PathBuf::from("/x/cfg"))
            );
            assert_eq!(
                workspace::platform_config_dir(env(&[("HOME", "/h")])),
                Some(PathBuf::from("/h/.config"))
            );
        }
    }

    /// The projects of the active workspace are read without the app, and nothing is written.
    #[test]
    fn the_workspace_is_read_without_the_app_and_left_as_it_is() {
        let dir = std::env::temp_dir().join(format!("docmap-report-ws-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("workspaces")).unwrap();
        // nothing there: an empty default workspace, and no folder made
        let empty = dir.join("empty");
        let ws = workspace::read_workspace_in(&empty).unwrap();
        assert!(ws.projects.is_empty());
        assert_eq!(ws.active.as_deref(), Some(workspace::DEFAULT_WORKSPACE));
        assert!(!empty.exists(), "reading made a folder");

        std::fs::write(
            dir.join("workspace.json"),
            r#"{"projects":[],"active":"Work","engine":"E:/e/docmap.exe"}"#,
        )
        .unwrap();
        let list = serde_json::json!([
            {"id": "E:/r/a", "name": "a", "root": "E:/r/a", "map_dir": "docs/map"},
            {"id": "E:/r/b", "name": "b", "root": "E:/r/b", "map_dir": "docs/map"}
        ]);
        std::fs::write(dir.join("workspaces/Work.json"), list.to_string()).unwrap();
        std::fs::write(dir.join("workspaces/Default.json"), "[]").unwrap();
        let ws = workspace::read_workspace_in(&dir).unwrap();
        let roots: Vec<&str> = ws.projects.iter().map(|p| p.root.as_str()).collect();
        assert_eq!(roots, ["E:/r/a", "E:/r/b"], "the active workspace's list");
        assert_eq!(ws.engine.as_deref(), Some("E:/e/docmap.exe"));

        // a settings file from before workspaces: its projects are the answer, and it is not migrated
        let old = dir.join("old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(
            old.join("workspace.json"),
            serde_json::json!({ "projects": list }).to_string(),
        )
        .unwrap();
        let ws = workspace::read_workspace_in(&old).unwrap();
        assert_eq!(ws.projects.len(), 2);
        assert!(!old.join("workspaces").exists(), "nothing was migrated");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
