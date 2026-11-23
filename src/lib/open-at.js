// Open the map at a file and line: what an outside caller (tasks-desktop's "open in DocMap", a start argument, the
// address of the web page) may ask for, and how it is resolved to a node of the map.
//
// The Rust side (`src-tauri/src/open_at.rs`) answers the same cases; `open-at-cases.json` holds both to one list.
// Input from outside is untrusted: a path is accepted only when it names a file *inside* a repository, relative to
// it. Nothing here reads a disk.

/**
 * A repository-relative, forward-slash path, or null when the text is not one: empty, absolute (`/x`, `\\x`, `C:\x`,
 * `C:x`), home or variable (`~`, `$HOME`, `%VAR%`), with a `..` segment, a NUL, or a colon (a stream name).
 * @param {unknown} raw
 * @returns {string | null}
 */
export function relativePath(raw) {
  if (typeof raw !== "string" || raw === "" || raw.includes("\0")) return null;
  if (/^[\\/~$%]/.test(raw) || raw.includes(":")) return null;
  const parts = [];
  for (const part of raw.replace(/\\/g, "/").split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") return null;
    parts.push(part);
  }
  return parts.length ? parts.join("/") : null;
}

/**
 * `file:line` or `file:first-last` → `{ file, line, end }`; null when it is neither, or the file is not
 * repository-relative. Lines are 1-based.
 * @param {unknown} raw
 * @returns {{ file: string, line: number, end: number | null } | null}
 */
export function parseTarget(raw) {
  if (typeof raw !== "string") return null;
  const m = /^(.+):(\d+)(?:-(\d+))?$/.exec(raw);
  if (!m) return null;
  const file = relativePath(m[1]);
  const line = Number(m[2]);
  if (!file || line < 1) return null;
  const end = m[3] === undefined ? null : Number(m[3]);
  if (end !== null && end < 1) return null;
  return { file, line, end };
}

/**
 * The known project a caller means, by id, name or root: never a path that is then read. `null` when there is none,
 * `"ambiguous"` when two projects share the name.
 * @param {{ id: string, name: string, root: string }[]} projects
 * @param {string} repo
 * @returns {{ id: string, name: string, root: string } | null | "ambiguous"}
 */
export function pickProject(projects, repo) {
  const want = String(repo ?? "").trim();
  if (!want) return null;
  const byId = projects.find((p) => p.id === want);
  if (byId) return byId;
  const norm = (/** @type {string} */ s) => s.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  const byName = projects.filter((p) => p.name.toLowerCase() === want.toLowerCase());
  if (byName.length === 1) return byName[0];
  if (byName.length > 1) return "ambiguous";
  return projects.find((p) => norm(p.root) === norm(want)) ?? null;
}

/**
 * The innermost function that holds the lines, else the one the range overlaps most, else the module that owns the
 * file, else null. The same answer as docmap.nvim's `find.at` (`core/find.lua`), read from `module_map.json`.
 * @param {any} map the parsed module_map.json
 * @param {string} file repository-relative
 * @param {number} line
 * @param {number | null} [end]
 * @returns {{ id: string, key: string | null } | null}
 */
export function resolveAt(map, file, line, end = null) {
  const want = String(file).replace(/\\/g, "/").replace(/^\.\//, "");
  const node = (map?.nodes ?? []).find((n) => typeof n.source === "string" && n.source.replace(/\\/g, "/") === want);
  if (!node) return null;
  const first = Math.min(line, end ?? line);
  const last = Math.max(line, end ?? line);
  const fns = Array.isArray(node.functions) ? node.functions : [];
  let best = null;
  let span = Infinity;
  for (const fn of fns) {
    const bottom = fn.line_end ?? fn.line;
    if (fn.line <= first && first <= bottom && bottom - fn.line < span) {
      best = fn;
      span = bottom - fn.line;
    }
  }
  if (!best) {
    let most = 0;
    for (const fn of fns) {
      const overlap = Math.min(fn.line_end ?? fn.line, last) - Math.max(fn.line, first) + 1;
      if (overlap > most) {
        best = fn;
        most = overlap;
      }
    }
  }
  return { id: node.id, key: best ? `${node.id}#${best.name}` : null };
}

/**
 * The fragment a map is loaded with: the panel the page last reported, or the node this window opened it at (then
 * the tree, unless the reader has since moved to another panel).
 * @param {string | null} tab
 * @param {{ id: string, key: string | null } | null} node
 */
export function mapHash(tab, node) {
  if (node && (!tab || tab === "tree")) return nodeFragment(node);
  return tab ? "#tab=" + encodeURIComponent(tab) : "";
}

/**
 * `#tab=tree&id=<node>[&fn=<key>]`: what the page reads to select the node (and the function).
 * @param {{ id: string, key: string | null }} hit
 */
export function nodeFragment(hit) {
  return "#tab=tree&id=" + encodeURIComponent(hit.id) + (hit.key ? "&fn=" + encodeURIComponent(hit.key) : "");
}
