//! The command line of docmap-desktop: `docmap-desktop --version`, `--help`, `status`, `stop` and `report`, and the two words the
//! installer calls (`--install-command`, `--uninstall-command`). Everything else starts the window, as before
//! (`--open-at <repo> <file>:<line>` among it).
//!
//! The exit codes are those of the other apps of the family: 0 done, 1 failed (`status`: no window is open), 2 wrong
//! arguments. The release build is a window program, so it attaches to the console of the terminal that started it
//! before it writes (`appkit_core::console`); started by the installer or a double click it has none and writes nowhere.
//!
//! The command `docmap-desktop` itself is a starter (`command_entry`) that the setup writes: PowerShell does not wait
//! for a window program, the starter does, so the output and the exit code of these commands reach the caller.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::command_entry::{self, Env, UserPath};
use crate::instances;
use crate::report::{self, ReportArgs};

/// The name of the command in the terminal.
pub(crate) const COMMAND: &str = "docmap-desktop";

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Version,
    /// `None`: all of it; `Some(word)`: what one command does.
    Help(Option<String>),
    Status,
    Stop,
    /// The report over many repositories (`report.rs`).
    Report(ReportArgs),
    /// What the setup calls after it copied the files: make the command and put its folder on the `PATH`.
    SetupInstall,
    /// What the setup calls before it removes the files.
    SetupUninstall,
}

/// What the arguments are (the program name already dropped): a command of the command line, wrong words of one
/// (the text says what), or none, and then the window starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Parsed {
    Run(Command),
    Usage(String),
    Window,
}

const WORDS: [&str; 10] = [
    "--version",
    "-V",
    "version",
    "--help",
    "-h",
    "help",
    "status",
    "stop",
    "report",
    "open-at",
];

fn usage_of(what: &str) -> Parsed {
    Parsed::Usage(format!("{what}\n\n{}", help_text()))
}

/// Read the arguments. Only the words of the command line are taken; a path, `--open-at` and anything unknown go to the
/// window as before.
pub(crate) fn parse(args: &[String]) -> Parsed {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => Parsed::Window,
        ["--version" | "-V" | "version"] => Parsed::Run(Command::Version),
        ["--help" | "-h" | "help"] => Parsed::Run(Command::Help(None)),
        ["help", word] => Parsed::Run(Command::Help(Some((*word).to_owned()))),
        ["--open-at", "--help" | "-h"] => Parsed::Run(Command::Help(Some("open-at".to_owned()))),
        [word, "--help" | "-h"] if WORDS.contains(word) => {
            Parsed::Run(Command::Help(Some((*word).to_owned())))
        }
        ["status"] => Parsed::Run(Command::Status),
        ["stop"] => Parsed::Run(Command::Stop),
        ["report", rest @ ..] if rest.iter().any(|w| matches!(*w, "--help" | "-h")) => {
            Parsed::Run(Command::Help(Some("report".to_owned())))
        }
        ["report", rest @ ..] => match report::parse(rest) {
            Ok(args) => Parsed::Run(Command::Report(args)),
            Err(what) => usage_of(&what),
        },
        ["--install-command"] => Parsed::Run(Command::SetupInstall),
        ["--uninstall-command"] => Parsed::Run(Command::SetupUninstall),
        [word @ ("status" | "stop" | "version"), ..] => {
            usage_of(&format!("`{word}` takes no arguments"))
        }
        ["--version" | "-V" | "--help" | "-h" | "--install-command" | "--uninstall-command", ..] => {
            usage_of("that command takes no arguments")
        }
        _ => Parsed::Window,
    }
}

/// The text of `--help`.
pub(crate) fn help_text() -> String {
    format!(
        "usage:
  {COMMAND}                              open the window (the projects you added)
  {COMMAND} --open-at <repo> <file>:<line>[-<last>]
                                          open the window at a place of the map of a project it knows
  {COMMAND} --open-at=<repo>|<file>:<line>[-<last>]
                                          the same in one word (for a caller that cannot pass two)
  {COMMAND} status                       is a window of this program open? exit code 0 yes, 1 no
  {COMMAND} stop                         close it as its close button does (a window that asks something stays;
                                          nothing is ended by force)
  {COMMAND} report [<root>...] [--json] [--top=<n>] [--with-generated]
                                          duplicates, quicks and findings over many repositories (default:
                                          the projects of the workspace); progress on stderr, the report on stdout
  {COMMAND} version | --version          the version
  {COMMAND} help [<command>]             this text, or what one command does (also <command> --help)
The setup of the app makes the command `{COMMAND}` for every new terminal; the engine `docmap.exe` next to it is
the app's own and has no command of its own.
Exit codes: 0 done, 1 failed (status: no window is open; report: a repository was not read), 2 wrong arguments."
    )
}

/// What one word does, for `help <command>`.
fn help_of(word: &str) -> Option<String> {
    let text = match word {
        "status" => format!(
            "{COMMAND} status\n  Whether a window of this program (this very file, not a build somewhere else) is open: prints it\n  with its process id. Exit code 0 when one is, 1 when none is."
        ),
        "stop" => format!(
            "{COMMAND} stop\n  Close the windows of this program the way their close button does, and wait a few seconds. A\n  window that asks something (a run of the agent) stays open and is said so (exit code 1). Nothing\n  is ended by force."
        ),
        "report" => format!(
            "{COMMAND} report [<root>...] [--json] [--top=<n>] [--with-generated]
  The Duplicates, Quicks and Findings tabs over many repositories at once, and what turns up in more than
  one of them: shapes of functions found in two or more repositories, the pairs that share the most, quicks
  and checks that are bad in several. Without <root> it reads the projects of the active workspace and
  leaves private ones out (a small built-in list of folder names, the engine's; stderr names each one it
  leaves out); a <root> named on purpose is read with a warning. It uses the engine and grammars of the
  window. Progress goes to stderr, the report to stdout (Markdown, or one JSON object with --json);
  nothing is written into a repository.
  --top=<n>          rows per table (default 25)
  --with-generated   keep built output (dist-*, bundles, node_modules), left out by default
  Exit code 0 done, 1 a repository was not read (the report lists the others) or no report was made,
  2 wrong arguments."
        ),
        "version" | "--version" => format!("{COMMAND} version   (also --version, -V)\n  The version."),
        "help" | "--help" => format!(
            "{COMMAND} help [<command>]\n  All commands, or what one does (also `{COMMAND} <command> --help`)."
        ),
        "open-at" | "--open-at" => format!(
            "{COMMAND} --open-at <repo> <file>:<line>[-<last>]\n  Open the window at a place of the map of a project it knows. Asked from outside (tasks-desktop's\n  \"open in DocMap\"); the repository is only matched against the projects of the workspace.
  The same in one word: {COMMAND} --open-at=<repo>|<file>:<line>[-<last>] (the first | splits the two)."
        ),
        _ => return None,
    };
    Some(text)
}

/// The version line.
pub(crate) fn version_line() -> String {
    format!("{COMMAND} {}", env!("CARGO_PKG_VERSION"))
}

/// Where the starter and the `PATH` are made: nowhere when a test of the program asks for no change to the machine
/// (`DOCMAP_DESKTOP_NO_LOGIN_SYNC`) and names no family folder of its own (`APPKIT_FAMILY_DIR`); there the starter goes
/// and the `PATH` of the user stays; on Windows the `PATH` of the user, elsewhere none (`~/.local/bin` is on it
/// already). The same rule as `appkit_hub::background`.
fn path_store(env: &Env) -> Option<Option<&'static dyn UserPath>> {
    let tests = env.get("DOCMAP_DESKTOP_NO_LOGIN_SYNC").is_some();
    if tests && env.get(command_entry::FAMILY_DIR_VAR).is_none() {
        return None;
    }
    if tests || !cfg!(windows) {
        return Some(None);
    }
    Some(Some(&command_entry::SystemPath))
}

/// The setup's word: make the command of the program that runs.
fn install_command() -> Result<String, String> {
    let env = Env::from_process();
    let Some(path) = path_store(&env) else {
        return Ok(String::new());
    };
    let program = std::env::current_exe().map_err(|e| format!("the program is not found: {e}"))?;
    command_entry::install(COMMAND, &program, &env, path)
}

/// The setup's word: take the command away (and the folder and its `PATH` entry with the last starter).
fn uninstall_command() -> Result<String, String> {
    let env = Env::from_process();
    let Some(path) = path_store(&env) else {
        return Ok(String::new());
    };
    command_entry::uninstall(COMMAND, &env, path)
}

/// How long `stop` waits for the windows to go.
const STOP_WAIT: Duration = Duration::from_secs(8);

fn other_windows(own: &Path) -> Result<Vec<u32>, String> {
    instances::others(own)
}

fn status() -> Result<i32, String> {
    let own = std::env::current_exe().map_err(|e| e.to_string())?;
    let pids = other_windows(&own)?;
    if pids.is_empty() {
        println!("{COMMAND}: not open");
        return Ok(1);
    }
    let list: Vec<String> = pids.iter().map(u32::to_string).collect();
    println!("{COMMAND}: open (process {})", list.join(", "));
    Ok(0)
}

fn stop() -> Result<i32, String> {
    let own = std::env::current_exe().map_err(|e| e.to_string())?;
    let pids = other_windows(&own)?;
    if pids.is_empty() {
        println!("{COMMAND}: not open");
        return Ok(0);
    }
    let asked = instances::close_windows(&pids);
    if asked == 0 {
        println!(
            "{COMMAND}: open (process {}), but it has no window to ask",
            pids[0]
        );
        return Ok(1);
    }
    let end = Instant::now() + STOP_WAIT;
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200));
        if other_windows(&own)?.is_empty() {
            println!("{COMMAND}: closed");
            return Ok(0);
        }
    }
    println!("{COMMAND}: still open: it asked something (look at its window)");
    Ok(1)
}

/// Run a command of the command line; the exit code.
pub(crate) fn run(command: &Command) -> i32 {
    // The release build is a window program: write where the person's terminal is, if there is one.
    let _console = appkit_core::console::attach_parent();
    let done = |result: Result<String, String>| match result {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(e) => {
            eprintln!("{COMMAND}: {e}");
            1
        }
    };
    match command {
        Command::Version => {
            println!("{}", version_line());
            0
        }
        Command::Help(None) => {
            println!("{}", help_text());
            0
        }
        Command::Help(Some(word)) => match help_of(word) {
            Some(text) => {
                println!("{text}");
                0
            }
            None => {
                eprintln!("{COMMAND}: no command {word:?}\n\n{}", help_text());
                2
            }
        },
        Command::Status => status().unwrap_or_else(|e| {
            eprintln!("{COMMAND}: {e}");
            1
        }),
        Command::Stop => stop().unwrap_or_else(|e| {
            eprintln!("{COMMAND}: {e}");
            1
        }),
        Command::Report(args) => report::run(args),
        Command::SetupInstall => done(install_command()),
        Command::SetupUninstall => done(uninstall_command()),
    }
}

/// At the start of `main`: the exit code when the arguments are a command of the command line, none when the window
/// starts.
pub(crate) fn early(args: &[String]) -> Option<i32> {
    match parse(args) {
        Parsed::Window => None,
        Parsed::Run(command) => Some(run(&command)),
        Parsed::Usage(text) => {
            let _console = appkit_core::console::attach_parent();
            eprintln!("{COMMAND}: {text}");
            Some(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(words: &[&str]) -> Parsed {
        parse(&words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn the_words_of_the_command_line_are_read_and_nothing_else_is() {
        assert_eq!(parsed(&[]), Parsed::Window);
        for word in ["--version", "-V", "version"] {
            assert_eq!(parsed(&[word]), Parsed::Run(Command::Version), "{word}");
        }
        for word in ["--help", "-h", "help"] {
            assert_eq!(parsed(&[word]), Parsed::Run(Command::Help(None)), "{word}");
        }
        assert_eq!(
            parsed(&["help", "stop"]),
            Parsed::Run(Command::Help(Some("stop".into())))
        );
        assert_eq!(
            parsed(&["status", "--help"]),
            Parsed::Run(Command::Help(Some("status".into())))
        );
        // Help for the request from outside is help, never a window; the request itself still is the window's.
        assert_eq!(
            parsed(&["--open-at", "--help"]),
            Parsed::Run(Command::Help(Some("open-at".into())))
        );
        assert_eq!(parsed(&["status"]), Parsed::Run(Command::Status));
        assert_eq!(parsed(&["stop"]), Parsed::Run(Command::Stop));
        assert_eq!(
            parsed(&["report"]),
            Parsed::Run(Command::Report(ReportArgs::default()))
        );
        assert_eq!(
            parsed(&["report", "E:/r/a", "--json", "--top=4"]),
            Parsed::Run(Command::Report(ReportArgs {
                roots: vec!["E:/r/a".into()],
                json: true,
                top: Some(4),
                with_generated: false,
            }))
        );
        for words in [
            &["report", "--help"][..],
            &["report", "a", "-h"],
            &["help", "report"],
        ] {
            assert_eq!(
                parsed(words),
                Parsed::Run(Command::Help(Some("report".into()))),
                "{words:?}"
            );
        }
        assert_eq!(
            parsed(&["--install-command"]),
            Parsed::Run(Command::SetupInstall)
        );
        assert_eq!(
            parsed(&["--uninstall-command"]),
            Parsed::Run(Command::SetupUninstall)
        );
    }

    /// What the window took before is the window's still: the request from outside, a path, a word of its own.
    #[test]
    fn what_the_window_took_before_still_starts_the_window() {
        for words in [
            &["--open-at", "repo", "src/a.rs:3"][..],
            &["C:\\work\\project"],
            &["/home/me/project"],
            &["--something-new"],
            &["statuses"],
        ] {
            assert_eq!(parsed(words), Parsed::Window, "{words:?}");
        }
    }

    #[test]
    fn extra_words_to_a_command_are_wrong_arguments_with_the_usage() {
        for words in [
            &["status", "now"][..],
            &["stop", "-f"],
            &["--version", "x"],
            &["version", "x"],
            &["--install-command", "x"],
            &["report", "--top=0"],
            &["report", "--dir=E:/repos"],
        ] {
            match parsed(words) {
                Parsed::Usage(text) => assert!(text.contains("usage:"), "{words:?}: {text}"),
                other => panic!("{words:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn every_command_has_its_help_and_the_usage_lists_them() {
        let usage = help_text();
        for word in ["status", "stop", "report", "version", "help", "open-at"] {
            assert!(help_of(word).is_some(), "{word}");
        }
        assert!(help_of("serve").is_none());
        for word in ["status", "stop", "report", "version", "--open-at", "help"] {
            assert!(
                usage.contains(&format!("{COMMAND} {word}")),
                "{word}: {usage}"
            );
        }
        assert!(usage.contains("Exit codes"), "{usage}");
        assert_eq!(
            version_line(),
            format!("docmap-desktop {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn the_setup_makes_the_command_only_where_it_is_allowed_to() {
        // A test of the program with no folder of its own: nothing is made.
        let quiet = Env::from_pairs([("DOCMAP_DESKTOP_NO_LOGIN_SYNC", "1")]);
        assert!(path_store(&quiet).is_none());
        // With a folder of its own: the starter goes there, the PATH of the user stays.
        let own = Env::from_pairs([
            ("DOCMAP_DESKTOP_NO_LOGIN_SYNC", "1"),
            (command_entry::FAMILY_DIR_VAR, "/family"),
        ]);
        assert!(matches!(path_store(&own), Some(None)));
        // A person's machine: Windows asks the PATH of the user, elsewhere nothing is changed.
        let person = Env::from_pairs([("LOCALAPPDATA", "/x")]);
        assert_eq!(
            path_store(&person).map(|p| p.is_some()),
            Some(cfg!(windows))
        );
    }
}
