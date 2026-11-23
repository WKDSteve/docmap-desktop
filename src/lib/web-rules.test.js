import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// Rules of the web platform that live in the stylesheet or the markup and
// that nothing else checks: each test reads the file as text.

const CSS = readFileSync(new URL("../style.css", import.meta.url), "utf8");

// Every declaration of `property` with the chain of @media conditions
// around it. Comments are blanked first so a `{` in prose cannot confuse
// the walk.
function declarations(css, property) {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
  const out = [];
  const stack = [];
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === "{") {
      stack.push(text.slice(start, i).trim());
      start = i + 1;
    } else if (ch === "}") {
      stack.pop();
      start = i + 1;
    } else if (ch === ";") {
      const decl = text.slice(start, i).trim();
      if (decl.startsWith(property + ":")) {
        out.push({
          selector: stack[stack.length - 1],
          media: stack.filter((s) => s.startsWith("@media") || s.startsWith("@supports")),
          value: decl.slice(property.length + 1).trim(),
        });
      }
      start = i + 1;
    }
  }
  return out;
}

test("declarations() sees the media chain of a declaration", () => {
  const found = declarations("@media (a) { .x { transition: 1s; } } .y { transition: 2s; }", "transition");
  assert.deepEqual(found.map((d) => d.media.length), [1, 0]);
});

test("no transition runs outside prefers-reduced-motion: no-preference", () => {
  const all = declarations(CSS, "transition").filter((d) => !/^(none|0s?)\b/.test(d.value));
  assert.ok(all.length >= 3, "expected the transitions of the stylesheet");
  for (const d of all) {
    assert.ok(
      d.media.some((m) => /prefers-reduced-motion:\s*no-preference/.test(m)),
      `${d.selector} animates (${d.value}) without prefers-reduced-motion: no-preference`,
    );
  }
});

const HTML = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const MAIN = readFileSync(new URL("../main.js", import.meta.url), "utf8");

test("every <code> in index.html and the brand name carry translate=\"no\"", () => {
  const markup = HTML.replace(/<!--[\s\S]*?-->/g, "");
  const tags = [...markup.matchAll(/<code\b[^>]*>/g)].map((m) => m[0]);
  assert.ok(tags.length >= 6, "expected the <code> elements of the page");
  for (const tag of tags) assert.match(tag, /translate="no"/, tag);
  assert.match(markup, /<button id="brand"[^>]*translate="no"/);
});

test("the <code> elements main.js builds are marked translate=\"no\"", () => {
  // `node("code", …)` marks it in the helper; the one direct createElement
  // sets the attribute on the next line.
  assert.match(MAIN, /if \(tag === "code"\) e\.setAttribute\("translate", "no"\)/);
  const direct = [...MAIN.matchAll(/document\.createElement\("code"\)/g)].length;
  const marked = [...MAIN.matchAll(/document\.createElement\("code"\);\s*\n\s*\w+\.setAttribute\("translate", "no"\)/g)].length;
  assert.equal(marked, direct);
});

test("main.js formats numbers, sizes and dates with the UI language", () => {
  const code = MAIN.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
  assert.doesNotMatch(code, /\.toFixed\(/, "toFixed writes a decimal point whatever the language");
  assert.doesNotMatch(code, /toISOString\(\)\.slice/, "that is the UTC day");
  assert.doesNotMatch(code, /\.toLocale(?:Date|Time)?String\(\)/, "toLocale*String() without the locale");
  assert.doesNotMatch(code, /formatBytes\([^,()]*(?:\([^()]*\))?[^,()]*\)/, "formatBytes without the locale");
});

// px of a font-size value: rem and em count 16 px, as the root does.
const px = (v) => (/rem$|em$/.test(v) ? parseFloat(v) * 16 : /px$/.test(v) ? parseFloat(v) : NaN);
const FIELD = /(^|[\s>,+~])(input|textarea|select)\b|\.fb-field|\.rules-query|\.finder-select/;

test("fields are set at 16px or more", () => {
  const sizes = declarations(CSS, "font-size").filter((d) => FIELD.test(d.selector));
  assert.ok(sizes.length >= 8, "expected the field rules of the stylesheet");
  for (const d of sizes) assert.ok(px(d.value) >= 16, `${d.selector} sets ${d.value}`);
});

test("a base rule sets every text field to 1rem, and .fb-field does not inherit 14px", () => {
  const base = declarations(CSS, "font-size").find((d) => /^input:not\(/.test(d.selector) && /textarea/.test(d.selector));
  assert.equal(base?.value, "1rem");
  const fb = declarations(CSS, "font-size").filter((d) => d.selector === ".fb-field");
  assert.equal(fb[0]?.value, "1rem");
});

test("fields that take a URL, a path, a command or a name switch spellcheck off", () => {
  const markup = HTML.replace(/<!--[\s\S]*?-->/g, "");
  for (const id of ["url-input", "ws-new", "editor-cmd", "repo-filter"]) {
    const tag = markup.match(new RegExp(`<input id="${id}"[^>]*>`))?.[0];
    assert.ok(tag, id);
    assert.match(tag, /spellcheck="false"/, id);
  }
});

test("changing numbers use tabular figures", () => {
  const withNums = declarations(CSS, "font-variant-numeric").filter((d) => d.value === "tabular-nums");
  const selectors = withNums.map((d) => d.selector).join(",\n");
  for (const sel of [".stat-tile .v", ".stats-table .n", ".ov-traffic", ".ov-deps-who", ".rules-count", ".rules-note", ".rules-run-state", ".finder-state", ".run-egress", ".run-go", ".matrix-hit"]) {
    assert.ok(selectors.includes(sel), `${sel} shows changing numbers without tabular-nums`);
  }
});

test("headings balance their lines only where text-wrap is known", () => {
  const found = declarations(CSS, "text-wrap").filter((d) => d.value === "balance");
  assert.ok(found.length, "no text-wrap: balance for the headings");
  for (const d of found) {
    assert.ok(d.media.some((m) => /^@supports\s*\(text-wrap:\s*balance\)/.test(m)), `${d.selector} is not behind @supports`);
    assert.match(d.selector, /h[1-4]/);
  }
});
