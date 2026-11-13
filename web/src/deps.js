// What depends on what across the projects of a workspace, from their maps alone (src-tauri/src/deps.rs, `resolve`):
// a module one project requires from outside itself is matched, by its exact name, against the modules the others
// declare.

/**
 * @param {any} map a module_map.json, or null when a project has none
 * @returns {{ declares: string[], requires: string[] } | null}
 */
function factsOf(map) {
  if (!map || !Array.isArray(map.nodes)) return null;
  const declares = [];
  const requires = [];
  for (const node of map.nodes) {
    if (typeof node?.module === 'string') declares.push(node.module);
    if (Array.isArray(node?.requires_external)) {
      for (const r of node.requires_external) if (typeof r === 'string') requires.push(r);
    }
  }
  return { declares, requires };
}

/** @param {string} a @param {string} b */
const byText = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

/**
 * @param {{ id: string, map: any }[]} projects in the order of the workspace: the first to declare a module owns it
 */
export function computeDeps(projects) {
  /** @type {Map<string, string>} */
  const owner = new Map();
  /** @type {{ id: string, requires: string[] }[]} */
  const facts = [];
  const unread = [];
  for (const { id, map } of projects) {
    const f = factsOf(map);
    if (!f) {
      unread.push(id);
      continue;
    }
    for (const m of f.declares) if (!owner.has(m)) owner.set(m, id);
    facts.push({ id, requires: f.requires });
  }
  /** @type {Map<string, { from: string, to: string, count: number, modules: Map<string, number> }>} */
  const edges = new Map();
  /** @type {Map<string, { count: number, projects: string[] }>} */
  const outside = new Map();
  for (const { id, requires } of facts) {
    for (const req of requires) {
      const to = owner.get(req);
      if (to === id) continue;
      if (to !== undefined) {
        const key = `${id}\0${to}`;
        const edge = edges.get(key) ?? { from: id, to, count: 0, modules: new Map() };
        edge.count += 1;
        edge.modules.set(req, (edge.modules.get(req) ?? 0) + 1);
        edges.set(key, edge);
      } else {
        const out = outside.get(req) ?? { count: 0, projects: [] };
        out.count += 1;
        if (!out.projects.includes(id)) out.projects.push(id);
        outside.set(req, out);
      }
    }
  }
  return {
    edges: [...edges.values()]
      .map((e) => ({
        from: e.from,
        to: e.to,
        count: e.count,
        modules: [...e.modules].sort((a, b) => b[1] - a[1] || byText(a[0], b[0])).map(([m]) => m),
      }))
      .sort((a, b) => b.count - a.count || byText(a.from, b.from) || byText(a.to, b.to)),
    outside: [...outside]
      .map(([name, o]) => ({ name, count: o.count, projects: [...o.projects].sort(byText) }))
      .sort((a, b) => b.count - a.count || byText(a.name, b.name)),
    unread: unread.sort(byText),
  };
}
