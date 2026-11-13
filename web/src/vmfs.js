// What the window asks about a project's files, answered from the file system inside the VM (the folder as it was
// read): the tree, a search, the languages, whether the map is older than the sources. The desktop app answers these
// from the disk in Rust (filetree.rs, search.rs, languages.rs, freshness.rs); the shapes are the same, so the page
// does not know which one it talks to.

import { isDir, isFile, mkdirTree, removeTree } from './engine.js';
import { VENDOR_DIRS } from './folder-read.js';

/**
 * @typedef {import('./engine.js').MemFs & { open(path: string, flags: string): { node?: { timestamp?: number } },
 *   close(stream: object): void }} Fs
 */

/**
 * Set the time a file was last changed (milliseconds). The file system of the VM (Emscripten's MEMFS, as wasmoon builds
 * it) exports no `utime`; the node of an open file carries the one time `stat` reports.
 * @param {Fs} fs
 * @param {string} path
 * @param {number} ms
 */
export function setMtime(fs, path, ms) {
  const stream = fs.open(path, 'r');
  try {
    if (!stream.node || typeof stream.node.timestamp !== 'number') {
      throw new Error('the file system of the VM keeps no time this page can set');
    }
    stream.node.timestamp = ms;
  } finally {
    fs.close(stream);
  }
}

/** The folders the window does not count as sources (languages.rs, `SKIP_DIRS`). */
export const SKIP_DIRS = new Set([
  '.git', '.claude', '.deps', '.direnv', 'third_party', 'bower_components', '.stack-work', '.dart_tool', '.hg',
  '.svn', '.jj', 'node_modules', 'vendor', 'target', 'dist', 'build', 'out', '.next', '.nuxt', '.svelte-kit',
  '__pycache__', '.venv', 'venv', '.tox', '.mypy_cache', '.pytest_cache', '.ruff_cache', '.gradle', '.idea',
  '.vscode', '.cache', 'coverage', '.terraform', 'Pods', 'DerivedData',
]);

const MAX_FILES = 20_000;

/**
 * Replace what is at `root` with `files` (path relative to it -> bytes), each with its time of change.
 * @param {Fs} fs
 * @param {string} root
 * @param {Record<string, Uint8Array>} files
 * @param {Record<string, number>} mtimes milliseconds
 */
export function mountTree(fs, root, files, mtimes = {}) {
  unmountTree(fs, root);
  mkdirTree(fs, root);
  for (const [rel, bytes] of Object.entries(files)) writeFileAt(fs, `${root}/${rel}`, bytes, mtimes[rel]);
}

/**
 * @param {Fs} fs
 * @param {string} path
 * @param {Uint8Array} bytes
 * @param {number} [mtime] milliseconds
 */
export function writeFileAt(fs, path, bytes, mtime) {
  const dir = path.slice(0, path.lastIndexOf('/'));
  if (dir) mkdirTree(fs, dir);
  fs.writeFile(path, bytes);
  if (typeof mtime === 'number' && mtime > 0) setMtime(fs, path, mtime);
}

/** @param {Fs} fs @param {string} root */
export function unmountTree(fs, root) {
  try {
    if (isDir(fs.stat(root).mode)) removeTree(fs, root);
  } catch {
    // nothing there
  }
}

/**
 * @param {Fs} fs
 * @param {string} path
 * @returns {{ bytes: Uint8Array, mtime: number } | null} `mtime` in milliseconds
 */
export function readFileAt(fs, path) {
  try {
    const st = fs.stat(path);
    if (!isFile(st.mode)) return null;
    return { bytes: fs.readFile(path), mtime: st.mtime.getTime() };
  } catch {
    return null;
  }
}

/** The files below `dir`, as path relative to it -> bytes. @param {Fs} fs @param {string} dir */
export function readTree(fs, dir) {
  /** @type {Record<string, Uint8Array>} */
  const out = {};
  /** @param {string} at @param {string} prefix */
  const walk = (at, prefix) => {
    let names;
    try {
      names = fs.readdir(at);
    } catch {
      return;
    }
    for (const name of names) {
      if (name === '.' || name === '..') continue;
      const path = `${at}/${name}`;
      const st = fs.stat(path);
      if (isDir(st.mode)) walk(path, `${prefix}${name}/`);
      else if (isFile(st.mode)) out[prefix + name] = fs.readFile(path);
    }
  };
  walk(dir, '');
  return out;
}

/** A path relative to a root: plain segments only. @param {string} sub */
export function plainSub(sub) {
  if (sub === '' || sub === '.') return '';
  const parts = sub.split('/').filter((p) => p !== '');
  if (parts.some((p) => p === '.' || p === '..' || /[\\\0]/.test(p))) throw new Error(`not a path inside the project: ${sub}`);
  return parts.join('/');
}

/**
 * One level of the tree (filetree.rs, `Listing`): folders first, then files, each by name.
 * @param {Fs} fs
 * @param {string} root
 * @param {string} sub
 */
export function fileTree(fs, root, sub) {
  const rel = plainSub(sub);
  const dir = rel ? `${root}/${rel}` : root;
  const entries = [];
  for (const name of fs.readdir(dir)) {
    if (name === '.' || name === '..') continue;
    const st = fs.stat(`${dir}/${name}`);
    const folder = isDir(st.mode);
    entries.push({
      name,
      path: rel ? `${rel}/${name}` : name,
      isDir: folder,
      size: folder ? null : st.size,
      modified: folder ? null : Math.floor(st.mtime.getTime() / 1000),
      skipped: false,
      nestedRepo: false,
      untracked: false,
      ignored: false,
    });
  }
  const byName = (/** @type {{name: string}} */ a, /** @type {{name: string}} */ b) =>
    a.name.toLowerCase() < b.name.toLowerCase() ? -1 : a.name.toLowerCase() > b.name.toLowerCase() ? 1 : 0;
  entries.sort((a, b) => (a.isDir === b.isDir ? byName(a, b) : a.isDir ? -1 : 1));
  return { entries, path: rel };
}

/**
 * The source files of a project: everything below `root` outside the skipped folders and the map's own folder.
 * @param {Fs} fs
 * @param {string} root
 * @param {string} mapDir absolute, inside the VM
 * @param {(rel: string, path: string, st: { size: number, mtime: Date }) => void} visit
 * @returns {boolean} whether the walk stopped at the limit
 */
export function walkSources(fs, root, mapDir, visit) {
  let visited = 0;
  const stack = [root];
  while (stack.length > 0) {
    const dir = /** @type {string} */ (stack.pop());
    let names;
    try {
      names = fs.readdir(dir);
    } catch {
      continue;
    }
    for (const name of names.sort()) {
      if (name === '.' || name === '..') continue;
      const path = `${dir}/${name}`;
      const st = fs.stat(path);
      if (isDir(st.mode)) {
        if (!SKIP_DIRS.has(name) && path !== mapDir) stack.push(path);
        continue;
      }
      if (!isFile(st.mode)) continue;
      if (visited >= MAX_FILES) return true;
      visited += 1;
      visit(path.slice(root.length + 1), path, st);
    }
  }
  return false;
}

/** @param {Fs} fs @param {string} mapDir */
function mapTime(fs, mapDir) {
  try {
    const st = fs.stat(`${mapDir}/module_map.json`);
    return isFile(st.mode) ? st.mtime.getTime() : null;
  } catch {
    return null;
  }
}

/**
 * Whether the map is older than the newest source (freshness.rs, `check`).
 * @param {Fs} fs
 * @param {string} root
 * @param {string} mapDir
 * @param {number} now milliseconds
 */
export function freshness(fs, root, mapDir, now) {
  const made = mapTime(fs, mapDir);
  if (made === null) {
    return { hasMap: false, stale: false, newest: null, behindSecs: null, truncated: false, generatedSecs: null };
  }
  /** @type {{ at: number, rel: string } | null} */
  let newest = null;
  const truncated = walkSources(fs, root, mapDir, (rel, _path, st) => {
    const at = st.mtime.getTime();
    if (newest === null || at > newest.at) newest = { at, rel };
  });
  const found = /** @type {{ at: number, rel: string } | null} */ (newest);
  // Stale by any amount, as the app counts it: a file saved half a second after the map is newer than the map.
  const stale = found !== null && found.at > made;
  return {
    hasMap: true,
    stale,
    newest: found ? found.rel : null,
    behindSecs: found !== null && stale ? Math.floor((found.at - made) / 1000) : null,
    truncated,
    generatedSecs: now >= made ? Math.floor((now - made) / 1000) : null,
  };
}

/**
 * The files changed after the map was written, newest first (freshness.rs, `changed_since_map`).
 * @param {Fs} fs
 * @param {string} root
 * @param {string} mapDir
 * @param {number} limit
 */
export function changedSinceMap(fs, root, mapDir, limit) {
  const made = mapTime(fs, mapDir);
  if (made === null) return { files: [], total: 0, truncated: false };
  /** @type {{ path: string, afterSecs: number, after: number }[]} */
  const found = [];
  const truncated = walkSources(fs, root, mapDir, (rel, _path, st) => {
    const after = st.mtime.getTime() - made;
    if (after > 0) found.push({ path: rel, afterSecs: Math.floor(after / 1000), after });
  });
  found.sort((a, b) => b.after - a.after);
  return {
    files: found.slice(0, limit).map(({ path, afterSecs }) => ({ path, afterSecs })),
    total: found.length,
    truncated,
  };
}

/** Extension -> [name, grammar, backend] (languages.rs, `language_for`). */
const LANGUAGES = (() => {
  /** @type {Record<string, [string, string | null, string | null]>} */
  const table = {};
  /** @param {string} exts @param {string} name @param {string | null} grammar @param {string | null} [backend] */
  const add = (exts, name, grammar, backend = null) => {
    for (const ext of exts.split(' ')) table[ext] = [name, grammar, backend];
  };
  add('s asm nasm inc', 'Assembly', null, 'asm');
  add('lua', 'Lua', 'lua');
  add('js mjs cjs', 'JavaScript', 'javascript');
  add('jsx', 'JSX', 'javascript');
  add('ts mts cts', 'TypeScript', 'typescript');
  add('tsx', 'TSX', 'tsx');
  add('py pyi', 'Python', 'python');
  add('rs', 'Rust', 'rust');
  add('go', 'Go', 'go');
  add('c h', 'C', 'c');
  add('cc cpp cxx hpp hh hxx', 'C++', 'cpp');
  add('cs', 'C#', 'c_sharp');
  add('java', 'Java', 'java');
  add('kt kts', 'Kotlin', 'kotlin');
  add('swift', 'Swift', 'swift');
  add('m mm', 'Objective-C', 'objc');
  add('rb', 'Ruby', 'ruby');
  add('php', 'PHP', 'php');
  add('pl pm', 'Perl', 'perl');
  add('sh bash zsh', 'Shell', 'bash');
  add('ps1 psm1', 'PowerShell', 'powershell');
  add('vim', 'Vimscript', 'vim');
  add('el', 'Emacs Lisp', 'elisp');
  add('ex exs', 'Elixir', 'elixir');
  add('erl hrl', 'Erlang', 'erlang');
  add('hs', 'Haskell', 'haskell');
  add('ml mli', 'OCaml', 'ocaml');
  add('scala sc', 'Scala', 'scala');
  add('clj cljs cljc', 'Clojure', 'clojure');
  add('dart', 'Dart', 'dart');
  add('zig', 'Zig', 'zig');
  add('nim', 'Nim', 'nim');
  add('jl', 'Julia', 'julia');
  add('r', 'R', 'r');
  add('sql', 'SQL', 'sql');
  add('vue', 'Vue', 'vue');
  add('svelte', 'Svelte', 'svelte');
  add('css scss sass less', 'CSS', 'css');
  add('html htm', 'HTML', 'html');
  return table;
})();

/** @param {string} name */
function extension(name) {
  const dot = name.lastIndexOf('.');
  return dot >= 0 && dot < name.length - 1 ? name.slice(dot + 1).toLowerCase() : null;
}

/**
 * The languages of a project by file count, most files first (languages.rs, `scan`).
 * @param {Fs} fs
 * @param {string} root
 * @param {string} mapDir
 */
export function scanLanguages(fs, root, mapDir) {
  /** @type {Map<string, { name: string, files: number, grammar: string | null, backend: string | null }>} */
  const counts = new Map();
  const truncated = walkSources(fs, root, mapDir, (rel) => {
    const ext = extension(rel.slice(rel.lastIndexOf('/') + 1));
    const known = ext === null ? undefined : LANGUAGES[ext];
    if (!known) return;
    const slot = counts.get(known[0]) ?? { name: known[0], files: 0, grammar: known[1], backend: known[2] };
    slot.files += 1;
    counts.set(known[0], slot);
  });
  const languages = [...counts.values()].sort((a, b) => b.files - a.files || (a.name < b.name ? -1 : 1));
  return { languages, total: languages.reduce((n, l) => n + l.files, 0), truncated };
}

const MAX_HITS = 500;
const MAX_LINE = 240;
const decoder = new TextDecoder('utf-8', { fatal: false });

/**
 * A search in the files of a project (search.rs, `Results`): `mode` "files" matches the path, anything else the text,
 * line by line. A file with a NUL byte is binary and left out.
 * @param {Fs} fs
 * @param {string} root
 * @param {string} mapDir
 * @param {{ sub?: string, query: string, mode?: string, caseSensitive?: boolean }} ask
 */
export function searchFiles(fs, root, mapDir, ask) {
  const query = ask.caseSensitive ? ask.query : ask.query.toLowerCase();
  /** @type {{ path: string, line: number | null, text: string | null, at: number | null }[]} */
  const hits = [];
  if (query === '') return { hits, filesSearched: 0, truncated: false, reason: null, cappedFiles: 0 };
  const sub = plainSub(ask.sub ?? '');
  const prefix = sub ? `${sub}/` : '';
  let filesSearched = 0;
  let truncated = false;
  const stopped = walkSources(fs, root, mapDir, (rel, path) => {
    if (truncated || (prefix && !rel.startsWith(prefix))) return;
    filesSearched += 1;
    if (ask.mode === 'files') {
      const name = ask.caseSensitive ? rel : rel.toLowerCase();
      if (name.includes(query)) hits.push({ path: rel, line: null, text: null, at: null });
    } else {
      const bytes = fs.readFile(path);
      if (bytes.includes(0)) return;
      const lines = decoder.decode(bytes).split('\n');
      for (let i = 0; i < lines.length && hits.length < MAX_HITS; i++) {
        const line = lines[i];
        const at = (ask.caseSensitive ? line : line.toLowerCase()).indexOf(query);
        if (at < 0) continue;
        // A long line is cut around the hit, so the hit stays in what is shown.
        const from = at > MAX_LINE - query.length ? Math.max(0, at - 40) : 0;
        hits.push({ path: rel, line: i + 1, text: line.slice(from, from + MAX_LINE).replace(/\r$/, ''), at: at - from });
      }
    }
    if (hits.length >= MAX_HITS) truncated = true;
  });
  return { hits, filesSearched, truncated: truncated || stopped, reason: truncated ? 'hits' : null, cappedFiles: 0 };
}

export { VENDOR_DIRS };
