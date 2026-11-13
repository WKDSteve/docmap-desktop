// What depends on what, from the maps alone: the cases of src-tauri/src/deps.rs, so both sides resolve alike.

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { computeDeps } from '../src/deps.js';

const map = (nodes) => ({ meta: { schema: 6 }, nodes });

test('an external require resolves to the project that declares it', () => {
  const lib = map([{ module: 'lib.nvim.notify' }, { module: 'lib.nvim.map' }]);
  const app = map([{ module: 'app.init', requires_external: ['lib.nvim.notify', 'lib.nvim.notify', 'lib.nvim.map'] }]);
  const d = computeDeps([
    { id: 'lib', map: lib },
    { id: 'app', map: app },
  ]);
  assert.deepEqual(d.edges, [{ from: 'app', to: 'lib', count: 3, modules: ['lib.nvim.notify', 'lib.nvim.map'] }]);
  assert.deepEqual(d.outside, []);
  assert.deepEqual(d.unread, []);
});

test('a name no project declares is reported as outside rather than dropped', () => {
  const app = map([{ module: 'app.init', requires_external: ['telescope', 'telescope'] }]);
  const other = map([{ module: 'other.init', requires_external: ['telescope', 'plenary'] }]);
  const d = computeDeps([
    { id: 'app', map: app },
    { id: 'other', map: other },
  ]);
  assert.deepEqual(d.edges, []);
  assert.deepEqual(d.outside, [
    { name: 'telescope', count: 3, projects: ['app', 'other'] },
    { name: 'plenary', count: 1, projects: ['other'] },
  ]);
});

test('a project requiring its own module is no edge; one without a readable map is unread', () => {
  const self = map([{ module: 'a.x' }, { module: 'a.y', requires_external: ['a.x'] }]);
  const d = computeDeps([
    { id: 'a', map: self },
    { id: 'zz', map: null },
    { id: 'b', map: { nodes: 'broken' } },
  ]);
  assert.deepEqual(d.edges, []);
  assert.deepEqual(d.unread, ['b', 'zz']);
});

test('the first project to declare a module owns it; edges are ordered by weight, then by name', () => {
  const first = map([{ module: 'shared' }]);
  const second = map([{ module: 'shared' }, { module: 'second.only' }]);
  const user = map([{ module: 'u', requires_external: ['shared', 'second.only', 'second.only'] }]);
  const d = computeDeps([
    { id: 'first', map: first },
    { id: 'second', map: second },
    { id: 'user', map: user },
  ]);
  assert.deepEqual(
    d.edges.map((e) => [e.from, e.to, e.count]),
    [
      ['user', 'second', 2],
      ['user', 'first', 1],
    ],
  );
});

test('what is not a map does not break the answer', () => {
  const odd = { nodes: [null, 7, { module: 9, requires_external: ['x', 3, null] }, { requires_external: 'text' }] };
  assert.deepEqual(computeDeps([{ id: 'odd', map: odd }]), {
    edges: [],
    outside: [{ name: 'x', count: 1, projects: ['odd'] }],
    unread: [],
  });
});
