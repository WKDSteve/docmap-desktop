import { test } from "node:test";
import assert from "node:assert/strict";

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import {
  STALE_DAYS,
  isStale,
  ageInDays,
  trend,
  windows,
  lineFor,
  hasNumbers,
  compareTraffic,
  githubUrl,
} from "./traffic.js";
import { MAIN_RS } from "./rust-main.js";

const here = fileURLToPath(new URL(".", import.meta.url));
const MAIN = readFileSync(here + "../main.js", "utf8");
const HTML = readFileSync(here + "../index.html", "utf8");

const DAY = 86_400_000;
const NOW = Date.parse("2026-09-28T12:00:00Z");

// The fixtures use the wire names `traffic.rs` serialises (camelCase for the
// list entries and the summary, snake_case for the status). Getting one wrong
// is silent — the field reads `undefined` and the project looks unknown.
const entry = (o = {}) => ({ id: "x", status: "ok", views30: 10, clones30: 4, ...o });

// ---------------------------------------------------------------- staleness

test("a digest is stale only after the threshold, and never for a value that does not parse", () => {
  assert.equal(isStale("2026-09-28T00:00:00Z", NOW), false);
  assert.equal(isStale(new Date(NOW - STALE_DAYS * DAY).toISOString(), NOW), false, "exactly at the limit");
  assert.equal(isStale(new Date(NOW - STALE_DAYS * DAY - 1000).toISOString(), NOW), true);
  // Not knowing how old it is is not a reason to call it old.
  for (const bad of [null, undefined, "", "yesterday", 12345, {}]) {
    assert.equal(isStale(bad, NOW), false, String(bad));
    assert.equal(ageInDays(bad, NOW), null, String(bad));
  }
});

test("age is in whole days and never negative", () => {
  assert.equal(ageInDays("2026-09-25T12:00:00Z", NOW), 3);
  assert.equal(ageInDays("2026-09-28T11:00:00Z", NOW), 0);
  assert.equal(ageInDays("2026-10-05T12:00:00Z", NOW), 0, "a clock ahead of ours");
});

// -------------------------------------------------------------------- trend

test("a trend is an arrow and a signed percentage", () => {
  assert.deepEqual(trend(12.5, "en"), { arrow: "↑", text: "+12.5 %" });
  assert.deepEqual(trend(-3, "en"), { arrow: "↓", text: "-3.0 %" });
  assert.deepEqual(trend(100, "en"), { arrow: "↑", text: "+100.0 %" });
});

test("a flat trend is flat, and an unknown one is empty rather than zero", () => {
  assert.deepEqual(trend(0.2, "en"), { arrow: "→", text: "±0 %" });
  assert.deepEqual(trend(-0.49, "en"), { arrow: "→", text: "±0 %" });
  assert.deepEqual(trend(0.5, "en").arrow, "↑");
  for (const unknown of [null, undefined, NaN, Infinity, "12", {}]) {
    assert.deepEqual(trend(unknown, "en"), { arrow: "", text: "" }, String(unknown));
  }
});

test("the trend follows the reader's number format", () => {
  assert.equal(trend(12.5, "de").text, "+12,5 %");
});

// ------------------------------------------------------------------ windows

test("the three windows read as one line, in the reader's number format", () => {
  const m = { d7: { count: 4 }, d30: { count: 1234 }, d90: { count: 1234567 } };
  assert.equal(windows(m, "count", "en"), "4 / 1,234 / 1,234,567");
  assert.equal(windows(m, "count", "de"), "4 / 1.234 / 1.234.567");
});

test("a window that is not a count is a dash, not a number it is not", () => {
  assert.equal(windows({ d7: { count: 4 }, d30: {}, d90: { count: -1 } }, "count", "en"), "4 / – / –");
  assert.equal(windows(null, "count", "en"), "– / – / –");
  assert.equal(windows({ d7: { count: "many" } }, "count", "en"), "– / – / –");
  assert.equal(windows({ d7: { uniques: 3 }, d30: { uniques: 5 }, d90: { uniques: 9 } }, "uniques", "en"), "3 / 5 / 9");
});

// -------------------------------------------------------------- what is shown

test("only four outcomes get a line; the other three say nothing", () => {
  assert.deepEqual(lineFor({ status: "ok" }), { key: "traffic.line", vars: {} });
  assert.deepEqual(lineFor({ status: "not_tracked", repo: "o/n" }), {
    key: "traffic.notTracked",
    vars: { repo: "o/n" },
  });
  assert.deepEqual(lineFor({ status: "unreadable", message: "not JSON" }), {
    key: "traffic.unreadable",
    vars: { reason: "not JSON" },
  });
  assert.deepEqual(lineFor({ status: "newer_schema", message: "schema 9" }), {
    key: "traffic.newer",
    vars: { reason: "schema 9" },
  });
  // What the command actually sends: an error with a key, rendered in the
  // window's language rather than passed on as an object.
  assert.deepEqual(
    lineFor({
      status: "newer_schema",
      message: { key: "err.traffic.schema", params: { schema: "9", known: "1" }, detail: null },
    }),
    { key: "traffic.newer", vars: { reason: "Schema 9, this app understands up to 1" } }
  );
  assert.deepEqual(
    lineFor({
      status: "unreadable",
      message: { key: "err.traffic.digestNotJson", params: {}, detail: "expected value at line 1" },
    }),
    { key: "traffic.unreadable", vars: { reason: "The digest is not JSON — expected value at line 1" } }
  );
  // No GitHub remote is not an error; no plugin behaves as before; opt-out is opt-out.
  for (const quiet of ["no_remote", "no_digest", "disabled", "something_new", undefined]) {
    assert.equal(lineFor({ status: quiet }), null, String(quiet));
  }
  assert.equal(lineFor(null), null);
});

test("every line key exists in both shipped locales", async () => {
  const { t, setLocale } = await import("./i18n.js");
  const keys = ["traffic.line", "traffic.notTracked", "traffic.unreadable", "traffic.newer"];
  for (const code of ["en", "de"]) {
    setLocale(code);
    for (const key of keys) {
      const value = t(key);
      assert.ok(value && value !== key, `${key} is missing from ${code}`);
    }
  }
  setLocale("en");
});

// --------------------------------------------------------------------- order

test("the traffic order: most views first, clones break a tie", () => {
  const list = [
    entry({ id: "low", views30: 5, clones30: 1 }),
    entry({ id: "high", views30: 90, clones30: 1 }),
    entry({ id: "tie-more-clones", views30: 5, clones30: 9 }),
  ];
  list.sort(compareTraffic);
  assert.deepEqual(
    list.map((e) => e.id),
    ["high", "tie-more-clones", "low"]
  );
});

test("a project with no numbers sorts after every project that has them, zero included", () => {
  const zero = entry({ id: "zero", views30: 0, clones30: 0 });
  const unknown = [
    { id: "no-remote", status: "no_remote" },
    { id: "untracked", status: "not_tracked" },
    { id: "unreadable", status: "unreadable" },
    { id: "hidden", status: "disabled" },
    undefined,
  ];
  for (const u of unknown) {
    assert.equal(hasNumbers(u), false);
    assert.equal(compareTraffic(zero, u), -1, `0 views outranks ${u && u.status}`);
    assert.equal(compareTraffic(u, zero), 1);
  }
  // Two unknowns tie, so the caller's name order decides.
  assert.equal(compareTraffic(unknown[0], unknown[1]), 0);
  assert.equal(compareTraffic(unknown[0], undefined), 0);
});

test("a missing clones count ties as zero rather than breaking the sort", () => {
  const a = entry({ id: "a", views30: 3, clones30: undefined });
  const b = entry({ id: "b", views30: 3, clones30: 2 });
  assert.equal(compareTraffic(a, b), 2);
  assert.equal(hasNumbers({ status: "ok", views30: NaN }), false);
});

// --------------------------------------------------------------------- links

test("a link is built only from a plain owner/name, always to github.com", () => {
  assert.equal(githubUrl("WKDSteve/docmap-desktop"), "https://github.com/WKDSteve/docmap-desktop");
  assert.equal(githubUrl("o/n.nvim"), "https://github.com/o/n.nvim");
  for (const bad of [
    null,
    undefined,
    "",
    "owner",
    "owner/",
    "/name",
    "o/n/extra",
    "o/..",
    "o/.",
    "-o/n",
    "o w/n",
    "javascript:alert(1)//x/y",
    "https://evil.example/o/n",
    "o/<script>",
    "o/n\n",
    "o/" + "x".repeat(101),
  ]) {
    assert.equal(githubUrl(bad), null, String(bad));
  }
});

// ---------------------------------------------- structural guards on main.js

test("no user-derived string is written with innerHTML in the traffic code", () => {
  // Everything the digest carries is text from outside — a referrer is
  // whatever a website sent. The traffic section builds its nodes with
  // textContent; this guards the section, not a list of today's strings.
  const start = MAIN.indexOf("// GitHub traffic");
  assert.ok(start > 0, "the traffic section of main.js should be marked");
  // The section's own header closes with a rule line; the next rule line
  // after that opens whatever comes next.
  const closing = MAIN.indexOf("\n// ====", start);
  const end = MAIN.indexOf("\n// ====", closing + 10);
  const section = MAIN.slice(start, end > start ? end : undefined);
  assert.ok(section.length > 3000, "expected the traffic section's code, found " + section.length);
  assert.match(section, /async function renderTraffic\(/, "the section must contain the code, not just its header");
  assert.doesNotMatch(section, /\.innerHTML\s*=/, "the traffic section must not assign innerHTML");
  assert.doesNotMatch(section, /insertAdjacentHTML/, "nor insertAdjacentHTML");
});

test("the fifth sort order is offered and handled", () => {
  assert.match(HTML, /<option value="traffic" data-i18n="sort\.traffic">/);
  assert.match(MAIN, /sortBy === "traffic"/);
  assert.match(MAIN, /compareTraffic/);
});

test("the traffic commands the window calls are the ones the backend registers", () => {
  const rust = MAIN_RS;
  const called = new Set([...MAIN.matchAll(/invoke\("(traffic_[a-z_]+)"/g)].map((m) => m[1]));
  assert.ok(called.size >= 5, `expected the traffic calls, found ${called.size}`);
  for (const name of called) {
    assert.match(rust, new RegExp(`\\b${name},`), `${name} is called but not registered in generate_handler!`);
  }
});

// ---------------------------------------------------------------- overview

test("the overview shows a figure only for a project that has numbers", () => {
  const start = MAIN.indexOf("async function fillOverviewTraffic");
  assert.ok(start > 0, "fillOverviewTraffic should exist");
  const body = MAIN.slice(start, MAIN.indexOf("\n}\n", start));
  // The gate is the same predicate the sort uses, so "has numbers" means one
  // thing in both places, and no remote / no plugin / not tracked add nothing.
  assert.match(body, /!hasNumbers\(entry\)\) continue/);
  assert.doesNotMatch(body, /\.innerHTML\s*=/);
});

test("the overview's traffic keys exist in both shipped locales, translated", async () => {
  const { t, setLocale } = await import("./i18n.js");
  for (const key of ["ov.traffic", "ov.trafficTitle"]) {
    const seen = {};
    for (const code of ["en", "de"]) {
      setLocale(code);
      seen[code] = t(key);
      assert.ok(seen[code] && seen[code] !== key, `${key} is missing from ${code}`);
    }
    assert.notEqual(seen.en, seen.de, `${key} was not translated`);
  }
  setLocale("en");
});

test("a row without a figure keeps the layout it always had", () => {
  const css = readFileSync(here + "../style.css", "utf8");
  // The extra grid area exists only under the modifier class.
  assert.match(css, /\.ov-open\.has-traffic\s*\{[^}]*"state traffic"/);
  const base = css.slice(css.indexOf(".ov-open {"), css.indexOf("}", css.indexOf(".ov-open {")));
  assert.doesNotMatch(base, /traffic/, "the base row must not know about traffic");
});

// --------------------------------------------------------- detail dialog

test("the detail dialog's elements main.js reaches for exist in the markup", () => {
  const ids = [...new Set([...MAIN.matchAll(/getElementById\("(traffic(?:box|-[a-z-]*))"\)/g)].map((m) => m[1]))];
  assert.ok(ids.length >= 4, `expected the dialog's elements, found ${ids.length}`);
  const missing = ids.filter((id) => !HTML.includes(`id="${id}"`));
  assert.deepEqual(missing, [], `main.js reads ids the markup does not define: ${missing}`);
});

test("a page title reaches the dialog only through textContent", () => {
  // Same guard as the section-wide one, scoped to the function that builds
  // the dialog's rows — a repository's own path title is text from outside,
  // same as every other string in this section.
  for (const fn of ["renderTrafficPaths"]) {
    const start = MAIN.indexOf(`function ${fn}(`);
    assert.ok(start > 0, `${fn} should exist`);
    const body = MAIN.slice(start, MAIN.indexOf("\n}\n", start));
    assert.match(body, /name\.textContent = /, `${fn} must set the name via textContent`);
    assert.doesNotMatch(body, /\.innerHTML\s*=/, `${fn} must not assign innerHTML`);
  }
});

test("a top-page row is a link only when the Rust side resolved project_path", () => {
  // The wire name is snake_case (`PathItem` carries no `rename_all`) — the
  // same class of bug the file-level comment above warns about, and the one
  // this specific assertion exists to catch: `projectPath` would read
  // `undefined` and silently make every row unclickable.
  const start = MAIN.indexOf("function renderTrafficPaths(");
  const body = MAIN.slice(start, MAIN.indexOf("\n}\n", start));
  assert.match(body, /p\.project_path/, "renderTrafficPaths must read project_path, not projectPath");
  assert.doesNotMatch(body, /p\.projectPath/);
});

test("open_in_editor and traffic_detail are the commands the dialog calls, and both are registered", () => {
  const rust = MAIN_RS;
  const start = MAIN.indexOf("// The traffic detail dialog");
  assert.ok(start > 0, "the detail dialog's section should be marked");
  const section = MAIN.slice(start, MAIN.indexOf("\n// ====", start));
  for (const cmd of ["traffic_detail", "open_in_editor"]) {
    assert.match(section, new RegExp(`invoke\\("${cmd}"`), `the dialog should call ${cmd}`);
    assert.match(rust, new RegExp(`\\b${cmd},`), `${cmd} should be in the invoke_handler list`);
  }
});

test("the detail dialog's keys exist in both shipped locales, translated", async () => {
  const { t, setLocale } = await import("./i18n.js");
  const keys = [
    "traffic.detail.open",
    "traffic.detail.title",
    "traffic.detail.span",
    "traffic.detail.paths",
    "traffic.detail.paths.empty",
    "traffic.detail.count",
    "traffic.detail.openFile",
    "traffic.detail.none",
  ];
  for (const key of keys) {
    const seen = {};
    for (const code of ["en", "de"]) {
      setLocale(code);
      seen[code] = t(key);
      assert.ok(seen[code] && seen[code] !== key, `${key} is missing from ${code}`);
    }
    assert.notEqual(seen.en, seen.de, `${key} was not translated`);
  }
  setLocale("en");
});
