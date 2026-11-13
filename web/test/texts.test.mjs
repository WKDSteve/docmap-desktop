// The words of the shell: both languages say the same things.

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { LOCALES, keysOf, text } from '../src/texts.js';

test('German and English have the same keys and the same placeholders', () => {
  assert.deepEqual(LOCALES, ['en', 'de']);
  assert.deepEqual(keysOf('de').sort(), keysOf('en').sort());
  const holes = (s) => [...s.matchAll(/\{([a-z]+)\}/g)].map((m) => m[1]).sort();
  for (const key of keysOf('en')) {
    assert.deepEqual(holes(text('de', key)), holes(text('en', key)), key);
    assert.notEqual(text('de', key), text('en', key), `${key} is not translated`);
  }
});

test('a text is filled, falls back to English, and never comes out blank', () => {
  assert.equal(
    text('en', 'access.many', { n: 3 }),
    '3 folders wait for your permission: the browser asks again after it was restarted.',
  );
  assert.equal(text('fr', 'access.allow'), 'Allow access');
  assert.equal(
    text('en', 'skipped.many', { n: 2 }),
    '2 files of {name} were left out (too large, unreadable or an unusable name).',
  );
});
