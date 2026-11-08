//! Paths inside a project: resolving below a root without leaving it, link walking, the map file, and opening a
//! file in the file manager or an editor.

use super::*;

/// `rel` resolved inside `root`, or an error — checked **before** the
/// filesystem is touched.
///
/// `root` must already be canonical. `Path::join` replaces the base when the
/// right-hand side is absolute, so a `rel` such as `\\host\share\x` becomes
/// that UNC path; `fs::canonicalize` then opens it, which on Windows makes the
/// machine connect to that host with the user's credentials (an NTLM hash
/// leak) and blocks for as long as the connection takes — and only after that
/// did the old `starts_with(root)` check say no. A path that arrives from a
/// map page, a search hit or a text box is therefore refused on its *shape*
/// first: no drive or UNC prefix, no leading separator. `..` is allowed to
/// pass here because the containment check below catches anything that really
/// leaves the root, and `a/../b` inside it is a legitimate path.
///
/// An empty `rel` is the root itself, which is what a folder search over the
/// whole project asks for.
pub(crate) fn resolve_inside(root: &Path, rel: &str) -> Result<PathBuf, AppError> {
    let mut steps = WALK_STEPS;
    resolve_inside_budgeted(root, rel, &mut steps)
}

/// [`resolve_inside`] with the work of the link walk (one `lstat` per component
/// looked at, the re-walk after every link hop included) drawn from `steps`.
///
/// The caps on `rel` do not bound what a *link* costs: its target is walked
/// again from the link's directory, and a target can name thousands of real
/// levels. A caller that resolves many paths on behalf of one request (the icon
/// lookup tries some seventy fixed names and up to 256 manifest entries) shares
/// one budget, so the request is bounded rather than each path. Spent, the walk
/// answers "too many links".
pub(crate) fn resolve_inside_budgeted(
    root: &Path,
    rel: &str,
    steps: &mut u32,
) -> Result<PathBuf, AppError> {
    #[cfg(not(windows))]
    let _ = &steps;
    if rel.contains('\0') {
        return Err(AppError::new("err.path.nul"));
    }
    // No path the OS accepts is longer, and `rel` can come out of a repository's
    // manifest at up to a megabyte.
    if rel.len() > MAX_REL_LEN {
        return Err(AppError::new("err.path.tooLong"));
    }
    let relative = Path::new(rel);
    if relative
        .components()
        .any(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
    {
        return Err(path_key("err.path.notInside", rel));
    }
    // Collapsed lexically, once, and *this* is the path that is examined and
    // then opened. `Path::join` on a verbatim (`\\?\`) root drops `.` and `..`
    // before the OS sees the path, and Win32 does the same to any other: an
    // examination that walked the components as written (`missing/../l`)
    // stopped at the first one that does not exist and never looked at `l`,
    // which is what `canonicalize` then opened.
    //
    // Built relative to nothing and joined to the root once: `push` onto a
    // verbatim (`\\?\`) path rebuilds the whole buffer, so pushing each name
    // onto the root made the loop quadratic - a manifest `src` of half a
    // million `a/` kept a thread busy for over an hour.
    let mut collapsed = PathBuf::new();
    let mut depth = 0usize;
    for part in relative.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if depth == 0 {
                    return Err(path_key("err.path.outside", rel));
                }
                collapsed.pop();
                depth -= 1;
            }
            Component::Normal(name) => {
                // `push` re-parses its argument: `C:l.png` has a drive prefix,
                // so it *replaces* everything pushed so far, and what is then
                // examined is a drive-relative path against the process's
                // current directory, not a path under the root.
                if reparsed_by_push(name) {
                    return Err(path_key("err.path.notInside", rel));
                }
                // The root is a canonical, verbatim path, so `evil.` is looked
                // at as itself here; handed on without the prefix (as the
                // icon path is, to the asset protocol, and the editor's) Win32
                // reads it as `evil` - through a link, if there is one beside it.
                if crate::languages::win32_rewrites_name(name) {
                    return Err(path_key("err.path.notInside", rel));
                }
                collapsed.push(name);
                depth += 1;
                // Looking at each prefix of an existing path costs a walk of
                // the whole prefix, so a path of thousands of real levels is
                // quadratic work in the kernel. No project has one.
                if depth > MAX_REL_COMPONENTS {
                    return Err(AppError::new("err.path.tooLong"));
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(path_key("err.path.notInside", rel));
            }
        }
    }
    let clean = if depth == 0 {
        root.to_path_buf()
    } else {
        root.join(&collapsed)
    };
    // Whatever else went wrong above, nothing past this point looks at a path
    // that is not under the root.
    if !clean.starts_with(root) {
        return Err(path_key("err.path.outside", rel));
    }
    #[cfg(windows)]
    match links_to_network(&clean, root.components().count(), &mut 16, steps) {
        LinkWalk::Local => {}
        LinkWalk::Network => return Err(path_key("err.path.network", rel)),
        LinkWalk::TooDeep => return Err(path_key("err.path.tooManyLinks", rel)),
        LinkWalk::Unusable => return Err(path_key("err.path.unsafeLink", rel)),
    }
    let target = fs::canonicalize(&clean).map_err(|_| path_key("err.path.missing", rel))?;
    if !target.starts_with(root) {
        return Err(path_key("err.path.outside", rel));
    }
    // What is handed on is the result, and a link can have led it through a
    // name Win32 reads as another (`a/l -> evil./f`): the check on `rel` does not
    // see that one. The root is the user's own and is left out of it.
    if target.strip_prefix(root).is_ok_and(|inside| {
        inside
            .components()
            .any(|c| matches!(c, Component::Normal(n) if crate::languages::win32_rewrites_name(n)))
    }) {
        return Err(path_key("err.path.notInside", rel));
    }
    Ok(target)
}

/// Would `PathBuf::push(name)` treat this one component as more than a name?
///
/// On Windows `C:x` is a drive-relative path and replaces the buffer; `a:b` is
/// also an NTFS stream spelling, and no file name contains a colon there. On
/// other systems a component is always just a name.
pub(crate) fn reparsed_by_push(name: &std::ffi::OsStr) -> bool {
    (cfg!(windows) && name.to_string_lossy().contains(':'))
        || Path::new(name).components().next() != Some(Component::Normal(name))
}

/// Is the map directory a plain directory on the way from the project root?
///
/// `docs/map` lies inside the project, so it is repository content: checked
/// out as a link it makes every reader of "the map" read somewhere else — a
/// share (`map_freshness` stats it for every project at start-up), the
/// repository root (`.env` next to a served `index.html`), a home directory.
/// No component between the root and the map directory may be a link, and none
/// is followed to find out (`symlink_metadata` does not, and reports a Windows
/// junction as a link too).
///
/// A map directory that is not under the root (`../maps`, saved in Project
/// settings before they refused it) is no: the engine never writes there, so
/// nothing the app reads should come from there. `..` in it is walked as the OS
/// does, so one that comes back into the project is the project's own.
///
/// A component that is not there yet is fine - there is nothing to read - but
/// the walk goes on past it, because a later `..` can lead back to one that is
/// (`dist/../docs/map`). Any other failure to look is a no.
///
/// This vets the *directory*. A file inside it can be a link as well: read it
/// through [`map_file`].
pub(crate) fn map_dir_is_plain(root: &Path, map_dir: &Path) -> bool {
    let Ok(rel) = map_dir.strip_prefix(root) else {
        return false;
    };
    let Ok(canon_root) = fs::canonicalize(root) else {
        return false;
    };
    let mut walked = canon_root.clone();
    for part in rel.components() {
        match part {
            Component::Normal(name) => {
                if reparsed_by_push(name) {
                    return false;
                }
                // Win32 drops a trailing dot or space before the file system
                // sees the name, so `map.` is `map` - which the verbatim path
                // walked here would report as missing.
                if languages::win32_rewrites_name(name) {
                    return false;
                }
                walked.push(name);
            }
            Component::CurDir => continue,
            Component::ParentDir => {
                walked.pop();
                continue;
            }
            _ => return false,
        }
        if !walked.starts_with(&canon_root) {
            continue;
        }
        match fs::symlink_metadata(&walked) {
            Ok(meta) if meta.file_type().is_symlink() => return false,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return false,
        }
    }
    walked.starts_with(&canon_root)
}

/// Where to read `name` - a file directly in the map directory - or `None`
/// when it must not be read: the directory is not plain, or the file is not
/// there, or it is a link that leaves the project or leads to another machine.
///
/// [`map_dir_is_plain`] covers the directory; a repository can just as well
/// commit `docs/map/module_map.json` as a link to `\\host\share\x`, and the
/// first `stat`, `is_file` or `canonicalize` of it would connect to that host
/// (a stall of 20 seconds and an NTLM negotiation) before any check on the
/// *result* could say no. Under the project root the file is therefore
/// resolved with [`resolve_inside`], which walks its links without following
/// them; a link that stays in the project still works.
pub(crate) fn map_file(root: &Path, map_dir: &Path, name: &str) -> Option<PathBuf> {
    if !map_dir_is_plain(root, map_dir) {
        return None;
    }
    let rel = map_dir.strip_prefix(root).ok()?;
    // Collapsed the way `map_dir_is_plain` walks it, on the canonical root: a
    // `..` that climbs out puts the directory outside the project, but one that
    // comes back in (`../<project>/docs/map`) is still the project's own.
    let canon_root = fs::canonicalize(root).ok()?;
    let mut walked = canon_root.clone();
    for part in rel.components() {
        match part {
            Component::Normal(n) => walked.push(n),
            Component::ParentDir => {
                walked.pop();
            }
            _ => {}
        }
    }
    let inside = walked.strip_prefix(&canon_root).ok()?;
    let mut rel_file = portable(inside);
    if !rel_file.is_empty() {
        rel_file.push('/');
    }
    rel_file.push_str(name);
    resolve_inside(&canon_root, &rel_file).ok()
}

/// The longest relative path [`resolve_inside`] looks at, in bytes and in
/// components.
pub(crate) const MAX_REL_LEN: usize = 32 * 1024;
pub(crate) const MAX_REL_COMPONENTS: usize = 256;

/// What one [`resolve_inside`] may spend on its link walk: components looked
/// at, over the whole chain of links. A real path needs a few dozen.
///
/// A step is not a constant cost: each look makes the kernel walk the whole
/// prefix, so what the budget bounds is about N^2/2 component lookups. 512
/// keeps the worst case under a second.
pub(crate) const WALK_STEPS: u32 = 512;
/// The same for one entry of the folder picker.
pub(crate) const PICKER_WALK_STEPS: u32 = 256;
// The longest relative path must fit in it with room for the links on the way.
const _: () = assert!(WALK_STEPS as usize >= MAX_REL_COMPONENTS + 64);
const _: () = assert!(PICKER_WALK_STEPS <= WALK_STEPS);

/// Does the link at `path` - an entry below `root`, which is not inspected -
/// stay on this machine? Always so off Windows, where a link cannot name a
/// share.
pub(crate) fn link_stays_local(root: &Path, path: &Path) -> bool {
    #[cfg(windows)]
    {
        let mut steps = PICKER_WALK_STEPS;
        matches!(
            links_to_network(path, root.components().count(), &mut 16, &mut steps),
            LinkWalk::Local
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (root, path);
        true
    }
}

/// What a walk along a path found out about the links on it.
#[cfg(windows)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LinkWalk {
    /// No link on the path leads off this machine.
    Local,
    /// A link leads to another machine (a share, a device namespace).
    Network,
    /// More links than any real path has, or a loop.
    TooDeep,
    /// A link whose target could not be read, or a component `push` would
    /// misread: not followed, because nothing can be said about it.
    Unusable,
}

/// Does `path` — walked from its start, component by component — go through a
/// symlink or junction whose target is on another machine?
///
/// The shape check refuses a UNC *string*; a link checked out inside the
/// project that points at one passes it, and `canonicalize` would connect to
/// the host before the containment check said no. So every component is
/// inspected with `symlink_metadata`, which does not follow, and only after
/// everything before it has been shown not to be such a link — nothing is
/// followed until it has been vetted.
///
/// **A link target is local only if it provably is.** `read_link` hands back
/// every NT-style target as a verbatim path — `\\.\GLOBALROOT\Device\Mup\host`
/// reads as `\\?\GLOBALROOT\...` and `\\.\pipe\x` as `\\?\pipe\x` — so a list of
/// the *remote* spellings (UNC, device namespace) misses the ones that reach a
/// share through the kernel's own redirector. A drive (`C:`, `\\?\C:`) or a
/// volume mount point (`\\?\Volume{...}`) is local; any other prefix is not.
///
/// When a component *is* a link, what remains of the path continues from the
/// link's target, and that whole path is walked the same way (a link whose
/// target passes through another link is the case a single look at the last
/// component misses). `budget` is shared across the chain. The first `skip`
/// components are not inspected — the canonical project root has no links.
#[cfg(windows)]
pub(crate) fn links_to_network(
    path: &Path,
    skip: usize,
    budget: &mut u8,
    steps: &mut u32,
) -> LinkWalk {
    use std::path::Prefix;

    let parts: Vec<Component> = path.components().collect();
    let mut walked = PathBuf::new();
    for (i, part) in parts.iter().enumerate() {
        match part {
            Component::CurDir => continue,
            Component::ParentDir => {
                walked.pop();
                continue;
            }
            // A drive or share prefix and the root are only built up, never
            // statted on their own: `\\?\C:` is not a path `symlink_metadata`
            // answers for, and an early "not there" here ended the walk before
            // it reached a single real component.
            Component::Prefix(_) | Component::RootDir => {
                walked.push(part.as_os_str());
                continue;
            }
            Component::Normal(name) => {
                if reparsed_by_push(name) {
                    return LinkWalk::Unusable;
                }
                walked.push(name);
            }
        }
        if i < skip {
            continue;
        }
        // A name Win32 reads as another one (`evil.` as `evil`): where it leads
        // depends on whoever opens the path, not on this walk. A name that
        // reaches here beyond the root comes from the path asked for or from a
        // link's target; both are the repository's.
        if matches!(part, Component::Normal(n) if crate::languages::win32_rewrites_name(n)) {
            return LinkWalk::Unusable;
        }
        // Each look costs the kernel a walk of the whole prefix, so a path of
        // thousands of real levels - or a link to one - is quadratic work.
        if *steps == 0 {
            return LinkWalk::TooDeep;
        }
        *steps -= 1;
        // Not there: `canonicalize` will say so, and nothing past it can be
        // reached.
        let Ok(meta) = fs::symlink_metadata(&walked) else {
            return LinkWalk::Local;
        };
        if !meta.file_type().is_symlink() {
            continue;
        }
        if *budget == 0 {
            return LinkWalk::TooDeep;
        }
        *budget -= 1;
        let Ok(target) = fs::read_link(&walked) else {
            return LinkWalk::Unusable;
        };
        if let Some(Component::Prefix(p)) = target.components().next() {
            let local = match p.kind() {
                Prefix::Disk(_) | Prefix::VerbatimDisk(_) => true,
                Prefix::Verbatim(name) => name
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("volume{"),
                _ => false,
            };
            if !local {
                return LinkWalk::Network;
            }
            // `C:x` names a drive and is relative to that drive's current
            // directory, which is the process's, not the link's.
            if !target.has_root() {
                return LinkWalk::Unusable;
            }
        } else if target.has_root() {
            // A root and no drive (`\foo`, or a raw NT path such as
            // `\Device\Mup\host\share\x` written into the reparse data by a
            // tool that does not go through the Win32 API): where it leads
            // depends on the kernel, not on this walk, and the redirector is
            // reachable by that spelling. Not local unless it provably is.
            return LinkWalk::Unusable;
        }
        // A relative target continues from the link's directory, and what it
        // shares with that directory has just been looked at and is plain: it
        // is not looked at again (each hop restarted from the drive, which made
        // a chain of links at the bottom of a deep tree cost the depth once per
        // hop). An absolute one can lead anywhere and is walked from its start.
        //
        // What is shared is counted, not assumed to be the whole directory:
        // `join` onto a verbatim path folds a `..` away, so `a\l -> ..\x` gives
        // `root\x`, no longer than `root\a`, and `x` - never looked at - would
        // sit inside a prefix counted as vetted.
        let (base, vetted) = if target.is_absolute() {
            (target, 0)
        } else {
            let parent = walked.parent().unwrap_or(&walked);
            let base = parent.join(target);
            let shared = parent
                .components()
                .zip(base.components())
                .take_while(|(a, b)| a == b)
                .count();
            (base, shared)
        };
        let rest: PathBuf = parts[i + 1..].iter().collect();
        return links_to_network(&base.join(rest), vetted, budget, steps);
    }
    LinkWalk::Local
}

/// A path as the desktop's own tools want it: the platform's separators and
/// no `\\?\` verbatim prefix.
///
/// [`portable`] is for display and for the editor template's `{file}`;
/// Explorer does not parse a forward-slash path — it opens the Documents
/// folder instead, measured on Windows 11 — so what is handed to it is this.
pub(crate) fn native_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
    }
    #[cfg(not(windows))]
    {
        text.into_owned()
    }
}

/// A stored forward-slash path (a project root) in the platform's separators.
pub(crate) fn native_str(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

/// Extensions whose "open" is to show the text, on every desktop this runs on.
///
/// A short list on purpose, and **not** the language table: `.js` is run by
/// Windows Script Host, `.py`/`.rb`/`.pl` by an installed interpreter, `.ps1`,
/// `.bat`, `.cmd`, `.sh`, `.command` by the shell, `.html`/`.svg` by a browser
/// with `file://` rights. What is opened for the reader comes from search hits,
/// a map's own file list and a message the embedded page can post — all of it
/// text from a repository somebody else wrote — so "open" must not mean "run".
pub(crate) const INERT_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mdx", "txt", "rst", "adoc", "json", "jsonc", "toml", "yaml", "yml", "log",
    "ini", "cfg", "conf", "lock", "lua", "vim", "rs", "c", "h", "cc", "cpp", "cxx", "hpp", "go",
    "java", "kt", "swift", "dart", "zig", "css", "scss", "less", "sql", "ex", "exs", "erl", "hs",
    "ml", "scala", "clj", "el",
];

/// Whether a file starts like something the system would execute: a shebang,
/// or the magic number of an ELF, Mach-O, fat Mach-O or PE binary.
#[cfg(unix)]
pub(crate) fn looks_like_program(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 4];
    let Ok(mut file) = fs::File::open(path) else {
        // Unreadable and executable: not worth the benefit of the doubt.
        return true;
    };
    let n = file.read(&mut head).unwrap_or(0);
    let head = &head[..n];
    head.starts_with(b"#!")
        || head.starts_with(b"\x7fELF")
        || head.starts_with(b"MZ")
        || matches!(
            head,
            [0xCF, 0xFA, 0xED, 0xFE]
                | [0xCE, 0xFA, 0xED, 0xFE]
                | [0xFE, 0xED, 0xFA, 0xCE]
                | [0xFE, 0xED, 0xFA, 0xCF]
                | [0xCA, 0xFE, 0xBA, 0xBE]
        )
}

/// Whether handing `path` to the desktop's file association is safe.
pub(crate) fn is_inert_document(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Marked executable *and* looks like a program: a script or a binary
        // is a program whatever it is called. The bit alone is not evidence —
        // FAT, exFAT and NTFS volumes, and some network shares, report every
        // file as executable, and refusing all of them would make "open" show
        // the file manager for every README on such a drive.
        if meta.permissions().mode() & 0o111 != 0 && looks_like_program(path) {
            return false;
        }
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| INERT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Show `native` in the desktop's file manager instead of opening it.
pub(crate) fn reveal_in_file_manager(native: &str) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("explorer");
        // `raw_arg`: Explorer wants `/select,"<path>"` with the quotes around
        // the path only; a whole-argument quote (what `arg` would add for a
        // path with a space) makes it ignore the selection.
        c.raw_arg(format!("/select,\"{native}\""));
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg("-R").arg(native);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        // `xdg-open` has no "select": the folder it is in is the nearest thing.
        let folder = Path::new(native)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| native.to_string());
        let mut c = std::process::Command::new("xdg-open");
        c.arg(folder);
        c
    };

    cmd.spawn().map(|_| ()).map_err(|e| {
        AppError::new("err.open.reveal")
            .with("path", native)
            .detail(e)
    })
}

/// Open a source file in an editor, at a line where one is known.
///
/// The path arrives repo-relative from the map page, because that is what
/// the artifact stores; it is resolved against the project it belongs to
/// here rather than there. The page has no idea where the repository sits
/// on this machine, and should not.
///
/// **The resolved path must stay inside the project.** The message comes
/// from a document this app embeds but does not author — a map generated
/// by an older engine, or one someone else produced — and `../../` in a
/// path is the difference between opening a file and opening any file.
#[tauri::command(async)]
pub(crate) fn open_in_editor(
    app: tauri::AppHandle,
    id: String,
    path: String,
    line: Option<u64>,
) -> Result<(), AppError> {
    let ws = read_workspace(&app)?;
    let project = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;

    let root = fs::canonicalize(&project.root).map_err(|e| {
        AppError::new("err.path.resolve")
            .with("path", &project.root)
            .detail(e)
    })?;
    if path.trim().is_empty() {
        return Err(AppError::new("err.editor.noFile"));
    }
    let target = resolve_inside(&root, &path)?;
    let file = portable(&target);

    let template = match ws.editor.as_deref() {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        // No template configured: hand it to the desktop — but only a file
        // whose "open" is to show it. An executable, a script or an archive
        // is shown in the file manager instead of being run.
        _ => {
            let native = native_path(&target);
            return if is_inert_document(&target) {
                open_externally(&native)
            } else {
                reveal_in_file_manager(&native)
            };
        }
    };

    // Split before substituting, so a path with a space in it stays one
    // argument rather than becoming two — `C:/Program Files/...` is the
    // normal case on this platform, not the exotic one.
    let mut parts = template.split_whitespace().map(|p| {
        p.replace("{file}", &file)
            .replace("{line}", &line.unwrap_or(1).to_string())
    });
    let program = parts
        .next()
        .ok_or_else(|| AppError::new("err.editor.empty"))?;
    let args: Vec<String> = parts.collect();

    let mut cmd = std::process::Command::new(&program);
    cmd.args(&args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().map(|_| ()).map_err(|e| {
        AppError::new("err.proc.run")
            .with("program", &program)
            .detail(e)
    })
}
