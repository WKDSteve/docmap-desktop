import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// The window takes its palette, type and shapes from appkit-ui's tokens
// (a pinned copy, see tools/appkit-ui.pin). Whether the copy is the kit's is
// tools/check-appkit-tokens.py's question; these tests ask whether the
// window actually uses it, which no Python check reads.

const read = (rel) => readFileSync(new URL(rel, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const HTML = read("../index.html");
const CSS = read("../style.css");
const KIT = read("../appkit-tokens.css");
const PALETTE = read("../palette.css");
const CI = read("../../.github/workflows/ci.yml");

const stripComments = (css) => css.replace(/\/\*[\s\S]*?\*\//g, "");

test("index.html loads the tokens, the palette, their fallback, then the window's rules", () => {
  const links = [...HTML.matchAll(/<link rel="stylesheet" href="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(links, ["appkit-tokens.css", "palette.css", "appkit-tokens-fallback.css", "style.css"]);
});

test("the palette sets only tokens of the kit, so a name in it never goes unread", () => {
  // palette.css is generated from the colours of the icon by palettes.mjs, the tool of the app icons.
  const names = (css) => new Set([...stripComments(css).matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]));
  const kit = names(KIT);
  const set = [...names(PALETTE)];
  assert.ok(set.includes("--accent") && set.includes("--accent-hover"), "the palette sets the accent");
  assert.deepEqual(set.filter((n) => !kit.has(n)), []);
});

test("style.css keeps no palette of its own: its names are aliases of the kit's", () => {
  const css = stripComments(CSS);
  assert.match(css, /--panel:\s*var\(--surface\);/);
  assert.match(css, /--ink:\s*var\(--text\);/);
  // The names the kit already has are not redefined here: a second --bg in
  // this file would quietly win over the tokens in one theme or both.
  for (const name of ["bg", "muted", "line", "accent", "accent-soft", "surface", "text"]) {
    assert.doesNotMatch(css, new RegExp(`--${name}\\s*:`), `--${name} belongs to the tokens`);
  }
  // The duplicated theme blocks live in the generated fallback now.
  assert.doesNotMatch(css, /prefers-color-scheme/);
  assert.doesNotMatch(css, /\[data-theme=/);
});

test("every token style.css reads is the kit's or its own alias", () => {
  const own = new Set([...stripComments(CSS).matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]));
  const kit = new Set([...stripComments(KIT).matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]));
  const unknown = [...new Set([...CSS.matchAll(/var\((--[\w-]+)/g)].map((m) => m[1]))].filter(
    (name) => !own.has(name) && !kit.has(name)
  );
  // --sb-w and --v are set from main.js / inline styles, with a default at
  // the place they are read.
  assert.deepEqual(unknown.filter((n) => !["--sb-w", "--v"].includes(n)), []);
});

test("the meaning colours of the kit stay unused: this window says a state in words", () => {
  assert.doesNotMatch(CSS, /var\(--(ok|warn|bad)\)/);
});

test("every focus ring is the kit's --focus, not the accent", () => {
  const rings = [...stripComments(CSS).matchAll(/outline:\s*\d+px\s+solid\s+([^;]+);/g)].map((m) => m[1].trim());
  assert.ok(rings.length >= 5, `expected the focus rings, found ${rings.length}`);
  assert.deepEqual(rings.filter((c) => c !== "var(--focus)"), []);
});

test("the body takes the kit's type", () => {
  assert.match(stripComments(CSS), /font:\s*var\(--font-size\)\s*\/\s*var\(--line-height\)\s*var\(--font\);/);
});

test("--mono is the bundled face in front of the kit's own stack", () => {
  const kitMono = KIT.match(/--mono:\s*([^;]+);/)[1].trim();
  const ours = stripComments(CSS).match(/--mono:\s*([^;]+);/)[1].trim();
  assert.equal(ours, `"JetBrains Mono", ${kitMono}`);
});

test("CI checks the copy against the pinned tag and the colours against the tokens", () => {
  // `python`, not `python3`: the runner is a Windows machine, where python3 is the Store stub.
  assert.match(CI, /python tools\/check-appkit-tokens\.py\s*$/m);
  assert.match(CI, /run: python tools\/check-colors\.py\s*$/m);
  assert.match(CI, /run: python -m unittest discover -s tools -p "test_\*\.py"\s*$/m);
});
