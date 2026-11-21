// The whole frontend: load the project list, show one project's map, switch.
//
// `convertFileSrc` is what makes the map viewable at all. The generated page
// is an ordinary file on disk, and a webview will not load `file://` into an
// iframe from an app-protocol page. Tauri's asset protocol exists for exactly
// this — it hands out a URL the webview will accept for a path the app is
// allowed to read (`assetProtocol.scope` in tauri.conf.json).

// A window that renders and then does nothing is the worst failure mode
// available here, and it is the one that happened: `window.__TAURI__` only
// exists when `app.withGlobalTauri` is set, so without it this module threw
// on its first line. The HTML and CSS still painted — a complete-looking
// sidebar with a button that ignored every click, and nothing anywhere
// saying why.
//
// So: say why, in the window. Every path below that can fail now ends up
// visible instead of in a console nobody has open.

// Both side-effect-free, so hoisting them above the `window.__TAURI__`
// guard below changes nothing about what that guard catches.
import { withBusyButton } from "./lib/busy-button.js";
import { imeComposing, onEnterSubmit } from "./lib/enter-submit.js";
import { escapeHtml, renderRich } from "./lib/rich-text.js";
import { closeOnBackdrop } from "./lib/backdrop.js";
import { nvimImportSummary } from "./lib/nvim-import.js";
import { lastKey, migrateLastKey } from "./lib/last-selection.js";
import { createStore } from "./lib/store.js";
import { mapStatus, invalidate } from "./lib/status-cache.js";
import { row as overviewRow, sortRows, summarize, RANK } from "./lib/overview.js";
import { usedBy, summarizeDeps } from "./lib/deps.js";
import {
  scopeToSub,
  scopeDisplay,
  splitMatch,
  mapTarget,
  truncatedKey,
  fileCount,
} from "./lib/finder.js";
import {
  headline as statsHeadline,
  composition as statsComposition,
  languageRows as statsLanguageRows,
  formatBytes,
} from "./lib/stats.js";
import { t, plural, setLocale, initialLocale, LOCALES, keys } from "./lib/i18n.js";
import { summarizeEngineLog, generatedMessage, PER_CHECK } from "./lib/engine-log.js";
import { createRunReport, createTaskLog } from "./lib/run-report.js";
import { createLiveStatus } from "./lib/live-status.js";
import { errorText, fillParams } from "./lib/errors.js";
import { contractVerdict } from "./lib/contract.js";
import {
  trend as trendOf,
  windows as trafficWindows,
  lineFor as trafficLine,
  compareTraffic,
  hasNumbers,
  isStale,
  ageInDays,
} from "./lib/traffic.js";
import {
  scanLanguages,
  invalidateLanguages,
  badgeText,
  summaryText,
  supportFor,
  callsSupportFor,
  grammarDiagnosis,
  engineLanguageText,
  engineVerdict,
} from "./lib/languages.js";
import {
  MAX_SELECTION_BYTES,
  list as rulesList,
  titleOf,
  laneOf,
  visibleRules,
  laneCounts,
  groupRules,
  orderedIds,
  checkState,
  toggleGroup,
  toggleRule,
  selectRange,
  selectManual,
  selectFamilies,
  splitSelection,
  runSelections,
  mergeRun,
  mechanicalCounts,
  sidebarCounts,
  judgedOf,
  notSendable,
  untrustedRulesets,
} from "./lib/rules.js";
import {
  backendName,
  defaultTitle as defaultRunTitle,
  egress,
  startBlocker,
  queueRows,
  proposalKey,
  tokensUsed,
  progress as runProgress,
  actionsFor as runActions,
  isLive as isRunLive,
  decideOffer,
  decisionKey,
  freshKey,
  waiverPaths,
} from "./lib/rules-run.js";
import {
  chatKey,
  newChat,
  chatArgs,
  egressFacts,
  relativeTo,
  addFile,
  attachedLine,
  canPropose,
} from "./lib/rules-chat.js";

function fatal(what, err) {
  const box = document.getElementById("placeholder");
  const frame = document.getElementById("map");
  if (frame) frame.hidden = true;
  if (!box) return;
  box.hidden = false;
  const h2 = document.createElement("h2");
  h2.textContent = what;
  const lead = document.createElement("p");
  lead.textContent = "This is a bug in docmap-desktop, not in the project you opened.";
  const detail = document.createElement("p");
  detail.className = "detail";
  detail.textContent = errorText(err);
  box.replaceChildren(h2, lead, detail);
}

window.addEventListener("error", (ev) => fatal("Something failed", ev.message));
window.addEventListener("unhandledrejection", (ev) => fatal("Something failed", ev.reason));

if (!window.__TAURI__) {
  // Reached when the page is opened outside the app, and when
  // `withGlobalTauri` is off. Naming both beats a TypeError three frames in.
  fatal(
    "Not running inside the app",
    "window.__TAURI__ is undefined. Either this page was opened directly in a " +
      "browser, or app.withGlobalTauri is not set in tauri.conf.json."
  );
  throw new Error("no tauri bridge");
}

const { invoke, convertFileSrc, Channel } = window.__TAURI__.core;
const { open, save } = window.__TAURI__.dialog;

// The web variant (web/, one HTML file opened from disk, the engine in the page) answers the same bridge from the
// browser and sets this beside it. Two things differ for this page there, and both are asked for by name below: the
// address of a map (a blob cannot carry `?theme=`), and a folder that changed on disk (nothing watches for the app).
const web = window.__DOCMAP_WEB__ ?? null;

const els = {
  sidebar: document.getElementById("sidebar"),
  sbPin: document.getElementById("sb-pin"),
  list: document.getElementById("projects"),
  sort: document.getElementById("proj-sort"),
  detail: document.getElementById("proj-detail"),
  icon: document.getElementById("proj-icon"),
  counts: document.getElementById("proj-counts"),
  langs: document.getElementById("proj-langs"),
  schema: document.getElementById("proj-schema"),
  stale: document.getElementById("proj-stale"),
  traffic: document.getElementById("proj-traffic"),
  trafficAge: document.getElementById("proj-traffic-age"),
  trafficDetailOpen: document.getElementById("proj-traffic-detail"),
  empty: document.getElementById("empty"),
  add: document.getElementById("add"),
  status: document.getElementById("status"),
  frame: document.getElementById("map"),
  placeholder: document.getElementById("placeholder"),
  runReport: document.getElementById("run-report"),
  runReportBody: document.getElementById("run-report-body"),
  runReportClose: document.getElementById("run-report-close"),
  overview: document.getElementById("overview"),
  ovHeadline: document.getElementById("ov-headline"),
  ovActions: document.getElementById("ov-actions"),
  ovGenBehind: document.getElementById("ov-gen-behind"),
  ovList: document.getElementById("ov-list"),
  ovDeps: document.getElementById("ov-deps"),
  ovDepsSum: document.getElementById("ov-deps-sum"),
  ovDepsList: document.getElementById("ov-deps-list"),
  ovDepsOutside: document.getElementById("ov-deps-outside"),
  gen: document.getElementById("gen"),
  engine: document.getElementById("engine"),
  engineSummary: document.getElementById("engine-summary"),
  engineState: document.getElementById("engine-state"),
  engineGrammars: document.getElementById("engine-grammars"),
  pickEngine: document.getElementById("pick-engine"),
  pickGrammars: document.getElementById("pick-grammars"),
  nvim: document.getElementById("nvim"),
  nvimSummary: document.getElementById("nvim-summary"),
  nvimState: document.getElementById("nvim-state"),
  contextNote: document.getElementById("context-note"),
  pickNvim: document.getElementById("pick-nvim"),
  pickNvimConfig: document.getElementById("pick-nvim-config"),
  pickNvimCache: document.getElementById("pick-nvim-cache"),
  help: document.getElementById("help"),
};


// What the engine says it can read. `null` until the probe has answered
// once — distinct from `{ languages: null }`, which is a *completed* probe
// against an engine too old to have the field. Everything downstream treats
// the two the same way (say nothing about support), but conflating them
// here would make "not asked yet" unobservable.

/**
 * Whether the selected project has call extraction behind it.
 *
 * Kept here rather than asked for when the note is rendered, because
 * `contextNoteFor` runs synchronously inside a `postMessage` handler and the
 * answer needs a directory walk. Recomputed with the language badge, which
 * is already fetched per selected project, and reset to `"unknown"` while
 * that is in flight — saying nothing is the right answer between two
 * projects, and a stale `"no"` would put a wrong sentence over the right
 * panel.
 *
 * @type {"yes"|"no"|"unknown"}
 */
/** What the grammars directory holds, for the diagnosis under the verdict.
    `null` until asked, and asked again after `set_grammars` — pointing at
    a different directory is precisely what changes the answer. */



// What the map frame is showing, kept with the other view state because
// `showPlaceholder` (far above where these used to be declared) has to reset
// them: a message that takes the map's place means there is no map in the
// frame, and everything that asks "is a map showing" reads `mapBase`.
//
// The frame's URL, without the theme. Kept so a theme change can rebuild it:
// the page is a separate document at a separate origin, so the `data-theme`
// this window stamps on itself never crosses -- which is why choosing dark
// used to leave the map white. It reads `?theme=` instead.

// The panel the page last reported through its one-way channel. A theme
// change reloads the frame, and landing back on Tree after having been in
// Notes would be the fix costing more than the bug.

// The blank-pane watchdog's timer, and whether the page has proved it ran.

/** The last selection for the workspace now open, or `null`.
 *
 * Never throws: a browser with storage disabled is a working app that
 * forgets, not a broken one. The keying rule and the migration live in
 * `lib/last-selection.js`, where they can be tested against a plain object
 * — a one-shot migration is hard to observe twice in a running window. */
function lastSelection() {
  try {
    return localStorage.getItem(lastKey(activeWorkspace));
  } catch (e) {
    void e;
    return null;
  }
}

// `escapeHtml` (lib/rich-text.js) turns a value into markup text for
// `showPlaceholder` and the other `renderRich` callers: language names come
// from a fixed table in Rust, but a path or a file name may follow.

// The view state of the window, in one store (`lib/store.js`). Code reads and
// assigns `view.<name>`; what has to follow a change subscribes to it below
// instead of every assigning place calling it by hand.
//
//   engine         the docmap engine: path, whether it came from PATH, bundled, grammars
//   engineLangs    what the engine says it can read (see the note above `probeEngineLanguages`)
//   grammarDir     what the grammars directory holds, for the diagnosis under the verdict
//   callsSupport   whether the selected project has call extraction behind it ("yes"|"no"|"unknown")
//   nvim           Neovim: path, config folder and where each came from
//   projects       the projects of the open workspace
//   selectedId     the selected project's id, or null
//   mapBase        the map frame's URL without the theme, or null when no map is showing
//   mapTab         the panel the page last reported through its one-way channel
//   mapWatch       the blank-pane watchdog's timer
//   mapLoaded      whether the page has proved it ran
const viewStore = createStore({
  engine: { path: null, from_path: true, bundled: false, grammars: null },
  // `null` until the probe has answered once -- distinct from `{ languages: null }`, a *completed* probe against
  // an engine too old to have the field.
  engineLangs: null,
  // `null` until asked, and asked again after `set_grammars`.
  grammarDir: null,
  callsSupport: "unknown",
  nvim: {
    path: null,
    from_path: true,
    config_dir: null,
    config_dir_from_default: true,
    cache_dir: null,
    cache_dir_from_default: true,
  },
  projects: [],
  selectedId: null,
  mapBase: null,
  mapTab: null,
  mapWatch: null,
  mapLoaded: false,
});
const view = viewStore.state;

const statusLine = createLiveStatus(els.status);

function say(msg) {
  statusLine.say(msg);
}

/**
 * A status line that is a sentence of the catalog (a run's outcome, the
 * counts in it): `say` keeps the words it was given, this keeps the way to
 * make them, so a language switch rewords the line.
 */
function sayLive(make) {
  statusLine.sayLive(make);
}

/**
 * Make a list row act like a button: focusable, announced as one, and
 * answering Enter and Space as well as a click. The rows that open a file were
 * click-only `<li>`s, unreachable from the keyboard.
 */
function makeOpenable(li, handler) {
  li.tabIndex = 0;
  li.setAttribute("role", "button");
  li.addEventListener("click", handler);
  li.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" || ev.key === " ") {
      ev.preventDefault();
      handler();
    }
  });
}

/** Show a message in the view area instead of a map. */
function showPlaceholder(title, body) {
  els.frame.hidden = true;
  els.frame.removeAttribute("src");
  // The message takes the map's place, so there is no map in the frame any
  // more. Left set, `setPane("map")` un-hid an iframe with no page in it and
  // hid the message, a theme switch loaded the old page into the hidden
  // frame, and the blank-pane watchdog kept guarding a frame nobody meant to
  // show.
  view.mapBase = null;
  view.mapTab = null;
  if (view.mapWatch) {
    clearTimeout(view.mapWatch);
    view.mapWatch = null;
  }
  // The overview answers the same empty screen. A message about one project
  // shown over a list of all of them is two subjects at once.
  els.overview.hidden = true;
  // Whatever is said here replaces the view, so the last run's report (a
  // different subject, drawn over a map that is no longer there) goes too.
  hideRunReport();
  taskLog.clear();
  els.placeholder.hidden = false;
  const h2 = document.createElement("h2");
  h2.textContent = title;
  const p = document.createElement("p");
  renderRich(p, body);
  els.placeholder.replaceChildren(h2, p);
  // Overlays the iframe specifically; nothing to explain about a panel
  // that is not even on screen.
  els.contextNote.hidden = true;
}

/**
 * A message about a *task* (generate, check): shown over the map, and
 * brought to the front. The Files and Statistics panes are opaque overlays, so
 * a message painted under one was never seen — the engine's report for
 * "Check exactly" went to a placeholder nobody could look at.
 */
function showTaskPlaceholder(title, body) {
  showPlaceholder(title, body);
  if (view.selectedId && (filesOpen || statsOpen || rulesOpen)) setPane("map");
}

// =====================================================================
// Interface language
//
// I18N.md's phase I18N-4 and only that: this window's chrome. The generated
// map is a separate artifact with its own translation, which is why the
// language control says so in its own help text.
//
// Applied by walking `data-i18n*` attributes rather than by rebuilding the
// DOM: the markup keeps its English text as the in-file default, so a window
// whose script never runs is still readable — the same reason the fatal
// handler exists at all.
// =====================================================================
const LANG_KEY = "docmap.locale";

function applyLocale() {
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const text = t(el.dataset.i18n);
    if (!text) return;
    // Several of these carry <strong>/<code>: `renderRich` builds those as
    // nodes, and only those tags (lib/rich-text.js).
    renderRich(el, text);
  });
  document.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
    const text = t(el.dataset.i18nPlaceholder);
    if (text) el.placeholder = text;
  });
  document.querySelectorAll("[data-i18n-aria]").forEach((el) => {
    const text = t(el.dataset.i18nAria);
    if (text) el.setAttribute("aria-label", text);
  });
  // Help bubbles read `data-help`, so the catalog is projected onto it.
  document.querySelectorAll("[data-i18n-help]").forEach((el) => {
    const text = t(el.dataset.i18nHelp);
    if (text) el.dataset.help = text;
  });
  document.documentElement.lang = locale;
}

let locale = "en";
{
  let saved = null;
  try {
    saved = localStorage.getItem(LANG_KEY);
  } catch (e) {
    void e;
  }
  locale = setLocale(initialLocale(saved, navigator.language), {
    // Marks every fallback visibly, so an unfinished locale is countable
    // rather than merely embarrassing.
    debug: new URLSearchParams(location.search).get("i18n") === "debug",
  });

  const sel = document.getElementById("lang");
  LOCALES.forEach((l) => {
    const o = document.createElement("option");
    o.value = l.code;
    // Endonym, and set as text rather than through the catalog: a language
    // name is not a string to translate.
    o.textContent = l.label;
    sel.append(o);
  });
  sel.value = locale;
  sel.addEventListener("change", () => chooseLocale(sel.value));
  applyLocale();
}

// =====================================================================
// Theme
//
// Three states, and "system" is one of them: a two-way toggle can only ever
// leave a reader pinned to a choice they made once, with no way to hand the
// decision back to the OS. Nothing stamped on <html> means system; a
// `data-theme` attribute means they decided.
//
// Applied before anything else renders, and read from localStorage rather
// than from the workspace file: this is a property of *this machine's* eyes,
// not of the project list, and syncing it through workspace.json would carry
// one machine's lighting preference to another.
// =====================================================================
const THEME_KEY = "docmap.theme";

function applyTheme(choice) {
  if (choice === "light" || choice === "dark") {
    document.documentElement.setAttribute("data-theme", choice);
  } else {
    // Removed, not set to "system": the CSS keys off the attribute's
    // absence, so a third value would be a state the stylesheet never sees.
    document.documentElement.removeAttribute("data-theme");
  }
}

{
  let saved = "system";
  try {
    saved = localStorage.getItem(THEME_KEY) || "system";
  } catch (e) {
    // A window that cannot remember the choice still honours it for this
    // session, which is strictly better than refusing to apply it.
    void e;
  }
  applyTheme(saved);
  const sel = document.getElementById("theme");
  sel.value = saved;
  // Through the same function the menu calls: one setter, so the select
  // and the checkmark can never end up describing different windows.
  sel.addEventListener("change", () => chooseTheme(sel.value));
}

// =====================================================================
// Button help
//
// Every sidebar control carries a `data-help` sentence saying what it does.
// The distinctions are the reason it exists at all: Generate map and
// Generate all differ in *what they overwrite*, not in speed, and Locate…
// exists only for the case where PATH does not already answer — none of
// which a five-word label can carry.
//
// A native `title` would have been free, and was turned down for one
// reason: it never appears on keyboard focus, so exactly the controls a
// keyboard user reaches would be the only ones with no explanation. The
// texts themselves are lifted from `docs/USAGE.md` rather than reworded, so
// there is one description of each button rather than two that can drift.
//
// Same rules as the generated page's keyword card, so the two surfaces
// behave alike: a deliberate hover rather than a pointer crossing the
// button, Escape closes, and it never covers the control it describes.
// =====================================================================
const HELP_DWELL = 400;
let helpTimer = null;

function hideHelp() {
  if (helpTimer) {
    clearTimeout(helpTimer);
    helpTimer = null;
  }
  els.help.hidden = true;
}

function showHelp(el) {
  const text = el.dataset.help;
  if (!text) return;
  els.help.textContent = text;
  els.help.hidden = false;

  // Measured after it is in the DOM, then placed beside the control rather
  // than under it: the sidebar is narrow and a bubble below a button covers
  // the next one, which is the button the reader is most likely comparing
  // it against.
  const a = el.getBoundingClientRect();
  const r = els.help.getBoundingClientRect();
  const left = Math.min(a.right + 8, window.innerWidth - r.width - 8);
  const top = Math.max(8, Math.min(a.top, window.innerHeight - r.height - 8));
  els.help.style.left = left + "px";
  els.help.style.top = top + "px";
}

document.addEventListener("mouseover", (ev) => {
  const el = ev.target.closest && ev.target.closest("[data-help]");
  if (helpTimer) {
    clearTimeout(helpTimer);
    helpTimer = null;
  }
  if (el) {
    helpTimer = setTimeout(() => showHelp(el), HELP_DWELL);
  } else if (!els.help.hidden) {
    hideHelp();
  }
});

// Keyboard parity — the whole reason this is not a `title`.
document.addEventListener("focusin", (ev) => {
  const el = ev.target.closest && ev.target.closest("[data-help]");
  if (!el) return hideHelp();
  if (helpTimer) {
    clearTimeout(helpTimer);
    helpTimer = null;
  }
  // A folded sidebar is still sliding in when this runs, and a bubble placed
  // against the control's rectangle at that instant lands on the sidebar's
  // own edge, over the control it describes. Wait for the slide (160 ms).
  if (document.body.classList.contains("sb-auto") && els.sidebar.contains(el)) {
    helpTimer = setTimeout(() => {
      helpTimer = null;
      if (document.activeElement === el) showHelp(el);
    }, 200);
  } else {
    showHelp(el);
  }
});
document.addEventListener("focusout", hideHelp);
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") hideHelp();
});
// A bubble positioned against a rectangle that has since moved is worse
// than none.
window.addEventListener("resize", hideHelp);
window.addEventListener("scroll", hideHelp, true);

// =====================================================================
// The project picker
//
// Was a list of rows; with thirty-three repositories the list *was* the
// sidebar. A `<select>` shows the one being looked at, and the detail that
// used to be on every row is shown for that one below it.
//
// What a list could do and this cannot is show every project's staleness at
// once. Sorting answers that instead — "which need regenerating" becomes one
// action rather than one glance.
// =====================================================================

const SORT_KEY = "docmap.sort";

/** Freshness per project id, so switching back does not re-walk a tree. */
const freshness = new Map();

let sortBy = "name";
try {
  sortBy = localStorage.getItem(SORT_KEY) || "name";
} catch (e) {
  void e;
}

/**
 * Projects in the chosen order.
 *
 * `added` is the workspace's own order and is therefore the identity sort —
 * it is offered because it is the only one that does not move when something
 * else changes, and a list that reorders itself while you are using it is one
 * you cannot build a habit with.
 */
function sortedProjects() {
  const list = view.projects.slice();
  if (sortBy === "name") {
    list.sort((a, b) => a.name.localeCompare(b.name));
  } else if (sortBy === "stale") {
    // Stale first, and within that the furthest behind first. A project
    // whose freshness has not been measured yet sorts as "not known to be
    // stale" rather than as fresh — it moves once the answer arrives.
    list.sort((a, b) => {
      const fa = freshness.get(a.id);
      const fb = freshness.get(b.id);
      const sa = fa && fa.stale ? (fa.behindSecs ?? 1) : 0;
      const sb = fb && fb.stale ? (fb.behindSecs ?? 1) : 0;
      if (sa !== sb) return sb - sa;
      return a.name.localeCompare(b.name);
    });
  } else if (sortBy === "generated") {
    // Oldest map first, because the question is "which have I left alone the
    // longest" — the same question staleness answers for a tree that moved,
    // and the one it cannot answer for a tree that did not: a repository
    // nobody has touched stays un-stale forever however old its map is.
    //
    // A project with no map sorts first. That is an ordering decision and
    // deliberately not a verdict — §3's rule that absent is not behind still
    // holds, and the mark still says so. Never generated is simply the
    // extreme end of the question this order asks.
    list.sort((a, b) => {
      const fa = freshness.get(a.id);
      const fb = freshness.get(b.id);
      const age = (f) => (f && f.hasMap ? (f.generatedSecs ?? 0) : Infinity);
      const aa = age(fa);
      const ab = age(fb);
      if (aa !== ab) return ab - aa;
      return a.name.localeCompare(b.name);
    });
  } else if (sortBy === "traffic") {
    // Most views in the last 30 days first. A project with no numbers — no
    // GitHub remote, no plugin, not tracked — comes after every project that
    // has some, zero included, and those tie on name: "nobody looked" and
    // "we cannot tell whether anybody looked" are not the same rank.
    list.sort(
      (a, b) =>
        compareTraffic(trafficEntries.get(a.id), trafficEntries.get(b.id)) ||
        a.name.localeCompare(b.name)
    );
  }
  return list;
}

/**
 * Measure every project that has not been measured yet.
 *
 * Sorting by staleness reads `freshness`, and `freshness` is filled in as
 * projects are selected — so before this existed, choosing that order on a
 * fresh window sorted by a map with one entry in it and quietly produced
 * alphabetical order. A sort control that appears to work and does not is
 * worse than one that takes a moment.
 *
 * Sequential rather than all at once: each call is a directory walk, and
 * thirty-three of them in parallel is thirty-three filesystem walks
 * competing for the same disk.
 */
async function measureAll() {
  const pending = view.projects.filter((p) => !freshness.has(p.id));
  if (!pending.length) return;
  els.sort.disabled = true;
  try {
    for (const p of pending) {
      try {
        freshness.set(p.id, await invoke("map_freshness", { id: p.id }));
      } catch (e) {
        // Not knowing is not the same as fresh, but it is the only thing
        // this can do about a project it cannot read. It stays unmeasured
        // and sorts with the ones that are not behind.
        void e;
      }
    }
  } finally {
    els.sort.disabled = false;
  }
}

async function render() {
  els.list.replaceChildren();
  els.empty.hidden = view.projects.length > 0;
  els.list.hidden = view.projects.length === 0;
  // Ordering is a question about *many* projects. With one selected there
  // is nothing to order, so the control only exists on the overview.
  els.sort.hidden = view.projects.length < 2 || !!view.selectedId;

  // Nothing selected is a state a `<select>` cannot express on its own: it
  // always has one option selected, so with no selection the control named
  // whichever project sorted first while the pane beside it said "nothing
  // selected" and Generate stayed disabled. Two answers to one question, in
  // adjacent controls. Measured after workspaces gained their own last
  // selection, which is when a switch can legitimately land on nothing.
  //
  // It used to be disabled and hidden — it existed only to be the closed
  // control's label, because choosing it led to a screen that said "pick a
  // project on the left". That is no longer true: no selection now shows the
  // workspace overview, which is a destination rather than a dead end, so
  // the option is a real row and always present.
  if (view.projects.length) {
    const none = document.createElement("option");
    none.value = "";
    none.textContent = t("picker.overview");
    els.list.append(none);
  }

  for (const p of sortedProjects()) {
    const o = document.createElement("option");
    o.value = p.id;
    // A mark inside the option, because the closed control shows exactly one
    // line and this is the one thing worth carrying into it.
    const f = freshness.get(p.id);
    o.textContent = (f && f.stale ? "• " : "") + p.name + rulesMark(p.id);
    els.list.append(o);
  }

  // `selectedId` or nothing. This used to fall back to whatever the control
  // showed before the re-render, which reads as harmless and is not: after a
  // workspace switch the previous value is a project from the workspace you
  // just left, so the picker went on naming it while `selectedId` was null,
  // the pane said "nothing selected" and Generate was disabled. Measured in
  // a browser -- the structural test for the empty state passed while this
  // was still wrong, because the option was being added and then overruled
  // one line later.
  els.list.value = view.selectedId ?? "";
  // The project bar (views and search) belongs to a selected project.
  document.body.classList.toggle("has-bar", !!view.selectedId);
  document.getElementById("projbar").hidden = !view.selectedId;
  renderDetail();
  renderOverview();

  // The overview ranks on freshness, and freshness arrives from a walk over
  // every tree. Kicking it off here rather than only from the sort control
  // is what makes the first screen after launch worth reading: without it
  // the list rendered once, said "checking…" against everything, and never
  // heard back — the measurement only ran when someone chose to sort by it.
  if (!view.selectedId && view.projects.length) {
    measureAll().then(() => {
      // Only if nothing has been opened in the meantime; a walk finishing
      // after the reader picked a project must not redraw over their map.
      if (!view.selectedId) renderOverview();
    });
  }
}

// =====================================================================
// The workspace overview
//
// Shown where "Nothing selected" used to be, because that state is not an
// absence of subject — it *is* the workspace, and this app is the only
// place a workspace exists. One repository's map cannot show it and the
// picker cannot either: the picker names one project and the detail pane
// describes that one.
//
// The ranking is `lib/overview.js`'s, and the measurement behind it is
// written down there. The short version, because it decided the layout:
// "behind the engine" leads and "stale" does not, even though staleness is
// the louder number — over 30 generated maps, 27 were behind on schema and
// 28 were stale, and for 17 of those the newest file was a `.gitignore`
// touched in one sweep.
//
// Freshness is whatever `measureAll` has gathered so far. Rows render
// immediately with what is known and say "checking…" for the rest, rather
// than holding the whole screen back for a walk over thirty trees: the
// ranking is useful before it is complete, and an empty window is not.
// =====================================================================

/** A rough age, in the largest unit that still reads as a number (`plural` is in `lib/i18n.js`). */
function agoText(secs) {
  if (typeof secs !== "number" || secs < 0) return null;
  if (secs < 3600) return plural("ov.ago.min", Math.max(1, Math.round(secs / 60)));
  if (secs < 86400) return plural("ov.ago.hour", Math.round(secs / 3600));
  return plural("ov.ago.day", Math.round(secs / 86400));
}

/** The one sentence a row puts under the project name. */
function stateText(r) {
  if (r.hasMap === null) return t("ov.state.unknown");
  if (!r.hasMap) return t("ov.state.noMap");
  // The lag still orders the rows — it just is not read out. "1 version(s)
  // back" is a number the reader can do nothing different with: the action
  // for one version behind and for three is the same button.
  if (r.schemaLag > 0) return t("ov.state.behind");
  if (r.stale === true) {
    const ago = agoText(r.behindSecs);
    return ago ? t("ov.state.stale.for").replace("{ago}", ago) : t("ov.state.stale");
  }
  if (r.stale === null) return t("ov.state.unknown");
  // The walk hit its file cap, so "nothing newer" is only "nothing newer
  // among the files it reached". Saying `up to date` here would be a
  // verdict the data does not support.
  return r.truncated ? t("ov.state.maybe") : t("ov.state.ok");
}

const RANK_CLASS = {
  [RANK.NO_MAP]: "ov-nomap",
  [RANK.BEHIND_SCHEMA]: "ov-behind",
  [RANK.STALE]: "ov-stale",
  [RANK.OK]: "ov-ok",
};

/** Build the rows from whatever is known right now. */
async function overviewRows() {
  const engineSchema = view.engineLangs && view.engineLangs.schema;
  const out = [];
  for (const p of view.projects) {
    let status = null;
    try {
      status = await mapStatus(invoke, p.map_dir);
    } catch (e) {
      // Unreadable is not "no map": one is a fact about the project, the
      // other about this moment. `null` keeps it unmeasured, which is
      // where the ranking already puts what it does not know.
      void e;
    }
    out.push(overviewRow(p, status, freshness.get(p.id) ?? null, engineSchema));
  }
  return sortRows(out);
}

/**
 * Who depends on whom, across the workspace.
 *
 * This is `HOSTING.md`'s "the artifact is the extension point" with a
 * consumer attached: nothing here asks the engine anything. `requires_external`
 * is exactly where one repository's map runs out of tree — it records that a
 * module outside was required and cannot say where it lives, because it never
 * saw it. Several artifacts can, and this window is the only place that holds
 * several.
 *
 * Failure is silent by design. The block simply does not appear: a workspace
 * whose projects have no maps yet has nothing to say here, and that is the
 * common case on a first run rather than an error worth a sentence.
 */
async function renderDeps() {
  els.ovDeps.hidden = true;
  let deps = null;
  try {
    deps = await invoke("workspace_deps");
  } catch (e) {
    void e;
    return;
  }

  // Cached for the matrix dialog, which reads this instead of asking the
  // engine a second time -- the same reasoning `repos` is cached for the
  // GitHub pick-list.
  lastDeps = deps;

  const s = summarizeDeps(deps);
  if (s.edges === 0 && s.outsideNames === 0) return;

  const names = {};
  for (const p of view.projects) names[p.id] = p.name;
  const rows = usedBy(deps.edges, names);

  els.ovDeps.hidden = false;
  els.ovDepsSum.textContent = rows.length
    ? t("deps.summary.some").replace("{n}", String(rows.length))
    : t("deps.summary.none");

  els.ovDepsList.replaceChildren();
  for (const r of rows) {
    const li = document.createElement("li");

    const name = document.createElement("span");
    name.className = "ov-deps-name";
    name.textContent = r.name;

    const who = document.createElement("span");
    who.className = "ov-deps-who";
    who.textContent =
      plural("deps.usedBy", r.users.length) + " · " + plural("deps.sites", r.sites);

    li.append(name, who);

    // Three names, because the list exists to answer "which parts" and a
    // full one is `lib.nvim`'s 124 modules. The rank is how many projects
    // reach for each, which is the only cross-project count the edge
    // carries — and the more useful one anyway.
    if (r.modules.length) {
      const mods = document.createElement("span");
      mods.className = "ov-deps-mods";
      mods.textContent = t("deps.modules").replace("{list}", r.modules.slice(0, 3).join(", "));
      li.append(mods);
    }

    els.ovDepsList.append(li);
  }

  // Third-party names get one line rather than a list. They are not projects
  // in this workspace, nothing here can act on them, and the fact worth
  // carrying is that they exist and roughly how many.
  const parts = [];
  const top = (deps.outside || []).slice(0, 5).map((o) => o.name);
  if (top.length) {
    const rest = (deps.outside || []).length - top.length;
    parts.push(
      rest > 0
        ? plural("deps.outside", rest).replace("{list}", top.join(", "))
        : t("deps.outside.all").replace("{list}", top.join(", "))
    );
  }
  if (s.unread > 0) parts.push(plural("deps.unread", s.unread));
  els.ovDepsOutside.textContent = parts.join(" ");
}

// ---------------------------------------------------------------------
// The dependency matrix
//
// The same edges `renderDeps` already fetched, as a picture instead of a
// list: `usedBy` folds edges into one row per *target* project, which is
// the right shape for "who finds out if I change this" but throws away who
// specifically reaches for what -- a 30-project workspace with 49 edges is
// a wall of text as a list and a shape at a glance as a grid. Rows require
// columns, matching `deps.rs`'s own `Edge { from, to }` direction.
//
// A matrix rather than a node-link graph, deliberately: no layout algorithm
// to write and maintain (this app ships no charting library — see the
// README's "no CDN, no build step" — a force simulation would be the one
// exception), and a grid stays readable at the sizes this workspace
// actually has, where a tangle of crossing arrows would not.
// ---------------------------------------------------------------------

/** The last `workspace_deps` result, cached by `renderDeps` so opening the
 *  matrix dialog asks the engine nothing a second time. */
let lastDeps = null;

const matrixbox = {
  el: document.getElementById("matrixbox"),
  wrap: document.getElementById("matrix-wrap"),
  table: document.getElementById("matrix-table"),
  empty: document.getElementById("matrix-empty"),
};

function renderMatrix() {
  const names = {};
  for (const p of view.projects) names[p.id] = p.name;

  const edges = (lastDeps && lastDeps.edges) || [];
  const ids = [...new Set(edges.flatMap((e) => [e.from, e.to]))].sort((a, b) =>
    (names[a] ?? a).localeCompare(names[b] ?? b)
  );

  matrixbox.table.replaceChildren();
  matrixbox.empty.hidden = ids.length > 0;
  matrixbox.wrap.hidden = ids.length === 0;
  if (ids.length === 0) return;

  const byPair = new Map(edges.map((e) => [`${e.from}\u0000${e.to}`, e]));
  const max = edges.reduce((m, e) => Math.max(m, e.count), 1);

  const thead = document.createElement("thead");
  const headRow = document.createElement("tr");
  headRow.append(document.createElement("th"));
  for (const id of ids) {
    const th = document.createElement("th");
    const span = document.createElement("span");
    span.textContent = names[id] ?? id;
    th.append(span);
    th.title = names[id] ?? id;
    headRow.append(th);
  }
  thead.append(headRow);
  matrixbox.table.append(thead);

  const tbody = document.createElement("tbody");
  for (const from of ids) {
    const tr = document.createElement("tr");
    const rowHead = document.createElement("th");
    rowHead.scope = "row";
    rowHead.textContent = names[from] ?? from;
    rowHead.title = names[from] ?? from;
    tr.append(rowHead);

    for (const to of ids) {
      const td = document.createElement("td");
      if (from === to) {
        td.className = "matrix-self";
      } else {
        const e = byPair.get(`${from}\u0000${to}`);
        if (e) {
          td.className = "matrix-hit";
          // 15-70%, never fully opaque: the count is still legible on the
          // heaviest cell, which a solid fill at the max would not be.
          td.style.setProperty("--v", String(Math.round(15 + 55 * (e.count / max))));
          td.textContent = String(e.count);
          const mods = e.modules.slice(0, 8).join(", ") + (e.modules.length > 8 ? "…" : "");
          td.title =
            `${names[from] ?? from} → ${names[to] ?? to}: ${plural("deps.sites", e.count)}\n${mods}`;
        }
      }
      tr.append(td);
    }
    tbody.append(tr);
  }
  matrixbox.table.append(tbody);
}

document.getElementById("ov-deps-matrix").addEventListener("click", () => {
  renderMatrix();
  matrixbox.el.showModal();
});

async function renderOverview() {
  const show = !view.selectedId && view.projects.length > 0;
  els.overview.hidden = !show;
  if (!show) return;

  // The placeholder and the overview are two answers to the same empty
  // screen, and both showing at once is how that screen used to look for a
  // frame after a workspace switch.
  els.placeholder.hidden = true;

  const rows = await overviewRows();
  const s = summarize(rows);
  const needs = s.noMap + s.behindSchema + s.stale;

  const parts = [];
  parts.push(
    needs === 0 && s.unmeasured === 0
      ? t("ov.headline.allGood").replace("{total}", String(s.total))
      : t("ov.headline.needs")
          .replace("{n}", String(needs))
          .replace("{total}", String(s.total))
  );
  if (s.unmeasured > 0) {
    parts.push(t("ov.headline.measuring").replace("{n}", String(s.unmeasured)));
  }
  parts.push(t("ov.pick"));
  els.ovHeadline.textContent = parts.join(" ");

  // Acting on the schema lag is the one bulk command the menu does not
  // already have: `generate stale` reads modification times, and a map
  // written by an older engine is not stale by that measure — it is
  // exactly as old as its sources and still missing what the engine can
  // do now. Same machinery, different set.
  const behind = rows.filter((r) => r.rank === RANK.BEHIND_SCHEMA).map((r) => r.id);
  els.ovActions.hidden = behind.length === 0 || !view.engine.path;
  els.ovGenBehind.textContent = plural("ov.genBehind", behind.length);
  els.ovGenBehind.onclick = () => generateAll(new Set(behind));

  els.ovList.replaceChildren();
  for (const r of rows) {
    const li = document.createElement("li");
    li.className = "ov-row " + (RANK_CLASS[r.rank] || "");
    li.dataset.id = r.id;

    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ov-open";
    // `textContent`, not innerHTML: a project name is the user's content
    // and travels verbatim, the rule the whole catalog is built on.
    const name = document.createElement("span");
    name.className = "ov-name";
    name.textContent = r.name;
    const state = document.createElement("span");
    state.className = "ov-state";
    state.textContent = stateText(r);
    btn.append(name, state);

    if (r.modules !== null || r.files !== null) {
      const counts = document.createElement("span");
      counts.className = "ov-counts";
      counts.textContent = t("ov.counts")
        .replace("{modules}", String(r.modules ?? 0))
        .replace("{files}", String(r.files ?? 0));
      btn.append(counts);
    }

    btn.addEventListener("click", () => select(r.id));
    li.append(btn);
    els.ovList.append(li);
  }

  // After the list, and not awaited into it: the graph reads every artifact
  // in the workspace, and the ranked list is the thing worth having on
  // screen first. The traffic figures follow for the same reason — the
  // first ask may run `git` once per project.
  renderDeps();
  fillOverviewTraffic();
}

/** Take the language badge off the screen, and with it whose it was. */
function clearLangs() {
  els.langs.hidden = true;
  els.langs.textContent = "";
  els.langs.removeAttribute("title");
  delete els.langs.dataset.project;
}

/** What the row used to carry, for the selected project only. */
async function renderDetail() {
  const p = view.projects.find((x) => x.id === view.selectedId);
  els.detail.hidden = !p;
  if (!p) return;

  // The badge belongs to the project whose scan set it. Another project's is
  // taken off before anything is awaited - the pane is already showing the new
  // name - but a re-render of the same project (after Generate, a traffic
  // refresh) keeps its badge until the new scan answers, rather than blanking
  // it for as long as the walk takes.
  if (els.langs.dataset.project !== p.id) clearLangs();

  const status = await mapStatus(invoke, p.map_dir);
  // Commands answer in the order they finish, not the order they were asked:
  // by now another project may be the one on screen.
  if (p.id !== view.selectedId) return;
  renderIcon(p.id);
  renderCounts(status);

  // A different question from "are the sources newer": this one is about
  // the *engine*, and regenerating is the answer to both.
  const older = schemaNote(status);
  els.schema.textContent = older || "";
  els.schema.hidden = !older;

  // Not awaited with the above: this walks the project directory, and
  // blocking the sidebar on a filesystem walk would trade a rendered panel
  // for a blank one.
  view.callsSupport = "unknown";
  scanLanguages(invoke, p.root, p.map_dir)
    .then((scan) => {
      if (p.id !== view.selectedId) return;
      // Set before the early return below: a project whose badge is empty
      // (no language clears the threshold) still has a Calls panel, and the
      // note over it is exactly as true.
      view.callsSupport = callsSupportFor(scan, view.engineLangs);
      const text = badgeText(scan);
      // The scan is the one that owns the badge: an empty answer takes it off.
      if (!text) {
        clearLangs();
        return;
      }
      els.langs.textContent = text;
      els.langs.title = summaryText(scan, supportFor(scan, view.engineLangs));
      els.langs.dataset.project = p.id;
      els.langs.hidden = false;
    })
    .catch(() => {
      // An unreadable directory costs its language line and nothing else.
      if (p.id === view.selectedId) clearLangs();
    });

  refreshFreshness(p.id);
  renderTraffic(p);
  countRulesFor();
}

/**
 * Ask whether the map has fallen behind, and say so.
 *
 * Worded as what it measures. `freshness.rs` compares modification times, so
 * this can say *sources are newer* and must not say *the map is wrong* — a
 * file saved without an edit in it counts here and would not change a byte
 * of the artifact.
 */
/**
 * The line under the picker for a project that has no map yet.
 *
 * It used to carry the map's own counts as links (modules, namespaces,
 * files, errors, warnings). Those are in the map's header, next to the
 * project name, so the sidebar copy said everything twice and was dropped.
 */
function renderCounts(status) {
  els.counts.hidden = status.exists;
  els.counts.textContent = status.exists ? "" : t("detail.nomap");
}

/**
 * Send the map to a view, by navigating the frame.
 *
 * Costs a reload, and that is the deliberate price of the page taking no
 * instructions: a host that could tell an embedded page what to do is a
 * different kind of program than one that can only ask it questions.
 */
function gotoMap(state) {
  if (!view.mapBase) return;
  const parts = Object.keys(state)
    .filter((k) => state[k])
    .map((k) => k + "=" + encodeURIComponent(state[k]));
  view.mapTab = null;
  const url = mapUrl(view.mapBase).split("#")[0] + "#" + parts.join("&");
  els.frame.src = url;
  // No watchdog here. This changes the fragment of the page that is already
  // loaded: a same-document navigation, so nothing loads and the page posts
  // only when its tab or view *changes* — a jump within the tab it is on says
  // nothing, and a watchdog armed for it fired 8 seconds later on a map that
  // was working. The one `select()` armed for the load still guards that.
}

/**
 * Show the project's own icon, if it has one.
 *
 * Hidden until the file has actually decoded: an `<img>` whose `src` fails
 * leaves a broken-image glyph, which is worse than the nothing most
 * repositories correctly get.
 */
async function renderIcon(id) {
  els.icon.hidden = true;
  els.icon.removeAttribute("src");
  try {
    const path = await invoke("project_icon", { id });
    if (!path || id !== view.selectedId) return;
    els.icon.onload = () => {
      if (id === view.selectedId) els.icon.hidden = false;
    };
    els.icon.onerror = () => {
      els.icon.hidden = true;
    };
    els.icon.title = path;
    els.icon.src = convertFileSrc(path);
  } catch (e) {
    // A project whose icon cannot be looked for is a project without one.
    void e;
  }
}

/**
 * Say when a map was written by an older engine than the one installed.
 *
 * This is the failure that cost an evening: the theme fix shipped, the app
 * had it, and the map on disk did not — because a generated page is baked
 * at generation time and the engine that baked it was older. From the
 * outside that is indistinguishable from a broken feature.
 *
 * Both halves are now readable: the engine reports the schema it writes,
 * and the artifact carries the schema it was written with. An engine that
 * reports nothing is simply older than the field, and then this says
 * nothing rather than guessing — the comparison needs both sides.
 */
function schemaNote(status) {
  const engineSchema = view.engineLangs && view.engineLangs.schema;
  if (!engineSchema || !status.exists) return null;
  // A map with no schema at all predates the field, which is strictly
  // older than any engine that reports one.
  const mapSchema = status.schema ?? 0;
  if (mapSchema >= engineSchema) return null;
  return t("detail.oldSchema")
    .replace("{map}", mapSchema || "?")
    .replace("{engine}", engineSchema);
}

async function refreshFreshness(id) {
  els.stale.hidden = true;
  try {
    const f = await invoke("map_freshness", { id });
    freshness.set(id, f);
    if (id !== view.selectedId) return;
    if (!f.hasMap || !f.stale) return;
    els.stale.textContent = t("detail.stale");
    // The help bubble, not `title`: it also appears on keyboard focus.
    els.stale.dataset.help = t("detail.staleWhy");
    els.stale.hidden = false;
    // The mark in the list, now that the answer is known.
    const option = [...els.list.options].find((o) => o.value === id);
    if (option && !option.textContent.startsWith("• ")) {
      option.textContent = "• " + option.textContent;
    }
  } catch (e) {
    // Not knowing whether a map is behind is not a reason to say it is.
    void e;
  }
}

els.list.addEventListener("change", () => select(els.list.value));

els.sort.value = sortBy;

/**
 * Adopt a sort order, from wherever it was chosen.
 *
 * Two controls now set the same thing — the sidebar dropdown and the one in
 * Settings — so the write, the persistence and the "measure before you can
 * order by it" rule live here rather than twice. The other control is
 * updated too, because both are on screen in the same session and a pair
 * that disagrees about the current order is worse than having only one.
 *
 * @param {string} value
 */
async function applySort(value) {
  sortBy = value;
  els.sort.value = value;
  const inPrefs = document.getElementById("prefs-sort");
  if (inPrefs) inPrefs.value = value;
  try {
    localStorage.setItem(SORT_KEY, sortBy);
  } catch (e) {
    void e;
  }
  // The answer has to exist before it can be ordered by.
  // Both orders read `freshness`, which is otherwise filled in only as
  // projects are selected — so both have to wait for the walk, or the sort
  // silently falls back to alphabetical on a fresh window.
  if (sortBy === "stale" || sortBy === "generated") await measureAll();
  // Same rule for the order that reads the digest: it has to have been read.
  if (sortBy === "traffic") await measureTraffic();
  render();
}

els.sort.addEventListener("change", () => applySort(els.sort.value));

/** Re-open whatever this workspace had open last, if it is still there.
 *
 * Silent when it is not: a project removed since, or a workspace never
 * opened, is not an error and has nothing to report. */
async function restoreLast() {
  const last = lastSelection();
  if (last && view.projects.some((p) => p.id === last)) await select(last);
}

async function select(id) {
  // The picker's overview row carries an empty value, and `""` is not a
  // project id. Normalised here rather than at the call site so every
  // `!selectedId` test in this file keeps meaning one thing.
  // Re-selecting what is already selected (after a generation, say) is a
  // reload, not a switch: the search and the folder being browsed stay.
  const switched = (id || null) !== view.selectedId;
  view.selectedId = id || null;
  // A report belongs to one run of one project. A caller that has one for
  // this selection draws it again after `select` returns (`generateFor`).
  hideRunReport();
  syncMenu();
  titleFor(view.projects.find((x) => x.id === id));
  try {
    localStorage.setItem(lastKey(activeWorkspace), id);
  } catch (e) {
    // A workspace that cannot remember the last selection still works.
    void e;
  }

  // The outgoing page's last-reported panel would otherwise linger on
  // screen for the instant before the new one loads and posts its own —
  // a note about the *previous* project's panel, shown over the next
  // one's map.
  els.contextNote.hidden = true;

  const p = view.projects.find((x) => x.id === view.selectedId);
  await render();
  if (switched) resetFinder();
  refreshPanes(!switched);
  if (!p) {
    // Going back to the overview has to take the previous project's map off
    // the screen with it. Without this the iframe stayed put and the
    // overview rendered underneath a map it no longer describes.
    els.frame.hidden = true;
    els.frame.removeAttribute("src");
    view.mapBase = null;
    setFiles(false);
    renderOverview();
    say("");
    return;
  }

  const status = await mapStatus(invoke, p.map_dir);
  // The same as in `renderDetail`: a slower answer for the project the reader
  // has since left must not put its map (or its placeholder) on screen.
  if (view.selectedId !== id) return;
  if (!status.exists) {
    // What is in this tree, on the one screen whose entire subject is that
    // there is nothing to show yet. Appended rather than replacing the
    // instruction: which button to press is still the more urgent half.
    let langLine = "";
    try {
      const scan = await scanLanguages(invoke, p.root, p.map_dir);
      langLine = summaryText(scan, supportFor(scan, view.engineLangs));
    } catch (e) {
      void e;
    }
    if (view.selectedId !== id) return;

    showPlaceholder(
      t("ph.nomap.title"),
      (view.engine.path ? t("ph.nomap.generate") : t("ph.nomap.noengine")) +
        (langLine ? `<br><span class="detail">${escapeHtml(langLine)}</span>` : "")
    );
    say(p.root);
    renderEngine();
    return;
  }

  // Over HTTP, not convertFileSrc. The page is the same either way; what
  // differs is that a real origin can answer the page's own /api/* fetches,
  // so the Telemetry and Loaded panels show data instead of advising a
  // `:DocMap serve` that would not have helped. If the server cannot start,
  // fall back to the asset protocol rather than showing nothing: a readable
  // map with two panels that report "no host" beats a blank window.
  els.placeholder.hidden = true;
  els.overview.hidden = true;
  els.frame.hidden = false;
  let served = null;
  try {
    served = await invoke("serve_project", { id });
  } catch (e) {
    say(errorText(e));
  }
  if (view.selectedId !== id) return;
  view.mapBase = served ?? convertFileSrc(status.index_path);
  view.mapTab = null;
  const url = mapUrl(view.mapBase);
  els.frame.src = url;
  watchMapLoad(url);
  // The path alone. The module count is already on the project row in the
  // list and again in the map's own header — three copies of one number, and
  // this was the third.
  say(p.root);
  renderEngine();
}

async function refresh(next) {
  view.projects = next ?? (await invoke("list_projects"));
  // The traffic order reads numbers that arrive from the backend, so a fresh
  // list needs them again before it can be ordered.
  if (sortBy === "traffic") await measureTraffic();
  await render();
  syncActions();
}

/// Generate straight after adding — but only when there is nothing to lose.
///
/// A project with no map yet opens on an empty view, which is useless and
/// makes the reader press a button whose outcome was never in doubt. One
/// with a map already is a different case: generation writes into
/// `docs/map` **inside the user's repository**, so regenerating unasked
/// would produce git changes they did not request, on a tree they may have
/// added just to look at. That one shows what is already there and leaves
/// the button to them.
async function autoGenerate(p) {
  if (!view.engine.path) return;
  const status = await mapStatus(invoke, p.map_dir);
  if (status.exists) {
    await select(p.id);
    return;
  }

  // Say what this tree is written in *before* generating it.
  //
  // The engine reads Lua and JS/TS today. Point it at a repository that is
  // two thirds Python and it returns a perfectly valid, nearly empty map --
  // success by every signal this app has, and useless. That news costs
  // nothing here and costs a scan plus a blank view at every later point,
  // which is the whole argument for putting it at this moment specifically.
  try {
    const scan = await scanLanguages(invoke, p.root, p.map_dir);
    const line = summaryText(scan, supportFor(scan, view.engineLangs));
    if (line) say(`${p.name} — ${line}`);
  } catch (e) {
    // Never a reason not to generate: this is context for the result, not a
    // precondition of producing one.
    void e;
  }

  await generateFor(p.id);
}


/// A prompt rather than a real input field: this app has no text-entry UI
/// anywhere yet, and a one-off URL is exactly the case a native prompt
/// exists for — no framework, on purpose, same reasoning as the rest of
/// this file's own header comment.

// A `<select>` answers Arrow, Home, End, Enter and type-ahead itself, in
// whatever way the platform does — which is more than the roving-tabindex
// handler that used to be here did, and none of it has to be kept correct.
//
// `Delete` is the one key it does not answer. It lives on File → Remove
// from workspace, which also names what it removes; a bare Delete key over
// a list of repositories was always the more frightening of the two.
els.list.addEventListener("keydown", (ev) => {
  if (ev.key === "Delete" && els.list.value) {
    ev.preventDefault();
    removeProject(els.list.value);
  }
});

/// Drop a project from the workspace. Never touches the repository — the
/// menu label says so, and this is what it does.
///
/// Extracted from the Delete-key handler when the menu grew an item for
/// it: two callers, one behaviour.
/**
 * Nothing is selected any more: no project, no map, no pane over the overview,
 * no search for a project that is gone.
 *
 * `selectedId = null` on its own left the Files or Statistics pane open (an
 * opaque overlay) over the overview, with the project bar — and so the only
 * tabs that could close it — hidden, and the removed project's rows still
 * clickable.
 */
function dropSelection() {
  view.selectedId = null;
  view.mapBase = null;
  view.mapTab = null;
  // The report is about the project that was just dropped.
  hideRunReport();
  closePanes();
  resetFinder();
  // The View menu's checks and the project items follow the selection and the
  // panes; both just changed.
  syncMenu();
}

async function removeProject(id) {
  try {
    const list = await invoke("remove_project", { id });
    if (view.selectedId === id) {
      dropSelection();
      showPlaceholder(t("ph.none.title"), t("ph.none.body"));
      titleFor(null);
    }
    await refresh(list);
  } catch (e) {
    say(errorText(e));
  }
}

// ------------------------------------------------------------- engine

function shortPath(p) {
  // Enough to recognise, not enough to wrap three lines in a 260px sidebar.
  const parts = String(p).split("/");
  return parts.length <= 3 ? p : ".../" + parts.slice(-2).join("/");
}

/**
 * Which grammar file is missing from which directory.
 *
 * Written into the sidebar panel and into Settings from one place: Settings
 * holds the button that fixes it, the sidebar holds the verdict that raises
 * the question, and two copies of one sentence assembled twice would be two
 * sentences the first time somebody edits one.
 *
 * `renderRich` because the sentence sets a path and a file name as `<code>`,
 * and both interpolated values are escaped first — a directory name is the
 * user's content, not this program's text, the same rule the scope dialog's
 * lead follows.
 */
function renderGrammarDiagnosis() {
  const diag = grammarDiagnosis(view.engineLangs, view.grammarDir);
  const html = diag
    ? fill(
        t(diag.key),
        Object.fromEntries(
          Object.entries(diag.params).map(([k, v]) => [k, escapeHtml(String(v))])
        )
      )
    : "";
  for (const el of [els.engineGrammars, document.getElementById("prefs-grammars-state")]) {
    if (!el) continue;
    renderRich(el, html);
    el.hidden = !html;
  }
}

/**
 * Draw facts as label / value rows.
 *
 * The engine and Neovim panels used to be one string — a path, a tag in
 * brackets, a middle dot, another path, a newline and a language list —
 * which read as a sausage and could not be coloured. Rows can: the label is
 * quiet, a path shows its directory dimmed and its file name strong, a tag
 * is a pill, a language is a chip.
 *
 * Everything is set with `textContent`: a path and a language name are the
 * user's content, never this program's markup.
 *
 * @param {HTMLElement|null} el
 * @param {{label: string, path?: string, text?: string, tag?: {text: string, on?: boolean},
 *          chips?: {text: string, title?: string, dim?: boolean}[],
 *          missing?: boolean, title?: string}[]} rows
 */
function renderFacts(el, rows) {
  if (!el) return;
  el.replaceChildren();
  for (const r of rows) {
    const row = document.createElement("div");
    row.className = "fact" + (r.missing ? " missing" : "");

    const k = document.createElement("span");
    k.className = "fact-k";
    k.textContent = r.label;

    const v = document.createElement("span");
    v.className = "fact-v";
    if (r.title) v.title = r.title;

    if (r.path) {
      // User-chosen paths come back from the dialog with backslashes on
      // Windows; normalised so the directory and file name can be told apart.
      const shown = shortPath(String(r.path).replace(/\\/g, "/"));
      const cut = shown.lastIndexOf("/") + 1;
      const dir = document.createElement("span");
      dir.className = "p-dir";
      dir.textContent = shown.slice(0, cut);
      const base = document.createElement("span");
      base.className = "p-base";
      base.textContent = shown.slice(cut);
      v.append(dir, base);
    }
    if (r.text) {
      const txt = document.createElement("span");
      txt.className = "fact-text";
      txt.textContent = r.text;
      v.append(txt);
    }
    if (r.tag) {
      const tag = document.createElement("span");
      tag.className = "fact-tag" + (r.tag.on ? " on" : "");
      tag.textContent = r.tag.text;
      v.append(tag);
    }
    for (const c of r.chips || []) {
      const chip = document.createElement("span");
      chip.className = "chip" + (c.dim ? " dim" : "");
      chip.textContent = c.text;
      if (c.title) chip.title = c.title;
      v.append(chip);
    }
    row.append(k, v);
    el.append(row);
  }
}

/** The rows of the Engine panel (and Settings' copy of it). */
function engineFacts() {
  if (!view.engine.path) {
    return [{ label: t("facts.binary"), text: t("facts.engine.missing"), missing: true }];
  }
  const rows = [
    {
      label: t("facts.binary"),
      path: view.engine.path,
      tag: {
        text: view.engine.bundled ? t("facts.tag.bundled") : view.engine.from_path ? t("facts.tag.path") : t("facts.tag.chosen"),
        on: view.engine.bundled,
      },
      title: view.engine.path,
    },
    view.engine.grammars
      ? { label: t("facts.grammars"), path: view.engine.grammars, title: view.engine.grammars }
      : { label: t("facts.grammars"), text: t("facts.grammars.none"), missing: true },
  ];
  const known = view.engineLangs?.languages ?? null;
  if (known && known.length) {
    rows.push({
      label: t("facts.reads"),
      // `grammar_loaded === false` is the only degraded state: `null` means
      // the backend needs no parser, which is full fidelity.
      chips: known.map((b) => ({
        text: b.name,
        dim: b.grammar_loaded === false,
        title: b.grammar_loaded === false ? t("facts.chip.nogrammar") : "",
      })),
    });
  } else if (view.engineLangs) {
    // Probed and answered, but with no list: an engine older than the field.
    rows.push({ label: t("facts.reads"), text: engineLanguageText(view.engineLangs), missing: true });
  }
  // The engine held to `engine.lock`: the one row that can say "this binary
  // is not the one this app was checked against" before a map comes back
  // short. Marked like a missing grammar when it is not in step.
  const verdict = contractVerdict(view.engineLangs);
  if (verdict) {
    rows.push({
      label: t("facts.contract"),
      text: fillParams(t(verdict.key), verdict.params),
      missing: !verdict.ok,
      title: verdict.params.commit,
    });
  }
  return rows;
}

function renderEngine() {
  const s = els.engineSummary;
  const rows = engineFacts();
  renderFacts(els.engineState, rows);
  renderFacts(document.getElementById("prefs-engine-state"), rows);
  // Fidelity, not the path: with the engine on PATH the path never changes
  // and is not worth a line, while "will this run produce per-function data"
  // is the one thing that differs run to run.
  //
  // Asked of the engine rather than inferred from the grammars directory --
  // see `engineVerdict` for the failure that inference had: a directory
  // holding one grammar out of four read "ready".
  s.textContent = engineVerdict(view.engine, view.engineLangs);
  s.className = "engine-summary" + (view.engine.path ? "" : " missing");

  // Escalate, never collapse. A missing engine is the one state worth
  // opening the panel for on its own; forcing it *shut* when things are
  // fine would slam it closed under the user the moment `set_grammars`
  // re-renders — right after they opened it to press that button.
  if (!view.engine.path) {
    els.engine.open = true;
  }

  renderGrammarDiagnosis();
  syncActions();
}

/** Both generate buttons depend on the engine; only one also needs a
    selection. Shared so the two can never disagree about whether the view.engine
    is usable. */
function syncActions() {
  els.gen.disabled = !view.engine.path || !view.selectedId;
}

async function loadEngine() {
  view.engine = await invoke("engine_info");
  probeEngineLanguages();
}

// The panels follow the state: whoever sets the engine, what it reads, the grammars directory or Neovim does not
// call the renderers after it.
viewStore.subscribe("engine", renderEngine);
viewStore.subscribe("engineLangs", () => {
  renderEngine();
  void render();
});
viewStore.subscribe("grammarDir", renderGrammarDiagnosis);
viewStore.subscribe("nvim", renderNvim);

/// Ask the engine which languages it reads, and repaint when it answers.
///
/// Not awaited by its callers, and a failure is swallowed: the language list
/// is context, never a precondition. A machine with no engine at all still
/// renders the panel, which then says the more important thing -- that there
/// is no engine.
///
/// Re-run after `set_engine` and `set_grammars` because both change the
/// answer: a different binary has different backends, and a grammars
/// directory is precisely what turns `grammar_loaded` from false to true.
/// This is also why the engine side refuses to cache its own probe.
function probeEngineLanguages() {
  invoke("engine_languages")
    .then((res) => {
      view.engineLangs = res;
    })
    .catch(() => {
      view.engineLangs = null;
    });

  // Asked separately and swallowed separately. A directory that cannot be
  // read is a reason to say less, never a reason for the panel above to
  // fail: `grammarDiagnosis` treats a missing answer as "no directory",
  // which is the weaker sentence and the safe one.
  invoke("grammar_dir")
    .then((res) => {
      view.grammarDir = res;
    })
    .catch(() => {
      view.grammarDir = null;
    });
}

els.pickEngine.addEventListener("click", async () => {
  try {
    const file = await open({ multiple: false, title: "Locate the docmap engine" });
    if (!file) return;
    view.engine = await invoke("set_engine", { path: file });
    probeEngineLanguages();
    say("Engine set");
  } catch (e) {
    say(errorText(e));
  }
});

els.pickGrammars.addEventListener("click", async () => {
  try {
    const dir = await open({ directory: true, multiple: false, title: "Tree-sitter grammars" });
    if (!dir) return;
    view.engine = await invoke("set_grammars", { path: dir });
    probeEngineLanguages();
    say("Grammars set");
  } catch (e) {
    say(errorText(e));
  }
});

// -------------------------------------------------------------- neovim

function nvimFacts() {
  if (!view.nvim.path) {
    return [{ label: t("facts.binary"), text: t("facts.nvim.missing"), missing: true }];
  }
  const rows = [
    {
      label: t("facts.binary"),
      path: view.nvim.path,
      tag: view.nvim.from_path ? { text: t("facts.tag.path") } : { text: t("facts.tag.chosen") },
      title: view.nvim.path,
    },
  ];
  rows.push(
    view.nvim.config_dir
      ? {
          label: t("facts.config"),
          path: view.nvim.config_dir,
          tag: view.nvim.config_dir_from_default ? { text: t("facts.tag.default") } : { text: t("facts.tag.chosen") },
          title: view.nvim.config_dir,
        }
      : { label: t("facts.config"), text: t("facts.nvim.noConfig"), missing: true }
  );
  // Where the telemetry is read from: worked out from the environment, the
  // way Neovim does, unless a folder was chosen. No Neovim is started for it.
  if (view.nvim.cache_dir) {
    rows.push({
      label: t("facts.cache"),
      path: view.nvim.cache_dir,
      tag: view.nvim.cache_dir_from_default ? { text: t("facts.tag.default") } : { text: t("facts.tag.chosen") },
      title: view.nvim.cache_dir,
    });
  }
  return rows;
}

function renderNvim() {
  const s = els.nvimSummary;
  const ok = !!(view.nvim.path && view.nvim.config_dir);
  const rows = nvimFacts();
  renderFacts(els.nvimState, rows);
  renderFacts(document.getElementById("prefs-nvim-state"), rows);
  s.textContent = ok ? "ready" : "not found";
  s.className = "engine-summary" + (ok ? "" : " missing");

  // Same escalate-never-collapse rule as the engine panel.
  if (!ok) {
    els.nvim.open = true;
  }
}

async function loadNvim() {
  view.nvim = await invoke("nvim_info");
}

els.pickNvim.addEventListener("click", async () => {
  try {
    const file = await open({ multiple: false, title: "Locate the nvim binary" });
    if (!file) return;
    view.nvim = await invoke("set_nvim_path", { path: file });
    say("nvim set");
  } catch (e) {
    say(errorText(e));
  }
});

els.pickNvimConfig.addEventListener("click", async () => {
  try {
    const dir = await open({ directory: true, multiple: false, title: "Locate the Neovim config directory" });
    if (!dir) return;
    view.nvim = await invoke("set_nvim_config_dir", { path: dir });
    say("Neovim config directory set");
  } catch (e) {
    say(errorText(e));
  }
});

els.pickNvimCache.addEventListener("click", async () => {
  try {
    const dir = await open({ directory: true, multiple: false, title: "Locate Neovim's cache folder" });
    if (!dir) return;
    view.nvim = await invoke("set_nvim_cache_dir", { path: dir });
    say("Neovim cache folder set");
  } catch (e) {
    say(errorText(e));
  }
});

/// Import every enabled, locally-checked-out personal plugin from the
/// user's Neovim config as a project. A real `nvim --headless` run, same
/// order of latency as one `generate` call, so it gets the same
/// placeholder-while-running treatment rather than a silent freeze.

// ---------------------------------------------------------- generation

async function generateFor(id, full = false) {
  const p = view.projects.find((x) => x.id === id);
  if (!p) return;

  // Who the reader was looking at when this started. A generation takes
  // minutes and nothing stops them choosing another project meanwhile; its
  // result belongs on screen if they are still where they began, or if they
  // have gone to the project that was generated — that is the one the result
  // is about. (Adding a project generates for one that is not selected yet,
  // which is why `started` is compared as well as `id`.)
  const started = view.selectedId;
  const here = () => view.selectedId === started || view.selectedId === id;

  // Replace the view while it runs: leaving the previous project's map on
  // screen during a rebuild is the same "wrong panel's data" problem the
  // generated page itself had to fix in its own fetch-backed panels.
  showTaskPlaceholder(
    t("ph.generating.title"),
    t("ph.generating.body").replace("{root}", escapeHtml(p.root))
  );
  say("Generating " + p.name);

  // `restoreDisabled: false`: whether this button ends up enabled depends on
  // engine and selection state, not on the run having finished, so the
  // trailing `renderEngine()` keeps owning that.
  try {
    await withBusyButton(
      els.gen,
      "Generating…",
      async () => {
        const res = await invoke("generate", { root: p.root, full });
        // The engine reports on stdout even when it succeeds, and its report
        // is the useful part — counts, coverage, findings. Show it either way.
        const log = [res.stdout, res.stderr].filter(Boolean).join("\n").trim();
        if (res.ok) {
          invalidate(p.map_dir);
          invalidateLanguages(p.root);
          // The cached "stale" verdict is about the map that was just
          // replaced. `select()` re-measures it for whoever is looking at this
          // project; for a reader who went elsewhere, nothing else would.
          freshness.delete(p.id);
          await refresh();
          if (here()) {
            await select(id);
            // Not `appendLog`: `select` has just put the map on screen and
            // hidden the placeholder, which is the only host `appendLog`
            // has - the report of a run that worked was dropped there, and
            // with it every finding the status line counts.
            if (log && view.selectedId === id) showRunReport(log);
          }
          sayLive(() => generatedMessageFor(p.name, log));
        } else {
          if (here()) {
            showTaskPlaceholder(
              t("ph.failed.title"),
              t("ph.failed.body").replace("{code}", String(res.code))
            );
            appendLog(log || "(no output)", true);
            // The one failure this app can explain better than the engine
            // can. `--full` is offered unconditionally, so the missing tool
            // is a normal outcome rather than a broken install, and saying
            // so in the placeholder beats leaving it to be found in the log.
            if (full && /lua-language-server/.test(log)) {
              showTaskPlaceholder(t("ph.failed.title"), t("gen.full.needsLuals"));
            }
          }
          say("Failed: " + p.name);
        }
      },
      { restoreDisabled: false }
    );
  } catch (e) {
    if (here()) showTaskPlaceholder(t("ph.enginefail.title"), escapeHtml(errorText(e)));
    say(errorText(e));
  } finally {
    renderEngine();
  }
}

/**
 * Put the engine's own words under whatever is on screen.
 *
 * The report lines (what was written, the counts, a crash's traceback) are
 * shown as they are; the findings - one line each, a hundred of them for a
 * big vault - are grouped per check with a few examples, and the rest is one
 * button away. See `lib/engine-log.js`.
 *
 * This is the report of a run that has *no map to show*: a failed generation
 * and "Check exactly", where the message is the view and the report is
 * under it. A run that worked has a map on screen instead and its report
 * goes to `showRunReport`.
 */
function appendLog(text, bad) {
  taskLog.append(text, bad);
}

/** The words of the report's controls, read at draw time (the language can change). */
const logLabels = {
  more: (n) => t("log.more").replace("{n}", String(n)),
  showAll: (n) => t("log.showAll").replace("{n}", String(n)),
  get fold() {
    return t("log.fold");
  },
  get title() {
    return t("log.title");
  },
};

/** The report under a placeholder message; also the placeholder's tab stop while it holds one. */
const taskLog = createTaskLog({ doc: document, host: els.placeholder, perCheck: PER_CHECK, labels: logLabels });

/**
 * The report of a run that worked, docked at the foot of the map.
 *
 * Over the map rather than instead of it: the map is what the reader asked
 * for, and the report is the answer to "what did the engine find while making
 * it". It scrolls on its own (the shell never does) and closes; it also goes
 * when another project or message takes the view.
 */
const runReport = createRunReport({
  doc: document,
  panel: els.runReport,
  body: els.runReportBody,
  close: els.runReportClose,
  perCheck: PER_CHECK,
  labels: logLabels,
  // Closing the report drops the focus on <body> otherwise. Generate is what
  // opened it; if that is busy or disabled, the project picker is always there.
  returnFocus: () => [els.gen, els.list],
});

function showRunReport(text) {
  runReport.show(text);
}

function hideRunReport() {
  runReport.hide();
}

/** "Generated X", plus what the engine found when it found anything. */
function generatedMessageFor(name, log) {
  return generatedMessage(name, summarizeEngineLog(log).tally, { t, plural });
}

els.gen.addEventListener("click", () => view.selectedId && generateFor(view.selectedId));

/// Regenerate every project, one after another.
///
/// **Sequential on purpose.** Each run is a separate engine process doing
/// CPU-bound parsing; starting a dozen at once would finish no sooner and
/// would make the machine unusable while it happened. The counter in the
/// button is there because the honest alternative to a progress indication
/// on a minutes-long job is a window that looks hung.
///
/// Unlike `autoGenerate` this *does* overwrite existing maps, and that is
/// the point rather than an oversight: pressing a button named "Generate
/// all" is the explicit request that the automatic path deliberately is
/// not.
/**
 * Generate every project in the workspace.
 *
 * No longer a sidebar button. `MENUBAR.md`'s rule is that the sidebar
 * keeps exactly one command rather than a mirror of the menu, and this is
 * the rare one — it writes into repositories you did not select, which is
 * also why its wording never gets shortened.
 *
 * The progress used to be the button's own label. It is the status bar's
 * now, which is a better home for it anyway: a bar that reports on the
 * whole window is where a job spanning every project belongs, and it does
 * not disappear when the sidebar is collapsed.
 */
async function generateAll(only) {
  if (!view.engine.path || view.projects.length === 0) return;

  const list = only ? view.projects.filter((p) => only.has(p.id)) : view.projects.slice();
  if (list.length === 0) return;
  const failed = [];
  let ok = 0;

  els.gen.disabled = true;
  showTaskPlaceholder(
    t("ph.genall.title"),
    t("ph.genall.body").replace("{n}", String(list.length))
  );

  for (let i = 0; i < list.length; i++) {
    const p = list[i];
    say(
      t("gen.all.progress")
        .replace("{n}", String(i + 1))
        .replace("{total}", String(list.length))
        .replace("{name}", p.name)
    );
    try {
      const res = await invoke("generate", { root: p.root, full: false });
      if (res.ok) {
        invalidate(p.map_dir);
        invalidateLanguages(p.root);
        // The map just moved; the cached verdict about it is now a claim
        // about a file that no longer exists in that state.
        freshness.delete(p.id);
        ok++;
      } else {
        failed.push(p.name);
      }
    } catch (e) {
      // One unreachable project must not abandon the rest — the report at
      // the end names which ones failed.
      void e;
      failed.push(p.name);
    }
  }

  await refresh();
  if (view.selectedId) await select(view.selectedId);
  renderEngine();
  say(
    failed.length
      ? ok + " generated, " + failed.length + " failed: " + failed.join(", ")
      : ok + " project(s) generated"
  );
}

/**
 * Generate only the projects whose maps have fallen behind.
 *
 * The staleness mark has been on screen since §6 with no command that acts
 * on it, which is the gap this closes: the answer to "four of these are out
 * of date" should not be four selections and four presses.
 *
 * The verdict is asked for fresh rather than read out of the cache. The
 * cache only holds projects that have been *looked at*, so acting on it
 * would quietly skip everything the reader has not opened — which is most
 * of what this command exists for.
 */
/**
 * Ask the engine whether the map would come out different, and write nothing.
 *
 * **The staleness mark and this answer different questions**, and the wording
 * below is the whole point of having both. The mark compares modification
 * times: it says "something was touched since this was written", and a file
 * saved without an edit in it counts. This runs the analysis and compares the
 * output byte for byte, which is the only thing that settles it — and it is
 * why the mark's own tooltip has always pointed at a command that did not
 * exist yet.
 *
 * The engine's report is shown the way generation's is: its own lines as they
 * are, its findings grouped per check and capped (`lib/engine-log.js`). That
 * only regroups what the engine printed - it counts and sorts, and says
 * nothing the engine did not - so this window still has no opinion of its own
 * about a tree it did not read.
 */
async function checkMap(id) {
  const p = view.projects.find((x) => x.id === id);
  if (!p) return;

  // See `generateFor`: the report is for whoever started the check.
  const started = view.selectedId;
  const here = () => view.selectedId === started;

  showTaskPlaceholder(
    t("ph.check.title"),
    t("ph.check.body").replace("{root}", escapeHtml(p.root))
  );
  say(t("check.running"));

  let res;
  try {
    res = await invoke("check_map", { root: p.root });
  } catch (e) {
    say(fill(t("check.failed"), { error: errorText(e) }));
    if (here()) appendLog(errorText(e), true);
    return;
  }

  const log = [res.stdout, res.stderr].filter(Boolean).join("\n").trim();
  // `--lenient` makes the exit code mean staleness and nothing else, so this
  // reads one bit rather than sniffing the engine's prose for a sentence that
  // is free to be reworded — see `check_map` in `main.rs`.
  // Exit code 2 is the engine saying it did not get as far as a verdict (it
  // refused to read a map through a link or an unsafe map directory, or could
  // not run): that is not "stale", and regenerating would not settle it.
  const stale = res.code !== 2;
  sayLive(() => (res.current ? t("check.current") : t(stale ? "check.stale" : "check.refused")));
  if (here()) appendLog(log || "(no output)", !res.current);
}

async function generateStale() {
  if (!view.engine.path || view.projects.length === 0) return;
  say(t("gen.stale.checking"));

  const stale = new Set();
  for (const p of view.projects) {
    try {
      const f = await invoke("map_freshness", { id: p.id });
      freshness.set(p.id, f);
      if (f.stale) stale.add(p.id);
    } catch (e) {
      // A project that cannot be measured is not evidence that it is
      // stale. Left alone, and named at the end by not being counted.
      void e;
    }
  }

  if (stale.size === 0) {
    say(t("gen.stale.none"));
    return;
  }
  await generateAll(stale);
}

// -------------------------------------------------------- context note

/// What the loaded page reports (docmap.nvim, `core/render/html.lua`'s
/// embedded `postContext()`) when the reader switches panels: `{ source: "docmap",
/// tab, atool, view }`. Two of those combinations ask for something this
/// app's engine cannot do at all, regardless of configuration -- not
/// "not set up yet" the way a missing engine path is:
///
///   * Telemetry needs a live Neovim session actually running the code
///     under `runtime-analysis.nvim`; nothing this app runs can make
///     that data exist.
///   * Hierarchy -> Types needs `lua-language-server` via `vim.system`,
///     which the standalone engine has no equivalent of (measured:
///     `standalone/vim_shim.lua` has no `vim.fn.executable`, which
///     `docmap.nvim`'s own `core/luals.lua` calls first).
///
/// So this is a note explaining why, not a button that would fail --
/// the same "say why, don't silently fail" rule `no grammars` and
/// "older engine without --api" already follow elsewhere in this
/// sidebar. `null` for every other panel: most of them need no
/// explanation at all.
function contextNoteFor(ctx) {
  if (ctx.tab === "analysis" && ctx.atool === "telemetry") {
    // **Requalified once start/stop landed.** This used to end "nothing here
    // can generate it", which stopped being true: collection can be switched
    // on from Settings. The half that stays true is the one that matters —
    // this app cannot run somebody's plugin code, so it shows what was
    // collected elsewhere. A note that overstates what the window cannot do
    // is the same failure as one that overstates what it can.
    return t("note.telemetry");
  }
  if (ctx.tab === "hierarchy" && ctx.view === "types") {
    return t("note.types");
  }
  // **An empty panel that says why.** The Calls and Module Calls views draw
  // nothing for nineteen of the engine's twenty-three backends, and they
  // draw nothing in exactly the same way for a project that genuinely has
  // no calls in it. The reader cannot tell those apart, and the second is
  // the one worth explaining.
  //
  // Only on `"no"`. `"unknown"` is a real third answer — no scan yet, or an
  // engine too old to have the field — and a guess would be wrong for
  // precisely the reader who most needs it to be right.
  if (
    ctx.tab === "hierarchy" &&
    (ctx.view === "calls" || ctx.view === "modulecalls") &&
    view.callsSupport === "no"
  ) {
    return t("note.calls");
  }
  return null;
}

window.addEventListener("message", (ev) => {
  const data = ev.data;
  if (!data || data.source !== "docmap") return;
  // Only the map frame speaks for the map. A message from any other window
  // (this one is reachable by every window with a reference to it) is not the
  // page reporting, and certainly not a reader asking to open a file.
  if (ev.source !== els.frame.contentWindow) return;

  // The page asking for something, rather than reporting where it is. Its
  // one request today is opening a file in an editor; the path is
  // repo-relative because that is what the artifact stores, and it is
  // resolved and bounds-checked in Rust rather than trusted here.
  if (data.kind === "open-file" && data.path && view.selectedId) {
    // Something the reader just did in the map (a right-click menu item),
    // not a page that loaded and asked on its own: a repository's own page
    // runs the moment it is selected, and an open-file request with no click
    // behind it is nothing the reader asked for.
    const activation = navigator.userActivation;
    if (activation && !activation.isActive) return;
    invoke("open_in_editor", {
      id: view.selectedId,
      path: data.path,
      line: data.line ?? null,
    }).catch((e) => say(errorText(e)));
    return;
  }

  // The page ran. Whatever else this message says, it is the evidence the
  // blank-pane watchdog is waiting for.
  view.mapLoaded = true;
  if (view.mapWatch) {
    clearTimeout(view.mapWatch);
    view.mapWatch = null;
  }
  if (data.tab) view.mapTab = data.tab;
  const note = contextNoteFor(data);
  renderRich(els.contextNote, note || "");
  els.contextNote.hidden = !note;
});

(async function start() {
  try {
    await loadEngine();
    await loadNvim();
    await refresh();
    // The dashboard decides for itself whether it is worth showing: one
    // workspace is not a choice. When it does show, nothing is restored
    // here — the selection belongs to whichever workspace is picked, and
    // `switchWorkspace` restores that one's own.
    const chose = await maybeOpenWorkspaces();
    if (chose) return;
    await restoreLast();
  } catch (e) {
    say(errorText(e));
  }
})();

// =====================================================================
// Add a project — one door, three ways
//
// Folder, Neovim config and URL were three sidebar buttons. They are three
// answers to one question ("where is the project"), and a sidebar that lists
// them separately makes the reader compare three labels before doing the
// thing they came to do. Tabs rather than a wizard: alternatives, not steps.
//
// Every action below is the same command the old buttons called. Nothing
// about adding, importing or cloning changed — only how they are reached.
// =====================================================================
const addbox = {
  el: document.getElementById("addbox"),
  tabs: [...document.querySelectorAll(".addbox-tab")],
  panes: [...document.querySelectorAll(".addbox-pane")],
  url: document.getElementById("url-input"),
  loadRepos: document.getElementById("load-repos"),
  repoFilter: document.getElementById("repo-filter"),
  repoState: document.getElementById("repo-state"),
  repoList: document.getElementById("repo-list"),
};

/** Repositories as last listed, so filtering never re-runs the network call. */
let repos = null;

function showPane(id) {
  addbox.tabs.forEach((t) => t.classList.toggle("on", t.dataset.pane === id));
  addbox.panes.forEach((p) => p.classList.toggle("on", p.id === id));
}

addbox.tabs.forEach((tab) => {
  tab.addEventListener("click", () => showPane(tab.dataset.pane));
});

els.add.addEventListener("click", () => {
  showPane("pane-folder");
  resetFolderScan();
  addbox.el.showModal();
});

/** Close, then run `fn` — the dialog must not sit over the result. */
async function closeThen(fn) {
  addbox.el.close();
  try {
    await fn();
  } catch (e) {
    // The same surface every other failure in this window uses. A dialog
    // action that fails silently is worse here than elsewhere: the reader
    // just asked for something specific.
    say(errorText(e));
  }
}

// ---------------------------------------------------------------------
// Bulk import from a parent folder
//
// `pick-folder` used to always add whatever was chosen. Now it asks first:
// `inspect_folder` says whether the chosen directory is a repository itself
// (unchanged behaviour, added straight away) or a container of several
// — a `$REPOS_DIR` holding three dozen plugins, say — in which case a
// checklist replaces the single button, the same pattern the GitHub
// pick-list already established for "list, then choose".
// ---------------------------------------------------------------------

const folderScan = {
  wrap: document.getElementById("folder-scan"),
  list: document.getElementById("folder-scan-list"),
  go: document.getElementById("folder-scan-go"),
  all: document.getElementById("folder-scan-all"),
  none: document.getElementById("folder-scan-none"),
  note: document.getElementById("folder-note"),
};

/** Subdirectories offered by the last `inspect_folder` call. */
let scanCandidates = [];

function resetFolderScan() {
  scanCandidates = [];
  folderScan.wrap.hidden = true;
  folderScan.list.replaceChildren();
  folderScan.note.textContent = "";
}

function updateFolderScanGo() {
  const n = scanCandidates.filter((c) => c.checked && !c.alreadyAdded).length;
  folderScan.go.disabled = n === 0;
  folderScan.go.textContent = t("add.folder.scan.go").replace("{n}", String(n));
}

function renderFolderScan() {
  folderScan.note.textContent = "";
  folderScan.list.replaceChildren();
  scanCandidates.forEach((c, i) => {
    const li = document.createElement("li");
    li.className = "repo";

    const label = document.createElement("label");
    label.className = "addbox-check";

    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = c.checked;
    box.disabled = c.alreadyAdded;
    box.addEventListener("change", () => {
      scanCandidates[i].checked = box.checked;
      updateFolderScanGo();
    });

    const name = document.createElement("span");
    name.className = "repo-name";
    name.textContent = c.name;

    label.append(box, name);

    const meta = document.createElement("span");
    meta.className = "repo-meta";
    meta.textContent = c.alreadyAdded
      ? t("add.folder.scan.already")
      : c.isGit
        ? ""
        : t("add.folder.scan.notGit");

    li.append(label, meta);
    folderScan.list.append(li);
  });
  folderScan.wrap.hidden = false;
  updateFolderScanGo();
}

folderScan.all.addEventListener("click", () => {
  scanCandidates.forEach((c) => (c.checked = !c.alreadyAdded));
  renderFolderScan();
});
folderScan.none.addEventListener("click", () => {
  scanCandidates.forEach((c) => (c.checked = false));
  renderFolderScan();
});

folderScan.go.addEventListener("click", () =>
  closeThen(async () => {
    const roots = scanCandidates.filter((c) => c.checked && !c.alreadyAdded).map((c) => c.path);
    resetFolderScan();
    if (roots.length === 0) return;
    const res = await invoke("import_many", { roots });
    await refresh();
    say(
      t("add.folder.scan.done")
        .replace("{added}", String(res.added.length))
        .replace("{found}", String(res.found))
    );
  })
);

document.getElementById("pick-folder").addEventListener("click", async () => {
  const dir = await open({ directory: true, multiple: false, title: "Add project" });
  if (!dir) return;

  resetFolderScan();
  let scan;
  try {
    scan = await invoke("inspect_folder", { root: dir });
  } catch (e) {
    folderScan.note.textContent = errorText(e);
    return;
  }

  if (scan.isGit) {
    await closeThen(async () => {
      const before = new Set(view.projects.map((x) => x.id));
      await refresh(await invoke("add_project", { root: dir }));
      say(`Added ${dir}`);
      const added = view.projects.find((x) => !before.has(x.id));
      if (added) await autoGenerate(added);
    });
    return;
  }

  if (scan.subrepos.length === 0) {
    folderScan.note.textContent = t("add.folder.scan.empty");
    return;
  }

  scanCandidates = scan.subrepos.map((r) => ({ ...r, checked: r.isGit && !r.alreadyAdded }));
  renderFolderScan();
});

// The import keeps the dialog open until the project list actually grew: the
// result (or the reason there is none) is written into the tab itself, where
// the reader is looking, and to the status line. Closing first and reporting
// only in the status line is what made this tab look like it did nothing.
const nvimImportBtn = document.getElementById("run-nvim-import");
const nvimImportResult = document.getElementById("nvim-import-result");
nvimImportBtn.addEventListener("click", async () => {
  const show = (text) => {
    nvimImportResult.textContent = text;
    nvimImportResult.hidden = false;
    say(text);
  };
  nvimImportResult.hidden = true;
  try {
    const res = await withBusyButton(nvimImportBtn, t("add.nvim.reading"), () =>
      invoke("import_from_nvim_config")
    );
    const summary = nvimImportSummary(res);
    show(summary.text);
    if (summary.changed) {
      // Same as the folder checklist's bulk add: no automatic generate for
      // a batch, the projects are in the list and each one generates on its
      // own button (or "Generate all"). The line in the tab goes with the
      // dialog, or the next opening would greet the reader with this result.
      nvimImportResult.hidden = true;
      addbox.el.close();
      await refresh();
      say(summary.text);
    }
  } catch (e) {
    show(errorText(e));
  }
});

const urlGo = document.getElementById("run-url-import");
const urlStop = document.getElementById("stop-url-import");
/** A clone runs: Stop is shown, and closing the dialog stops it. */
let cloning = false;

/**
 * Clone whatever the URL field holds. Shared by the button and the list.
 *
 * The dialog stays open while git runs, with a Stop next to the busy button:
 * a clone of a large repository takes minutes, and the person must be able to
 * end it. Stop, or closing the dialog, ends git with everything it started
 * (`cancel_import`); the half-cloned folder is removed on that side.
 */
async function cloneUrl(url) {
  if (!url || cloning) return;
  cloning = true;
  urlStop.hidden = false;
  say(`Cloning ${url}…`);
  try {
    const projects = await withBusyButton(urlGo, t("add.url.cloning"), () =>
      invoke("import_from_url", { url })
    );
    cloning = false;
    addbox.el.close();
    await refresh(projects);
    say(`Added ${url}`);
  } catch (e) {
    say(errorText(e));
  } finally {
    cloning = false;
    urlStop.hidden = true;
  }
}

function stopClone() {
  if (cloning) invoke("cancel_import").catch(() => {});
}

urlGo.addEventListener("click", () => cloneUrl(addbox.url.value.trim()));
urlStop.addEventListener("click", stopClone);
addbox.el.addEventListener("close", stopClone);
onEnterSubmit(addbox.url, () => cloneUrl(addbox.url.value.trim()));

// ---------------------------------------------------------------------
// The GitHub pick-list
//
// Opt-in, never on open: listing repositories is a network call against
// someone's account. Delegated to `gh`, so this program never holds a
// credential — see `src-tauri/src/github.rs`.
// ---------------------------------------------------------------------

/** What to say for each way the listing can fail to produce a list. */
const REPO_PROBLEMS = {
  not_installed: "repos.notInstalled",
  not_authenticated: "repos.notAuthenticated",
  failed: "repos.failed",
  unreadable: "repos.unreadable",
};

function renderRepos() {
  const q = (addbox.repoFilter.value || "").toLowerCase().trim();
  const shown = (repos || []).filter(
    (r) =>
      !q ||
      r.name_with_owner.toLowerCase().includes(q) ||
      (r.description || "").toLowerCase().includes(q)
  );

  addbox.repoList.replaceChildren();
  shown.forEach((r) => {
    const li = document.createElement("li");
    li.className = "repo";

    const name = document.createElement("span");
    name.className = "repo-name";
    name.textContent = r.name_with_owner;

    const meta = document.createElement("span");
    meta.className = "repo-meta";
    // GitHub's own primary-language guess, labelled as theirs rather than
    // presented as this app's answer — `scan_languages` counts a local tree
    // itself, and conflating the two would make one look as measured as the
    // other.
    meta.textContent = [r.is_private ? "private" : null, r.language]
      .filter(Boolean)
      .join(" · ");

    li.append(name, meta);
    if (r.description) {
      const d = document.createElement("span");
      d.className = "repo-desc";
      d.textContent = r.description;
      li.append(d);
    }

    li.addEventListener("click", () => cloneUrl(r.url));
    addbox.repoList.append(li);
  });

  addbox.repoList.hidden = shown.length === 0;
  if (repos && shown.length === 0) {
    addbox.repoState.textContent = q
      ? `No repository matches “${q}”.`
      : t("repos.none");
  } else if (repos) {
    addbox.repoState.textContent = `${shown.length} of ${repos.length} repositories — click one to clone it.`;
  }
}

addbox.loadRepos.addEventListener("click", async () => {
  addbox.repoState.textContent = t("repos.asking");
  addbox.loadRepos.disabled = true;
  try {
    const res = await invoke("list_github_repos");
    if (res.problem) {
      repos = null;
      addbox.repoList.hidden = true;
      addbox.repoFilter.hidden = true;
      // `gh`'s own message, appended rather than replaced: the sentence
      // above says what to do, and gh's says what happened.
      // gh's own message is appended verbatim and never translated: it is
      // someone else's output, and the sentence before it is the part this
      // program is responsible for.
      addbox.repoState.textContent =
        t(REPO_PROBLEMS[res.problem] || REPO_PROBLEMS.failed) +
        (res.message ? " — " + res.message : "");
      return;
    }
    repos = res.repos;
    addbox.repoFilter.hidden = false;
    renderRepos();
  } catch (e) {
    addbox.repoState.textContent = errorText(e);
  } finally {
    addbox.loadRepos.disabled = false;
  }
});

addbox.repoFilter.addEventListener("input", renderRepos);


// =====================================================================
// View state the menu has to show, not just act on
//
// Theme, language, zoom and sidebar visibility are all properties of *this
// machine* rather than of the project list — `localStorage`, not
// `workspace.json`, for the same reason the theme has always been there: a
// window's lighting and text size do not travel with a repository.
//
// The menu shows them as checkmarks, which means every one of them has to
// tell the menu when it changes. Changing a setting from the sidebar and
// leaving the menu claiming the old value is worse than having no checkmark
// at all.
// =====================================================================

const ZOOM_KEY = "docmap.zoom";
const SIDEBAR_KEY = "docmap.sidebar";
const ZOOM_STEPS = [0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];

let zoom = 1;
let sidebarShown = true;

/** Nearest step to the current factor, so repeated presses always move. */
function zoomStep(direction) {
  const i = ZOOM_STEPS.reduce(
    (best, v, n) => (Math.abs(v - zoom) < Math.abs(ZOOM_STEPS[best] - zoom) ? n : best),
    0
  );
  const next = Math.min(Math.max(i + direction, 0), ZOOM_STEPS.length - 1);
  return ZOOM_STEPS[next];
}

async function applyZoom(factor) {
  try {
    // Rust clamps and answers with what it actually set, so the value kept
    // here is the window's, not the request's — otherwise pressing zoom-out
    // at the floor would keep decrementing a number nothing honours, and the
    // first four presses back up would do nothing visible.
    zoom = await invoke("set_zoom", { factor });
    localStorage.setItem(ZOOM_KEY, String(zoom));
  } catch (e) {
    say(errorText(e));
  }
}

function applySidebar(shown) {
  sidebarShown = shown;
  els.sidebar.hidden = !shown;
  // The grid needs to know too: a hidden sidebar still owned its column.
  document.body.classList.toggle("sb-hidden", !shown);
  try {
    localStorage.setItem(SIDEBAR_KEY, shown ? "1" : "0");
  } catch (e) {
    void e;
  }
}

// ---------------------------------------------------------------------
// Pinned or auto-hiding sidebar
//
// Pinned is how the window has always looked. Unpinned, the sidebar folds
// to a thin edge and opens as an overlay while the pointer is on it, so the
// map gets the whole width and the controls are one hover away.
//
// Opening is by hover *and* by focus: a keyboard user tabbing into the
// folded sidebar must see where they are. Closing waits a moment, and never
// happens while a `<select>` in the sidebar has focus -- its dropdown list
// is drawn by the platform outside this page, and moving onto it looks like
// leaving the sidebar.
// ---------------------------------------------------------------------
const PIN_KEY = "docmap.sidebarPinned";
let sidebarPinned = true;
let sidebarHovered = false;
let peekTimer = null;

/** The pin's words, in the current language — also run when it changes. */
function syncPinLabels() {
  els.sbPin.setAttribute("aria-pressed", String(sidebarPinned));
  els.sbPin.dataset.help = t(sidebarPinned ? "help.pin.on" : "help.pin.off");
  // One label for what the button *is*, with `aria-pressed` for its state; a
  // label that named the action as well contradicted the pressed state.
  els.sbPin.setAttribute("aria-label", t("pin.label"));
}

function applyPinned(pinned) {
  sidebarPinned = pinned;
  document.body.classList.toggle("sb-auto", !pinned);
  syncPinLabels();
  if (pinned) els.sidebar.classList.remove("peek");
  try {
    localStorage.setItem(PIN_KEY, pinned ? "1" : "0");
  } catch (e) {
    void e;
  }
}

function peek(on) {
  if (peekTimer) {
    clearTimeout(peekTimer);
    peekTimer = null;
  }
  if (sidebarPinned) return;
  if (on) {
    els.sidebar.classList.add("peek");
    return;
  }
  peekTimer = setTimeout(() => {
    peekTimer = null;
    const a = document.activeElement;
    const holding = a && a.tagName === "SELECT" && els.sidebar.contains(a);
    if (!sidebarHovered && !holding) els.sidebar.classList.remove("peek");
  }, 350);
}

els.sidebar.addEventListener("mouseenter", () => {
  sidebarHovered = true;
  peek(true);
});
els.sidebar.addEventListener("mouseleave", () => {
  sidebarHovered = false;
  peek(false);
});
// Keyboard focus opens the sidebar. The focus a closing dialog hands back to
// the button that opened it, and a mouse click, do not: the sidebar followed
// them open and stayed open with the pointer elsewhere.
els.sidebar.addEventListener("focusin", (ev) => {
  const keyboard = ev.target.matches && ev.target.matches(":focus-visible");
  peek(!!keyboard);
});
els.sidebar.addEventListener("focusout", () => peek(false));
// Whether the last thing done in the sidebar was a pointer or a key.
let viaPointer = false;
els.sidebar.addEventListener("pointerdown", () => {
  viaPointer = true;
});
els.sidebar.addEventListener("keydown", () => {
  viaPointer = false;
});
// Picking from a dropdown with the pointer is the end of the interaction: let
// go of focus so the sidebar can fold again. Not for the keyboard — a closed
// `<select>` fires `change` on every arrow key, and dropping focus after the
// first one ended the walk through the list.
els.sidebar.addEventListener("change", (ev) => {
  if (!sidebarPinned && viaPointer && ev.target && ev.target.tagName === "SELECT") {
    ev.target.blur();
    peek(false);
  }
});
els.sbPin.addEventListener("click", () => {
  applyPinned(!sidebarPinned);
  syncMenu();
});

{
  let saved = "1";
  try {
    saved = localStorage.getItem(PIN_KEY) ?? "1";
  } catch (e) {
    void e;
  }
  applyPinned(saved !== "0");
}

/** `base` with the theme, and the panel to land on, appended. */
function mapUrl(base) {
  const theme = currentTheme();
  if (web) return web.mapUrl(base, theme, view.mapTab);
  let url = base;
  if (theme === "light" || theme === "dark") {
    url += (url.includes("?") ? "&" : "?") + "theme=" + theme;
  }
  // `tab` only. The page deliberately posts a coarse context and not its
  // whole state, so this restores the panel and nothing finer -- which is
  // the part a reader would notice losing.
  if (view.mapTab) url += "#tab=" + encodeURIComponent(view.mapTab);
  return url;
}

/**
 * Watch the frame until the page proves it loaded, and say so if it never
 * does.
 *
 * Reported from the installed build: after a regenerate the main pane went
 * white. It could not be reproduced here — the harness reloads correctly,
 * the local server sends `Cache-Control: no-store` so a stale copy is not
 * it, and the frame ends up visible with a src set. So rather than guess at
 * a cause, this removes the *silence*: a pane that stays blank now says it
 * stayed blank, and names the URL it was asked for.
 *
 * Liveness comes from the page's own outbound message, which it posts once
 * on load. That is proof it ran, where `onload` only proves the browser
 * fetched something.
 */
function watchMapLoad(url) {
  if (view.mapWatch) clearTimeout(view.mapWatch);
  view.mapLoaded = false;
  view.mapWatch = setTimeout(() => {
    view.mapWatch = null;
    // `mapBase` is "a map is meant to be showing"; the frame's own `hidden`
    // is also set by the Files and Statistics panes, which disarmed the
    // watchdog for a reader who opened one within 8 seconds.
    if (view.mapLoaded || !view.mapBase) return;
    showPlaceholder(
      t("map.blank.title"),
      t("map.blank.body") + '<br><span class="detail">' + escapeHtml(url) + "</span>"
    );
  }, 8000);
}

/** Reload the map with the current theme, if one is showing. */
function retheme() {
  if (view.mapBase) els.frame.src = mapUrl(view.mapBase);
}

function currentTheme() {
  return document.documentElement.getAttribute("data-theme") || "system";
}

/** What the menu needs in order to draw its checkmarks. */
function viewState() {
  return {
    theme: currentTheme(),
    locale,
    // Endonyms, straight from the catalog's own list — the menu never
    // translates a language name.
    locales: LOCALES.map((l) => ({ code: l.code, label: l.label })),
    files: filesOpen,
    stats: statsOpen,
    rules: rulesOpen,
    sidebar: sidebarShown,
    sidebarAuto: !sidebarPinned,
  };
}

{
  let savedZoom = 1;
  let savedSidebar = "1";
  try {
    savedZoom = Number(localStorage.getItem(ZOOM_KEY)) || 1;
    savedSidebar = localStorage.getItem(SIDEBAR_KEY) ?? "1";
  } catch (e) {
    void e;
  }
  applySidebar(savedSidebar !== "0");
  if (savedZoom !== 1) applyZoom(savedZoom);
}


// =====================================================================
// Feedback
//
// Builds a prefilled GitHub form and opens it; the writer reads it there
// and submits it themselves. `src-tauri/src/feedback.rs` carries the two
// reasons — this app holds no credentials, and filing to a public tracker
// is publishing, which is not something a dialog should do on someone's
// behalf while they are looking at a Send button.
//
// The environment block is opt-in and shown verbatim before anything opens,
// because it is the one part of the report the writer did not type.
// =====================================================================

const fb = {
  box: document.getElementById("fbbox"),
  topic: document.getElementById("fb-topic"),
  repo: document.getElementById("fb-repo"),
  subject: document.getElementById("fb-subject"),
  body: document.getElementById("fb-body"),
  attach: document.getElementById("fb-attach"),
  env: document.getElementById("fb-env"),
  problem: document.getElementById("fb-problem"),
  go: document.getElementById("fb-go"),
};

let aboutInfo = null;

/** The attached block, exactly as it will be sent. Never translated: it is
 *  read by whoever triages the report, not by the person writing it. */
function envBlock() {
  if (!aboutInfo) return "";
  const lines = [
    "docmap-desktop " + aboutInfo.appVersion,
    aboutInfo.os + " / " + aboutInfo.arch,
    "engine: " + (aboutInfo.enginePath || "not configured"),
    "grammars: " + (aboutInfo.grammars || "none"),
    "interface: " + locale,
  ];
  return lines.join("\n");
}

function renderEnv() {
  // Nothing to attach is a real state —  can fail, and a report
  // is still worth filing without it. The whole row goes away rather than
  // leaving a ticked checkbox above an empty box, which would claim
  // something is being attached when nothing is.
  const text = envBlock();
  fb.env.textContent = text;
  fb.env.hidden = !fb.attach.checked || !text;
  fb.attach.closest(".fb-check").hidden = !text;
}

async function openFeedback() {
  fb.problem.hidden = true;
  if (!aboutInfo) {
    try {
      aboutInfo = await invoke("about_info");
    } catch (e) {
      // A report without the version block is still worth filing.
      void e;
    }
  }
  renderEnv();
  fb.box.showModal();
  fb.subject.focus();
}

fb.attach.addEventListener("change", renderEnv);

fb.go.addEventListener("click", async () => {
  const subject = fb.subject.value.trim();
  if (!subject) {
    fb.problem.textContent = t("fb.needsSubject");
    fb.problem.hidden = false;
    fb.subject.focus();
    return;
  }
  const parts = [fb.body.value.trim()];
  if (fb.attach.checked && aboutInfo) {
    parts.push("---", envBlock());
  }
  try {
    await invoke("open_feedback", {
      repo: fb.repo.value,
      topic: fb.topic.value,
      title: subject,
      body: parts.filter(Boolean).join("\n\n"),
    });
    fb.box.close();
  } catch (e) {
    fb.problem.textContent = errorText(e);
    fb.problem.hidden = false;
  }
});


// =====================================================================
// Telemetry
//
// This window cannot produce any, and says so rather than implying it
// could: the counts come from wrapping functions inside a running Neovim
// that is executing the plugin's own code. What it can do is read what was
// collected and flip the persistent switch that decides whether the next
// session collects — which is a real capability rather than a workaround,
// because `runtime-analysis.nvim` keeps that flag on disk precisely so a
// decision taken outside a session survives into the next one.
//
// So every wording here carries "from the next session". A switch that
// says "on" and means "on tomorrow" is the kind of half-truth this window
// keeps removing from itself.
// =====================================================================

const tel = {
  state: document.getElementById("tel-state"),
  toggle: document.getElementById("tel-toggle"),
  snapsLabel: document.getElementById("tel-snaps-label"),
  snaps: document.getElementById("tel-snaps"),
};

/** The namespace for a project: a telemetry namespace is a plugin name. */
function telemetryNamespace(project) {
  return project ? project.name : null;
}

function fill(text, vars) {
  // A function as the replacement, so a `$&` in a path or an error's detail
  // is inserted as written rather than read as a replacement pattern.
  return Object.keys(vars).reduce((s, k) => s.replace("{" + k + "}", () => vars[k]), text);
}

async function renderTelemetry() {
  const p = view.projects.find((x) => x.id === view.selectedId);
  const ns = telemetryNamespace(p);
  tel.toggle.hidden = true;
  tel.snaps.hidden = true;
  tel.snapsLabel.hidden = true;
  if (!ns) {
    tel.state.textContent = "";
    return;
  }

  let info;
  try {
    info = await invoke("telemetry_info", { namespace: ns });
  } catch (e) {
    void e;
    tel.state.textContent = t("tel.failed");
    return;
  }

  // Three states that look alike and are not: the tracker was never here,
  // the tracker is here but has never seen this project, and it has.
  if (!info.installed) {
    tel.state.textContent = t("tel.absent");
    return;
  }
  if (!info.known) {
    tel.state.textContent = fill(t("tel.unknown"), { name: ns });
    return;
  }

  tel.state.textContent = fill(t(info.disabled ? "tel.off" : "tel.on"), {
    sessions: info.sessions,
    days: info.days.length,
  });
  tel.toggle.textContent = t(info.disabled ? "tel.enable" : "tel.disable");
  tel.toggle.hidden = false;
  tel.toggle.dataset.ns = ns;
  tel.toggle.dataset.enable = info.disabled ? "1" : "";

  tel.snapsLabel.hidden = false;
  if (!info.snapshots.length) {
    // The actionable sentence, not a blank. An empty list here is the normal
    // state — snapshots are never automatic — and without saying so it reads
    // as a feature that does not work.
    renderRich(tel.snapsLabel, t("tel.snaps.none"));
    return;
  }
  tel.snapsLabel.textContent = t("tel.snaps");
  tel.snaps.replaceChildren();
  info.snapshots.forEach((s) => {
    const li = document.createElement("li");
    const when = new Date(s.takenAt * 1000);
    // Through the catalog: the last version built this line by
    // concatenation and left "session(s)" in English inside a German
    // dialog. `toLocaleString` already follows the reader's locale.
    li.textContent = fill(t("tel.snapItem"), {
      name: s.name,
      when: when.toLocaleString(),
      sessions: s.sessions,
    });
    tel.snaps.append(li);
  });
  tel.snaps.hidden = false;
}

tel.toggle.addEventListener("click", async () => {
  const ns = tel.toggle.dataset.ns;
  if (!ns) return;
  tel.toggle.disabled = true;
  try {
    await invoke("set_telemetry", { namespace: ns, enabled: !!tel.toggle.dataset.enable });
    await renderTelemetry();
  } catch (e) {
    tel.state.textContent = errorText(e);
  } finally {
    tel.toggle.disabled = false;
  }
});


// =====================================================================
// GitHub traffic
//
// The numbers are `github_stats.nvim`'s, read through the `traffic_*`
// commands from the small digest that plugin writes; this window never talks
// to GitHub and never sees a token. Two places show them: a line under the
// project in the sidebar, and a fifth sort order. Settings holds the panel
// that says where the digest was found and offers the two ways to point at it
// when the default is wrong.
//
// **Everything from the digest is text from outside** — a referrer is
// whatever a website sent — so nothing in this section assigns `innerHTML`.
// The catalog's own markup (`<strong>`, `<code>`) reaches the page through
// `data-i18n`, never through here; what this section writes is `textContent`.
//
// And absent is not zero: a project with no GitHub remote, no plugin, or a
// repository the plugin does not track says nothing (or says which of those
// it is) — it never shows `0`.
// =====================================================================

/** The list entries `traffic_list` returned, by project id. Filled when the
    order is asked for, and again by "Look again". */
const trafficEntries = new Map();

/**
 * Read every listed project's numbers, so the order has something to sort by.
 * One call for the whole list — the backend resolves each project's remote
 * once and remembers it, so this is cheap after the first time.
 */
async function measureTraffic({ busy = true } = {}) {
  if (busy) els.sort.disabled = true;
  try {
    const list = await invoke("traffic_list", { ids: view.projects.map((p) => p.id) });
    trafficEntries.clear();
    for (const entry of list) trafficEntries.set(entry.id, entry);
  } catch (e) {
    // Not knowing is not the same as no traffic; the entries stay empty and
    // every project sorts as "unknown", i.e. by name.
    void e;
  } finally {
    if (busy) els.sort.disabled = false;
  }
}

/**
 * The last-30-days figure on each row of the overview, where it has one.
 *
 * A chip only for a project that has numbers: no remote, no plugin and
 * "not tracked" leave the row exactly as it was, because a `0` there would
 * be a claim. The ranking itself is untouched — this list is ordered by what
 * needs doing, and traffic is a fact about the project, not a task.
 */
async function fillOverviewTraffic() {
  if (!trafficEntries.size) await measureTraffic({ busy: false });
  const nf = new Intl.NumberFormat(locale);
  for (const li of els.ovList.children) {
    const entry = trafficEntries.get(li.dataset.id);
    const open = li.querySelector(".ov-open");
    if (!open || open.querySelector(".ov-traffic") || !hasNumbers(entry)) continue;
    const chip = document.createElement("span");
    chip.className = "ov-traffic";
    chip.textContent = fill(t("ov.traffic"), { views: nf.format(entry.views30) });
    chip.title = fill(t("ov.trafficTitle"), { clones: nf.format(entry.clones30 ?? 0) });
    open.classList.add("has-traffic");
    open.append(chip);
  }
}

/** A day, in the reader's own date format. `null` if the value is unusable. */
function trafficDay(iso) {
  const at = Date.parse(iso);
  return Number.isFinite(at) ? new Date(at).toLocaleDateString(locale) : null;
}

/** The sidebar's traffic line for one project — or nothing at all. */
async function renderTraffic(p) {
  els.traffic.hidden = true;
  els.trafficAge.hidden = true;
  els.trafficDetailOpen.hidden = true;
  els.traffic.textContent = "";
  els.trafficAge.textContent = "";
  els.traffic.removeAttribute("title");
  els.trafficAge.classList.remove("old");
  if (!p) return;

  let info;
  try {
    info = await invoke("traffic_info", { id: p.id });
  } catch (e) {
    // A failed read costs the line and nothing else.
    void e;
    return;
  }
  // The answer can arrive after the selection moved on; writing it then would
  // put one project's numbers under another's name.
  if (view.selectedId !== p.id) return;

  const line = trafficLine(info);
  if (!line) return;

  if (info.status !== "ok") {
    els.traffic.textContent = fill(t(line.key), line.vars);
    if (info.status === "not_tracked") els.traffic.title = t("traffic.notTrackedTitle");
    els.traffic.hidden = false;
    return;
  }

  const s = info.summary;
  const tr = trendOf(s.views.trend, locale);
  els.traffic.textContent = fill(t("traffic.line"), {
    views: trafficWindows(s.views, "count", locale),
    clones: trafficWindows(s.clones, "count", locale),
    trend: tr.arrow ? tr.arrow + " " + tr.text : "",
  }).trim();
  els.traffic.title = fill(t("traffic.lineTitle"), {
    viewsUniques: trafficWindows(s.views, "uniques", locale),
    clonesUniques: trafficWindows(s.clones, "uniques", locale),
    trend: tr.text || "–",
  });
  els.traffic.hidden = false;
  els.trafficDetailOpen.hidden = false;

  // How old the data is, and a plain statement when it is old: a number that
  // is three weeks stale and looks current is the one way this line can lie.
  const asOf = s.fetched || s.generated;
  const day = trafficDay(asOf);
  if (day) {
    const old = isStale(asOf);
    els.trafficAge.textContent = old
      ? fill(t("traffic.stale"), { date: day, days: String(ageInDays(asOf)) })
      : fill(t("traffic.asof"), { date: day });
    els.trafficAge.classList.toggle("old", old);
    els.trafficAge.hidden = false;
  }
}

const trafficUi = {
  state: document.getElementById("traffic-state"),
  looked: document.getElementById("traffic-looked"),
  choose: document.getElementById("traffic-choose"),
  ask: document.getElementById("traffic-ask"),
  clear: document.getElementById("traffic-clear"),
  refresh: document.getElementById("traffic-refresh"),
  show: document.getElementById("traffic-show"),
};

/** What the panel says about the discovery chain: what was found, or what was
    looked at and why it did not qualify. */
function renderTrafficSurvey(survey) {
  trafficUi.clear.hidden = !survey.explicit;
  trafficUi.looked.hidden = true;
  trafficUi.looked.textContent = "";

  if (survey.found) {
    const f = survey.found;
    trafficUi.state.textContent = fill(t("traffic.found"), {
      dir: f.dir,
      repos: String(f.repos),
      when: f.newest ? new Date(f.newest * 1000).toLocaleDateString(locale) : "–",
      via: t("traffic.via." + f.via),
    });
    return;
  }

  trafficUi.state.textContent = t("traffic.none");
  const tried = survey.attempts.map((a) =>
    fill(t("traffic.lookedItem"), {
      dir: a.dir,
      via: t("traffic.via." + a.via),
      // Why this folder did not qualify, as the command's error says it.
      reason: a.reason ? errorText(a.reason) : "",
    })
  );
  if (tried.length) {
    trafficUi.looked.textContent = fill(t("traffic.looked"), { list: tried.join("; ") });
    trafficUi.looked.hidden = false;
  }
}

/** Fill the Settings panel. Asked when the dialog opens, like telemetry. */
async function renderTrafficSettings() {
  const p = view.projects.find((x) => x.id === view.selectedId);
  trafficUi.show.disabled = !p;
  trafficUi.show.checked = p ? !p.traffic_hidden : true;

  try {
    renderTrafficSurvey(await invoke("traffic_settings"));
  } catch (e) {
    void e;
    trafficUi.state.textContent = t("traffic.failed");
  }
}

/** Every reader of the numbers, after something that changes them. */
async function refreshTraffic() {
  trafficEntries.clear();
  if (sortBy === "traffic") {
    await measureTraffic();
    await render();
  } else {
    renderDetail();
    // The overview's per-row chips (fillOverviewTraffic) are the other
    // reader of trafficEntries; without this they keep showing the source
    // that was just replaced or cleared.
    if (!view.selectedId) renderOverview();
  }
}

trafficUi.choose.addEventListener("click", async () => {
  try {
    const dir = await open({ directory: true, multiple: false, title: t("traffic.chooseTitle") });
    if (!dir) return;
    renderTrafficSurvey(await invoke("traffic_set_dir", { path: dir }));
    await refreshTraffic();
  } catch (e) {
    say(errorText(e));
  }
});

trafficUi.clear.addEventListener("click", async () => {
  try {
    renderTrafficSurvey(await invoke("traffic_set_dir", { path: null }));
    await refreshTraffic();
  } catch (e) {
    say(errorText(e));
  }
});

trafficUi.ask.addEventListener("click", async () => {
  // A real `nvim --headless` run, so it gets the same busy treatment as the
  // other long buttons rather than a silent freeze.
  await withBusyButton(trafficUi.ask, t("traffic.asking"), async () => {
    try {
      renderTrafficSurvey(await invoke("traffic_ask_neovim"));
      await refreshTraffic();
    } catch (e) {
      trafficUi.state.textContent = errorText(e);
    }
  });
});

trafficUi.refresh.addEventListener("click", async () => {
  try {
    await invoke("traffic_refresh");
    renderTrafficSurvey(await invoke("traffic_settings"));
    await refreshTraffic();
  } catch (e) {
    say(errorText(e));
  }
});

// The opt-out. Per project, kept in this app: a repository someone tracks in
// the plugin and still does not want on screen or in a screenshot. Off means
// nothing is read for it — not even its remote.
trafficUi.show.addEventListener("change", async () => {
  const p = view.projects.find((x) => x.id === view.selectedId);
  if (!p) return;
  const hidden = !trafficUi.show.checked;
  try {
    await invoke("traffic_set_hidden", { id: p.id, hidden });
    p.traffic_hidden = hidden;
    await refreshTraffic();
  } catch (e) {
    // The checkbox already flipped to `!hidden` when the user clicked it,
    // before this handler ran. `hidden` is the pre-click state, so revert
    // to `hidden` (not `!hidden`, which would just re-apply the failed
    // toggle) to match `p.traffic_hidden`, which was never updated.
    trafficUi.show.checked = hidden;
    say(errorText(e));
  }
});

// ---------------------------------------------------------------------
// The traffic detail dialog
//
// The top pages GitHub reported for one project, and the span the stored
// numbers cover (github_stats.nvim keeps well past GitHub's own 14 days).
// Reached from the sidebar line, on a project that has one. Charts and the
// referrer list used to be here and were dropped: the pages are what is
// worth reading.
//
// Same reasoning as the rest of this section for the list: a page title is
// text from outside, so every row is built with `textContent`, never
// `innerHTML`.
// ---------------------------------------------------------------------

const trafficBox = {
  el: document.getElementById("trafficbox"),
  span: document.getElementById("traffic-detail-span"),
  paths: document.getElementById("traffic-paths"),
  pathsEmpty: document.getElementById("traffic-paths-empty"),
};

/**
 * A top-page row — a link to the file, but only for the entries the Rust
 * side resolved (`project_path`): a raw GitHub path is untrusted text, and
 * `traffic_detail` already did the one check that decides whether this row
 * may jump anywhere. Everything else stays a plain row.
 */
function renderTrafficPaths(list, id) {
  trafficBox.paths.replaceChildren();
  const rows = Array.isArray(list) ? list : [];
  trafficBox.pathsEmpty.hidden = rows.length > 0;
  const nf = new Intl.NumberFormat(locale);
  for (const p of rows) {
    const li = document.createElement("li");
    const name = document.createElement("span");
    name.className = "ov-deps-name";
    name.textContent = p.title || p.path;
    const who = document.createElement("span");
    who.className = "ov-deps-who";
    who.textContent = fill(t("traffic.detail.count"), {
      count: nf.format(p.count ?? 0),
      uniques: nf.format(p.uniques ?? 0),
    });
    li.append(name, who);
    if (p.project_path) {
      li.classList.add("traffic-path-open");
      li.title = t("traffic.detail.openFile");
      makeOpenable(li, () => {
        invoke("open_in_editor", { id, path: p.project_path, line: null }).catch((err) =>
          say(errorText(err))
        );
      });
    }
    trafficBox.paths.append(li);
  }
}

/** Fill the dialog from one project's whole digest. */
function renderTrafficDetail(d, id) {
  trafficBox.span.textContent = d.span
    ? fill(t("traffic.detail.span"), {
        from: trafficDay(d.span.from) ?? d.span.from,
        to: trafficDay(d.span.to) ?? d.span.to,
      })
    : "";

  renderTrafficPaths(d.paths, id);
}

document.getElementById("proj-traffic-detail").addEventListener("click", async () => {
  const id = view.selectedId;
  if (!id) return;
  try {
    const d = await invoke("traffic_detail", { id });
    // The answer can arrive after the selection moved on — same guard as
    // `renderTraffic`, so a slow read never shows one project's traffic
    // under another's name.
    if (view.selectedId !== id) return;
    if (!d) {
      say(t("traffic.detail.none"));
      return;
    }
    renderTrafficDetail(d, id);
    trafficBox.el.showModal();
  } catch (e) {
    say(errorText(e));
  }
});

// =====================================================================
// What changed since the map was made
//
// The evidence behind the "map outdated" chip. Newest first and capped: a
// repository that was reformatted has thousands of touched files, and the
// reader needs the shape of the change, not an inventory. The total is
// still reported, so a capped list says it is capped.
// =====================================================================

const changesBox = {
  el: document.getElementById("changesbox"),
  lead: document.getElementById("changes-lead"),
  list: document.getElementById("changes-list"),
  more: document.getElementById("changes-more"),
  generate: document.getElementById("changes-generate"),
};

const CHANGES_LIMIT = 100;

els.stale.addEventListener("click", async () => {
  const id = view.selectedId;
  if (!id) return;
  try {
    const c = await invoke("map_changes", { id, limit: CHANGES_LIMIT });
    if (view.selectedId !== id) return;
    changesBox.lead.textContent = plural("changes.lead", c.total);
    changesBox.list.replaceChildren();
    for (const f of c.files) {
      const li = document.createElement("li");
      li.classList.add("traffic-path-open");
      li.title = t("traffic.detail.openFile");
      const name = document.createElement("span");
      name.className = "ov-deps-name";
      name.textContent = f.path;
      const when = document.createElement("span");
      when.className = "ov-deps-who";
      const ago = agoText(f.afterSecs);
      when.textContent = ago ? fill(t("changes.after"), { ago }) : "";
      li.append(name, when);
      makeOpenable(li, () => {
        invoke("open_in_editor", { id, path: f.path, line: null }).catch((err) => say(errorText(err)));
      });
      changesBox.list.append(li);
    }
    const rest = c.total - c.files.length;
    changesBox.more.hidden = rest <= 0 && !c.truncated;
    changesBox.more.textContent = c.truncated
      ? t("changes.truncated")
      : fill(t("changes.more"), { n: String(rest) });
    // Generating needs an engine and a selection; with neither, the button
    // would close the dialog and do nothing.
    changesBox.generate.disabled = els.gen.disabled;
    changesBox.el.showModal();
  } catch (e) {
    say(errorText(e));
  }
});

changesBox.generate.addEventListener("click", () => {
  changesBox.el.close();
  els.gen.click();
});

// =====================================================================
// Asking the map a question
//
// The map is a cross-origin document: this window cannot read into it. The
// page answers a fixed, tiny set of questions about itself and takes no
// instructions — see `core/render/html.lua`'s own note on why that shape
// was chosen over a command channel.
//
// One asker, promise-shaped, with a timeout. A page that is still loading,
// or one generated before the channel existed, simply never answers — and
// "never answers" has to become a sentence rather than a spinner that stays
// forever.
// =====================================================================

let askId = 0;
const pending = new Map();

window.addEventListener("message", (ev) => {
  const d = ev.data;
  if (!d || d.source !== "docmap" || d.replyTo === undefined) return;
  if (ev.source !== els.frame.contentWindow) return;
  const waiting = pending.get(d.replyTo);
  if (!waiting) return;
  pending.delete(d.replyTo);
  waiting(d);
});

/** Ask the map something. Resolves with its answer, or `null` on silence. */
function askMap(ask, timeoutMs = 2000) {
  const frame = els.frame.contentWindow;
  if (!frame) return Promise.resolve(null);
  const id = ++askId;
  return new Promise((resolve) => {
    pending.set(id, resolve);
    setTimeout(() => {
      if (pending.delete(id)) resolve(null);
    }, timeoutMs);
    // `"*"` as the target: the frame's origin is the local server's, which
    // changes port per run. The message carries nothing secret — it is a
    // question, and the page answers only to the origin that asked.
    frame.postMessage({ source: "docmap-host", id, ask }, "*");
  });
}

/**
 * Save the diagram the map is currently showing.
 *
 * Three outcomes, and each says which: there is no diagram on this tab, the
 * page never answered, or a file was written where the reader chose.
 */
async function exportCurrentView() {
  const reply = await askMap("export-svg");
  if (!reply) {
    say(t("export.silent"));
    return;
  }
  if (!reply.ok) {
    say(t("export.none"));
    return;
  }
  const path = await save({
    defaultPath: reply.name,
    filters: [{ name: "SVG", extensions: ["svg"] }],
  });
  // A cancelled dialog is a decision, not a failure: nothing is said about
  // it, because the reader already knows what they did.
  if (!path) return;
  try {
    await invoke("save_text", { path, contents: reply.data });
    say(t("export.done").replace("{name}", path));
  } catch (e) {
    say(errorText(e));
  }
}


// =====================================================================
// The repository as it is on disk
//
// Beside the map rather than inside it. The map is a snapshot of one
// generation and deliberately shows only modules; this is what is actually
// there, read now — and it is read by this program because this program is
// the one that can also open a file when asked.
//
// One directory per call. A monorepo is tens of thousands of files and a
// reader opens a dozen folders, so walking everything to draw one level is
// work nobody asked for and a window that stalls.
// =====================================================================

let filesOpen = false;
let statsOpen = false;
let rulesOpen = false;
let filesPath = "";

function formatSize(n) {
  if (n === null || n === undefined) return "";
  if (n < 1024) return n + " B";
  if (n < 1024 * 1024) return Math.round(n / 1024) + " KB";
  return (n / (1024 * 1024)).toFixed(1) + " MB";
}

/// Which `renderFiles` call is the latest. The listing runs off the main thread
/// and reading git's state inside it takes anywhere from milliseconds to seconds, so
/// answers arrive in the order they finish: only the newest one may draw, and
/// not after the pane was closed. Bumped before the early return, so a call
/// that finds nothing to show retires whatever is still on its way as well.
let filesSeq = 0;

async function renderFiles() {
  const mine = ++filesSeq;
  if (!filesOpen || !view.selectedId) return;
  const crumb = document.getElementById("files-crumb");
  const list = document.getElementById("files-list");

  let listing;
  try {
    listing = await invoke("file_tree", { id: view.selectedId, sub: filesPath });
  } catch (e) {
    if (mine !== filesSeq || !filesOpen) return;
    list.replaceChildren();
    crumb.textContent = "";
    const li = document.createElement("li");
    li.className = "files-msg";
    li.textContent = errorText(e);
    list.append(li);
    return;
  }
  if (mine !== filesSeq || !filesOpen) return;

  // Breadcrumb: the project name, then every segment of the current path.
  crumb.replaceChildren();
  const project = view.projects.find((p) => p.id === view.selectedId);
  const segments = filesPath ? filesPath.split("/") : [];
  const crumbButton = (label, target) => {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = label;
    b.addEventListener("click", () => {
      filesPath = target;
      renderFiles();
    });
    return b;
  };
  crumb.append(crumbButton(project ? project.name : "/", ""));
  segments.forEach((seg, i) => {
    crumb.append(document.createTextNode(" / "));
    if (i === segments.length - 1) {
      const here = document.createElement("span");
      here.className = "here";
      here.textContent = seg;
      crumb.append(here);
      return;
    }
    crumb.append(crumbButton(seg, segments.slice(0, i + 1).join("/")));
  });

  list.replaceChildren();
  if (filesPath) {
    const up = document.createElement("li");
    const b = document.createElement("button");
    b.type = "button";
    b.className = "files-row up";
    b.textContent = "..";
    b.addEventListener("click", () => {
      filesPath = filesPath.split("/").slice(0, -1).join("/");
      renderFiles();
    });
    up.append(b);
    list.append(up);
  }

  if (!listing.entries.length) {
    const li = document.createElement("li");
    li.className = "files-msg";
    li.textContent = t("files.empty");
    list.append(li);
    return;
  }

  listing.entries.forEach((e) => {
    const li = document.createElement("li");
    const row = document.createElement("button");
    row.type = "button";
    row.className = "files-row" + (e.isDir ? " dir" : "");

    const name = document.createElement("span");
    name.className = "n";
    name.textContent = e.name;
    row.append(name);

    // What the map does with this entry, and what git thinks of it — said
    // here rather than left to be wondered about. Every one of these is a
    // true statement about the disk.
    //
    // **The two halves answer opposite surprises**, which is why both exist.
    // `skipped`/`nested` explain a folder that is on screen and *not* in the
    // map. `ignored` explains the reverse and is the one nothing else in this
    // window could say: the scan does not read `.gitignore` — a repository
    // can quite reasonably ignore a directory the map should still describe —
    // so a folder you ignore in git is mapped anyway, and only this line
    // tells you that before you go looking for a bug.
    //
    // At most one, and in this order. A row that carried three notes would
    // be a row nobody reads, and the outermost fact wins: a folder that is
    // not scanned at all makes what git thinks of it beside the point.
    const why = e.nestedRepo
      ? "files.nested"
      : e.skipped
        ? "files.skipped"
        : e.ignored
          ? "files.ignored"
          : e.untracked
            ? "files.untracked"
            : null;
    if (why) {
      const el = document.createElement("span");
      // `ignored` and `untracked` are about git rather than about the scan,
      // so they are toned differently — the reader is being told a fact, not
      // given a reason the map is missing something.
      el.className = "why" + (why === "files.ignored" || why === "files.untracked" ? " git" : "");
      el.textContent = t(why);
      row.append(el);
    }

    const meta = document.createElement("span");
    meta.className = "m";
    meta.textContent = e.isDir ? "" : formatSize(e.size);
    row.append(meta);

    row.addEventListener("click", () => {
      if (e.isDir) {
        filesPath = e.path;
        renderFiles();
        return;
      }
      // A file opens where files open — the editor setting decides which,
      // and an empty setting means the desktop's own association.
      invoke("open_in_editor", { id: view.selectedId, path: e.path, line: null }).catch((err) =>
        say(errorText(err))
      );
    });

    li.append(row);
    list.append(li);
  });
}

/**
 * Which of the three views fills the main pane: the map, the file tree or
 * the statistics. The latter two are overlays on the map (the frame keeps its
 * page, so coming back costs no reload), which is why they are one function:
 * showing one hides the other and the frame stays put underneath.
 *
 * @param {"map"|"files"|"stats"|"rules"} name
 */
function setPane(name) {
  // The panes belong to a project; with none selected the overview is the
  // view, and a pane opened over it (View menu items are enabled without a
  // project) would be an empty overlay with no tabs to leave it by.
  if (!view.selectedId) name = "map";
  filesOpen = name === "files";
  statsOpen = name === "stats";
  rulesOpen = name === "rules";
  const overlay = filesOpen || statsOpen || rulesOpen;
  document.getElementById("files").hidden = !filesOpen;
  document.getElementById("stats").hidden = !statsOpen;
  document.getElementById("rules").hidden = !rulesOpen;
  // The map keeps its src: coming back should not cost a reload of a
  // two-megabyte page that has not changed.
  els.frame.hidden = overlay || !view.mapBase;
  els.placeholder.hidden = overlay || !!view.mapBase || (!view.selectedId && view.projects.length > 0);
  els.overview.hidden = overlay || !!view.mapBase || !!view.selectedId || view.projects.length === 0;
  if (filesOpen) {
    filesPath = "";
    renderFiles();
  }
  if (statsOpen) renderStats();
  if (rulesOpen) renderRules();
  syncPaneTabs();
  syncMenu();
}

/** Hide both panes without touching what the map frame shows. */
function closePanes() {
  filesOpen = false;
  statsOpen = false;
  rulesOpen = false;
  document.getElementById("files").hidden = true;
  document.getElementById("stats").hidden = true;
  document.getElementById("rules").hidden = true;
  syncPaneTabs();
}

/** Show the filetree instead of the map, or the other way back. */
function setFiles(on) {
  setPane(on ? "files" : "map");
}

function setStats(on) {
  setPane(on ? "stats" : "map");
}

/** A pane that is open shows the project that is now selected. */
function refreshPanes(keepPath = false) {
  if (filesOpen) {
    if (!keepPath) filesPath = "";
    renderFiles();
  }
  if (statsOpen) renderStats();
  if (rulesOpen) renderRules();
  syncPaneTabs();
}

const paneTabs = [...document.querySelectorAll(".viewtabs button")];

function syncPaneTabs() {
  const now = rulesOpen ? "rules" : statsOpen ? "stats" : filesOpen ? "files" : "map";
  for (const b of paneTabs) {
    const on = b.dataset.pane === now;
    b.classList.toggle("on", on);
    b.setAttribute("aria-selected", String(on));
  }
}

for (const b of paneTabs) b.addEventListener("click", () => setPane(b.dataset.pane));

// =====================================================================
// Rules
//
// A project's rules, from the engine (`rules_catalog`), and the mechanical
// run of the ones that have a check (`rules_run`). The list logic is in
// `lib/rules.js`; this is the drawing and the clicks.
//
// Nothing here asks a model. Start runs the automated rules in the engine on
// this machine. The manual ones of a selection are left for an agent run,
// which is a separate, explicit step: for them the engine is only asked what
// it *would* send (`agent_preview`, which sends nothing), so a rule that
// could not be sent says why in its row.
// =====================================================================

/** Per project id: catalog, results, selection and how the list is shown. */
const rulesState = new Map();

function rulesOf(id) {
  let st = rulesState.get(id);
  if (!st) {
    st = {
      catalog: null,
      loading: null,
      results: new Map(),
      selected: new Set(),
      notes: new Map(),
      collapsed: new Set(),
      open: new Set(),
      lane: "all",
      query: "",
      grouping: null,
      anchor: null,
      running: false,
    };
    rulesState.set(id, st);
  }
  return st;
}

const rulesUi = {
  title: document.getElementById("rules-title"),
  state: document.getElementById("rules-state"),
  trust: document.getElementById("rules-trust"),
  problems: document.getElementById("rules-problems"),
  body: document.getElementById("rules-body"),
  byFile: document.getElementById("rules-by-file"),
  byFamily: document.getElementById("rules-by-family"),
  chips: document.getElementById("rules-chips"),
  gates: document.getElementById("rules-gates"),
  query: document.getElementById("rules-query"),
  list: document.getElementById("rules-list"),
  selAuto: document.getElementById("rules-sel-auto"),
  selManual: document.getElementById("rules-sel-manual"),
  clear: document.getElementById("rules-clear"),
  note: document.getElementById("rules-start-note"),
  start: document.getElementById("rules-start"),
  refresh: document.getElementById("rules-refresh"),
  count: document.getElementById("proj-rules"),
};

/** The shape that goes with a lane's word, so the lane is never colour alone. */
const LANE_SHAPE = { auto: "◆", manual: "○" };
/** And with a mechanical status. */
const STATUS_SHAPE = { pass: "✓", fail: "✕", error: "!", waived: "–" };
/** And with what a person decided about a manual rule (the Agent column). */
const JUDGED_SHAPE = { violation: "✕", compliant: "✓", waived: "–" };
/** And with the trust of a ruleset. */
const TRUST_SHAPE = { trusted: "✓", untrusted: "○", changed: "!", unreadable: "✕" };

function laneLabel(lane, n) {
  const word = lane === "auto" ? t("rules.filter.auto") : t("rules.filter.manual");
  return `${LANE_SHAPE[lane]} ${fill(word, { n })}`;
}

/**
 * Read the project's catalog, once at a time per project. The answer is kept:
 * the sidebar's count and the tab read the same one.
 */
function loadRules(id) {
  const st = rulesOf(id);
  if (!st.loading) {
    // Both are engine calls (seconds each on Windows), and neither needs the other's answer: started together, the
    // count is right as soon as the catalog is there, not one engine call later.
    const judged = loadJudged(id);
    st.loading = invoke("rules_catalog", { id })
      .then((catalog) => {
        st.catalog = catalog;
        const rules = rulesList(catalog.rules);
        const known = new Set(rules.map((r) => r.id));
        st.selected = new Set([...st.selected].filter((rid) => known.has(rid)));
        if (!st.grouping) {
          // A ruleset with one file per rule makes "by file" a list of single
          // rules; its families are the readable grouping then.
          const files = new Set(rules.map((r) => r.source_file)).size;
          st.grouping = files * 2 > rules.length && rules.length > 8 ? "family" : "file";
        }
        return judged.then(() => catalog);
      })
      .finally(() => {
        st.loading = null;
      });
  }
  return st.loading;
}

/**
 * Read which rules a person decided about (`.rules-verdicts.json`,
 * `.rules-waivers.json`, through the engine's `rules/judged`): the Agent column shows it, and the sidebar does not
 * count those rules as open. A failure leaves nothing decided, which counts
 * every manual rule as open: too many, never too few.
 */
async function loadJudged(id) {
  const st = rulesOf(id);
  try {
    st.judged = await invoke("rules_judged", { id });
  } catch (e) {
    st.judged = null;
    void e;
  }
}

async function renderRules(force = false) {
  const id = view.selectedId;
  if (!rulesOpen || !id) return;
  const p = view.projects.find((x) => x.id === id);
  rulesUi.title.textContent = p ? fill(t("rules.titleFor"), { name: p.name }) : t("rules.title");
  const st = rulesOf(id);
  if (force) st.catalog = null;
  if (!st.catalog) {
    rulesUi.body.hidden = true;
    rulesUi.trust.hidden = true;
    rulesUi.problems.hidden = true;
    rulesUi.state.textContent = t("rules.reading");
    rulesUi.refresh.disabled = true;
    try {
      await loadRules(id);
    } catch (e) {
      if (id === view.selectedId) rulesUi.state.textContent = errorText(e);
      return;
    } finally {
      rulesUi.refresh.disabled = false;
    }
  }
  // The answer can arrive after the reader moved on or left the pane.
  if (id !== view.selectedId || !rulesOpen) return;
  drawRules(id);
  renderRulesCount();
  // the runs of the project, from their records (no engine, no network)
  loadRuns(id);
  if (rulesOf(id).openRun) refreshRun(id);
  // the picker's mark for this project, now that there is something to say
  void render();
}

/**
 * Draw the tab for project `id`. The list is rebuilt on every change, so what
 * the reader holds on to is carried over the rebuild: how far the pane is
 * scrolled (an emptied list would otherwise let it snap back to the top) and
 * which control has the keyboard (`data-fk`, so Space on a checkbox can be
 * followed by Tab).
 */
function drawRules(id) {
  const pane = document.getElementById("rules");
  const top = pane.scrollTop;
  const held = document.activeElement && pane.contains(document.activeElement)
    ? document.activeElement.dataset.fk
    : undefined;
  drawRulesNow(id);
  pane.scrollTop = top;
  if (held) {
    const again = [...pane.querySelectorAll("[data-fk]")].find((el) => el.dataset.fk === held);
    if (again) again.focus({ preventScroll: true });
  }
}

function drawRulesNow(id) {
  const st = rulesOf(id);
  const catalog = st.catalog;
  if (!catalog || catalog.available !== true) {
    runUi.view.hidden = true;
    runUi.runs.hidden = true;
    rulesUi.body.hidden = true;
    rulesUi.trust.hidden = true;
    rulesUi.problems.hidden = true;
    rulesUi.state.textContent = fill(t("rules.unavailable"), {
      reason: (catalog && catalog.reason) || "",
    });
    return;
  }
  const rules = rulesList(catalog.rules);
  const all = laneCounts(rules);
  rulesUi.state.textContent = fill(t("rules.summary"), {
    rules: all.total,
    auto: all.auto,
    manual: all.manual,
  });
  drawRulesProblems(catalog, st.judged);
  drawRulesTrust(id, catalog);
  rulesUi.body.hidden = false;

  for (const [button, grouping] of [
    [rulesUi.byFile, "file"],
    [rulesUi.byFamily, "family"],
  ]) {
    const on = st.grouping === grouping;
    button.classList.toggle("on", on);
    button.setAttribute("aria-pressed", String(on));
  }
  if (rulesUi.query.value !== st.query) rulesUi.query.value = st.query;

  // The chips count what the query leaves, so a chip never promises rows the
  // list will not show.
  const queried = visibleRules(rules, { query: st.query });
  const lanes = laneCounts(queried);
  rulesUi.chips.replaceChildren();
  for (const [lane, label] of [
    ["all", fill(t("rules.filter.all"), { n: lanes.total })],
    ["auto", laneLabel("auto", lanes.auto)],
    ["manual", laneLabel("manual", lanes.manual)],
  ]) {
    const chip = node("button", "rules-chip-filter" + (lane === "all" ? "" : " " + lane), label);
    chip.type = "button";
    chip.classList.toggle("on", st.lane === lane);
    chip.setAttribute("aria-pressed", String(st.lane === lane));
    chip.dataset.fk = "lane:" + lane;
    chip.addEventListener("click", () => {
      st.lane = lane;
      drawRules(id);
    });
    rulesUi.chips.append(chip);
  }

  const shown = visibleRules(rules, { lane: st.lane, query: st.query });
  // The gates of `.rules.json`: one click selects the shown rules of a gate's
  // families. Absent when the project names none.
  const gates = rulesList(catalog.gates);
  rulesUi.gates.replaceChildren();
  rulesUi.gates.hidden = gates.length === 0;
  for (const gate of gates) {
    const pick = node("button", "rules-pick", fill(t("rules.gate"), { name: gate.name }));
    pick.type = "button";
    pick.title = fill(t("rules.gate.title"), { families: rulesList(gate.families).join(", ") });
    pick.dataset.fk = "gate:" + gate.name;
    pick.addEventListener("click", () => {
      st.selected = selectFamilies(st.selected, shown, gate.families);
      drawRules(id);
    });
    rulesUi.gates.append(pick);
  }
  const groups = groupRules(shown, st.grouping, rulesList(catalog.rulesets));
  const order = orderedIds(groups);
  rulesUi.list.replaceChildren();
  if (!shown.length) rulesUi.list.append(node("p", "rules-note", t("rules.noMatch")));
  for (const group of groups) rulesUi.list.append(ruleGroup(id, st, group, order));
  drawRulesBar(id);
  drawAgentButton(id);
  drawRuns(id);
  syncRunView(id);
}

/**
 * What the engine could not read of the rulesets, what it warns about, and a
 * verdicts or waivers file of the project that could not be read.
 */
function drawRulesProblems(catalog, judged) {
  const lines = [...rulesList(catalog.warnings), ...rulesList(catalog.errors)].map(String);
  if (catalog.trustStoreError) lines.unshift(errorText(catalog.trustStoreError));
  for (const file of rulesList(judged?.unreadable)) lines.unshift(fill(t("rules.judged.unreadable"), { file }));
  rulesUi.problems.replaceChildren();
  rulesUi.problems.hidden = lines.length === 0;
  const SHOWN = 8;
  for (const line of lines.slice(0, SHOWN)) rulesUi.problems.append(node("li", null, line));
  if (lines.length > SHOWN) {
    rulesUi.problems.append(
      node("li", "more", fill(t("rules.more"), { n: lines.length - SHOWN }))
    );
  }
}

/** The rulesets that bring code, and what this machine thinks of each. */
function drawRulesTrust(id, catalog) {
  const withCode = rulesList(catalog.rulesets).filter(
    (r) => (r.trust?.predicates ?? 0) > 0 || r.trust?.state === "changed"
  );
  rulesUi.trust.replaceChildren();
  rulesUi.trust.hidden = withCode.length === 0;
  if (!withCode.length) return;
  if (untrustedRulesets(catalog).length) {
    rulesUi.trust.append(node("p", "rules-note", t("rules.trust.effect")));
  }
  for (const ruleset of withCode) {
    const state = ruleset.trust?.state ?? "untrusted";
    const row = node("div", "rules-trust-row " + state);
    const mark = node(
      "span",
      "rules-trust-state",
      `${TRUST_SHAPE[state] ?? "○"} ${t("rules.trust.state." + state)}`
    );
    const what = node(
      "span",
      "rules-trust-what",
      fill(t("rules.trust.row"), {
        path: ruleset.trust?.real ?? ruleset.path,
        n: ruleset.trust?.predicates ?? 0,
      })
    );
    row.append(mark, what);
    if (state === "trusted" && ruleset.trust?.by) {
      const at = ruleset.trust.at ? new Date(ruleset.trust.at).toLocaleDateString(locale) : "";
      row.append(
        node("span", "rules-trust-by", fill(t("rules.trust.byAt"), { by: ruleset.trust.by, date: at }))
      );
    }
    if (ruleset.trust?.problem) row.append(node("span", "rules-trust-by", errorText(ruleset.trust.problem)));
    if (state === "trusted") {
      const revoke = node("button", "addbox-secondary", t("rules.trust.revoke"));
      revoke.type = "button";
      revoke.addEventListener("click", async () => {
        revoke.disabled = true;
        try {
          await invoke("rules_trust_revoke", { id, path: ruleset.path });
        } catch (e) {
          say(errorText(e));
        }
        renderRules(true);
      });
      row.append(revoke);
    } else if (state !== "unreadable") {
      const open = node(
        "button",
        "addbox-secondary",
        state === "changed" ? t("rules.trust.again") : t("rules.trust.open")
      );
      open.type = "button";
      open.addEventListener("click", () => openTrust(id, ruleset));
      row.append(open);
    }
    rulesUi.trust.append(row);
  }
}

/** A checkbox that can be all, some or none. */
function triBox(state, label) {
  const box = document.createElement("input");
  box.type = "checkbox";
  box.checked = state === "all";
  box.indeterminate = state === "some";
  box.setAttribute("aria-label", label);
  if (state === "some") box.setAttribute("aria-checked", "mixed");
  return box;
}

function ruleGroup(id, st, group, order) {
  const wrap = node("section", "rules-group");
  const head = node("div", "rules-group-head");
  const folded = st.collapsed.has(group.key);

  const fold = node("button", "rules-fold", folded ? "▸" : "▾");
  fold.type = "button";
  fold.setAttribute("aria-expanded", String(!folded));
  fold.setAttribute("aria-label", t("rules.fold"));
  fold.dataset.fk = "fold:" + group.key;
  fold.addEventListener("click", () => {
    if (st.collapsed.has(group.key)) st.collapsed.delete(group.key);
    else st.collapsed.add(group.key);
    drawRules(id);
  });

  const box = triBox(
    checkState(group.rules, st.selected),
    fill(t("rules.select.group"), { name: group.label })
  );
  box.dataset.fk = "group:" + group.key;
  box.addEventListener("change", () => {
    st.selected = toggleGroup(st.selected, group.rules);
    drawRules(id);
  });

  const counts = laneCounts(group.rules);
  const name = node("span", "rules-group-name", group.label);
  name.title = group.label;
  head.append(fold, box, name);
  if (counts.auto) head.append(node("span", "rules-lane auto", laneLabel("auto", counts.auto)));
  if (counts.manual) {
    head.append(node("span", "rules-lane manual", laneLabel("manual", counts.manual)));
    const pick = node("button", "rules-pick", t("rules.pickManual"));
    pick.type = "button";
    pick.dataset.fk = "pick:" + group.key;
    pick.addEventListener("click", () => {
      st.selected = selectManual(st.selected, group.rules);
      drawRules(id);
    });
    head.append(pick);
  }
  wrap.append(head);
  if (folded) return wrap;

  const subs = group.subgroups.length ? group.subgroups : [{ key: group.key, label: "", rules: group.rules }];
  for (const sub of subs) {
    if (sub.label) {
      const subHead = node("div", "rules-sub-head");
      const subBox = triBox(
        checkState(sub.rules, st.selected),
        fill(t("rules.select.group"), { name: sub.label })
      );
      subBox.dataset.fk = "sub:" + sub.key;
      subBox.addEventListener("change", () => {
        st.selected = toggleGroup(st.selected, sub.rules);
        drawRules(id);
      });
      subHead.append(subBox, node("span", "rules-sub-name", sub.label));
      wrap.append(subHead);
    }
    for (const rule of sub.rules) wrap.append(ruleRow(id, st, rule, order));
  }
  return wrap;
}

function ruleRow(id, st, rule, order) {
  const lane = laneOf(rule);
  const wrap = node("div", "rules-item");
  const row = node("div", "rules-row " + lane);
  row.dataset.rule = rule.id;

  const box = document.createElement("input");
  box.type = "checkbox";
  box.checked = st.selected.has(rule.id);
  box.setAttribute("aria-label", fill(t("rules.select.rule"), { id: rule.id }));
  box.dataset.fk = "rule:" + rule.id;
  box.addEventListener("click", (ev) => {
    st.selected = ev.shiftKey
      ? selectRange(st.selected, order, st.anchor, rule.id)
      : toggleRule(st.selected, rule.id);
    st.anchor = rule.id;
    drawRules(id);
  });

  const mark = node(
    "span",
    "rules-lane " + lane,
    `${LANE_SHAPE[lane]} ${lane === "auto" ? t("rules.lane.auto") : t("rules.lane.manual")}`
  );
  const title = node("button", "rules-title", "");
  title.type = "button";
  title.append(node("span", "rules-id", rule.id), node("span", "rules-text", titleOf(rule)));
  title.setAttribute("aria-expanded", String(st.open.has(rule.id)));
  title.dataset.fk = "title:" + rule.id;
  title.addEventListener("click", () => {
    if (st.open.has(rule.id)) st.open.delete(rule.id);
    else st.open.add(rule.id);
    drawRules(id);
  });

  // Two slots that never share: what a check on this machine found, and what
  // an agent proposed (empty until there is a run to show).
  const mech = node("span", "rules-mech");
  const result = st.results.get(rule.id);
  if (result) {
    // The chip carries the status; whose result it is, is the column's head
    // (which stays in view) and the chip's own full name for a reader that
    // does not see columns.
    const word = t("rules.status." + result.status);
    const chip = node("span", "rules-status " + result.status, `${STATUS_SHAPE[result.status] ?? "?"} ${word}`);
    const full = fill(t("rules.mechanical"), { status: word });
    chip.title = full;
    chip.setAttribute("aria-label", full);
    mech.append(chip);
  }
  const agent = node("span", "rules-agent");
  const note = st.notes.get(rule.id);
  // What a person decided, from the project's verdicts and waivers. Here and
  // never in the mechanical slot: a verdict is a person's answer to a proposal.
  const judged = judgedOf(rule, st.judged);
  if (judged) {
    const chip =
      judged.kind === "verdict"
        ? node(
            "span",
            "rules-judged verdict",
            `${JUDGED_SHAPE[judged.verdict] ?? "■"} ${t("rules.judged.verdict." + judged.verdict)}`
          )
        : node("span", "rules-judged waived", `${JUDGED_SHAPE.waived} ${t("rules.judged.waived")}`);
    const full =
      judged.kind === "verdict"
        ? fill(t("rules.judged.verdictBy"), { by: judged.by, date: judged.date })
        : judged.expires
          ? fill(t("rules.judged.waivedUntil"), { reason: judged.reason, date: judged.expires })
          : fill(t("rules.judged.waivedFor"), { reason: judged.reason });
    chip.title = full;
    chip.setAttribute("aria-label", `${chip.textContent}: ${full}`);
    agent.append(chip);
  } else if (note) agent.append(node("span", "rules-left", t("rules.left")));

  row.append(box, mark, title, mech, agent);
  wrap.append(row);

  const detail = node("div", "rules-detail");
  if (st.open.has(rule.id)) {
    if (rule.text) detail.append(node("p", "rules-rule-text", rule.text));
    detail.append(
      node(
        "p",
        "rules-source",
        fill(t("rules.source"), { file: rule.source_file ?? "", line: rule.source_line ?? "" })
      )
    );
  }
  if (note && note.why) {
    detail.append(node("p", "rules-blocked", fill(t("rules.blocked"), { reason: note.why })));
  }
  if (result && (result.status === "fail" || result.status === "error")) {
    const findings = rulesList(result.findings);
    const SHOWN = 20;
    const ul = node("ul", "rules-findings");
    for (const f of findings.slice(0, SHOWN)) {
      const li = node("li");
      const where = f.file && f.file !== "." ? `${f.file}${f.line ? ":" + f.line : ""}` : "";
      if (where) {
        const open = node("button", "rules-finding-file", where);
        open.type = "button";
        open.addEventListener("click", () => {
          invoke("open_in_editor", { id, path: f.file, line: f.line ?? null }).catch((err) =>
            say(errorText(err))
          );
        });
        li.append(open);
      }
      li.append(node("span", "rules-finding-text", f.text ?? ""));
      ul.append(li);
    }
    if (findings.length > SHOWN) {
      ul.append(node("li", "more", fill(t("rules.more"), { n: findings.length - SHOWN })));
    }
    if (findings.length) detail.append(ul);
  }
  if (detail.childNodes.length) wrap.append(detail);
  return wrap;
}

/** The selection bar: the two lanes counted apart, and what Start will do. */
function drawRulesBar(id) {
  const st = rulesOf(id);
  const { auto, manual } = splitSelection(rulesList(st.catalog?.rules), st.selected);
  rulesUi.selAuto.textContent = `${LANE_SHAPE.auto} ${fill(t("rules.sel.auto"), { n: auto.length })}`;
  rulesUi.selManual.textContent = `${LANE_SHAPE.manual} ${fill(t("rules.sel.manual"), { n: manual.length })}`;
  rulesUi.clear.disabled = st.running || auto.length + manual.length === 0;
  rulesUi.start.disabled = st.running || auto.length === 0;
  rulesUi.start.textContent = st.running
    ? t("rules.start.running")
    : auto.length
      ? plural("rules.start", auto.length)
      : t("rules.start.none");
  rulesUi.note.textContent = manual.length
    ? plural("rules.note.mixed", manual.length)
    : t("rules.note.local");
}

/**
 * Start: the automated rules of the selection run in the engine, in as many
 * calls as the engine's limits ask for. A call that dies marks its rules as
 * errors instead of leaving them blank. The manual rules are marked as left
 * for an agent run, with the reason where one could not be sent.
 */
async function startRules() {
  const id = view.selectedId;
  if (!id) return;
  const st = rulesOf(id);
  if (st.running || !st.catalog) return;
  const { auto, manual } = splitSelection(rulesList(st.catalog.rules), st.selected);
  if (!auto.length) return;
  st.running = true;
  st.notes = new Map(manual.map((rid) => [rid, {}]));
  const redraw = () => {
    if (id === view.selectedId && rulesOpen) drawRules(id);
  };
  redraw();
  try {
    for (const selection of runSelections(auto)) {
      const asked = selection.split(",");
      let answer;
      try {
        answer = await invoke("rules_run", { id, selection });
        if (!answer || answer.available !== true) {
          answer = { failed: (answer && answer.reason) || t("err.rules.shape") };
        }
      } catch (e) {
        answer = { failed: errorText(e) };
      }
      st.results = mergeRun(st.results, asked, answer);
      redraw();
    }
    for (const selection of runSelections(manual)) {
      try {
        const plan = await invoke("agent_preview", { id, selection, model: "" });
        for (const [rid, why] of notSendable(plan.skipped)) st.notes.set(rid, { why });
      } catch (e) {
        // An engine without the agent half cannot say what would be sent; the
        // rows keep "left for an agent run" and the reason goes to the status.
        say(errorText(e));
        break;
      }
    }
  } finally {
    st.running = false;
    redraw();
    renderRulesCount();
    void render();
  }
  const counts = mechanicalCounts(
    rulesList(st.catalog.rules).filter((r) => auto.includes(r.id)),
    st.results
  );
  say(fill(t("rules.done"), counts));
}

/**
 * The sidebar's line for the selected project: manual rules still open and
 * automated ones whose last run did not pass. Hidden until the catalog has
 * been read, and for a project without rules.
 */
function renderRulesCount() {
  const id = view.selectedId;
  const counts = id
    ? sidebarCounts(rulesState.get(id)?.catalog ?? null, rulesOf(id).results, rulesOf(id).judged)
    : null;
  rulesUi.count.hidden = !counts;
  if (!counts) return;
  rulesUi.count.textContent = counts.failed
    ? fill(t("rules.count.failed"), counts)
    : fill(t("rules.count"), counts);
  rulesUi.count.title = t("rules.count.title");
  rulesUi.count.classList.toggle("failed", counts.failed > 0);
}

/** The same two numbers as a short mark in the project picker, when known. */
function rulesMark(id) {
  const st = rulesState.get(id);
  const counts = st ? sidebarCounts(st.catalog, st.results, st.judged) : null;
  if (!counts) return "";
  return ` (${LANE_SHAPE.manual}${counts.manual}${counts.failed ? ` ${STATUS_SHAPE.fail}${counts.failed}` : ""})`;
}

/**
 * The sidebar line of the project that was just selected: what is known of
 * its rules, and nothing when they have not been read yet. Reading them is
 * not done here: it starts the engine, which takes seconds on Windows, and a
 * project switch is not a request to do that. Opening the tab is.
 */
function countRulesFor() {
  renderRulesCount();
}

rulesUi.count.addEventListener("click", () => setPane("rules"));
rulesUi.refresh.addEventListener("click", () => renderRules(true));
rulesUi.start.addEventListener("click", () => startRules());
rulesUi.clear.addEventListener("click", () => {
  const id = view.selectedId;
  if (!id) return;
  rulesOf(id).selected = new Set();
  drawRules(id);
});
for (const [button, grouping] of [
  [rulesUi.byFile, "file"],
  [rulesUi.byFamily, "family"],
]) {
  button.addEventListener("click", () => {
    const id = view.selectedId;
    if (!id) return;
    rulesOf(id).grouping = grouping;
    drawRules(id);
  });
}
rulesUi.query.addEventListener("input", () => {
  const id = view.selectedId;
  if (!id) return;
  rulesOf(id).query = rulesUi.query.value;
  drawRules(id);
  // Drawing replaced the chips, not the box: the caret stays where it was.
});

// ---------------------------------------------------------------------
// Trusting a ruleset. The dialog shows what the backend would trust and
// hands back the hash it showed; the backend refuses if the content is no
// longer that. Trusting is never the default action.
// ---------------------------------------------------------------------

const trustUi = {
  box: document.getElementById("trustbox"),
  path: document.getElementById("trust-path"),
  facts: document.getElementById("trust-facts"),
  state: document.getElementById("trust-state"),
  predicates: document.getElementById("trust-predicates"),
  seen: document.getElementById("trust-seen"),
  cancel: document.getElementById("trust-cancel"),
  go: document.getElementById("trust-go"),
};

/** What the open dialog showed: `{ id, path, sha256 }`, or null. */
let trustShown = null;

function syncTrustGo() {
  trustUi.go.disabled = !trustShown || !trustUi.seen.checked;
}

async function openTrust(id, ruleset) {
  trustShown = null;
  trustUi.path.textContent = ruleset.trust?.real ?? ruleset.path;
  trustUi.facts.textContent = t("rules.trust.reading");
  trustUi.state.textContent = "";
  trustUi.predicates.replaceChildren();
  trustUi.seen.checked = false;
  trustUi.seen.disabled = true;
  syncTrustGo();
  trustUi.box.showModal();
  let shown;
  try {
    shown = await invoke("rules_trust_preview", { id, path: ruleset.path });
  } catch (e) {
    trustUi.facts.textContent = "";
    trustUi.state.textContent = errorText(e);
    return;
  }
  if (!trustUi.box.open) return;
  trustUi.path.textContent = shown.path;
  trustUi.facts.textContent = fill(t("rules.trust.facts"), {
    files: shown.files,
    size: formatBytes(shown.bytes),
    n: shown.predicates.length,
    hash: shown.sha256.slice(0, 12),
  });
  if (shown.state === "changed") trustUi.state.textContent = t("rules.trust.changedNote");
  if (!shown.predicates.length) {
    trustUi.state.textContent = t("rules.trust.none");
    return;
  }
  for (const rule of shown.predicates) {
    const li = node("li");
    const head = node("div", "trust-rule");
    head.append(node("span", "rules-id", rule.id), node("span", "rules-text", titleOf(rule)));
    const show = node("button", "addbox-secondary", t("rules.trust.show"));
    show.type = "button";
    show.setAttribute("aria-expanded", "false");
    const code = node("pre", "trust-code");
    code.hidden = true;
    show.addEventListener("click", async () => {
      if (!code.hidden) {
        code.hidden = true;
        show.textContent = t("rules.trust.show");
        show.setAttribute("aria-expanded", "false");
        return;
      }
      if (!code.dataset.read) {
        try {
          const src = await invoke("rules_predicate_source", { id, rule: rule.id });
          code.textContent = src.text + (src.truncated ? "\n" + t("rules.trust.truncated") : "");
        } catch (e) {
          code.textContent = errorText(e);
        }
        code.dataset.read = "1";
      }
      code.hidden = false;
      show.textContent = t("rules.trust.hide");
      show.setAttribute("aria-expanded", "true");
    });
    head.append(show);
    li.append(head, code);
    trustUi.predicates.append(li);
  }
  trustShown = { id, path: ruleset.path, sha256: shown.sha256 };
  trustUi.seen.disabled = false;
  syncTrustGo();
}

trustUi.seen.addEventListener("change", syncTrustGo);
trustUi.cancel.addEventListener("click", () => trustUi.box.close());
trustUi.box.addEventListener("close", () => {
  trustShown = null;
});
trustUi.go.addEventListener("click", async () => {
  if (!trustShown || !trustUi.seen.checked) return;
  const { id, path, sha256 } = trustShown;
  trustUi.go.disabled = true;
  try {
    await invoke("rules_trust_grant", { id, path, sha256 });
  } catch (e) {
    // The content changed under the dialog, or the store cannot be written:
    // nothing was trusted, and the dialog stays to say so.
    trustUi.state.textContent = errorText(e);
    trustShown = null;
    trustUi.seen.checked = false;
    syncTrustGo();
    return;
  }
  trustUi.box.close();
  renderRules(true);
});

// ---------------------------------------------------------------------
// Agent runs over the manual rules.
//
// A run is the backend's (`agent_*`): it plans, asks ai-desktop, checks
// every quote against the tree and stores proposals in the project. This is
// its dialog and its view. Two things are fixed here on purpose:
//
// - **Nothing is asked of a model unless the person presses Start in the
//   dialog**, and the dialog says before that what would be sent and where.
//   Opening the dialog asks the engine for the plan and ai-desktop for its
//   health; neither sends a file.
// - **The view decides nothing by itself.** A proposal is shown as a
//   proposal, a model's "compliant" as "no violation found" with what it
//   covered. Only a person turns one into a verdict or a waiver, one at a
//   time, with the buttons in the detail of its row (see "deciding about one
//   proposal" below).
// ---------------------------------------------------------------------

const runUi = {
  agent: document.getElementById("rules-agent"),
  runs: document.getElementById("rules-runs"),
  view: document.getElementById("rules-run"),
  back: document.getElementById("rules-run-back"),
  title: document.getElementById("rules-run-title"),
  state: document.getElementById("rules-run-state"),
  meta: document.getElementById("rules-run-meta"),
  error: document.getElementById("rules-run-error"),
  pause: document.getElementById("rules-run-pause"),
  resume: document.getElementById("rules-run-resume"),
  cancel: document.getElementById("rules-run-cancel"),
  queue: document.getElementById("rules-run-queue"),
  detail: document.getElementById("rules-run-detail"),
  box: document.getElementById("runbox"),
  fTitle: document.getElementById("run-title"),
  fNotes: document.getElementById("run-notes"),
  fModel: document.getElementById("run-model"),
  models: document.getElementById("run-models"),
  service: document.getElementById("run-service"),
  refused: document.getElementById("run-refused"),
  previewState: document.getElementById("run-preview-state"),
  batches: document.getElementById("run-batches"),
  batchRows: document.getElementById("run-batch-rows"),
  skippedBox: document.getElementById("run-skipped-box"),
  skipped: document.getElementById("run-skipped"),
  ignore: document.getElementById("run-ignore"),
  egress: document.getElementById("run-egress"),
  dontStart: document.getElementById("run-cancel"),
  go: document.getElementById("run-go"),
};

/** The shape that goes with a row's state in the queue. */
const RUN_ROW_SHAPE = { queued: "·", asking: "…", proposed: "◇", unclear: "?", error: "!", notSent: "–" };
const RUN_MODEL_KEY = "docmap.rules.model";

/** The agent button of the selection bar: how many manual rules a run would be about. */
function drawAgentButton(id) {
  const st = rulesOf(id);
  const { manual } = splitSelection(rulesList(st.catalog?.rules), st.selected);
  runUi.agent.disabled = manual.length === 0;
  runUi.agent.textContent = manual.length ? plural("rules.agent", manual.length) : t("rules.agent.none");
}

/** The runs of the project, read from its records. Cheap: no engine, no network. */
async function loadRuns(id) {
  const st = rulesOf(id);
  try {
    st.runs = await invoke("agent_list", { id });
  } catch (e) {
    st.runs = [];
    void e;
  }
  if (id === view.selectedId && rulesOpen) drawRuns(id);
}

function runStateWord(state) {
  return t("rules.run.state." + state);
}

function runDestination(run) {
  return run.local
    ? fill(t("rules.run.dest.local"), { model: run.model })
    : fill(t("rules.run.dest.cloud"), { backend: backendName(run.backend), model: run.model });
}

function drawRuns(id) {
  const st = rulesOf(id);
  const runs = rulesList(st.runs);
  runUi.runs.replaceChildren();
  runUi.runs.hidden = runs.length === 0 || !!st.openRun;
  if (!runs.length) return;
  runUi.runs.append(node("p", "rules-runs-head", fill(t("rules.runs"), { n: runs.length })));
  const SHOWN = 6;
  for (const run of runs.slice(0, SHOWN)) {
    const row = node("div", "rules-runs-row " + run.state);
    const done = runProgress(run);
    row.append(
      node("span", "rules-run-state " + run.state, runStateWord(run.state)),
      node("span", "rules-runs-title", run.title),
      node(
        "span",
        "rules-trust-by",
        fill(t("rules.runs.meta"), {
          done: done.done,
          total: done.total,
          proposals: run.proposals,
          dest: runDestination(run),
          date: new Date(run.createdMs).toLocaleDateString(locale),
        })
      )
    );
    const open = node("button", "addbox-secondary", t("rules.runs.open"));
    open.type = "button";
    open.dataset.fk = "run:" + run.id;
    open.addEventListener("click", () => openRun(id, run.id));
    row.append(open);
    runUi.runs.append(row);
  }
  if (runs.length > SHOWN) {
    runUi.runs.append(node("p", "rules-note", fill(t("rules.more"), { n: runs.length - SHOWN })));
  }
}

/** The list or one run: only one of the two is on screen. */
function syncRunView(id) {
  const st = rulesOf(id);
  const open = !!st.openRun && !!st.run;
  runUi.view.hidden = !open;
  if (!open) return;
  // One run fills the pane: the list, and the notes about rulesets that sit
  // above it, make room (at the minimum window height they took half of it).
  rulesUi.body.hidden = true;
  rulesUi.trust.hidden = true;
  rulesUi.problems.hidden = true;
  drawRunView(id);
  drawChat(id);
}

async function openRun(id, runId) {
  const st = rulesOf(id);
  st.openRun = runId;
  st.run = null;
  st.runRule = null;
  // asked again on every opening: the rules or the tree may have changed in between
  st.runFresh = null;
  await refreshRun(id);
  if (id === view.selectedId && rulesOpen) drawRules(id);
}

/** Read the open run again; while it is running, again in a moment. */
async function refreshRun(id) {
  const st = rulesOf(id);
  const runId = st.openRun;
  if (!runId) return;
  clearTimeout(st.runTimer);
  try {
    const run = await invoke("agent_status", { id, runId });
    if (st.openRun !== runId) return;
    st.run = run;
    st.runError = null;
  } catch (e) {
    if (st.openRun !== runId) return;
    st.runError = errorText(e);
    if (!st.run) {
      st.openRun = null;
      say(st.runError);
      return;
    }
  }
  if (id === view.selectedId && rulesOpen) syncRunView(id);
  // Only while somebody is looking: a run goes on without this, and the list
  // of runs says so the next time the tab is opened.
  if (st.run && isRunLive(st.run.state) && id === view.selectedId && rulesOpen) {
    st.runTimer = setTimeout(() => {
      refreshRun(id).then(() => {
        if (st.run && !isRunLive(st.run.state)) loadRuns(id);
      });
    }, 1500);
  } else if (st.run) {
    void checkFresh(id);
  }
}

/**
 * Whether the proposals of the open run still match the rules and the tree
 * (`agent_freshness`: the engine plans the run's selection again, no network).
 * Asked once a run stands still, and again when it has more proposals than at
 * the last look. Until the answer is there, no proposal can be accepted.
 */
async function checkFresh(id) {
  const st = rulesOf(id);
  const runId = st.openRun;
  const count = rulesList(st.run?.proposals).length;
  const known = st.runFresh;
  if (!runId || !count || (known && known.runId === runId && known.count === count)) return;
  st.runFresh = { runId, count, map: null, error: null };
  try {
    const map = await invoke("agent_freshness", { id, runId });
    if (st.runFresh?.runId === runId) st.runFresh.map = map;
  } catch (e) {
    if (st.runFresh?.runId === runId) st.runFresh.error = errorText(e);
  }
  if (st.openRun === runId && st.run && id === view.selectedId && rulesOpen) drawRunView(id);
}

function closeRun(id) {
  const st = rulesOf(id);
  clearTimeout(st.runTimer);
  st.openRun = null;
  st.run = null;
  st.chatKey = null;
  chatUi.box.hidden = true;
  runUi.view.hidden = true;
  drawRules(id);
  loadRuns(id);
}

function drawRunView(id) {
  const st = rulesOf(id);
  const run = st.run;
  runUi.title.textContent = run.title;
  runUi.state.className = "rules-run-state " + run.state;
  runUi.state.textContent = runStateWord(run.state);
  const done = runProgress(run);
  const used = tokensUsed(run);
  const nf = new Intl.NumberFormat(locale);
  runUi.meta.textContent = fill(t("rules.run.meta"), {
    dest: runDestination(run),
    done: done.done,
    total: done.total,
    tokens: nf.format(used.total),
    prompt: nf.format(used.prompt),
    completion: nf.format(used.completion),
  });
  const problem = st.runError || (run.error ? errorText(run.error) : "");
  runUi.error.hidden = !problem;
  runUi.error.textContent = problem;
  const can = runActions(run.state);
  runUi.pause.hidden = !can.pause;
  runUi.resume.hidden = !can.resume;
  runUi.cancel.hidden = !can.cancel;

  const fresh = st.runFresh && st.runFresh.runId === run.id ? st.runFresh.map : null;
  const rows = queueRows(run, fresh);
  if (!st.runRule || !rows.some((r) => r.id === st.runRule)) st.runRule = rows[0]?.id ?? null;
  const rules = new Map(rulesList(st.catalog?.rules).map((r) => [r.id, r]));
  const held = runUi.queue.contains(document.activeElement) ? document.activeElement.dataset.fk : undefined;
  runUi.queue.replaceChildren();
  for (const row of rows) {
    const li = node("li");
    const pick = node("button", "rules-run-row " + row.state + (row.id === st.runRule ? " on" : ""));
    pick.type = "button";
    pick.dataset.fk = "runrow:" + row.id;
    pick.setAttribute("aria-current", String(row.id === st.runRule));
    const rule = rules.get(row.id);
    pick.append(
      node("span", "rules-run-shape", RUN_ROW_SHAPE[row.state] ?? "·"),
      node("span", "rules-id", row.id),
      node("span", "rules-run-word", t("rules.run.row." + row.state)),
      // What the model proposed, in its own slot: never blended with the state.
      node("span", "rules-run-says", row.status && row.state !== "error" ? t(proposalKey(row.status)) : ""),
      // And what the check of the evidence did with it, in a third.
      node("span", "rules-run-evidence", row.evidence ? runEvidence(row) : ""),
      // And what a person made of it, or that it is outdated, on a line of its own.
      node("span", "rules-run-mark", runMark(row))
    );
    if (rule) pick.title = titleOf(rule);
    pick.addEventListener("click", () => {
      if (st.runRule !== row.id && st.decide) {
        // what was typed about one proposal is not carried to the next
        Object.assign(st.decide, { note: "", reason: "", expires: "", paths: "", waiving: null });
      }
      st.runRule = row.id;
      drawRunView(id);
    });
    li.append(pick);
    runUi.queue.append(li);
  }
  if (held) {
    const again = [...runUi.queue.querySelectorAll("[data-fk]")].find((el) => el.dataset.fk === held);
    if (again) again.focus({ preventScroll: true });
  }
  // The detail is rebuilt too: a field or a button of the decision keeps the keyboard.
  const inDetail = runUi.detail.contains(document.activeElement) ? document.activeElement.dataset.fk : undefined;
  drawRunDetail(id, rows.find((r) => r.id === st.runRule) ?? null, rules.get(st.runRule) ?? null);
  if (inDetail) {
    const again = [...runUi.detail.querySelectorAll("[data-fk]")].find((el) => el.dataset.fk === inDetail);
    if (again && !again.disabled) again.focus({ preventScroll: true });
  }
}

/** A row's decision in a word, else whether it is outdated; nothing for an open, current one. */
function runMark(row) {
  const decided = decisionKey(row.decision);
  if (decided) return `${DECIDED_SHAPE[row.decision.kind]} ${t(decided + ".short")}`;
  const stale = freshKey(row.fresh);
  return stale ? `! ${t(stale + ".short")}` : "";
}

/** The evidence check in a few words: what held, what was dropped, what was covered. */
function runEvidence(row) {
  const e = row.evidence;
  if (row.status === "violation") {
    return fill(t("rules.run.evidence.findings"), { held: e.held, dropped: e.dropped });
  }
  if (row.status === "compliant") {
    return fill(t("rules.run.evidence.files"), { checked: e.checked, unchecked: e.unchecked });
  }
  return e.dropped ? fill(t("rules.run.evidence.dropped"), { dropped: e.dropped }) : "";
}

function drawRunDetail(id, row, rule) {
  const st = rulesOf(id);
  const box = runUi.detail;
  box.replaceChildren();
  if (!row) {
    box.append(node("p", "rules-note", t("rules.run.empty")));
    return;
  }
  const head = node("p", "rules-run-detail-head");
  head.append(node("span", "rules-id", row.id), node("span", "rules-text", rule ? titleOf(rule) : ""));
  box.append(head);
  if (row.state !== "notSent") {
    const talk = node("button", "addbox-secondary rules-chat-open", fill(t("rules.chat.rule"), { rule: row.id }));
    talk.type = "button";
    talk.dataset.fk = "chat:" + row.id;
    talk.addEventListener("click", () => openChat(id, row.id));
    box.append(talk);
  }
  if (rule && rule.text) box.append(node("p", "rules-rule-text", rule.text));

  const rec = rulesList(st.run.proposals).find((p) => p.ruleId === row.id);
  if (!rec) {
    const why = row.state === "notSent" ? fill(t("rules.blocked"), { reason: row.reason ?? "" }) : t("rules.run.row." + row.state);
    box.append(node("p", "rules-note", why));
    if (row.error) box.append(node("p", "rules-blocked", errorText(row.error)));
    return;
  }
  const p = rec.proposal && typeof rec.proposal === "object" ? rec.proposal : {};
  const says = node("p", "rules-run-proposal");
  // `proposed` in words on every proposal: this is an agent's answer, not a verdict.
  says.append(
    node("span", "rules-run-tag", t("rules.run.proposed")),
    node("span", "rules-run-verdict", t(proposalKey(rec.status)))
  );
  box.append(says);
  const stale = freshKey(row.fresh);
  if (stale && !row.decision) box.append(node("p", "rules-run-stale", `! ${t(stale)}`));
  drawDecide(id, row, box);
  if (rec.status === "compliant") {
    box.append(
      node(
        "p",
        "rules-note",
        fill(t("rules.run.covered"), {
          checked: rulesList(p.checked).length,
          unchecked: rulesList(p.unchecked).length,
        })
      )
    );
  }
  if (rec.error) box.append(node("p", "rules-blocked", errorText(rec.error)));
  if (p.reason) box.append(node("p", "rules-run-reason", String(p.reason)));

  const findings = rulesList(p.findings);
  if (findings.length) {
    box.append(node("h4", null, fill(t("rules.run.findings"), { n: findings.length })));
    const ul = node("ul", "rules-run-findings");
    for (const f of findings) {
      const li = node("li");
      const where = node("button", "rules-finding-file", `${f.file}:${f.line}`);
      where.type = "button";
      where.title = t("rules.run.openFile");
      where.addEventListener("click", () => {
        invoke("open_in_editor", { id, path: f.file, line: f.line ?? null }).catch((err) => say(errorText(err)));
      });
      li.append(where, node("code", "rules-run-quote", f.quote ?? ""));
      if (f.why) li.append(node("span", "rules-finding-text", f.why));
      ul.append(li);
    }
    box.append(ul);
  }
  const dropped = rulesList(p.dropped);
  if (dropped.length) {
    box.append(node("h4", null, fill(t("rules.run.dropped"), { n: dropped.length })));
    const ul = node("ul", "rules-run-findings dropped");
    for (const d of dropped.slice(0, 20)) {
      const f = d.finding && typeof d.finding === "object" ? d.finding : {};
      const li = node("li");
      li.append(
        node("span", "rules-source", `${f.file ?? "?"}${f.line ? ":" + f.line : ""}`),
        node("span", "rules-finding-text", String(d.reason ?? ""))
      );
      ul.append(li);
    }
    box.append(ul);
  }
  if (typeof p.raw === "string" && p.raw) {
    box.append(node("h4", null, t("rules.run.raw")));
    box.append(node("pre", "trust-code", p.raw.length > 4000 ? p.raw.slice(0, 4000) + "\n…" : p.raw));
  }
  if (row.fromCache) box.append(node("p", "rules-note", t("rules.run.fromCache")));
  if (rec.source === "chat") box.append(node("p", "rules-note", t("rules.run.fromChat")));
}

// ---- deciding about one proposal ------------------------------------
//
// Accept, Waive… and Reject sit in the detail of one row and act on that row
// only: there is no "accept all", and no list of checkboxes that could become
// one. Accept and Waive are written by the engine (`rules/accept`,
// `rules/waive` into `.rules-verdicts.json` and `.rules-waivers.json`), Reject
// writes nothing outside the run. A refusal of the engine stays in the detail,
// and the proposal stays open.

/** The shape that goes with a decision, so it is never colour alone. */
const DECIDED_SHAPE = { accepted: "●", waived: "–", rejected: "○" };

/** The form of the decision being prepared, per project: who decides, and the fields of a waiver. */
function decideForm(st) {
  if (!st.decide) st.decide = { by: null, note: "", reason: "", expires: "", paths: "", waiving: null, busy: false, error: null, errorRule: null };
  return st.decide;
}

/** The name to offer for "decided by", asked once per project; the person can change it. */
async function loadPerson(id) {
  const f = decideForm(rulesOf(id));
  if (f.by !== null) return;
  f.by = "";
  try {
    f.by = (await invoke("agent_person", { id })) || "";
  } catch (e) {
    void e;
  }
  const st = rulesOf(id);
  if (st.run && id === view.selectedId && rulesOpen) drawRunView(id);
}

function decideField(label, input, fk) {
  const wrap = node("label", "rules-decide-field");
  input.dataset.fk = fk;
  wrap.append(node("span", null, label), input);
  return wrap;
}

function textInput(value, onInput, type = "text") {
  const input = document.createElement("input");
  input.type = type;
  input.value = value ?? "";
  input.addEventListener("input", () => onInput(input.value));
  return input;
}

function drawDecide(id, row, box) {
  const st = rulesOf(id);
  const f = decideForm(st);
  if (row.decision) {
    const d = row.decision;
    const date = d.atMs ? new Date(d.atMs).toLocaleDateString(locale) : "";
    let text = fill(t(decisionKey(d)), { by: d.by, date, verdict: d.verdict ? t("rules.judged.verdict." + d.verdict) : "" });
    if (d.kind === "waived") {
      text += " " + (d.expires ? fill(t("rules.run.decided.until"), { reason: d.reason ?? "", date: d.expires }) : fill(t("rules.run.decided.for"), { reason: d.reason ?? "" }));
    }
    if (d.note) text += " " + fill(t("rules.run.decided.note"), { note: d.note });
    box.append(node("p", "rules-run-decided", `${DECIDED_SHAPE[d.kind] ?? ""} ${text}`));
    return;
  }
  const offer = decideOffer(row);
  if (!offer) return;
  if (f.by === null) void loadPerson(id);

  const wrap = node("div", "rules-decide");
  wrap.append(node("p", "rules-decide-lead", t("rules.run.decide.lead")));
  const fields = node("div", "rules-decide-fields");
  fields.append(
    decideField(t("rules.run.decide.by"), textInput(f.by ?? "", (v) => (f.by = v)), "decide:by"),
    decideField(t("rules.run.decide.note"), textInput(f.note, (v) => (f.note = v)), "decide:note")
  );
  wrap.append(fields);

  const bar = node("div", "rules-decide-bar");
  const accept = node("button", "addbox-secondary rules-decide-go", t("rules.run.decide.accept"));
  accept.type = "button";
  accept.dataset.fk = "decide:accept";
  accept.disabled = f.busy || !offer.accept.ok;
  accept.addEventListener("click", () =>
    decideCall(id, row.id, (args) =>
      invoke("agent_accept", { ...args, by: f.by ?? "", note: f.note.trim() || null })
    )
  );
  const waive = node("button", "addbox-secondary", t("rules.run.decide.waive"));
  waive.type = "button";
  waive.dataset.fk = "decide:waive";
  waive.disabled = f.busy;
  waive.setAttribute("aria-expanded", String(f.waiving === row.id));
  waive.addEventListener("click", () => {
    f.waiving = f.waiving === row.id ? null : row.id;
    drawRunView(id);
  });
  const reject = node("button", "addbox-secondary", t("rules.run.decide.reject"));
  reject.type = "button";
  reject.dataset.fk = "decide:reject";
  reject.disabled = f.busy;
  reject.addEventListener("click", () =>
    decideCall(id, row.id, (args) => invoke("agent_reject", { ...args, by: f.by ?? "" }))
  );
  bar.append(accept, waive, reject);
  wrap.append(bar);
  // Why Accept is off, in words next to it rather than in a tooltip alone.
  if (!offer.accept.ok) {
    const why = offer.accept.why === "rules.run.accept.checking" && st.runFresh?.error
      ? fill(t("rules.run.accept.unknown"), { error: st.runFresh.error })
      : t(offer.accept.why);
    accept.title = why;
    // an outdated proposal says so above, once is enough
    if (!freshKey(row.fresh)) wrap.append(node("p", "rules-note rules-decide-why", why));
  }

  if (f.waiving === row.id) {
    const form = node("div", "rules-decide-waive");
    form.append(node("p", "rules-decide-lead", t("rules.run.waive.lead")));
    const paths = document.createElement("textarea");
    paths.rows = 2;
    paths.value = f.paths;
    paths.addEventListener("input", () => (f.paths = paths.value));
    form.append(
      decideField(t("rules.run.waive.reason"), textInput(f.reason, (v) => (f.reason = v)), "waive:reason"),
      decideField(t("rules.run.waive.expires"), textInput(f.expires, (v) => (f.expires = v), "date"), "waive:expires"),
      decideField(t("rules.run.waive.paths"), paths, "waive:paths")
    );
    const go = node("button", "addbox-secondary rules-decide-go", t("rules.run.waive.go"));
    go.type = "button";
    go.dataset.fk = "waive:go";
    go.disabled = f.busy;
    go.addEventListener("click", () =>
      decideCall(id, row.id, (args) =>
        invoke("agent_waive", {
          ...args,
          args: { by: f.by ?? "", reason: f.reason, expires: f.expires || null, paths: waiverPaths(f.paths) },
        })
      )
    );
    const cancel = node("button", "addbox-secondary", t("rules.run.waive.cancel"));
    cancel.type = "button";
    cancel.addEventListener("click", () => {
      f.waiving = null;
      drawRunView(id);
    });
    const goBar = node("div", "rules-decide-bar");
    goBar.append(go, cancel);
    form.append(goBar);
    wrap.append(form);
  }
  // The engine's refusal, or a field it would not take: here, where the decision was asked for.
  if (f.error && f.errorRule === row.id) wrap.append(node("p", "rules-blocked", f.error));
  box.append(wrap);
}

/**
 * One decision about the proposal `rule` of the open run. `call` is the
 * command itself, written out at its button. The answer is the run with the
 * decision in it; the project's verdicts and waivers are read again for the
 * Rules tab and the sidebar.
 */
async function decideCall(id, rule, call) {
  const st = rulesOf(id);
  const f = decideForm(st);
  const runId = st.openRun;
  if (!runId || f.busy) return;
  f.busy = true;
  f.error = null;
  f.errorRule = null;
  drawRunView(id);
  try {
    const run = await call({ id, runId, rule });
    if (st.openRun === runId) st.run = run;
    f.waiving = null;
    f.note = "";
    f.reason = "";
    f.expires = "";
    f.paths = "";
    await loadJudged(id);
    renderRulesCount();
    void render();
  } catch (e) {
    f.error = errorText(e);
    f.errorRule = rule;
  } finally {
    f.busy = false;
  }
  if (st.openRun === runId && st.run && id === view.selectedId && rulesOpen) drawRunView(id);
}

/**
 * Pause, resume or cancel the open run. `call` is the command itself, written
 * out at the button, so the check that every invoked command is registered
 * (and the web variant's list of what it refuses) sees all three by name.
 */
async function runCommand(call) {
  const id = view.selectedId;
  if (!id) return;
  const st = rulesOf(id);
  const runId = st.openRun;
  if (!runId) return;
  for (const b of [runUi.pause, runUi.resume, runUi.cancel]) b.disabled = true;
  try {
    const run = await call({ id, runId });
    if (st.openRun === runId) st.run = run;
    st.runError = null;
  } catch (e) {
    st.runError = errorText(e);
  } finally {
    for (const b of [runUi.pause, runUi.resume, runUi.cancel]) b.disabled = false;
  }
  await refreshRun(id);
  loadRuns(id);
}

runUi.pause.addEventListener("click", () => runCommand((args) => invoke("agent_pause", args)));
runUi.resume.addEventListener("click", () => runCommand((args) => invoke("agent_resume", args)));
runUi.cancel.addEventListener("click", () => runCommand((args) => invoke("agent_cancel", args)));
runUi.back.addEventListener("click", () => {
  if (view.selectedId) closeRun(view.selectedId);
});

// ---- the chat about one rule or the whole run -----------------------
//
// A conversation with the model in the run view (`agent/chat.rs`). Opened by
// a button only, never by itself; nothing is sent until the person presses
// Send, and the lines above the input say what a message sends and where
// (the preview: the engine plans, nothing goes out). It cannot open files: a
// person attaches one, and the engine's deny list decides whether it goes. It
// decides nothing: Make this a proposal sends one answer through the check of
// every run answer, and what comes out waits for a person like any proposal.

const chatUi = {
  box: document.getElementById("rules-chat"),
  title: document.getElementById("rules-chat-title"),
  close: document.getElementById("rules-chat-close"),
  dest: document.getElementById("rules-chat-dest"),
  egress: document.getElementById("rules-chat-egress"),
  trimmed: document.getElementById("rules-chat-trimmed"),
  log: document.getElementById("rules-chat-log"),
  files: document.getElementById("rules-chat-files"),
  text: document.getElementById("rules-chat-text"),
  error: document.getElementById("rules-chat-error"),
  send: document.getElementById("rules-chat-send"),
  stop: document.getElementById("rules-chat-stop"),
  attach: document.getElementById("rules-chat-attach"),
  run: document.getElementById("rules-run-chat"),
};

let chatCount = 0;

/** The open conversation of the project's open run, or null. */
function openChatOf(st) {
  if (!st.chatKey || !st.chats) return null;
  const c = st.chats.get(st.chatKey);
  return c && c.runId === st.openRun ? c : null;
}

/** Open the conversation about `rule` (null: the whole run) of the open run. Sends nothing. */
function openChat(id, rule) {
  const st = rulesOf(id);
  if (!st.openRun) return;
  if (!st.chats) st.chats = new Map();
  // the draft of the conversation that was open stays with it
  const before = openChatOf(st);
  if (before) before.draft = chatUi.text.value;
  const key = chatKey(st.openRun, rule);
  if (!st.chats.has(key)) st.chats.set(key, newChat(st.openRun, rule, ++chatCount));
  st.chatKey = key;
  const c = st.chats.get(key);
  chatUi.text.value = c.draft;
  drawChat(id);
  chatUi.box.scrollIntoView({ block: "nearest" });
  chatUi.text.focus({ preventScroll: true });
  void refreshChatPreview(id);
}

function closeChat(id) {
  const st = rulesOf(id);
  const c = openChatOf(st);
  if (c) c.draft = chatUi.text.value;
  st.chatKey = null;
  chatUi.box.hidden = true;
}

/** What the next message would send and where: the engine plans, nothing goes out. */
async function refreshChatPreview(id) {
  const st = rulesOf(id);
  const c = openChatOf(st);
  if (!c || c.previewing) return;
  c.previewing = true;
  c.previewError = null;
  drawChat(id);
  try {
    c.preview = await invoke("agent_chat_preview", { id, args: chatArgs(c) });
    c.dropped = c.preview.dropped;
  } catch (e) {
    c.previewError = errorText(e);
  } finally {
    c.previewing = false;
  }
  if (openChatOf(st) === c) drawChat(id);
}

/** The where-it-goes line and the egress line, from the last preview and the draft. */
function drawChatEgress(c) {
  const p = c.preview;
  if (!p) {
    chatUi.dest.textContent = c.previewError ? "" : t("rules.chat.preview.loading");
    chatUi.egress.textContent = "";
    return;
  }
  if (p.refused) {
    chatUi.dest.textContent = errorText(p.refused);
    chatUi.egress.textContent = "";
    return;
  }
  const facts = egressFacts(p, chatUi.text.value.length);
  chatUi.dest.textContent = facts.local
    ? fill(t("rules.chat.dest.local"), { model: facts.model })
    : fill(t("rules.chat.dest.cloud"), { backend: backendName(facts.backend), model: facts.model });
  const nf = new Intl.NumberFormat(locale);
  chatUi.egress.textContent = fill(t(c.rule ? "rules.chat.egress.rule" : "rules.chat.egress.run"), {
    rule: facts.rule,
    files: nf.format(facts.files),
    attached: nf.format(facts.attached),
    messages: nf.format(facts.messages),
    tokens: nf.format(facts.tokens),
  });
}

function drawChat(id) {
  const st = rulesOf(id);
  const c = openChatOf(st);
  if (!c) {
    chatUi.box.hidden = true;
    return;
  }
  chatUi.box.hidden = false;
  chatUi.title.textContent = c.rule ? fill(t("rules.chat.title.rule"), { rule: c.rule }) : t("rules.chat.title.run");
  chatUi.text.placeholder = t(c.rule ? "rules.chat.placeholder" : "rules.chat.placeholder.run");
  drawChatEgress(c);
  // Trimmed is said every time it is so, never left to be noticed.
  chatUi.trimmed.hidden = !c.dropped;
  chatUi.trimmed.textContent = c.dropped
    ? fill(t("rules.chat.trimmed"), {
        n: c.dropped,
        limit: new Intl.NumberFormat(locale).format(c.preview?.limitChars ?? 0),
      })
    : "";
  drawChatLog(id, c);
  drawChatFiles(id, c);
  const problem = c.error || c.previewError;
  chatUi.error.hidden = !problem;
  chatUi.error.textContent = problem ?? "";
  const refused = Boolean(c.preview?.refused);
  chatUi.send.disabled = c.busy || refused || !c.preview;
  chatUi.stop.hidden = !c.busy;
  chatUi.attach.disabled = c.busy || c.files.length >= 5;
}

function drawChatLog(id, c) {
  const nearEnd = chatUi.log.scrollHeight - chatUi.log.scrollTop - chatUi.log.clientHeight < 40;
  chatUi.log.replaceChildren();
  if (!c.messages.length) chatUi.log.append(node("li", "rules-note", t("rules.chat.empty")));
  c.messages.forEach((m, i) => {
    const li = node("li", "rules-chat-msg " + m.role);
    li.append(
      node("span", "rules-chat-who", t(m.role === "user" ? "rules.chat.you" : "rules.chat.model")),
      node("p", "rules-chat-text", m.text)
    );
    if (m.role === "assistant") {
      if (m.stopped) li.append(node("p", "rules-note", t("rules.chat.stopped")));
      if (m.failed) li.append(node("p", "rules-blocked", fill(t("rules.chat.failed"), { error: m.failed })));
      if (!m.partial && m.tokens !== undefined) {
        li.append(node("p", "rules-note", fill(t("rules.chat.answerTokens"), { tokens: new Intl.NumberFormat(locale).format(m.tokens) })));
      }
      const bar = node("div", "rules-decide-bar");
      // The model can only ask; the file is added by a person, with a click.
      for (const file of m.wants ?? []) {
        if (c.files.includes(file)) continue;
        const want = node("button", "addbox-secondary", fill(t("rules.chat.attachWanted"), { file }));
        want.type = "button";
        want.disabled = c.busy || c.files.length >= 5;
        want.addEventListener("click", () => attachChatFile(id, file));
        bar.append(want);
      }
      if (canPropose(c, m)) {
        const go = node("button", "addbox-secondary rules-decide-go", t("rules.chat.propose"));
        go.type = "button";
        go.title = fill(t("rules.chat.propose.why"), { rule: c.rule });
        go.disabled = c.busy || c.proposing;
        go.addEventListener("click", () => proposeFromChat(id, i));
        bar.append(go);
      }
      if (bar.childElementCount) li.append(bar);
      if (m.note) li.append(node("p", "rules-note", m.note));
    }
    chatUi.log.append(li);
  });
  if (nearEnd) chatUi.log.scrollTop = chatUi.log.scrollHeight;
}

function drawChatFiles(id, c) {
  chatUi.files.hidden = c.files.length === 0;
  chatUi.files.replaceChildren();
  if (!c.files.length) return;
  chatUi.files.append(node("li", "rules-chat-who", t("rules.chat.files")));
  const judged = new Map((c.preview?.attached ?? []).map((a) => [a.file, a]));
  for (const file of c.files) {
    const a = judged.get(file);
    const line = a ? attachedLine(a) : { key: "rules.chat.file.pending", vars: { file } };
    const li = node("li");
    li.append(node("span", a && !a.sent ? "rules-blocked" : null, fill(t(line.key), line.vars)));
    const remove = node("button", "addbox-secondary", t("rules.chat.file.remove"));
    remove.type = "button";
    remove.disabled = c.busy;
    remove.addEventListener("click", () => {
      c.files = c.files.filter((f) => f !== file);
      drawChat(id);
      void refreshChatPreview(id);
    });
    li.append(remove);
    chatUi.files.append(li);
  }
}

/** Attach a file: `file` relative to the project, or one picked in the file dialog. */
async function attachChatFile(id, file) {
  const st = rulesOf(id);
  const c = openChatOf(st);
  if (!c || c.busy) return;
  c.error = null;
  if (!file) {
    const root = view.projects.find((p) => p.id === id)?.root ?? "";
    const picked = await open({ multiple: false, directory: false, title: t("rules.chat.attachTitle"), defaultPath: root || undefined });
    if (!picked) return;
    file = relativeTo(root, picked);
    if (!file) {
      c.error = fill(t("rules.chat.outside"), { file: String(picked) });
      drawChat(id);
      return;
    }
  }
  addFile(c, file);
  drawChat(id);
  void refreshChatPreview(id);
}

/** Send the person's message: the one call of the chat that reaches a model. */
async function sendChat(id) {
  const st = rulesOf(id);
  const c = openChatOf(st);
  if (!c || c.busy) return;
  const text = chatUi.text.value;
  if (!text.trim()) return;
  c.busy = true;
  c.error = null;
  const args = chatArgs(c, text);
  const asked = { role: "user", text };
  const answer = { role: "assistant", text: "", partial: true };
  c.messages.push(asked, answer);
  chatUi.text.value = "";
  c.draft = "";
  let sent = false;
  const events = new Channel();
  events.onmessage = (ev) => {
    if (ev.kind === "sending") {
      sent = true;
      c.preview = ev.sent;
      c.dropped = ev.sent.dropped;
    } else if (ev.kind === "delta") {
      answer.text += ev.text;
    }
    if (openChatOf(st) === c) drawChat(id);
  };
  drawChat(id);
  try {
    const reply = await invoke("agent_chat_send", { id, chat: c.chatId, args, onEvent: events });
    Object.assign(answer, { text: reply.text, partial: false, tokens: reply.answerTokens, wants: reply.wants });
    c.preview = reply.sent;
    c.dropped = reply.sent.dropped;
  } catch (e) {
    if (!sent) {
      // Nothing went out: the message is the person's again, as it was.
      c.messages.splice(c.messages.indexOf(asked), 2);
      if (!chatUi.text.value) chatUi.text.value = text;
      c.error = errorText(e);
    } else if (e && e.key === "err.agent.chatStopped") {
      answer.stopped = true;
    } else {
      answer.failed = errorText(e);
    }
  } finally {
    c.busy = false;
  }
  if (openChatOf(st) === c) drawChat(id);
  // What the turn sent is known; what the next one would send is asked again (the answer is part of it now).
  if (sent) void refreshChatPreview(id);
}

/** Make answer `index` the proposal of the open run for the conversation's rule. Decides nothing. */
async function proposeFromChat(id, index) {
  const st = rulesOf(id);
  const c = openChatOf(st);
  const m = c?.messages[index];
  if (!c || !m || !canPropose(c, m) || c.proposing) return;
  c.proposing = true;
  c.error = null;
  drawChat(id);
  try {
    const run = await invoke("agent_chat_propose", { id, runId: c.runId, rule: c.rule, text: m.text });
    if (st.openRun === c.runId) st.run = run;
    const rec = rulesList(run.proposals).find((p) => p.ruleId === c.rule);
    m.note = fill(t("rules.chat.proposed"), { rule: c.rule, status: rec ? t(proposalKey(rec.status)) : "" });
    // the new proposal is asked about again before it can be accepted
    st.runFresh = null;
    st.runRule = c.rule;
  } catch (e) {
    c.error = errorText(e);
  } finally {
    c.proposing = false;
  }
  if (st.openRun === c.runId && st.run && id === view.selectedId && rulesOpen) {
    drawRunView(id);
    void checkFresh(id);
  }
  if (openChatOf(st) === c) drawChat(id);
}

chatUi.run.addEventListener("click", () => {
  if (view.selectedId) openChat(view.selectedId, null);
});
chatUi.close.addEventListener("click", () => {
  if (view.selectedId) closeChat(view.selectedId);
});
chatUi.send.addEventListener("click", () => {
  if (view.selectedId) void sendChat(view.selectedId);
});
chatUi.stop.addEventListener("click", () => {
  const id = view.selectedId;
  const c = id ? openChatOf(rulesOf(id)) : null;
  if (c?.busy) invoke("agent_chat_cancel", { chat: c.chatId }).catch((e) => say(errorText(e)));
});
chatUi.attach.addEventListener("click", () => {
  if (view.selectedId) void attachChatFile(view.selectedId, null);
});
chatUi.text.addEventListener("input", () => {
  const id = view.selectedId;
  const c = id ? openChatOf(rulesOf(id)) : null;
  if (c) drawChatEgress(c);
});
chatUi.text.addEventListener("keydown", (e) => {
  // Ctrl+Enter sends; Enter alone is a new line of the message.
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && view.selectedId) {
    e.preventDefault();
    void sendChat(view.selectedId);
  }
});

// ---- the dialog ----------------------------------------------------

/** The open dialog: `{ id, ids, preview, previewError, health, healthError, loading, seq }`. */
let runForm = null;

function drawRunDialog() {
  const f = runForm;
  if (!f) return;
  // ai-desktop: where it is, and which cloud backends it has a key for.
  if (f.healthError) {
    runUi.service.textContent = f.healthError;
  } else if (f.health) {
    const have = Object.entries(f.health.backends || {})
      .filter(([, on]) => on)
      .map(([name]) => backendName(name));
    runUi.service.textContent = fill(t("rules.run.service"), {
      url: f.health.url,
      backends: have.length ? have.join(", ") : t("rules.run.service.none"),
    });
  } else {
    runUi.service.textContent = t("rules.run.service.asking");
  }

  const p = f.preview;
  runUi.models.replaceChildren();
  for (const name of rulesList(p?.models)) {
    const option = document.createElement("option");
    option.value = name;
    runUi.models.append(option);
  }
  const blocker = startBlocker({
    title: runUi.fTitle.value,
    model: runUi.fModel.value,
    preview: p,
    health: f.health,
    healthError: !!f.healthError,
    loading: f.loading,
  });
  const refused = p && p.refused ? errorText(p.refused) : "";
  // The project's refusal in its own words where there is one, else why Start is off.
  const why = refused && runUi.fModel.value.trim() ? refused : blocker && blocker !== "rules.run.block.loading" ? t(blocker) : "";
  runUi.refused.hidden = !why;
  runUi.refused.textContent = why;

  runUi.previewState.textContent = f.loading ? t("rules.run.reading") : (f.previewError ?? "");
  const batches = rulesList(p?.batches);
  runUi.batches.hidden = f.loading || batches.length === 0;
  runUi.batchRows.replaceChildren();
  const nf = new Intl.NumberFormat(locale);
  for (const b of batches) {
    const tr = document.createElement("tr");
    tr.append(
      node("td", null, b.id),
      node("td", "run-batch-rules", rulesList(b.rules).map((r) => r.id).join(", ")),
      node("td", "n", nf.format(rulesList(b.files).length)),
      node("td", "n", formatBytes(b.bytes ?? 0)),
      node("td", "n", nf.format(b.tokens ?? 0))
    );
    tr.title = rulesList(b.files).join("\n");
    runUi.batchRows.append(tr);
  }
  const skipped = f.loading ? [] : rulesList(p?.skipped);
  runUi.skippedBox.hidden = skipped.length === 0;
  runUi.skipped.replaceChildren();
  for (const s of skipped) {
    const li = node("li");
    li.append(node("span", "rules-id", s.id ?? ""), node("span", "rules-finding-text", String(s.reason ?? s.status ?? "")));
    runUi.skipped.append(li);
  }

  // The proposals are written into the project; say so when git would pick them up.
  const ig = p?.ignore;
  runUi.ignore.replaceChildren();
  runUi.ignore.hidden = !(ig && ig.git && !ig.ignored) || f.loading;
  if (!runUi.ignore.hidden) {
    runUi.ignore.append(node("span", null, fill(t("rules.run.ignore"), { entry: ig.entry })));
    const add = node("button", "addbox-secondary", t("rules.run.ignore.add"));
    add.type = "button";
    add.addEventListener("click", async () => {
      add.disabled = true;
      try {
        const state = await invoke("agent_ignore", { id: f.id, add: true });
        if (f.preview) f.preview.ignore = state;
      } catch (e) {
        say(errorText(e));
      }
      drawRunDialog();
    });
    runUi.ignore.append(add);
  }

  const out = f.loading ? null : egress(p);
  if (out && out.requests) {
    const facts = { files: nf.format(out.files), size: formatBytes(out.bytes), model: out.model, backend: backendName(out.backend) };
    runUi.egress.textContent = out.local ? fill(t("rules.run.egress.local"), facts) : fill(t("rules.run.egress.cloud"), facts);
    runUi.egress.classList.toggle("leaves", !out.local);
    runUi.go.textContent = out.local ? t("rules.run.go.local") : fill(t("rules.run.go.cloud"), facts);
  } else {
    runUi.egress.textContent = t("rules.run.egress.none");
    runUi.egress.classList.remove("leaves");
    runUi.go.textContent = t("rules.run.go");
  }
  runUi.go.disabled = blocker !== null;
}

/** Ask the engine what a run with this model would send. Sends nothing. */
async function refreshRunPreview() {
  const f = runForm;
  if (!f) return;
  const seq = ++f.seq;
  f.loading = true;
  f.previewError = null;
  drawRunDialog();
  const selection = f.ids.join(",");
  try {
    if (selection.length > MAX_SELECTION_BYTES) throw new Error(t("rules.run.tooMany"));
    const preview = await invoke("agent_preview", { id: f.id, selection, model: runUi.fModel.value.trim() });
    if (runForm !== f || seq !== f.seq) return;
    f.preview = preview;
  } catch (e) {
    if (runForm !== f || seq !== f.seq) return;
    f.preview = null;
    f.previewError = errorText(e);
  }
  f.loading = false;
  drawRunDialog();
}

function openRunDialog(id) {
  const st = rulesOf(id);
  const { manual } = splitSelection(rulesList(st.catalog?.rules), st.selected);
  if (!manual.length) return;
  runForm = { id, ids: manual, preview: null, previewError: null, health: null, healthError: null, loading: true, seq: 0 };
  const form = runForm;
  runUi.fTitle.value = defaultRunTitle(manual, new Date().toISOString().slice(0, 10));
  runUi.fNotes.value = "";
  let model = "";
  try {
    model = localStorage.getItem(RUN_MODEL_KEY) || "";
  } catch (e) {
    void e;
  }
  runUi.fModel.value = model;
  runUi.box.showModal();
  drawRunDialog();
  invoke("agent_health")
    .then((h) => {
      form.health = h;
    })
    .catch((e) => {
      form.healthError = errorText(e);
    })
    .finally(() => {
      if (runForm === form) drawRunDialog();
    });
  refreshRunPreview();
}

runUi.agent.addEventListener("click", () => {
  if (view.selectedId) openRunDialog(view.selectedId);
});
runUi.fTitle.addEventListener("input", drawRunDialog);
runUi.fModel.addEventListener("change", refreshRunPreview);
runUi.dontStart.addEventListener("click", () => runUi.box.close());
runUi.box.addEventListener("close", () => {
  runForm = null;
});
runUi.go.addEventListener("click", async () => {
  const f = runForm;
  if (!f || runUi.go.disabled) return;
  const model = runUi.fModel.value.trim();
  runUi.go.disabled = true;
  let run;
  try {
    run = await invoke("agent_start", {
      id: f.id,
      args: {
        title: runUi.fTitle.value.trim(),
        notes: runUi.fNotes.value,
        selection: f.ids.join(","),
        model,
        refresh: false,
      },
    });
  } catch (e) {
    // Nothing was started: the dialog stays and says why.
    runUi.refused.hidden = false;
    runUi.refused.textContent = errorText(e);
    runUi.go.disabled = false;
    return;
  }
  try {
    localStorage.setItem(RUN_MODEL_KEY, model);
  } catch (e) {
    void e;
  }
  runUi.box.close();
  loadRuns(f.id);
  openRun(f.id, run.id);
});

// =====================================================================
// Project statistics
//
// What the project is made of, counted when asked for — it opens every
// file, which is not something to do for thirty projects at start-up. The
// numbers come from the disk, not from the map: the map describes modules,
// and "how much of this is documentation" is a question about files.
// =====================================================================

/** Per project id, so switching back does not recount a tree. */
const statsCache = new Map();
/** Counts in flight, per project id. */
const statsPending = new Map();

const statsUi = {
  title: document.getElementById("stats-title"),
  state: document.getElementById("stats-state"),
  body: document.getElementById("stats-body"),
  tiles: document.getElementById("stats-tiles"),
  bar: document.getElementById("stats-bar"),
  legend: document.getElementById("stats-legend"),
  rows: document.getElementById("stats-rows"),
  largest: document.getElementById("stats-largest"),
  foot: document.getElementById("stats-foot"),
  refresh: document.getElementById("stats-refresh"),
};

function node(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined && text !== null) e.textContent = text;
  return e;
}

/** The button belongs to the project on screen: disabled only while *its*
    count is running, not while some other project's is. */
function syncStatsRefresh() {
  statsUi.refresh.disabled = !!view.selectedId && statsPending.has(view.selectedId);
}

async function renderStats(force = false) {
  const id = view.selectedId;
  if (!statsOpen || !id) return;
  const p = view.projects.find((x) => x.id === id);
  statsUi.title.textContent = p ? fill(t("stats.titleFor"), { name: p.name }) : t("stats.title");

  let s = force ? null : statsCache.get(id);
  if (!s) {
    statsUi.body.hidden = true;
    statsUi.state.textContent = t("stats.counting");
    // One count per project at a time: toggling the pane, switching away and
    // back, or pressing "Count again" while it runs would each start another
    // full walk of the same tree, with no way to cancel any of them.
    let call = statsPending.get(id);
    if (!call) {
      call = invoke("project_stats", { id }).finally(() => statsPending.delete(id));
      statsPending.set(id, call);
    }
    syncStatsRefresh();
    try {
      s = await call;
    } catch (e) {
      if (id === view.selectedId) statsUi.state.textContent = errorText(e);
      return;
    } finally {
      syncStatsRefresh();
    }
    statsCache.set(id, s);
  }
  syncStatsRefresh();
  // The count can finish after the reader moved on or left the pane.
  if (id !== view.selectedId || !statsOpen) return;

  const nf = new Intl.NumberFormat(locale);
  const h = statsHeadline(s);
  statsUi.state.textContent = t("stats.note");

  const tile = (value, label, sub) => {
    const el = node("div", "stat-tile");
    el.append(node("div", "v", nf.format(value)), node("div", "l", label));
    if (sub) el.append(node("div", "s", sub));
    return el;
  };
  statsUi.tiles.replaceChildren(
    tile(h.files, t("stats.tile.files"), formatBytes(s.bytes)),
    tile(h.lines, t("stats.tile.lines")),
    tile(h.code, t("stats.tile.code"), fill(t("stats.sub.files"), { n: nf.format(h.codeFiles) })),
    tile(
      h.comment,
      t("stats.tile.comments"),
      fill(t("stats.sub.ratio"), { p: nf.format(h.commentRatio) })
    ),
    tile(h.docs, t("stats.tile.docs"), fill(t("stats.sub.files"), { n: nf.format(h.docsFiles) })),
    tile(h.data, t("stats.tile.data"), fill(t("stats.sub.files"), { n: nf.format(h.dataFiles) })),
    tile(h.blank, t("stats.tile.blank"))
  );

  const parts = statsComposition(s);
  statsUi.bar.replaceChildren(
    ...parts.map((part) => {
      const seg = node("span", "seg-" + part.key);
      seg.style.width = part.percent + "%";
      seg.title = `${t("stats.tile." + legendKey(part.key))}: ${nf.format(part.lines)}`;
      return seg;
    })
  );
  statsUi.legend.replaceChildren(
    ...parts.map((part) => {
      const li = node("li");
      const dot = node("i", "seg-" + part.key);
      li.append(dot, `${t("stats.tile." + legendKey(part.key))} ${nf.format(part.percent)} %`);
      return li;
    })
  );

  statsUi.rows.replaceChildren(
    ...statsLanguageRows(s).map((l) => {
      const tr = node("tr");
      const name = node("td", null, l.name);
      name.append(node("span", "kind", t("stats.kind." + l.kind)));
      const meter = node("span", "meter");
      meter.style.width = Math.max(l.width, 1) + "%";
      meter.title = nf.format(l.share) + " %";
      const barCell = node("td", "bar");
      barCell.append(meter);
      tr.append(
        name,
        node("td", "n", nf.format(l.files)),
        node("td", "n", nf.format(l.lines.total)),
        node("td", "n", nf.format(l.lines.code)),
        node("td", "n", l.kind === "code" ? nf.format(l.lines.comment) : "–"),
        node("td", "n", nf.format(l.lines.blank)),
        barCell
      );
      return tr;
    })
  );

  statsUi.largest.replaceChildren(
    ...s.largest.map((f) => {
      const li = node("li", "traffic-path-open");
      li.title = t("traffic.detail.openFile");
      li.append(
        node("span", "ov-deps-name", f.path),
        node("span", "ov-deps-who", fill(t("stats.lines"), { n: nf.format(f.lines) }))
      );
      makeOpenable(li, () => {
        invoke("open_in_editor", { id, path: f.path, line: null }).catch((err) => say(errorText(err)));
      });
      return li;
    })
  );

  const bits = [];
  if (h.other) bits.push(fill(t("stats.foot.other"), { n: nf.format(h.other) }));
  if (s.truncated) bits.push(t("stats.foot.truncated"));
  statsUi.foot.textContent = bits.join(" ");
  statsUi.body.hidden = false;
}

function legendKey(key) {
  return { code: "code", comment: "comments", docs: "docs", data: "data", blank: "blank" }[key];
}

statsUi.refresh.addEventListener("click", () => {
  statsCache.delete(view.selectedId);
  renderStats(true);
});


// =====================================================================
// Search
//
// One box, three questions — text in files (a grep), file names (a find),
// and what the map shows. Which one is the *scope*: a folder of the project,
// or the view. The folder search is answered by the backend walking the disk;
// the view search by reading the map's own data, because the page itself is a
// separate document this window cannot read into.
//
// Opening a result goes where the result lives: a file match opens in the
// editor at its line, a map match sends the map there.
// =====================================================================

const FINDER_KEY = "docmap.finder";

const finder = {
  root: document.getElementById("finder"),
  input: document.getElementById("finder-input"),
  panel: document.getElementById("finder-panel"),
  path: document.getElementById("finder-path"),
  pick: document.getElementById("finder-pick"),
  scope: document.getElementById("finder-scope"),
  modes: document.getElementById("finder-modes"),
  modeText: document.getElementById("finder-mode-text"),
  modeFiles: document.getElementById("finder-mode-files"),
  caseBox: document.getElementById("finder-case"),
  state: document.getElementById("finder-state"),
  results: document.getElementById("finder-results"),
};

/** Folder to search, relative to the project root. */
let finderSub = "";
let finderMode = "text";
let finderTimer = null;
let finderSeq = 0;

try {
  const saved = JSON.parse(localStorage.getItem(FINDER_KEY) || "{}");
  if (saved.mode === "files") finderMode = "files";
  if (saved.scope === "view") finder.scope.value = "view";
  finder.caseBox.checked = !!saved.matchCase;
} catch (e) {
  void e;
}

function saveFinder() {
  try {
    localStorage.setItem(
      FINDER_KEY,
      JSON.stringify({
        mode: finderMode,
        scope: finder.scope.value,
        matchCase: finder.caseBox.checked,
      })
    );
  } catch (e) {
    void e;
  }
}

function finderProject() {
  return view.projects.find((p) => p.id === view.selectedId) || null;
}

function openFinder() {
  finder.panel.hidden = false;
}

function closeFinder() {
  finder.panel.hidden = true;
}

/** A different project means a different folder, and last project's results
    would be the wrong project's files. */
function resetFinder() {
  finderSeq++;
  clearTimeout(finderTimer);
  finderSub = "";
  finder.input.value = "";
  finder.results.replaceChildren();
  finder.state.textContent = "";
  closeFinder();
  syncFinder();
}

/** Bring every control in line with the state variables. */
function syncFinder() {
  const view = finder.scope.value === "view";
  const p = finderProject();
  // Folder-only controls are meaningless for the view scope: it has no
  // folder, and "text or file names" does not apply to a map's data.
  finder.path.parentElement.querySelectorAll(".finder-label, .finder-path, .finder-btn").forEach((el) => {
    el.hidden = view;
  });
  // Neither a mode nor a case rule applies to the view: it is searched by
  // name and by words, in the map's own data.
  finder.modes.hidden = view;
  finder.modeText.classList.toggle("on", finderMode === "text");
  finder.modeText.setAttribute("aria-pressed", String(finderMode === "text"));
  finder.modeFiles.classList.toggle("on", finderMode === "files");
  finder.modeFiles.setAttribute("aria-pressed", String(finderMode === "files"));
  finder.path.classList.remove("bad");
  if (p) finder.path.value = scopeDisplay(p.root, finderSub);
  finder.input.placeholder = t(
    view ? "find.placeholder.view" : finderMode === "files" ? "find.placeholder.files" : "find.placeholder"
  );
}

function setFinderState(text) {
  finder.state.textContent = text || "";
}

/** A result row: a button, so it is a tab stop and answers Enter. */
function hitButton() {
  const li = document.createElement("li");
  const b = document.createElement("button");
  b.type = "button";
  b.className = "finder-hit";
  li.append(b);
  return { li, b };
}

function renderFolderHits(res, query) {
  const nf = new Intl.NumberFormat(locale);
  finder.results.replaceChildren();
  const hits = res.hits || [];
  if (!hits.length) {
    setFinderState(t(res.truncated ? truncatedKey(res.reason) : "find.none"));
    return;
  }
  const files = fileCount(hits);
  let line =
    finderMode === "files"
      ? fill(t("find.count.files"), { n: nf.format(hits.length) })
      : fill(t("find.count.text"), { n: nf.format(hits.length), files: nf.format(files) });
  if (res.truncated) line += " " + t(truncatedKey(res.reason));
  // The per-file cap is silent otherwise: "N matches" would pass for all of
  // them.
  if (res.cappedFiles > 0) line += " " + t("find.cut.perfile");
  setFinderState(line);

  const len = Array.from(query).length;
  for (const h of hits) {
    const { li, b } = hitButton();
    const where = document.createElement("span");
    where.className = "where";
    const p = document.createElement("span");
    p.className = "p";
    p.textContent = h.path;
    where.append(p);
    if (h.line) {
      const n = document.createElement("span");
      n.textContent = ":" + h.line;
      where.append(n);
    }
    b.append(where);
    if (h.text) {
      const what = document.createElement("span");
      what.className = "what";
      const parts = splitMatch(h.text, h.at, len);
      what.append(parts.before);
      if (parts.match) {
        const m = document.createElement("mark");
        m.textContent = parts.match;
        what.append(m);
      }
      what.append(parts.after);
      b.append(what);
    }
    b.addEventListener("click", () => {
      invoke("open_in_editor", { id: view.selectedId, path: h.path, line: h.line ?? null }).catch((e) =>
        say(errorText(e))
      );
    });
    finder.results.append(li);
  }
}

function renderViewHits(res) {
  const nf = new Intl.NumberFormat(locale);
  finder.results.replaceChildren();
  if (!res.available) {
    setFinderState(t("find.view.nomap"));
    return;
  }
  const hits = res.hits || [];
  if (!hits.length) {
    setFinderState(t("find.none"));
    return;
  }
  setFinderState(
    fill(t("find.count.view"), { n: nf.format(hits.length) }) + (res.truncated ? " " + t("find.cut.limit") : "")
  );

  for (const h of hits) {
    const { li, b } = hitButton();
    const where = document.createElement("span");
    where.className = "where";
    const kind = document.createElement("span");
    kind.className = "finder-kind";
    kind.textContent = t("find.kind." + h.kind) || h.kind;
    const label = document.createElement("span");
    label.className = "p";
    label.textContent = h.label;
    where.append(kind, label);
    if (h.file) {
      // A sibling button, not a span inside the row's button: a control
      // nested in a control is not reachable from the keyboard. The row's own
      // action is the map; this one is the file.
      const open = document.createElement("button");
      open.type = "button";
      open.className = "open-file";
      open.textContent = t("find.openFile");
      open.title = h.file;
      li.classList.add("finder-hit-row");
      open.addEventListener("click", () => {
        invoke("open_in_editor", { id: view.selectedId, path: h.file, line: h.line ?? null }).catch((e) =>
          say(errorText(e))
        );
      });
      li.append(open);
    }
    const what = document.createElement("span");
    what.className = "what";
    what.textContent = h.detail;
    b.append(where, what);

    b.addEventListener("click", () => {
      const target = mapTarget(h);
      if (target) {
        // The map has to be what is showing for it to go anywhere.
        if (filesOpen || statsOpen || rulesOpen) setPane("map");
        gotoMap(target);
        closeFinder();
      } else if (h.file) {
        invoke("open_in_editor", { id: view.selectedId, path: h.file, line: h.line ?? null }).catch((e) =>
          say(errorText(e))
        );
      }
    });
    finder.results.append(li);
  }
}

/** One search at a time: a pause longer than the debounce while typing used
    to start a second full walk of the tree for an answer nobody would see. */
let finderBusy = false;
let finderDirty = false;

async function runFinder() {
  if (finderBusy) {
    finderDirty = true;
    return;
  }
  finderBusy = true;
  try {
    do {
      finderDirty = false;
      await runFinderOnce();
    } while (finderDirty);
  } finally {
    finderBusy = false;
  }
}

async function runFinderOnce() {
  const id = view.selectedId;
  const query = finder.input.value.trim();
  const seq = ++finderSeq;
  if (!id || !query) {
    finder.results.replaceChildren();
    setFinderState("");
    return;
  }
  setFinderState(t("find.searching"));
  try {
    if (finder.scope.value === "view") {
      const res = await invoke("view_search", { id, query });
      if (seq === finderSeq) renderViewHits(res);
    } else {
      const res = await invoke("project_search", {
        id,
        sub: finderSub,
        query,
        mode: finderMode,
        caseSensitive: finder.caseBox.checked,
      });
      if (seq === finderSeq) renderFolderHits(res, query);
    }
  } catch (e) {
    // A stale answer to a question nobody is asking any more stays quiet.
    if (seq === finderSeq) {
      finder.results.replaceChildren();
      setFinderState(errorText(e));
    }
  }
}

/** Run after a pause in typing, so each keystroke is not a walk of the tree. */
function scheduleFinder() {
  clearTimeout(finderTimer);
  finderTimer = setTimeout(runFinder, 260);
}

/** Take whatever is in the scope box as the folder, or say why it is not one. */
function applyScopeText() {
  const p = finderProject();
  if (!p) return;
  const r = scopeToSub(p.root, finder.path.value);
  if (r.error) {
    finder.path.classList.add("bad");
    setFinderState(t(r.error === "outside" ? "find.scope.outside" : "find.scope.dotdot"));
    return;
  }
  finderSub = r.sub;
  syncFinder();
  if (finder.input.value.trim()) runFinder();
}

finder.input.addEventListener("focus", openFinder);
finder.input.addEventListener("click", openFinder);
finder.input.addEventListener("input", () => {
  openFinder();
  scheduleFinder();
});
// The native clear button of a search box fires `search`, not `input`.
finder.input.addEventListener("search", scheduleFinder);
finder.input.addEventListener("keydown", (ev) => {
  if (ev.key === "Enter") {
    ev.preventDefault();
    // The Enter that confirms an IME conversion is not a search.
    if (imeComposing(ev)) return;
    clearTimeout(finderTimer);
    runFinder();
  } else if (ev.key === "ArrowDown") {
    const first = finder.results.querySelector(".finder-hit");
    if (first) {
      ev.preventDefault();
      first.focus();
    }
  } else if (ev.key === "Escape") {
    // Without this a search box also clears itself on Escape, natively,
    // leaving the results of a query that is no longer there.
    ev.preventDefault();
    closeFinder();
    finder.input.blur();
  }
});

// Escape from anywhere in the panel (the folder, the scope, the toggles) puts
// it away, not only from the box.
finder.root.addEventListener("keydown", (ev) => {
  if (ev.key !== "Escape" || ev.target === finder.input || finder.panel.hidden) return;
  closeFinder();
  if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
});

// Arrow keys walk the result list; Escape goes back to the box.
finder.results.addEventListener("keydown", (ev) => {
  const hits = [...finder.results.querySelectorAll(".finder-hit")];
  const i = hits.indexOf(document.activeElement);
  if (i < 0) return;
  if (ev.key === "ArrowDown") {
    ev.preventDefault();
    (hits[i + 1] || hits[i]).focus();
  } else if (ev.key === "ArrowUp") {
    ev.preventDefault();
    (i === 0 ? finder.input : hits[i - 1]).focus();
  }
});

finder.path.addEventListener("change", applyScopeText);
onEnterSubmit(finder.path, applyScopeText);

finder.pick.addEventListener("click", async () => {
  const p = finderProject();
  if (!p) return;
  try {
    // Starts in the project, not wherever the OS last was: a scope outside
    // it is refused anyway, so the dialog opens where the answer is.
    const dir = await open({
      directory: true,
      multiple: false,
      defaultPath: scopeDisplay(p.root, finderSub),
      title: t("find.pick.title"),
    });
    if (!dir) return;
    finder.path.value = dir;
    applyScopeText();
  } catch (e) {
    say(errorText(e));
  }
});

finder.scope.addEventListener("change", () => {
  syncFinder();
  saveFinder();
  if (finder.input.value.trim()) runFinder();
  else {
    finder.results.replaceChildren();
    setFinderState("");
  }
});

for (const [btn, mode] of [
  [finder.modeText, "text"],
  [finder.modeFiles, "files"],
]) {
  btn.addEventListener("click", () => {
    finderMode = mode;
    syncFinder();
    saveFinder();
    if (finder.input.value.trim()) runFinder();
    finder.input.focus();
  });
}

finder.caseBox.addEventListener("change", () => {
  saveFinder();
  if (finder.input.value.trim()) runFinder();
});

// Clicking anywhere else puts the panel away. `mousedown` rather than
// `click`, so it closes before whatever was clicked reacts.
document.addEventListener("mousedown", (ev) => {
  if (!finder.panel.hidden && !finder.root.contains(ev.target)) closeFinder();
});

// A click inside the map never reaches this document (it is a cross-origin
// frame), but the window loses focus to it: that is the signal. The folder
// dialog also takes focus, which is why only the frame counts.
window.addEventListener("blur", () => {
  if (!finder.panel.hidden && document.activeElement === els.frame) closeFinder();
});

// Ctrl+K from anywhere in this window. Not from inside the map: key events do
// not cross out of a cross-origin frame, so there it simply does not fire.
document.addEventListener("keydown", (ev) => {
  if ((ev.ctrlKey || ev.metaKey) && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "k") {
    if (!view.selectedId) return;
    ev.preventDefault();
    finder.input.focus();
    finder.input.select();
  }
});

syncFinder();

// =====================================================================
// Workspaces
//
// A workspace is a set of projects and nothing else. Theme, language, zoom
// and the engine paths stay properties of this machine — carrying them per
// workspace would change somebody's lighting when they switch project sets,
// which is not what switching project sets is for.
//
// **When the dashboard appears** is a decision rather than a setting:
// whenever there is more than one workspace. Somebody with a single
// workspace never sees it, which is what a "don't show again" checkbox buys
// — without a setting to find. The checkbox exists anyway, for the reader
// who has several and still wants to land in the last one.
//
// An earlier version of this comment also claimed "on a first run", which
// the code below never did and should not: a first run has exactly one
// workspace, and a chooser with one row is the click in front of the thing
// you wanted that the rule above exists to avoid. Corrected here rather
// than implemented, because the comment was the part that was wrong.
// =====================================================================

const WS_SKIP_KEY = "docmap.skipWorkspaceDashboard";

/**
 * The workspace the app is in, and how many exist.
 *
 * Kept here rather than asked for at each use: the window title needs both
 * on every project change, and a title is not worth a round trip.
 */
let activeWorkspace = null;
let workspaceCount = 1;

const ws = {
  box: document.getElementById("wsbox"),
  list: document.getElementById("ws-list"),
  name: document.getElementById("ws-new"),
  create: document.getElementById("ws-create"),
  problem: document.getElementById("ws-problem"),
  skip: document.getElementById("ws-skip"),
};

function wsSkipped() {
  try {
    return localStorage.getItem(WS_SKIP_KEY) === "1";
  } catch (e) {
    void e;
    return false;
  }
}

async function renderWorkspaces() {
  let entries;
  try {
    entries = await invoke("list_workspaces");
  } catch (e) {
    ws.problem.textContent = errorText(e);
    ws.problem.hidden = false;
    return [];
  }
  activeWorkspace = (entries.find((w) => w.active) || {}).name || null;
  migrateLastKey(localStorage, activeWorkspace);
  workspaceCount = entries.length;
  ws.list.replaceChildren();
  entries.forEach((w) => {
    const li = document.createElement("li");
    li.className = "ws-row" + (w.active ? " active" : "");

    const open = document.createElement("button");
    open.type = "button";
    open.className = "ws-open";
    const n = document.createElement("span");
    n.className = "n";
    n.textContent = w.name;
    const c = document.createElement("span");
    c.className = "c";
    c.textContent = fill(t("ws.count"), { n: w.projects });
    open.append(n, c);
    open.addEventListener("click", () => useWorkspace(w.name));
    li.append(open);

    const ren = document.createElement("button");
    ren.type = "button";
    ren.className = "ws-act";
    ren.textContent = t("ws.rename");
    ren.addEventListener("click", async () => {
      const to = prompt(t("ws.rename.prompt"), w.name);
      if (!to || to === w.name) return;
      try {
        await invoke("rename_workspace", { from: w.name, to });
        await renderWorkspaces();
      } catch (e) {
        ws.problem.textContent = errorText(e);
        ws.problem.hidden = false;
      }
    });

    const del = document.createElement("button");
    del.type = "button";
    del.className = "ws-act";
    del.textContent = t("ws.delete");
    del.addEventListener("click", async () => {
      // The wording has to survive being misread: this removes a list, never
      // a repository — the same promise "Remove from workspace" makes, one
      // scale up.
      if (!confirm(fill(t("ws.delete.confirm"), { name: w.name }))) return;
      try {
        await invoke("delete_workspace", { name: w.name });
        await renderWorkspaces();
      } catch (e) {
        ws.problem.textContent = errorText(e);
        ws.problem.hidden = false;
      }
    });

    li.append(ren, del);
    ws.list.append(li);
  });
  return entries;
}

/** Switch to a workspace and close the dashboard. */
async function useWorkspace(name) {
  ws.problem.hidden = true;
  try {
    const list = await invoke("switch_workspace", { name });
    activeWorkspace = name;
    // Nothing from the old workspace may survive the switch: a selection
    // pointing at a project this workspace does not contain would leave the
    // sidebar naming one thing and the map showing another.
    dropSelection();
    freshness.clear();
    statsCache.clear();
    showPlaceholder(t("ph.none.title"), t("ph.none.body"));
    await refresh(list);
    // Re-listed because switching can *create*: the count in the title
    // would otherwise be one behind the workspace you are now in.
    await renderWorkspaces();
    titleFor(null);
    ws.box.close();
    // Land where this workspace was left, now that its own selection is
    // stored under its own key. After `close()`, so the restore paints
    // behind a dialog that is already gone rather than under one.
    await restoreLast();
  } catch (e) {
    ws.problem.textContent = errorText(e);
    ws.problem.hidden = false;
  }
}

ws.create.addEventListener("click", () => {
  const name = ws.name.value.trim();
  if (!name) return;
  ws.name.value = "";
  useWorkspace(name);
});

/**
 * Store "start with the workspace overview", from either control.
 *
 * The dashboard's own checkbox is phrased as the negative ("do not show
 * this again") because that is what somebody looking at the dashboard wants
 * to say; Settings phrases it as the positive, because a list of
 * preferences reads better when every line is a thing that is on. One
 * stored value, inverted at one of the two ends — see `setStartDashboard`'s
 * single caller pair below.
 *
 * @param {boolean} show
 */
function setStartDashboard(show) {
  try {
    localStorage.setItem(WS_SKIP_KEY, show ? "0" : "1");
  } catch (e) {
    void e;
  }
  ws.skip.checked = !show;
  const box = document.getElementById("prefs-dashboard");
  if (box) box.checked = show;
}

ws.skip.addEventListener("change", () => setStartDashboard(!ws.skip.checked));

async function openWorkspaces() {
  ws.problem.hidden = true;
  ws.skip.checked = wsSkipped();
  await renderWorkspaces();
  ws.box.showModal();
}

/**
 * Show the dashboard at startup, or do not.
 *
 * Returns whether it was shown, so the caller knows whether a project list
 * is already on screen or is about to be chosen.
 */
async function maybeOpenWorkspaces() {
  // Listed even when the dashboard is skipped: the window title needs to
  // know which workspace this is and how many there are, and skipping the
  // *dashboard* is not skipping the fact.
  const entries = await renderWorkspaces();
  if (wsSkipped()) return false;
  // One workspace is not a choice, and a chooser with one row is a click in
  // front of the thing you wanted.
  if (entries.length < 2) return false;
  ws.skip.checked = false;
  ws.box.showModal();
  return true;
}

// =====================================================================
// The menu bar
//
// `src-tauri/src/menu.rs` owns the structure — which items exist, their
// order, their accelerators. This owns the strings, because they are
// translations and the catalog next door is already the one place those
// live, with a spec that fails when a locale is short a key.
//
// Every item calls the same function its sidebar button calls. Two ways to
// reach one implementation, never two implementations: `MENUBAR.md`'s
// last "what I would not build" entry is exactly this, and it is the reason
// the sidebar keeps one button rather than a mirror of the menu.
// =====================================================================

/** Item id → what it does. Keys must match `menu.rs`; `menu.test.js` checks. */
const MENU_ACTIONS = {
  "menu.file.add": () => els.add.click(),
  "menu.file.open_browser": async () => {
    if (!view.selectedId) return;
    try {
      await invoke("open_map_in_browser", { id: view.selectedId });
    } catch (e) {
      say(errorText(e));
    }
  },
  "menu.file.export": () => exportCurrentView(),
  "menu.file.reveal": async () => {
    if (!view.selectedId) return;
    try {
      await invoke("reveal_project", { id: view.selectedId });
    } catch (e) {
      say(errorText(e));
    }
  },
  "menu.file.copy_path": async () => {
    const p = view.projects.find((x) => x.id === view.selectedId);
    if (!p) return;
    try {
      // The webview's own clipboard rather than a plugin: the text is a
      // path this process already handed the page, so nothing new crosses
      // the boundary and there is no second capability to grant.
      await navigator.clipboard.writeText(p.root);
      say(fill(t("copy.path.done"), { path: p.root }));
    } catch (e) {
      say(errorText(e));
    }
  },
  "menu.file.remove": () => view.selectedId && removeProject(view.selectedId),
  "menu.file.workspaces": () => openWorkspaces(),
  "menu.file.settings": () => openPrefs(),
  "menu.project.scope": () => view.selectedId && openScope(),
  "menu.project.generate": () => view.selectedId && generateFor(view.selectedId),
  "menu.project.generate_all": () => generateAll(),
  "menu.project.generate_stale": () => generateStale(),
  "menu.project.check": () => view.selectedId && checkMap(view.selectedId),
  "menu.project.generate_full": () => view.selectedId && generateFor(view.selectedId, true),
  // `generateFor` already reloads the map on success; the extra `select()`
  // that used to follow it ran after a *failure* too, and put the old map
  // back over the failure message.
  "menu.project.regenerate": () => view.selectedId && generateFor(view.selectedId),
  "menu.view.theme.system": () => chooseTheme("system"),
  "menu.view.theme.light": () => chooseTheme("light"),
  "menu.view.theme.dark": () => chooseTheme("dark"),
  "menu.view.zoom_in": async () => {
    await applyZoom(zoomStep(1));
    syncMenu();
  },
  "menu.view.zoom_out": async () => {
    await applyZoom(zoomStep(-1));
    syncMenu();
  },
  "menu.view.zoom_reset": async () => {
    await applyZoom(1);
    syncMenu();
  },
  "menu.view.files": () => setFiles(!filesOpen),
  "menu.view.stats": () => setStats(!statsOpen),
  "menu.view.rules": () => setPane(rulesOpen ? "map" : "rules"),
  "menu.view.sidebar": () => {
    applySidebar(!sidebarShown);
    syncMenu();
  },
  "menu.view.sidebar_auto": () => {
    applyPinned(!sidebarPinned);
    syncMenu();
  },
  "menu.help.feedback": () => openFeedback(),
  "menu.help.settings_folder": async () => {
    try {
      await invoke("reveal_settings");
    } catch (e) {
      say(errorText(e));
    }
  },
  "menu.help.about": () => openAbout(),
  "menu.help.usage": () => openDocs("usage"),
  "menu.help.engine": () => openDocs("engine"),
  "menu.help.docs_search": () => openDocSearch(),
  "menu.help.repo": () => openDocs("repo"),
  "menu.help.license": () => openDocs("license"),
};

/**
 * Name the window after the selected project.
 *
 * The heading in the sidebar is the application; the title bar is the
 * subject. Before this they both read `docmap`, one directly above the
 * other.
 *
 * The project name is never translated and never escaped away — it is the
 * subject, and the interface is the only thing the catalog speaks for.
 */
function titleFor(project) {
  // The workspace is named only when there is more than one, which is the
  // only time the answer is news. `<project> — <workspace> — docmap` is a
  // long title to carry for somebody who will never have a second one.
  const parts = [];
  if (project) parts.push(project.name);
  if (workspaceCount > 1 && activeWorkspace) parts.push(activeWorkspace);
  parts.push("docmap");
  invoke("set_window_title", { title: parts.join(" — ") }).catch((e) => void e);
}

/** Must match `menu::LOCALE_ID`. */
const LOCALE_ITEM = "menu.view.lang:";

/**
 * Change the theme from either surface.
 *
 * The sidebar select is updated too, rather than left showing the old
 * value: two controls for one setting is already a compromise, and two
 * controls disagreeing about it is a bug.
 */
function chooseTheme(choice) {
  applyTheme(choice);
  try {
    localStorage.setItem(THEME_KEY, choice);
  } catch (e) {
    void e;
  }
  document.getElementById("theme").value = choice;
  retheme();
  syncMenu();
}

/** The same, for the interface language. */
function chooseLocale(code) {
  locale = setLocale(code);
  try {
    localStorage.setItem(LANG_KEY, locale);
  } catch (e) {
    void e;
  }
  document.getElementById("lang").value = locale;
  applyLocale();
  // Everything built in script is not covered by the markup walk above: put
  // the new language on it too, last, so the finder's mode-specific
  // placeholder and the statistics heading win over the generic ones.
  syncPinLabels();
  syncFinder();
  renderEngine();
  renderNvim();
  if (statsOpen) renderStats();
  if (rulesOpen) renderRules();
  void render();
  syncMenu();
  // The reports are drawn in script, with the words of the language that was
  // current then: the button, the "and N more" rows, and the status line that
  // counts them.
  runReport.refresh();
  taskLog.refresh();
  statusLine.refresh();
}


/* ---------------------------------------------------- per-project settings

   What this repository is, as opposed to what this machine is: which
   languages to read here, which paths to leave out, where the sources and
   the map live, what a source link should point at, and whether generating
   it means generating it fully. All of it lives on the project rather than
   on this machine — see `scopebox`'s own comment in `index.html` — and all
   of it is passed to the view.engine on the Rust side, looked up by root, so
   `Generate`, `Generate all` and `Check exactly` honour it without each
   remembering to.

   The view.engine also reads a `.docmap.json` in the repository itself now. What
   is set here wins over that file, because a flag beats a config file in the
   view.engine's own precedence — so an empty field here is not "override with
   nothing", it is "leave whatever the repository says alone". That is why
   every field stores `null` when cleared rather than an empty string.

   Read on open and written on Save, rather than live: this is a form, and a
   form that writes on every keystroke turns a half-typed path into a stored
   setting. */

/** The project the dialog is currently editing, so Save cannot follow a
    selection change made behind the modal. */
let scopeProject = null;

/**
 * Render the language checkboxes.
 *
 * Built from `engine_languages()` — the engine's own capability handshake —
 * rather than from a list here. A list here would be a twenty-fourth backend
 * away from lying, and it would also have nothing to say about grammars.
 *
 * **The three grammar states stay three.** `grammar_loaded: false` means a
 * backend that wants a grammar and has none: a complete module tree and no
 * function-level data. `null` means a backend that needs no parser at all,
 * which is full fidelity rather than a degradation — assembly is the one.
 * Collapsing them would report a healthy backend as broken, which is exactly
 * what the engine keeps them apart to prevent.
 *
 * When the engine cannot be asked, the list is replaced by the sentence
 * saying so. An empty list of checkboxes would read as "this engine supports
 * no languages", which is a different and much worse claim.
 *
 * @param {string[]|null} chosen Names ticked, or null for "all of them".
 */
function renderScopeLanguages(chosen) {
  const list = document.getElementById("scope-langs");
  list.textContent = "";
  const known = view.engineLangs && view.engineLangs.languages;
  if (!known || known.length === 0) {
    const li = document.createElement("li");
    li.textContent = t("scope.languages.unknown");
    li.className = "scope-hint";
    list.appendChild(li);
    return;
  }
  const picked = new Set(chosen || []);
  for (const lang of known) {
    const li = document.createElement("li");
    const label = document.createElement("label");
    const box = document.createElement("input");
    box.type = "checkbox";
    box.value = lang.name;
    box.checked = picked.has(lang.name);
    const name = document.createElement("code");
    name.textContent = lang.name;
    label.appendChild(box);
    label.appendChild(name);
    li.appendChild(label);

    let hint = null;
    if (lang.grammar_loaded === false) {
      hint = t("scope.languages.nogrammar");
    } else if (lang.grammar_loaded === null || lang.grammar_loaded === undefined) {
      hint = t("scope.languages.nogrammarneeded");
    }
    if (hint) {
      const span = document.createElement("span");
      span.className = "scope-hint";
      span.textContent = hint;
      li.appendChild(span);
    }
    list.appendChild(li);
  }
}

/** Open the per-project settings dialog for the selected project. */
/** A stored Map directory the settings would refuse (an old `../maps`): said when the dialog opens, not only on Generate. */
async function showStoredMapDirProblem(dir) {
  if (!dir) return;
  let refused = false;
  try {
    refused = await invoke("map_dir_refused", { dir });
  } catch (e) {
    void e;
  }
  if (!refused) return;
  const problem = document.getElementById("scope-problem");
  problem.textContent = t("err.outDir.stored").replace("{value}", dir);
  problem.hidden = false;
}

async function openScope() {
  const p = view.projects.find((x) => x.id === view.selectedId);
  if (!p) return;
  scopeProject = p;

  const lead = document.getElementById("scope-lead");
  // `renderRich` because the sentence carries a `<strong>` around the project
  // name, and the name is the one substituted value — escaped, because a
  // directory name is not this program's text. Same rule the placeholder
  // bodies follow.
  renderRich(lead, fill(t("scope.lead"), { name: escapeHtml(p.name) }));

  const problem = document.getElementById("scope-problem");
  problem.hidden = true;
  problem.textContent = "";

  let scope = { exclude: [], languages: null };
  try {
    scope = await invoke("project_scope_get", { id: p.id });
  } catch (e) {
    void e;
  }
  document.getElementById("scope-exclude").value = (scope.exclude || []).join("\n");
  // `?? ""` rather than `|| ""`: identical for these values today, and the
  // one that stays right if a field ever legitimately holds a falsy value
  // somebody meant.
  document.getElementById("scope-source").value = scope.source ?? "";
  document.getElementById("scope-outdir").value = scope.outDir ?? "";
  await showStoredMapDirProblem(scope.outDir ?? "");
  document.getElementById("scope-repourl").value = scope.repoUrl ?? "";
  document.getElementById("scope-branch").value = scope.branch ?? "";
  document.getElementById("scope-full").checked = Boolean(scope.full);
  renderScopeLanguages(scope.languages);
  document.getElementById("scopebox").showModal();
}

/** A field's value, or `null` when it was left empty.

    `null`, never `""`: an empty field means "the engine decides", and the
    view.engine's answer includes whatever the repository's own `.docmap.json`
    says. An empty string would reach it as a real `--out-dir=` and override
    that file with nothing. Rust normalises the non-empty case; this only has
    to get the empty one right.

    @param {string} id
    @returns {string|null} */
function scopeField(id) {
  const value = document.getElementById(id).value.trim();
  return value === "" ? null : value;
}

document.getElementById("scope-save").addEventListener("click", async () => {
  if (!scopeProject) return;
  const boxes = [...document.querySelectorAll("#scope-langs input:checked")];
  const exclude = document
    .getElementById("scope-exclude")
    .value.split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  try {
    await invoke("project_scope_set", {
      id: scopeProject.id,
      exclude,
      // Nothing ticked is `null`, not `[]`: the two mean the same thing to
      // the engine, and one spelling means the dialog can un-tick everything
      // without inventing a third state that reads as "an empty map, please".
      languages: boxes.length > 0 ? boxes.map((b) => b.value) : null,
      outDir: scopeField("scope-outdir"),
      source: scopeField("scope-source"),
      repoUrl: scopeField("scope-repourl"),
      branch: scopeField("scope-branch"),
      full: document.getElementById("scope-full").checked,
    });
    say(fill(t("scope.saved"), { name: scopeProject.name }));
    document.getElementById("scopebox").close();
    // The map directory may have moved, and `map_dir` moved with it on the
    // Rust side. Everything on screen that names a map — the picker's
    // detail block, the freshness mark, the overview row — is drawn from
    // the project list this call did not refresh, so without this the
    // window keeps reading the old location and looks like the setting did
    // not take.
    freshness.delete(scopeProject.id);
    await refresh();
  } catch (e) {
    const problem = document.getElementById("scope-problem");
    problem.textContent = fill(t("scope.failed"), { error: errorText(e) });
    problem.hidden = false;
  }
});

/* A folder picker beside the textarea, because the paths are
   repository-relative and typing one by hand is where the typo lives. The
   picker returns an absolute path; the only thing this has to do is refuse a
   directory outside the project rather than storing a path that can never
   match. */
document.getElementById("scope-add").addEventListener("click", async () => {
  if (!scopeProject) return;
  const problem = document.getElementById("scope-problem");
  try {
    const dir = await open({
      directory: true,
      multiple: false,
      defaultPath: scopeProject.root,
      title: t("scope.add"),
    });
    if (!dir) return;
    // `split`/`join` rather than a regex: the pattern would be a backslash
    // class inside a literal, which is the one place this file has had to be
    // escaped three deep. Two plain string operations say the same thing and
    // cannot be misread.
    const slash = (s) => s.split("\\").join("/").replace(/\/+$/, "");
    const root = slash(scopeProject.root);
    const picked = slash(String(dir));
    if (picked === root || picked.indexOf(root + "/") !== 0) {
      problem.textContent = fill(t("scope.outside"), { path: picked });
      problem.hidden = false;
      return;
    }
    const rel = picked.slice(root.length + 1);
    const field = document.getElementById("scope-exclude");
    const lines = field.value.split("\n").map((l) => l.trim()).filter(Boolean);
    if (!lines.includes(rel)) {
      lines.push(rel);
    }
    field.value = lines.join("\n");
    problem.hidden = true;
  } catch (e) {
    problem.textContent = fill(t("scope.failed"), { error: errorText(e) });
    problem.hidden = false;
  }
});

/**
 * Open Settings.
 *
 * The engine and Neovim state is copied in when it opens rather than kept
 * in step continuously: the dialog is modal, so the sidebar's own copy is
 * behind it and the two are never on screen together — and a path being
 * pointed at is exactly what someone wants to read while they point at a
 * different one.
 */
function openPrefs() {
  // The two behaviour controls mirror state that lives elsewhere — the
  // sidebar's dropdown and the dashboard's own checkbox — so they are filled
  // on open for the same reason the engine state is: the dialog is modal,
  // nothing can change them behind it, and reading them here means there is
  // no second copy to keep in step while it is shut.
  //
  // The options are cloned from the sidebar's own `<select>` rather than
  // written out again in the markup: they are the same four orders, and a
  // second list would be a second place to add the fifth.
  const sortBox = document.getElementById("prefs-sort");
  if (sortBox && sortBox.options.length === 0) {
    for (const option of els.sort.options) {
      sortBox.appendChild(option.cloneNode(true));
    }
  }
  if (sortBox) sortBox.value = sortBy;
  document.getElementById("prefs-dashboard").checked = !wsSkipped();

  // Read on open rather than kept in sync: it is a text field nothing else
  // writes.
  invoke("editor_command", { set: null })
    .then((cmd) => {
      document.getElementById("editor-cmd").value = cmd || "";
    })
    .catch((e) => void e);
  // Asked when the dialog opens rather than kept live: reading it walks a
  // directory, and nothing changes it while the dialog is shut.
  renderTelemetry();
  renderTrafficSettings();
  document.getElementById("prefsbox").showModal();
}

/**
 * The versions, as text somebody can paste.
 *
 * Never translated. This block is read by whoever triages the report, not
 * by the person writing it — the same rule the feedback dialog's
 * environment block follows.
 *
 * `dirty` is carried through rather than dropped: a binary built from a
 * modified tree has a commit that does not describe it, and a bug report
 * quoting that commit sends the reader to the wrong diff.
 */
function aboutFacts() {
  const lines = [];
  if (aboutInfo) {
    lines.push("docmap-desktop " + aboutInfo.appVersion);
    lines.push(aboutInfo.os + " / " + aboutInfo.arch);
  }
  lines.push("engine: " + (view.engine.path || t("about.engineNone")));
  if (view.engineLangs && view.engineLangs.schema !== null && view.engineLangs.schema !== undefined) {
    lines.push("schema: " + view.engineLangs.schema);
  }
  const b = view.engineLangs && view.engineLangs.build;
  if (b && b.commit) {
    lines.push(
      "build: " + b.commit + (b.committedAt ? " (" + b.committedAt + ")" : "")
    );
    if (b.dirty) lines.push("  " + t("about.dirty"));
  } else {
    lines.push("build: " + t("about.buildNone"));
  }
  lines.push("grammars: " + (view.engine.grammars || "none"));
  lines.push("interface: " + locale);
  return lines.join("\n");
}

async function openAbout() {
  if (!aboutInfo) {
    try {
      aboutInfo = await invoke("about_info");
    } catch (e) {
      void e;
    }
  }
  document.getElementById("about-facts").textContent = aboutFacts();
  document.getElementById("aboutbox").showModal();
}

document.getElementById("about-copy").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(aboutFacts());
    say(t("about.copied"));
  } catch (e) {
    // A clipboard the window is not allowed to touch is not a reason to
    // lose the text: it is already on screen and selectable.
    say(errorText(e));
  }
});

document.getElementById("editor-cmd").addEventListener("change", (ev) => {
  invoke("editor_command", { set: ev.target.value }).catch((e) => say(errorText(e)));
});

document.getElementById("prefs-sort").addEventListener("change", (ev) => {
  applySort(ev.target.value);
});

document.getElementById("prefs-dashboard").addEventListener("change", (ev) => {
  setStartDashboard(ev.target.checked);
});

async function openDocs(page) {
  try {
    await invoke("open_docs", { page });
  } catch (e) {
    say(errorText(e));
  }
}

document.getElementById("about-repo").addEventListener("click", () => openDocs("repo"));
document.getElementById("about-license").addEventListener("click", () => openDocs("license"));

// Help -> Search the documentation: the words go to Rust, which opens
// GitHub's search over this repository's Markdown (`search_docs`).
const docSearch = {
  box: document.getElementById("docsearchbox"),
  q: document.getElementById("docsearch-q"),
  go: document.getElementById("docsearch-go"),
};

function openDocSearch() {
  docSearch.box.showModal();
  docSearch.q.select();
}

async function runDocSearch() {
  const query = docSearch.q.value.trim();
  if (query === "") {
    docSearch.q.focus();
    return;
  }
  try {
    await invoke("search_docs", { query });
    docSearch.box.close();
  } catch (e) {
    say(errorText(e));
  }
}

docSearch.go.addEventListener("click", runDocSearch);
onEnterSubmit(docSearch.q, runDocSearch);

// The name at the top of the sidebar leads to the start: the workspaces.
document.getElementById("brand").addEventListener("click", () => openWorkspaces());

// A click beside a dialog closes it, the way Escape does, as in the other
// desktop apps (appkit-ui's closeOnBackdrop, see lib/backdrop.js).
document.querySelectorAll("dialog").forEach((box) => closeOnBackdrop(box));

/**
 * Hand Rust the labels and the enable state, and let it rebuild the menu.
 *
 * Called after the locale is settled, on every language change, and on every
 * selection change — the items that act on a project are greyed when there
 * is none, which is the one thing the sidebar's disabled Generate button
 * never explained.
 */
async function syncMenu() {
  const labels = {};
  for (const key of keys()) {
    if (key.startsWith("menu.")) labels[key] = t(key);
  }
  try {
    await invoke("set_menu", {
      labels,
      hasProject: Boolean(view.selectedId),
      state: viewState(),
    });
  } catch (e) {
    // Surfaced rather than swallowed: a window with no menu is a window
    // missing half its commands, and silence here would look like a menu
    // bar that is simply slow.
    say(errorText(e));
  }
}

// =====================================================================
// The per-project context menu
//
// Right-click on the detail block under the picker. Not on the picker
// itself: a `<select>` answers a right-click with the platform's own menu,
// and fighting a native control over the one thing it is for is a losing
// argument. The detail block is where the selected project is named, which
// is what these commands act on.
//
// Every item calls the same function its menu-bar twin calls — the third
// surface for one implementation, and still no second copy of any of them.
// =====================================================================

const projmenu = document.getElementById("projmenu");

/** Same ids as the menu bar, so the labels come from the same catalog keys. */
const PROJECT_CONTEXT = [
  "menu.project.generate",
  "menu.project.regenerate",
  "sep",
  "menu.file.open_browser",
  "menu.file.reveal",
  "menu.file.copy_path",
  "sep",
  "menu.file.remove",
];

function closeProjMenu() {
  projmenu.hidden = true;
}

function openProjMenu(x, y) {
  if (!view.selectedId) return;
  projmenu.replaceChildren();
  PROJECT_CONTEXT.forEach((id) => {
    if (id === "sep") {
      const li = document.createElement("li");
      li.className = "sep";
      li.setAttribute("role", "separator");
      projmenu.append(li);
      return;
    }
    const li = document.createElement("li");
    li.setAttribute("role", "none");
    const b = document.createElement("button");
    b.type = "button";
    b.setAttribute("role", "menuitem");
    b.textContent = t(id);
    b.addEventListener("click", () => {
      closeProjMenu();
      const run = MENU_ACTIONS[id];
      if (run) run();
    });
    li.append(b);
    projmenu.append(li);
  });

  // Shown before measuring: a hidden element has no size, so flipping it
  // away from the edges has to happen after it is on screen.
  projmenu.hidden = false;
  projmenu.style.left = "0px";
  projmenu.style.top = "0px";
  const r = projmenu.getBoundingClientRect();
  const left = Math.min(x, window.innerWidth - r.width - 6);
  const top = Math.min(y, window.innerHeight - r.height - 6);
  projmenu.style.left = Math.max(6, left) + "px";
  projmenu.style.top = Math.max(6, top) + "px";
  const first = projmenu.querySelector("button");
  if (first) first.focus();
}

els.detail.addEventListener("contextmenu", (ev) => {
  if (!view.selectedId) return;
  ev.preventDefault();
  openProjMenu(ev.clientX, ev.clientY);
});

// Closing, by every route someone would expect: a click anywhere else,
// Escape, a scroll, and the window losing focus. The map is an iframe and
// swallows clicks that never reach this document, so its own pointer events
// are not one of the routes — which is why the window `blur` is.
document.addEventListener("click", (ev) => {
  if (!projmenu.hidden && !projmenu.contains(ev.target)) closeProjMenu();
});
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape" && !projmenu.hidden) {
    closeProjMenu();
    els.detail.focus?.();
  }
});
window.addEventListener("blur", closeProjMenu);
window.addEventListener("resize", closeProjMenu);

{
  const { listen } = window.__TAURI__.event;
  listen("menu", (ev) => {
    // The language items are built from the locale list rather than from
    // `menu.rs`, so their ids carry the code after a colon and cannot be
    // table entries.
    if (ev.payload.startsWith(LOCALE_ITEM)) {
      chooseLocale(ev.payload.slice(LOCALE_ITEM.length));
      return;
    }
    const run = MENU_ACTIONS[ev.payload];
    if (run) run();
  });
  if (web) {
    // Folders were read again: what was measured from the old state is forgotten. The payload names the projects
    // that changed on disk; an empty one means the browser was allowed to read folders it could not read before.
    listen("web:folders-changed", (ev) => {
      for (const p of view.projects) {
        invalidate(p.map_dir);
        invalidateLanguages(p.root);
      }
      freshness.clear();
      const changed = Array.isArray(ev.payload) ? ev.payload : [];
      if (changed.length > 0 && view.selectedId && view.mapBase) {
        // A map is being read: it stays where the reader is (the page is not loaded again every time a file is
        // saved), and only the mark that it is older than its sources is looked at again.
        if (changed.includes(view.selectedId)) refreshFreshness(view.selectedId);
        return;
      }
      select(view.selectedId);
    });
  }
  syncMenu();
}
