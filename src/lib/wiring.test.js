import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { keys } from "./i18n.js";
import { MAIN_RS } from "./rust-main.js";

// The window is plain HTML plus one script, joined by nothing but string
// equality: an element id, a command name, a catalog key. A typo in any of
// them is not a build error — it is a control that does nothing, found by
// somebody clicking it. These read the sources and check the joins.
//
// CRLF is normalised for the same reason the other source-reading specs do it.
function read(path) {
  return readFileSync(new URL(path, import.meta.url), "utf8").replace(/\r\n/g, "\n");
}

const MAIN = read("../main.js");
const HTML = read("../index.html");
const RUST = MAIN_RS;

test("every element main.js looks up by id exists in the markup", () => {
  const ids = [...new Set([...MAIN.matchAll(/getElementById\("([A-Za-z0-9_-]+)"\)/g)].map((m) => m[1]))];
  assert.ok(ids.length > 80, `expected the whole window, found ${ids.length} ids`);
  const missing = ids.filter((id) => !HTML.includes(`id="${id}"`));
  assert.deepEqual(missing, [], `main.js reads ids the markup does not define: ${missing}`);
});

test("every command main.js invokes is registered with Tauri", () => {
  const called = [...new Set([...MAIN.matchAll(/invoke\("([a-z_]+)"/g)].map((m) => m[1]))];
  assert.ok(called.length > 30, `expected the whole command surface, found ${called.length}`);
  const handler = RUST.slice(RUST.indexOf("generate_handler!["), RUST.indexOf("])", RUST.indexOf("generate_handler![")));
  const missing = called.filter((c) => !new RegExp(`\\b${c},?\\b`).test(handler));
  assert.deepEqual(missing, [], `main.js invokes commands the backend does not register: ${missing}`);
});

test("every catalog key main.js asks for exists", () => {
  const catalog = new Set(keys());
  // `t("a.b")` with a literal key. Keys built at run time (`"count." + x`)
  // are not visible here, which is why the pieces that build them have their
  // own checks below.
  const asked = [...new Set([...MAIN.matchAll(/\bt\("([a-zA-Z0-9_.]+)"\)/g)].map((m) => m[1]))];
  assert.ok(asked.length > 100, `expected the whole interface, found ${asked.length} keys`);
  const missing = asked.filter((k) => !catalog.has(k));
  assert.deepEqual(missing, [], `main.js asks for keys the catalog lacks: ${missing}`);
});

test("the keys built from a kind or a tile name exist for every kind", () => {
  const catalog = new Set(keys());
  const wanted = [];
  for (const kind of [
    "module",
    "namespace",
    "file",
    "function",
    "symbol",
    "type",
    "doc",
    "feature",
    "binding",
    "endpoint",
    "marker",
    "plugin",
  ]) {
    wanted.push(`find.kind.${kind}`);
  }
  for (const kind of ["code", "docs", "data"]) wanted.push(`stats.kind.${kind}`);
  for (const tile of ["code", "comments", "docs", "data", "blank"]) wanted.push(`stats.tile.${tile}`);
  const missing = wanted.filter((k) => !catalog.has(k));
  assert.deepEqual(missing, [], `catalog keys built at run time are missing: ${missing}`);
});

test("the search kinds the backend emits are the ones the interface can name", () => {
  const search = read("../../src-tauri/src/search.rs");
  // The doc comment on `ViewHit::kind` lists them; the catalog has to cover
  // every one, or a result shows a raw word.
  const listed = [...search.slice(search.indexOf("What it is:"), search.indexOf("pub kind: String")).matchAll(/`([a-z]+)`/g)].map(
    (m) => m[1]
  );
  assert.ok(listed.length >= 10, `expected the kinds list, found ${listed.length}`);
  const catalog = new Set(keys());
  const missing = listed.filter((k) => !catalog.has(`find.kind.${k}`));
  assert.deepEqual(missing, [], `the backend emits kinds the catalog cannot name: ${missing}`);
});

test("the project bar is only shown for a selected project", () => {
  assert.match(MAIN, /classList\.toggle\("has-bar", !!view\.selectedId\)/);
  assert.match(MAIN, /getElementById\("projbar"\)\.hidden = !view\.selectedId/);
});

test("the sort control only exists on the overview", () => {
  assert.match(MAIN, /els\.sort\.hidden = view\.projects\.length < 2 \|\| !!view\.selectedId/);
});

test("a hidden control inside the search panel is actually hidden", () => {
  // The trap every `display:` rule sets for `[hidden]`; the panel opts out
  // wholesale rather than one control at a time.
  const css = read("../style.css");
  assert.match(css, /\.finder-panel \[hidden\]\s*\{\s*display:\s*none\s*!important/);
  assert.match(css, /#sidebar\[hidden\]\s*\{\s*display:\s*none/);
});

test("every i18n attribute in the markup names a catalog key", () => {
  // `data-i18n`, `-help`, `-aria` and `-placeholder` are applied by walking the
  // markup, so a key that does not exist is a control with no label or a
  // tooltip that never appears — silently, in both languages.
  const catalog = new Set(keys());
  const used = [
    ...HTML.matchAll(/data-i18n(?:-help|-aria|-placeholder)?="([a-zA-Z0-9_.]+)"/g),
  ].map((m) => m[1]);
  assert.ok(used.length > 100, `expected the whole markup, found ${used.length} attributes`);
  const missing = [...new Set(used)].filter((k) => !catalog.has(k));
  assert.deepEqual(missing, [], `the markup names keys the catalog lacks: ${missing}`);
});

test("no English catalog key is dead", () => {
  // The locale specs compare each locale with the English catalog, not with the
  // code, so a key nobody asks for survives forever. A key counts as used when
  // its name appears as a quoted string in the app's own sources or markup, or
  // belongs to a family built at run time.
  const sources = [
    MAIN,
    HTML,
    read("./deps.js"),
    read("./languages.js"),
    read("./overview.js"),
    read("./traffic.js"),
    read("./finder.js"),
    read("./stats.js"),
    read("./engine-log.js"),
    read("./errors.js"),
    read("./contract.js"),
    read("./nvim-import.js"),
    // A command's error names its key in Rust: `apperr::ENGLISH` lists every
    // one, and `errors.test.js` holds that list against the catalog.
    read("../../src-tauri/src/apperr.rs"),
    // The web variant answers the same commands from the browser, and its
    // refusals name their keys (`web.*`) there.
    read("../../web/src/backend.js").replaceAll("'", '"'),
    read("../../web/src/shell.js").replaceAll("'", '"'),
  ].join("\n");
  const RUNTIME = [
    "menu.", // the Rust menu builder asks for these by id
    "traffic.via.",
    "stats.tile.",
    "stats.kind.",
    "find.kind.",
    "count.",
    "sort.",
    "scope.lang.",
    "grammars.diag.",
    "ph.",
  ];
  const dead = keys().filter((k) => {
    if (RUNTIME.some((p) => k.startsWith(p))) return false;
    // `.one` is the singular of a plural pair, chosen by `plural()`.
    const base = k.replace(/\.one$/, "");
    return !sources.includes(`"${base}"`) && !sources.includes(`"${k}"`);
  });
  assert.deepEqual(dead, [], `catalog keys nothing asks for: ${dead}`);
});

test("a message that takes the map's place says there is no map any more", () => {
  const start = MAIN.indexOf("function showPlaceholder(");
  const body = MAIN.slice(start, MAIN.indexOf("\n}\n", start));
  assert.match(body, /view\.mapBase = null/, "showPlaceholder must reset view.mapBase");
  assert.match(body, /mapTab = null/);
  // Declared before the function that writes it, or the first call is a
  // temporal-dead-zone error.
  assert.ok(MAIN.indexOf("const viewStore = createStore(") < start, "the view state must be declared above showPlaceholder");
});

test("removing a project or switching workspace leaves no pane over the overview", () => {
  for (const fn of ["async function removeProject(", "async function useWorkspace("]) {
    const start = MAIN.indexOf(fn);
    assert.ok(start > 0, `${fn} should exist`);
    const body = MAIN.slice(start, MAIN.indexOf("\n}\n", start));
    assert.match(body, /dropSelection\(\)/, `${fn} must go through dropSelection`);
  }
  const drop = MAIN.slice(MAIN.indexOf("function dropSelection("), MAIN.indexOf("async function removeProject("));
  assert.match(drop, /closePanes\(\)/);
  assert.match(drop, /resetFinder\(\)/);
});

test("only the map frame can speak for the map", () => {
  // Both listeners: the page reporting and the page answering.
  const listeners = MAIN.match(/window\.addEventListener\("message"[\s\S]*?\n\}\);/g) || [];
  assert.ok(listeners.length >= 2, `expected the message listeners, found ${listeners.length}`);
  for (const l of listeners) {
    if (!l.includes('"docmap"')) continue;
    assert.match(l, /ev\.source !== els\.frame\.contentWindow/, "a docmap message must come from the frame");
  }
});

test("a request to open a file needs a click behind it", () => {
  assert.match(MAIN, /navigator\.userActivation/);
  const start = MAIN.indexOf('data.kind === "open-file"');
  assert.ok(MAIN.slice(start, start + 900).includes("activation.isActive"));
});

test("task messages are brought in front of the Files and Statistics panes", () => {
  for (const fn of ["async function generateFor(", "async function generateAll(", "async function checkMap("]) {
    const start = MAIN.indexOf(fn);
    const body = MAIN.slice(start, MAIN.indexOf("\n}\n", start));
    assert.doesNotMatch(body, /[^k]showPlaceholder\(/, `${fn} must use showTaskPlaceholder`);
    assert.match(body, /showTaskPlaceholder\(/);
  }
});

test("the opaque panes sit above the map's context note", () => {
  const css = read("../style.css");
  for (const sel of [".files", ".stats"]) {
    const start = css.indexOf(`\n${sel} {`);
    assert.ok(start > 0, `${sel} should exist`);
    assert.match(css.slice(start, css.indexOf("}", start)), /z-index:\s*2/);
  }
});

test("a result about a project is shown wherever that project is shown", () => {
  const start = MAIN.indexOf("async function generateFor(");
  const body = MAIN.slice(start, MAIN.indexOf("\n}\n", start));
  assert.match(body, /const here = \(\) => view\.selectedId === started \|\| view\.selectedId === id;/);
  // The cached verdict is about the map that was just replaced.
  assert.match(body, /freshness\.delete\(p\.id\)/);
});

test("a jump inside the map does not arm the blank-pane watchdog", () => {
  const start = MAIN.indexOf("function gotoMap(");
  const body = MAIN.slice(start, MAIN.indexOf("\n}\n", start));
  assert.doesNotMatch(body, /watchMapLoad\(/, "a fragment change loads no document");
});

test("dropping the selection tells the native menu", () => {
  const start = MAIN.indexOf("function dropSelection(");
  const body = MAIN.slice(start, MAIN.indexOf("\n}\n", start));
  assert.match(body, /syncMenu\(\)/);
});

test("the Count-again button follows the project on screen", () => {
  assert.match(MAIN, /statsUi\.refresh\.disabled = !!view\.selectedId && statsPending\.has\(view\.selectedId\)/);
  assert.doesNotMatch(MAIN, /statsUi\.refresh\.disabled = (true|false)/);
});

// ---- repository links: the guards are joined to the commands by call sites ----
//
// A guard that is not called is a guard that is not there, and the Rust tests
// exercise the helpers, not the commands (those take a live AppHandle).
//
// Comment lines are dropped first: a guard that is only commented out is not
// there, and a pattern that still matched it would call it present.
// A `*` only counts as a comment when it continues a block comment (`* text`,
// `*/`): `*steps -= 1;` is a dereference and stays.
const code = (text) => text.replace(/^\s*(\/\/|\/\*|\*(\s|\/|$)).*$/gm, "");

function rustFn(name) {
  const start = RUST.search(new RegExp(`\\n(?:pub\\(crate\\) )?(?:async )?fn ${name}\\b`));
  assert.ok(start >= 0, `no fn ${name} in the command modules`);
  return code(RUST.slice(start, RUST.indexOf("\n}\n", start)));
}

function jsFn(name) {
  const start = MAIN.indexOf(name);
  assert.ok(start >= 0, `no ${name} in main.js`);
  return code(MAIN.slice(start, MAIN.indexOf("\n}\n", start)));
}

// ---- the engine's report: where it is drawn, and that it can be reached ----
//
// What follows is the part of the report no function test can see. It shipped
// once with the report invisible after a successful run (the only place it was
// drawn was a placeholder that `select()` had just hidden) and unscrollable
// after "Show all" (a centred grid in a window whose body does not scroll).
// Both were "verified in the preview", which showed neither state. These pin
// the joins; whether the layout really scrolls is measured in a browser with
// `tools/preview/probe-report.js`, which needs a layout engine and so is not
// part of `node --test`.

/** The declarations of the first rule whose selector is exactly `selector`. */
function cssRule(css, selector) {
  const start = css.search(new RegExp(`(^|\\n)${selector.replace(/[.[\]()]/g, "\\$&")} \\{`));
  assert.ok(start >= 0, `no rule for ${selector} in style.css`);
  return css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
}

test("the report of a run that worked is drawn on the map, not in the placeholder select() hid", () => {
  const gen = jsFn("async function generateFor(");
  const ok = gen.slice(gen.indexOf("if (res.ok)"), gen.indexOf("} else {", gen.indexOf("if (res.ok)")));
  assert.match(ok, /await select\(id\);/);
  assert.match(ok, /showRunReport\(log\)/, "a successful run draws no report");
  assert.doesNotMatch(ok, /appendLog\(/, "appendLog has no host once select() has hidden the placeholder");
  // And drawn after `select`, which hides any report: before it, it would be
  // removed again the moment it was shown.
  assert.ok(ok.indexOf("await select(id)") < ok.indexOf("showRunReport(log)"));

  // The behaviour lives in lib/run-report.js (run-report.test.js drives it);
  // this is only the join: main.js hands it the three elements.
  assert.match(jsFn("function showRunReport("), /runReport\.show\(text\)/);
  assert.match(jsFn("function hideRunReport("), /runReport\.hide\(\)/);
  const make = jsFn("const runReport = createRunReport(");
  assert.match(make, /panel: els\.runReport,/);
  assert.match(make, /body: els\.runReportBody,/);
  assert.match(make, /close: els\.runReportClose,/);
  // Closing it must have somewhere to put the keyboard (run-report.test.js).
  assert.match(make, /returnFocus: \(\) => \[els\.gen, els\.list\]/);
  assert.doesNotMatch(make, /placeholder/, "the docked report must not depend on the placeholder");
});

test("a report goes when its project or its message goes", () => {
  // Left up, the last run's findings would sit over the next project's map.
  for (const fn of ["async function select(", "function showPlaceholder(", "function dropSelection("]) {
    assert.match(jsFn(fn), /hideRunReport\(\)/, `${fn} leaves the old report up`);
  }
});

test("the docked report has the markup, the scroll and the hidden rule it needs", () => {
  for (const id of ["run-report", "run-report-body", "run-report-close"]) {
    assert.ok(HTML.includes(`id="${id}"`), `no #${id} in the markup`);
  }
  // A scroller the keyboard can reach: a report with nothing to fold
  // has no button to land on.
  assert.match(HTML, /id="run-report-body"[^>]*tabindex="0"/);
  assert.match(HTML, /id="run-report"[^>]*role="region"/);

  const css = read("../style.css");
  assert.match(cssRule(css, ".run-report-body"), /overflow-y:\s*auto/);
  assert.match(cssRule(css, ".run-report"), /max-height:/, "an unbounded report would cover the map");
  // A layout dock, not an overlay: the last item of the `#view` column, so the
  // map is shorter by its height and no pane is drawn over it. The layout
  // itself is measured in the browser (tools/preview/probe-report.js).
  const dock = cssRule(css, ".run-report");
  assert.doesNotMatch(dock, /position:\s*absolute/, "an overlay hides the map's scrollbar and the report's own rows");
  assert.match(dock, /flex:\s*none/);
  assert.match(cssRule(css, "#view"), /display:\s*flex/);
  assert.match(cssRule(css, "#view"), /flex-direction:\s*column/);
  for (const sel of ["#map", ".files", ".stats", ".overview", ".placeholder"]) {
    assert.match(cssRule(css, sel), /flex:\s*1 1 0/, `${sel} must share the column with the docked report`);
  }
  for (const sel of [".files", ".stats"]) {
    // `relative`: in the column (the report is never under them), and still
    // above the context note, which is absolute with z-index 1.
    assert.doesNotMatch(cssRule(css, sel), /position:\s*absolute/, `${sel} drawn over the column would cover the report`);
    assert.match(cssRule(css, sel), /position:\s*relative/);
  }
  // `display: flex` outranks the UA's `[hidden]` rule: without this a closed
  // report stays on screen (the trap `.placeholder[hidden]` already documents).
  assert.match(cssRule(css, ".run-report[hidden]"), /display:\s*none/);
});

test("a message with a long report scrolls instead of being centred off the top", () => {
  const css = read("../style.css");
  const ph = cssRule(css, ".placeholder");
  assert.match(ph, /overflow-y:\s*auto/, "the placeholder must scroll");
  // `place-content: center` pushes the overflow out of both ends, so the top
  // of the report ends up above the window with nothing to scroll back to.
  assert.doesNotMatch(ph, /place-content:\s*center/);
  assert.doesNotMatch(ph, /align-content:\s*center/);
  assert.match(cssRule(css, ".placeholder > :first-child"), /margin-top:\s*auto/);
  assert.match(cssRule(css, ".placeholder > :last-child"), /margin-bottom:\s*auto/);
});

test("the button that folds the report stays at the foot of what scrolls", () => {
  const css = read("../style.css");
  assert.match(cssRule(css, ".gen-log-foot"), /position:\s*sticky/);
  assert.match(cssRule(css, ".gen-log-foot"), /bottom:\s*0/);
});

test("Generate runs the engine through generate_command, with --lenient", () => {
  // The Rust test reads the arguments of `generate_command`; this is the join
  // to the command itself. A `generate` that builds its own `Command` again
  // leaves that test green and a vault with findings "Failed" with its map on
  // disk. The test has to go through the entry point, not only the helper.
  const body = rustFn("generate");
  assert.match(body, /generate_command\(&engine, &root, full, &flags\)/);
  assert.doesNotMatch(body, /Command::new/, "generate builds its own command line");
  assert.match(rustFn("check_map"), /cmd\.arg\("--lenient"\)/);
});

test("the engine is not started on a project whose output is a link", () => {
  for (const name of ["generate", "check_map"]) {
    const body = rustFn(name);
    const guard = body.indexOf("refuse_linked_output(&root, &flags)?;");
    assert.ok(guard >= 0, `${name} does not refuse a linked output`);
    assert.ok(guard < body.indexOf("engine_info("), `${name}: the guard comes after the engine is looked up`);
    assert.ok(guard < body.indexOf("spawn_blocking"), `${name}: the guard comes after the engine is started`);
  }
});

test("the readers of the map read it through map_file", () => {
  for (const name of ["map_status", "view_search", "workspace_deps"]) {
    assert.match(rustFn(name), /map_file\(/, `${name} reads the map without map_file`);
  }
  const status = rustFn("map_status");
  assert.doesNotMatch(status, /Path::new\(&index\)/);
  assert.doesNotMatch(status, /join\("module_map\.json"\)/);
});

test("the commands that walk a repository's directories run off the main thread", () => {
  for (const name of ["map_status", "workspace_deps", "inspect_folder", "file_tree", "scan_languages"]) {
    const at = RUST.search(new RegExp(`\\bfn ${name}\\b`));
    assert.ok(at >= 0, name);
    assert.match(RUST.slice(Math.max(0, at - 120), at), /#\[tauri::command\(async\)\]/, `${name} is a sync command`);
  }
});

test("inspect_folder looks at .git without following it", () => {
  const body = rustFn("inspect_folder");
  assert.match(body, /languages::has_git_entry\(root_path\)/);
  assert.doesNotMatch(body, /join\("\.git"\)/);
});

// Commands run concurrently now, so answers arrive in the order they finish: a
// guard has to sit right after the await whose answer it protects, before
// anything is drawn from it.
function guardAfter(body, awaited, guard, before, what) {
  const at = body.indexOf(awaited);
  assert.ok(at >= 0, `${what}: no ${awaited}`);
  const g = body.indexOf(guard, at);
  assert.ok(g >= 0, `${what}: nothing guards the answer of ${awaited}`);
  const drawn = body.indexOf(before, at);
  assert.ok(drawn < 0 || g < drawn, `${what}: ${before} comes before the guard`);
}

test("an answer for a project the reader has left is dropped", () => {
  const detail = jsFn("async function renderDetail(");
  const stale = "if (p.id !== view.selectedId) return;";
  guardAfter(detail, "await mapStatus(", stale, "renderIcon(", "renderDetail");
  guardAfter(detail, "scanLanguages(", stale, "callsSupport = callsSupportFor", "renderDetail's scan");

  const sel = jsFn("async function select(");
  const left = "if (view.selectedId !== id) return;";
  guardAfter(sel, "await mapStatus(", left, "if (!status.exists)", "select");
  guardAfter(sel, "await scanLanguages(", left, "showPlaceholder(", "select");
  guardAfter(sel, 'await invoke("serve_project"', left, "mapBase = ", "select");

  const files = jsFn("async function renderFiles(");
  assert.ok(
    files.indexOf("const mine = ++filesSeq;") >= 0 &&
      files.indexOf("const mine = ++filesSeq;") < files.indexOf("if (!filesOpen || !view.selectedId) return;"),
    "renderFiles takes its ticket before it can return early"
  );
  guardAfter(files, 'await invoke("file_tree"', "mine !== filesSeq", "crumb.innerHTML", "renderFiles");
  // The success path is guarded as well as the catch branch: past the end of
  // the catch block and before the first line that draws the listing.
  const catchAt = files.indexOf("} catch");
  const catchEnd = files.indexOf("\n  }\n", catchAt);
  assert.ok(catchEnd > catchAt, "renderFiles: no catch block");
  const drawn = files.indexOf("crumb.innerHTML");
  const success = files.indexOf("mine !== filesSeq", catchEnd);
  assert.ok(success >= 0 && success < drawn, "renderFiles draws a stale listing");
});

test("the language badge belongs to one project and is cleared when another is shown", () => {
  const detail = jsFn("async function renderDetail(");
  // Another project's badge goes before anything is awaited (the pane already
  // shows the new name); the same project's stays until its scan answers.
  const awaited = detail.indexOf("await mapStatus(");
  const clear = detail.indexOf("clearLangs()");
  assert.match(detail, /els\.langs\.dataset\.project !== p\.id/);
  assert.ok(clear >= 0 && clear < awaited, "the badge of the previous project is still up while the answer is awaited");
  // The scan owns the badge: an empty answer and a failure take it off.
  const scan = detail.slice(detail.indexOf("scanLanguages("));
  const thenAt = scan.indexOf(".then(");
  const emptyClear = scan.indexOf("clearLangs()", thenAt);
  assert.ok(emptyClear > thenAt && emptyClear < scan.indexOf("els.langs.textContent = text"), "an empty scan leaves the old badge");
  assert.match(scan.slice(scan.indexOf(".catch(")), /clearLangs\(\)/);
});

test("a language switch redraws what main.js painted in script: both reports and the status line", () => {
  // The behaviour is in lib/run-report.js and lib/live-status.js (their tests
  // change the language and read the text); this is the join that calls them.
  const body = jsFn("function chooseLocale(");
  for (const call of ["runReport.refresh()", "taskLog.refresh()", "statusLine.refresh()"]) {
    assert.ok(body.includes(call), `chooseLocale does not call ${call}`);
  }
  // The run's outcome is a recipe, not a frozen string.
  assert.match(MAIN, /sayLive\(\(\) => \(res\.current \?/, "the check verdict stays in the old language");
});
