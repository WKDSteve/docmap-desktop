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
