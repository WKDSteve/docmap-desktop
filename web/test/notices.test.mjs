// The notices of what the page bundles: current, and fit for a comment of the page.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { NOTICES_FILE, noticesMarkdown, noticesText, readLock, readOfl, resolved, versionIn } from '../notices.mjs';

test('the file next to the page is what the lockfile and the font license give', () => {
  assert.equal(readFileSync(NOTICES_FILE, 'utf8').replaceAll('\r\n', '\n'), noticesMarkdown(readLock(), readOfl()));
});

test('a version is the one of the lockfile, and a package that is not there stops the build', () => {
  const lock = readLock();
  assert.match(versionIn(lock, 'wasmoon'), /^\d+\.\d+\.\d+$/);
  assert.equal(resolved(lock).find((c) => c.pkg === 'wasmoon').version, versionIn(lock, 'wasmoon'));
  assert.throws(() => versionIn(lock, 'no-such-package'), /0 times in the lockfile/);
});

test('the text for the page opens no markup, ends no comment and names no address', () => {
  const text = noticesText(readLock());
  assert.equal(/[<>&]|--/.test(text), false);
  assert.equal(/\b(?:https?|wss?|ftp):\/\//.test(text), false);
  for (const name of ['Lua', 'wasmoon', 'Emscripten', 'JetBrains Mono', 'SIL Open Font License']) assert.ok(text.includes(name), name);
  assert.match(text, /Permission is hereby granted/);
});
