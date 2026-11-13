// The search in what the map shows: the cases of src-tauri/src/search.rs (`view`).

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { searchView } from '../src/view-search.js';

const MAP = {
  nodes: [
    {
      id: 'lua/core/favorites.lua',
      kind: 'file',
      name: 'favorites.lua',
      path: 'lua/core/favorites.lua',
      source: 'lua/core/favorites.lua',
      summary: 'Manage favorites',
      body: '',
      functions: [
        {
          name: 'M.toggle',
          line: 10,
          snippet: 'local secret = 1',
          summary: 'Flip the favorite flag',
          params: [{ name: 'cmd', desc: 'the command line' }],
        },
      ],
      children: ['not searched'],
    },
  ],
  docs: { files: [{ path: 'docs/USAGE.md', title: 'Using it' }] },
  features: {
    files: [
      { path: 'docs/FEATURES/CORE.md', entries: [{ name: 'Pinning', line: 4, tab: 'hierarchy', text: 'Pin a node to keep it' }] },
    ],
  },
};

test('it finds names, summaries and the fields of a function', () => {
  const byName = searchView(MAP, 'FAVORITES');
  assert.equal(byName.available, true);
  assert.equal(byName.hits[0].kind, 'file');
  assert.equal(byName.hits[0].node, 'lua/core/favorites.lua');
  const byParam = searchView(MAP, 'command line');
  assert.deepEqual(byParam.hits, [
    {
      kind: 'function',
      label: 'M.toggle',
      detail: 'desc: the command line',
      node: 'lua/core/favorites.lua',
      file: 'lua/core/favorites.lua',
      line: 10,
    },
  ]);
});

test('code snippets and the structure are not what the page shows', () => {
  assert.deepEqual(searchView(MAP, 'secret').hits, []);
  assert.deepEqual(searchView(MAP, 'not searched').hits, []);
});

test('documents and features are found by what they are called', () => {
  const doc = searchView(MAP, 'using');
  assert.deepEqual(doc.hits, [
    { kind: 'doc', label: 'Using it', detail: 'docs/USAGE.md', node: null, file: 'docs/USAGE.md', line: null },
  ]);
  const feature = searchView(MAP, 'pin a node');
  assert.deepEqual(feature.hits, [
    {
      kind: 'feature',
      label: 'Pinning',
      detail: 'text: Pin a node to keep it',
      node: null,
      file: 'docs/FEATURES/CORE.md',
      line: 4,
    },
  ]);
  assert.deepEqual(searchView(MAP, 'hierarchy').hits, [], 'the tab of a feature is where it is shown, not what it says');
});

test('without a map the search is unavailable, not an error; an empty query finds nothing', () => {
  assert.deepEqual(searchView(null, 'x'), { hits: [], truncated: false, available: false });
  assert.deepEqual(searchView(MAP, '   '), { hits: [], truncated: false, available: true });
});

test('the hits stop at the limit and say so; a long line is cut around the match', () => {
  const many = { nodes: Array.from({ length: 5 }, (_, i) => ({ id: `n${i}`, name: `needle ${i}` })) };
  const cut = searchView(many, 'needle', 3);
  assert.equal(cut.hits.length, 3);
  assert.equal(cut.truncated, true);
  const long = { nodes: [{ id: 'a', name: 'a', body: `${'x'.repeat(300)} needle ${'y'.repeat(300)}` }] };
  const [hit] = searchView(long, 'needle').hits;
  assert.ok(hit.detail.length < 140);
  assert.match(hit.detail, /^body: ….*needle.*…$/);
});
