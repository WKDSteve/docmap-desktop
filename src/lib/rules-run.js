// An agent run over manual rules, without a DOM: what the run dialog says
// before anything is sent, and how a run's record becomes the rows of the run
// view.
//
// The run itself is the backend's (`agent/` in src-tauri): it plans, asks,
// checks every quote against the tree and stores proposals. Nothing here
// decides anything about a rule. What this file does decide is wording that
// must not drift: where the data goes (D6), and that a model's "compliant" is
// shown as "no violation found" with what it covered, never as a pass (D3).

/** A list the engine may encode as `[]` or, when empty, as `{}`. */
function list(value) {
  return Array.isArray(value) ? value : [];
}

/** How ai-desktop's four destinations are named to a reader. */
export const BACKEND_NAMES = {
  ollama: "Ollama",
  openai: "OpenAI",
  anthropic: "Anthropic",
  gemini: "Google Gemini",
};

/** The name of a backend, or the word itself for one this table does not know. */
export function backendName(backend) {
  return BACKEND_NAMES[backend] ?? String(backend ?? "");
}

/**
 * The title a run starts with: what was selected, and the day. A person can
 * change it; it is what the run is found by later.
 *
 * @param {string[]} ids   the selected manual rules
 * @param {string} day     `YYYY-MM-DD`
 */
export function defaultTitle(ids, day) {
  const shown = ids.slice(0, 3).join(", ");
  const more = ids.length > 3 ? ` +${ids.length - 3}` : "";
  return `${shown}${more} (${day})`;
}

/**
 * What a run would send and where, from `agent_preview`: the numbers of the
 * egress line. `null` while there is no destination (no model named, or the
 * project does not allow the model's backend).
 *
 * @param {object|null} preview
 * @returns {{local: boolean, backend: string, model: string, files: number,
 *            bytes: number, tokens: number, requests: number}|null}
 */
export function egress(preview) {
  const to = preview && preview.destination;
  if (!to) return null;
  const batches = list(preview.batches);
  const totals = preview.totals && typeof preview.totals === "object" ? preview.totals : {};
  const sum = (key) => batches.reduce((n, b) => n + (Number(b[key]) || 0), 0);
  return {
    local: !!to.local,
    backend: to.backend,
    model: to.model,
    // The plan's own totals where it gives them; a file sent in two batches is sent twice.
    files: Number(totals.files) || batches.reduce((n, b) => n + list(b.files).length, 0),
    bytes: Number(totals.bytes) || sum("bytes"),
    tokens: Number(totals.tokens) || sum("tokens"),
    requests: batches.length,
  };
}

/**
 * Why Start is off, as a catalog key, or `null` when a run can start. The
 * order is the order a person fixes things in.
 *
 * @param {{title: string, model: string, preview: object|null, health: object|null,
 *          healthError: boolean, loading: boolean}} form
 */
export function startBlocker(form) {
  if (form.loading) return "rules.run.block.loading";
  if (!String(form.title ?? "").trim()) return "rules.run.block.title";
  if (!String(form.model ?? "").trim()) return "rules.run.block.model";
  const preview = form.preview;
  if (!preview) return "rules.run.block.preview";
  if (!preview.destination) return "rules.run.block.refused";
  if (!list(preview.batches).length) return "rules.run.block.nothing";
  if (form.healthError) return "rules.run.block.unreachable";
  const backends = form.health && form.health.backends;
  // ai-desktop says which cloud backends have a key; a local model needs none.
  if (!preview.destination.local && backends && backends[preview.destination.backend] === false) {
    return "rules.run.block.backend";
  }
  return null;
}

/** The states of one rule in a run, in the order the queue's counts list them. */
export const ROW_STATES = ["asking", "queued", "proposed", "unclear", "error", "notSent"];

/**
 * One row per rule of a run: where it stands, what was proposed, and, kept
 * apart from both, what the check of the evidence did with the answer.
 *
 * - `state` is the run's word: `queued`, `asking`, `proposed` (an answer that
 *   says violation or no violation), `unclear` (no usable answer), `error`
 *   (the batch or the check failed), `notSent`;
 * - `status` is what the model proposed (`violation`, `compliant`, `unclear`),
 *   absent until there is a proposal;
 * - `evidence` counts the findings that held up against the tree, the ones
 *   that were dropped, and the files the answer covered and left out.
 *
 * Rows come in the order of the batches, then the rules that were not sent.
 *
 * @param {object} run   a run as `agent_status` gives it
 */
export function queueRows(run) {
  const proposals = new Map(list(run?.proposals).map((p) => [p.ruleId, p]));
  const rows = [];
  for (const batch of list(run?.batches)) {
    for (const id of list(batch.rules)) {
      const rec = proposals.get(id);
      const row = { id, batch: batch.id, fromCache: !!batch.fromCache, state: "queued" };
      if (rec) {
        const p = rec.proposal && typeof rec.proposal === "object" ? rec.proposal : {};
        row.status = rec.status;
        row.state = rec.status === "error" ? "error" : rec.status === "unclear" ? "unclear" : "proposed";
        row.reason = p.reason ?? null;
        row.error = rec.error ?? null;
        row.evidence = {
          held: list(p.findings).length,
          dropped: list(p.dropped).length,
          checked: list(p.checked).length,
          unchecked: list(p.unchecked).length,
        };
      } else if (batch.state === "asking") {
        row.state = "asking";
      } else if (batch.state === "failed") {
        row.state = "error";
        row.error = batch.error ?? null;
      }
      rows.push(row);
    }
  }
  for (const skipped of list(run?.skipped)) {
    if (typeof skipped?.id !== "string") continue;
    rows.push({
      id: skipped.id,
      state: "notSent",
      reason: String(skipped.reason ?? skipped.status ?? ""),
    });
  }
  return rows;
}

/** How many rows each state has. */
export function rowCounts(rows) {
  const counts = Object.fromEntries(ROW_STATES.map((s) => [s, 0]));
  for (const row of rows) if (row.state in counts) counts[row.state] += 1;
  return counts;
}

/**
 * The catalog key for what a proposal says. A model's `compliant` is "no
 * violation found": it looked at some files and found nothing, which is less
 * than the word compliant claims (D3).
 */
export function proposalKey(status) {
  if (status === "violation") return "rules.run.says.violation";
  if (status === "compliant") return "rules.run.says.noViolation";
  if (status === "error") return "rules.run.says.error";
  return "rules.run.says.unclear";
}

/** The tokens a run has used so far, as its backends reported them (exact, not an estimate). */
export function tokensUsed(run) {
  const usage = (run && run.usage) || {};
  const prompt = Number(usage.promptTokens) || 0;
  const completion = Number(usage.completionTokens) || 0;
  return { prompt, completion, total: prompt + completion };
}

/** Batches done and in all, from a run or from a summary of one. */
export function progress(run) {
  if (!run) return { done: 0, total: 0 };
  if (typeof run.batchesTotal === "number") {
    return { done: run.batchesDone ?? 0, total: run.batchesTotal };
  }
  const batches = list(run.batches);
  return { done: batches.filter((b) => b.state === "done").length, total: batches.length };
}

/** What a run can be told to do in its state. */
export function actionsFor(state) {
  return {
    pause: state === "running",
    resume: state === "paused" || state === "failed",
    cancel: state === "running" || state === "paused",
  };
}

/** Whether a run is still changing, so its view has to ask again. */
export function isLive(state) {
  return state === "running";
}
