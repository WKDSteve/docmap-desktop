// Preview-only Tauri bridge stub. NOT part of the app — used to look at the
// real markup and CSS in a browser. Data below is real: the language list is
// `lang_registry.report()` from docmap.nvim at 5d2b98d.
const LANGS = [
  {"grammar":"lua","name":"lua","grammar_loaded":true,"calls":true},
  {"grammar":"javascript","name":"js","grammar_loaded":false,"calls":true},
  {"grammar":"typescript","name":"ts","grammar_loaded":false,"calls":true},
  {"grammar":"tsx","name":"tsx","grammar_loaded":false,"calls":true},
  {"grammar":"zig","name":"zig","grammar_loaded":false,"calls":false},
  {"grammar":"java","name":"java","grammar_loaded":false,"calls":false},
  {"grammar":"c","name":"c","grammar_loaded":true,"calls":false},
  {"grammar":"cpp","name":"cpp","grammar_loaded":false,"calls":false},
  {"name":"asm","calls":false},
  {"grammar":"python","name":"python","grammar_loaded":false,"calls":false},
  {"grammar":"c_sharp","name":"csharp","grammar_loaded":false,"calls":false},
  {"grammar":"go","name":"go","grammar_loaded":false,"calls":false},
  {"grammar":"rust","name":"rust","grammar_loaded":true,"calls":false},
  {"grammar":"php","name":"php","grammar_loaded":false,"calls":false},
  {"grammar":"ruby","name":"ruby","grammar_loaded":false,"calls":false},
  {"grammar":"kotlin","name":"kotlin","grammar_loaded":false,"calls":false},
  {"grammar":"swift","name":"swift","grammar_loaded":false,"calls":false},
  {"grammar":"dart","name":"dart","grammar_loaded":false,"calls":false},
  {"grammar":"scala","name":"scala","grammar_loaded":false,"calls":false},
  {"grammar":"haskell","name":"haskell","grammar_loaded":false,"calls":false},
  {"grammar":"elixir","name":"elixir","grammar_loaded":false,"calls":false},
  {"grammar":"erlang","name":"erlang","grammar_loaded":false,"calls":false},
  {"grammar":"ocaml","name":"ocaml","grammar_loaded":false,"calls":false}
];

// The first two are the shapes this harness has always used: one ordinary
// project and one name long enough to find a layout that cannot hold it.
//
// The rest exist for the workspace overview, and their numbers are measured
// rather than invented -- `schema`, the module and file counts and the
// staleness below are what these repositories actually reported on
// 2026-08-21, when 27 of 30 generated maps in that tree were three artifact
// versions behind the engine. A preview of a ranked list is worth nothing if
// every row ranks the same, and a corpus where they legitimately differ is
// the thing that was hardest to imagine and easiest to read off a disk.
const PROJECTS = [
  { id: "p1", name: "docmap.nvim", root: "E:/repos/docmap.nvim",
    map_dir: "E:/repos/docmap.nvim/docs/map", exclude: [], languages: null },
  { id: "p2", name: "a-monorepo-with-a-very-long-name", root: "E:/repos/mono",
    map_dir: "E:/repos/mono/docs/map", exclude: ["vendor", "third_party/grpc"],
    languages: ["go", "python"] },
  { id: "p3", name: "lib.nvim", root: "E:/repos/lib.nvim",
    map_dir: "E:/repos/lib.nvim/docs/map", exclude: [], languages: null },
  { id: "p4", name: "sandbox.nvim", root: "E:/repos/sandbox.nvim",
    map_dir: "E:/repos/sandbox.nvim/docs/map", exclude: [], languages: null },
  { id: "p5", name: "runtime-analysis.nvim", root: "E:/repos/runtime-analysis.nvim",
    map_dir: "E:/repos/runtime-analysis.nvim/docs/map", exclude: [], languages: null },
  { id: "p6", name: "debugging.nvim", root: "E:/repos/debugging.nvim",
    map_dir: "E:/repos/debugging.nvim/docs/map", exclude: [], languages: null },
  { id: "p7", name: "docmap-desktop", root: "E:/repos/docmap-desktop",
    map_dir: "E:/repos/docmap-desktop/docs/map", exclude: [], languages: null },
  { id: "p8", name: "reposcope.nvim", root: "E:/repos/reposcope.nvim",
    map_dir: "E:/repos/reposcope.nvim/docs/map", exclude: [], languages: null }
];

/** Per-project `map_status` and `map_freshness`, keyed by id. */
const STATE = {
  p1: { st: { modules: 5, files: 120, namespaces: 6, schema: 5 }, fr: { stale: false } },
  p2: { st: { modules: 142, files: 210, namespaces: 12, schema: 3 },
        fr: { stale: true, behind_secs: 7200, newest: "services/api/main.go", truncated: true } },
  p3: { st: { modules: 124, files: 124, namespaces: 24, schema: 2 },
        fr: { stale: true, behind_secs: 432000, newest: "lua/lib/nvim/cross/fs/separators/normalize/init.lua" } },
  p4: { st: { modules: 3, files: 263, namespaces: 43, schema: 2 },
        fr: { stale: true, behind_secs: 207360, newest: ".gitignore" } },
  p5: { st: { modules: 3, files: 35, namespaces: 2, schema: 5 }, fr: { stale: false } },
  p6: { st: { modules: 8, files: 21, namespaces: 7, schema: 3 },
        fr: { stale: true, behind_secs: 3600, newest: "lua/debugging/init.lua" } },
  p7: { st: null, fr: { has_map: false } },
  p8: { st: { modules: 5, files: 96, namespaces: 26, schema: 2 },
        fr: { stale: true, behind_secs: 216000, newest: "doc/reposcope.txt" } }
};

/** The id whose `map_dir` this is -- `map_status` is asked by directory. */
function idForDir(dir) {
  const hit = PROJECTS.find((p) => p.map_dir === dir);
  return hit ? hit.id : "p1";
}

let ACTIVE = "Default";
const WORKSPACES = [{ name: "Default", projects: 8 }, { name: "Work", projects: 2 }];

const SCOPES = {
  p1: { exclude: [], languages: null },
  p2: { exclude: ["vendor", "third_party/grpc"], languages: ["go", "python"] }
};

// GitHub traffic. The numbers are illustrative; the *shapes* are what
// `traffic.rs` serialises (camelCase fields, snake_case status), and the mix of
// outcomes is the point: one project with fresh data, one whose data is old,
// one the plugin does not track, one with an unreadable digest, one from a
// newer plugin, and the rest with no GitHub remote — which show nothing.
const daysAgo = (n) => new Date(Date.now() - n * 86400000).toISOString();
const metric = (d7, d30, d90, trend) => ({
  d7: { count: d7, uniques: Math.ceil(d7 * 0.6) },
  d30: { count: d30, uniques: Math.ceil(d30 * 0.6) },
  d90: { count: d90, uniques: Math.ceil(d90 * 0.6) },
  trend
});
const okInfo = (repo, fetchedDaysAgo, views, clones) => ({
  status: "ok", repo, message: null, dir: "C:/Users/me/AppData/Local/nvim-data/github_stats.nvim/digest",
  via: "root",
  summary: {
    generated: daysAgo(fetchedDaysAgo), fetched: daysAgo(fetchedDaysAgo),
    spanFrom: "2026-06-29", spanTo: "2026-09-23", views, clones, daysKept: 87,
    hasReferrers: true, hasPaths: true
  }
});
const TRAFFIC = {
  p1: okInfo("WKDSteve/docmap.nvim", 1, metric(41, 210, 655, 12.5), metric(19, 88, 300, -3)),
  p3: okInfo("WKDSteve/lib.nvim", 9, metric(2, 17, 60, null), metric(0, 4, 21, null)),
  p4: { status: "not_tracked", repo: "WKDSteve/sandbox.nvim", message: null, summary: null, dir: "x", via: "root" },
  p5: { status: "unreadable", repo: "WKDSteve/runtime-analysis.nvim", message: "not JSON: expected value at line 1 column 1", summary: null, dir: "x", via: "root" },
  p6: { status: "newer_schema", repo: "WKDSteve/debugging.nvim", message: "schema 2, this app understands up to 1", summary: null, dir: "x", via: "root" }
};
const trafficEntry = (id) => {
  const info = TRAFFIC[id] || { status: "no_remote" };
  const s = info.summary;
  return {
    id, status: info.status,
    views7: s ? s.views.d7.count : null, views30: s ? s.views.d30.count : null,
    clones30: s ? s.clones.d30.count : null, trend: s ? s.views.trend : null,
    fetched: s ? s.fetched : null
  };
};
// The detail dialog. Shapes are `traffic.rs`'s `Digest` verbatim -- unlike
// `Summary` above, `Digest`/`PathItem`/`Referrer` carry no `rename_all`, so
// every field here is the Rust name as-is (snake_case where it has one,
// `project_path` included). Two things worth previewing on purpose: a
// referrer name that looks like a markup injection (must render as inert
// text, never parsed), and a `paths` entry with `project_path: null` next
// to ones that resolved, so an unclickable row is visibly different from a
// clickable one.
const dailySeries = (days, base, amplitude) =>
  Array.from({ length: days }, (_, i) => {
    const iso = new Date(Date.now() - (days - 1 - i) * 86400000).toISOString().slice(0, 10);
    const v = Math.max(0, Math.round(base + amplitude * Math.sin(i / 6) + (i % 5) * 4));
    return [iso, v, Math.ceil(v * 0.6)];
  });
const TRAFFIC_DETAIL = {
  p1: {
    schema: 1,
    repo: "WKDSteve/docmap.nvim",
    generated: daysAgo(1),
    fetched: daysAgo(1),
    span: { from: "2026-06-29", to: "2026-09-23" },
    views: metric(41, 210, 655, 12.5),
    clones: metric(19, 88, 300, -3),
    daily: { views: dailySeries(87, 8, 30), clones: dailySeries(87, 3, 10) },
    referrers: [
      { referrer: "github.com", count: 120, uniques: 95 },
      { referrer: "google.com", count: 44, uniques: 40 },
      { referrer: "<img src=x onerror=alert(1)>", count: 3, uniques: 2 },
    ],
    paths: [
      { path: "/WKDSteve/docmap.nvim/blob/main/README.md", title: "README.md", count: 300, uniques: 210, project_path: "README.md" },
      { path: "/WKDSteve/docmap.nvim/blob/main/docs/USAGE.md", title: "docs/USAGE.md", count: 120, uniques: 88, project_path: "docs/USAGE.md" },
      { path: "/WKDSteve/docmap.nvim/blob/main/docs/deleted.md", title: "docs/deleted.md", count: 12, uniques: 9, project_path: null },
    ],
  },
  p3: {
    schema: 1,
    repo: "WKDSteve/lib.nvim",
    generated: daysAgo(9),
    fetched: daysAgo(9),
    span: { from: "2026-07-10", to: "2026-09-15" },
    views: metric(2, 17, 60, null),
    clones: metric(0, 4, 21, null),
    daily: { views: dailySeries(67, 1, 3), clones: [] },
    referrers: [],
    paths: null,
  },
};

const SURVEY = {
  explicit: null, asked: null, defaultDir: "C:/Users/me/AppData/Local/nvim-data/github_stats.nvim",
  attempts: [{ via: "root", dir: "C:/Users/me/AppData/Local/nvim-data/github_stats.nvim", ok: true, reason: null }],
  found: { via: "root", dir: "C:/Users/me/AppData/Local/nvim-data/github_stats.nvim/digest", repos: 6, newest: Math.floor(Date.now() / 1000) - 86400 }
};

const R = {
  traffic_info: (a) => TRAFFIC[a.id] || { status: "no_remote", repo: null, message: null, summary: null, dir: null, via: null },
  traffic_list: (a) => (a.ids || []).map(trafficEntry),
  traffic_detail: (a) => TRAFFIC_DETAIL[a.id] || null,
  traffic_settings: () => SURVEY,
  traffic_set_dir: (a) => ({ ...SURVEY, explicit: a.path || null }),
  traffic_ask_neovim: () => SURVEY,
  traffic_set_hidden: () => null,
  traffic_refresh: () => null,
  list_projects: () => PROJECTS,
  list_workspaces: () => WORKSPACES.map((w) => ({ ...w, active: w.name === ACTIVE })),
  switch_workspace: (a) => {
    ACTIVE = a.name;
    if (!WORKSPACES.some((w) => w.name === ACTIVE)) WORKSPACES.push({ name: ACTIVE, projects: 0 });
    return PROJECTS;
  },
  engine_info: () => ({ path: "C:/tools/docmap.exe", from_path: false, bundled: false,
                        grammars: "C:/tools/docmap-grammars" }),
  // Schema 3 against a contract of 6, on purpose: the Contract row then shows
  // its "older" sentence, the longest it has, at the minimum window size.
  engine_languages: () => ({ languages: LANGS, schema: 3,
                             build: { commit: "5d2b98d", committed_at: "2026-08-20", clean: true },
                             routes: ["checklist", "commits", "loaded", "loaded/snapshots", "telemetry", "telemetry/snapshots"],
                             contract: { commit: "5fdb34c659ed9f93eb96dbe4f7341b5f8aebe683", schema: 6,
                                         routes: ["checklist", "commits", "loaded", "loaded/snapshots", "telemetry", "telemetry/snapshots"] } }),
  nvim_info: () => ({ path: "C:/Program Files/Neovim/bin/nvim.exe", from_path: true,
                      config_dir: "C:/Users/me/AppData/Local/nvim", config_dir_from_default: true }),
  // Measured, like everything else here: these are the real edges between
  // the projects listed above, on 2026-08-21. The single reverse edge
  // (lib.nvim requiring runtime-analysis.telemetry once) is in deliberately
  // -- a graph where every arrow points the same way hides whether the
  // layout can show one that does not.
  workspace_deps: () => ({
    edges: [
      { from: "p6", to: "p3", count: 57,
        modules: ["lib.nvim.notify", "lib.nvim.window", "lib.nvim.buf_win_tab.buffer_utils",
                  "lib.nvim.cache.memory"] },
      { from: "p1", to: "p3", count: 43,
        modules: ["lib.nvim.fs.read", "lib.nvim.safe_api", "lib.nvim.autocmd"] },
      { from: "p5", to: "p3", count: 43,
        modules: ["lib.nvim.notify", "lib.nvim.autocmd", "lib.nvim.cache.disk"] },
      { from: "p8", to: "p3", count: 30,
        modules: ["lib.nvim.cross.fs.expand_path", "lib.nvim.ui.kit",
                  "lib.nvim.cross.uv.spawn_capture"] },
      { from: "p4", to: "p3", count: 24,
        modules: ["lib.nvim.window", "lib.nvim.ui.kit", "lib.nvim.usercmd.composer"] },
      { from: "p3", to: "p5", count: 1, modules: ["runtime-analysis.telemetry"] }
    ],
    outside: [
      { name: "which-key", count: 21, projects: ["p1"] },
      { name: "telescope.actions", count: 19, projects: ["p1"] },
      { name: "fzf-lua", count: 19, projects: ["p8"] },
      { name: "telescope.actions.state", count: 18, projects: ["p1"] },
      { name: "telescope", count: 16, projects: ["p1"] },
      { name: "dap", count: 16, projects: ["p6"] },
      { name: "telescope.pickers", count: 15, projects: ["p1"] }
    ],
    unread: ["p7"]
  }),
  map_status: (a) => {
    const st = STATE[idForDir(a && a.mapDir)].st;
    return st
      ? { exists: true, index_path: "E:/repos/docmap.nvim/docs/map/index.html", ...st }
      : { exists: false, index_path: "" };
  },
  map_freshness: (a) => {
    const fr = STATE[(a && a.id) in STATE ? a.id : "p1"].fr;
    // camelCase, as `freshness.rs` serialises it (`#[serde(rename_all =
    // "camelCase")]`); the table above keeps the older snake_case spelling.
    return {
      hasMap: fr.has_map ?? true, stale: fr.stale ?? false, truncated: fr.truncated ?? false,
      newest: fr.newest ?? null, behindSecs: fr.behind_secs ?? null, generatedSecs: 100000,
    };
  },
  project_stats: () => ({
    languages: [
      { name: "Lua", kind: "code", files: 142, lines: { total: 21840, code: 15210, comment: 4820, blank: 1810 }, bytes: 840000 },
      { name: "Markdown", kind: "docs", files: 38, lines: { total: 6100, code: 4900, comment: 0, blank: 1200 }, bytes: 210000 },
      { name: "JavaScript", kind: "code", files: 12, lines: { total: 1900, code: 1500, comment: 250, blank: 150 }, bytes: 70000 },
      { name: "JSON", kind: "data", files: 9, lines: { total: 640, code: 640, comment: 0, blank: 0 }, bytes: 22000 },
      { name: "Rust", kind: "code", files: 6, lines: { total: 520, code: 400, comment: 60, blank: 60 }, bytes: 18000 },
    ],
    code: { files: 160, lines: { total: 24260, code: 17110, comment: 5130, blank: 2020 } },
    docs: { files: 38, lines: { total: 6100, code: 4900, comment: 0, blank: 1200 } },
    data: { files: 9, lines: { total: 640, code: 640, comment: 0, blank: 0 } },
    other: { files: 14, lines: { total: 0, code: 0, comment: 0, blank: 0 } },
    files: 221,
    bytes: 1160000,
    largest: [
      { path: "lua/lib/nvim/cross/fs/init.lua", lines: 1210 },
      { path: "lua/lib/nvim/ui/kit.lua", lines: 980 },
      { path: "lua/lib/nvim/window.lua", lines: 760 },
    ],
    truncated: false,
  }),
  project_search: (a) => ({
    hits: a.mode === "files"
      ? [
          { path: "lua/lib/nvim/init.lua", line: null, text: null, at: null },
          { path: "lua/lib/nvim/window/init.lua", line: null, text: null, at: null },
        ]
      : [
          { path: "lua/lib/nvim/window.lua", line: 42, text: "local function open_window(opts) -- " + a.query, at: 3 },
          { path: "lua/lib/nvim/window.lua", line: 97, text: "return " + a.query + "(buf)", at: 7 },
          { path: "docs/USAGE.md", line: 12, text: "Use " + a.query + " to open it", at: 4 },
        ],
    filesSearched: 211, truncated: false, reason: null,
  }),
  view_search: (a) => ({
    available: true, truncated: false,
    hits: [
      { kind: "file", label: "window.lua", detail: "path: lua/lib/nvim/window.lua", node: "lua/lib/nvim/window.lua", file: "lua/lib/nvim/window.lua", line: null },
      { kind: "function", label: "M.open", detail: "summary: Opens a floating " + a.query, node: "lua/lib/nvim/window.lua", file: "lua/lib/nvim/window.lua", line: 42 },
      { kind: "doc", label: "Using it", detail: "docs/USAGE.md", node: null, file: "docs/USAGE.md", line: null },
    ],
  }),
  map_changes: (a) => ({
    files: [
      { path: "lua/lib/nvim/cross/fs/init.lua", afterSecs: 600 },
      { path: "lua/lib/nvim/window.lua", afterSecs: 7200 },
      { path: "README.md", afterSecs: 90000 },
    ],
    total: (a.id === "p2") ? 250 : 3,
    truncated: false,
  }),
  scan_languages: (a) => (String(a.root).includes("mono")
    ? { total: 210, truncated: false, languages: [
        { name: "Go", files: 150, grammar: "go", backend: null },
        { name: "Python", files: 60, grammar: "python", backend: null }] }
    : { total: 160, truncated: false, languages: [
        { name: "Lua", files: 142, grammar: "lua", backend: null },
        { name: "JavaScript", files: 12, grammar: "javascript", backend: null },
        { name: "Rust", files: 6, grammar: "rust", backend: null }] }),
  project_scope_get: (a) => SCOPES[a.id] ?? { exclude: [], languages: null },
  grammar_dir: () => ({
    dir: "C:/tools/docmap-grammars",
    from_setting: true,
    exists: true,
    files: ["javascript.dll", "lua.dll", "tsx.dll", "typescript.dll"],
    more: 0
  }),
  project_scope_set: () => null,
  // A run in the engine's own report format, in the three outcomes the real
  // app shows, chosen by project so each can be looked at:
  //
  //   * `mono` (a-monorepo...): a crash after the findings. Exit 1, no map
  //     written, the old one (it has one) stays - "Failed" with the report.
  //   * `runtime-analysis`: a clean run. Exit 0, a map, nothing found.
  //   * every other one: a run that wrote its map and found a lot in the tree
  //     (28 errors, 71 warnings: the "many findings" case). A project that had
  //     no map has one afterwards, which is what makes the window select it and
  //     leave the placeholder behind - the state the real app is in then.
  //
  // The vault names in the findings are made up; the *shape* (relative paths
  // with many `../`, a `$REPOS_DIR` placeholder, 44/22/4/1 warnings per check)
  // is the report of a real run. The tally is the engine's own line: it counts
  // every finding including the `info` ones stderr never lists, so errors and
  // warnings agree with the listed rows and `info` does not.
  generate: (a) => {
    const row = (sev, check, msg) => `  [${sev}] ${check.padEnd(22)} ${msg}`;
    const project = PROJECTS.find((p) => p.root === a.root);
    const crashed = String(a.root).includes("mono");
    const clean = String(a.root).includes("runtime-analysis");
    const err = [];
    let errors = 0;
    let warnings = 0;
    if (!clean) {
      for (let i = 0; i < 28; i++, errors++) err.push(row("error", "missing-module-tag", `./vault-a/tests/t${i}.lua has no ---@module annotation`));
      for (let i = 0; i < 44; i++, warnings++) {
        const target = i % 3 ? `../../../../../docmap.nvim/docs/d${i}.md` : `$REPOS_DIR/docmap.nvim/docs/d${i}.md`;
        err.push(row("warn", "dead-readme-link", `vault-b/plugin-x/ROADMAP/R${i}.md links to '${target}' which does not exist`));
      }
      for (let i = 0; i < 22; i++, warnings++) err.push(row("warn", "missing-summary", `./vault-c/f${i}.lua has no summary`));
      for (let i = 0; i < 4; i++, warnings++) err.push(row("warn", "require-not-declared", `./a${i}.lua requires 'b${i}'`));
      err.push(row("warn", "doc-references-missing", "./README.md references a missing doc"));
      warnings++;
    }
    const out = [
      "wrote docs/map/index.html",
      "wrote docs/map/module_map.json",
      "2 modules, 1090 namespaces, 57 files",
      "",
      `${errors} errors, ${warnings} warnings, ${clean ? 0 : 176} info`,
      "",
    ];
    if (crashed) err.push("docmap: source directory not found", "stack traceback:", "\t[C]: in function 'assert'");
    if (!crashed && project && !STATE[project.id].st) {
      STATE[project.id].st = { modules: 2, files: 57, namespaces: 1090, schema: 5 };
      STATE[project.id].fr = { stale: false };
    }
    return { ok: !crashed, code: crashed ? 1 : 0, stdout: out.join("\n"), stderr: err.join("\n") + "\n" };
  },
  telemetry_info: () => ({ enabled: false }),
  about_info: () => ({ app: "docmap-desktop", version: "0.1.0" }),
  project_icon: () => null,
  editor_command: () => "nvim",
  file_tree: () => ({ name: "root", children: [] }),
  // `pick-folder`'s dialog.open below always answers this path, so this is
  // the one shape worth previewing: not itself a repository, a mix of
  // already-added names (from PROJECTS, matched by name to exercise the
  // disabled-checkbox state), a plain new one, and one non-`.git` folder.
  inspect_folder: () => {
    const already = new Set(PROJECTS.map((p) => p.name));
    const names = ["docmap.nvim", "lib.nvim", "sandbox.nvim", "new-plugin.nvim", "notes"];
    return {
      isGit: false,
      subrepos: names.map((name) => ({
        name,
        path: `E:/repos/${name}`,
        isGit: name !== "notes",
        alreadyAdded: already.has(name)
      }))
    };
  },
  // The Rules tab, over a seeded ruleset: three families in two files, most
  // rules manual (as in a real ruleset), two with a predicate so the trust
  // row and its dialog have something to show. `p2` has no rules at all, to
  // look at the message that takes the list's place. Trust lives in the page
  // (`RULES_TRUSTED`), so trusting and revoking can be clicked through.
  rules_catalog: (a) => {
    if (a.id === "p2") {
      return {
        available: false,
        reason: "no ruleset is configured: name one in the project's .rules.json (rulesets), or set a vault"
      };
    }
    return {
      available: true,
      rulesets: [{
        name: "../ruleset", origin: "project", path: RULESET, trusted: RULES_TRUSTED,
        trust: {
          state: RULES_TRUSTED ? "trusted" : "untrusted", real: RULESET, predicates: 2,
          by: RULES_TRUSTED ? "steve" : null, at: RULES_TRUSTED ? 1794000000000 : null, problem: null
        }
      }],
      gates: [{ name: "release", families: ["D", "L"] }],
      rules: SEEDED_RULES,
      counts: { rules: SEEDED_RULES.length, automated: 5, manual: SEEDED_RULES.length - 5, predicates: 2 },
      errors: [`${RULESET}/style.md:162: rule L-99: invalid agent field (unknown key qustion) -- ignored`],
      warnings: ["project .rules.json: a rulesets entry leaves the project folder; it is used, but its lua_predicate checks do not run, see trust"]
    };
  },
  rules_run: (a) => {
    const asked = String(a.selection || "").split(",");
    const verdict = {
      "D-01": ["fail", [
        { file: "lua/docmap/core/deps.lua", line: 41, text: "require() inside a loop" },
        { file: "lua/docmap/init.lua", line: 7, text: "require() inside a loop" }
      ]],
      "D-04": ["pass", []],
      "L-01": RULES_TRUSTED
        ? ["pass", []]
        : ["error", [{ file: ".", text: "predicate not trusted: the host has not trusted this ruleset (--trust=<folder>)" }]],
      "L-05": RULES_TRUSTED
        ? ["fail", [{ file: "lua/docmap/health.lua", line: 3, text: "no health module" }]]
        : ["error", [{ file: ".", text: "predicate not trusted: the host has not trusted this ruleset (--trust=<folder>)" }]],
      "S-03": ["waived", []]
    };
    return {
      available: true,
      results: asked.filter((id) => verdict[id]).map((id) => ({
        id, family: id.split("-")[0], status: verdict[id][0], findings: verdict[id][1]
      }))
    };
  },
  // The agent half, canned: what a run would send (two requests, one rule that cannot be sent), an ai-desktop that
  // has a key for one cloud backend, one finished run to open, and a run that Start creates and that moves one
  // request further every time its status is read. No model is anywhere near this.
  agent_preview: (a) => stubPreview(a),
  agent_health: () => ({ url: "http://127.0.0.1:8765", status: "ok", backends: { anthropic: true, openai: false, gemini: false } }),
  agent_ignore: (a) => {
    if (a.add) STUB_IGNORED = true;
    return { ignored: STUB_IGNORED, entry: ".rules-proposals/", git: true };
  },
  agent_list: () => STUB_RUNS.map(stubSummary),
  agent_status: (a) => {
    const run = STUB_RUNS.find((r) => r.id === a.runId);
    if (!run) throw { key: "err.agent.noRun", params: { id: a.runId }, message: "No such run: " + a.runId };
    if (run.state === "running") stubAdvance(run);
    return JSON.parse(JSON.stringify(run));
  },
  agent_start: (a) => {
    const ids = String(a.args.selection).split(",");
    const sent = ids.filter((id) => id !== "S-02");
    const local = !/^(claude-|gpt-|gemini-)/.test(a.args.model);
    const run = {
      id: "run-" + (STUB_RUNS.length + 1), title: a.args.title, notes: a.args.notes, root: "E:/repos/docmap.nvim",
      selection: a.args.selection, refresh: false, backend: local ? "ollama" : "anthropic", model: a.args.model, local,
      state: "running", createdMs: 1794700000000, updatedMs: 1794700000000, finishedMs: null,
      batches: [sent.slice(0, 2), sent.slice(2)].filter((b) => b.length).map((rules, n) => ({
        id: "b" + (n + 1), rules, files: 2, bytes: 3000, tokens: 800, key: "k" + n,
        state: n === 0 ? "asking" : "queued", fromCache: false, usage: { promptTokens: 0, completionTokens: 0 }, error: null
      })),
      proposals: [],
      skipped: ids.includes("S-02") ? [{ id: "S-02", status: "manual", reason: "scope matches no file that may be sent (2 denied)" }] : [],
      usage: { promptTokens: 0, completionTokens: 0 }, error: null
    };
    STUB_RUNS.unshift(run);
    return JSON.parse(JSON.stringify(run));
  },
  agent_pause: (a) => stubSet(a.runId, (run) => {
    run.state = "paused";
    for (const b of run.batches) if (b.state === "asking") b.state = "queued";
  }),
  agent_resume: (a) => stubSet(a.runId, (run) => {
    run.state = "running";
    const next = run.batches.find((b) => b.state === "queued");
    if (next) next.state = "asking";
  }),
  agent_cancel: (a) => stubSet(a.runId, (run) => {
    run.state = "cancelled";
    for (const b of run.batches) if (b.state === "asking") b.state = "queued";
  }),
  // Freshness and the three decisions, canned: the compliant proposal of the finished run is outdated (its files
  // changed), the others are current. A decision lands in the run and, for accept and waive, in STUB_JUDGED, which the
  // Rules tab's Agent column and the sidebar read. Nothing is written anywhere.
  agent_freshness: (a) => {
    const run = STUB_RUNS.find((r) => r.id === a.runId);
    return Object.fromEntries((run ? run.proposals : []).map((p) => [p.ruleId, p.status === "compliant" ? "files" : "current"]));
  },
  agent_person: () => "A Person",
  agent_accept: (a) => stubDecide(a, "accepted"),
  agent_waive: (a) => stubDecide(a, "waived"),
  agent_reject: (a) => stubDecide(a, "rejected"),
  rules_judged: () => JSON.parse(JSON.stringify(STUB_JUDGED)),
  rules_trust_preview: () => ({
    path: RULESET, sha256: "3f9a5c0d1e7b42a68c1d0f6e9b7a3c5d2e4f60718293a4b5c6d7e8f9a0b1c2d3",
    files: 2, bytes: 18432, state: RULES_TRUSTED ? "trusted" : "untrusted",
    predicates: SEEDED_RULES.filter((r) => r.check === "lua_predicate").map((r) => ({
      id: r.id, title: r.title, file: r.source_file, line: r.source_line
    }))
  }),
  rules_trust_grant: () => { RULES_TRUSTED = true; return null; },
  rules_trust_revoke: () => { RULES_TRUSTED = false; return null; },
  rules_predicate_source: (a) => ({
    id: a.rule, file: `${RULESET}/style.md`, line: 74, truncated: false,
    text: [
      "```rule",
      `id = "${a.rule}",`,
      'severity = "recommended",',
      "check = {",
      '  type = "lua_predicate",',
      "  fn = function(root)",
      '    local hits = vim.fn.glob(root .. "/lua/*/health.lua", false, true)',
      "    if #hits > 0 then",
      "      return true",
      "    end",
      '    return false, "no health module"',
      "  end,",
      "},",
      "```"
    ].join("\n")
  }),
  import_many: (a) => ({
    found: (a.roots || []).length,
    added: (a.roots || []).map((r) => ({
      id: r, name: r.split("/").pop(), root: r, map_dir: `${r}/docs/map`,
      exclude: [], languages: null
    })),
    already_present: 0,
    errors: []
  })
};

// The seeded ruleset of the Rules tab (see `R.rules_catalog`).
const RULESET = "E:/repos/shared-rules/lua";
let RULES_TRUSTED = false;
const SEEDED_RULES = [
  ["D-01", "deps.md", "Loading", "grep", "`D-01` — no require() inside a loop"],
  ["D-02", "deps.md", "Loading", null, "`D-02` — a module names every dependency it loads at its top"],
  ["D-03", "deps.md", "Loading", null, "`D-03` — optional dependencies are probed, never required blindly"],
  ["D-04", "deps.md", "Versions", "file_exists", "`D-04` — the lock file is committed"],
  ["D-05", "deps.md", "Versions", null, "`D-05` — a version bump says in its commit what it was tested against, and a very long title like this one has to end in an ellipsis instead of pushing the result columns out of the window"],
  ["L-01", "style.md", "Names", "lua_predicate", "`L-01` — module names match their path"],
  ["L-02", "style.md", "Names", null, "`L-02` — a boolean is named as a question"],
  ["L-03", "style.md", "Names", null, "`L-03` — no abbreviations in public names"],
  ["L-04", "style.md", "Errors", null, "`L-04` — an error names the thing that failed"],
  ["L-05", "style.md", "Errors", "lua_predicate", "`L-05` — a health module exists"],
  ["S-01", "style.md", "Safety", null, "`S-01` — no shell string is built from input"],
  ["S-02", "style.md", "Safety", null, "`S-02` — secrets never reach a log"],
  ["S-03", "style.md", "Safety", "json_key_absent", "`S-03` — no token in a checked-in config"],
  ["S-04", "style.md", "Safety", null, "`S-04` — a path from outside is resolved before it is used"]
].map(([id, file, section, check, title], i) => ({
  id, family: id.split("-")[0], title, section, severity: "recommended",
  text: `The full text of ${id}, as the ruleset states it.\nA second line, so the detail shows how a longer rule wraps.`,
  source_file: `${RULESET}/${file}`, source_line: 10 + i * 12,
  has_check: !!check, check: check || undefined
}));

// The agent runs of the Rules tab (see `R.agent_*`).
let STUB_IGNORED = false;
const STUB_PROPOSALS = {
  violation: (id) => ({
    ruleId: id, batch: "b1", key: "k", status: "violation", error: null, createdMs: 1794600000000,
    proposal: {
      rule_id: id, status: "violation", proposed: true, verified: false,
      findings: [
        { file: "lua/docmap/core/deps.lua", line: 41, quote: 'local mod = require("docmap.core." .. name)', why: "the module is loaded by a name built at run time, so the dependency is not named at the top" }
      ],
      dropped: [{ finding: { file: "lua/docmap/init.lua", line: 7, quote: "require(x)" }, reason: "the quote does not occur near that line" }],
      checked: ["lua/docmap/core/deps.lua", "lua/docmap/init.lua"], unchecked: []
    }
  }),
  compliant: (id) => ({
    ruleId: id, batch: "b1", key: "k", status: "compliant", error: null, createdMs: 1794600000000,
    proposal: {
      rule_id: id, status: "compliant", proposed: true, verified: false, findings: [], dropped: [],
      checked: ["lua/docmap/core/deps.lua"], unchecked: ["lua/docmap/init.lua"]
    }
  }),
  unclear: (id) => ({
    ruleId: id, batch: "b2", key: "k", status: "unclear", error: null, createdMs: 1794600000000,
    proposal: {
      rule_id: id, status: "unclear", proposed: true, verified: false, findings: [], dropped: [], checked: [], unchecked: [],
      reason: "the answer is not the JSON that was asked for", raw: "Looks fine to me, nothing to report."
    }
  })
};
const STUB_KINDS = ["violation", "compliant", "unclear"];
const STUB_RUNS = [{
  id: "run-0", title: "D-02, D-03, L-02 +1 (2026-11-13)", notes: "", root: "E:/repos/docmap.nvim",
  selection: "D-02,D-03,L-02,S-02", refresh: false, backend: "anthropic", model: "claude-example", local: false,
  state: "done", createdMs: 1794600000000, updatedMs: 1794600060000, finishedMs: 1794600060000,
  batches: [
    { id: "b1", rules: ["D-02", "D-03"], files: 2, bytes: 4100, tokens: 1100, key: "k1", state: "done", fromCache: false, usage: { promptTokens: 1180, completionTokens: 240 }, error: null },
    { id: "b2", rules: ["L-02"], files: 1, bytes: 1900, tokens: 500, key: "k2", state: "done", fromCache: true, usage: { promptTokens: 0, completionTokens: 0 }, error: null }
  ],
  proposals: [STUB_PROPOSALS.violation("D-02"), STUB_PROPOSALS.compliant("D-03"), STUB_PROPOSALS.unclear("L-02")],
  skipped: [{ id: "S-02", status: "manual", reason: "scope matches no file that may be sent (2 denied)" }],
  usage: { promptTokens: 1180, completionTokens: 240 }, error: null
}];
function stubSummary(run) {
  return {
    id: run.id, root: run.root, title: run.title, selection: run.selection, backend: run.backend, model: run.model,
    local: run.local, state: run.state, createdMs: run.createdMs, updatedMs: run.updatedMs, finishedMs: run.finishedMs,
    batchesTotal: run.batches.length, batchesDone: run.batches.filter((b) => b.state === "done").length,
    proposals: run.proposals.length, usage: run.usage
  };
}
/** One request further: the one that was asking is done and brings its proposals, the next one asks. */
function stubAdvance(run) {
  const asking = run.batches.find((b) => b.state === "asking");
  if (asking) {
    asking.state = "done";
    asking.usage = { promptTokens: 900, completionTokens: 150 };
    run.usage.promptTokens += 900;
    run.usage.completionTokens += 150;
    asking.rules.forEach((id, n) => run.proposals.push(STUB_PROPOSALS[STUB_KINDS[(run.proposals.length + n) % 3]](id)));
  }
  const next = run.batches.find((b) => b.state === "queued");
  if (next) next.state = "asking";
  else run.state = "done";
}
let STUB_JUDGED = {
  verdicts: {},
  waivers: { "D-03": { reason: "generated code", expires: null, expired: false, partial: false } },
  unreadable: []
};
function stubDecide(a, kind) {
  const run = STUB_RUNS.find((r) => r.id === a.runId);
  const rec = run && run.proposals.find((p) => p.ruleId === a.rule);
  if (!rec) throw { key: "err.agent.noProposal", params: { rule: a.rule }, message: "This run has no proposal about " + a.rule };
  run.decisions = run.decisions || {};
  if (run.decisions[a.rule]) throw { key: "err.agent.decided", params: { rule: a.rule }, message: "decided already" };
  const by = kind === "waived" ? a.args.by : a.by;
  if (!String(by || "").trim()) throw { key: "err.agent.by", params: {}, message: "Name the person who decides" };
  const d = { kind, by, atMs: 1794700000000, verdict: null, note: a.note || null, reason: null, expires: null, paths: [], file: null };
  if (kind === "accepted") {
    if (rec.status !== "violation" && rec.status !== "compliant") {
      throw { key: "err.agent.notAnswer", params: { rule: a.rule }, message: "no answer" };
    }
    d.verdict = rec.status;
    d.file = ".rules-verdicts.json";
    STUB_JUDGED.verdicts[a.rule] = { verdict: rec.status, date: "2026-11-15", by };
  } else if (kind === "waived") {
    if (!String(a.args.reason || "").trim()) throw { key: "err.agent.reason", params: {}, message: "A waiver needs a reason" };
    if (STUB_JUDGED.waivers[a.rule]) {
      throw { key: "err.agent.refused", params: {}, detail: `${a.rule} is waived already: renew it to change the expiry or the reason, or remove it first`, message: "refused" };
    }
    Object.assign(d, { reason: a.args.reason, expires: a.args.expires, paths: a.args.paths, file: ".rules-waivers.json" });
    STUB_JUDGED.waivers[a.rule] = { reason: a.args.reason, expires: a.args.expires, expired: false, partial: a.args.paths.length > 0 };
  }
  run.decisions[a.rule] = d;
  return JSON.parse(JSON.stringify(run));
}
function stubSet(runId, change) {
  const run = STUB_RUNS.find((r) => r.id === runId);
  change(run);
  return JSON.parse(JSON.stringify(run));
}
function stubPreview(a) {
  const ids = String(a.selection || "").split(",").filter(Boolean);
  const sent = ids.filter((id) => id !== "S-02");
  const model = String(a.model || "").trim();
  const cloud = /^(claude-|gpt-|gemini-)/.test(model);
  const batches = [sent.slice(0, 2), sent.slice(2)].filter((b) => b.length).map((rules, n) => ({
    id: "b" + (n + 1), rules: rules.map((id) => ({ id, severity: "recommended", title: id, source: "family" })),
    files: n === 0 ? ["lua/docmap/core/deps.lua", "lua/docmap/init.lua"] : ["lua/docmap/core/deps.lua"],
    bytes: n === 0 ? 4100 : 1900, tokens: n === 0 ? 1100 : 500
  }));
  return {
    destination: model ? { backend: cloud ? "anthropic" : "ollama", model, local: !cloud } : null,
    refused: model ? null : { key: "err.agent.model", params: {}, message: "A model is required: its name decides which backend answers" },
    models: ["claude-example", "qwen-example"],
    batches,
    skipped: ids.includes("S-02") ? [{ id: "S-02", status: "manual", reason: "scope matches no file that may be sent (2 denied)" }] : [],
    totals: { requests: batches.length, files: batches.reduce((n, b) => n + b.files.length, 0), bytes: batches.reduce((n, b) => n + b.bytes, 0), tokens: batches.reduce((n, b) => n + b.tokens, 0) },
    extra: {}, ignore: { ignored: STUB_IGNORED, entry: ".rules-proposals/", git: true }
  };
}

window.__stubListeners = {};
window.__stubEmit = (name, payload) => {
  for (const cb of window.__stubListeners[name] || []) cb({ payload });
};
window.__TAURI__ = {
  core: {
    invoke: async (cmd, args) => {
      if (R[cmd]) return R[cmd](args || {});
      console.warn("[stub] unhandled command:", cmd, args);
      return null;
    },
    // The map page itself is not worth stubbing faithfully; `map-stub.html`
    // only has to run and say so (see its header for why).
    convertFileSrc: () => "/tools/preview/map-stub.html"
  },
  // Always the same directory: enough to preview `inspect_folder`'s
  // checklist path (see `R.inspect_folder`) without a real filesystem.
  dialog: { open: async () => "E:/repos", save: async () => null },
  event: {
    listen: async (name, cb) => {
      (window.__stubListeners[name] ||= []).push(cb);
      return () => {};
    }
  }
};
