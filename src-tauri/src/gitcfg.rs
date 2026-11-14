//! Reads what git wrote down about a checkout, without starting `git`.
//!
//! Two questions are answered here by reading files: where the git directory
//! of a checkout is (a `.git` directory, or the `.git` *file* of a linked
//! worktree or a submodule), and which URLs its config names for a remote.
//! Both used to cost one `git` process each, ten of them when the app opened
//! with ten projects.
//!
//! **What this reader does not do, and `git remote get-url` does.** It reads
//! the one config file of the repository. It does not read the user's or the
//! system's config, follows no `include.path` / `includeIf`, applies no
//! `url.<base>.insteadOf` rewrite and reads no `config.worktree`. A remote
//! whose URL only becomes a GitHub address through such a rewrite (an alias
//! like `gh:owner/name`) is therefore not recognised as one; the address
//! written in the file is what counts. `git config --file <f> --get`, the
//! other call this replaces, never did any of that either.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A config larger than this is not one a person or git wrote for a checkout.
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
/// A `.git` file or a `commondir` file holds one path.
const MAX_POINTER_BYTES: u64 = 64 * 1024;

/// Read at most `limit` bytes of a regular file as text.
fn read_capped(path: &Path, limit: u64) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The git directory of the checkout whose top folder is `dir`.
///
/// `.git` is a directory in an ordinary clone. In a linked worktree and in a
/// submodule it is a file with one line, `gitdir: <path>`, relative to `dir`
/// unless absolute.
pub(crate) fn git_dir_at(dir: &Path) -> Option<PathBuf> {
    let dot = dir.join(".git");
    let meta = fs::metadata(&dot).ok()?;
    if meta.is_dir() {
        return Some(dot);
    }
    let text = read_capped(&dot, MAX_POINTER_BYTES)?;
    let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    if target.is_empty() || !names_this_machine(target) {
        return None;
    }
    let git_dir = dir.join(target);
    git_dir.is_dir().then_some(git_dir)
}

/// Does a path written in a `.git` or `commondir` file stay on this machine?
///
/// Such a file comes with the checkout, which somebody else may have
/// prepared. A share (`\\host\share\...`), a device path or a root without a
/// drive is not looked at: merely asking whether it is a directory makes
/// Windows contact the host it names, with the user's credentials. A relative
/// path and a drive path are what git writes there.
fn names_this_machine(target: &str) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let path = Path::new(target);
        match path.components().next() {
            Some(Component::Prefix(p)) => {
                matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) && path.has_root()
            }
            // `\foo` and `/foo`: a root on whatever drive is current.
            _ => !path.has_root(),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        true
    }
}

/// The directory that holds what all worktrees of a repository share, the
/// config among it.
///
/// The git directory of a linked worktree (`<main>/.git/worktrees/<name>`)
/// names it in a file `commondir`, relative to itself. Every other git
/// directory, a submodule's `.git/modules/<name>` included, is its own.
pub(crate) fn common_dir(git_dir: &Path) -> PathBuf {
    match read_capped(&git_dir.join("commondir"), MAX_POINTER_BYTES) {
        Some(text) if !text.trim().is_empty() && names_this_machine(text.trim()) => {
            git_dir.join(text.trim())
        }
        _ => git_dir.to_path_buf(),
    }
}

/// The config file of the repository `start` lies in: `start` itself or the
/// nearest folder above it that is a checkout, the way `git -C <start>` finds
/// its repository.
pub(crate) fn config_above(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        if let Some(git_dir) = git_dir_at(dir) {
            let config = common_dir(&git_dir).join("config");
            if config.is_file() {
                return Some(config);
            }
        }
    }
    None
}

/// Every `url` of the remote `name` in the config file `config`, in the order
/// of the file. Empty when the file cannot be read or names none.
///
/// The two callers pick differently, each like the `git` call it replaces:
/// `git remote get-url` answers with the first, `git config --get` with the
/// last.
pub(crate) fn remote_urls(config: &Path, name: &str) -> Vec<String> {
    match read_capped(config, MAX_CONFIG_BYTES) {
        Some(text) => values(&text, "remote", Some(name), "url"),
        None => Vec::new(),
    }
}

/// The values of `section[.subsection].key` in the text of a git config file.
///
/// The syntax is git's own (`git help config`, "Syntax"): section and key
/// names compare without case, a quoted subsection compares exactly, the old
/// `[section.subsection]` form compares without case; a value ends at a
/// comment outside quotes, loses the whitespace around it, keeps what is
/// inside quotes, and continues on the next line after a trailing backslash.
/// A key without `=` is a boolean and has no value to return. A line this
/// reader cannot make sense of is skipped rather than failing the file: the
/// question is only ever "which URL", and a wrong answer to it is worse than
/// none.
pub(crate) fn values(
    text: &str,
    section: &str,
    subsection: Option<&str>,
    key: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_section = false;
    let mut chars = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .chars()
        .peekable();

    loop {
        // Between entries: whitespace and comments.
        match chars.peek() {
            None => break,
            Some(c) if c.is_whitespace() => {
                chars.next();
                continue;
            }
            Some('#') | Some(';') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                continue;
            }
            Some('[') => {
                chars.next();
                in_section = match read_header(&mut chars) {
                    Some((sec, sub)) => {
                        sec.eq_ignore_ascii_case(section)
                            && match (&sub, subsection) {
                                (None, None) => true,
                                (Some(Sub::Exact(s)), Some(want)) => s == want,
                                (Some(Sub::Folded(s)), Some(want)) => s.eq_ignore_ascii_case(want),
                                _ => false,
                            }
                    }
                    None => false,
                };
                continue;
            }
            Some(_) => {}
        }

        // A key, then an optional value.
        let mut name = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_alphanumeric() || c == '-' {
                name.push(c);
                chars.next();
            } else {
                break;
            }
        }
        if name.is_empty() {
            // Not a key: drop the rest of the line.
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        while matches!(chars.peek(), Some(' ') | Some('\t')) {
            chars.next();
        }
        let value = if chars.peek() == Some(&'=') {
            chars.next();
            Some(read_value(&mut chars))
        } else {
            None
        };
        if in_section && name.eq_ignore_ascii_case(key) {
            if let Some(v) = value {
                out.push(v);
            }
        }
    }
    out
}

/// The subsection of a header, in one of its two spellings.
enum Sub {
    /// `[remote "origin"]`: compared exactly.
    Exact(String),
    /// `[remote.origin]`: compared without case.
    Folded(String),
}

/// The rest of a section header after `[`, up to and including `]`.
fn read_header(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Option<(String, Option<Sub>)> {
    let mut name = String::new();
    let mut sub = None;
    loop {
        match chars.next()? {
            ']' => break,
            '\n' => return None,
            c if c == ' ' || c == '\t' => {
                // Only a quoted subsection may follow.
                while matches!(chars.peek(), Some(' ') | Some('\t')) {
                    chars.next();
                }
                if chars.next()? != '"' {
                    return None;
                }
                let mut s = String::new();
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\n' => return None,
                        '\\' => s.push(chars.next()?),
                        c => s.push(c),
                    }
                }
                sub = Some(Sub::Exact(s));
                while matches!(chars.peek(), Some(' ') | Some('\t')) {
                    chars.next();
                }
                if chars.next()? != ']' {
                    return None;
                }
                break;
            }
            c if c.is_ascii_alphanumeric() || c == '-' || c == '.' => name.push(c),
            _ => return None,
        }
    }
    if sub.is_none() {
        if let Some((sec, rest)) = name.split_once('.') {
            let (sec, rest) = (sec.to_string(), rest.to_string());
            return Some((sec, Some(Sub::Folded(rest))));
        }
    }
    (!name.is_empty()).then_some((name, sub))
}

/// A value after `=`, up to the end of its (possibly continued) line.
fn read_value(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut out = String::new();
    // Whitespace seen outside quotes that only counts if more value follows.
    let mut pending = String::new();
    let mut quoted = false;
    let mut comment = false;
    while let Some(c) = chars.next() {
        if c == '\n' {
            break;
        }
        if comment {
            continue;
        }
        match c {
            '\r' if chars.peek() == Some(&'\n') => {}
            '\\' => match chars.next() {
                // A continued line.
                Some('\n') => {}
                Some('\r') if chars.peek() == Some(&'\n') => {
                    chars.next();
                }
                Some(e) => {
                    out.push_str(&pending);
                    pending.clear();
                    out.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'b' => '\u{8}',
                        other => other,
                    });
                }
                None => break,
            },
            '"' => {
                quoted = !quoted;
                out.push_str(&pending);
                pending.clear();
            }
            '#' | ';' if !quoted => comment = true,
            c if c.is_whitespace() && !quoted => {
                if !out.is_empty() {
                    pending.push(c);
                }
            }
            c => {
                out.push_str(&pending);
                pending.clear();
                out.push(c);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("docmap-gitcfg-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn git(dir: &Path, args: &[&str]) -> Option<String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    #[test]
    fn the_syntax_of_a_config_file() {
        let text = "\u{feff}# a comment\n\
            [core]\n\tbare = false\n\
            [remote \"origin\"]\n\
            \turl = https://github.com/o/n.git   ; trailing comment\n\
            \tfetch = +refs/heads/*:refs/remotes/origin/*\n\
            [remote \"Origin\"]\n\turl = wrong-case\n\
            [REMOTE \"origin\"] URL = \"quoted # not a comment\"  \n\
            [remote \"origin\"]\n\tprune\n\turl = one\\\n two\n\
            [remote \"a\\\"b\"]\n\turl = escaped\n\
            [remote.ORIGIN]\n\turl = old form\r\n\
            [branch \"main\"]\n\turl = not a remote\n";
        assert_eq!(
            values(text, "remote", Some("origin"), "url"),
            [
                "https://github.com/o/n.git",
                "quoted # not a comment",
                "one two",
                "old form"
            ]
        );
        assert_eq!(values(text, "remote", Some("a\"b"), "url"), ["escaped"]);
        assert_eq!(values(text, "core", None, "bare"), ["false"]);
        // A boolean key has no value; a missing one has none either.
        assert!(values(text, "remote", Some("origin"), "prune").is_empty());
        assert!(values(text, "remote", Some("upstream"), "url").is_empty());
        // Broken input ends in "nothing", not in a panic or a guess.
        assert!(values(
            "[remote \"origin\n url = x\n",
            "remote",
            Some("origin"),
            "url"
        )
        .is_empty());
        assert!(values("[remote \"origin\"", "remote", Some("origin"), "url").is_empty());
        assert!(values("= x\n[]\nurl", "remote", Some("origin"), "url").is_empty());
    }

    /// The reader against git itself, on a file git wrote and on spellings a
    /// person can type. Skips without git (see `filetree::tests::git_repo`).
    #[test]
    fn the_reader_agrees_with_git_on_a_real_repository() {
        let root = tmp("real");
        if git(&root, &["init", "-q"]).is_none() {
            return;
        }
        assert!(remote_urls(&config_above(&root).unwrap(), "origin").is_empty());

        git(
            &root,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/Owner/Name.git",
            ],
        )
        .unwrap();
        git(
            &root,
            &["remote", "add", "other", "git@example.com:x/y.git"],
        )
        .unwrap();
        git(
            &root,
            &[
                "remote",
                "set-url",
                "--add",
                "origin",
                "git@github.com:Owner/Second.git",
            ],
        )
        .unwrap();
        git(
            &root,
            &[
                "config",
                "remote.origin.note",
                "a \"quoted\" value; with # signs \\ and a tab\there",
            ],
        )
        .unwrap();

        let config = config_above(&root).unwrap();
        let urls = remote_urls(&config, "origin");
        // `git remote get-url` answers with the first URL...
        assert_eq!(
            urls.first().cloned(),
            git(&root, &["remote", "get-url", "origin"])
        );
        // ...and `git config --get` with the last.
        assert_eq!(
            urls.last().cloned(),
            git(
                &root,
                &[
                    "config",
                    "--file",
                    ".git/config",
                    "--get",
                    "remote.origin.url"
                ]
            )
        );
        assert_eq!(urls.len(), 2);
        assert_eq!(
            remote_urls(&config, "other"),
            [git(&root, &["remote", "get-url", "other"]).unwrap()]
        );
        // Escapes survive the round trip through git's own writer.
        let text = fs::read_to_string(&config).unwrap();
        assert_eq!(
            values(&text, "remote", Some("origin"), "note")
                .last()
                .cloned(),
            git(&root, &["config", "--get", "remote.origin.note"])
        );

        // From a folder below the top, the repository above is found.
        let below = root.join("a").join("b");
        fs::create_dir_all(&below).unwrap();
        assert_eq!(config_above(&below), Some(config.clone()));
        let _ = fs::remove_dir_all(&root);
    }

    /// `.git` is a file in a linked worktree and in a submodule; the config is
    /// the repository's, not a file next to that pointer.
    #[test]
    fn a_linked_worktree_and_a_submodule_find_their_config() {
        let base = tmp("linked");
        let main = base.join("main");
        fs::create_dir_all(&main).unwrap();
        if git(&main, &["init", "-q"]).is_none() {
            return;
        }
        git(&main, &["config", "user.email", "t@example.com"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["config", "commit.gpgsign", "false"]);
        git(
            &main,
            &["remote", "add", "origin", "https://github.com/o/main.git"],
        )
        .unwrap();
        fs::write(main.join("f"), b"x").unwrap();
        git(&main, &["add", "f"]).unwrap();
        if git(&main, &["commit", "-q", "-m", "t"]).is_none() {
            return;
        }

        let wt = base.join("wt");
        if git(&main, &["worktree", "add", "-q", wt.to_str().unwrap()]).is_none() {
            return;
        }
        assert!(wt.join(".git").is_file());
        let config = config_above(&wt).expect("the worktree has a config");
        assert_eq!(
            remote_urls(&config, "origin").first().cloned(),
            git(&wt, &["remote", "get-url", "origin"])
        );

        // A submodule: `.git` points into `<super>/.git/modules/<name>`, which
        // has a config of its own with its own origin.
        let sup = base.join("super");
        let module = sup.join(".git").join("modules").join("sub");
        let sub = sup.join("sub");
        fs::create_dir_all(&module).unwrap();
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(sup.join(".git")).unwrap();
        fs::write(
            sup.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = https://github.com/o/super.git\n",
        )
        .unwrap();
        fs::write(
            module.join("config"),
            "[remote \"origin\"]\n\turl = https://github.com/o/sub.git\n",
        )
        .unwrap();
        fs::write(sub.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();
        let config = config_above(&sub).unwrap();
        assert_eq!(
            remote_urls(&config, "origin"),
            ["https://github.com/o/sub.git"]
        );

        // A pointer to nowhere is no checkout; the search goes on above it.
        let stray = sup.join("stray");
        fs::create_dir_all(&stray).unwrap();
        fs::write(stray.join(".git"), "gitdir: ../gone\n").unwrap();
        assert_eq!(
            remote_urls(&config_above(&stray).unwrap(), "origin"),
            ["https://github.com/o/super.git"]
        );
        let _ = fs::remove_dir_all(&base);
    }

    /// A `.git` file is part of a checkout somebody else may have made: one
    /// that names a share or a device is not followed, not even to ask
    /// whether it is there.
    #[cfg(windows)]
    #[test]
    fn a_pointer_to_another_machine_is_not_followed() {
        for remote in [
            r"\\host\share\repo.git",
            "//host/share/repo.git",
            r"\\?\UNC\host\share\repo.git",
            r"\\.\pipe\x",
            r"\rooted\without\drive",
            "/rooted/without/drive",
            "C:relative-to-the-drive",
        ] {
            assert!(!names_this_machine(remote), "{remote}");
        }
        for local in [
            r"C:\repos\x\.git",
            "C:/repos/x/.git",
            "../.git/modules/sub",
            ".git",
        ] {
            assert!(names_this_machine(local), "{local}");
        }

        let base = tmp("remote-pointer");
        let top = base.join("top");
        let inner = top.join("inner");
        fs::create_dir_all(top.join(".git")).unwrap();
        fs::create_dir_all(&inner).unwrap();
        fs::write(
            top.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = https://github.com/o/top.git\n",
        )
        .unwrap();
        fs::write(inner.join(".git"), "gitdir: //host/share/repo.git\n").unwrap();
        assert_eq!(git_dir_at(&inner), None);
        // The search goes on above it, as for any `.git` that is no checkout.
        assert_eq!(
            remote_urls(&config_above(&inner).unwrap(), "origin"),
            ["https://github.com/o/top.git"]
        );
        // A `commondir` that names a share leaves the git directory its own.
        fs::write(top.join(".git").join("commondir"), "//host/share/common\n").unwrap();
        assert_eq!(common_dir(&top.join(".git")), top.join(".git"));
        let _ = fs::remove_dir_all(&base);
    }

    /// The three questions this module and the file tree answer start no
    /// process: outside their tests, the file tree, the traffic view and this
    /// reader name no `Command` at all, and the import names exactly one, the
    /// `git clone` that stays a child process by decision.
    #[test]
    fn the_git_questions_start_no_process() {
        let starts = |source: &str| {
            let production = source.split("#[cfg(test)]").next().unwrap();
            production.matches("Command::new").count()
        };
        assert_eq!(starts(include_str!("filetree.rs")), 0, "filetree.rs");
        assert_eq!(starts(include_str!("traffic.rs")), 0, "traffic.rs");
        assert_eq!(starts(include_str!("gitcfg.rs")), 0, "gitcfg.rs");
        assert_eq!(starts(include_str!("import.rs")), 1, "import.rs");
    }
}
