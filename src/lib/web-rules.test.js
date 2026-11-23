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
          media: stack.filter((s) => s.startsWith("@media")),
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
