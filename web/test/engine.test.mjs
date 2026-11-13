// The engine of the pinned commits in the VM: what it offers is what engine.lock says, and it maps a folder.

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readLock } from '../engine-bundle.mjs';
import { parseAnswer } from '../src/engine.js';
import { engineBundle, fixtureFiles, newEngine } from './helpers.mjs';

const ROOT = '/w/p1/sample';
const mounted = () => Object.fromEntries(Object.entries(fixtureFiles()).map(([rel, bytes]) => [`${ROOT}/${rel}`, bytes]));
const text = (engine, path) => new TextDecoder().decode(engine.fs.readFile(path));

test('the engine answers --capabilities with the schema and the routes of engine.lock', async () => {
  const engine = await newEngine();
  const lock = readLock();
  const answer = engine.capabilities();
  assert.equal(answer.status, 'ok');
  const caps = JSON.parse(answer.payload);
  assert.equal(caps.schema, lock.schema);
  assert.deepEqual(caps.routes, lock.routes);
  assert.deepEqual(caps.capabilities, ['api', 'languages', 'schema']);
  assert.ok(caps.languages.some((l) => l.name === 'lua'), 'the language backends are in the bundle');
  // No parser in the page: no grammar is loaded, which is what the window shows as "no function-level facts".
  assert.deepEqual(caps.languages.filter((l) => l.grammar_loaded === true), []);
  engine.close();
});

test('the bundle is the pinned commits and holds the pipeline whole', () => {
  const lock = readLock();
  const made = engineBundle();
  assert.deepEqual(made.engine, { commit: lock.commit, lib_commit: lock.lib_commit });
  for (const name of ['standalone.vim_shim', 'docmap.core.cli', 'docmap.core.api', 'docmap.core.lang.lua', 'docmap.config', 'lib.nvim.fs.read']) {
    assert.ok(name in made.modules, `${name} is in the bundle`);
  }
  // The C parser's binding and the Windows link reader are never loaded here.
  assert.equal('standalone.treesitter' in made.modules, false);
  assert.equal('standalone.win_links' in made.modules, false);
  // Comments are cut, the lines stay: an error names the line of the file.
  assert.equal(/^\s*---/m.test(made.modules['docmap.core.scan']), false);
});

test('generate writes the map of a folder into the VM, and nothing outside the project', async () => {
  const engine = await newEngine();
  engine.mount(mounted());
  const run = engine.generate({ root: ROOT, argv: ['--lenient'] });
  assert.equal(run.status, 'ok', run.payload);
  assert.equal(run.code, 0);
  const out = run.log.filter((l) => l.stream === 'out').map((l) => l.text).join('');
  assert.match(out, /wrote docs\/map\/index\.html/);
  assert.deepEqual(engine.fs.readdir(`${ROOT}/docs/map`).sort(), ['.', '..', 'index.html', 'module_map.json', 'overview.md']);
  const map = JSON.parse(text(engine, `${ROOT}/docs/map/module_map.json`));
  assert.equal(map.meta.schema, readLock().schema);
  assert.equal(map.meta.title, 'sample', 'the title is the name of the folder');
  const modules = map.nodes.map((n) => n.module).filter(Boolean).sort();
  assert.deepEqual(modules, ['sample', 'sample.util']);
  const entry = map.nodes.find((n) => n.module === 'sample');
  assert.match(entry.summary, /entry of the sample project/);
  assert.ok(text(engine, `${ROOT}/docs/map/index.html`).startsWith('<!DOCTYPE html>') || text(engine, `${ROOT}/docs/map/index.html`).startsWith('<!doctype html>'));
  // UTF-8 goes through the VM unchanged.
  assert.match(text(engine, `${ROOT}/docs/map/module_map.json`), /Grüße/);
  assert.deepEqual(engine.fs.readdir('/w').sort(), ['.', '..', 'p1']);
  engine.close();
});

test('the same folder gives the same bytes, in one VM and in another', async () => {
  const outputs = [];
  for (let i = 0; i < 2; i++) {
    const engine = await newEngine();
    engine.mount(mounted());
    assert.equal(engine.generate({ root: ROOT, argv: ['--lenient'] }).code, 0);
    const first = text(engine, `${ROOT}/docs/map/index.html`) + text(engine, `${ROOT}/docs/map/module_map.json`);
    // A second run over the map it just wrote: the map's own folder is not a source.
    assert.equal(engine.generate({ root: ROOT, argv: ['--lenient'] }).code, 0);
    assert.equal(text(engine, `${ROOT}/docs/map/index.html`) + text(engine, `${ROOT}/docs/map/module_map.json`), first);
    outputs.push(first);
    engine.close();
  }
  assert.equal(outputs[0], outputs[1]);
});

test('--check says whether the map in the folder is the one the sources give', async () => {
  const engine = await newEngine();
  engine.mount(mounted());
  assert.notEqual(engine.generate({ root: ROOT, argv: ['--check', '--lenient'] }).code, 0, 'no map yet: not current');
  assert.equal(engine.generate({ root: ROOT, argv: ['--lenient'] }).code, 0);
  assert.equal(engine.generate({ root: ROOT, argv: ['--check', '--lenient'] }).code, 0);
  engine.mount({ [`${ROOT}/lua/sample/extra.lua`]: "---@module 'sample.extra'\n---@brief One more.\nreturn {}\n" });
  const stale = engine.generate({ root: ROOT, argv: ['--check', '--lenient'] });
  assert.notEqual(stale.code, 0);
  assert.match(stale.log.map((l) => l.text).join(''), /stale/i);
  engine.close();
});

test('every route of the lock answers; what needs git or a Neovim session says it is not available', async () => {
  const engine = await newEngine();
  engine.mount(mounted());
  assert.equal(engine.generate({ root: ROOT, argv: ['--lenient'] }).code, 0);
  for (const route of readLock().routes) {
    const answer = engine.api({ root: ROOT, route });
    assert.equal(answer.status, 'ok', `${route}: ${answer.payload} ${JSON.stringify(answer.log)}`);
    const doc = JSON.parse(answer.payload);
    assert.equal(typeof doc, 'object');
    if ('available' in doc) assert.equal(doc.available, false, `${route} has nothing to read in a browser`);
  }
  const unknown = engine.api({ root: ROOT, route: 'no-such-route' });
  assert.equal(unknown.status, 'exit');
  assert.match(unknown.log.map((l) => l.text).join(''), /no such route/);
  engine.close();
});

test('a folder the engine cannot map ends with its message, not with a broken VM', async () => {
  const engine = await newEngine();
  const none = engine.generate({ root: '/w/p9/missing', argv: ['--lenient'] });
  assert.notEqual(none.code, 0);
  // The VM is still good for the next project.
  engine.mount(mounted());
  assert.equal(engine.generate({ root: ROOT, argv: ['--lenient'] }).code, 0);
  engine.close();
});

test('an answer is told apart by bytes, not by characters', () => {
  const payload = '{"a":"Grüße"}';
  const bytes = new TextEncoder().encode(payload).length;
  const answer = parseAnswer(`ok ${bytes}\n${payload}owrote x\n\0ewarn ü\n`);
  assert.equal(answer.payload, payload);
  assert.deepEqual(answer.log, [
    { stream: 'out', text: 'wrote x\n' },
    { stream: 'err', text: 'warn ü\n' },
  ]);
  assert.throws(() => parseAnswer('nonsense'), /without a head/);
});
