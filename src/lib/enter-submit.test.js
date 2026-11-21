import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { imeComposing, onEnterSubmit } from "./enter-submit.js";

/** A keydown as a browser reports it, on a target the handler is attached to. */
function press(target, init) {
  const ev = Object.assign(new Event("keydown", { cancelable: true }), init);
  target.dispatchEvent(ev);
  return ev;
}

test("a plain Enter submits and is consumed", () => {
  const el = new EventTarget();
  let n = 0;
  onEnterSubmit(el, () => n++);
  const ev = press(el, { key: "Enter", isComposing: false, keyCode: 13 });
  assert.equal(n, 1);
  assert.equal(ev.defaultPrevented, true);
});

test("other keys do not submit", () => {
  const el = new EventTarget();
  let n = 0;
  onEnterSubmit(el, () => n++);
  press(el, { key: "a", isComposing: false, keyCode: 65 });
  assert.equal(n, 0);
});

test("Enter during a composition (compositionstart, isComposing) does not submit", () => {
  const el = new EventTarget();
  let n = 0;
  onEnterSubmit(el, () => n++);
  el.dispatchEvent(new Event("compositionstart"));
  press(el, { key: "Enter", isComposing: true, keyCode: 229 });
  assert.equal(n, 0);
});

test("Safari order: compositionend first, the confirming Enter reports keyCode 229", () => {
  const el = new EventTarget();
  let n = 0;
  onEnterSubmit(el, () => n++);
  el.dispatchEvent(new Event("compositionstart"));
  el.dispatchEvent(new Event("compositionend"));
  press(el, { key: "Enter", isComposing: false, keyCode: 229 });
  assert.equal(n, 0);
  // The next Enter, after the composition, submits.
  press(el, { key: "Enter", isComposing: false, keyCode: 13 });
  assert.equal(n, 1);
});

test("the confirming Enter is consumed too, so it neither submits implicitly nor adds a newline", () => {
  const el = new EventTarget();
  onEnterSubmit(el, () => {});
  const ev = press(el, { key: "Enter", isComposing: true, keyCode: 229 });
  assert.equal(ev.defaultPrevented, true);
});

test("imeComposing reads both signals", () => {
  assert.equal(imeComposing({ isComposing: true, keyCode: 13 }), true);
  assert.equal(imeComposing({ isComposing: false, keyCode: 229 }), true);
  assert.equal(imeComposing({ isComposing: false, keyCode: 13 }), false);
});

// main.js is not unit-tested, so its handlers are held to the rule by reading
// the source: every Enter that submits a text field goes through the helper.
test("every Enter-submit handler in main.js is IME-safe", () => {
  const lines = readFileSync(new URL("../main.js", import.meta.url), "utf8").replace(/\r\n/g, "\n").split("\n");
  const bad = [];
  lines.forEach((line, i) => {
    if (!/key === "Enter"/.test(line)) return;
    // Ctrl/Cmd+Enter does not collide with the IME; makeOpenable's keys act on
    // a role=button list item, not on a text field.
    if (/ctrlKey|metaKey|key === " "/.test(line)) return;
    if (!/imeComposing/.test(lines.slice(i, i + 4).join("\n"))) bad.push(i + 1);
  });
  assert.deepEqual(bad, [], `Enter handlers without an imeComposing guard at main.js lines ${bad}`);
  assert.match(lines.join("\n"), /onEnterSubmit\(addbox\.url/);
  assert.match(lines.join("\n"), /onEnterSubmit\(finder\.path/);
  assert.match(lines.join("\n"), /onEnterSubmit\(docSearch\.q/);
});
