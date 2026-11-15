// The chat of the run view: its state and its wording, from canned answers.
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  estimateTokens,
  chatKey,
  newChat,
  transcriptFor,
  chatArgs,
  egressFacts,
  relativeTo,
  addFile,
  attachedLine,
  canPropose,
} from "./rules-chat.js";

test("the estimate is four characters a token, as the engine's", () => {
  assert.equal(estimateTokens(0), 0);
  assert.equal(estimateTokens(1), 1);
  assert.equal(estimateTokens(8), 2);
  assert.equal(estimateTokens(9), 3);
  assert.equal(estimateTokens(-5), 0);
});

test("one conversation per run and rule, and one for the run as a whole", () => {
  assert.notEqual(chatKey("r1", "A-1"), chatKey("r1", "A-2"));
  assert.notEqual(chatKey("r1", "A-1"), chatKey("r1", null));
  assert.equal(chatKey("r1", null), chatKey("r1", undefined));
  const c = newChat("r1", null, 3);
  assert.equal(c.rule, null);
  assert.equal(c.chatId, "chat-r1-run-3");
  assert.deepEqual(c.messages, []);
});

test("a stopped answer stays on screen and is not sent again", () => {
  const messages = [
    { role: "user", text: "q1" },
    { role: "assistant", text: "half", partial: true },
    { role: "user", text: "q2" },
    { role: "assistant", text: "a2" },
    { role: "notice", text: "trimmed" },
  ];
  assert.deepEqual(transcriptFor(messages), [
    { role: "user", text: "q1" },
    { role: "user", text: "q2" },
    { role: "assistant", text: "a2" },
  ]);
});

test("a turn hands over the transcript, the draft and the attached files", () => {
  const c = newChat("r1", "A-1", 1);
  c.messages.push({ role: "user", text: "q1" }, { role: "assistant", text: "a1" });
  c.files.push("lua/a.lua");
  assert.deepEqual(chatArgs(c, "q2"), {
    runId: "r1",
    rule: "A-1",
    messages: [
      { role: "user", text: "q1" },
      { role: "assistant", text: "a1" },
      { role: "user", text: "q2" },
    ],
    files: ["lua/a.lua"],
  });
  // a preview without a draft, and an empty draft is no message
  assert.equal(chatArgs(c).messages.length, 2);
  assert.equal(chatArgs(c, "   ").messages.length, 2);
  // the files are a copy: attaching later does not change what was handed over
  const args = chatArgs(c);
  c.files.push("x");
  assert.deepEqual(args.files, ["lua/a.lua"]);
});

test("the egress line counts what goes, and the draft only as an estimate", () => {
  const preview = {
    destination: { backend: "anthropic", model: "claude-x", local: false },
    rule: "A-1",
    contextFiles: ["a.lua", "b.lua"],
    attached: [
      { file: "c.lua", sent: true, tokens: 9 },
      { file: ".env", sent: false, denied: true },
    ],
    sentMessages: 4,
    tokens: 100,
  };
  assert.deepEqual(egressFacts(preview, 9), {
    local: false,
    backend: "anthropic",
    model: "claude-x",
    rule: "A-1",
    files: 2,
    attached: 1,
    messages: 4,
    tokens: 103,
  });
  const refused = egressFacts({ destination: null, refused: { key: "err.agent.localOnly" } }, 0);
  assert.equal(refused.local, null);
  assert.equal(refused.tokens, 0);
});

test("a picked file is named relative to the project, or not at all", () => {
  assert.equal(relativeTo("C:\\repo\\proj", "C:\\repo\\proj\\lua\\a.lua"), "lua/a.lua");
  assert.equal(relativeTo("C:\\repo\\proj\\", "c:\\REPO\\proj\\lua\\a.lua"), "lua/a.lua", "case, on Windows");
  assert.equal(relativeTo("C:\\repo\\proj", "C:\\repo\\project2\\a.lua"), null, "a sibling with the same start");
  assert.equal(relativeTo("C:\\repo\\proj", "C:\\repo\\proj"), null, "the root itself");
  assert.equal(relativeTo("C:\\repo\\proj", "C:\\repo\\proj\\..\\x"), null);
  assert.equal(relativeTo("/home/a/proj", "/home/a/proj/src/x.js"), "src/x.js");
  assert.equal(relativeTo("/home/a/proj", "/home/a/PROJ/src/x.js"), null, "case counts elsewhere");
  assert.equal(relativeTo("", "/x"), null);
  assert.equal(relativeTo("/p", null), null);
});

test("a file is attached once, and only so many", () => {
  const c = newChat("r", "A", 1);
  assert.equal(addFile(c, "a"), true);
  assert.equal(addFile(c, "a"), false);
  assert.equal(addFile(c, ""), false);
  for (const f of ["b", "c", "d", "e"]) addFile(c, f);
  assert.equal(addFile(c, "f"), false);
  assert.equal(c.files.length, 5);
});

test("each attached file says whether it goes, and why not", () => {
  assert.deepEqual(attachedLine({ file: "a", sent: true, tokens: 3 }), {
    key: "rules.chat.file.sent",
    vars: { file: "a", tokens: 3 },
  });
  assert.equal(attachedLine({ file: ".env", sent: false, denied: true }).key, "rules.chat.file.denied");
  assert.deepEqual(attachedLine({ file: "big", sent: false, denied: false, reason: "too large" }), {
    key: "rules.chat.file.not",
    vars: { file: "big", reason: "too large" },
  });
});

test("only a whole answer about one rule can become a proposal", () => {
  const rule = newChat("r", "A-1", 1);
  const run = newChat("r", null, 2);
  const whole = { role: "assistant", text: "it is fine" };
  assert.equal(canPropose(rule, whole), true);
  assert.equal(canPropose(run, whole), false, "the run as a whole has no one rule");
  assert.equal(canPropose(rule, { ...whole, partial: true }), false);
  assert.equal(canPropose(rule, { role: "user", text: "q" }), false);
  assert.equal(canPropose(rule, { role: "assistant", text: "  " }), false);
});
