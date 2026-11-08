import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { LAST_KEY, lastKey, migrateLastKey } from "./last-selection.js";

/** `localStorage`, minus the browser. */
function store(initial = {}) {
  const data = { ...initial };
  return {
    data,
    getItem: (k) => (k in data ? data[k] : null),
    setItem: (k, v) => {
      data[k] = String(v);
    },
    removeItem: (k) => {
      delete data[k];
    },
  };
}

test("each workspace gets its own key", () => {
  assert.equal(lastKey("Default"), "docmap.lastProject.Default");
  assert.equal(lastKey("Work"), "docmap.lastProject.Work");
  assert.notEqual(lastKey("Default"), lastKey("Work"));
});

test("before a workspace is known, the bare key is the answer", () => {
  // Not a fallback: until `list_workspaces` says otherwise there is exactly
  // one workspace, and the bare key is where its selection already lives.
  assert.equal(lastKey(null), LAST_KEY);
});

test("an installation from before workspaces keeps its selection", () => {
  const s = store({ [LAST_KEY]: "p1" });
  assert.equal(migrateLastKey(s, "Default"), true);
  assert.equal(s.getItem("docmap.lastProject.Default"), "p1");
  assert.equal(s.getItem(LAST_KEY), null, "the legacy key is consumed, not copied");
});

test("migrating never overwrites a workspace's own answer", () => {
  const s = store({ [LAST_KEY]: "legacy", "docmap.lastProject.Work": "mine" });
  assert.equal(migrateLastKey(s, "Work"), false);
  assert.equal(s.getItem("docmap.lastProject.Work"), "mine");
  // Still consumed: leaving it would migrate it into the *next* workspace
  // opened, which is the wrong workspace by construction.
  assert.equal(s.getItem(LAST_KEY), null);
});

test("migrating without a known workspace does nothing at all", () => {
  // Called before `list_workspaces` answers, this would have to guess which
  // workspace the legacy value belongs to. It waits instead.
  const s = store({ [LAST_KEY]: "p1" });
  assert.equal(migrateLastKey(s, null), false);
  assert.equal(s.getItem(LAST_KEY), "p1");
});

test("storage that refuses to answer is a forgetful app, not a broken one", () => {
  const angry = {
    getItem() {
      throw new Error("denied");
    },
    setItem() {
      throw new Error("denied");
    },
    removeItem() {},
  };
  assert.doesNotThrow(() => migrateLastKey(angry, "Default"));
});

// ------------------------------------------------------------------ joins

const MAIN = readFileSync(new URL("../main.js", import.meta.url), "utf8");
const HTML = readFileSync(new URL("../index.html", import.meta.url), "utf8");

test("main.js stores the selection under the active workspace, never bare", () => {
  assert.ok(
    MAIN.includes("lastKey(activeWorkspace), id)"),
    "select() must write under the workspace it is selecting in"
  );
  // The bare constant belongs to the migration now. Its reappearance in
  // main.js would mean a second, workspace-blind path back into storage.
  assert.ok(!/localStorage\.setItem\("docmap\.lastProject"/.test(MAIN));
});

test("switching a workspace restores that workspace's own selection", () => {
  const switcher = MAIN.slice(MAIN.indexOf('invoke("switch_workspace"'));
  const body = switcher.slice(0, switcher.indexOf("} catch"));
  assert.ok(
    body.includes("restoreLast()"),
    "arriving in a workspace should land where it was left"
  );
});

test("the picker can say that nothing is selected", () => {
  // A `<select>` always has one option selected, so with no selection the
  // control named whichever project sorted first while the pane beside it
  // said "nothing selected" and Generate stayed disabled — two answers to
  // one question, in adjacent controls.
  //
  // The option outgrew being a label. No selection now shows the workspace
  // overview, so the row is a destination and is offered whenever there is
  // more than nothing to list — the `!selectedId` guard this used to assert
  // was there to keep an unreachable option out of the open list, and it
  // would now hide the only way back to that screen.
  assert.ok(MAIN.includes('t("picker.overview")'), "the empty state needs its own label");
  assert.ok(
    /if \(view\.projects\.length\) \{[\s\S]{0,120}createElement\("option"\)/.test(MAIN),
    "and it is offered whenever there are projects to survey"
  );
  assert.ok(
    !/none\.disabled = true/.test(MAIN),
    "a destination is choosable — it was disabled only while it led nowhere"
  );
  assert.ok(HTML.includes('id="projects"'), "the picker itself still exists");
});

test("the picker never falls back to what it showed before", () => {
  // The empty-state option above was added and then overruled one line
  // later by a `previous` fallback, so a workspace switch went on naming a
  // project from the workspace just left. The structural test passed
  // throughout; the browser did not. This asserts the fallback stays gone.
  assert.ok(
    /els\.list\.value = view\.selectedId \?\? "";/.test(MAIN),
    "the picker shows the selection or nothing"
  );
  assert.ok(!/const previous = els\.list\.value/.test(MAIN));
});
