// The commands of the window, answered in the browser: the backend with the real engine (in this process), folders that
// behave like the picker's, and an IndexedDB that is a Map.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { grantAccess, permissionOf } from 'appkit-web/folder';
import { readLock } from '../engine-bundle.mjs';
import { UNAVAILABLE, createBackend, plainOutDir, themed } from '../src/backend.js';
import { fakeFolder } from './fake-folder.mjs';
import { fixtureFiles, startedClient, web } from './helpers.mjs';

const lock = readLock();
let client;
before(async () => {
  client = await startedClient();
});
after(() => client.close());

const GIT = { '.git/HEAD': 'ref: refs/heads/main\n' };
const sample = (name = 'sample', extra = {}) => fakeFolder(name, { ...fixtureFiles(), ...GIT, ...extra });

/** A page: a backend over `store` (what IndexedDB kept) that picks from `folders` in turn. */
function page({ store = new Map(), folders = [], clock = { now: 1_800_000_000_000 } } = {}) {
  const ui = { menus: [], titles: [], zooms: [], downloads: [], opened: [], blobs: new Map(), revoked: [], notices: [] };
  let nextBlob = 1;
  const queue = [...folders];
  const backend = createBackend({
    client,
    kv: { get: async (key) => structuredCloneKeepingHandles(store.get(key)), put: async (key, value) => void store.set(key, value) },
    pick: async () => {
      const next = queue.shift();
      if (next === undefined) return { kind: 'cancelled' };
      if (next === 'blocked') return { kind: 'blocked', message: 'a policy forbids it' };
      return { kind: 'picked', handle: next.handle };
    },
    permission: { of: (handle) => permissionOf(handle, 'read'), grant: (handle) => grantAccess(handle, 'read') },
    now: () => clock.now,
    info: { version: '9.9.9', build: 'test', engine: { commit: lock.commit, lib_commit: lock.lib_commit }, contract: { commit: lock.commit, schema: lock.schema, routes: lock.routes }, platform: 'test' },
    ui: {
      setMenu: (args) => ui.menus.push(args),
      setTitle: (title) => ui.titles.push(title),
      setZoom: (factor) => ui.zooms.push(factor),
      download: (name, contents) => ui.downloads.push({ name, contents }),
      openUrl: (url) => ui.opened.push(url),
      blobUrl: (html) => {
        const url = `blob:test/${nextBlob++}`;
        ui.blobs.set(url, html);
        return url;
      },
      revokeUrl: (url) => {
        ui.revoked.push(url);
        ui.blobs.delete(url);
      },
      notice: (state) => ui.notices.push(state),
    },
  });
  return { backend, ui, store, clock, queue };
}

/** What a real IndexedDB does to the state: a copy, in which a handle is still a handle of the same folder. */
function structuredCloneKeepingHandles(value) {
  if (value === undefined) return undefined;
  const copy = (v) => {
    if (Array.isArray(v)) return v.map(copy);
    if (v && typeof v === 'object') {
      if (v.kind === 'directory') return v;
      return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, copy(x)]));
    }
    return v;
  };
  return copy(value);
}

/** Add the next folder of the page as a project, the way the window does it. */
async function addPicked(p) {
  const token = await p.backend.dialog.open({ directory: true });
  const scan = await p.backend.invoke('inspect_folder', { root: token });
  assert.equal(scan.isGit, true);
  return p.backend.invoke('add_project', { root: token });
}

test('every command the page calls has an answer here, or a refusal by name', async () => {
  const sources = [readFileSync(join(web, '..', 'src', 'main.js'), 'utf8')];
  for (const lib of ['languages.js', 'status-cache.js']) sources.push(readFileSync(join(web, '..', 'src', 'lib', lib), 'utf8'));
  const called = new Set(sources.flatMap((s) => [...s.matchAll(/invoke\(\s*"([a-z_]+)"/g)].map((m) => m[1])));
  assert.ok(called.size >= 55, `expected the commands of the page, found ${called.size}`);
  const { backend } = page();
  const answered = new Set(backend.commands());
  assert.deepEqual([...called].filter((c) => !answered.has(c)), [], 'a command of the page this variant does not know');
  // What is refused is refused as a command's error, with the name of what was asked for.
  for (const name of UNAVAILABLE) {
    await assert.rejects(backend.invoke(name, {}), (err) => err.key === 'web.unavailable' && err.params.what === name);
  }
  await assert.rejects(backend.invoke('a_command_of_tomorrow', {}), (err) => err.key === 'web.unavailable');
  // Nothing is refused that the page does not call: a refusal nobody can meet is dead text.
  assert.deepEqual(UNAVAILABLE.filter((name) => !called.has(name)), []);
});

test('a picked folder becomes a project, is mapped in memory, and its map is served as a page', async () => {
  const folder = sample();
  const p = page({ folders: [folder] });
  assert.deepEqual(await p.backend.invoke('list_projects'), []);
  const list = await addPicked(p);
  assert.equal(list.length, 1);
  const [project] = list;
  assert.deepEqual({ ...project, id: 'x', root: 'x', map_dir: 'x' }, {
    id: 'x', name: 'sample', root: 'x', map_dir: 'x', exclude: [], languages: null, out_dir: null, source: null,
    repo_url: null, branch: null, full: false, traffic_hidden: false,
  });
  assert.equal(project.root, `/w/${project.id}/sample`);
  assert.equal(project.map_dir, `${project.root}/docs/map`);
  assert.equal('handle' in project, false, 'the handle never leaves the backend');

  assert.deepEqual(await p.backend.invoke('map_status', { mapDir: project.map_dir }), {
    exists: false, index_path: `${project.map_dir}/index.html`, modules: null, files: null, namespaces: null, schema: null,
  });
  assert.equal((await p.backend.invoke('map_freshness', { id: project.id })).hasMap, false);

  const run = await p.backend.invoke('generate', { root: project.root, full: false });
  assert.equal(run.ok, true, run.stderr);
  assert.equal(run.code, 0);
  assert.match(run.stdout, /wrote docs\/map\/index\.html/);

  const status = await p.backend.invoke('map_status', { mapDir: project.map_dir });
  assert.deepEqual(status, { exists: true, index_path: `${project.map_dir}/index.html`, modules: 1, files: status.files, namespaces: status.namespaces, schema: lock.schema });
  const fresh = await p.backend.invoke('map_freshness', { id: project.id });
  assert.equal(fresh.hasMap, true);
  assert.equal(fresh.stale, false);

  const base = await p.backend.invoke('serve_project', { id: project.id });
  const dark = p.backend.mapUrl(base, 'dark', 'analysis');
  assert.match(dark, /^blob:test\/\d+#tab=analysis$/);
  const html = p.ui.blobs.get(dark.split('#')[0]);
  assert.match(html, /<script>document\.documentElement\.setAttribute\("data-theme","dark"\)<\/script>\s*<\/head>/i);
  assert.equal(p.backend.mapUrl(base, 'dark', null), dark.split('#')[0], 'one blob per theme, made once');
  const system = p.backend.mapUrl(base, 'system', null);
  assert.equal(p.ui.blobs.get(system).includes('setAttribute("data-theme","dark")</script></head>'), false);

  // Nothing was written into the folder: the page asked for it read only, and a fake has no way to write at all.
  assert.equal(folder.state.asked, 0);
});

test('the theme goes into the page of a map without touching what the page does itself', () => {
  const html = '<!doctype html><html><head><title>t</title></head><body></body></html>';
  assert.equal(themed(html, 'system'), html);
  assert.equal(themed(html, 'light'), '<!doctype html><html><head><title>t</title><script>document.documentElement.setAttribute("data-theme","light")</script></head><body></body></html>');
  // Without a head, or with `</head>` also inside a script (where an insert would break the script), the theme is set
  // at the very end: later, but never inside something.
  assert.match(themed('<p>no head</p>', 'dark'), /^<p>no head<\/p><script>.*<\/script>$/);
  const tricky = '<html><head><script>var s = "</head>";</script></head><body></body></html>';
  assert.equal(themed(tricky, 'dark'), `${tricky}<script>document.documentElement.setAttribute("data-theme","dark")</script>`);
});

test('the page of a map stays what it is while the folder is read again or checked, until it is asked for anew', async () => {
  const folder = sample();
  const p = page({ folders: [folder] });
  const [project] = await addPicked(p);
  assert.equal((await p.backend.invoke('generate', { root: project.root, full: false })).ok, true);
  const base = await p.backend.invoke('serve_project', { id: project.id });
  const light = p.backend.mapUrl(base, 'light', null);
  // Checking the map reads the folder again; the window does not ask for the map again afterwards, and then the
  // person switches the theme: the frame must get a page, not an empty address.
  assert.equal((await p.backend.invoke('check_map', { root: project.root })).current, true);
  assert.deepEqual(p.ui.revoked, []);
  assert.equal(p.backend.mapUrl(base, 'light', null), light);
  assert.match(p.backend.mapUrl(base, 'dark', null), /^blob:test\/\d+$/);
  // Asked for anew, the old pages are let go.
  await p.backend.invoke('serve_project', { id: project.id });
  assert.ok(p.ui.revoked.includes(light));
  assert.equal(p.ui.blobs.has(light), false);
});

test('a full run is the plain run: nothing that starts a program reaches the engine', async () => {
  const p = page({ folders: [sample()] });
  const [project] = await addPicked(p);
  const asked = await p.backend.invoke('generate', { root: project.root, full: true });
  assert.equal(asked.ok, true, asked.stderr);
  await p.backend.invoke('project_scope_set', { id: project.id, exclude: [], languages: null, outDir: null, full: true });
  const always = await p.backend.invoke('generate', { root: project.root, full: false });
  assert.equal(always.ok, true, always.stderr);
  // The whole report of the engine comes back, not its first line.
  assert.match(always.stdout, /wrote docs\/map\/index\.html\n[\s\S]*\d+ modules?, /);
});

test('what the store says about where a project lies is not believed', async () => {
  const first = page({ folders: [sample(), sample('other')] });
  const [project] = await addPicked(first);
  await addPicked(first);
  const stored = structuredCloneKeepingHandles(first.store.get('state'));
  const [a, b] = stored.workspaces[0].projects;
  // Another page of the same origin rewrote the list: one project points into the other, ids repeat, the counter
  // went back, and a name is a path.
  a.root = b.root;
  a.map_dir = '/w/p2/other/docs/map';
  a.out_dir = '../../p2/other';
  a.exclude = 'not a list';
  stored.workspaces[0].projects.push({ ...b }, { ...b, id: 'p9', name: '../escape' });
  stored.nextId = 1;
  const p = page({ store: new Map([['state', stored]]), folders: [sample('third')] });
  const list = await p.backend.invoke('list_projects');
  assert.deepEqual(list.map((x) => [x.id, x.name, x.root, x.map_dir]), [
    [project.id, 'sample', `/w/${project.id}/sample`, `/w/${project.id}/sample/docs/map`],
    [b.id, 'other', `/w/${b.id}/other`, `/w/${b.id}/other/docs/map`],
  ]);
  assert.deepEqual(list[0].exclude, []);
  assert.equal(list[0].out_dir, null);
  const added = await addPicked(p);
  assert.equal(new Set(added.map((x) => x.id)).size, 3, 'a new project never takes an id that is in use');
});

test('a run that asks waits for a read that is under way, and then asks itself', async () => {
  const folder = sample();
  const first = page({ folders: [folder] });
  const [project] = await addPicked(first);
  folder.state.permission = 'prompt';
  const p = page({ store: first.store });
  // The window measures the list on its own (no click, so nothing is asked), and the person clicks Generate at once.
  const quiet = p.backend.invoke('map_freshness', { id: project.id });
  const run = p.backend.invoke('generate', { root: project.root, full: false });
  assert.equal((await quiet).hasMap, false);
  assert.equal((await run).ok, true);
  assert.equal(folder.state.asked, 1);
});

test('a file that changed on disk makes the map stale, and generating reads the folder again', async () => {
  const folder = sample();
  const p = page({ folders: [folder] });
  const [project] = await addPicked(p);
  assert.equal((await p.backend.invoke('generate', { root: project.root, full: false })).ok, true);
  const base = await p.backend.invoke('serve_project', { id: project.id });
  const before = p.backend.mapUrl(base, 'system', null);

  assert.deepEqual(await p.backend.refreshChanged(), [], 'nothing changed: nothing is read again');
  // Another program adds a file a minute after the map was made.
  const source = new TextEncoder().encode("---@module 'sample.more'\n---@brief Added later.\nreturn {}\n");
  folder.raw('lua/sample/more.lua', { kind: 'file', name: 'more.lua', bytes: source, time: Date.now() + 60_000 });
  assert.deepEqual(await p.backend.refreshChanged(), [project.id]);
  const stale = await p.backend.invoke('map_freshness', { id: project.id });
  assert.equal(stale.stale, true, 'the generated map survived the new read and is older than the new file');
  assert.equal(stale.newest, 'lua/sample/more.lua');
  assert.ok(stale.behindSecs >= 58 && stale.behindSecs <= 60, String(stale.behindSecs));
  assert.deepEqual((await p.backend.invoke('map_changes', { id: project.id, limit: 5 })).files.map((f) => f.path), ['lua/sample/more.lua']);
  assert.equal(p.backend.mapUrl(base, 'system', null), before, 'the page that is shown stays until it is asked for anew');
  assert.equal((await p.backend.invoke('check_map', { root: project.root })).current, false);

  // Later (the file is now in the past) the map is generated again: it holds the new module and is fresh.
  folder.raw('lua/sample/more.lua', { kind: 'file', name: 'more.lua', bytes: source, time: Date.now() - 1000 });
  assert.equal((await p.backend.invoke('generate', { root: project.root, full: false })).ok, true);
  const found = await p.backend.invoke('view_search', { id: project.id, query: 'added later' });
  assert.equal(found.available, true);
  assert.deepEqual(found.hits.map((h) => h.file), ['lua/sample/more.lua']);
  assert.equal((await p.backend.invoke('map_freshness', { id: project.id })).stale, false);
  assert.equal((await p.backend.invoke('check_map', { root: project.root })).current, true);
});

test('a map that already lies in the folder is shown as it is, without generating', async () => {
  const first = page({ folders: [sample()] });
  const [made] = await addPicked(first);
  await first.backend.invoke('generate', { root: made.root, full: false });
  const mapFiles = await client.ask('readTree', { root: made.root, dir: made.map_dir });
  const committed = Object.fromEntries(Object.entries(mapFiles).map(([k, v]) => [`docs/map/${k}`, v]));

  const p = page({ folders: [sample('sample', committed)] });
  const [project] = await addPicked(p);
  const status = await p.backend.invoke('map_status', { mapDir: project.map_dir });
  assert.equal(status.exists, true);
  assert.equal(status.schema, lock.schema);
  assert.equal((await p.backend.invoke('map_freshness', { id: project.id })).hasMap, true);
  assert.match(p.ui.blobs.get(p.backend.mapUrl(await p.backend.invoke('serve_project', { id: project.id }), 'system', null)), /sample/);
});

test('projects and workspaces come back from the store; a broken store is an empty start', async () => {
  const folder = sample();
  const first = page({ folders: [folder, sample('other')] });
  const [project] = await addPicked(first);
  await first.backend.invoke('switch_workspace', { name: 'Work' });
  assert.deepEqual(await first.backend.invoke('list_projects'), []);
  await addPicked(first);
  assert.deepEqual(await first.backend.invoke('list_workspaces'), [
    { name: 'Default', projects: 1, active: false },
    { name: 'Work', projects: 1, active: true },
  ]);

  // The same store, a new page (the tab was reloaded).
  const second = page({ store: first.store });
  assert.deepEqual((await second.backend.invoke('list_projects')).map((x) => x.name), ['other']);
  assert.deepEqual((await second.backend.invoke('switch_workspace', { name: 'Default' })).map((x) => x.id), [project.id]);
  const status = await second.backend.invoke('map_status', { mapDir: project.map_dir });
  assert.equal(status.exists, false, 'a map made in the tab before is gone with it: nothing is kept');
  // A new project gets a new id, never one that was handed out before.
  const third = page({ store: first.store, folders: [sample('third')] });
  const added = await addPicked(third);
  assert.equal(new Set([...added.map((x) => x.id), project.id]).size, added.length);

  await second.backend.invoke('rename_workspace', { from: 'Work', to: 'Job' });
  await assert.rejects(second.backend.invoke('rename_workspace', { from: 'Job', to: 'Default' }), (e) => e.key === 'err.workspace.exists');
  await second.backend.invoke('delete_workspace', { name: 'Job' });
  await assert.rejects(second.backend.invoke('delete_workspace', { name: 'Default' }), (e) => e.key === 'err.workspace.last');
  assert.deepEqual((await second.backend.invoke('list_workspaces')).map((w) => w.name), ['Default']);

  for (const broken of [null, 'text', { version: 2 }, { version: 1, workspaces: [] }, { version: 1, workspaces: [{ name: 'x', projects: [{ id: '../evil', name: 'n', handle: {} }, { id: 'p1', name: 'no handle' }] }] }]) {
    const p = page({ store: new Map([['state', broken]]) });
    assert.deepEqual(await p.backend.invoke('list_projects'), [], JSON.stringify(broken));
  }
});

test('after a restart of the browser a folder waits for the permission, and one click reads it', async () => {
  const folder = sample();
  const first = page({ folders: [folder] });
  const [project] = await addPicked(first);
  folder.state.permission = 'prompt';

  const p = page({ store: first.store });
  assert.equal((await p.backend.invoke('list_projects')).length, 1);
  // Asked without a click (the list is measured at the start): nothing is requested, the project just has no map yet.
  assert.equal((await p.backend.invoke('map_status', { mapDir: project.map_dir })).exists, false);
  assert.equal((await p.backend.invoke('map_freshness', { id: project.id })).hasMap, false);
  assert.equal(folder.state.asked, 0);
  await p.backend.publishNotice();
  assert.deepEqual(p.ui.notices.at(-1).waiting, [{ id: project.id, name: 'sample', state: 'prompt' }]);

  assert.equal(await p.backend.allowAccess(), 1);
  assert.equal(folder.state.asked, 1);
  assert.deepEqual(p.ui.notices.at(-1).waiting, []);
  assert.equal((await p.backend.invoke('scan_languages', { root: project.root })).total, 4);

  // A permission the person refuses stays a refusal with the name of the folder.
  const denied = sample('locked');
  const q = page({ folders: [denied] });
  const [locked] = await addPicked(q);
  denied.state.permission = 'denied';
  await assert.rejects(q.backend.invoke('generate', { root: locked.root, full: false }), (e) => e.key === 'web.access' && e.params.name === 'locked');
  // A folder that is gone says so.
  denied.state.gone = true;
  await assert.rejects(q.backend.invoke('file_tree', { id: locked.id, sub: '' }), (e) => e.key === 'web.gone');
  await q.backend.publishNotice();
  assert.equal(q.ui.notices.at(-1).waiting[0].state, 'gone');
});

test('a folder that is no checkout lists the checkouts inside it, and several are added at once', async () => {
  const parent = fakeFolder('repos', {
    'one.nvim/.git/HEAD': 'r', 'one.nvim/lua/one/init.lua': "---@module 'one'\n---@brief One.\nreturn {}\n",
    'two.nvim/.git/HEAD': 'r', 'two.nvim/lua/two/init.lua': "---@module 'two'\n---@brief Two needs one.\nlocal one = require('one')\nreturn { one = one }\n",
    'notes/a.md': 'n',
  });
  const p = page({ folders: [parent, parent] });
  const token = await p.backend.dialog.open({ directory: true });
  assert.equal(token, 'folder:repos');
  const scan = await p.backend.invoke('inspect_folder', { root: token });
  assert.equal(scan.isGit, false);
  assert.deepEqual(scan.subrepos, [
    { name: 'notes', path: 'folder:repos/notes', isGit: false, alreadyAdded: false },
    { name: 'one.nvim', path: 'folder:repos/one.nvim', isGit: true, alreadyAdded: false },
    { name: 'two.nvim', path: 'folder:repos/two.nvim', isGit: true, alreadyAdded: false },
  ]);
  const res = await p.backend.invoke('import_many', { roots: ['folder:repos/one.nvim', 'folder:repos/two.nvim', 'folder:repos/never-picked'] });
  assert.equal(res.found, 3);
  assert.deepEqual(res.added.map((a) => a.name), ['one.nvim', 'two.nvim']);
  // A failure has the shape the app gives it: the name it was asked by, and a command's error.
  assert.equal(res.errors.length, 1);
  assert.equal(res.errors[0].name, 'folder:repos/never-picked');
  assert.equal(res.errors[0].error.key, 'err.path.notDirectory');
  // The same folder picked again is known.
  const again = await p.backend.invoke('inspect_folder', { root: await p.backend.dialog.open({ directory: true }) });
  assert.deepEqual(again.subrepos.map((s) => s.alreadyAdded), [false, true, true]);
  assert.equal((await p.backend.invoke('import_many', { roots: ['folder:repos/one.nvim'] })).already_present, 1);

  // What depends on what, from the maps alone.
  const [one, two] = await p.backend.invoke('list_projects');
  assert.deepEqual((await p.backend.invoke('workspace_deps')).unread.sort(), [one.id, two.id].sort());
  for (const project of [one, two]) assert.equal((await p.backend.invoke('generate', { root: project.root, full: false })).ok, true);
  const deps = await p.backend.invoke('workspace_deps');
  assert.deepEqual(deps.edges, [{ from: two.id, to: one.id, count: 1, modules: ['one'] }]);
  assert.deepEqual(deps.unread, []);

  assert.deepEqual((await p.backend.invoke('remove_project', { id: one.id })).map((x) => x.name), ['two.nvim']);
  await assert.rejects(p.backend.invoke('map_freshness', { id: one.id }), (e) => e.key === 'err.project.unknown');
  assert.equal(await client.ask('read', { root: one.root, path: `${one.root}/lua/one/init.lua` }), null, 'its files left the VM');
});

test('the scope of a project is kept and reaches the engine', async () => {
  const p = page({ folders: [sample()] });
  const [project] = await addPicked(p);
  assert.deepEqual(await p.backend.invoke('project_scope_get', { id: project.id }), {
    exclude: [], languages: null, outDir: null, source: null, repoUrl: null, branch: null, full: false,
  });
  await assert.rejects(p.backend.invoke('project_scope_set', { id: project.id, exclude: [], languages: null, outDir: '../outside' }), (e) => e.key === 'err.outDir.unsafe');
  await p.backend.invoke('project_scope_set', { id: project.id, exclude: ['web'], languages: null, outDir: 'out/map', source: null, repoUrl: null, branch: null, full: false });
  const [changed] = await p.backend.invoke('list_projects');
  assert.equal(changed.map_dir, `${project.root}/out/map`);
  assert.equal(await p.backend.invoke('map_dir_refused', { dir: changed.map_dir }), false);
  assert.equal(await p.backend.invoke('map_dir_refused', { dir: 'C:/elsewhere' }), true);
  assert.equal((await p.backend.invoke('generate', { root: project.root, full: false })).ok, true);
  assert.equal((await p.backend.invoke('map_status', { mapDir: changed.map_dir })).exists, true);
  assert.equal((await p.backend.invoke('map_status', { mapDir: project.map_dir })).exists, false);
});

test('a relative folder inside the project is one; anything else is none', () => {
  for (const [given, want] of [['docs/map', 'docs/map'], ['docs\\map\\', 'docs/map'], ['./out', 'out'], ['a/./b', 'a/b']]) assert.equal(plainOutDir(given), want);
  for (const bad of ['', '.', '/abs', '\\abs', 'C:/x', 'c:\\x', '../up', 'a/../../b', 'a\0b', null, 7]) assert.equal(plainOutDir(bad), null, String(bad));
});

test('what the window asks of the engine itself is answered from the page', async () => {
  const p = page();
  const info = await p.backend.invoke('engine_info');
  assert.match(info.path, /^web\+Lua5\.4 \(docmap\.nvim [0-9a-f]{12}\)$/);
  assert.equal(info.bundled, true);
  const langs = await p.backend.invoke('engine_languages');
  assert.equal(langs.schema, lock.schema);
  assert.deepEqual(langs.routes, lock.routes);
  assert.deepEqual(langs.contract, { commit: lock.commit, schema: lock.schema, routes: lock.routes });
  assert.equal(langs.build.commit, lock.commit.slice(0, 12));
  assert.ok(langs.languages.length > 10);
  assert.match((await p.backend.invoke('about_info')).appVersion, /^9\.9\.9 \(web, test\)$/);
  assert.deepEqual(await p.backend.invoke('traffic_list', { ids: ['a'] }), [{ id: 'a', status: 'no_remote', views7: null, views30: null, clones30: null, trend: null, fetched: null }]);
  assert.equal(await p.backend.invoke('set_zoom', { factor: 9 }), 3);
  assert.equal(await p.backend.invoke('set_zoom', { factor: 0.1 }), 0.5);
  await p.backend.invoke('set_window_title', { title: 'sample — docmap' });
  assert.deepEqual(p.ui.titles, ['sample — docmap']);
  await p.backend.invoke('save_text', { path: await p.backend.dialog.save({ defaultPath: 'C:\\x\\diagram.svg' }), contents: '<svg/>' });
  assert.deepEqual(p.ui.downloads, [{ name: 'diagram.svg', contents: '<svg/>' }]);
});

test('the picker: a file cannot be picked, a cancel is nothing, a block is an error that says why', async () => {
  const p = page({ folders: ['blocked'] });
  assert.equal(await p.backend.dialog.open({ multiple: false }), null, 'only a folder can be picked');
  await assert.rejects(p.backend.dialog.open({ directory: true }), (e) => e.key === 'web.picker' && /policy/.test(e.detail));
  assert.equal(await p.backend.dialog.open({ directory: true }), null);
  await assert.rejects(p.backend.invoke('add_project', { root: 'folder:never' }), (e) => e.key === 'err.path.notDirectory');
});

test('events reach who listens, until they stop listening', async () => {
  const { backend } = page();
  const got = [];
  const stop = await backend.listen('menu', (ev) => got.push(ev.payload));
  backend.emit('menu', 'menu.help.about');
  backend.emit('other', 'x');
  stop();
  backend.emit('menu', 'menu.file.add');
  assert.deepEqual(got, ['menu.help.about']);
});
