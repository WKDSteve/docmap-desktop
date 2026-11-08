import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { MAIN_RS } from "./rust-main.js";

// A dialog's own action has to be on screen when the dialog opens.
//
// `<dialog>` caps itself at the viewport, and three of these are taller than
// the window's minimum height (600, `tauri.conf.json`), so they scroll.
// Measured in a browser at 900x600 before the rule below existed: the
// project-settings dialog wanted 715px, got 564, and put **Save** 149px
// under the fold — reachable only by scrolling a box whose most obvious
// wheel targets (the language grid, the exclude textarea) scroll something
// else. This is what the tests could not see and a window could.
//
// Asserted from the sources rather than from a rendered page, like the
// other tests here: what has to hold is that every dialog's last-word
// button is *covered by the rule*, and both halves of that live in text.

const HTML = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const CSS = readFileSync(new URL("../style.css", import.meta.url), "utf8");

/** The dialogs, as `{ id, body }`, from `<dialog id="…" …> … </dialog>`. */
function dialogs() {
  return [...HTML.matchAll(/<dialog id="([a-z]+)"[^>]*>([\s\S]*?)<\/dialog>/g)].map((m) => ({
    id: m[1],
    body: m[2],
  }));
}

/** The rule that pins a dialog's own action, as written in the stylesheet. */
function stickyRule() {
  const m = CSS.match(/\.addbox > \.addbox-go \{([\s\S]*?)\}/);
  return m ? m[1] : null;
}

test("the dialogs are found at all", () => {
  const found = dialogs();
  assert.ok(found.length >= 5, `expected the app's dialogs, found ${found.length}`);
  assert.ok(
    found.some((d) => d.id === "scopebox"),
    "the project-settings dialog is the one this rule was written for"
  );
});

test("a dialog's own action is pinned rather than scrolled to", () => {
  const rule = stickyRule();
  assert.ok(rule, "style.css must carry a `.addbox > .addbox-go` rule");
  assert.match(rule, /position:\s*sticky/, "the action has to stay in view while the dialog scrolls");
  assert.match(rule, /bottom:\s*0/, "measured against the scrollport, which includes the padding");
  // Without a block box there is nothing for `sticky` to hold in place: a
  // button is inline by default, and an inline sticky box is not the same
  // promise.
  assert.match(rule, /display:\s*block/);
});

test("only the dialog's own action is pinned, not every button that shares the class", () => {
  // `pick-folder` sits inside a pane and `ws-create` inside a button row;
  // both are `.addbox-go` and both belong exactly where they are. The rule
  // is a child selector for that reason, and this is the assertion that
  // notices if it is ever loosened to a descendant one.
  assert.ok(!/\.addbox \.addbox-go \{/.test(CSS), "the rule must stay a direct-child selector");

  const nested = dialogs().filter((d) =>
    /<div[^>]*>[\s\S]*class="addbox-go"[\s\S]*<\/div>/.test(d.body)
  );
  assert.ok(
    nested.length >= 1,
    "at least one dialog has an `.addbox-go` that is not its last word — that is what the selector protects"
  );
});

// The grammar diagnosis crosses three files by string equality alone: a
// command name, two element ids and four catalog keys. Every one of them
// fails the same way when it drifts — silently, in the running window.

const MAIN = readFileSync(new URL("../main.js", import.meta.url), "utf8");
const RUST = MAIN_RS;

test("the grammar_dir command is defined and registered", () => {
  assert.ok(MAIN.includes('invoke("grammar_dir")'), "main.js should ask for it");
  assert.ok(RUST.includes("fn grammar_dir("), "grammar_dir should be defined");
  // Defined is not enough: an unregistered command compiles and fails at
  // the call.
  assert.ok(/^\s*(?:[a-z_]+::)?grammar_dir,$/m.test(RUST), "grammar_dir should be in the invoke_handler list");
});

test("both places the diagnosis is written to exist in the markup", () => {
  for (const id of ["engine-grammars", "prefs-grammars-state"]) {
    assert.ok(HTML.includes(`id="${id}"`), `the markup should define #${id}`);
    assert.ok(MAIN.includes(`"${id}"`), `main.js should write to #${id}`);
  }
});

test("every field grammarDiagnosis reads is one the Rust struct serialises", () => {
  // `GrammarDir` is `Serialize` with default naming, so the JSON keys are
  // the field names. A renamed field would leave the diagnosis reading
  // `undefined` and quietly reporting "no directory configured" on a
  // machine that has one.
  for (const field of ["dir", "from_setting", "exists", "files", "more"]) {
    assert.ok(
      new RegExp("^[ \\t]*" + field + ":", "m").test(RUST),
      `GrammarDir should carry ${field}`
    );
  }
});
