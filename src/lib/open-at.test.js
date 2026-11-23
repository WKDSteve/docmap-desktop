import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { mapHash, nodeFragment, parseTarget, pickProject, relativePath, resolveAt } from "./open-at.js";

const cases = JSON.parse(readFileSync(new URL("./open-at-cases.json", import.meta.url), "utf8"));

test("paths answer like src-tauri/src/open_at.rs", () => {
  for (const c of cases.paths) assert.equal(relativePath(c.in), c.out, JSON.stringify(c.in));
});

test("targets answer like src-tauri/src/open_at.rs", () => {
  for (const c of cases.targets) {
    const got = parseTarget(c.in);
    if (c.file === null) assert.equal(got, null, c.in);
    else assert.deepEqual(got, { file: c.file, line: c.line, end: c.end }, c.in);
  }
});

test("a project is matched by id, name or root, never made up", () => {
  const projects = [
    { id: "p1", name: "docmap.nvim", root: "E:/repos/docmap.nvim" },
    { id: "p2", name: "twin", root: "E:/a/twin" },
    { id: "p3", name: "Twin", root: "E:/b/twin" },
  ];
  assert.equal(pickProject(projects, "p1").id, "p1");
  assert.equal(pickProject(projects, "DocMap.nvim").id, "p1");
  assert.equal(pickProject(projects, "E:\\a\\twin\\").id, "p2");
  assert.equal(pickProject(projects, "twin"), "ambiguous");
  for (const unknown of ["", "nope", "E:/elsewhere", ".."]) assert.equal(pickProject(projects, unknown), null);
});

const map = {
  nodes: [
    {
      id: "lua/docmap",
      source: "lua/docmap/init.lua",
      functions: [
        { name: "outer", line: 10, line_end: 40 },
        { name: "inner", line: 20, line_end: 25 },
        { name: "later", line: 50, line_end: 60 },
      ],
    },
    { id: "lua/other", source: "lua/other.lua" },
  ],
};

test("a line resolves to the innermost function, else the module, else nothing", () => {
  assert.deepEqual(resolveAt(map, "lua/docmap/init.lua", 22), { id: "lua/docmap", key: "lua/docmap#inner" });
  assert.deepEqual(resolveAt(map, "lua/docmap/init.lua", 30), { id: "lua/docmap", key: "lua/docmap#outer" });
  assert.deepEqual(resolveAt(map, "lua/docmap/init.lua", 45), { id: "lua/docmap", key: null });
  assert.deepEqual(resolveAt(map, "lua/other.lua", 3), { id: "lua/other", key: null });
  assert.equal(resolveAt(map, "lua/missing.lua", 3), null);
  assert.equal(resolveAt(null, "lua/other.lua", 3), null);
});

test("a range that starts outside a function takes the one it overlaps most", () => {
  assert.deepEqual(resolveAt(map, "lua/docmap/init.lua", 45, 58), { id: "lua/docmap", key: "lua/docmap#later" });
  assert.deepEqual(resolveAt(map, "lua/docmap/init.lua", 58, 45), { id: "lua/docmap", key: "lua/docmap#later" });
});

test("the fragment is what the page reads, and a reload keeps the node", () => {
  const hit = { id: "lua/docmap", key: "lua/docmap#inner" };
  assert.equal(nodeFragment(hit), "#tab=tree&id=lua%2Fdocmap&fn=lua%2Fdocmap%23inner");
  assert.equal(nodeFragment({ id: "m", key: null }), "#tab=tree&id=m");
  assert.equal(mapHash(null, hit), nodeFragment(hit));
  assert.equal(mapHash("tree", hit), nodeFragment(hit));
  // The reader moved to another panel: that panel is restored, not the node.
  assert.equal(mapHash("analysis", hit), "#tab=analysis");
  assert.equal(mapHash(null, null), "");
  assert.equal(mapHash("graph", null), "#tab=graph");
});
