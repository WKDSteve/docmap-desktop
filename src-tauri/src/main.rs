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
mod engine;
mod feedback;
mod filetree;
mod freshness;
mod gitcfg;
mod github;
mod icon;
mod import;
mod languages;
mod menu;
mod nvim;
mod paths;
mod proc;
mod safe_read;
mod search;
mod server;
mod shellopen;
mod stats;
mod telemetry;
#[cfg(test)]
mod testutil;
mod traffic;
mod traffic_cmds;
mod views;
mod workspace;

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

// The backend lives in the modules above; their items are re-exported so that `crate::name` and the tests keep one
// namespace. main.rs itself only wires the commands.
#[allow(unused_imports)]
pub(crate) use engine::*;
#[allow(unused_imports)]
pub(crate) use import::*;
#[allow(unused_imports)]
pub(crate) use nvim::*;
#[allow(unused_imports)]
pub(crate) use paths::*;
#[allow(unused_imports)]
pub(crate) use traffic_cmds::*;
#[allow(unused_imports)]
pub(crate) use views::*;
#[allow(unused_imports)]
pub(crate) use workspace::*;

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
            workspace::map_dir_refused,
            workspace::list_projects,
            workspace::add_project,
            import::inspect_folder,
            import::import_many,
            workspace::remove_project,
            workspace::project_scope_get,
            workspace::project_scope_set,
            import::import_from_url,
            engine::map_status,
            views::workspace_deps,
            engine::scan_languages,
            import::list_github_repos,
            engine::engine_info,
            engine::engine_languages,
            engine::grammar_dir,
            engine::set_engine,
            engine::set_grammars,
            engine::generate,
            engine::check_map,
            nvim::nvim_info,
            nvim::set_nvim_path,
            nvim::set_nvim_config_dir,
            nvim::import_from_nvim_config,
            engine::serve_project,
            views::set_menu,
            views::open_map_in_browser,
            views::reveal_project,
            views::reveal_settings,
            views::open_docs,
            views::search_docs,
            views::set_zoom,
            views::set_window_title,
            views::open_feedback,
            views::about_info,
            views::map_freshness,
            views::map_changes,
            views::project_stats,
            views::project_search,
            views::view_search,
            views::project_icon,
            paths::open_in_editor,
            views::file_tree,
            workspace::list_workspaces,
            workspace::switch_workspace,
            workspace::rename_workspace,
            workspace::delete_workspace,
            nvim::editor_command,
            nvim::telemetry_info,
            nvim::set_telemetry,
            traffic_cmds::traffic_info,
            traffic_cmds::traffic_detail,
            traffic_cmds::traffic_list,
            traffic_cmds::traffic_settings,
            traffic_cmds::traffic_set_dir,
            traffic_cmds::traffic_ask_neovim,
            traffic_cmds::traffic_set_hidden,
            traffic_cmds::traffic_refresh,
            views::save_text,
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
    fn a_write_does_not_follow_what_lies_at_the_old_temp_name() {
        let dir = crate::testutil::fresh_dir("docmap-write-atomic-tmp");
        let target = dir.join("a.json");
        let tmp = dir.join("a.json.tmp");
        fs::write(&tmp, "someone else's").unwrap();
        write_atomic(&target, "mine").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "mine");
        assert_eq!(fs::read_to_string(&tmp).unwrap(), "someone else's");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2, "no temp file stays");
    }

    #[test]
    fn two_writers_of_one_file_do_not_meet() {
        let dir = crate::testutil::fresh_dir("docmap-write-atomic-two");
        let target = dir.join("a.json");
        let threads: Vec<_> = (0..2)
            .map(|n| {
                let target = target.clone();
                std::thread::spawn(move || {
                    let body = vec![b'a' + n; 50_000];
                    (0..100)
                        .filter(|_| write_atomic(&target, &body).is_err())
                        .count()
                })
            })
            .collect();
        let failed: usize = threads.into_iter().map(|t| t.join().unwrap()).sum();
        assert_eq!(failed, 0);
        let body = fs::read(&target).unwrap();
        assert_eq!(body.len(), 50_000);
        assert!(
            body.iter().all(|b| *b == body[0]),
            "one writer's whole text"
        );
    }

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

    /// The code of a source file outside its tests, without comment lines.
    fn production_code(source: &str) -> String {
        source
            .split("#[cfg(test)]")
            .next()
            .unwrap()
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // Opening and revealing start no `explorer.exe` of the app's own: the
    // shell is asked directly on Windows. Nothing here opens a window; what
    // is checked is that the old child is gone from the code and that a
    // target that is not there comes back as an error (`explorer <missing>`
    // started fine and opened Documents).
    #[test]
    fn opening_and_revealing_start_no_explorer() {
        for (name, source) in [
            ("views.rs", include_str!("views.rs")),
            ("paths.rs", include_str!("paths.rs")),
            ("shellopen.rs", include_str!("shellopen.rs")),
        ] {
            let code = production_code(source);
            assert!(!code.contains("\"explorer"), "{name} starts explorer");
            assert!(!code.contains("\"cmd"), "{name} starts cmd");
        }
        assert!(!production_code(include_str!("shellopen.rs")).contains("Command::new"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn a_target_that_is_not_there_is_an_error_of_the_app() {
        let dir = std::env::temp_dir().join(format!("docmap-open-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let missing = dir.join("Foo & Bar").join("file.txt");
        let missing = missing.to_string_lossy();

        let err = open_externally(&missing).unwrap_err();
        assert_eq!(err.key(), "err.open.failed");

        // A name that is not there is not completed to a program that is:
        // `notes` must not run `notes.bat`, which the shell would find.
        fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("ran.txt");
        fs::write(
            dir.join("notes.bat"),
            format!("@echo ran> \"{}\"\r\n", marker.display()),
        )
        .unwrap();
        let err = open_externally(&dir.join("notes").to_string_lossy()).unwrap_err();
        assert_eq!(err.key(), "err.open.failed");
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!marker.exists(), "the shell ran notes.bat for `notes`");
        assert!(is_web_address("HTTPS://github.com/x"));
        assert!(is_web_address("http://127.0.0.1:4000/index.html"));
        assert!(!is_web_address(r"C:\http://x"));
        assert!(!is_web_address("file:///C:/x.html"));
        let _ = fs::remove_dir_all(&dir);
        let err = reveal_in_file_manager(&missing).unwrap_err();
        assert_eq!(err.key(), "err.open.reveal");
    }

    // The editor command is the user's own and may be a terminal editor: it
    // gets a console of its own and is never started without a window, where
    // it would run unseen for good.
    #[test]
    fn the_editor_command_is_never_started_hidden() {
        let code = production_code(include_str!("paths.rs"));
        assert!(!code.contains("CREATE_NO_WINDOW"));
        assert!(!code.contains("DETACHED_PROCESS"));
        assert!(!code.contains("0x0800_0000"));
        #[cfg(windows)]
        {
            let flags = editor_creation_flags();
            assert_eq!(flags & 0x0800_0000, 0, "CREATE_NO_WINDOW");
            assert_eq!(flags & 0x0000_0008, 0, "DETACHED_PROCESS");
            assert_ne!(flags & 0x0000_0010, 0, "CREATE_NEW_CONSOLE");
            assert_ne!(flags & 0x0000_0200, 0, "CREATE_NEW_PROCESS_GROUP");
        }
        // A program that does not exist is an error, not a silent nothing.
        assert!(start_editor("docmap-no-such-editor-program", &[]).is_err());
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
    fn the_window_allows_no_inline_script_and_no_file_by_default() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let security = &conf["app"]["security"];
        let csp = security["csp"].as_str().expect("a policy");
        let scripts = csp
            .split(';')
            .find(|d| d.trim().starts_with("script-src"))
            .unwrap();
        assert_eq!(scripts.trim(), "script-src 'self'");
        for needed in [
            "object-src 'none'",
            "base-uri 'none'",
            "frame-src http://127.0.0.1:*",
        ] {
            assert!(csp.contains(needed), "{needed}");
        }
        assert_eq!(security["assetProtocol"]["scope"], serde_json::json!([]));
    }

    #[test]
    fn a_map_directory_outside_the_project_is_not_read() {
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
        fs::write(outer.join("maps").join("module_map.json"), "{}").unwrap();
        assert!(!map_dir_is_plain(&root, &dir("../maps")));
        assert!(!map_dir_is_plain(&root, &dir("../elsewhere/maps")));
        assert!(map_file(&root, &dir("../maps"), "module_map.json").is_none());
        // Another path altogether is not the project's either.
        assert!(!map_dir_is_plain(&root, &outer.join("maps")));
        assert!(dir_link(&outer.join("linked"), &outer.join("maps")));
        assert!(!map_dir_is_plain(&root, &dir("../linked")));
        // What the dialog says when it opens: an old value is refused, a good one is not.
        assert!(map_dir_refused("../maps".to_owned()));
        assert!(!map_dir_refused("docs/map".to_owned()));

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
    fn a_file_of_a_map_directory_outside_the_project_is_not_read() {
        let outer = fresh_dir("docmap-map-file-outside");
        let project = outer.join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(outer.join("maps")).unwrap();
        fs::write(outer.join("maps").join("module_map.json"), "{}").unwrap();
        let root_str = portable(&project);
        let root = PathBuf::from(&root_str);
        let map = PathBuf::from(format!("{root_str}/../maps"));
        assert!(map_file(&root, &map, "module_map.json").is_none());
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

    fn nvim_entry(name: &str, dir: &Path) -> NvimProjectEntry {
        NvimProjectEntry {
            name: name.to_string(),
            repo: format!("owner/{name}"),
            dir: dir.to_string_lossy().to_string(),
        }
    }

    #[test]
    fn nvim_import_adds_the_config_itself_and_its_plugins() {
        let config = scratch("nvim-import-config");
        let plugin = scratch("nvim-import-plugin");
        let mut ws = Workspace::default();

        let res = import_entries(
            &mut ws,
            Some(&config.to_string_lossy()),
            &[nvim_entry("plugin.nvim", &plugin)],
        );

        assert_eq!(res.found, 2, "the config counts as one of what was found");
        assert_eq!(res.added.len(), 2);
        assert_eq!(res.already_present, 0);
        assert!(res.errors.is_empty());
        assert_eq!(ws.projects.len(), 2);
    }

    #[test]
    fn nvim_import_with_nothing_found_still_adds_the_config() {
        // An export that lists no checkout (`{}, nil` in the config's
        // contract) is an answer, not a failure - and the config is still a
        // project of its own.
        let config = scratch("nvim-import-empty");
        let mut ws = Workspace::default();

        let res = import_entries(&mut ws, Some(&config.to_string_lossy()), &[]);
        assert_eq!((res.found, res.added.len()), (1, 1));

        let mut empty = Workspace::default();
        let none = import_entries(&mut empty, None, &[]);
        assert_eq!(
            (none.found, none.added.len(), none.already_present),
            (0, 0, 0)
        );
        assert!(empty.projects.is_empty());
    }

    #[test]
    fn nvim_import_twice_counts_everything_as_already_present() {
        let config = scratch("nvim-import-twice-config");
        let plugin = scratch("nvim-import-twice-plugin");
        let missing = scratch("nvim-import-twice-missing");
        fs::remove_dir_all(&missing).unwrap();
        let entries = [
            nvim_entry("plugin.nvim", &plugin),
            nvim_entry("missing.nvim", &missing),
        ];
        let mut ws = Workspace::default();
        let config = config.to_string_lossy().to_string();

        let first = import_entries(&mut ws, Some(&config), &entries);
        assert_eq!(first.added.len(), 2);
        assert_eq!(first.errors.len(), 1);
        assert_eq!(first.errors[0].name, "missing.nvim");

        let second = import_entries(&mut ws, Some(&config), &entries);
        assert!(second.added.is_empty(), "nothing is added twice");
        assert_eq!(second.already_present, 2);
        assert_eq!(
            second.errors.len(),
            1,
            "the missing one still fails, by name"
        );
        assert_eq!(ws.projects.len(), 2);
    }

    #[test]
    fn nvim_export_output_parses_with_and_without_a_banner() {
        let json = r#"[{"repo":"o/a.nvim","name":"a.nvim","dir":"E:/repos/a.nvim"}]"#;
        let plain = parse_nvim_export(&format!("{json}\n")).unwrap();
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0].dir, "E:/repos/a.nvim");

        let banner = parse_nvim_export(&format!("lazy: 3 updates\n{json}\n")).unwrap();
        assert_eq!(banner[0].name, "a.nvim");
        // A banner that itself starts with `[` must not be taken for the JSON.
        let bracket = parse_nvim_export(&format!("[lazy] 3 updates\r\n{json}\r\n\r\n")).unwrap();
        assert_eq!(bracket[0].name, "a.nvim");

        assert!(parse_nvim_export("[]").unwrap().is_empty());
        let err = parse_nvim_export("not json").unwrap_err();
        assert_eq!(err.key(), "err.nvim.notJson");
    }

    #[test]
    fn nvim_export_needs_no_script_file_in_the_config() {
        // The import broke once because the script it ran lived in the nvim
        // config and was deleted there. The chunk is now this program's own.
        let cmd = nvim_export_command("nvim");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(args[0], "--headless");
        assert!(args.iter().all(|a| !a.contains("luafile")));
        assert!(args[2].starts_with("lua "));
        assert!(args[2].contains("plugins.personal.core.export"));
        assert!(!args[2].contains('"'), "one -c argument, no double quotes");
        assert_eq!(args.last().map(String::as_str), Some("qa"));
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
            .build(tauri::generate_context!())
            .expect("failed to build mock app from this crate's real tauri.conf.json");
        let handle = app.handle();

        // `engine_sidecar` resolves correctly even run from here: a
        // `cargo test` binary lives in `target/debug/deps/`, and the lookup
        // goes up one level to `target/debug/`, where the sidecar is staged.
        let sidecar = engine_sidecar();
        assert!(
            sidecar.is_some(),
            "expected src-tauri/binaries/docmap-<this platform's target triple> to resolve              as a real sidecar file -- see README.md, 'The engine'"
        );
        let program = sidecar.unwrap();
        assert!(
            program.is_file(),
            "resolved sidecar path {program:?} is not a real file"
        );

        // `resolve_grammars` goes through `app.path().resource_dir()`
        // instead, which has no equivalent `deps/`-awareness -- measured
        // directly (`docmap-desktop.exe --debug-resolve-and-exit`, a
        // one-off probe, not kept): the real compiled app resolves both
        // correctly from `target/debug/`, and only a `cargo test` binary
        // itself (running from `target/debug/deps/`) sees the offset. So
        // this recomputes the same "if I'm in `deps/`, my sibling files
        // are one level up" adjustment `sidecar_beside` already applies
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

    #[test]
    fn the_sidecar_is_looked_up_beside_the_executable() {
        let dir = std::env::temp_dir().join(format!("docmap-sidecar-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let deps = dir.join("deps");
        let other = dir.join("other");
        fs::create_dir_all(&deps).unwrap();
        let name = if cfg!(windows) {
            "docmap.exe"
        } else {
            "docmap"
        };
        let exe = dir.join("docmap-desktop.exe");
        assert_eq!(sidecar_beside(&exe), None, "nothing staged, nothing found");

        fs::write(dir.join(name), b"").unwrap();
        assert_eq!(sidecar_beside(&exe), Some(dir.join(name)));

        // A test binary runs from `deps/`, one level below the staged file.
        assert_eq!(
            sidecar_beside(&deps.join("docmap_desktop-0123.exe")),
            Some(dir.join(name))
        );

        // A directory of that name is not an engine.
        fs::create_dir_all(other.join(name)).unwrap();
        assert_eq!(sidecar_beside(&other.join("app.exe")), None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// The page starts no process: the engine is started by the backend
    /// alone, so no capability may grant the window anything of a shell
    /// plugin, and the plugin itself is not a dependency.
    #[test]
    fn no_capability_grants_the_page_a_shell() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut seen = 0;
        for entry in fs::read_dir(manifest.join("capabilities")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            seen += 1;
            let text = fs::read_to_string(&path).unwrap();
            assert!(
                !text.contains("shell:"),
                "{path:?} names a shell permission"
            );
        }
        assert!(seen > 0, "no capability file was read");
        let cargo = fs::read_to_string(manifest.join("Cargo.toml")).unwrap();
        assert!(
            !cargo.contains("tauri-plugin-shell"),
            "Cargo.toml depends on tauri-plugin-shell again"
        );
    }
}
