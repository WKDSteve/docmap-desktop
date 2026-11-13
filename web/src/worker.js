// The worker of the web variant: the engine (engine.js) and the projects' files run here, off the page's thread.
//
// A classic worker started from a blob, never `type: module`: a module worker from a blob does not start on a page
// opened from disk (`file://`, probe of the tasks-desktop pilot, 2026-10-28), a classic one does. The page hands in
// everything at `init`: the WASM of the VM, the Lua sources and the host, so the worker never loads a file and never
// asks the network.
//
// The VM's loader asks `fetch` for its WASM. For the one address WASM_URL this worker answers from the bytes it got;
// every other request goes to the real `fetch`, which the page's CSP (`connect-src 'none'`) refuses.

import { WebEngine } from './engine.js';
import * as vmfs from './vmfs.js';

export const WASM_URL = 'embedded:glue.wasm';

/** Where the projects lie inside the VM: `<PROJECTS>/<project id>/<folder name>`. */
export const PROJECTS = '/w';

/**
 * Let `fetch(WASM_URL)` answer with `bytes`, without a request.
 * @param {{ fetch: typeof fetch }} scope
 * @param {ArrayBuffer} bytes
 */
export function serveEmbeddedWasm(scope, bytes) {
  const real = scope.fetch.bind(scope);
  scope.fetch = (input, init) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    if (url === WASM_URL) {
      return Promise.resolve(new Response(bytes, { headers: { 'Content-Type': 'application/wasm' } }));
    }
    return real(input, init);
  };
}

/** A root the page may name: `<PROJECTS>/<id>/<name>`, plain segments. @param {unknown} root */
export function projectRoot(root) {
  if (typeof root !== 'string') return false;
  const parts = root.split('/');
  return (
    parts.length === 4 &&
    parts[0] === '' &&
    `/${parts[1]}` === PROJECTS &&
    /^[A-Za-z0-9_-]+$/.test(parts[2]) &&
    parts[3] !== '' &&
    parts[3] !== '.' &&
    parts[3] !== '..' &&
    !/[\\\0]/.test(parts[3])
  );
}

/** A path inside a project: its root, then plain segments. @param {string} root @param {unknown} path */
function inside(root, path) {
  if (typeof path !== 'string' || !(path === root || path.startsWith(`${root}/`))) return false;
  return path
    .slice(root.length)
    .split('/')
    .every((p) => p !== '.' && p !== '..' && !/[\\\0]/.test(p));
}

/**
 * The requests besides `init`, each answered with `{ id, ok: true, value }` or `{ id, ok: false, message }`.
 * @type {Record<string, (engine: WebEngine, args: any) => { value: unknown, transfer?: Transferable[] }>}
 */
const HANDLERS = {
  mount: (e, { root, files, mtimes, overlay, overlayAt }) => {
    for (const rel of [...Object.keys(files), ...Object.keys(overlay ?? {})]) {
      if (!inside(root, `${root}/${rel}`) || rel === '' || rel.split('/').includes('')) {
        throw new Error(`not a plain path inside the project: ${JSON.stringify(rel)}`);
      }
    }
    const fs = /** @type {vmfs.Fs} */ (e.fs);
    vmfs.mountTree(fs, root, files, mtimes);
    // What the engine wrote before the folder was read again (the map generated in this session) goes back on top.
    for (const [rel, bytes] of Object.entries(overlay ?? {})) vmfs.writeFileAt(fs, `${root}/${rel}`, bytes, overlayAt);
    return { value: Object.keys(files).length };
  },
  unmount: (e, { root }) => {
    vmfs.unmountTree(/** @type {vmfs.Fs} */ (e.fs), root);
    return { value: true };
  },
  generate: (e, { request }) => ({ value: e.generate(request) }),
  api: (e, { request }) => ({ value: e.api(request) }),
  capabilities: (e) => ({ value: e.capabilities() }),
  read: (e, { root, path }) => {
    if (!inside(root, path)) throw new Error('not a path inside the project');
    const got = vmfs.readFileAt(/** @type {vmfs.Fs} */ (e.fs), path);
    // A copy: the bytes of the VM's file system must not leave with the transfer.
    const bytes = got ? got.bytes.slice() : null;
    return { value: got ? { bytes, mtime: got.mtime } : null, transfer: bytes ? [bytes.buffer] : [] };
  },
  readTree: (e, { root, dir }) => {
    if (!inside(root, dir)) throw new Error('not a path inside the project');
    const files = vmfs.readTree(/** @type {vmfs.Fs} */ (e.fs), dir);
    for (const key of Object.keys(files)) files[key] = files[key].slice();
    return { value: files, transfer: Object.values(files).map((b) => b.buffer) };
  },
  stat: (e, { root, path }) => {
    if (!inside(root, path)) throw new Error('not a path inside the project');
    const got = vmfs.statFileAt(/** @type {vmfs.Fs} */ (e.fs), path);
    return { value: got };
  },
  tree: (e, { root, sub }) => ({ value: vmfs.fileTree(/** @type {vmfs.Fs} */ (e.fs), root, sub) }),
  freshness: (e, { root, mapDir, now }) => ({ value: vmfs.freshness(/** @type {vmfs.Fs} */ (e.fs), root, mapDir, now) }),
  changes: (e, { root, mapDir, limit }) => ({
    value: vmfs.changedSinceMap(/** @type {vmfs.Fs} */ (e.fs), root, mapDir, limit),
  }),
  languages: (e, { root, mapDir }) => ({ value: vmfs.scanLanguages(/** @type {vmfs.Fs} */ (e.fs), root, mapDir) }),
  search: (e, { root, mapDir, ask }) => ({ value: vmfs.searchFiles(/** @type {vmfs.Fs} */ (e.fs), root, mapDir, ask) }),
};

/** The requests that name no project. */
const ROOTLESS = new Set(['capabilities']);

/**
 * @param {{ fetch: typeof fetch, postMessage(message: any, transfer?: Transferable[]): void, onmessage: any }} scope
 * @param {{ factory?: () => any, LuaFactory?: any }} [options]
 *   `factory` is the VM factory of a test in Node (the stock one); the browser's loads the WASM from the bytes of
 *   `init`.
 */
export function startWorker(scope, options = {}) {
  /** @type {Promise<WebEngine> | null} */
  let engine = null;
  /** @param {any} reply @param {Transferable[]} [transfer] */
  const post = (reply, transfer) => scope.postMessage(reply, transfer ?? []);

  scope.onmessage = async (/** @type {{ data: any }} */ event) => {
    const msg = event.data;
    if (msg.type === 'init') {
      // One engine per worker: a second init would wrap `fetch` again and start a second VM beside the first.
      if (engine) {
        post({ type: 'error', message: 'the engine is already started (one init per worker)' });
        return;
      }
      const t0 = performance.now();
      if (msg.wasm) serveEmbeddedWasm(scope, msg.wasm);
      engine = WebEngine.create({
        factory: options.factory ? options.factory() : new options.LuaFactory(WASM_URL),
        bundle: msg.bundle,
        host: msg.host,
      });
      try {
        await engine;
        post({ type: 'ready', ms: Math.round(performance.now() - t0) });
      } catch (err) {
        post({ type: 'error', message: String(err) });
      }
      return;
    }
    const handler = HANDLERS[msg.type];
    if (!handler) {
      post({ id: msg.id, ok: false, message: `unknown request: ${String(msg.type)}` });
      return;
    }
    if (!engine) {
      post({ id: msg.id, ok: false, message: 'the engine is not started (init first)' });
      return;
    }
    try {
      const e = await engine;
      const root = msg.args?.root ?? msg.args?.request?.root;
      if (!ROOTLESS.has(msg.type) && !projectRoot(root)) {
        throw new Error(`not the root of a project: ${JSON.stringify(root)}`);
      }
      const t0 = performance.now();
      const { value, transfer } = handler(e, msg.args ?? {});
      post({ id: msg.id, ok: true, value, ms: Math.round(performance.now() - t0) }, transfer);
    } catch (err) {
      post({ id: msg.id, ok: false, message: String(err instanceof Error ? err.message : err) });
    }
  };
}
