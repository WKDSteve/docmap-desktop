// The chat of the run view, without a DOM: the state of one conversation and
// the wording that says what a message would send and where.
//
// The conversation itself is the backend's (`agent/chat.rs`): it builds every
// turn from the rule, its files, the proposal and the transcript, trims the
// transcript and streams the answer. This file keeps the transcript the window
// shows and decides the words that must not drift: that every number is an
// estimate, that a trimmed conversation says so, and where the text goes
// (AI is never pushed: nothing is sent unless the person sends it, and before that the
// window says what goes where).

/** Four characters a token: the engine's own estimate (`rules.engine.agent.request`). */
export function estimateTokens(chars) {
  return Math.ceil(Math.max(0, chars) / 4);
}

/** The key a conversation is kept under: one per run and rule, one per run for the run as a whole. */
export function chatKey(runId, rule) {
  return `${runId}\u0000${rule ?? ""}`;
}

/** A conversation as the window starts it: empty, nothing attached, nothing sent. */
export function newChat(runId, rule, n) {
  return {
    runId,
    rule: rule ?? null,
    // the window's name for it, which a cancel names; one per conversation
    chatId: `chat-${runId}-${rule ?? "run"}-${n}`,
    messages: [],
    files: [],
    draft: "",
    preview: null,
    previewError: null,
    previewing: false,
    busy: false,
    streaming: null,
    error: null,
    dropped: 0,
    proposing: false,
    note: null,
  };
}

/**
 * The transcript that goes with the next turn: what the person wrote and the
 * answers that came back whole. A stopped or failed answer stays on screen
 * and is not sent again.
 */
export function transcriptFor(messages) {
  return messages
    .filter((m) => (m.role === "user" || m.role === "assistant") && !m.partial)
    .map((m) => ({ role: m.role, text: m.text }));
}

/** What a preview or a turn hands the backend. */
export function chatArgs(chat, draft) {
  const messages = transcriptFor(chat.messages);
  if (draft !== undefined && draft.trim() !== "") messages.push({ role: "user", text: draft });
  return { runId: chat.runId, rule: chat.rule, messages, files: [...chat.files] };
}

/**
 * The words of the egress line: where the text goes, and what goes with every
 * message. `tokens` is the preview's estimate plus the draft's, never a count.
 *
 * @param {object} preview  `agent_chat_preview`'s answer
 * @param {number} draftChars  characters of the message not yet sent
 */
export function egressFacts(preview, draftChars) {
  const d = preview.destination;
  const attached = (preview.attached ?? []).filter((a) => a.sent).length;
  return {
    local: d ? d.local : null,
    backend: d ? d.backend : null,
    model: d ? d.model : null,
    rule: preview.rule ?? "",
    files: (preview.contextFiles ?? []).length,
    attached,
    messages: preview.sentMessages ?? 0,
    tokens: (preview.tokens ?? 0) + estimateTokens(draftChars),
  };
}

/**
 * A path the file dialog gave, relative to the project root, with `/`; null
 * when it is not inside the project. Windows paths compare without regard to
 * case, as the file system does.
 */
export function relativeTo(root, path) {
  const norm = (p) => String(p ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
  const r = norm(root);
  const p = norm(path);
  if (!r || !p) return null;
  const windows = /^[A-Za-z]:/.test(r) || /^\/\//.test(r);
  const same = windows ? p.toLowerCase().startsWith(r.toLowerCase() + "/") : p.startsWith(r + "/");
  if (!same) return null;
  const rest = p.slice(r.length + 1);
  if (!rest || rest.split("/").some((s) => s === "..")) return null;
  return rest;
}

/** Attach `file` once; at most `max`. Answers whether it was added. */
export function addFile(chat, file, max = 5) {
  if (!file || chat.files.includes(file) || chat.files.length >= max) return false;
  chat.files.push(file);
  return true;
}

/** The engine's word about one attached file, as a key and its values. */
export function attachedLine(a) {
  if (a.sent) return { key: "rules.chat.file.sent", vars: { file: a.file, tokens: a.tokens ?? 0 } };
  if (a.denied) return { key: "rules.chat.file.denied", vars: { file: a.file } };
  return { key: "rules.chat.file.not", vars: { file: a.file, reason: a.reason ?? "" } };
}

/** Whether an answer can become a proposal: a whole answer, in a conversation about one rule. */
export function canPropose(chat, message) {
  return Boolean(chat.rule) && message.role === "assistant" && !message.partial && message.text.trim() !== "";
}
