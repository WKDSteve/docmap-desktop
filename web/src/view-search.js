// The search in what the map shows (src-tauri/src/search.rs, `view`): the names, summaries and elements of the nodes of
// a module_map.json, the documents and the features, never the code snippets and never the structure.

const SKIP = new Set([
  'snippet',
  'shape',
  'children',
  'parent',
  'requires',
  'required_by',
  'requires_external',
  'calls_external',
  'stats',
  'depth',
]);

/** The fields of a node that hold an element each, and what a hit in one is called. */
const ELEMENTS = /** @type {const} */ ([
  ['functions', 'function'],
  ['symbols', 'symbol'],
  ['types_detail', 'type'],
  ['bindings', 'binding'],
  ['endpoints', 'endpoint'],
  ['markers', 'marker'],
  ['plugins', 'plugin'],
]);

const OWN = ['name', 'path', 'module', 'summary', 'body', 'language'];
const LIMIT = 200;
const WIDTH = 120;

/** @param {string} line @param {number} at @param {number} length */
function snippet(line, at, length) {
  if (line.length <= WIDTH) return line;
  // The match stays in what is shown: the cut starts a little before it.
  const from = at + length <= WIDTH ? 0 : Math.max(0, at - 30);
  const cut = line.slice(from, from + WIDTH);
  return (from > 0 ? '…' : '') + cut + (from + cut.length < line.length ? '…' : '');
}

/**
 * The first text below `value` that holds `needle`, cut around the match, with the field it was found in.
 * @param {unknown} value
 * @param {string} needle lower case
 * @param {Set<string>} skip
 * @param {string} field
 * @returns {string | null}
 */
function firstMatch(value, needle, skip, field) {
  if (typeof value === 'string') {
    for (const line of value.split(/\r?\n/)) {
      const at = line.toLowerCase().indexOf(needle);
      if (at < 0) continue;
      const shown = snippet(line, at, needle.length);
      return field === '' ? shown : `${field}: ${shown}`;
    }
    return null;
  }
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = firstMatch(item, needle, skip, field);
      if (found !== null) return found;
    }
    return null;
  }
  if (value && typeof value === 'object') {
    for (const [key, inner] of Object.entries(value)) {
      if (skip.has(key)) continue;
      const found = firstMatch(inner, needle, skip, key);
      if (found !== null) return found;
    }
  }
  return null;
}

/** @param {any} v @param {string} key */
const strOf = (v, key) => (v && typeof v[key] === 'string' ? /** @type {string} */ (v[key]) : null);

/**
 * @param {any} map a module_map.json, or null when the project has none
 * @param {string} query
 * @param {number} [limit]
 */
export function searchView(map, query, limit = LIMIT) {
  /** @type {{ kind: string, label: string, detail: string, node: string | null, file: string | null, line: number | null }[]} */
  const hits = [];
  const out = { hits, truncated: false, available: map !== null && typeof map === 'object' };
  const needle = query.trim().toLowerCase();
  if (!out.available || needle === '') return out;
  /** @param {(typeof hits)[number]} hit */
  const push = (hit) => {
    if (hits.length >= limit) {
      out.truncated = true;
      return false;
    }
    hits.push(hit);
    return true;
  };
  const line = (/** @type {any} */ item) => (Number.isInteger(item?.line) && item.line >= 0 ? item.line : null);

  for (const node of Array.isArray(map.nodes) ? map.nodes : []) {
    const id = strOf(node, 'id') ?? '';
    const kind = strOf(node, 'kind') ?? 'module';
    const label = strOf(node, 'name') ?? id;
    const file = strOf(node, 'source') ?? strOf(node, 'path');
    for (const key of OWN) {
      if (node?.[key] === undefined) continue;
      const detail = firstMatch(node[key], needle, SKIP, key);
      if (detail === null) continue;
      if (!push({ kind, label, detail, node: id, file, line: null })) return out;
      break;
    }
    for (const [key, what] of ELEMENTS) {
      if (!Array.isArray(node?.[key])) continue;
      for (const item of node[key]) {
        const detail = firstMatch(item, needle, SKIP, '');
        if (detail === null) continue;
        const name = strOf(item, 'name') ?? strOf(item, 'key') ?? (typeof item === 'string' ? item : label);
        if (!push({ kind: what, label: name, detail, node: id, file, line: line(item) })) return out;
      }
    }
  }
  for (const f of Array.isArray(map.docs?.files) ? map.docs.files : []) {
    const title = strOf(f, 'title') ?? '';
    const path = strOf(f, 'path') ?? '';
    if (!`${title} ${path}`.toLowerCase().includes(needle)) continue;
    if (!push({ kind: 'doc', label: title, detail: path, node: null, file: path, line: null })) return out;
  }
  const featureSkip = new Set(['tab', 'line']);
  for (const f of Array.isArray(map.features?.files) ? map.features.files : []) {
    const page = strOf(f, 'path') ?? '';
    for (const entry of Array.isArray(f?.entries) ? f.entries : []) {
      const detail = firstMatch(entry, needle, featureSkip, '');
      if (detail === null) continue;
      if (!push({ kind: 'feature', label: strOf(entry, 'name') ?? page, detail, node: null, file: page, line: line(entry) })) {
        return out;
      }
    }
  }
  return out;
}
