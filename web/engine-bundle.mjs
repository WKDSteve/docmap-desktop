// Bundles the Lua sources of the engine for the web variant: docmap.nvim and lib.nvim at the commits of engine.lock.
//
//   node engine-bundle.mjs [--out=<file>] [--report]        (from web/)
//
// The sources come from the git objects of the pinned commits, never from the working tree, so a local change in a
// checkout cannot slip into the page. The checkouts are DOCMAP_ENGINE_DIR and LIB_NVIM_DIR, else REPOS_DIR/docmap.nvim
// and REPOS_DIR/lib.nvim. The bundle is what the standalone engine of docmap.nvim runs (standalone/docmap.lua): its vim
// shim (standalone/vim_shim.lua, the same file the sidecar is built from), the pipeline under lua/docmap/core and
// lua/docmap/config whole (the language backends are required by a name made at run time), and of lib.nvim the closure
// of what those require by a literal name. A small Lua lexer finds the requires, so a require in a comment or a string
// is not one, and it drops the comments: the newlines stay, so a line number in an error is still the line of the file.
// `--report` prints the modules, their size and the requires that are not a literal.
//
// The lexer and the reading of git objects are those of tasks-desktop's ui/web/engine-bundle.mjs (the pilot).

import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..');

// Whole trees of the engine: every module under them is in the bundle.
const WHOLE = ['docmap.core', 'docmap.config'];
// Single modules of the engine besides: the shim of the standalone build and what it requires by name.
const ENTRY = ['standalone.vim_shim', 'standalone.shell_quote'];
// Never in the bundle: the binding to the C parser of the sidecar (there is no parser in the page, the shim keeps its
// inert vim.treesitter) and the Windows link reader (the VM is no Windows).
const LEFT_OUT = new Set(['standalone.treesitter', 'standalone.win_links']);

/** @returns {{ repo: string, commit: string, lib_commit: string, schema: number, routes: string[] }} */
export function readLock(file = join(repoRoot, 'engine.lock')) {
  const lock = {};
  for (const line of readFileSync(file, 'utf8').split(/\r?\n/)) {
    const m = line.match(/^\s*([a-z_]+)\s*=\s*(.*?)\s*$/);
    if (m) lock[m[1]] = m[2];
  }
  if (!/^[0-9a-f]{40}$/.test(lock.commit ?? '') || !/^[0-9a-f]{40}$/.test(lock.lib_commit ?? '')) {
    throw new Error('engine.lock: commit and lib_commit must be full commit hashes');
  }
  return {
    repo: lock.repo,
    commit: lock.commit,
    lib_commit: lock.lib_commit,
    schema: Number(lock.schema),
    routes: (lock.routes ?? '').split(/\s+/).filter(Boolean),
  };
}

export function checkout(envName, name) {
  if (process.env[envName]) return process.env[envName];
  if (process.env.REPOS_DIR) return join(process.env.REPOS_DIR, name);
  throw new Error(`${envName} or REPOS_DIR must name the ${name} checkout`);
}

function git(dir, args) {
  return execFileSync('git', ['-C', dir, ...args], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
}

/**
 * The files of one commit under `prefixes`, as a map path -> content (Buffer), read in one `git cat-file --batch`.
 * @param {string} dir
 * @param {string} commit
 * @param {string[]} prefixes
 */
export function gitFiles(dir, commit, prefixes) {
  // -z: without it git quotes a path outside ASCII ("lua/\303\244.lua"), and cat-file would not find it
  const paths = git(dir, ['ls-tree', '-r', '-z', '--name-only', commit, '--', ...prefixes])
    .split('\0')
    .filter(Boolean);
  const out = execFileSync('git', ['-C', dir, 'cat-file', '--batch'], {
    input: paths.map((p) => `${commit}:${p}\n`).join(''),
    maxBuffer: 256 * 1024 * 1024,
  });
  const files = new Map();
  let at = 0;
  for (const path of paths) {
    const eol = out.indexOf(0x0a, at);
    const header = out.subarray(at, eol).toString('utf8');
    if (!/^[0-9a-f]+ blob \d+$/.test(header)) throw new Error(`git cat-file: ${header} (${path})`);
    const size = Number(header.split(' ')[2]);
    files.set(path, out.subarray(eol + 1, eol + 1 + size));
    at = eol + 1 + size + 1;
  }
  return files;
}

/**
 * The Lua files of one commit, as a map module name -> source. Under `lua/` the name starts after it; under another
 * folder (`standalone/`) the folder is part of the name. `x.lua` wins over `x/init.lua`, as in Lua's search order.
 */
export function luaFiles(dir, commit, prefixes = ['lua/']) {
  const files = new Map();
  const fromInit = new Set();
  for (const [path, content] of gitFiles(dir, commit, prefixes)) {
    if (!path.endsWith('.lua')) continue;
    const isInit = path.endsWith('/init.lua');
    const rel = path.startsWith('lua/') ? path.slice('lua/'.length) : path;
    const mod = rel
      .slice(0, -'.lua'.length)
      .replace(/\/init$/, '')
      .replaceAll('/', '.');
    if (files.has(mod) && (isInit || !fromInit.has(mod))) continue;
    files.set(mod, content.toString('utf8'));
    if (isInit) fromInit.add(mod);
    else fromInit.delete(mod);
  }
  return files;
}

/**
 * The modules directly inside the folder of `mod` (`mod.x`, not `mod.x.y`). lib.nvim's folders are registries: the
 * `init.lua` of a folder requires its siblings by a name made at run time (lib/nvim/registry.lua), which no lexer
 * sees. So a module that has such siblings brings them along.
 * @param {Map<string, string>} files
 * @param {string} mod
 */
export function childrenOf(files, mod) {
  const prefix = `${mod}.`;
  // `@types` holds annotations for the language server; one that is required by name is found as a literal.
  return [...files.keys()].filter((m) => {
    const rest = m.startsWith(prefix) ? m.slice(prefix.length) : '.';
    return !rest.includes('.') && rest !== '@types';
  });
}

/**
 * Lua tokens of `source`: names, strings (with their value), other symbols; comments are skipped but reported as
 * ranges, so they can be cut out. Follows the Lua 5.1-5.4 lexer where it matters for that: long brackets
 * (`[[`, `[==[`), quoted strings with escapes, `--` comments of both kinds.
 * @param {string} source
 */
export function lex(source) {
  const tokens = [];
  const comments = [];
  const n = source.length;
  let i = 0;
  const longBracket = (at) => {
    // `[` `=`* `[` at `at`: the level, or -1
    let j = at + 1;
    while (source[j] === '=') j++;
    return source[j] === '[' ? j - at - 1 : -1;
  };
  const closeLong = (from, level) => {
    const end = source.indexOf(`]${'='.repeat(level)}]`, from);
    if (end < 0) throw new Error(`unfinished long bracket at ${from}`);
    return end + level + 2;
  };
  while (i < n) {
    const c = source[i];
    if (c === '-' && source[i + 1] === '-') {
      const start = i;
      i += 2;
      const level = source[i] === '[' ? longBracket(i) : -1;
      if (level >= 0) i = closeLong(i + level + 2, level);
      else {
        const nl = source.indexOf('\n', i);
        i = nl < 0 ? n : nl;
      }
      comments.push([start, i]);
    } else if (c === '[' && longBracket(i) >= 0) {
      const level = longBracket(i);
      let from = i + level + 2;
      // a newline right after the opening bracket is not part of the string
      if (source[from] === '\r') from++;
      if (source[from] === '\n') from++;
      const end = closeLong(from, level);
      tokens.push({ type: 'string', value: source.slice(from, end - level - 2), at: i });
      i = end;
    } else if (c === '"' || c === "'") {
      let j = i + 1;
      let value = '';
      while (j < n && source[j] !== c) {
        if (source[j] === '\\') {
          value += source[j + 1] ?? '';
          j += 2;
        } else if (source[j] === '\n') {
          throw new Error(`unfinished string at ${i}`);
        } else value += source[j++];
      }
      tokens.push({ type: 'string', value, at: i });
      i = j + 1;
    } else if (/[A-Za-z_]/.test(c)) {
      let j = i + 1;
      while (j < n && /\w/.test(source[j])) j++;
      tokens.push({ type: 'name', value: source.slice(i, j), at: i });
      i = j;
    } else if (/\s/.test(c)) {
      i++;
    } else {
      tokens.push({ type: 'symbol', value: c, at: i });
      i++;
    }
  }
  return { tokens, comments };
}

/** The requires of `source`: literal names, and the arguments of the others (for the report). */
export function requiresOf(tokens) {
  const literal = [];
  const dynamic = [];
  for (let k = 0; k < tokens.length; k++) {
    const t = tokens[k];
    if (t.type !== 'name' || t.value !== 'require') continue;
    const prev = tokens[k - 1];
    const next = tokens[k + 1];
    const after = tokens[k + 2];
    if (next?.type === 'string') literal.push(next.value);
    else if (next?.value === '(' && after?.type === 'string' && tokens[k + 3]?.value === ')') literal.push(after.value);
    else if (next?.value === ',' && prev?.value === '(' && tokens[k - 2]?.value === 'pcall') {
      // `pcall(require, "x")`; `pcall(require, "x" .. y)` is a name made at run time
      if (after?.type === 'string' && tokens[k + 3]?.value === ')') literal.push(after.value);
      else dynamic.push(after ? `${after.value}...` : '?');
    } else if (next?.value === '(') dynamic.push(after ? `${after.value}...` : '?');
  }
  return { literal, dynamic };
}

/** `source` without its comments; every newline of a comment stays, so the lines keep their numbers. */
export function stripComments(source, comments) {
  let out = '';
  let at = 0;
  for (const [start, end] of comments) {
    out += source.slice(at, start) + source.slice(start, end).replace(/[^\n]/g, '');
    at = end;
  }
  return out + source.slice(at);
}

/**
 * The bundle: the whole trees, the entries and the closure of their literal requires.
 * @returns {{ engine: { commit: string, lib_commit: string }, modules: Record<string, string>, report: object[],
 *   missing: string[] }}
 */
export function bundle({ lock = readLock(), engineDir, libDir, strip = true } = {}) {
  engineDir ??= checkout('DOCMAP_ENGINE_DIR', 'docmap.nvim');
  libDir ??= checkout('LIB_NVIM_DIR', 'lib.nvim');
  const engine = luaFiles(engineDir, lock.commit, ['lua/', 'standalone/']);
  const sources = [engine, luaFiles(libDir, lock.lib_commit)];
  const modules = {};
  const report = [];
  const whole = [...engine.keys()].filter(
    (m) => WHOLE.some((w) => m === w || m.startsWith(`${w}.`)) && !m.includes('@types'),
  );
  const queue = [...ENTRY, ...whole.sort()];
  const missing = new Set();
  while (queue.length > 0) {
    const mod = queue.shift();
    if (mod in modules || LEFT_OUT.has(mod)) continue;
    const src = sources.find((s) => s.has(mod));
    if (!src) {
      missing.add(mod);
      continue;
    }
    const text = src.get(mod);
    const { tokens, comments } = lex(text);
    const code = stripComments(text, comments);
    modules[mod] = strip ? code : text;
    const { literal, dynamic } = requiresOf(tokens);
    report.push({ mod, bytes: text.length, stripped: code.length, requires: literal, dynamic });
    queue.push(...literal, ...childrenOf(src, mod));
  }
  return {
    engine: { commit: lock.commit, lib_commit: lock.lib_commit },
    modules,
    report,
    missing: [...missing].sort(),
  };
}

/** The bundle as the text the page carries: the keys sorted, so the same sources give the same bytes. */
export function bundleJson({ engine, modules }) {
  const sorted = Object.fromEntries(
    Object.keys(modules)
      .sort()
      .map((k) => [k, modules[k]]),
  );
  return JSON.stringify({ engine, modules: sorted });
}

function main() {
  const outArg = process.argv.find((a) => a.startsWith('--out='));
  const out = resolve(outArg ? outArg.slice('--out='.length) : join(here, 'dist', 'engine.json'));
  const made = bundle();
  if (process.argv.includes('--report')) {
    for (const r of made.report) {
      console.log(`${r.mod}  ${r.bytes} -> ${r.stripped}${r.dynamic.length ? '  dynamic: ' + r.dynamic.join(' | ') : ''}`);
    }
    const raw = made.report.reduce((n, r) => n + r.bytes, 0);
    const stripped = made.report.reduce((n, r) => n + r.stripped, 0);
    console.log(`\n${made.report.length} modules, ${raw} bytes, ${stripped} without comments`);
    console.log('not found (optional, built-in or named in a string): ' + made.missing.join(', '));
  }
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, bundleJson(made));
  console.log(`${out}  ${Object.keys(made.modules).length} modules`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
