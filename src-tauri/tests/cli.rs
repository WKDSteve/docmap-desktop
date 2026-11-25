//! The command line of the program as a person (or the setup) runs it: arguments in, text and exit code out. No window
//! is ever opened here: only the words of the command line are run, never a path or `--open-at`. The folders of the
//! command are folders of the test (`APPKIT_FAMILY_DIR`) and `DOCMAP_DESKTOP_NO_LOGIN_SYNC` keeps the `PATH` of the
//! machine as it is.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The tests run one at a time: `status` looks at every process of this program file, and the others start it.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

const PROGRAM: &str = env!("CARGO_BIN_EXE_docmap-desktop");
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A folder of the test that goes away with it.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("docmap-desktop-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(PROGRAM)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the program starts")
}

fn run_with(args: &[&str], family: &Path) -> Output {
    Command::new(PROGRAM)
        .args(args)
        .env("APPKIT_FAMILY_DIR", family)
        .env("DOCMAP_DESKTOP_NO_LOGIN_SYNC", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the program starts")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn the_version_is_one_line_with_the_name_in_every_spelling() {
    let _one = serial();
    for word in ["--version", "-V", "version"] {
        let o = run(&[word]);
        assert_eq!(o.status.code(), Some(0), "{word}: {o:?}");
        assert_eq!(out(&o), format!("docmap-desktop {VERSION}\n"), "{word}");
    }
}

#[test]
fn help_names_every_command_and_one_command_has_its_own_text() {
    let _one = serial();
    for word in ["--help", "-h", "help"] {
        let o = run(&[word]);
        assert_eq!(o.status.code(), Some(0), "{word}: {o:?}");
        let text = out(&o);
        for command in [
            "status",
            "stop",
            "report",
            "version",
            "--open-at",
            "help",
            "Exit codes",
        ] {
            assert!(text.contains(command), "{word}: {command}: {text}");
        }
    }
    assert!(out(&run(&["help", "stop"])).starts_with("docmap-desktop stop\n"));
    assert!(out(&run(&["status", "--help"])).starts_with("docmap-desktop status\n"));
    assert!(out(&run(&["report", "--help"])).starts_with("docmap-desktop report"));
    // The flag of the request from outside has a help of its own, and asking for it opens no window.
    assert!(out(&run(&["--open-at", "--help"])).starts_with("docmap-desktop --open-at"));
}

#[test]
fn wrong_words_end_with_exit_code_2_and_the_usage() {
    let _one = serial();
    for args in [
        &["status", "now"][..],
        &["stop", "-f"],
        &["version", "x"],
        &["--version", "x"],
        &["--install-command", "x"],
        &["help", "frobnicate"],
        &["report", "--top=0"],
        &["report", "--dir=E:/repos"],
    ] {
        let o = run(args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {o:?}");
        assert!(err(&o).contains("usage:"), "{args:?}: {}", err(&o));
    }
}

/// No window of this very file is open while the tests run: `status` says so with exit code 1 and `stop` has nothing to
/// do. (Elsewhere than on Windows the two commands say that they are not built there.)
#[test]
fn status_and_stop_without_an_open_window() {
    let _one = serial();
    let status = run(&["status"]);
    let stop = run(&["stop"]);
    if cfg!(windows) {
        assert_eq!(status.status.code(), Some(1), "{status:?}");
        assert_eq!(out(&status), "docmap-desktop: not open\n");
        assert_eq!(stop.status.code(), Some(0), "{stop:?}");
        assert_eq!(out(&stop), "docmap-desktop: not open\n");
    } else {
        assert_eq!(status.status.code(), Some(1), "{status:?}");
        assert!(err(&status).contains("Windows"), "{}", err(&status));
        assert_eq!(stop.status.code(), Some(1), "{stop:?}");
    }
}

/// The setup calls these two words: the starter is made in the family folder of the test, it runs the program with its
/// arguments and hands the output and the exit code back, and the uninstall takes it and the folder away.
#[test]
fn the_setup_words_make_the_command_run_it_and_take_it_away() {
    let _one = serial();
    let dir = Dir::new("setup");
    let family = dir.0.join("family");
    let bin = family.join("bin");

    let made = run_with(&["--install-command"], &family);
    assert_eq!(made.status.code(), Some(0), "{made:?}");
    assert!(
        out(&made).contains("command: `docmap-desktop`"),
        "{}",
        out(&made)
    );
    let starters: Vec<PathBuf> = if cfg!(windows) {
        vec![bin.join("docmap-desktop.cmd"), bin.join("docmap-desktop")]
    } else {
        vec![bin.join("docmap-desktop")]
    };
    for starter in &starters {
        assert!(starter.is_file(), "{}", starter.display());
        assert!(std::fs::read_to_string(starter)
            .unwrap()
            .contains("appkit starter"));
    }

    // A second call changes nothing it says (no new PATH entry, the starters as they are).
    let again = run_with(&["--install-command"], &family);
    assert_eq!(again.status.code(), Some(0), "{again:?}");

    // The starter reaches the program: its output and its exit codes come back (version 0, `status` 1, wrong words 2).
    let through = |args: &[&str]| -> Output {
        let mut c = if cfg!(windows) {
            let mut c = Command::new("cmd.exe");
            c.args(["/d", "/c"]).arg(&starters[0]);
            c
        } else {
            let mut c = Command::new("sh");
            c.arg(&starters[0]);
            c
        };
        c.args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("the starter starts")
    };
    let version = through(&["--version"]);
    assert_eq!(version.status.code(), Some(0), "{version:?}");
    assert_eq!(out(&version), format!("docmap-desktop {VERSION}\n"));
    assert_eq!(through(&["status"]).status.code(), Some(1));
    assert_eq!(through(&["status", "now"]).status.code(), Some(2));

    let removed = run_with(&["--uninstall-command"], &family);
    assert_eq!(removed.status.code(), Some(0), "{removed:?}");
    assert!(out(&removed).contains("removed"), "{}", out(&removed));
    for starter in &starters {
        assert!(!starter.exists(), "{}", starter.display());
    }
    assert!(!bin.exists(), "the folder goes with the last starter");

    // Nothing there any more: nothing to say, no error.
    let nothing = run_with(&["--uninstall-command"], &family);
    assert_eq!(nothing.status.code(), Some(0), "{nothing:?}");
}

/// A test of the program with no folder of its own never makes a command on the machine of the person.
#[test]
fn the_setup_words_change_nothing_without_a_folder_of_their_own() {
    let _one = serial();
    let o = Command::new(PROGRAM)
        .arg("--install-command")
        .env("DOCMAP_DESKTOP_NO_LOGIN_SYNC", "1")
        .env_remove("APPKIT_FAMILY_DIR")
        .output()
        .expect("the program starts");
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    assert_eq!(out(&o), "");
    let o = Command::new(PROGRAM)
        .arg("--uninstall-command")
        .env("DOCMAP_DESKTOP_NO_LOGIN_SYNC", "1")
        .env_remove("APPKIT_FAMILY_DIR")
        .output()
        .expect("the program starts");
    assert_eq!(o.status.code(), Some(0), "{o:?}");
}

/// A function big enough (above the duplicate floor of 40 tree nodes) whose shape two repositories share under other
/// names: the report must find it in both.
fn shared_function(name: &str, list: &str) -> String {
    format!(
        "local M = {{}}

function M.{name}(items)
  local {list} = {{}}
  for index, item in ipairs(items) do
    if item ~= '' and #item > 2 then
      {list}[#{list} + 1] = {{ at = index, text = item:upper(), size = #item }}
    elseif item == '-' then
      {list}[#{list} + 1] = {{ at = index, text = item, size = 1 }}
    end
  end
  table.sort({list}, function(a, b)
    if a.size ~= b.size then
      return a.size > b.size
    end
    return a.at < b.at
  end)
  return {list}
end

return M
"
    )
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The engine for the report and its grammars: `DOCMAP_TEST_ENGINE` and `DOCMAP_TEST_GRAMMARS` (CI downloads the
/// rolling release), else the app's own beside the test program (`target/<profile>/docmap.exe` and `grammars`). None
/// when that engine cannot report over many repositories (an older one, or the placeholder of a build without one);
/// with `DOCMAP_TEST_ENGINE_REQUIRED` set that is a failure, not a skip.
fn fleet_engine() -> Option<(PathBuf, PathBuf)> {
    let beside = Path::new(PROGRAM).parent().unwrap().to_path_buf();
    let engine = std::env::var_os("DOCMAP_TEST_ENGINE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            beside.join(if cfg!(windows) {
                "docmap.exe"
            } else {
                "docmap"
            })
        });
    let grammars = std::env::var_os("DOCMAP_TEST_GRAMMARS")
        .map(PathBuf::from)
        .unwrap_or_else(|| beside.join("grammars"));
    let answer = Command::new(&engine).arg("--capabilities").output();
    let fleet = answer
        .as_ref()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("\"fleet\""));
    if fleet && grammars.is_dir() {
        return Some((engine, grammars));
    }
    let why = format!(
        "no engine with `fleet` and grammars ({} answered {:?}; grammars {})",
        engine.display(),
        answer.map(|o| String::from_utf8_lossy(&o.stdout)
            .chars()
            .take(120)
            .collect::<String>()),
        grammars.display()
    );
    assert!(
        std::env::var_os("DOCMAP_TEST_ENGINE_REQUIRED").is_none(),
        "{why}"
    );
    println!("n/a  {why}");
    None
}

/// `docmap-desktop report` over two small repositories that share a function, and a private one in the workspace that
/// is left out: the report on stdout, the progress and what was left out on stderr, the exit codes of the command.
/// The repositories lie below the build folder, not the temp folder (the standalone engine does not come back from a
/// project below AppData\Local\Temp on Windows, see scripts/e2e/map-history-fixture.mjs).
#[test]
fn report_joins_the_repositories_of_the_workspace_and_leaves_private_ones_out() {
    let _one = serial();
    let base =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let config = base.join("config");
    // a folder name the app takes for a private repository, assembled (this public source names none)
    let private_name = concat!("Notes-", "WK", "DBook");
    let (one, two, private) = (base.join("one"), base.join("two"), base.join(private_name));
    write(
        &one.join("lua/one/a.lua"),
        &shared_function("collect", "found"),
    );
    write(
        &two.join("lua/two/b.lua"),
        &shared_function("gather", "hits"),
    );
    write(
        &private.join("lua/p/c.lua"),
        &shared_function("secret", "kept"),
    );
    let report = |args: &[&str]| {
        Command::new(PROGRAM)
            .arg("report")
            .args(args)
            .env("DOCMAP_DESKTOP_CONFIG_DIR", &config)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("the program starts")
    };

    // An empty workspace and no repository named: nothing to report on, said so. No engine is asked.
    std::fs::create_dir_all(&config).unwrap();
    let nothing = report(&[]);
    assert_eq!(nothing.status.code(), Some(1), "{nothing:?}");
    assert!(
        err(&nothing).contains("no project to report on"),
        "{}",
        err(&nothing)
    );
    assert_eq!(out(&nothing), "");

    let Some((engine, grammars)) = fleet_engine() else {
        let _ = std::fs::remove_dir_all(&base);
        return;
    };
    let project = |p: &Path| serde_json::json!({ "id": slash(p), "name": p.file_name().unwrap().to_string_lossy(), "root": slash(p), "map_dir": "docs/map" });
    write(
        &config.join("workspace.json"),
        &serde_json::json!({ "projects": [], "engine": slash(&engine), "grammars": slash(&grammars) }).to_string(),
    );
    write(
        &config.join("workspaces/Default.json"),
        &serde_json::json!([project(&one), project(&private), project(&two)]).to_string(),
    );

    // The projects of the workspace: the shared shape is found, the private repository is left out and said so.
    let o = report(&["--top=5"]);
    let (stdout, stderr) = (out(&o), err(&o));
    assert_eq!(o.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(
        stdout.contains("# docmap across the repositories"),
        "{stdout}"
    );
    assert!(
        stdout.contains("2 repositories; 1 shapes appear in at least two"),
        "{stdout}"
    );
    assert!(stdout.contains("one + two"), "{stdout}");
    assert!(
        !stdout.contains(private_name),
        "the private repository is in the report: {stdout}"
    );
    assert!(
        stderr.contains("left out") && stderr.contains(private_name),
        "{stderr}"
    );
    assert!(
        stderr.contains("one: 1 functions above the floor"),
        "{stderr}"
    );
    assert!(
        !stdout.contains("functions above the floor"),
        "progress on stdout: {stdout}"
    );

    // Named repositories, as JSON.
    let o = report(&[&slash(&one), &slash(&two), "--json"]);
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let joined: serde_json::Value = serde_json::from_str(&out(&o)).expect("one JSON object");
    assert_eq!(
        joined["shapes"].as_array().map(Vec::len),
        Some(1),
        "{joined}"
    );
    assert_eq!(
        joined["failed"].as_array().map(Vec::len),
        Some(0),
        "{joined}"
    );

    // A repository that is not there: exit code 1, the others are still reported.
    let missing = base.join("missing");
    let o = report(&[&slash(&one), &slash(&missing)]);
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    assert!(out(&o).contains("Not scanned:"), "{}", out(&o));
    assert!(out(&o).contains("| one |"), "{}", out(&o));

    // Nobody reads stdout any more (`report | head` once head has its lines; here closed before the first one): the
    // command ends quietly with exit code 0, it does not panic on the write.
    let mut child = Command::new(PROGRAM)
        .args(["report", &slash(&one), &slash(&two)])
        .env("DOCMAP_DESKTOP_CONFIG_DIR", &config)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the program starts");
    drop(child.stdout.take());
    let o = child.wait_with_output().expect("it ends");
    assert!(!err(&o).contains("panicked"), "{}", err(&o));
    assert_eq!(o.status.code(), Some(0), "{o:?}");

    let _ = std::fs::remove_dir_all(&base);
}
