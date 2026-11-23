import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, statSync } from "node:fs";

// A `@font-face` pointing at a file that is not there fails the way this
// project keeps getting caught by: silently. The browser falls through to
// the next name in the stack, the window looks very slightly different, and
// nothing anywhere says the bundled font is missing. So the declaration and
// the files are compared to each other.

const CSS = readFileSync(new URL("../style.css", import.meta.url), "utf8");

function declaredSources() {
  return [...CSS.matchAll(/src:\s*url\("([^"]+)"\)/g)].map((m) => m[1]);
}

test("every font the stylesheet declares is actually shipped", () => {
  const sources = declaredSources();
  assert.ok(sources.length >= 2, `expected the bundled faces, found ${sources.length}`);
  for (const src of sources) {
    const path = new URL("../" + src, import.meta.url);
    const size = statSync(path).size;
    // A zero-byte file is what a failed copy leaves behind, and it loads as
    // "no font" rather than as an error.
    assert.ok(size > 10_000, `${src} is ${size} bytes, which is not a font`);
  }
});

test("the licence the fonts require is shipped beside them", () => {
  // SIL OFL 1.1 permits redistribution only with the licence included.
  // Shipping the font and dropping this file is a licence violation that
  // nothing else in the build would notice.
  const text = readFileSync(new URL("../fonts/OFL.txt", import.meta.url), "utf8");
  assert.match(text, /SIL Open Font License/, "OFL.txt must be the actual licence");
  assert.match(text, /JetBrains Mono/, "…and must name the font it covers");
});

test("the mono token names the bundled family first, and still has a fallback", () => {
  const token = CSS.match(/--mono:\s*([^;]+);/);
  assert.ok(token, "--mono must be defined — two rules referenced it before it was");
  assert.match(token[1].trim(), /^"JetBrains Mono"/, "the bundled face comes first");
  assert.match(token[1], /monospace\s*$/, "a generic fallback stays at the end");
});

test("nothing hardcodes a monospace stack around the token", () => {
  // One answer to "which monospace face does this window use". Three rules
  // carried their own copy before this, so the token and the rules could
  // disagree.
  const stacks = [...CSS.matchAll(/font-family:\s*ui-monospace[^;]*;/g)];
  assert.deepEqual(
    stacks.map((m) => m[0]),
    [],
    "use var(--mono) rather than repeating the stack"
  );
});

test("every @font-face swaps instead of hiding the text while the file loads", () => {
  const faces = CSS.match(/@font-face\s*\{[^}]*\}/g) ?? [];
  assert.ok(faces.length >= 2, "expected the bundled faces");
  for (const face of faces) assert.match(face, /font-display:\s*swap\s*;/, face);
});
