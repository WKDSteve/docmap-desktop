// The single file: what goes into it, and that its shell loads nothing.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { policyProblems, verifyShell } from 'appkit-web/pack';
import { CSP, PART_IDS, buildPage, pageParts, styles } from '../build.mjs';
import { PART_IDS as LOADED } from '../src/embedded.js';
import { HIDDEN } from '../src/hidden.js';
import { web } from './helpers.mjs';

const INDEX = readFileSync(join(web, '..', 'src', 'index.html'), 'utf8');

test('the policy lets the page reach nothing, and allows exactly what the page is made of', () => {
  assert.deepEqual(policyProblems(CSP), []);
  const directives = Object.fromEntries(CSP.split('; ').map((d) => [d.split(' ')[0], d.split(' ').slice(1).join(' ')]));
  assert.equal(directives['connect-src'], "'none'");
  assert.equal(directives['default-src'], "'none'");
  // The map is a page of its own in a frame, made from a blob; the engine is a worker from a blob.
  assert.equal(directives['frame-src'], 'blob:');
  assert.equal(directives['worker-src'], 'blob:');
  assert.equal(directives['font-src'], 'data:');
});

test('the markup of the window goes in without its script, its comments and anything it would load', () => {
  const { markup, sheets } = pageParts(INDEX);
  assert.deepEqual(sheets, ['appkit-tokens.css', 'palette.css', 'appkit-tokens-fallback.css', 'style.css']);
  assert.equal(/<script/i.test(markup), false);
  assert.equal(/<!--/.test(markup), false);
  for (const m of markup.matchAll(/\bsrc="([^"]*)"/g)) assert.ok(m[1].startsWith('data:image/svg+xml;base64,'), m[1].slice(0, 40));
  for (const id of ['sidebar', 'view', 'map', 'placeholder', 'status', 'add', 'pick-folder', 'projects']) {
    assert.ok(markup.includes(`id="${id}"`), `the element #${id} of the window is in the markup`);
  }
});

test('a page this build cannot make self-contained is refused', () => {
  const body = (inner) => `<html><head><link rel="stylesheet" href="style.css" /></head><body>${inner}</body></html>`;
  assert.throws(() => pageParts('<html><head></head></html>'), /no body/);
  assert.throws(() => pageParts('<html><head></head><body><script type="module" src="main.js"></script></body></html>'), /no stylesheet/);
  assert.throws(() => pageParts(body('<p>x</p>')), /the one script/);
  assert.throws(() => pageParts(body('<script src="other.js"></script>')), /the one script/);
  assert.throws(() => pageParts(body('<script type="module" src="main.js"></script><a href="x.html">x</a>')), /does not inline/);
  assert.throws(() => pageParts(body('<script type="module" src="main.js"></script><img src="../outside.svg">')), /not a file of src/);
  assert.throws(() => pageParts(body('<script type="module" src="main.js"></script><img src="photo.gif">')), /no type/);
});

test('the styles carry their fonts as data', () => {
  const css = styles(['appkit-tokens.css', 'style.css'].map((f) => join(web, '..', 'src', f)));
  const urls = [...css.matchAll(/url\(\s*["']?([^"')]{0,30})/g)].map((m) => m[1]);
  assert.ok(urls.length >= 2, 'the two fonts of the window');
  for (const url of urls) assert.ok(url.startsWith('data:font/ttf;base64,'), url);
});

test('what the shell hides exists in the page of the app', () => {
  const { markup } = pageParts(INDEX);
  const present = (selector) => {
    const id = /^#([\w-]+)$/.exec(selector);
    if (id) return markup.includes(`id="${id[1]}"`);
    const attr = /^\[([\w-]+)="([^"]+)"\]$/.exec(selector);
    return attr ? markup.includes(`${attr[1]}="${attr[2]}"`) : false;
  };
  for (const selector of [...HIDDEN.elements, ...HIDDEN.sectionsOf]) assert.ok(present(selector), `${selector} is not in src/index.html`);
  // A section is hidden through a control inside it: each of those lies in a section of the settings.
  for (const selector of HIDDEN.sectionsOf) {
    const at = markup.indexOf(`id="${selector.slice(1)}"`);
    const open = markup.lastIndexOf('<section class="prefs-section"', at);
    const close = markup.lastIndexOf('</section>', at);
    assert.ok(open > close, `${selector} is not inside a section of the settings`);
  }
});

test('the built page is one file whose shell loads nothing, with the parts the page unpacks', async () => {
  const { html, sizes } = await buildPage({ build: 'test' });
  assert.deepEqual(verifyShell(html, { partIds: PART_IDS }), []);
  assert.deepEqual([...LOADED], PART_IDS, 'the page unpacks the parts the build packs');
  assert.match(html, /<title>docmap<\/title>/);
  assert.match(html, /docmap-desktop web variant \d+\.\d+\.\d+, build test, engine docmap\.nvim [0-9a-f]{12}/);
  assert.ok(html.includes(`<meta http-equiv="Content-Security-Policy" content="${CSP}">`));
  assert.ok(sizes.bundle > 500_000 && sizes.wasm > 100_000 && sizes.worker > 50_000);
  // A page for a `git pull` on a machine that may install nothing: it stays small enough to read from disk at once.
  assert.ok(html.length < 4 * 1024 * 1024, `the page weighs ${html.length} bytes`);
  // The same sources give the same file: what is published can be built again and compared.
  assert.equal((await buildPage({ build: 'test' })).html, html);
});
