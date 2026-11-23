import { test } from "node:test";
import assert from "node:assert/strict";

import { percent, headline, composition, languageRows, formatBytes } from "./stats.js";

const lines = (total, code, comment, blank) => ({ total, code, comment, blank });

/** A small project: 100 lines of Lua, 20 of Markdown, 10 of JSON. */
const STATS = {
  languages: [
    { name: "Lua", kind: "code", files: 4, lines: lines(100, 70, 20, 10), bytes: 4000 },
    { name: "Markdown", kind: "docs", files: 2, lines: lines(20, 15, 0, 5), bytes: 900 },
    { name: "JSON", kind: "data", files: 1, lines: lines(10, 10, 0, 0), bytes: 200 },
  ],
  code: { files: 4, lines: lines(100, 70, 20, 10) },
  docs: { files: 2, lines: lines(20, 15, 0, 5) },
  data: { files: 1, lines: lines(10, 10, 0, 0) },
  other: { files: 3, lines: lines(0, 0, 0, 0) },
  files: 10,
  bytes: 5100,
  largest: [],
  truncated: false,
};

test("a percentage has one decimal and survives an empty whole", () => {
  assert.equal(percent(1, 3), 33.3);
  assert.equal(percent(0, 0), 0);
  assert.equal(percent(5, 0), 0);
});

test("the headline separates code, comments, documentation and data", () => {
  const h = headline(STATS);
  assert.equal(h.files, 10);
  assert.equal(h.lines, 130, "everything that was measured");
  assert.equal(h.code, 70);
  assert.equal(h.comment, 20);
  assert.equal(h.docs, 15, "non-blank documentation lines");
  assert.equal(h.data, 10);
  assert.equal(h.blank, 15, "blank lines are summed across all three kinds");
  assert.equal(h.other, 3, "files that were counted but not measured");
});

test("the comment ratio is over source lines, not over everything", () => {
  // 20 of (70 + 20) source lines; documentation does not dilute it.
  assert.equal(headline(STATS).commentRatio, 22.2);
});

test("the composition leaves out what is not there", () => {
  const noDocs = {
    ...STATS,
    docs: { files: 0, lines: lines(0, 0, 0, 0) },
    data: { files: 0, lines: lines(0, 0, 0, 0) },
  };
  const keys = composition(noDocs).map((p) => p.key);
  assert.deepEqual(keys, ["code", "comment", "blank"]);
});

test("the composition adds up to the whole, within rounding", () => {
  const sum = composition(STATS).reduce((a, p) => a + p.percent, 0);
  assert.ok(Math.abs(sum - 100) < 0.5, `got ${sum}`);
});

test("language rows carry a share of all lines and a width relative to the largest", () => {
  const rows = languageRows(STATS);
  assert.equal(rows[0].name, "Lua");
  assert.equal(rows[0].width, 100, "the biggest fills its cell");
  assert.equal(rows[1].width, 20);
  assert.equal(rows[0].share, 76.9);
});

test("nothing measured gives zeros, not NaN", () => {
  const empty = {
    ...STATS,
    languages: [],
    code: { files: 0, lines: lines(0, 0, 0, 0) },
    docs: { files: 0, lines: lines(0, 0, 0, 0) },
    data: { files: 0, lines: lines(0, 0, 0, 0) },
    files: 0,
  };
  assert.equal(headline(empty).lines, 0);
  assert.equal(headline(empty).commentRatio, 0);
  assert.deepEqual(composition(empty), []);
  assert.deepEqual(languageRows(empty), []);
});

test("sizes read as a number and a unit", () => {
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(2048), "2 KB");
  assert.equal(formatBytes(3 * 1024 * 1024), "3.0 MB");
  assert.equal(formatBytes(-1), "");
});

test("sizes follow the reader's locale", () => {
  assert.equal(formatBytes(3 * 1024 * 1024, "de"), "3,0 MB");
  assert.equal(formatBytes(1536 * 1024 * 1024, "de"), "1,5 GB");
  assert.equal(formatBytes(1500 * 1024, "de"), "1,5 MB");
});
