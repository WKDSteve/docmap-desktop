// The third-party notices of the web page. MIT asks for the notice in all copies, and docmap.html is handed on as one
// file, so the page carries it as a comment; the same text, and the license of the font in full, goes next to it as a
// file. The version of a package comes from the lockfile, so it cannot go stale. As in tasks-desktop's ui/web/notices.mjs.
//
//   node notices.mjs           (from web/) write THIRD-PARTY-NOTICES.md
//   node notices.mjs --check   fail when that file is not what the sources give (a test does the same)

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
export const NOTICES_FILE = join(here, 'THIRD-PARTY-NOTICES.md');

/**
 * What the page bundles. `pkg` is the name in the lockfile, which gives the version; a part that is no package of its
 * own says its version in `version`.
 */
export const COMPONENTS = [
  { name: 'Lua (compiled to WebAssembly by wasmoon)', version: '5.4', license: 'MIT', copyright: 'Copyright (C) 1994-2024 Lua.org, PUC-Rio.' },
  { name: 'wasmoon', pkg: 'wasmoon', license: 'MIT', copyright: 'Copyright (c) 2023 Gabriel Francisco.' },
  { name: 'Emscripten runtime (JavaScript glue inside wasmoon)', version: 'as built by wasmoon', license: 'MIT', copyright: 'Copyright (c) 2010-2014 Emscripten authors, see the AUTHORS file of Emscripten.' },
  { name: 'JetBrains Mono (the font of the window, two cuts)', version: 'as in src/fonts', license: 'SIL Open Font License 1.1', copyright: 'Copyright 2020 The JetBrains Mono Project Authors.' },
];

const MIT = `Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`;

const FIRST_PARTY =
  'First-party code in the page, all MIT, Copyright (c) 2026 Stefan Bartl: docmap-desktop (the window, the web shell), appkit (tokens, the packer), and the Lua engine docmap.nvim with lib.nvim.';

/** The version a package has in `pnpm-lock.yaml`. A package that is not there, or there twice, stops the build. */
export function versionIn(lock, pkg) {
  const packages = lock.slice(lock.indexOf('\npackages:'), lock.indexOf('\nsnapshots:'));
  const escaped = pkg.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const found = [...packages.matchAll(new RegExp(`^  '?${escaped}@(\\d[^:'(]*)'?:`, 'gm'))].map((m) => m[1]);
  if (found.length !== 1) throw new Error(`notices: ${pkg} is ${found.length} times in the lockfile, expected once`);
  return found[0];
}

/** The components with their versions. */
export function resolved(lock) {
  return COMPONENTS.map((c) => ({ ...c, version: c.pkg ? versionIn(lock, c.pkg) : c.version }));
}

/** The notices as plain text, without markup, without an address and without `--`: what goes into a comment of the page. */
export function noticesText(lock) {
  return [
    'Third-party notices. This page bundles:',
    ...resolved(lock).map((c) => `  ${c.name} ${c.version}, ${c.license}. ${c.copyright}`),
    '',
    FIRST_PARTY,
    '',
    'The text of the SIL Open Font License 1.1 is in THIRD-PARTY-NOTICES.md next to this page.',
    '',
    'MIT License (Lua, wasmoon, the Emscripten runtime)',
    '',
    MIT,
  ].join('\n');
}

/** The notices as the file next to the page. `ofl` is the license of the font as it lies in src/fonts. */
export function noticesMarkdown(lock, ofl) {
  return `${[
    '# Third-party notices',
    '',
    '`docmap.html` is built by [docmap-desktop](https://github.com/WKDSteve/docmap-desktop) and bundles the code and the',
    'font below; the page also carries the notices as a comment. The versions come from the lockfile of the build',
    '(`web/notices.mjs`).',
    '',
    '| Component | Version | License |',
    '| --- | --- | --- |',
    ...resolved(lock).map((c) => `| ${c.name} | ${c.version} | ${c.license} |`),
    '',
    FIRST_PARTY,
    '',
    '## Copyright notices',
    '',
    ...resolved(lock).map((c) => `- ${c.name}: ${c.copyright}`),
    '',
    '## MIT License (Lua, wasmoon, the Emscripten runtime)',
    '',
    MIT,
    '',
    '## SIL Open Font License 1.1 (JetBrains Mono)',
    '',
    '```',
    ofl.trimEnd(),
    '```',
  ].join('\n')}\n`;
}

export const readLock = () => readFileSync(join(here, 'pnpm-lock.yaml'), 'utf8').replaceAll('\r\n', '\n');
export const readOfl = () => readFileSync(join(here, '..', 'src', 'fonts', 'OFL.txt'), 'utf8').replaceAll('\r\n', '\n');

function main() {
  const text = noticesMarkdown(readLock(), readOfl());
  if (process.argv.includes('--check')) {
    if (readFileSync(NOTICES_FILE, 'utf8').replaceAll('\r\n', '\n') !== text) {
      console.error('THIRD-PARTY-NOTICES.md is not what the sources give: run "node notices.mjs"');
      process.exit(1);
    }
    return;
  }
  writeFileSync(NOTICES_FILE, text);
  console.log(NOTICES_FILE);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
