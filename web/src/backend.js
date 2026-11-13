// The commands of the window, answered in the browser: what `src-tauri` answers in the desktop app.
//
// The page (src/main.js) knows one way to reach its backend, `invoke(command, args)`. Here every command it calls has
// an answer of the same shape, or a refusal that says so (`web.unavailable`): there is no program behind this
// page, so nothing that needs one (an editor, git, GitHub, Neovim, files outside the folder a person picked).
//
//   projects      the folders a person picked, kept as handles in IndexedDB, in workspaces as in the app
//   the engine    docmap.nvim in a Lua VM in a worker (engine.js); a map is generated in memory, never written back
//   the files     the folder as it was read, inside the VM (vmfs.js)
//
// Nothing here touches the DOM: the caller hands in what it needs of the browser (`deps`), a test hands in fakes.

import { computeDeps } from './deps.js';
import { FolderTooLarge, folderSignature, inspectFolder, readFolder } from './folder-read.js';
import { searchView } from './view-search.js';
import { PROJECTS } from './worker.js';

/** What the page calls and this variant has no answer for: each is refused by name. */
export const UNAVAILABLE = [
  'open_in_editor',
  'editor_command',
  'reveal_project',
  'reveal_settings',
  'import_from_url',
  'import_from_nvim_config',
  'list_github_repos',
  'set_engine',
  'set_grammars',
  'set_nvim_path',
  'set_nvim_config_dir',
  'set_telemetry',
  'telemetry_info',
  'project_stats',
  'open_docs',
  'search_docs',
  'open_feedback',
  'traffic_detail',
  'traffic_set_dir',
  'traffic_ask_neovim',
  'traffic_set_hidden',
];

const DEFAULT_WORKSPACE = 'Default';
const DEFAULT_OUT_DIR = 'docs/map';
const STATE_KEY = 'state';
const decoder = new TextDecoder();

/**
 * An error as a command of the app fails (src-tauri/src/apperr.rs): a key of the catalog, its parameters, a detail.
 * @param {string} key
 * @param {Record<string, string>} [params]
 * @param {string} [detail]
 */
export function appError(key, params = {}, detail) {
  const message = `${key}${Object.keys(params).length ? ' ' + JSON.stringify(params) : ''}`;
  return { key, params, detail: detail ?? null, message: detail ? `${message} — ${detail}` : message };
}

/** @param {string} what */
const unavailable = (what) => appError('web.unavailable', { what });

/**
 * A relative folder inside a project, as the settings give it (`docs/map`), or null when it is none.
 * @param {unknown} dir
 */
export function plainOutDir(dir) {
  if (typeof dir !== 'string') return null;
  const parts = dir.replaceAll('\\', '/').split('/').filter((p) => p !== '' && p !== '.');
  if (parts.length === 0 || /^[A-Za-z]:$/.test(parts[0]) || dir.startsWith('/') || dir.startsWith('\\')) return null;
  if (parts.some((p) => p === '..' || /\0/.test(p))) return null;
  return parts.join('/');
}

/**
 * @typedef {{ id: string, name: string, root: string, map_dir: string, exclude: string[], languages: string[] | null,
 *   out_dir: string | null, source: string | null, repo_url: string | null, branch: string | null, full: boolean,
 *   traffic_hidden: boolean, handle: any }} StoredProject
 * @typedef {{ version: 1, active: string, nextId: number, workspaces: { name: string, projects: StoredProject[] }[] }} State
 * @typedef {{ loading: Promise<void> | null, loaded: boolean, signature: string | null,
 *   generated: { files: Record<string, Uint8Array>, dir: string, at: number } | null,
 *   skipped: import('./folder-read.js').Skipped[], mapHtml: string | null, urls: Map<string, string> }} Session
 */

/**
 * @param {{
 *   client: { ask(type: string, args?: object, transfer?: Transferable[]): Promise<any> },
 *   kv: { get(key: string): Promise<unknown>, put(key: string, value: unknown): Promise<void> },
 *   pick: () => Promise<{ kind: 'picked', handle: any } | { kind: 'cancelled' } | { kind: 'blocked' | 'failed', message: string }>,
 *   permission: { of(handle: any): Promise<string>, grant(handle: any): Promise<string> },
 *   now: () => number,
 *   info: { version: string, build: string | null, engine: { commit: string, lib_commit: string },
 *     contract: { commit: string, schema: number, routes: string[] }, platform: string },
 *   ui: { setMenu(args: object): void, setTitle(title: string): void, setZoom(factor: number): void,
 *     download(name: string, contents: string): void, openUrl(url: string): void,
 *     blobUrl(html: string): string, revokeUrl(url: string): void, notice(state: object): void },
 * }} deps
 */
export function createBackend(deps) {
  /** @type {State | null} */
  let state = null;
  /** @type {Map<string, Session>} */
  const sessions = new Map();
  /** The folders picked in this session, by the name the page was given for them. @type {Map<string, any>} */
  const picked = new Map();
  /** @type {Map<string, Set<(event: { payload: any }) => void>>} */
  const listeners = new Map();

  // ------------------------------------------------------------------ state

  async function load() {
    if (state) return state;
    const stored = /** @type {any} */ (await deps.kv.get(STATE_KEY).catch(() => null));
    if (stored && stored.version === 1 && Array.isArray(stored.workspaces) && stored.workspaces.length > 0) {
      // What IndexedDB holds is not trusted to be whole: a project without a handle or an id is dropped.
      const workspaces = stored.workspaces
        .filter((/** @type {any} */ w) => w && typeof w.name === 'string' && Array.isArray(w.projects))
        .map((/** @type {any} */ w) => ({
          name: w.name,
          projects: w.projects.filter(
            (/** @type {any} */ p) =>
              p && typeof p.id === 'string' && /^[A-Za-z0-9_-]+$/.test(p.id) && typeof p.name === 'string' && p.handle,
          ),
        }));
      if (workspaces.length > 0) {
        state = {
          version: 1,
          active: workspaces.some((/** @type {{name: string}} */ w) => w.name === stored.active)
            ? stored.active
            : workspaces[0].name,
          nextId: Number.isInteger(stored.nextId) ? stored.nextId : 1,
          workspaces,
        };
        return state;
      }
    }
    state = { version: 1, active: DEFAULT_WORKSPACE, nextId: 1, workspaces: [{ name: DEFAULT_WORKSPACE, projects: [] }] };
    return state;
  }

  async function save() {
    // A browser that cannot keep the list (private mode, storage refused) still works for this session.
    await deps.kv.put(STATE_KEY, state).catch(() => {});
  }

  /** @param {State} s */
  const active = (s) => /** @type {State['workspaces'][number]} */ (s.workspaces.find((w) => w.name === s.active));

  /** @param {StoredProject} p */
  const shown = (p) => {
    const { handle: _handle, ...rest } = p;
    return rest;
  };

  /** @param {State} s */
  const listed = (s) => active(s).projects.map(shown);

  /** @param {State} s */
  const everyProject = (s) => s.workspaces.flatMap((w) => w.projects);

  /** @param {string} id */
  async function projectById(id) {
    const p = active(await load()).projects.find((x) => x.id === id);
    if (!p) throw appError('err.project.unknown', { id: String(id) });
    return p;
  }

  /** @param {string} root */
  async function projectByRoot(root) {
    const p = everyProject(await load()).find((x) => x.root === root);
    if (!p) throw appError('err.path.notDirectory', { path: String(root) });
    return p;
  }

  /** @param {StoredProject} p */
  const mapDirOf = (p) => `${p.root}/${plainOutDir(p.out_dir) ?? DEFAULT_OUT_DIR}`;

  // --------------------------------------------------------------- sessions

  /** @param {StoredProject} p @returns {Session} */
  function sessionOf(p) {
    let s = sessions.get(p.id);
    if (!s) {
      s = { loading: null, loaded: false, signature: null, generated: null, skipped: [], mapHtml: null, urls: new Map() };
      sessions.set(p.id, s);
    }
    return s;
  }

  /** @param {Session} s */
  function dropMapPage(s) {
    for (const url of s.urls.values()) deps.ui.revokeUrl(url);
    s.urls.clear();
    s.mapHtml = null;
  }

  /** @param {string} id */
  async function forget(id) {
    const s = sessions.get(id);
    if (s) dropMapPage(s);
    sessions.delete(id);
  }

  /** What the page shows about access: the projects whose folder the browser will not read without a click. */
  async function publishNotice() {
    const s = await load();
    const waiting = [];
    const skipped = [];
    for (const p of active(s).projects) {
      const session = sessions.get(p.id);
      if (session?.loaded) {
        if (session.skipped.length > 0) skipped.push({ id: p.id, name: p.name, skipped: session.skipped });
        continue;
      }
      const perm = await deps.permission.of(p.handle).catch(() => 'gone');
      if (perm !== 'granted') waiting.push({ id: p.id, name: p.name, state: perm });
    }
    deps.ui.notice({ waiting, skipped });
  }

  /**
   * Read the folder of `p` into the VM. `ask` true asks the browser for the permission, which it only does right after
   * a click of the person: so only what a click starts asks (generating, checking, "Allow access"). Everything the
   * window asks on its own (the list at the start, the project selected last time) only reads a folder that may be
   * read already, and otherwise fails with `web.access`, which the strip at the top turns into one button.
   * @param {StoredProject} p
   * @param {{ ask?: boolean, again?: boolean }} [opts] `again` reads a folder that is loaded once more
   */
  function ensureLoaded(p, opts = {}) {
    const s = sessionOf(p);
    if (s.loading) return s.loading;
    if (s.loaded && !opts.again) return Promise.resolve();
    s.loading = (async () => {
      let perm = await deps.permission.of(p.handle).catch(() => 'gone');
      if (perm === 'prompt' && opts.ask) perm = await deps.permission.grant(p.handle).catch(() => 'prompt');
      if (perm === 'gone') throw appError('web.gone', { name: p.name });
      if (perm !== 'granted') throw appError('web.access', { name: p.name });
      let read;
      try {
        read = await readFolder(p.handle);
      } catch (err) {
        if (err instanceof FolderTooLarge) {
          throw appError('web.tooLarge', { name: p.name, limit: String(err.limit), what: err.what });
        }
        throw appError('web.read', { name: p.name }, String(err instanceof Error ? err.message : err));
      }
      s.signature = await folderSignature(p.handle).catch(() => null);
      const buffers = Object.values(read.files).map((b) => b.buffer);
      await deps.client.ask(
        'mount',
        { root: p.root, files: read.files, mtimes: read.mtimes, overlay: s.generated?.files ?? {}, overlayAt: s.generated?.at },
        buffers,
      );
      s.skipped = read.skipped;
      s.loaded = true;
      dropMapPage(s);
    })().finally(() => {
      s.loading = null;
      publishNotice().catch(() => {});
    });
    return s.loading;
  }

  /** The folder of `p`, loaded when it may be read; false when it may not (the page then shows "no map"). */
  async function loadedQuietly(/** @type {StoredProject} */ p) {
    try {
      await ensureLoaded(p);
      return true;
    } catch {
      return false;
    }
  }

  /** @param {StoredProject} p @param {string} path absolute inside the VM */
  async function readText(p, path) {
    const got = await deps.client.ask('read', { root: p.root, path });
    return got ? { text: decoder.decode(got.bytes), mtime: got.mtime } : null;
  }

  /** @param {StoredProject} p */
  async function artifact(p) {
    if (!(await loadedQuietly(p))) return null;
    const got = await readText(p, `${mapDirOf(p)}/module_map.json`);
    if (!got) return null;
    try {
      return JSON.parse(got.text);
    } catch {
      return null;
    }
  }

  // --------------------------------------------------------------- projects

  /** @param {any} handle @param {StoredProject[]} among */
  async function sameAs(handle, among) {
    for (const p of among) {
      try {
        if (typeof handle.isSameEntry === 'function' && (await handle.isSameEntry(p.handle))) return p;
      } catch {
        // a handle that cannot be compared is another folder
      }
    }
    return null;
  }

  /** @param {State} s @param {any} handle @returns {StoredProject} */
  function newProject(s, handle) {
    const id = `p${s.nextId++}`;
    const name = String(handle.name);
    const root = `${PROJECTS}/${id}/${name}`;
    return {
      id,
      name,
      root,
      map_dir: `${root}/${DEFAULT_OUT_DIR}`,
      exclude: [],
      languages: null,
      out_dir: null,
      source: null,
      repo_url: null,
      branch: null,
      full: false,
      traffic_hidden: false,
      handle,
    };
  }

  /** @param {string} token */
  function pickedHandle(token) {
    const handle = picked.get(token);
    if (!handle) throw appError('err.path.notDirectory', { path: String(token) });
    return handle;
  }

  /** @param {StoredProject} p @param {string[]} argv */
  async function run(p, argv) {
    // The folder is read again first: the map is the map of what is on the disk now, not of the last look at it.
    await ensureLoaded(p, { ask: true, again: true });
    const answer = await deps.client.ask('generate', {
      request: {
        root: p.root,
        exclude: p.exclude.map((e) => e.trim()).filter(Boolean),
        languages: p.languages ?? [],
        out_dir: plainOutDir(p.out_dir) ?? undefined,
        source: p.source ? p.source.split(',').map((x) => x.trim()).filter(Boolean) : [],
        repo_url: p.repo_url || undefined,
        branch: p.branch || undefined,
        argv,
      },
    });
    const text = (/** @type {'out' | 'err'} */ stream) =>
      answer.log.filter((/** @type {{stream: string}} */ l) => l.stream === stream).map((/** @type {{text: string}} */ l) => l.text).join('');
    if (answer.status === 'error') return { code: -1, stdout: text('out'), stderr: text('err') + answer.payload + '\n' };
    return { code: /** @type {number} */ (answer.code ?? -1), stdout: text('out'), stderr: text('err') };
  }

  // ---------------------------------------------------------------- commands

  /** @type {Record<string, (args: any) => any>} */
  const commands = {
    list_projects: async () => listed(await load()),

    list_workspaces: async () => {
      const s = await load();
      return s.workspaces.map((w) => ({ name: w.name, projects: w.projects.length, active: w.name === s.active }));
    },

    switch_workspace: async ({ name }) => {
      const s = await load();
      const clean = String(name ?? '').trim();
      if (clean === '') throw appError('err.workspace.exists', { name: clean });
      if (!s.workspaces.some((w) => w.name === clean)) s.workspaces.push({ name: clean, projects: [] });
      s.active = clean;
      await save();
      publishNotice().catch(() => {});
      return listed(s);
    },

    rename_workspace: async ({ from, to }) => {
      const s = await load();
      const clean = String(to ?? '').trim();
      const ws = s.workspaces.find((w) => w.name === from);
      if (!ws) return null;
      if (clean === '' || s.workspaces.some((w) => w !== ws && w.name === clean)) {
        throw appError('err.workspace.exists', { name: clean });
      }
      if (s.active === ws.name) s.active = clean;
      ws.name = clean;
      await save();
      return null;
    },

    delete_workspace: async ({ name }) => {
      const s = await load();
      if (s.workspaces.length <= 1) throw appError('err.workspace.last');
      const gone = s.workspaces.find((w) => w.name === name);
      if (!gone) return null;
      s.workspaces = s.workspaces.filter((w) => w !== gone);
      for (const p of gone.projects) {
        await deps.client.ask('unmount', { root: p.root }).catch(() => {});
        await forget(p.id);
      }
      if (s.active === name) s.active = s.workspaces[0].name;
      await save();
      return null;
    },

    inspect_folder: async ({ root }) => {
      const handle = pickedHandle(root);
      const s = await load();
      const scan = await inspectFolder(handle);
      const subrepos = [];
      for (const sub of scan.subfolders) {
        const path = `${root}/${sub.name}`;
        picked.set(path, sub.handle);
        subrepos.push({
          name: sub.name,
          path,
          isGit: sub.isGit,
          alreadyAdded: (await sameAs(sub.handle, active(s).projects)) !== null,
        });
      }
      return { isGit: scan.isGit, subrepos };
    },

    add_project: async ({ root }) => {
      const handle = pickedHandle(root);
      const s = await load();
      if (!(await sameAs(handle, active(s).projects))) {
        active(s).projects.push(newProject(s, handle));
        await save();
      }
      publishNotice().catch(() => {});
      return listed(s);
    },

    import_many: async ({ roots }) => {
      const s = await load();
      const added = [];
      let already = 0;
      const errors = [];
      for (const root of roots ?? []) {
        const handle = picked.get(root);
        if (!handle) {
          errors.push({ root: String(root), error: 'not a folder that was picked' });
          continue;
        }
        if (await sameAs(handle, active(s).projects)) {
          already += 1;
          continue;
        }
        const p = newProject(s, handle);
        active(s).projects.push(p);
        added.push(shown(p));
      }
      await save();
      publishNotice().catch(() => {});
      return { found: (roots ?? []).length, added, already_present: already, errors };
    },

    remove_project: async ({ id }) => {
      const s = await load();
      const ws = active(s);
      const gone = ws.projects.find((p) => p.id === id);
      if (gone) {
        ws.projects = ws.projects.filter((p) => p !== gone);
        await deps.client.ask('unmount', { root: gone.root }).catch(() => {});
        await forget(gone.id);
        await save();
      }
      publishNotice().catch(() => {});
      return listed(s);
    },

    project_scope_get: async ({ id }) => {
      const p = await projectById(id);
      return {
        exclude: p.exclude,
        languages: p.languages,
        outDir: p.out_dir,
        source: p.source,
        repoUrl: p.repo_url,
        branch: p.branch,
        full: p.full,
      };
    },

    project_scope_set: async (args) => {
      const p = await projectById(args.id);
      const text = (/** @type {unknown} */ v) => (typeof v === 'string' && v.trim() !== '' ? v.trim() : null);
      const outDir = text(args.outDir);
      if (outDir !== null && plainOutDir(outDir) === null) throw appError('err.outDir.unsafe', { value: outDir });
      p.exclude = Array.isArray(args.exclude) ? args.exclude.map(String) : [];
      p.languages = Array.isArray(args.languages) && args.languages.length > 0 ? args.languages.map(String) : null;
      p.out_dir = outDir;
      p.source = text(args.source);
      p.repo_url = text(args.repoUrl);
      p.branch = text(args.branch);
      p.full = Boolean(args.full);
      p.map_dir = mapDirOf(p);
      await save();
      return null;
    },

    map_dir_refused: async ({ dir }) => {
      // A stored map folder is `<root>/<relative folder>`; one that names anything else is refused.
      const s = await load();
      return !everyProject(s).some((p) => typeof dir === 'string' && dir.startsWith(`${p.root}/`) && plainOutDir(dir.slice(p.root.length + 1)) !== null);
    },

    map_status: async ({ mapDir }) => {
      const index = `${mapDir}/index.html`;
      const none = { exists: false, index_path: index, modules: null, files: null, namespaces: null, schema: null };
      const p = everyProject(await load()).find((x) => x.map_dir === mapDir);
      if (!p || !(await loadedQuietly(p))) return none;
      const page = await deps.client.ask('read', { root: p.root, path: index });
      if (!page) return none;
      const doc = await artifact(p);
      const counts = doc?.meta?.counts ?? {};
      const number = (/** @type {unknown} */ v) => (Number.isInteger(v) ? v : null);
      return {
        exists: true,
        index_path: index,
        modules: number(counts.module),
        files: number(counts.file),
        namespaces: number(counts.namespace),
        schema: number(doc?.meta?.schema),
      };
    },

    map_freshness: async ({ id }) => {
      const p = await projectById(id);
      if (!(await loadedQuietly(p))) {
        return { hasMap: false, stale: false, newest: null, behindSecs: null, truncated: false, generatedSecs: null };
      }
      return deps.client.ask('freshness', { root: p.root, mapDir: mapDirOf(p), now: deps.now() });
    },

    map_changes: async ({ id, limit }) => {
      const p = await projectById(id);
      if (!(await loadedQuietly(p))) return { files: [], total: 0, truncated: false };
      return deps.client.ask('changes', { root: p.root, mapDir: mapDirOf(p), limit: Number(limit) || 50 });
    },

    scan_languages: async ({ root }) => {
      const p = await projectByRoot(root);
      await ensureLoaded(p);
      return deps.client.ask('languages', { root: p.root, mapDir: mapDirOf(p) });
    },

    generate: async ({ root, full }) => {
      const p = await projectByRoot(root);
      const s = sessionOf(p);
      const res = await run(p, [...(full || p.full ? ['--full'] : []), '--lenient']);
      if (res.code === 0) {
        const dir = mapDirOf(p);
        const rel = dir.slice(p.root.length + 1);
        const files = await deps.client.ask('readTree', { root: p.root, dir });
        // The time the VM gave the map, so that a later read of the folder puts it back as old as it is.
        const written = await deps.client.ask('read', { root: p.root, path: `${dir}/module_map.json` });
        s.generated = {
          // Kept as paths relative to the root, which is how a later read of the folder puts them back.
          files: Object.fromEntries(Object.entries(files).map(([k, v]) => [`${rel}/${k}`, v])),
          dir,
          at: written?.mtime ?? deps.now(),
        };
      }
      dropMapPage(s);
      return { ok: res.code === 0, code: res.code, stdout: res.stdout, stderr: res.stderr };
    },

    check_map: async ({ root }) => {
      const p = await projectByRoot(root);
      const res = await run(p, ['--check', '--lenient']);
      return { current: res.code === 0, code: res.code, stdout: res.stdout, stderr: res.stderr };
    },

    serve_project: async ({ id }) => {
      const p = await projectById(id);
      await ensureLoaded(p);
      const s = sessionOf(p);
      const page = await readText(p, `${mapDirOf(p)}/index.html`);
      if (!page) throw appError('err.project.unknown', { id: String(id) });
      dropMapPage(s);
      s.mapHtml = page.text;
      return `webmap:${p.id}`;
    },

    open_map_in_browser: async ({ id }) => {
      const p = await projectById(id);
      const url = backend.mapUrl(`webmap:${p.id}`, 'system', null);
      if (!url) throw appError('err.project.unknown', { id: String(id) });
      deps.ui.openUrl(url);
      return url;
    },

    file_tree: async ({ id, sub }) => {
      const p = await projectById(id);
      await ensureLoaded(p);
      return deps.client.ask('tree', { root: p.root, sub: String(sub ?? '') });
    },

    project_search: async ({ id, sub, query, mode, caseSensitive }) => {
      const p = await projectById(id);
      await ensureLoaded(p);
      return deps.client.ask('search', {
        root: p.root,
        mapDir: mapDirOf(p),
        ask: { sub: String(sub ?? ''), query: String(query ?? ''), mode, caseSensitive: Boolean(caseSensitive) },
      });
    },

    view_search: async ({ id, query }) => searchView(await artifact(await projectById(id)), String(query ?? '')),

    workspace_deps: async () => {
      const s = await load();
      const maps = [];
      for (const p of active(s).projects) maps.push({ id: p.id, map: await artifact(p) });
      return computeDeps(maps);
    },

    project_icon: async () => null,

    engine_info: async () => ({ path: `web+Lua5.4 (docmap.nvim ${deps.info.engine.commit.slice(0, 12)})`, from_path: false, bundled: true, grammars: null }),

    engine_languages: async () => {
      const answer = await deps.client.ask('capabilities');
      /** @type {any} */
      const caps = answer.status === 'ok' ? JSON.parse(answer.payload) : {};
      return {
        languages: caps.languages ?? null,
        schema: caps.schema ?? null,
        build: { commit: deps.info.engine.commit.slice(0, 12), committed_at: null, dirty: false },
        routes: caps.routes ?? null,
        contract: deps.info.contract,
      };
    },

    grammar_dir: async () => ({ dir: null, from_setting: false, exists: false, files: [], more: 0 }),

    nvim_info: async () => ({ path: null, from_path: false, config_dir: null, config_dir_from_default: false }),

    about_info: async () => ({
      appVersion: `${deps.info.version} (web${deps.info.build ? ', ' + deps.info.build : ''})`,
      os: 'browser',
      arch: deps.info.platform,
      enginePath: `web+Lua5.4 (docmap.nvim ${deps.info.engine.commit.slice(0, 12)})`,
      grammars: null,
    }),


    traffic_list: async ({ ids }) =>
      (ids ?? []).map((/** @type {string} */ id) => ({
        id,
        status: 'no_remote',
        views7: null,
        views30: null,
        clones30: null,
        trend: null,
        fetched: null,
      })),
    traffic_info: async () => ({ status: 'no_remote', repo: null, message: null, summary: null, dir: null, via: null }),
    traffic_settings: async () => ({ explicit: null, asked: null, defaultDir: '', attempts: [], found: null }),
    traffic_refresh: async () => null,

    save_text: async ({ path, contents }) => {
      deps.ui.download(String(path).replace(/^download:/, ''), String(contents));
      return null;
    },

    set_menu: async (args) => {
      deps.ui.setMenu(args);
      return null;
    },
    set_window_title: async ({ title }) => {
      deps.ui.setTitle(String(title));
      return null;
    },
    set_zoom: async ({ factor }) => {
      const clamped = Math.min(3, Math.max(0.5, Number(factor) || 1));
      deps.ui.setZoom(clamped);
      return clamped;
    },
  };

  for (const name of UNAVAILABLE) {
    commands[name] = async () => {
      throw unavailable(name);
    };
  }

  const backend = {
    /** @param {string} command @param {object} [args] */
    invoke: async (command, args) => {
      const run = commands[command];
      if (!run) throw unavailable(command);
      return run(args ?? {});
    },

    /** The names of the commands this variant answers (refusals included). */
    commands: () => Object.keys(commands).sort(),

    dialog: {
      /** Only a folder can be picked: the picker of the browser, read only. @param {{ directory?: boolean }} [options] */
      open: async (options = {}) => {
        if (!options.directory) return null;
        const result = await deps.pick();
        if (result.kind === 'cancelled') return null;
        if (result.kind !== 'picked') throw appError('web.picker', {}, result.message);
        const token = `folder:${result.handle.name}`;
        picked.set(token, result.handle);
        return token;
      },
      /** A file is saved as a download: the name is all the page learns. @param {{ defaultPath?: string }} [options] */
      save: async (options = {}) => `download:${String(options.defaultPath ?? 'export.txt').split(/[\\/]/).pop()}`,
    },

    /** @param {string} name @param {(event: { payload: any }) => void} listener */
    listen: async (name, listener) => {
      const set = listeners.get(name) ?? new Set();
      set.add(listener);
      listeners.set(name, set);
      return () => set.delete(listener);
    },

    /** @param {string} name @param {any} payload */
    emit: (name, payload) => {
      for (const listener of listeners.get(name) ?? []) listener({ payload });
    },

    /**
     * The address of the map of a project for the frame: a blob of the page with the theme set in it (a blob's
     * address cannot carry a query, which is how the page of the app is told its theme).
     * @param {string} base what `serve_project` answered
     * @param {string} theme `light`, `dark` or anything else for the system's
     * @param {string | null} tab
     */
    mapUrl: (base, theme, tab) => {
      const id = String(base).replace(/^webmap:/, '');
      const s = sessions.get(id);
      if (!s || s.mapHtml === null) return '';
      const key = theme === 'light' || theme === 'dark' ? theme : 'system';
      let url = s.urls.get(key);
      if (!url) {
        url = deps.ui.blobUrl(themed(s.mapHtml, key));
        s.urls.set(key, url);
      }
      return tab ? `${url}#tab=${encodeURIComponent(tab)}` : url;
    },

    /**
     * Ask the browser for the folders that wait for a permission (a click of the person is on the stack), then read
     * them. Resolves with the number that can be read now.
     */
    allowAccess: async () => {
      const s = await load();
      let granted = 0;
      for (const p of active(s).projects) {
        if (sessions.get(p.id)?.loaded) continue;
        try {
          await ensureLoaded(p, { ask: true });
          granted += 1;
        } catch {
          // stays in the notice
        }
      }
      await publishNotice();
      return granted;
    },

    /**
     * Look at the folders that are loaded: one that changed on disk is read again. Resolves with the ids that were.
     * @returns {Promise<string[]>}
     */
    refreshChanged: async () => {
      const s = await load();
      const changed = [];
      for (const p of active(s).projects) {
        const session = sessions.get(p.id);
        if (!session?.loaded || session.loading) continue;
        const now = await folderSignature(p.handle).catch(() => null);
        if (now === null || now === session.signature) continue;
        await ensureLoaded(p, { again: true }).catch(() => {});
        changed.push(p.id);
      }
      return changed;
    },

    publishNotice,
  };
  return backend;
}

/**
 * The page of a map with its theme set: one script before the end of the head. The page reads `?theme=` from its own
 * address first; this sets the same attribute after it, so the choice of the window wins.
 * @param {string} html
 * @param {'light' | 'dark' | 'system'} theme
 */
export function themed(html, theme) {
  if (theme === 'system') return html;
  const script = `<script>document.documentElement.setAttribute("data-theme","${theme}")</script>`;
  const at = html.search(/<\/head>/i);
  return at < 0 ? script + html : html.slice(0, at) + script + html.slice(at);
}
