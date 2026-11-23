//! Open the map at a file and line, asked from outside.
//!
//! `docmap-desktop --open-at <repo> <file>:<line>[-<last>]` (what tasks-desktop's "open in DocMap" starts) names a
//! project the app already knows and a file inside it. The window picks the request up once at start
//! (`take_open_request`), resolves it against the project's `module_map.json` (`map_artifact`, `src/lib/open-at.js`)
//! and shows the map at the node.
//!
//! The request is untrusted. The repository is only ever *matched* against the projects of the workspace (never
//! searched for on a disk), and the file is only ever a repository-relative path: no absolute path, no `~`, no
//! variable, no `..`. `src/lib/open-at-cases.json` holds this file and `open-at.js` to the same answers.

use std::sync::Mutex;

use serde::Serialize;

use crate::apperr::AppError;
use crate::workspace::{read_workspace, Project};

/// The flag a caller starts the app with.
pub const FLAG: &str = "--open-at";

/// What the window is asked to show: a known project and a place in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OpenAt {
    pub(crate) project: String,
    pub(crate) file: String,
    pub(crate) line: u32,
    pub(crate) end: Option<u32>,
}

/// The request found in the start arguments, until the window takes it.
#[derive(Default)]
pub(crate) struct Pending(Mutex<Option<Result<(String, String), AppError>>>);

impl Pending {
    /// `--open-at <repo> <target>` among `args` (the program name already dropped), if there is one.
    /// A flag without its two values is a request too: the window says what was wrong with it.
    pub(crate) fn from_args(args: &[String]) -> Pending {
        let Some(at) = args.iter().position(|a| a == FLAG) else {
            return Pending::default();
        };
        let request = match (args.get(at + 1), args.get(at + 2)) {
            (Some(repo), Some(target)) if !repo.starts_with("--") && !target.starts_with("--") => {
                Ok((repo.clone(), target.clone()))
            }
            _ => Err(AppError::new("err.open.args")),
        };
        Pending(Mutex::new(Some(request)))
    }

    fn take(&self) -> Option<Result<(String, String), AppError>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

/// A repository-relative, forward-slash path; refuses everything that could leave the repository.
pub(crate) fn relative_path(raw: &str) -> Result<String, AppError> {
    let refused = || AppError::new("err.open.path").with("path", raw);
    let first = raw.chars().next();
    if raw.is_empty()
        || raw.contains('\0')
        || raw.contains(':')
        || matches!(first, Some('/' | '\\' | '~' | '$' | '%'))
    {
        return Err(refused());
    }
    let mut parts = Vec::new();
    for part in raw.replace('\\', "/").split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(refused()),
            _ => parts.push(part.to_string()),
        }
    }
    if parts.is_empty() {
        return Err(refused());
    }
    Ok(parts.join("/"))
}

/// `file:line` or `file:first-last` (1-based).
pub(crate) fn parse_target(raw: &str) -> Result<(String, u32, Option<u32>), AppError> {
    let bad = || AppError::new("err.open.target").with("target", raw);
    let (file, range) = raw.rsplit_once(':').ok_or_else(bad)?;
    let (first, last) = match range.split_once('-') {
        Some((a, b)) => (a, Some(b)),
        None => (range, None),
    };
    let number = |s: &str| -> Option<u32> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse::<u32>().ok().filter(|n| *n >= 1)
    };
    let line = number(first).ok_or_else(bad)?;
    let end = match last {
        Some(l) => Some(number(l).ok_or_else(bad)?),
        None => None,
    };
    Ok((relative_path(file)?, line, end))
}

fn norm(s: &str) -> String {
    s.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// The known project `repo` means, by id, name or root. Never a path that is then read.
pub(crate) fn pick_project<'a>(
    projects: &'a [Project],
    repo: &str,
) -> Result<&'a Project, AppError> {
    let want = repo.trim();
    let unknown = || AppError::new("err.open.project").with("repo", repo);
    if want.is_empty() {
        return Err(unknown());
    }
    if let Some(p) = projects.iter().find(|p| p.id == want) {
        return Ok(p);
    }
    let named: Vec<&Project> = projects
        .iter()
        .filter(|p| p.name.to_lowercase() == want.to_lowercase())
        .collect();
    match named.as_slice() {
        [one] => return Ok(one),
        [] => {}
        _ => return Err(AppError::new("err.open.ambiguous").with("repo", repo)),
    }
    projects
        .iter()
        .find(|p| norm(&p.root) == norm(want))
        .ok_or_else(unknown)
}

/// The request of the start arguments, once: `None` when the app was started without one.
#[tauri::command]
pub(crate) fn take_open_request(
    app: tauri::AppHandle,
    pending: tauri::State<'_, Pending>,
) -> Result<Option<OpenAt>, AppError> {
    let Some(request) = pending.take() else {
        return Ok(None);
    };
    let (repo, target) = request?;
    let (file, line, end) = parse_target(&target)?;
    let ws = read_workspace(&app)?;
    let project = pick_project(&ws.projects, &repo)?;
    Ok(Some(OpenAt {
        project: project.id.clone(),
        file,
        line,
        end,
    }))
}

/// The parsed `module_map.json` of a project, for the window to resolve a position against.
#[tauri::command]
pub(crate) fn map_artifact(
    app: tauri::AppHandle,
    id: String,
) -> Result<serde_json::Value, AppError> {
    let ws = read_workspace(&app)?;
    let p = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(&id))?;
    let nomap = || AppError::new("err.open.nomap").with("repo", &p.name);
    let path = crate::paths::map_file(
        std::path::Path::new(&p.root),
        std::path::Path::new(&p.map_dir),
        "module_map.json",
    )
    .ok_or_else(nomap)?;
    let raw = crate::safe_read::read_text(&path, crate::safe_read::MAP_JSON_MAX, false)
        .ok_or_else(nomap)?;
    serde_json::from_str(&raw).map_err(|e| nomap().detail(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn cases() -> Value {
        serde_json::from_str(include_str!("../../src/lib/open-at-cases.json")).unwrap()
    }

    fn project(id: &str, name: &str, root: &str) -> Project {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": name, "root": root, "map_dir": format!("{root}/docs/map")
        }))
        .unwrap()
    }

    #[test]
    fn paths_answer_like_the_shared_cases() {
        for c in cases()["paths"].as_array().unwrap() {
            let input = c["in"].as_str().unwrap();
            match c["out"].as_str() {
                Some(out) => assert_eq!(relative_path(input).unwrap(), out, "{input:?}"),
                None => assert!(relative_path(input).is_err(), "{input:?} must be refused"),
            }
        }
    }

    #[test]
    fn targets_answer_like_the_shared_cases() {
        for c in cases()["targets"].as_array().unwrap() {
            let input = c["in"].as_str().unwrap();
            match c["file"].as_str() {
                Some(file) => {
                    let (f, line, end) = parse_target(input).unwrap();
                    assert_eq!(f, file, "{input:?}");
                    assert_eq!(u64::from(line), c["line"].as_u64().unwrap(), "{input:?}");
                    assert_eq!(end.map(u64::from), c["end"].as_u64(), "{input:?}");
                }
                None => assert!(parse_target(input).is_err(), "{input:?} must be refused"),
            }
        }
    }

    #[test]
    fn a_project_is_matched_never_searched_for() {
        let projects = vec![
            project("p1", "docmap.nvim", "E:/repos/docmap.nvim"),
            project("p2", "twin", "E:/repos/a/twin"),
            project("p3", "Twin", "E:/repos/b/twin"),
        ];
        assert_eq!(pick_project(&projects, "docmap.nvim").unwrap().id, "p1");
        assert_eq!(pick_project(&projects, "DOCMAP.NVIM").unwrap().id, "p1");
        assert_eq!(pick_project(&projects, "p1").unwrap().id, "p1");
        assert_eq!(
            pick_project(&projects, "E:\\repos\\a\\twin\\").unwrap().id,
            "p2"
        );
        assert_eq!(
            pick_project(&projects, "twin").unwrap_err().key(),
            "err.open.ambiguous"
        );
        for unknown in ["", "nope", "E:/repos/elsewhere", ".."] {
            assert_eq!(
                pick_project(&projects, unknown).unwrap_err().key(),
                "err.open.project"
            );
        }
    }

    #[test]
    fn the_start_arguments_carry_one_request() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(Pending::from_args(&args(&[])).take().is_none());
        let p = Pending::from_args(&args(&["--open-at", "repo", "a.lua:3"]));
        assert_eq!(p.take(), Some(Ok(("repo".into(), "a.lua:3".into()))));
        assert!(p.take().is_none(), "taken once");
        for bad in [
            &["--open-at"][..],
            &["--open-at", "repo"],
            &["--open-at", "repo", "--x"],
        ] {
            let got = Pending::from_args(&args(bad)).take().unwrap();
            assert_eq!(got.unwrap_err().key(), "err.open.args");
        }
    }
}
