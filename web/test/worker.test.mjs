// The worker with the real engine, in this process: what the page asks about a project's files, and what it refuses.

import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { projectRoot } from '../src/worker.js';
import { fixtureFiles, startedClient } from './helpers.mjs';

const ROOT = '/w/p1/sample';
const MAP = `${ROOT}/docs/map`;
const T0 = 1_700_000_000_000;
let client;

const mtimes = (files, at) => Object.fromEntries(Object.keys(files).map((k) => [k, at]));
const mount = (files = fixtureFiles(), extra = {}) =>
  client.ask('mount', { root: ROOT, files, mtimes: mtimes(files, T0), ...extra });

before(async () => {
  client = await startedClient();
});
after(() => client.close());

test('a root is a place under the projects folder and nothing else', () => {
  for (const ok of ['/w/p1/sample', '/w/p12/a b', '/w/x_1/ä.nvim']) assert.equal(projectRoot(ok), true, ok);
  for (const bad of ['/', '/w', '/w/p1', '/w/p1/', '/w/../etc/x', '/w/p1/..', '/w/p 1/x', '/x/p1/sample', '/w/p1/a/b', 'w/p1/a', '/w/p1/a\\b', null, 7]) {
    assert.equal(projectRoot(bad), false, String(bad));
  }
});

test('requests that name no project, or a path outside one, are refused', async () => {
  await assert.rejects(client.ask('mount', { root: '/home/web', files: {}, mtimes: {} }), /not the root of a project/);
  await assert.rejects(client.ask('generate', { request: { root: '/' } }), /not the root of a project/);
  await assert.rejects(client.ask('tree', { root: '/w/p1/..', sub: '' }), /not the root of a project/);
  await assert.rejects(mount({ '../escape.lua': new Uint8Array([1]) }), /not a plain path/);
  await assert.rejects(mount({ 'a//b.lua': new Uint8Array([1]) }), /not a plain path/);
  await mount();
  await assert.rejects(client.ask('read', { root: ROOT, path: '/w/p2/other/secret' }), /not a path inside the project/);
  await assert.rejects(client.ask('read', { root: ROOT, path: `${ROOT}/../../p2/x` }), /not a path inside the project/);
  await assert.rejects(client.ask('tree', { root: ROOT, sub: '../..' }), /not a path inside the project/);
  await assert.rejects(client.ask('nonsense', { root: ROOT }), /unknown request/);
});

test('a second init is refused: one engine per worker', async () => {
  await assert.rejects(client.init({ bundle: {}, host: '' }), /already started/);
});

test('mounting replaces the project, and the map generated before goes back on top', async () => {
  await mount({ 'old.lua': new Uint8Array([1]) });
  await mount();
  assert.equal(await client.ask('read', { root: ROOT, path: `${ROOT}/old.lua` }), null);
  const run = await client.ask('generate', { request: { root: ROOT, argv: ['--lenient'] } });
  assert.equal(run.code, 0, JSON.stringify(run));
  const made = await client.ask('readTree', { root: ROOT, dir: MAP });
  assert.deepEqual(Object.keys(made).sort(), ['index.html', 'module_map.json', 'overview.md']);
  const overlay = Object.fromEntries(Object.entries(made).map(([k, v]) => [`docs/map/${k}`, v]));
  await mount(fixtureFiles(), { overlay, overlayAt: T0 + 60_000 });
  const page = await client.ask('read', { root: ROOT, path: `${MAP}/index.html` });
  assert.ok(page.bytes.length > 1000);
  assert.equal(page.mtime, T0 + 60_000);
});

test('the map is fresh until a source is newer, and the changed files are listed newest first', async () => {
  const files = fixtureFiles();
  const made = await client.ask('readTree', { root: ROOT, dir: MAP });
  const overlay = Object.fromEntries(Object.entries(made).map(([k, v]) => [`docs/map/${k}`, v]));
  await client.ask('mount', { root: ROOT, files, mtimes: mtimes(files, T0), overlay, overlayAt: T0 + 60_000 });
  const fresh = await client.ask('freshness', { root: ROOT, mapDir: MAP, now: T0 + 120_000 });
  assert.deepEqual(fresh, { hasMap: true, stale: false, newest: fresh.newest, behindSecs: null, truncated: false, generatedSecs: 60 });
  assert.deepEqual(await client.ask('changes', { root: ROOT, mapDir: MAP, limit: 10 }), { files: [], total: 0, truncated: false });

  const times = { ...mtimes(files, T0), 'lua/sample/util.lua': T0 + 90_000, 'README.md': T0 + 200_000 };
  await client.ask('mount', { root: ROOT, files: fixtureFiles(), mtimes: times, overlay, overlayAt: T0 + 60_000 });
  const stale = await client.ask('freshness', { root: ROOT, mapDir: MAP, now: T0 + 300_000 });
  assert.equal(stale.stale, true);
  assert.equal(stale.newest, 'README.md');
  assert.equal(stale.behindSecs, 140);
  const changes = await client.ask('changes', { root: ROOT, mapDir: MAP, limit: 1 });
  assert.deepEqual(changes, { files: [{ path: 'README.md', afterSecs: 140 }], total: 2, truncated: false });
});

test('a project without a map has none, and says so', async () => {
  await mount();
  const none = await client.ask('freshness', { root: ROOT, mapDir: MAP, now: T0 });
  assert.equal(none.hasMap, false);
  assert.equal(none.stale, false);
});

test('the tree lists one level, folders first', async () => {
  await mount();
  const top = await client.ask('tree', { root: ROOT, sub: '' });
  assert.equal(top.path, '');
  assert.deepEqual(top.entries.map((e) => [e.name, e.isDir]), [['docs', true], ['lua', true], ['TESTS', true], ['web', true], ['README.md', false]]);
  const readme = top.entries.at(-1);
  assert.equal(readme.path, 'README.md');
  assert.ok(readme.size > 0);
  assert.equal(readme.modified, T0 / 1000);
  const inner = await client.ask('tree', { root: ROOT, sub: 'lua/sample' });
  assert.deepEqual(inner.entries.map((e) => e.path), ['lua/sample/init.lua', 'lua/sample/util.lua']);
  await assert.rejects(client.ask('tree', { root: ROOT, sub: 'no/such' }));
});

test('the languages are counted by file, most files first', async () => {
  await mount();
  const scan = await client.ask('languages', { root: ROOT, mapDir: MAP });
  assert.deepEqual(scan, {
    languages: [
      { name: 'Lua', files: 3, grammar: 'lua', backend: null },
      { name: 'JavaScript', files: 1, grammar: 'javascript', backend: null },
    ],
    total: 4,
    truncated: false,
  });
});

test('a search finds text line by line, or files by their path', async () => {
  await mount();
  const ask = (ask) => client.ask('search', { root: ROOT, mapDir: MAP, ask });
  const text = await ask({ query: 'TRIM', mode: 'text', caseSensitive: false });
  assert.deepEqual(text.hits.map((h) => [h.path, h.line]), [['lua/sample/init.lua', 16], ['lua/sample/util.lua', 9]]);
  assert.equal(text.hits[1].text.slice(text.hits[1].at, text.hits[1].at + 4), 'trim');
  assert.equal(text.truncated, false);
  assert.deepEqual((await ask({ query: 'TRIM', mode: 'text', caseSensitive: true })).hits, []);
  const files = await ask({ query: 'util', mode: 'files', caseSensitive: false });
  assert.deepEqual(files.hits, [{ path: 'lua/sample/util.lua', line: null, text: null, at: null }]);
  const sub = await ask({ query: 'sample', mode: 'text', caseSensitive: false, sub: 'docs' });
  assert.ok(sub.hits.length > 0 && sub.hits.every((h) => h.path.startsWith('docs/')));
  assert.deepEqual((await ask({ query: '', mode: 'text' })).hits, []);
});
