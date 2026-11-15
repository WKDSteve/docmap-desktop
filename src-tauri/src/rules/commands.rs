//! The Tauri commands of the Rules tab.
//!
//! A project is named by its `id` as everywhere else; a ruleset by the path
//! the catalog listed, which `tab.rs` looks up again before it does anything
//! with it. A command that starts an engine process runs on Tauri's pool
//! (`command(async)`), and so do the others: they hash a ruleset.
//!
//! **One engine call per look, not per click.** An engine call takes seconds
//! on Windows and minutes on a busy machine, and the steps of the trust
//! dialog (what would be trusted, show the code, trust, revoke) and a run all
//! need the catalog only to know which rulesets and predicate rules the
//! project has. So the last catalog of a project is kept ([`Remembering`])
//! and handed out again **while nothing it was read from has changed**: the
//! project's `.rules.json` and every file of every ruleset, by hash. A list of
//! predicate rules shown in the dialog is therefore always the list of the
//! content whose hash is trusted. `rules_catalog` itself always asks the
//! engine: opening the tab and **Read again** are requests to look.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use super::engine::{CliRoutes, Routes};
use super::tab::{self, PredicateSource, TrustPreview};
use super::trust;
use crate::apperr::AppError;

/// One writer of the store at a time: a grant is read, change, write.
static STORE: Mutex<()> = Mutex::new(());

/// A catalog and what it was read from.
struct Kept {
    catalog: Value,
    /// [`sources`] at the time of the read.
    sources: Vec<Option<String>>,
}

/// The last catalog of each project, by project root.
static KEPT: Mutex<Option<HashMap<PathBuf, Kept>>> = Mutex::new(None);

/// What a catalog of the project at `root` depends on, as hashes: the
/// project's `.rules.json`, then every ruleset the catalog lists. `None` for
/// one that cannot be read, which never equals a hash.
fn sources(root: &Path, catalog: &Value) -> Vec<Option<String>> {
    let hash = |path: &Path| {
        trust::real_path(path)
            .and_then(|real| trust::digest(&real))
            .ok()
            .map(|d| d.sha256)
    };
    let mut out = vec![hash(&root.join(".rules.json"))];
    for ruleset in tab::ruleset_paths(catalog) {
        out.push(hash(Path::new(&ruleset)));
    }
    out
}

/// The engine, with the last catalog of a project handed out again while its
/// sources are unchanged (`reuse`), and every catalog it does fetch kept.
struct Remembering<'a> {
    engine: &'a CliRoutes,
    reuse: bool,
}

impl Routes for Remembering<'_> {
    fn catalog(&self, root: &Path) -> Result<Value, AppError> {
        if self.reuse {
            let kept = KEPT.lock().ok().and_then(|k| {
                k.as_ref()?
                    .get(root)
                    .map(|kept| (kept.catalog.clone(), kept.sources.clone()))
            });
            if let Some((catalog, then)) = kept {
                let now = sources(root, &catalog);
                if now == then && now.iter().skip(1).all(Option::is_some) {
                    return Ok(catalog);
                }
            }
        }
        let catalog = self.engine.catalog(root)?;
        if let Ok(mut kept) = KEPT.lock() {
            let map = kept.get_or_insert_with(HashMap::new);
            if catalog.get("available").and_then(Value::as_bool) == Some(true) {
                let sources = sources(root, &catalog);
                map.insert(
                    root.to_path_buf(),
                    Kept {
                        catalog: catalog.clone(),
                        sources,
                    },
                );
            } else {
                map.remove(root);
            }
        }
        Ok(catalog)
    }

    fn run(&self, root: &Path, select: &str, trust: &[String]) -> Result<Value, AppError> {
        self.engine.run(root, select, trust)
    }
}

fn project_root(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, AppError> {
    let ws = crate::read_workspace(app)?;
    let p = ws
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::no_project(id))?;
    Ok(PathBuf::from(&p.root))
}

fn routes(app: &tauri::AppHandle) -> Result<CliRoutes, AppError> {
    let info = crate::engine_info(app.clone())?;
    let path = info.path.ok_or_else(|| AppError::new("err.engine.none"))?;
    CliRoutes::new(path, info.grammars)
}

/// The store's file: in the app's own settings folder, never in a project.
fn store_file(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    Ok(crate::config_dir(app)?.join(trust::STORE_FILE))
}

/// Every rule of the project's rulesets, with this machine's trust per
/// ruleset. Always asks the engine.
#[tauri::command(async)]
pub fn rules_catalog(app: tauri::AppHandle, id: String) -> Result<Value, AppError> {
    let root = project_root(&app, &id)?;
    let engine = routes(&app)?;
    let look = Remembering {
        engine: &engine,
        reuse: false,
    };
    tab::catalog(&look, &root, &store_file(&app)?)
}

/// The mechanical run of `selection` (rule ids, families, `gate:<name>`), in
/// the engine on this machine. Nothing is sent anywhere.
#[tauri::command(async)]
pub fn rules_run(app: tauri::AppHandle, id: String, selection: String) -> Result<Value, AppError> {
    let root = project_root(&app, &id)?;
    let engine = routes(&app)?;
    let known = Remembering {
        engine: &engine,
        reuse: true,
    };
    tab::run(&known, &root, &store_file(&app)?, &selection)
}

/// What trusting the ruleset at `path` would mean. Trusts nothing.
#[tauri::command(async)]
pub fn rules_trust_preview(
    app: tauri::AppHandle,
    id: String,
    path: String,
) -> Result<TrustPreview, AppError> {
    let root = project_root(&app, &id)?;
    let engine = routes(&app)?;
    let known = Remembering {
        engine: &engine,
        reuse: true,
    };
    tab::preview(&known, &root, &store_file(&app)?, &path)
}

/// Trust the ruleset at `path` with the content the dialog showed (`sha256`).
#[tauri::command(async)]
pub fn rules_trust_grant(
    app: tauri::AppHandle,
    id: String,
    path: String,
    sha256: String,
) -> Result<(), AppError> {
    let root = project_root(&app, &id)?;
    let engine = routes(&app)?;
    let known = Remembering {
        engine: &engine,
        reuse: true,
    };
    let file = store_file(&app)?;
    let _one = STORE.lock().map_err(|_| AppError::new("err.lock"))?;
    tab::grant(
        &known,
        &root,
        &file,
        &path,
        &sha256,
        &trust::account(),
        crate::agent::now_ms(),
    )
}

/// Take the trust of the ruleset at `path` back.
#[tauri::command(async)]
pub fn rules_trust_revoke(app: tauri::AppHandle, id: String, path: String) -> Result<(), AppError> {
    let root = project_root(&app, &id)?;
    let engine = routes(&app)?;
    let known = Remembering {
        engine: &engine,
        reuse: true,
    };
    let file = store_file(&app)?;
    let _one = STORE.lock().map_err(|_| AppError::new("err.lock"))?;
    tab::revoke(&known, &root, &file, &path)
}

/// The verdicts and waivers of the project, as the tab shows them. Reads two
/// files; no engine.
#[tauri::command(async)]
pub fn rules_judged(app: tauri::AppHandle, id: String) -> Result<super::judged::Judged, AppError> {
    let root = project_root(&app, &id)?;
    Ok(super::judged::read(
        &root,
        &crate::agent::date_of(crate::agent::now_ms()),
    ))
}

/// The code of one predicate rule, for the trust dialog.
#[tauri::command(async)]
pub fn rules_predicate_source(
    app: tauri::AppHandle,
    id: String,
    rule: String,
) -> Result<PredicateSource, AppError> {
    let root = project_root(&app, &id)?;
    let engine = routes(&app)?;
    let known = Remembering {
        engine: &engine,
        reuse: true,
    };
    tab::predicate_source(&known, &root, &rule)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fresh_dir;
    use serde_json::json;
    use std::fs;

    #[test]
    fn the_sources_of_a_catalog_change_with_the_project_file_and_with_any_ruleset_file() {
        let base = fresh_dir("docmap-rules-kept-sources");
        let project = base.join("project");
        let ruleset = base.join("ruleset");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&ruleset).unwrap();
        fs::write(project.join(".rules.json"), "{}").unwrap();
        fs::write(ruleset.join("RULES.md"), "one\n").unwrap();
        let catalog = json!({"available": true, "rulesets": [{"path": crate::portable(&ruleset)}]});

        let first = sources(&project, &catalog);
        assert_eq!(first.len(), 2);
        assert!(first.iter().all(Option::is_some));
        assert_eq!(sources(&project, &catalog), first, "nothing changed");

        fs::write(ruleset.join("RULES.md"), "two\n").unwrap();
        let second = sources(&project, &catalog);
        assert_ne!(second, first, "a rule file changed");
        fs::write(project.join(".rules.json"), "{ \"rulesets\": [] }").unwrap();
        assert_ne!(
            sources(&project, &catalog),
            second,
            "the project file changed"
        );

        // a ruleset that is gone has no hash, and no hash never counts as unchanged
        fs::remove_dir_all(&ruleset).unwrap();
        assert_eq!(sources(&project, &catalog)[1], None);
        // a project without the file (the vault is used then) is a state like any other
        fs::remove_file(project.join(".rules.json")).unwrap();
        assert_eq!(sources(&project, &catalog)[0], None);
    }
}
