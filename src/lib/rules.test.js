// The Rules tab's list logic: grouping, the filter, the tri-state selection
// and how a run's answers reach the rows.
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  MAX_SELECTION_BYTES,
  titleOf,
  laneOf,
  visibleRules,
  laneCounts,
  fileLabel,
  groupRules,
  orderedIds,
  checkState,
  toggleGroup,
  toggleRule,
  selectRange,
  selectManual,
  selectFamilies,
  splitSelection,
  runSelections,
  mergeRun,
  mechanicalCounts,
  sidebarCounts,
  notSendable,
  untrustedRulesets,
} from "./rules.js";

const RULESET = "E:/shared/sets/lua";

/** A catalog like the engine's: sorted by id, two files, three families. */
function catalog() {
  const rule = (id, file, section, check) => ({
    id,
    family: id.split("-")[0],
    title: `title of ${id}`,
    text: `text of ${id}`,
    section,
    source_file: `${RULESET}/${file}`,
    source_line: 1,
    has_check: !!check,
    check: check || undefined,
  });
  return {
    available: true,
    rulesets: [{ name: "lua", path: RULESET, trust: { state: "untrusted", predicates: 1 } }],
    rules: [
      rule("D-01", "deps.md", "Loading", "grep"),
      rule("D-02", "deps.md", "Loading", null),
      rule("D-03", "deps.md", "Versions", null),
      rule("L-01", "style.md", "Names", "lua_predicate"),
      rule("L-02", "style.md", "Names", null),
      rule("S-01", "style.md", "Safety", null),
    ],
  };
}

const ids = (rules) => rules.map((r) => r.id);

test("a rule with a check is automated, one without is manual", () => {
  const { rules } = catalog();
  assert.equal(laneOf(rules[0]), "auto");
  assert.equal(laneOf(rules[1]), "manual");
  assert.deepEqual(laneCounts(rules), { auto: 2, manual: 4, total: 6 });
});

test("a title is shown without the rule's own id in front", () => {
  assert.equal(titleOf({ id: "D-01", title: "`D-01` — one loader" }), "one loader");
  assert.equal(titleOf({ id: "D-01", title: "D-01: one loader" }), "one loader");
  assert.equal(titleOf({ id: "D-01", title: "D-01 - one loader" }), "one loader");
  assert.equal(titleOf({ id: "D-01", title: "Loaders are D-01's subject" }), "Loaders are D-01's subject");
  // no title, or one that is only the id: the first line of the text
  assert.equal(titleOf({ id: "D-01", title: "`D-01`", text: "First line.\nSecond." }), "First line.");
  assert.equal(titleOf({ id: "D-01" }), "");
});

test("the filter shows one lane or both, and the query narrows by id, title and text", () => {
  const { rules } = catalog();
  assert.deepEqual(ids(visibleRules(rules, { lane: "auto" })), ["D-01", "L-01"]);
  assert.equal(visibleRules(rules, { lane: "manual" }).length, 4);
  assert.equal(visibleRules(rules, {}).length, 6);
  assert.deepEqual(ids(visibleRules(rules, { query: "d-0" })), ["D-01", "D-02", "D-03"]);
  assert.deepEqual(ids(visibleRules(rules, { query: "TEXT OF L-", lane: "manual" })), ["L-02"]);
  assert.deepEqual(visibleRules(rules, { query: "nothing like it" }), []);
});

test("by file: one group per file, in it the sections in the order they come", () => {
  const { rules, rulesets } = catalog();
  const groups = groupRules(rules, "file", rulesets);
  assert.deepEqual(
    groups.map((g) => [g.label, g.kind, g.subgroups.map((s) => [s.label, ids(s.rules)])]),
    [
      ["deps.md", "file", [["Loading", ["D-01", "D-02"]], ["Versions", ["D-03"]]]],
      ["style.md", "file", [["Names", ["L-01", "L-02"]], ["Safety", ["S-01"]]]],
    ]
  );
  assert.deepEqual(orderedIds(groups), ids(rules));
});

test("by family: one group per family across files, sorted, without sections", () => {
  const { rules } = catalog();
  // a family that spans two files
  rules.push({ ...rules[0], id: "D-09", source_file: `${RULESET}/style.md`, has_check: false });
  const groups = groupRules(rules, "family");
  assert.deepEqual(
    groups.map((g) => [g.label, g.kind, ids(g.rules), g.subgroups.length]),
    [
      ["D", "family", ["D-01", "D-02", "D-03", "D-09"], 0],
      ["L", "family", ["L-01", "L-02"], 0],
      ["S", "family", ["S-01"], 0],
    ]
  );
  assert.deepEqual(orderedIds(groups).slice(0, 4), ["D-01", "D-02", "D-03", "D-09"]);
});

test("a group the filter emptied is not in the list", () => {
  const { rules, rulesets } = catalog();
  const groups = groupRules(visibleRules(rules, { lane: "auto" }), "family", rulesets);
  assert.deepEqual(groups.map((g) => g.label), ["D", "L"]);
});

test("a file is named relative to its ruleset, with the ruleset in front when there are two", () => {
  const one = [{ name: "lua", path: RULESET }];
  assert.equal(fileLabel(`${RULESET}/sub/deps.md`, one), "sub/deps.md");
  assert.equal(fileLabel("E:\\vault\\rulesets\\lua\\deps.md", one), "deps.md");
  const two = [...one, { name: "@vault/native", path: "E:/shared/sets/native" }];
  assert.equal(fileLabel(`${RULESET}/deps.md`, two), "lua: deps.md");
  assert.equal(fileLabel("E:/shared/sets/native/a.md", two), "native: a.md");
  // a sibling whose name only starts the same is not inside
  assert.equal(fileLabel("E:/shared/sets/lua-old/a.md", one), "a.md");
  // a ruleset that is one file
  assert.equal(fileLabel(`${RULESET}/RULES.md`, [{ name: "r", path: `${RULESET}/RULES.md` }]), "RULES.md");
});

test("a checkbox over a group is all, some or none", () => {
  const { rules } = catalog();
  assert.equal(checkState(rules, new Set()), "none");
  assert.equal(checkState(rules, new Set(["D-01"])), "some");
  assert.equal(checkState(rules, new Set(ids(rules))), "all");
  assert.equal(checkState([], new Set(["D-01"])), "none");
});

test("ticking a group selects its visible rules and nothing the filter hides", () => {
  const { rules } = catalog();
  const dep = rules.filter((r) => r.family === "D");
  // with Manual shown, the family's checkbox only ever sees its manual rules
  const shown = visibleRules(dep, { lane: "manual" });
  const picked = toggleGroup(new Set(), shown);
  assert.deepEqual([...picked], ["D-02", "D-03"]);
  // a second click clears exactly those, and leaves a hidden selected rule alone
  const withHidden = new Set([...picked, "D-01"]);
  assert.deepEqual([...toggleGroup(withHidden, shown)], ["D-01"]);
  // a partly selected group is completed, not cleared
  assert.deepEqual([...toggleGroup(new Set(["D-02"]), shown)].sort(), ["D-02", "D-03"]);
});

test("a shift-click selects the range from the last click, in the order on screen", () => {
  const { rules, rulesets } = catalog();
  const order = orderedIds(groupRules(rules, "file", rulesets));
  let sel = toggleRule(new Set(), "D-02");
  sel = selectRange(sel, order, "D-02", "L-01");
  assert.deepEqual([...sel].sort(), ["D-02", "D-03", "L-01"]);
  // backwards, and clearing: the target is selected, so the range is cleared
  sel = selectRange(sel, order, "L-01", "D-03");
  assert.deepEqual([...sel], ["D-02"]);
  // no anchor on screen: a plain click
  assert.deepEqual([...selectRange(new Set(), order, null, "S-01")], ["S-01"]);
  assert.deepEqual([...selectRange(new Set(), order, "G-1", "S-01")], ["S-01"]);
});

test("a range goes over what is shown, so a filtered-out rule in between stays out", () => {
  const { rules, rulesets } = catalog();
  const shown = visibleRules(rules, { lane: "manual" });
  const order = orderedIds(groupRules(shown, "file", rulesets));
  const sel = selectRange(new Set(["D-02"]), order, "D-02", "L-02");
  assert.deepEqual([...sel].sort(), ["D-02", "D-03", "L-02"]);
  assert.ok(!sel.has("L-01"), "the automated rule between them is hidden and not selected");
});

test("select all manual adds the manual rules of the group and keeps the rest", () => {
  const { rules } = catalog();
  const lua = rules.filter((r) => r.source_file.endsWith("style.md"));
  const sel = selectManual(new Set(["D-01"]), lua);
  assert.deepEqual([...sel].sort(), ["D-01", "L-02", "S-01"]);
});

test("a gate selects the shown rules of its families and keeps what was selected", () => {
  const { rules } = catalog();
  const sel = selectFamilies(new Set(["S-01"]), rules, ["D", "NOSUCH"]);
  assert.deepEqual([...sel].sort(), ["D-01", "D-02", "D-03", "S-01"]);
  // under the manual filter the gate does not reach the automated rule of the family
  const shown = visibleRules(rules, { lane: "manual" });
  assert.deepEqual([...selectFamilies(new Set(), shown, ["D"])].sort(), ["D-02", "D-03"]);
  assert.equal(selectFamilies(new Set(), rules, {}).size, 0, "a gate without families selects nothing");
});

test("a mixed selection is split into its lanes and never added up", () => {
  const { rules } = catalog();
  const split = splitSelection(rules, new Set(["D-01", "D-02", "L-01", "S-01", "G-9"]));
  assert.deepEqual(split, { auto: ["D-01", "L-01"], manual: ["D-02", "S-01"] });
});

test("a long run is cut into calls below the engine's limits", () => {
  assert.deepEqual(runSelections([]), []);
  assert.deepEqual(runSelections(["A-1", "A-2"]), ["A-1,A-2"]);
  const many = Array.from({ length: 5 }, (_, i) => `R-${i}`);
  assert.deepEqual(runSelections(many, 1024, 2), ["R-0,R-1", "R-2,R-3", "R-4"]);
  // by bytes: each id is 4 bytes and a comma
  assert.deepEqual(runSelections(["AAAA", "BBBB", "CCCC"], 10, 99), ["AAAA,BBBB", "CCCC"]);
  // the default stays below what one Windows command line can carry
  const big = runSelections(Array.from({ length: 6000 }, (_, i) => `FAMILY-${i}`));
  assert.ok(big.length > 1);
  assert.ok(big.every((s) => s.length <= MAX_SELECTION_BYTES && MAX_SELECTION_BYTES <= 16 * 1024), "each call fits a command line with room for the rest");
  assert.equal(big.join(",").split(",").length, 6000, "no rule is lost between the calls");
  // one id longer than the limit still goes, alone
  assert.deepEqual(runSelections(["LONGLONGID", "B"], 4, 99), ["LONGLONGID", "B"]);
});

test("the answers of a run land on their rows, and a rule not answered keeps what it had", () => {
  let results = new Map();
  results = mergeRun(results, ["D-01", "L-01"], {
    available: true,
    results: [
      { id: "D-01", status: "fail", findings: [{ file: "a.lua", line: 3, text: "x" }] },
      { id: "L-01", status: "error", findings: {} },
    ],
  });
  assert.equal(results.get("D-01").status, "fail");
  assert.deepEqual(results.get("L-01").findings, [], "an empty list encoded as {} is a list");
  results = mergeRun(results, ["D-01"], { available: true, results: [{ id: "D-01", status: "pass" }] });
  assert.equal(results.get("D-01").status, "pass");
  assert.equal(results.get("L-01").status, "error");
});

test("a run that died is an error row for every rule it was asked about", () => {
  const before = new Map([["S-01", { id: "S-01", status: "pass", findings: [] }]]);
  const results = mergeRun(before, ["D-01", "L-01"], { failed: "stopped after 120 s" });
  for (const id of ["D-01", "L-01"]) {
    assert.equal(results.get(id).status, "error");
    assert.equal(results.get(id).findings[0].text, "stopped after 120 s");
    assert.equal(results.get(id).stopped, true);
  }
  assert.equal(results.get("S-01").status, "pass", "a rule of another call is untouched");
  assert.equal(before.size, 1, "the old map is not changed");
});

test("counts of the mechanical results, and the two numbers of the sidebar", () => {
  const cat = catalog();
  const results = mergeRun(new Map(), [], {
    results: [
      { id: "D-01", status: "fail" },
      { id: "L-01", status: "error" },
      { id: "G-1", status: "fail" },
    ],
  });
  assert.deepEqual(mechanicalCounts(cat.rules, results), { fail: 1, error: 1, pass: 0, waived: 0 });
  assert.deepEqual(sidebarCounts(cat, results), { manual: 4, failed: 2 });
  assert.deepEqual(sidebarCounts(cat, new Map()), { manual: 4, failed: 0 });
  assert.equal(sidebarCounts({ available: false, reason: "no ruleset" }, results), null);
  assert.equal(sidebarCounts(null, results), null);
});

test("why a manual rule would not be sent, by rule", () => {
  const why = notSendable([
    { id: "D-02", status: "manual", reason: "no scope to look in" },
    { id: "S-01", status: "unclear" },
    { nonsense: true },
  ]);
  assert.equal(why.get("D-02"), "no scope to look in");
  assert.equal(why.get("S-01"), "unclear");
  assert.equal(why.size, 2);
  assert.equal(notSendable({}).size, 0);
});

test("a ruleset with predicates that is not trusted is named, one without is not", () => {
  const cat = catalog();
  assert.equal(untrustedRulesets(cat).length, 1);
  cat.rulesets[0].trust.state = "changed";
  assert.equal(untrustedRulesets(cat).length, 1);
  cat.rulesets[0].trust.state = "trusted";
  assert.equal(untrustedRulesets(cat).length, 0);
  cat.rulesets[0].trust = { state: "untrusted", predicates: 0 };
  assert.equal(untrustedRulesets(cat).length, 0);
});
