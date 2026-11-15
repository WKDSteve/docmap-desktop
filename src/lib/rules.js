// The Rules tab without a DOM: how the catalog is grouped, filtered and
// selected, how a selection is split into what runs here and what is left for
// an agent run, and how the answers of a run land on the rows.
//
// Everything here is a pure function over the engine's `rules/catalog` and
// `rules/run` answers, so `rules.test.js` can hold the list to what the task
// asks for without a window: a selection that respects the filter, automated
// and manual rules counted apart everywhere, and a mechanical result that
// never shares a slot with what an agent proposed.

/** The two lanes a rule can be in. A rule with a check is run, one without is read. */
export const LANES = ["auto", "manual"];

/** The filter of the list: both lanes, or one of them. */
export const FILTERS = ["all", "auto", "manual"];

/** The two ways the list is grouped. */
export const GROUPINGS = ["file", "family"];

/** What a mechanical run says about a rule, in the order the counts are shown. */
export const MECHANICAL = ["fail", "error", "pass", "waived"];

/**
 * `rules/run` refuses a selection of more than 64 KiB or 4096 items, and the
 * selection travels as one argument of the engine's command line, which
 * Windows caps at 32 767 characters for the whole line (the program, the
 * project and the trusted folders are on it too). A run of more is cut into
 * several calls well below all three.
 */
export const MAX_SELECTION_BYTES = 16 * 1024;
export const MAX_SELECTION_ITEMS = 2000;

/** @param {{has_check?: boolean}} rule */
export function laneOf(rule) {
  return rule.has_check ? "auto" : "manual";
}

/**
 * A rule's title without its own id in front: the parser hands over the
 * heading as written (`` `D-01` — one loader ``), and the row already shows
 * the id. A rule without a title is named by the first line of its text.
 *
 * @param {{id?: string, title?: string, text?: string}} rule
 */
export function titleOf(rule) {
  const id = String(rule.id ?? "");
  let title = String(rule.title ?? "").trim();
  for (const lead of ["`" + id + "`", id]) {
    if (id && title.startsWith(lead)) {
      title = title.slice(lead.length).replace(/^\s*[—–:-]\s*/, "").trim();
      break;
    }
  }
  if (title) return title;
  return String(rule.text ?? "").trim().split("\n")[0] ?? "";
}

/** A list the engine may encode as `[]` or, when empty, as `{}`. */
export function list(value) {
  return Array.isArray(value) ? value : [];
}

/**
 * The rules the filter shows: one lane or both, and those whose id, title or
 * text contains the query (case does not matter).
 *
 * @param {object[]} rules
 * @param {{lane?: string, query?: string}} filter
 */
export function visibleRules(rules, filter = {}) {
  const lane = filter.lane ?? "all";
  const query = (filter.query ?? "").trim().toLowerCase();
  return rules.filter((rule) => {
    if (lane !== "all" && laneOf(rule) !== lane) return false;
    if (!query) return true;
    return [rule.id, rule.title, rule.text, rule.family].some(
      (field) => typeof field === "string" && field.toLowerCase().includes(query)
    );
  });
}

/** How many rules of each lane, and in all. */
export function laneCounts(rules) {
  let auto = 0;
  for (const rule of rules) if (rule.has_check) auto += 1;
  return { auto, manual: rules.length - auto, total: rules.length };
}

/**
 * A rule file as the list names it: relative to the ruleset it belongs to,
 * with the ruleset's name in front when the project has more than one.
 *
 * @param {string} file
 * @param {{name?: string, path?: string}[]} rulesets
 */
export function fileLabel(file, rulesets) {
  const norm = (p) => String(p ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
  const path = norm(file);
  const lower = path.toLowerCase();
  let best = null;
  for (const ruleset of rulesets) {
    const root = norm(ruleset.path);
    if (!root) continue;
    const inside = lower === root.toLowerCase() || lower.startsWith(root.toLowerCase() + "/");
    if (inside && (!best || root.length > norm(best.path).length)) best = ruleset;
  }
  if (!best) return path.split("/").pop() || path;
  const root = norm(best.path);
  const rel = path.length > root.length ? path.slice(root.length + 1) : root.split("/").pop();
  if (rulesets.length < 2) return rel;
  const name = norm(best.name).split("/").pop() || root.split("/").pop();
  return `${name}: ${rel}`;
}

/**
 * The list as a tree of groups, sub-groups and rules.
 *
 * - `file`: one group per rule file, in it one sub-group per section (a file
 *   holds several families, so the section is what a reader of the file sees);
 * - `family`: one group per family, no sub-groups (a family can span files).
 *
 * Groups and sub-groups keep the order the rules come in (the catalog sorts
 * them by id); only the family groups are sorted by name. A group that the
 * filter emptied is not there.
 *
 * @param {object[]} rules   the visible rules
 * @param {"file"|"family"} grouping
 * @param {object[]} [rulesets]
 * @returns {{key: string, label: string, kind: string, rules: object[],
 *            subgroups: {key: string, label: string, rules: object[]}[]}[]}
 */
export function groupRules(rules, grouping, rulesets = []) {
  const groups = new Map();
  for (const rule of rules) {
    const byFile = grouping !== "family";
    const key = byFile ? `file:${rule.source_file ?? ""}` : `family:${rule.family ?? ""}`;
    let group = groups.get(key);
    if (!group) {
      group = {
        key,
        kind: byFile ? "file" : "family",
        label: byFile ? fileLabel(rule.source_file ?? "", rulesets) : (rule.family ?? ""),
        rules: [],
        subgroups: [],
        sections: new Map(),
      };
      groups.set(key, group);
    }
    group.rules.push(rule);
    if (byFile) {
      const section = rule.section ?? "";
      let sub = group.sections.get(section);
      if (!sub) {
        sub = { key: `${key}#${section}`, label: section, rules: [] };
        group.sections.set(section, sub);
        group.subgroups.push(sub);
      }
      sub.rules.push(rule);
    }
  }
  const out = [...groups.values()].map(({ sections, ...group }) => group);
  if (grouping === "family") out.sort((a, b) => a.label.localeCompare(b.label));
  return out;
}

/** The ids of the visible rules in the order the list draws them. */
export function orderedIds(groups) {
  const ids = [];
  for (const group of groups) {
    if (group.subgroups.length) {
      for (const sub of group.subgroups) for (const rule of sub.rules) ids.push(rule.id);
    } else {
      for (const rule of group.rules) ids.push(rule.id);
    }
  }
  return ids;
}

/**
 * The state of a tri-state checkbox over `rules`: every one selected, some,
 * or none. An empty group is `none`.
 *
 * @param {object[]} rules
 * @param {Set<string>} selected
 * @returns {"all"|"some"|"none"}
 */
export function checkState(rules, selected) {
  let on = 0;
  for (const rule of rules) if (selected.has(rule.id)) on += 1;
  if (on === 0) return "none";
  return on === rules.length ? "all" : "some";
}

/**
 * A click on the checkbox of a group: with every visible rule of it selected
 * the click clears them, otherwise it selects them. `rules` are the visible
 * rules of the group, so what the filter hides is never touched.
 *
 * @param {Set<string>} selected
 * @param {object[]} rules
 * @returns {Set<string>} a new set
 */
export function toggleGroup(selected, rules) {
  const next = new Set(selected);
  const clear = checkState(rules, selected) === "all";
  for (const rule of rules) {
    if (clear) next.delete(rule.id);
    else next.add(rule.id);
  }
  return next;
}

/** A click on one rule. */
export function toggleRule(selected, id) {
  const next = new Set(selected);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return next;
}

/**
 * A shift-click: every visible rule from the last clicked one (`anchor`) to
 * `target`, both included, takes the state the click gives `target`. Without
 * an anchor that is still on screen it is a plain click.
 *
 * @param {Set<string>} selected
 * @param {string[]} order   `orderedIds` of what is shown
 * @param {string|null} anchor
 * @param {string} target
 */
export function selectRange(selected, order, anchor, target) {
  const from = anchor === null ? -1 : order.indexOf(anchor);
  const to = order.indexOf(target);
  if (from < 0 || to < 0) return toggleRule(selected, target);
  const on = !selected.has(target);
  const next = new Set(selected);
  const [lo, hi] = from <= to ? [from, to] : [to, from];
  for (let i = lo; i <= hi; i += 1) {
    if (on) next.add(order[i]);
    else next.delete(order[i]);
  }
  return next;
}

/** "Select all manual in this group": adds the manual rules of `rules`, keeps the rest. */
export function selectManual(selected, rules) {
  const next = new Set(selected);
  for (const rule of rules) if (!rule.has_check) next.add(rule.id);
  return next;
}

/**
 * A click on a gate of `.rules.json` (a named list of families): adds the
 * shown rules of those families, keeps the rest. `rules` are the visible
 * rules, so a gate picked under a filter selects nothing hidden.
 *
 * @param {Set<string>} selected
 * @param {object[]} rules
 * @param {string[]} families
 */
export function selectFamilies(selected, rules, families) {
  const wanted = new Set(list(families));
  const next = new Set(selected);
  for (const rule of rules) if (wanted.has(rule.family)) next.add(rule.id);
  return next;
}

/**
 * The selection, kept to rules that still exist (a ruleset changed between
 * two looks) and split into its lanes. The two are never added up: the bar
 * shows them apart, and Start treats them apart.
 *
 * @param {object[]} rules   every rule of the catalog
 * @param {Set<string>} selected
 * @returns {{auto: string[], manual: string[]}}
 */
export function splitSelection(rules, selected) {
  const auto = [];
  const manual = [];
  for (const rule of rules) {
    if (!selected.has(rule.id)) continue;
    (rule.has_check ? auto : manual).push(rule.id);
  }
  return { auto, manual };
}

/**
 * Rule ids as the selections of one or more `rules/run` calls, each below the
 * limits the engine sets for one call.
 *
 * @param {string[]} ids
 * @returns {string[]}
 */
export function runSelections(ids, maxBytes = MAX_SELECTION_BYTES, maxItems = MAX_SELECTION_ITEMS) {
  const out = [];
  let chunk = [];
  let bytes = 0;
  for (const id of ids) {
    const size = id.length + 1;
    if (chunk.length && (bytes + size > maxBytes || chunk.length >= maxItems)) {
      out.push(chunk.join(","));
      chunk = [];
      bytes = 0;
    }
    chunk.push(id);
    bytes += size;
  }
  if (chunk.length) out.push(chunk.join(","));
  return out;
}

/**
 * The mechanical results of a project, by rule id, after one more answer.
 *
 * `answer` is a `rules/run` answer, or `{ failed: <text> }` when the call
 * itself failed (the engine was stopped at its time limit, it crashed): then
 * every rule that was asked for gets an `error` row with that text, so a run
 * that died is on screen in the rows it concerned and not only in a toast.
 * A rule the engine did not answer for keeps what it had.
 *
 * @param {Map<string, object>} results
 * @param {string[]} asked     the ids of this call
 * @param {object} answer
 * @returns {Map<string, object>} a new map
 */
export function mergeRun(results, asked, answer) {
  const next = new Map(results);
  if (answer && typeof answer.failed === "string") {
    for (const id of asked) {
      next.set(id, { id, status: "error", findings: [{ text: answer.failed }], stopped: true });
    }
    return next;
  }
  for (const result of list(answer?.results)) {
    if (typeof result?.id !== "string") continue;
    next.set(result.id, { ...result, findings: list(result.findings) });
  }
  return next;
}

/**
 * How many rules each mechanical status has, over the rules that still exist.
 *
 * @param {object[]} rules
 * @param {Map<string, object>} results
 */
export function mechanicalCounts(rules, results) {
  const counts = { fail: 0, error: 0, pass: 0, waived: 0 };
  for (const rule of rules) {
    const status = results.get(rule.id)?.status;
    if (status in counts) counts[status] += 1;
  }
  return counts;
}

/**
 * What a person decided about a manual rule, from `rules_judged` (the
 * project's `.rules-verdicts.json` and `.rules-waivers.json`): its verdict,
 * else its waiver while that has not expired, else `null`. A rule with a check
 * gets `null` here: its waiver is the mechanical run's to show (`waived`), and
 * a verdict is only ever written for a rule without one.
 *
 * @param {{id: string, has_check?: boolean}} rule
 * @param {{verdicts?: object, waivers?: object}|null} judged
 * @returns {{kind: "verdict", verdict: string, by: string, date: string}
 *          |{kind: "waived", reason: string, expires: string|null}|null}
 */
export function judgedOf(rule, judged) {
  if (!judged || laneOf(rule) !== "manual") return null;
  const v = judged.verdicts && judged.verdicts[rule.id];
  if (v && typeof v === "object") {
    return { kind: "verdict", verdict: String(v.verdict), by: String(v.by ?? ""), date: String(v.date ?? "") };
  }
  const w = judged.waivers && judged.waivers[rule.id];
  if (w && typeof w === "object" && !w.expired) {
    return { kind: "waived", reason: String(w.reason ?? ""), expires: w.expires ?? null };
  }
  return null;
}

/**
 * The two numbers the sidebar shows for a project: manual rules that are
 * still open (no verdict and no waiver in force, see `judgedOf`), and
 * automated ones whose last run did not pass (`fail` or `error`). `null` when
 * the project has no catalog to count.
 *
 * @param {object|null} catalog
 * @param {Map<string, object>} results
 * @param {{verdicts?: object, waivers?: object}|null} [judged]
 * @returns {{manual: number, failed: number}|null}
 */
export function sidebarCounts(catalog, results, judged = null) {
  if (!catalog || catalog.available !== true) return null;
  const rules = list(catalog.rules);
  const mech = mechanicalCounts(rules, results);
  const open = rules.filter((r) => laneOf(r) === "manual" && !judgedOf(r, judged)).length;
  return { manual: open, failed: mech.fail + mech.error };
}

/**
 * Why a manual rule of the selection would not go to a model, by rule id:
 * the `skipped` list of `rules/plan` (`{ id, status, reason }`).
 *
 * @param {object[]} skipped
 * @returns {Map<string, string>}
 */
export function notSendable(skipped) {
  const out = new Map();
  for (const item of list(skipped)) {
    if (typeof item?.id === "string") out.set(item.id, String(item.reason ?? item.status ?? ""));
  }
  return out;
}

/** The trust states of a ruleset, as the engine side names them. */
export const TRUST_STATES = ["trusted", "untrusted", "changed", "unreadable"];

/**
 * What the tab says about the trust of the rulesets: the ones that have
 * predicate rules and are not trusted (those rules will report an error), in
 * the order of the catalog.
 *
 * @param {object} catalog
 */
export function untrustedRulesets(catalog) {
  return list(catalog?.rulesets).filter(
    (r) => (r.trust?.predicates ?? 0) > 0 && r.trust?.state !== "trusted"
  );
}
