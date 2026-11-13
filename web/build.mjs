// Builds the web variant: one file, web/dist/docmap.html, opened from disk (file://).
//
//   pnpm build                       (= node build.mjs, from web/)
//   node build.mjs [--out=<folder>]
//
// In:  the page of the app as it is (src/index.html, its stylesheets, src/main.js), the shell of the web variant
//      (web/src), the engine of docmap.nvim at the commits of engine.lock (engine-bundle.mjs) and the Lua VM (wasmoon).
// Out: <out>/docmap.html and <out>/SHA256SUMS. Everything is in the file: the script and the style inline, the fonts
//      and the icon as data, the parts of the engine (worker, WASM, Lua sources, host) gzip-compressed as base64, and
//      a Content-Security-Policy as a meta tag that forbids every request.
//
// The packing and its check are the kit's (appkit-web/pack), as in tasks-desktop. This file names what the docmap page
// carries: its parts, its policy (the one of a page that runs an engine, plus a frame from a blob for the map and
// fonts from data), its title. What the script does at run time is the browser test's to check (e2e/): it opens the
// file and counts the requests.

import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build as esbuild } from 'esbuild';
import { assemble, verifyShell, writePage } from 'appkit-web/pack';
import { bundle, bundleJson, readLock } from './engine-bundle.mjs';
import { menuModel } from './menu-model.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..');
const src = join(repo, 'src');

/** The parts of the page, in the order they are written. */
export const PART_IDS = ['worker', 'wasm', 'bundle', 'host'];

/**
 * Nothing may leave the page: no request of any kind. Scripts and styles only inline; the engine's worker and the map's
 * frame only from a blob the page made itself; fonts and images only from data in the file.
 */
export const CSP = [
  "default-src 'none'",
  "script-src 'unsafe-inline' 'wasm-unsafe-eval'",
  "style-src 'unsafe-inline'",
  'img-src data: blob:',
  'font-src data:',
  'worker-src blob:',
  'frame-src blob:',
  "connect-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
].join('; ');

const MIME = { '.svg': 'image/svg+xml', '.png': 'image/png', '.ttf': 'font/ttf', '.woff2': 'font/woff2' };

/** A file of src/ as a data: address. @param {string} rel */
function dataUrl(rel) {
  const type = MIME[extname(rel).toLowerCase()];
  if (!type) throw new Error(`build: no type for ${rel}`);
  if (rel.includes('..') || /^[a-z]+:|^\//i.test(rel)) throw new Error(`build: ${rel} is not a file of src/`);
  return `data:${type};base64,${readFileSync(join(src, rel)).toString('base64')}`;
}

/**
 * The page of the app, taken apart: the markup of its body (without the script tag, without comments, every `src` a
 * data: address) and its stylesheets in their order.
 * @param {string} html the text of src/index.html
 */
export function pageParts(html) {
  const body = /<body[^>]*>([\s\S]*)<\/body>/i.exec(html);
  if (!body) throw new Error('build: src/index.html has no body');
  const sheets = [...html.matchAll(/<link\b[^>]*\brel="stylesheet"[^>]*\bhref="([^"]+)"/g)].map((m) => m[1]);
  if (sheets.length === 0) throw new Error('build: src/index.html links no stylesheet');
  const scripts = [...body[1].matchAll(/<script\b[^>]*>/g)];
  if (scripts.length !== 1 || !/\bsrc="main\.js"/.test(scripts[0][0])) {
    throw new Error('build: expected the one script of the page, main.js');
  }
  const markup = body[1]
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<script\b[^>]*>\s*<\/script>/g, '')
    .replace(/\bsrc="([^"]+)"/g, (_, rel) => `src="${dataUrl(rel)}"`)
    .replace(/\n\s*\n/g, '\n');
  if (/\b(?:href|srcset|poster|action)\s*=/.test(markup)) throw new Error('build: the body of the page loads a file by an attribute this build does not inline');
  return { markup, sheets };
}

/** The stylesheets as one text, every `url(...)` a data: address. @param {string[]} files paths of the sheets */
export function styles(files) {
  return files
    .map((file) => readFileSync(file, 'utf8'))
    .join('\n')
    .replace(/url\(\s*["']?([^"')]+)["']?\s*\)/g, (whole, rel) => (rel.startsWith('data:') ? whole : `url("${dataUrl(rel)}")`));
}

function commit() {
  try {
    return execFileSync('git', ['-C', here, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

/** The version of the app, from the manifest of the window. */
function version() {
  const conf = JSON.parse(readFileSync(join(repo, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  return String(conf.version ?? '0.0.0');
}

/** Bundle one entry into one script. @param {string} entry @param {object} options */
async function script(entry, options) {
  const out = await esbuild({
    entryPoints: [entry],
    bundle: true,
    write: false,
    minify: true,
    legalComments: 'none',
    target: 'es2022',
    logLevel: 'warning',
    ...options,
  });
  if (out.outputFiles.length !== 1) throw new Error(`build: ${entry} gave ${out.outputFiles.length} files`);
  return out.outputFiles[0].text;
}

export async function buildPage({ build = process.env.WEB_BUILD ?? commit() } = {}) {
  const lock = readLock();
  const page = pageParts(readFileSync(join(src, 'index.html'), 'utf8'));
  const css = styles([...page.sheets.map((s) => join(src, s)), join(here, 'src', 'web.css')]);
  const info = {
    version: version(),
    build,
    engine: { commit: lock.commit, lib_commit: lock.lib_commit },
    contract: { commit: lock.commit, schema: lock.schema, routes: lock.routes },
  };
  const js = await script(join(here, 'src', 'main.web.js'), {
    format: 'esm',
    platform: 'browser',
    define: {
      __WEB_BODY__: JSON.stringify(page.markup),
      __WEB_MENU__: JSON.stringify(menuModel()),
      __WEB_INFO__: JSON.stringify(info),
    },
  });
  // A classic script (no `import`): a module worker from a blob does not start on a page opened from disk.
  const worker = await script(join(here, 'src', 'worker-entry.js'), {
    format: 'iife',
    platform: 'browser',
    // wasmoon's loader names Node's modules on the paths it takes under Node only.
    external: ['module', 'url', 'fs', 'path', 'child_process', 'crypto', 'ws'],
  });
  const parts = {
    worker: Buffer.from(worker, 'utf8'),
    wasm: readFileSync(join(here, 'node_modules', 'wasmoon', 'dist', 'glue.wasm')),
    bundle: Buffer.from(bundleJson(bundle({ lock })), 'utf8'),
    host: readFileSync(join(here, 'src', 'lua', 'host.lua')),
  };
  const html = assemble({
    css,
    js,
    partIds: PART_IDS,
    parts,
    csp: CSP,
    title: 'docmap',
    comment: `docmap-desktop web variant ${info.version}, build ${build}, engine docmap.nvim ${lock.commit.slice(0, 12)}`,
  });
  const problems = verifyShell(html, { partIds: PART_IDS });
  if (problems.length) throw new Error('build: ' + problems.join('; '));
  return { html, sizes: { script: js.length, style: css.length, ...Object.fromEntries(Object.entries(parts).map(([k, v]) => [k, v.length])) } };
}

async function main() {
  const given = process.argv.find((a) => a.startsWith('--out='));
  const out = resolve(given ? given.slice('--out='.length) : join(here, 'dist'));
  const { html, sizes } = await buildPage();
  const sha = writePage(out, 'docmap.html', html);
  console.log(`${sha}  ${join(out, 'docmap.html')} (${html.length} bytes: ${JSON.stringify(sizes)})`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
