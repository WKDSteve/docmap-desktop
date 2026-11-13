// The menu of the window as data, read from the one place that defines it: `GROUPS` in src-tauri/src/menu.rs.
//
// The desktop app builds a native menu bar from that table; a browser has none, so the web variant draws the same
// menu in the page (src/menubar.js). Reading the table at build time keeps one definition: a new item in menu.rs is in
// the web menu with the next build, and an entry this reader does not understand fails the build instead of going
// missing.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

/**
 * @typedef {{ type: 'item', id: string, needsProject: boolean } | { type: 'check', id: string }
 *   | { type: 'sub', id: string, items: MenuNode[] } | { type: 'locales' } | { type: 'separator' }} MenuNode
 * @typedef {{ id: string, items: MenuNode[] }} MenuGroup
 */

/** The table without comments and strings confused: Rust `//` comments are cut, string contents are kept. */
function stripComments(source) {
  return source.replace(/"(?:[^"\\]|\\.)*"|\/\/[^\n]*/g, (m) => (m.startsWith('"') ? m : ''));
}

/**
 * @param {string} source the text of menu.rs
 * @returns {MenuGroup[]}
 */
export function parseMenu(source) {
  const start = source.indexOf('const GROUPS: &[Group] = &[');
  if (start < 0) throw new Error('menu.rs: const GROUPS not found');
  const text = stripComments(source.slice(start + 'const GROUPS: &[Group] = &['.length));
  /** @type {MenuGroup[]} */
  const groups = [];
  /** The lists being filled, innermost last. @type {MenuNode[][]} */
  const open = [];
  /** A list that the next `&[` opens. @type {MenuNode[] | null} */
  let next = null;
  const token =
    /Group\s*\{\s*id:\s*"([^"]+)"\s*,\s*items:|item\(\s*"([^"]+)"\s*,\s*(?:Some\(\s*"[^"]*"\s*\)|None)\s*,\s*(true|false)\s*,?\s*\)|check\(\s*"([^"]+)"\s*,\s*(?:Some\(\s*"[^"]*"\s*\)|None)\s*,?\s*\)|Node::Sub\(\s*"([^"]+)"\s*,|Node::Separator|Node::Locales|&\[|\]|[A-Za-z_][A-Za-z0-9_:]*|"/g;
  let depth = 0;
  for (const m of text.matchAll(token)) {
    const [whole, group, item, needs, check, sub] = m;
    const here = open[open.length - 1];
    if (group !== undefined) {
      const g = { id: group, items: [] };
      groups.push(g);
      next = g.items;
    } else if (item !== undefined) {
      here.push({ type: 'item', id: item, needsProject: needs === 'true' });
    } else if (check !== undefined) {
      here.push({ type: 'check', id: check });
    } else if (sub !== undefined) {
      /** @type {MenuNode & { type: 'sub' }} */
      const node = { type: 'sub', id: sub, items: [] };
      here.push(node);
      next = node.items;
    } else if (whole === 'Node::Separator') {
      here.push({ type: 'separator' });
    } else if (whole === 'Node::Locales') {
      here.push({ type: 'locales' });
    } else if (whole === '&[') {
      if (!next) throw new Error('menu.rs: a list that belongs to nothing this reader knows');
      open.push(next);
      next = null;
      depth += 1;
    } else if (whole === ']') {
      // The bracket that closes GROUPS itself ends the table.
      if (depth === 0) break;
      open.pop();
      depth -= 1;
    } else if (whole !== 'Group' && whole !== 'id' && whole !== 'items') {
      // Anything else between the entries is something this reader was not written for.
      if (open.length > 0 && !['Node', 'Some', 'None', 'true', 'false'].includes(whole)) {
        throw new Error(`menu.rs: not understood in GROUPS: ${whole}`);
      }
    }
  }
  if (groups.length === 0 || groups.some((g) => g.items.length === 0)) throw new Error('menu.rs: GROUPS read as empty');
  return groups;
}

/** The menu of this checkout. */
export function menuModel(file = join(here, '..', 'src-tauri', 'src', 'menu.rs')) {
  return parseMenu(readFileSync(file, 'utf8'));
}

/** Every id of a model: groups, items, checks and submenus. @param {MenuGroup[]} groups */
export function menuIds(groups) {
  /** @type {string[]} */
  const ids = [];
  /** @param {MenuNode[]} nodes */
  const walk = (nodes) => {
    for (const n of nodes) {
      if (n.type === 'item' || n.type === 'check') ids.push(n.id);
      else if (n.type === 'sub') {
        ids.push(n.id);
        walk(n.items);
      }
    }
  };
  for (const g of groups) {
    ids.push(g.id);
    walk(g.items);
  }
  return ids;
}
