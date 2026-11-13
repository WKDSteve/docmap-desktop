// The menu of the web variant is the menu of the app: read from menu.rs, and every item has what it does in main.js.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { menuIds, menuModel, parseMenu } from '../menu-model.mjs';
import { LOCALE_ID, WEB_UNAVAILABLE, isChecked, menuRows } from '../src/menubar.js';
import { web } from './helpers.mjs';

const RUST = readFileSync(join(web, '..', 'src-tauri', 'src', 'menu.rs'), 'utf8');
const MAIN = readFileSync(join(web, '..', 'src', 'main.js'), 'utf8');
const model = menuModel();

test('the model holds every id menu.rs names, in its order', () => {
  // Every string of the table that looks like an id, read without the model's own reader.
  const table = RUST.slice(RUST.indexOf('const GROUPS'), RUST.indexOf('fn is_checked'));
  const named = [...table.matchAll(/"(menu\.[a-z_.]+)"/g)].map((m) => m[1]);
  assert.ok(named.length >= 35, `expected the whole menu, found ${named.length}`);
  assert.deepEqual(menuIds(model), named);
  assert.deepEqual(
    model.map((g) => g.id),
    ['menu.file', 'menu.project', 'menu.view', 'menu.help'],
  );
});

test('every item of the menu has an action in the page, and the language prefix is the one of the page', () => {
  const actions = MAIN.slice(MAIN.indexOf('const MENU_ACTIONS = {'), MAIN.indexOf('const LOCALE_ITEM'));
  const items = [];
  const walk = (nodes) => {
    for (const n of nodes) {
      if (n.type === 'sub') walk(n.items);
      else if (n.type === 'item' || n.type === 'check') items.push(n.id);
    }
  };
  for (const g of model) walk(g.items);
  for (const id of items) assert.ok(actions.includes(`"${id}"`), `${id} has no action in main.js`);
  assert.ok(MAIN.includes(`const LOCALE_ITEM = "${LOCALE_ID}"`));
  assert.ok(RUST.includes(`pub const LOCALE_ID: &str = "${LOCALE_ID}"`));
});

test('what is switched off in the web variant is an item of the menu', () => {
  const ids = new Set(menuIds(model));
  assert.deepEqual(
    [...WEB_UNAVAILABLE].filter((id) => !ids.has(id)),
    [],
  );
});

test('a table this reader does not understand fails instead of losing an item', () => {
  const start = 'const GROUPS: &[Group] = &[';
  assert.throws(() => parseMenu('nothing here'), /GROUPS not found/);
  assert.throws(
    () => parseMenu(`${start} Group { id: "menu.x", items: &[ Node::Future("menu.x.y"), ], }, ];`),
    /not understood/,
  );
  assert.throws(() => parseMenu(`${start} ];`), /empty/);
  // A comment that mentions an item is not one; a string with `//` in it is not a comment.
  const parsed = parseMenu(
    `${start} Group { id: "menu.a", items: &[ // item("menu.a.ghost", None, false),\n item("menu.a.real", Some("Ctrl+//"), true), Node::Separator, ], }, ];`,
  );
  assert.deepEqual(parsed, [
    { id: 'menu.a', items: [{ type: 'item', id: 'menu.a.real', needsProject: true }, { type: 'separator' }] },
  ]);
});

test('the rows show the labels, the checks and what cannot be chosen', () => {
  const args = {
    labels: Object.fromEntries(menuIds(model).map((id) => [id, `L:${id}`])),
    hasProject: false,
    state: {
      theme: 'dark',
      locale: 'de',
      locales: [
        { code: 'en', label: 'English' },
        { code: 'de', label: 'Deutsch' },
      ],
      files: true,
      stats: false,
      sidebar: true,
      sidebarAuto: false,
    },
  };
  const groups = menuRows(model, args);
  const row = (id, from = groups) => from.flatMap((g) => g.rows).find((r) => r.id === id);
  assert.equal(groups[0].label, 'L:menu.file');
  assert.equal(row('menu.file.add').enabled, true);
  assert.equal(row('menu.file.remove').enabled, false, 'needs a project');
  assert.equal(row('menu.view.theme.dark').checked, true);
  assert.equal(row('menu.view.theme.system').checked, false);
  assert.equal(row('menu.view.files').checked, true);
  assert.equal(row(`${LOCALE_ID}de`).checked, true);
  assert.equal(row(`${LOCALE_ID}en`).label, 'English');
  assert.deepEqual([row('menu.help.usage').enabled, row('menu.help.usage').unavailable], [false, true]);
  assert.equal(row('menu.help.about').enabled, true);
  assert.equal(row('menu.file.remove', menuRows(model, { ...args, hasProject: true })).enabled, true);
  assert.equal(isChecked('menu.view.sidebar_auto', { sidebarAuto: true }), true);
  // Nothing breaks before the page has sent its first labels.
  assert.equal(menuRows(model, { labels: {}, hasProject: false, state: {} }).length, 4);
});
