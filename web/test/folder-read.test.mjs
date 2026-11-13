// Reading a picked folder: what the engine walks and nothing else, names that are not trusted, limits that hold.

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { FolderTooLarge, VENDOR_DIRS, folderSignature, inspectFolder, readFolder, safeName } from '../src/folder-read.js';
import { engineBundle } from './helpers.mjs';
import { fakeFolder } from './fake-folder.mjs';

const text = (bytes) => new TextDecoder().decode(bytes);

test('a folder is read with its files, their bytes and their times', async () => {
  const folder = fakeFolder('p', { 'README.md': '# p', 'lua/p/init.lua': 'return {}', 'docs/a/b.md': 'Grüße' });
  const read = await readFolder(folder.handle);
  assert.deepEqual(Object.keys(read.files).sort(), ['README.md', 'docs/a/b.md', 'lua/p/init.lua']);
  assert.equal(text(read.files['docs/a/b.md']), 'Grüße');
  assert.equal(read.bytes, 3 + 9 + 7);
  assert.ok(read.mtimes['README.md'] > 0);
  assert.ok(read.mtimes['lua/p/init.lua'] > read.mtimes['README.md'], 'each file has its own time');
  assert.deepEqual(read.skipped, []);
});

test('the folders the engine never walks are never read', async () => {
  const files = { 'lua/x.lua': 'x' };
  for (const dir of VENDOR_DIRS) files[`${dir}/big.bin`] = 'y';
  files['src/node_modules/deep.js'] = 'z';
  const folder = fakeFolder('p', files);
  const read = await readFolder(folder.handle);
  assert.deepEqual(Object.keys(read.files), ['lua/x.lua']);
  assert.equal(folder.state.reads, 1, 'no file of a skipped folder was opened');
});

test('the list of skipped folders is the engine\'s own (core/scan.lua, VENDOR_DIRS)', () => {
  const scan = engineBundle().modules['docmap.core.scan'];
  const table = /M\.VENDOR_DIRS\s*=\s*\{([\s\S]*?)\n\}/.exec(scan);
  assert.ok(table, 'core/scan.lua defines VENDOR_DIRS');
  const names = [...table[1].matchAll(/\["([^"]+)"\]\s*=\s*true/g)].map((m) => m[1]).sort();
  assert.deepEqual([...VENDOR_DIRS].sort(), names);
});

test('a name that could leave the folder is never made a path', async () => {
  for (const bad of ['', '.', '..', 'a/b', 'a\\b', 'a\0b', 'trailing.', 'trailing ']) assert.equal(safeName(bad), false, JSON.stringify(bad));
  for (const good of ['a', '.hidden', 'a b', 'ä.lua', 'a..b']) assert.equal(safeName(good), true, good);
  const folder = fakeFolder('p', { 'ok.lua': 'x' });
  folder.raw('x', { kind: 'file', name: '../escape.lua', bytes: new Uint8Array([1]), time: 1 });
  folder.raw('x', { kind: 'directory', name: '..', children: new Map([['inner.lua', { kind: 'file', name: 'inner.lua', bytes: new Uint8Array([1]), time: 1 }]]) });
  const read = await readFolder(folder.handle);
  assert.deepEqual(Object.keys(read.files), ['ok.lua']);
  assert.deepEqual(read.skipped.map((s) => s.reason), ['name', 'name']);
  assert.ok(read.skipped.every((s) => !s.path.includes('/..')), 'the reported path cannot be read as a path either');
});

test('a file over the limit is left out and reported; an unreadable one too', async () => {
  const folder = fakeFolder('p', { 'small.lua': 'x', 'big.bin': 'y'.repeat(50) });
  folder.raw('locked.lua', { kind: 'file', name: 'locked.lua', bytes: new Uint8Array([1]), time: 1, unreadable: true });
  const read = await readFolder(folder.handle, { fileBytes: 10 });
  assert.deepEqual(Object.keys(read.files), ['small.lua']);
  assert.deepEqual(read.skipped, [
    { path: 'big.bin', reason: 'too-large' },
    { path: 'locked.lua', reason: 'unreadable' },
  ]);
});

test('a folder over the budget is refused as a whole', async () => {
  const many = fakeFolder('p', { 'a.lua': '1', 'b.lua': '2', 'c.lua': '3' });
  await assert.rejects(readFolder(many.handle, { files: 2 }), (err) => err instanceof FolderTooLarge && err.what === 'files' && err.limit === 2);
  const heavy = fakeFolder('p', { 'a.lua': 'x'.repeat(6), 'b.lua': 'x'.repeat(6) });
  await assert.rejects(readFolder(heavy.handle, { bytes: 10 }), (err) => err instanceof FolderTooLarge && err.what === 'bytes');
  // The size that counts is the one that was read, not the one a file claimed.
  const liar = fakeFolder('p', {});
  liar.raw('grown.lua', { kind: 'file', name: 'grown.lua', bytes: new Uint8Array(20), size: 1, time: 1 });
  await assert.rejects(readFolder(liar.handle, { bytes: 10 }), FolderTooLarge);
});

test('folders nested deeper than the limit are reported, not followed', async () => {
  const folder = fakeFolder('p', { 'a/b/c/deep.lua': 'x', 'a/top.lua': 'y' });
  const read = await readFolder(folder.handle, { depth: 2 });
  assert.deepEqual(Object.keys(read.files), ['a/top.lua']);
  assert.deepEqual(read.skipped, [{ path: 'a/b/c', reason: 'too-deep' }]);
});

test('the signature changes with a file and not with a folder the engine does not walk', async () => {
  const folder = fakeFolder('p', { 'lua/x.lua': 'x', 'node_modules/m.js': 'm' });
  const before = await folderSignature(folder.handle);
  assert.equal(await folderSignature(folder.handle), before, 'the same folder reads the same');
  folder.write('node_modules/m.js', 'changed');
  assert.equal(await folderSignature(folder.handle), before);
  folder.write('lua/x.lua', 'changed');
  const changed = await folderSignature(folder.handle);
  assert.notEqual(changed, before);
  folder.write('lua/new.lua', 'n');
  assert.notEqual(await folderSignature(folder.handle), changed);
});

test('a picked folder says whether it is a checkout, and lists the folders inside it', async () => {
  const repo = fakeFolder('repo', { '.git/HEAD': 'ref', 'lua/x.lua': 'x' });
  assert.equal((await inspectFolder(repo.handle)).isGit, true);
  const worktree = fakeFolder('wt', { '.git': 'gitdir: elsewhere', 'x.lua': 'x' });
  assert.equal((await inspectFolder(worktree.handle)).isGit, true, 'a worktree has a file named .git');
  const parent = fakeFolder('repos', { 'b.nvim/.git/HEAD': 'r', 'A.nvim/.git/HEAD': 'r', 'notes/x.md': 'n', '.hidden/y': 'y', 'file.txt': 't' });
  const scan = await inspectFolder(parent.handle);
  assert.equal(scan.isGit, false);
  assert.deepEqual(scan.subfolders.map((s) => [s.name, s.isGit]), [['A.nvim', true], ['b.nvim', true], ['notes', false]]);
});
