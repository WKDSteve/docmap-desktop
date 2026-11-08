//! Adding projects: a folder with its sub-repositories, a clone from a URL, the GitHub list.

use super::*;

/// One candidate directory found under a folder someone pointed at, before
/// it becomes a `Project`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubRepo {
    pub(crate) name: String,
    pub(crate) path: String,
    /// Has a `.git` entry — a plain clone or a worktree, either is a file or
    /// a directory named `.git`. Informational only: a directory without one
    /// is still offered, because not everything worth mapping is a checkout.
    pub(crate) is_git: bool,
    pub(crate) already_added: bool,
}

/// What picking a single folder in the add dialog needs to know before it
/// decides between "add this one" and "list what is inside it".
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FolderScan {
    /// The folder itself has a `.git` entry, so it is a project on its own
    /// rather than a container of several.
    pub(crate) is_git: bool,
    /// Immediate subdirectories, alphabetical. Hidden ones (leading `.`) are
    /// left out — `.git` itself included — the same as everything else in
    /// this program's own directory listings.
    pub(crate) subrepos: Vec<SubRepo>,
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
pub(crate) fn list_subdirs(root: &Path) -> Result<Vec<(String, String, bool)>, AppError> {
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
pub(crate) fn inspect_folder(app: tauri::AppHandle, root: String) -> Result<FolderScan, AppError> {
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
pub(crate) fn import_many(
    app: tauri::AppHandle,
    roots: Vec<String>,
) -> Result<ImportResult, AppError> {
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

/// Where cloned repositories land. A subdirectory of the same app-config
/// directory `workspace.json` lives in, not the system temp dir: a clone
/// this app made is something to keep and reopen, not scratch space.
pub(crate) fn repos_cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
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
pub(crate) const CLONE_PROTOCOLS: &str = "https:http:ssh:git";

/// Why `url` may not be handed to `git clone`, or `None` when it may.
///
/// The URL is user input that becomes an argument of another program, so the
/// shape is decided here and not left to git: a URL that starts with `-` is
/// read by git as an option (`--upload-pack=…` runs a command), and a
/// `<transport>::<address>` URL selects a remote helper. What stays allowed is
/// what a user pastes from a hosting page: `https://`, `http://`, `ssh://`,
/// `git://` and the scp form `user@host:path`.
pub(crate) fn clone_url_problem(url: &str) -> Option<&'static str> {
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
pub(crate) fn repo_dir_name(url: &str) -> Option<String> {
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
pub(crate) const CASE_INSENSITIVE_HOSTS: [&str; 4] =
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
pub(crate) fn clone_url_key(url: &str) -> String {
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
pub(crate) fn clone_dir_name(url: &str) -> Option<String> {
    let key = clone_url_key(url);
    let name = repo_dir_name(&key)?;
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    Some(format!("{name}-{:08x}", hash >> 32))
}

/// Where an import of `url` goes, decided before anything is cloned.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CloneTarget {
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
pub(crate) fn clone_target(cache: &Path, url: &str) -> Result<CloneTarget, AppError> {
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
pub(crate) const CLONE_DIR_SLOTS: u32 = 16;

/// The sidebar name of a project imported from `url`: the repository name as
/// the URL spells it (`foo`, not the clone folder `foo-c2edc6b0`), or
/// `owner/foo` when a project of that name is already listed, so two
/// repositories of one name can be told apart. `None` leaves the folder name.
pub(crate) fn imported_name(url: &str, projects: &[Project]) -> Option<String> {
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
pub(crate) fn origin_url(dir: &Path) -> Option<String> {
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
pub(crate) fn is_windows_device_name(name: &str) -> bool {
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
pub(crate) fn add_imported(
    app: &tauri::AppHandle,
    dir: &Path,
    url: &str,
) -> Result<Vec<Project>, AppError> {
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
pub(crate) async fn import_from_url(
    app: tauri::AppHandle,
    url: String,
) -> Result<Vec<Project>, AppError> {
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

/// The signed-in user's GitHub repositories, for the URL tab's picker.
///
/// Delegated to `gh` rather than the API on purpose — see `github.rs` for
/// why this program must not hold a credential. Never called when the dialog
/// opens: listing repositories is a network call against someone's account,
/// and a dialog that makes one just for being looked at is doing something
/// the reader did not ask for.
#[tauri::command]
pub(crate) async fn list_github_repos() -> github::RepoList {
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
