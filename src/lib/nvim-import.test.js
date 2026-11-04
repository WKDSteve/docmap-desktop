import { test } from "node:test";
import assert from "node:assert/strict";

import { setLocale } from "./i18n.js";
import { nvimImportSummary } from "./nvim-import.js";

setLocale("en");

test("nothing found says so instead of '0 of 0 added'", () => {
  const s = nvimImportSummary({ found: 0, added: [], already_present: 0, errors: [] });
  assert.match(s.text, /lists no project/);
  assert.equal(s.changed, false);
});

test("everything already present is its own sentence", () => {
  const s = nvimImportSummary({ found: 5, added: [], already_present: 5, errors: [] });
  assert.match(s.text, /Nothing new: all 5/);
  assert.equal(s.changed, false);
});

test("a partial add counts added, found and present", () => {
  const s = nvimImportSummary({
    found: 4,
    added: [{ id: "a" }, { id: "b" }],
    already_present: 2,
    errors: [],
  });
  assert.match(s.text, /2 of 4 .*\(2 already in the list\)/);
  assert.equal(s.changed, true);
});

test("failures are named with their reason", () => {
  const s = nvimImportSummary({
    found: 2,
    added: [],
    already_present: 1,
    errors: [{ name: "gone.nvim", error: { key: "err.path.notDirectory", params: { path: "E:/gone" } } }],
  });
  assert.match(s.text, /0 of 2/);
  assert.match(s.text, /1 could not be added: gone\.nvim \(/);
  assert.match(s.text, /E:\/gone/);
  assert.equal(s.changed, false);
});

test("German has every sentence", () => {
  setLocale("de");
  try {
    assert.match(nvimImportSummary({ found: 0, added: [], errors: [] }).text, /kein Projekt/);
    assert.match(nvimImportSummary({ found: 3, added: [], already_present: 3, errors: [] }).text, /Nichts Neues/);
  } finally {
    setLocale("en");
  }
});
