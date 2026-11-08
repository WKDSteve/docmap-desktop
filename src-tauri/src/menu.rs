//! The window's menu bar.
//!
//! **Structure here, strings in the frontend.** Which items exist, what order
//! they sit in, which are platform-predefined and what each is bound to are
//! facts about this application and belong in one place. The *labels* are
//! translations, and translations already live in `src/lib/i18n.js` with a
//! spec that fails when a locale is missing a key or carries one the source
//! no longer has. A second catalog in Rust would be two files with one
//! meaning, and `MENUBAR.md` names that as the wrong answer before it
//! was written — the same drift that cost `to_json` and the page payload six
//! fields between them.
//!
//! So the frontend builds the menu: it calls `set_menu` with a label per id
//! after i18n has initialised, and again whenever the language, the theme,
//! the zoom or the selection changes. Missing keys are an error naming them
//! rather than a silent fallback, because the catalog test guarantees they
//! cannot be missing and a quiet English label in a German menu is exactly
//! the failure that test exists to prevent.
//!
//! Clicking an item emits `menu` to the webview with the item's id. The
//! handlers stay in `main.js` beside the buttons that already call them —
//! one implementation, two ways to reach it, rather than a second copy
//! behind the menu.

use std::collections::HashMap;

use serde::Deserialize;
use tauri::menu::{
    CheckMenuItemBuilder, Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::{AppHandle, Emitter, Runtime};

use crate::apperr::AppError;

/// A step of building the menu that Tauri refused; its words are the detail.
fn build_err(e: tauri::Error) -> AppError {
    AppError::new("err.menu.build").detail(e)
}

/// The window state the menu has to *show*, not just act on.
///
/// A checkmark is a claim about how the window currently is, so it cannot be
/// inferred here: theme, interface language and sidebar visibility all live
/// in the frontend (in `localStorage`, per `MENUBAR.md`'s note on why
/// they are properties of this machine rather than of the project list).
/// They are handed over on every rebuild instead, which is also what makes
/// the marks correct after a change made from somewhere other than the menu.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ViewState {
    /// `"system"`, `"light"` or `"dark"` — three states, because "system"
    /// has to stay choosable. A two-way toggle can only ever leave a reader
    /// pinned to a decision they made once.
    pub theme: String,
    /// The active locale code.
    pub locale: String,
    /// The locales to offer, with their endonyms. Sent as data rather than
    /// hardcoded here so adding a language stays a one-file change in the
    /// catalog — and the labels are endonyms, never translated: a language
    /// name spelled the reader's own way is how they find it.
    pub locales: Vec<Locale>,
    /// The main pane is showing the filetree rather than the map.
    pub files: bool,
    /// The main pane is showing the project statistics.
    pub stats: bool,
    pub sidebar: bool,
    /// The sidebar is in auto-hide mode (not pinned).
    pub sidebar_auto: bool,
}

#[derive(Deserialize)]
pub struct Locale {
    pub code: String,
    pub label: String,
}

/// One clickable item: the id the frontend will receive, and its accelerator.
///
/// Accelerators are written in Tauri's own notation and deliberately *not*
/// translated — `Ctrl+G` is a key on the keyboard, not a word.
struct Item {
    id: &'static str,
    accel: Option<&'static str>,
    /// Whether this item acts on the selected project, and so is disabled
    /// when there is none. The sidebar's Generate button is disabled today
    /// with nothing saying why; a greyed menu item at least sits under a
    /// heading called Project.
    needs_project: bool,
}

const fn item(id: &'static str, accel: Option<&'static str>, needs_project: bool) -> Node {
    Node::Item(Item {
        id,
        accel,
        needs_project,
    })
}

/// A checkable item. What it is checked *by* is decided in `is_checked`,
/// which is the one place that reads `ViewState`.
const fn check(id: &'static str, accel: Option<&'static str>) -> Node {
    Node::Check(Item {
        id,
        accel,
        needs_project: false,
    })
}

/// One entry in a menu or submenu.
enum Node {
    Item(Item),
    Check(Item),
    /// A nested submenu: its own label key, then its contents.
    Sub(&'static str, &'static [Node]),
    /// The list of interface languages, built from `ViewState::locales`
    /// rather than from this file. Its item ids are `menu.view.lang:<code>`,
    /// which is why they are not literals anyone can grep for here.
    Locales,
    Separator,
}

/// The tree, as data. Read top to bottom this is the whole menu;
/// `MENUBAR.md` argues each placement.
struct Group {
    id: &'static str,
    items: &'static [Node],
}

const GROUPS: &[Group] = &[
    Group {
        id: "menu.file",
        items: &[
            item("menu.file.add", Some("CmdOrCtrl+N"), false),
            item("menu.file.open_browser", Some("CmdOrCtrl+Shift+O"), true),
            item("menu.file.reveal", None, true),
            item("menu.file.copy_path", None, true),
            item("menu.file.export", Some("CmdOrCtrl+E"), true),
            Node::Separator,
            item("menu.file.remove", Some("Delete"), true),
            Node::Separator,
            item("menu.file.workspaces", Some("CmdOrCtrl+Shift+W"), false),
            item("menu.file.settings", Some("CmdOrCtrl+,"), false),
        ],
    },
    Group {
        id: "menu.project",
        items: &[
            item("menu.project.generate", Some("CmdOrCtrl+G"), true),
            item(
                "menu.project.generate_all",
                Some("CmdOrCtrl+Shift+G"),
                false,
            ),
            item("menu.project.generate_stale", None, false),
            Node::Separator,
            // Reads, never writes — which is why it sits apart from the three
            // above rather than among them.
            item("menu.project.check", None, true),
            Node::Separator,
            item("menu.project.regenerate", Some("F5"), true),
            Node::Separator,
            item("menu.project.generate_full", None, true),
            Node::Separator,
            // Per-project, so it belongs here rather than in File → Settings,
            // which is explicitly this *machine's* preferences. `needs_project`
            // for the same reason: there is nothing to configure without one.
            item("menu.project.scope", None, true),
        ],
    },
    Group {
        id: "menu.view",
        items: &[
            Node::Sub(
                "menu.view.theme",
                &[
                    check("menu.view.theme.system", None),
                    check("menu.view.theme.light", None),
                    check("menu.view.theme.dark", None),
                ],
            ),
            Node::Sub("menu.view.language", &[Node::Locales]),
            Node::Separator,
            // The map is a dense page, and this is the single most useful
            // thing a menu bar adds to it. `CmdOrCtrl+Plus` as well as
            // `Equal`: the unshifted key is what people actually press.
            item("menu.view.zoom_in", Some("CmdOrCtrl+Plus"), false),
            item("menu.view.zoom_out", Some("CmdOrCtrl+-"), false),
            item("menu.view.zoom_reset", Some("CmdOrCtrl+0"), false),
            Node::Separator,
            check("menu.view.files", Some("CmdOrCtrl+Shift+F")),
            check("menu.view.stats", None),
            check("menu.view.sidebar", Some("CmdOrCtrl+B")),
            check("menu.view.sidebar_auto", None),
        ],
    },
    Group {
        id: "menu.help",
        items: &[
            item("menu.help.usage", None, false),
            item("menu.help.engine", None, false),
            item("menu.help.docs_search", None, false),
            item("menu.help.settings_folder", None, false),
            Node::Separator,
            item("menu.help.repo", None, false),
            item("menu.help.license", None, false),
            Node::Separator,
            item("menu.help.feedback", None, false),
            Node::Separator,
            item("menu.help.about", None, false),
        ],
    },
];

/// Whether a checkable item is currently checked.
///
/// The one place that reads `ViewState`, so "what the window is" is compared
/// against the menu in exactly one function rather than threaded through the
/// tree as data that could disagree with itself.
fn is_checked(id: &str, state: &ViewState) -> bool {
    match id {
        "menu.view.theme.system" => state.theme != "light" && state.theme != "dark",
        "menu.view.theme.light" => state.theme == "light",
        "menu.view.theme.dark" => state.theme == "dark",
        "menu.view.files" => state.files,
        "menu.view.stats" => state.stats,
        "menu.view.sidebar" => state.sidebar,
        "menu.view.sidebar_auto" => state.sidebar_auto,
        _ => false,
    }
}

fn walk(nodes: &'static [Node], keys: &mut Vec<&'static str>) {
    for node in nodes {
        match node {
            Node::Item(it) | Node::Check(it) => keys.push(it.id),
            Node::Sub(id, children) => {
                keys.push(id);
                walk(children, keys);
            }
            // Built from `ViewState::locales`, and its labels are endonyms
            // rather than catalog entries.
            Node::Locales | Node::Separator => {}
        }
    }
}

/// Every label key this module needs, in one list.
///
/// Exposed so a test can assert the frontend catalog covers it: the two are
/// joined by string keys, and nothing else would notice the day an id is
/// renamed on one side.
pub fn label_keys() -> Vec<&'static str> {
    let mut keys = Vec::new();
    for group in GROUPS {
        keys.push(group.id);
        walk(group.items, &mut keys);
    }
    // Quit and Close window are predefined items, and the platform supplies
    // their text — but not their submenu placement, so File still needs to
    // know they belong to it. No label key: asking the frontend to translate
    // "Quit" would override the OS's own wording, which on macOS is where
    // the item does not even live.
    keys
}

/// The id prefix a language item carries, joined to its locale code.
pub const LOCALE_ID: &str = "menu.view.lang:";

fn fill<'a, R: Runtime>(
    app: &'a AppHandle<R>,
    sub: SubmenuBuilder<'a, R, AppHandle<R>>,
    nodes: &'static [Node],
    labels: &HashMap<String, String>,
    state: &ViewState,
    has_project: bool,
) -> Result<SubmenuBuilder<'a, R, AppHandle<R>>, AppError> {
    let mut sub = sub;
    for node in nodes {
        sub = match node {
            Node::Separator => sub.separator(),
            Node::Item(it) => {
                let mut b = MenuItemBuilder::with_id(it.id, &labels[it.id])
                    .enabled(!it.needs_project || has_project);
                if let Some(accel) = it.accel {
                    b = b.accelerator(accel);
                }
                sub.item(&b.build(app).map_err(build_err)?)
            }
            Node::Check(it) => {
                let mut b = CheckMenuItemBuilder::with_id(it.id, &labels[it.id])
                    .checked(is_checked(it.id, state));
                if let Some(accel) = it.accel {
                    b = b.accelerator(accel);
                }
                sub.item(&b.build(app).map_err(build_err)?)
            }
            Node::Sub(id, children) => {
                let nested = SubmenuBuilder::new(app, &labels[*id]);
                let nested = fill(app, nested, children, labels, state, has_project)?;
                sub.item(&nested.build().map_err(build_err)?)
            }
            Node::Locales => {
                let mut acc = sub;
                for loc in &state.locales {
                    let b = CheckMenuItemBuilder::with_id(
                        format!("{LOCALE_ID}{}", loc.code),
                        // The endonym, verbatim. Never translated, and never
                        // put through a CSS case transform elsewhere either:
                        // a `text-transform` on a language name is the
                        // Turkish dotless-ı bug waiting to happen.
                        &loc.label,
                    )
                    .checked(loc.code == state.locale);
                    acc = acc.item(&b.build(app).map_err(build_err)?);
                }
                acc
            }
        };
    }
    Ok(sub)
}

/// Build the menu from the frontend's labels and set it on the main window.
pub fn build<R: Runtime + 'static>(
    app: &AppHandle<R>,
    labels: &HashMap<String, String>,
    has_project: bool,
    state: &ViewState,
) -> Result<Menu<R>, AppError> {
    let missing: Vec<&str> = label_keys()
        .into_iter()
        .filter(|k| !labels.contains_key(*k))
        .collect();
    if !missing.is_empty() {
        // Named rather than defaulted. The catalog spec makes this
        // unreachable; if it is ever reached, the menu is out of step with
        // the interface and saying so is more use than an English label
        // sitting in a German menu.
        return Err(AppError::new("err.menu.labels").with("keys", missing.join(", ")));
    }

    let menu = Menu::new(app).map_err(build_err)?;

    for group in GROUPS {
        let mut sub = fill(
            app,
            SubmenuBuilder::new(app, &labels[group.id]),
            group.items,
            labels,
            state,
            has_project,
        )?;
        // The window-level items go at the bottom of File, where the
        // convention puts them on Windows and Linux. `PredefinedMenuItem`
        // rather than our own: it carries the platform's own wording and,
        // on macOS, the platform's own placement.
        if group.id == "menu.file" {
            sub = sub.separator();
            sub = sub.item(&PredefinedMenuItem::close_window(app, None).map_err(build_err)?);
            sub = sub.item(&PredefinedMenuItem::quit(app, None).map_err(build_err)?);
        }
        menu.append(&sub.build().map_err(build_err)?)
            .map_err(build_err)?;
    }

    Ok(menu)
}

/// Forward every click to the webview as a `menu` event carrying the id.
///
/// Registered once at startup rather than per rebuild: `set_menu` replaces
/// the menu whenever the language or the selection changes, and a handler
/// re-registered each time would fire once per rebuild it survived.
pub fn forward_clicks<R: Runtime>(app: &AppHandle<R>) {
    app.on_menu_event(|app, event| {
        let _ = app.emit("menu", event.id().0.clone());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The labels the frontend would send, faked with the key as its own
    /// text. The *content* is `src/lib/i18n.js`'s problem and
    /// `src/lib/menu.test.js` checks the two lists agree; what cannot be
    /// checked from JavaScript is whether Tauri will actually build this
    /// tree, which is what these exercise.
    fn labels() -> HashMap<String, String> {
        label_keys()
            .into_iter()
            .map(|k| (k.to_string(), k.to_string()))
            .collect()
    }

    fn state() -> ViewState {
        ViewState {
            theme: "system".into(),
            locale: "de".into(),
            locales: vec![
                Locale {
                    code: "en".into(),
                    label: "English".into(),
                },
                Locale {
                    code: "de".into(),
                    label: "Deutsch".into(),
                },
            ],
            files: false,
            stats: false,
            sidebar: true,
            sidebar_auto: false,
        }
    }

    // **Not on macOS.** `muda::Menu` refuses to be created off the main
    // thread there, and Rust's test harness runs every test on a worker.
    // Nothing about the menu differs on macOS — the builder simply cannot
    // be called from where a test lives, so the assertion is unreachable
    // rather than untrue. `MENUBAR.md` already records that the tree
    // itself needs a documented macOS variant before shipping there, and
    // that is the check this cannot substitute for.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_menu_builds_with_a_complete_label_set() {
        // The one thing this environment cannot verify by eye. A window with
        // no menu looks, from here, exactly like a window whose menu is fine
        // — `mock_app()` gives a real `AppHandle` with no window, which is
        // enough for the menu builder to succeed or fail for real.
        let app = tauri::test::mock_app();
        assert!(build(app.handle(), &labels(), true, &state()).is_ok());
    }

    // **Not on macOS.** `muda::Menu` refuses to be created off the main
    // thread there, and Rust's test harness runs every test on a worker.
    // Nothing about the menu differs on macOS — the builder simply cannot
    // be called from where a test lives, so the assertion is unreachable
    // rather than untrue. `MENUBAR.md` already records that the tree
    // itself needs a documented macOS variant before shipping there, and
    // that is the check this cannot substitute for.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn it_builds_the_same_with_nothing_selected() {
        // The project-scoped items are disabled rather than absent, so the
        // tree has to build in both states — a menu that only exists once
        // something is selected is a menu nobody finds.
        let app = tauri::test::mock_app();
        assert!(build(app.handle(), &labels(), false, &state()).is_ok());
    }

    // **Not on macOS.** `muda::Menu` refuses to be created off the main
    // thread there, and Rust's test harness runs every test on a worker.
    // Nothing about the menu differs on macOS — the builder simply cannot
    // be called from where a test lives, so the assertion is unreachable
    // rather than untrue. `MENUBAR.md` already records that the tree
    // itself needs a documented macOS variant before shipping there, and
    // that is the check this cannot substitute for.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn it_builds_with_no_locales_offered() {
        // The language submenu is the one part built from data rather than
        // from this file, so it is the one part that can arrive empty.
        let app = tauri::test::mock_app();
        let mut s = state();
        s.locales.clear();
        assert!(build(app.handle(), &labels(), true, &s).is_ok());
    }

    #[test]
    fn a_missing_label_is_an_error_naming_it() {
        // Rather than an English label quietly sitting in a German menu,
        // which is the failure the catalog spec exists to prevent and would
        // be undone by a silent fallback here.
        let app = tauri::test::mock_app();
        let mut incomplete = labels();
        // Whatever key is removed has to still exist, or this passes by
        // removing nothing -- which is how it failed when the Tools menu was
        // folded into Settings, and is the reason the assertion below names
        // the key rather than merely checking for an error.
        let victim = "menu.file.settings";
        assert!(
            labels().contains_key(victim),
            "{victim} is no longer a menu label"
        );
        incomplete.remove(victim);
        let err = match build(app.handle(), &incomplete, true, &state()) {
            Err(e) => e,
            Ok(_) => panic!("an incomplete label set must not produce a menu"),
        };
        assert!(err.contains(victim), "unhelpful error: {err}");
    }

    #[test]
    fn every_id_is_unique() {
        // Two items sharing an id would send one event for two commands, and
        // the frontend's action table would run whichever it mapped.
        let keys = label_keys();
        let mut seen = std::collections::HashSet::new();
        for key in &keys {
            assert!(seen.insert(*key), "duplicate menu id: {key}");
        }
        assert!(
            keys.len() >= 20,
            "expected the whole menu, got {}",
            keys.len()
        );
    }

    #[test]
    fn system_is_the_theme_check_when_nothing_was_chosen() {
        // Three states, and the absence of a choice is one of them. An empty
        // string is what the frontend sends when nothing is stamped on
        // `<html>`, and reading that as "not system" would leave the theme
        // submenu with no mark at all.
        let mut s = state();
        s.theme = String::new();
        assert!(is_checked("menu.view.theme.system", &s));
        assert!(!is_checked("menu.view.theme.dark", &s));

        s.theme = "dark".into();
        assert!(is_checked("menu.view.theme.dark", &s));
        assert!(!is_checked("menu.view.theme.system", &s));
    }
}
