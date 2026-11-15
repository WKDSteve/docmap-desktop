// The run dialog's wording and the run view's rows, from canned records.
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  backendName,
  defaultTitle,
  egress,
  startBlocker,
  queueRows,
  rowCounts,
  proposalKey,
  tokensUsed,
  progress,
  actionsFor,
  isLive,
} from "./rules-run.js";

function preview(over = {}) {
  return {
    destination: { backend: "anthropic", model: "claude-x", local: false },
    refused: null,
    models: ["claude-x"],
    batches: [
      { id: "b1", rules: [{ id: "M-01" }, { id: "M-02" }], files: ["lua/a.lua", "lua/b.lua"], bytes: 4000, tokens: 1000 },
      { id: "b2", rules: [{ id: "M-03" }], files: ["lua/a.lua"], bytes: 2000, tokens: 500 },
    ],
    skipped: [{ id: "N-01", status: "manual", reason: "no scope to look in" }],
    totals: { requests: 2, files: 3, bytes: 6000, tokens: 1500 },
    ...over,
  };
}

test("a backend is named as a reader knows it", () => {
  assert.equal(backendName("anthropic"), "Anthropic");
  assert.equal(backendName("ollama"), "Ollama");
  assert.equal(backendName("somethingnew"), "somethingnew");
});

test("the default title names the selection and the day", () => {
  assert.equal(defaultTitle(["M-01"], "2026-11-15"), "M-01 (2026-11-15)");
  assert.equal(defaultTitle(["M-01", "M-02", "M-03", "N-01", "N-02"], "2026-11-15"), "M-01, M-02, M-03 +2 (2026-11-15)");
});

test("the egress line counts what the plan sends, and a file in two batches twice", () => {
  assert.deepEqual(egress(preview()), {
    local: false,
    backend: "anthropic",
    model: "claude-x",
    files: 3,
    bytes: 6000,
    tokens: 1500,
    requests: 2,
  });
  // without the plan's totals the batches are added up
  const summed = egress(preview({ totals: {} }));
  assert.deepEqual([summed.files, summed.bytes, summed.tokens], [3, 6000, 1500]);
  // no destination, no line
  assert.equal(egress(preview({ destination: null })), null);
  assert.equal(egress(null), null);
});

test("a local destination is said to be local", () => {
  const e = egress(preview({ destination: { backend: "ollama", model: "qwen", local: true } }));
  assert.equal(e.local, true);
});

test("Start is off until there is a title, a model, a destination and something to send", () => {
  const ok = { title: "t", model: "claude-x", preview: preview(), health: { backends: { anthropic: true } } };
  assert.equal(startBlocker(ok), null);
  assert.equal(startBlocker({ ...ok, loading: true }), "rules.run.block.loading");
  assert.equal(startBlocker({ ...ok, title: "  " }), "rules.run.block.title");
  assert.equal(startBlocker({ ...ok, model: "" }), "rules.run.block.model");
  assert.equal(startBlocker({ ...ok, preview: null }), "rules.run.block.preview");
  assert.equal(startBlocker({ ...ok, preview: preview({ destination: null }) }), "rules.run.block.refused");
  assert.equal(startBlocker({ ...ok, preview: preview({ batches: {} }) }), "rules.run.block.nothing");
  assert.equal(startBlocker({ ...ok, healthError: true }), "rules.run.block.unreachable");
});

test("a cloud backend without a key in ai-desktop blocks Start, a local model needs none", () => {
  const cloud = { title: "t", model: "claude-x", preview: preview(), health: { backends: { anthropic: false } } };
  assert.equal(startBlocker(cloud), "rules.run.block.backend");
  const local = {
    title: "t",
    model: "qwen",
    preview: preview({ destination: { backend: "ollama", model: "qwen", local: true } }),
    health: { backends: { anthropic: false } },
  };
  assert.equal(startBlocker(local), null);
  // a backend ai-desktop does not list at all is not known to be missing
  assert.equal(startBlocker({ ...cloud, health: { backends: {} } }), null);
});

function run() {
  return {
    state: "running",
    usage: { promptTokens: 1200, completionTokens: 300 },
    batches: [
      { id: "b1", rules: ["M-01", "M-02", "M-03", "M-04"], state: "done", fromCache: false },
      { id: "b2", rules: ["M-05"], state: "asking" },
      { id: "b3", rules: ["M-06"], state: "queued" },
      { id: "b4", rules: ["M-07"], state: "failed", error: { key: "err.agent.timeout" } },
    ],
    proposals: [
      {
        ruleId: "M-01",
        status: "violation",
        proposal: {
          status: "violation",
          findings: [{ file: "lua/a.lua", line: 3, quote: "x" }],
          dropped: [{ finding: {}, reason: "quote not found" }, { finding: {}, reason: "file not sent" }],
          checked: ["lua/a.lua"],
          unchecked: ["lua/b.lua"],
        },
      },
      { ruleId: "M-02", status: "compliant", proposal: { status: "compliant", findings: {}, checked: ["lua/a.lua", "lua/b.lua"], unchecked: {} } },
      { ruleId: "M-03", status: "unclear", proposal: { status: "unclear", reason: "the answer is not JSON" } },
      { ruleId: "M-04", status: "error", proposal: {}, error: { key: "err.agent.engine" } },
    ],
    skipped: [{ id: "N-01", status: "manual", reason: "no scope to look in" }],
  };
}

test("one row per rule: the run's state, the proposal, and the evidence check apart from both", () => {
  const rows = queueRows(run());
  assert.deepEqual(
    rows.map((r) => [r.id, r.state, r.status ?? null]),
    [
      ["M-01", "proposed", "violation"],
      ["M-02", "proposed", "compliant"],
      ["M-03", "unclear", "unclear"],
      ["M-04", "error", "error"],
      ["M-05", "asking", null],
      ["M-06", "queued", null],
      ["M-07", "error", null],
      ["N-01", "notSent", null],
    ]
  );
  // what held and what was dropped is counted, not folded into the status
  assert.deepEqual(rows[0].evidence, { held: 1, dropped: 2, checked: 1, unchecked: 1 });
  assert.deepEqual(rows[1].evidence, { held: 0, dropped: 0, checked: 2, unchecked: 0 });
  assert.equal(rows[2].reason, "the answer is not JSON");
  assert.equal(rows[6].error.key, "err.agent.timeout", "a failed batch says why on its rules");
  assert.equal(rows[7].reason, "no scope to look in");
  assert.deepEqual(rowCounts(rows), { asking: 1, queued: 1, proposed: 2, unclear: 1, error: 2, notSent: 1 });
});

test("an empty or half-written record has no rows and does not throw", () => {
  assert.deepEqual(queueRows(null), []);
  assert.deepEqual(queueRows({ batches: {}, proposals: {}, skipped: {} }), []);
});

test("a model's compliant is no violation found, never a pass", () => {
  assert.equal(proposalKey("compliant"), "rules.run.says.noViolation");
  assert.equal(proposalKey("violation"), "rules.run.says.violation");
  assert.equal(proposalKey("unclear"), "rules.run.says.unclear");
  assert.equal(proposalKey("error"), "rules.run.says.error");
  assert.equal(proposalKey(undefined), "rules.run.says.unclear");
});

test("tokens are the backends' own numbers, and progress counts finished batches", () => {
  assert.deepEqual(tokensUsed(run()), { prompt: 1200, completion: 300, total: 1500 });
  assert.deepEqual(tokensUsed({}), { prompt: 0, completion: 0, total: 0 });
  assert.deepEqual(progress(run()), { done: 1, total: 4 });
  assert.deepEqual(progress({ batchesDone: 2, batchesTotal: 5 }), { done: 2, total: 5 });
  assert.deepEqual(progress(null), { done: 0, total: 0 });
});

test("what a run can be told depends on its state", () => {
  assert.deepEqual(actionsFor("running"), { pause: true, resume: false, cancel: true });
  assert.deepEqual(actionsFor("paused"), { pause: false, resume: true, cancel: true });
  assert.deepEqual(actionsFor("failed"), { pause: false, resume: true, cancel: false });
  assert.deepEqual(actionsFor("done"), { pause: false, resume: false, cancel: false });
  assert.deepEqual(actionsFor("cancelled"), { pause: false, resume: false, cancel: false });
  assert.equal(isLive("running"), true);
  assert.equal(isLive("paused"), false);
});
