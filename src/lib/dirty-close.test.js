import { test } from "node:test";
import assert from "node:assert/strict";

import { guardDirty } from "./dirty-close.js";

/** A dialog with just the events and the close() the guard uses. */
function fakeDialog() {
  const on = {};
  return {
    closed: 0,
    closedBy: undefined,
    addEventListener(type, fn) {
      (on[type] ??= []).push(fn);
    },
    close() {
      this.closed++;
      for (const fn of on.close ?? []) fn({});
    },
    fire(type, props = {}) {
      const e = { type, isTrusted: true, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, ...props };
      for (const fn of on[type] ?? []) fn(e);
      return e;
    },
  };
}

test("a clean dialog closes on cancel without a question", () => {
  const d = fakeDialog();
  let asked = 0;
  guardDirty(d, { ask: () => (asked++, false), closedBy: false });
  const e = d.fire("cancel");
  assert.equal(e.defaultPrevented, false);
  assert.equal(asked, 0);
});

test("typed input makes cancel ask, and a no keeps the dialog open", () => {
  const d = fakeDialog();
  let asked = 0;
  guardDirty(d, { ask: () => (asked++, false), closedBy: false });
  d.fire("input");
  const e = d.fire("cancel");
  assert.equal(e.defaultPrevented, true);
  assert.equal(asked, 1);
  assert.equal(d.closed, 0);
});

test("a yes closes it, and the next opening starts clean", () => {
  const d = fakeDialog();
  const g = guardDirty(d, { ask: () => true, closedBy: false });
  d.fire("input");
  d.fire("cancel");
  assert.equal(d.closed, 1);
  assert.equal(g.dirty(), false);
});

test("a value the program sets is not input", () => {
  const d = fakeDialog();
  const g = guardDirty(d, { ask: () => false, closedBy: false });
  d.fire("input", { isTrusted: false });
  assert.equal(g.dirty(), false);
});

test("hasInput: typing and then deleting it again is not dirty", () => {
  const d = fakeDialog();
  let text = "";
  const g = guardDirty(d, { ask: () => false, hasInput: () => text !== "", closedBy: false });
  text = "a";
  d.fire("input");
  assert.equal(g.dirty(), true);
  text = "";
  assert.equal(g.dirty(), false);
});

test("with closedby the dialog says none while dirty and answers Escape itself", () => {
  const d = fakeDialog();
  let asked = 0;
  guardDirty(d, { ask: () => (asked++, true), closedBy: true });
  d.fire("input");
  assert.equal(d.closedBy, "none");
  const e = d.fire("keydown", { key: "Escape" });
  assert.equal(e.defaultPrevented, true);
  assert.equal(asked, 1);
  assert.equal(d.closed, 1);
  assert.equal(d.closedBy, "closerequest");
});

test("without closedby support the property is never set", () => {
  const d = fakeDialog();
  guardDirty(d, { ask: () => true, closedBy: false });
  d.fire("input");
  assert.equal(d.closedBy, undefined);
});

// The `close` event comes as a task of its own after close(). A dialog that was closed clean, opened again and typed
// into before that event ran must still ask: the late event is not the end of this opening (flake of web-rules.mjs,
// fbbox and prefsbox under load).
test("a late close event of the earlier opening does not make the new opening clean", () => {
  const d = fakeDialog();
  d.open = true;
  const g = guardDirty(d, { ask: () => false, closedBy: false });
  d.open = false; // the earlier opening was closed, its close event is still queued
  d.open = true; // opened again ...
  d.fire("input"); // ... and typed into
  d.fire("close"); // the queued event arrives only now
  assert.equal(g.dirty(), true);
  const e = d.fire("cancel");
  assert.equal(e.defaultPrevented, true);
});

test("the close event of a dialog that stays closed makes it clean", () => {
  const d = fakeDialog();
  d.open = true;
  const g = guardDirty(d, { ask: () => false, closedBy: false });
  d.fire("input");
  d.open = false;
  d.fire("close");
  assert.equal(g.dirty(), false);
});
