//! The views of a project (file tree, stats, search, dependencies, freshness) and the window commands.

use super::*;

/// Write text the page handed over to a path the reader chose.
///
/// The bytes come from the map page through its inbound channel, and the
/// path from a save dialog — so nothing is written anywhere nobody named.
/// The page's own `<a download>` still exists for a browser; inside this
/// window it would land wherever the webview decided, which is the reason
/// this route exists at all.
#[tauri::command]
pub(crate) fn save_text(path: String, contents: String) -> Result<(), AppError> {
    fs::write(&path, contents).map_err(|e| {
        AppError::new("err.path.write")
            .with("path", &path)
            .detail(e)
    })
}

/// One directory of a project, as it is on disk.
///
/// One level per call: a repository is tens of thousands of files and a
/// reader opens perhaps a dozen directories. See `filetree.rs` on why this
/// is read live here rather than collected into the artifact.
#[tauri::command(async)]
pub(crate) fn file_tree(
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
pub(crate) fn project_icon(app: tauri::AppHandle, id: String) -> Result<Option<String>, AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;
    let found = icon::find(Path::new(&project.root));
    if let Some(path) = &found {
        allow_asset(&app, path);
    }
    Ok(found.map(|p| portable(&p)))
}

/// Let the webview load this one file through the asset protocol. The static scope is empty: a page in the window could
/// otherwise read any file of the user, and only the icon of a project and the fallback of its map are ever shown so.
pub(crate) fn allow_asset(app: &tauri::AppHandle, path: &Path) {
    if let Err(e) = app.asset_protocol_scope().allow_file(path) {
        eprintln!("the asset scope refused a file: {e}");
    }
}

/// Whether a project's map has fallen behind its sources.
///
/// Cheap by construction — modification times, not a regeneration — so the
/// answer is "something was touched since", never "the map is wrong". See
/// `freshness.rs` on why the wording of the mark follows from that.
#[tauri::command(async)]
pub(crate) fn map_freshness(
    app: tauri::AppHandle,
    id: String,
) -> Result<freshness::Freshness, AppError> {
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
pub(crate) fn map_changes(
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
pub(crate) async fn project_stats(
    app: tauri::AppHandle,
    id: String,
) -> Result<stats::Stats, AppError> {
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
pub(crate) async fn project_search(
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
pub(crate) fn view_search(
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
pub(crate) fn workspace_deps(app: tauri::AppHandle) -> Result<deps::Deps, AppError> {
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
pub(crate) fn windows_open_command(target: &str) -> std::process::Command {
    let mut c = std::process::Command::new("explorer");
    c.arg(target);
    c
}

pub(crate) fn open_externally(target: &str) -> Result<(), AppError> {
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
pub(crate) fn open_map_in_browser(app: tauri::AppHandle, id: String) -> Result<String, AppError> {
    let url = serve_project(app, id)?;
    open_externally(&url)?;
    Ok(url)
}

/// The repository this app is built from: the root of every page below.
const REPO: &str = "https://github.com/WKDSteve/docmap-desktop";

/// The address of one of this project's own pages, by name, or `None`.
///
/// An allowlist, not a URL parameter. The webview asks for a *page* by name
/// and this decides what that means — so nothing the page can say turns
/// into a browser opening an arbitrary address, which is what a generic
/// `open_url` command would be.
pub(crate) fn docs_url(page: &str) -> Option<String> {
    let base = format!("{REPO}/blob/main/docs/");
    Some(match page {
        "usage" => format!("{base}USAGE.md"),
        // The single most common confusion this app produces: that the
        // engine is a separate program this is a window onto.
        "engine" => format!("{base}USAGE.md#the-engine-indicator"),
        // The Help menu and the About dialog of the other desktop apps name
        // the repository and the license; this one does the same.
        "repo" => REPO.to_string(),
        "license" => format!("{REPO}/blob/main/LICENSE"),
        _ => return None,
    })
}

/// Open one of this project's own documentation pages (see `docs_url`).
#[tauri::command]
pub(crate) fn open_docs(page: String) -> Result<(), AppError> {
    let target =
        docs_url(&page).ok_or_else(|| AppError::new("err.docs.page").with("page", &page))?;
    open_externally(&target)
}

/// Percent-encode everything but the unreserved characters of RFC 3986, so
/// a query is one parameter value and never a second parameter or a path.
pub(crate) fn encode_query(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The address of GitHub's search over this repository's Markdown (README
/// and `docs/`) for the words typed, or `None` for no words.
///
/// The words only ever land in the value of `q`, encoded, behind a fixed
/// repository: the page cannot make this open anything but a search here.
pub(crate) fn docs_search_url(query: &str) -> Option<String> {
    let words = query.trim();
    if words.is_empty() {
        return None;
    }
    let q = format!("repo:WKDSteve/docmap-desktop language:Markdown {words}");
    Some(format!(
        "https://github.com/search?type=code&q={}",
        encode_query(&q)
    ))
}

/// Search the documentation (Help → Search the documentation…) on GitHub.
#[tauri::command]
pub(crate) fn search_docs(query: String) -> Result<(), AppError> {
    let target = docs_search_url(&query).ok_or_else(|| AppError::new("err.docs.query"))?;
    open_externally(&target)
}

/// Show a project's root in the desktop's file manager.
///
/// Answers "where is this actually" without a path to copy out of a
/// tooltip. The repository root, not the map directory: the map is an
/// output, and the question is about the project.
#[tauri::command]
pub(crate) fn reveal_project(app: tauri::AppHandle, id: String) -> Result<(), AppError> {
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
pub(crate) fn reveal_settings(app: tauri::AppHandle) -> Result<(), AppError> {
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
pub(crate) fn open_feedback(
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
pub(crate) fn about_info(app: tauri::AppHandle) -> AboutInfo {
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
pub(crate) struct AboutInfo {
    pub(crate) app_version: String,
    pub(crate) os: String,
    pub(crate) arch: String,
    pub(crate) engine_path: Option<String>,
    pub(crate) grammars: Option<String>,
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
pub(crate) fn set_window_title(app: tauri::AppHandle, title: String) -> Result<(), AppError> {
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
pub(crate) fn set_zoom(app: tauri::AppHandle, factor: f64) -> Result<f64, AppError> {
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
pub(crate) fn set_menu(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_of_the_help_menu_are_an_allowlist_on_this_repository() {
        assert_eq!(
            docs_url("repo").as_deref(),
            Some("https://github.com/WKDSteve/docmap-desktop")
        );
        assert_eq!(
            docs_url("license").as_deref(),
            Some("https://github.com/WKDSteve/docmap-desktop/blob/main/LICENSE")
        );
        assert!(docs_url("usage").unwrap().ends_with("/docs/USAGE.md"));
        assert_eq!(docs_url("https://example.com"), None);
        assert_eq!(docs_url(""), None);
    }

    #[test]
    fn a_search_puts_the_words_into_the_value_of_q_and_nowhere_else() {
        let url = docs_search_url("  map & legend#x?y=1/..  ").unwrap();
        let (base, q) = url.split_once("&q=").unwrap();
        assert_eq!(base, "https://github.com/search?type=code");
        // Nothing the words hold can start another parameter, a fragment or a path.
        assert!(!q.contains(['&', '#', '?', '/', ' ', '=']), "{q}");
        assert!(
            q.starts_with(
                "repo%3AWKDSteve%2Fdocmap-desktop%20language%3AMarkdown%20map%20%26%20legend"
            ),
            "{q}"
        );
        assert!(q.ends_with("%23x%3Fy%3D1%2F.."), "{q}");
    }

    #[test]
    fn a_search_without_words_opens_nothing() {
        assert_eq!(docs_search_url(""), None);
        assert_eq!(docs_search_url("   "), None);
    }

    #[test]
    fn the_encoding_keeps_letters_of_other_scripts_as_their_utf8_bytes() {
        assert_eq!(encode_query("Größe"), "Gr%C3%B6%C3%9Fe");
        assert_eq!(encode_query("a-b_c.d~e"), "a-b_c.d~e");
    }
}
