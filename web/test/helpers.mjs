// What the Node tests share: the engine of the pinned commits in the stock VM, and a folder of the disk read into it.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { LuaFactory } from 'wasmoon';
import { bundle } from '../engine-bundle.mjs';
import { WebEngine } from '../src/engine.js';

export const here = dirname(fileURLToPath(import.meta.url));
export const web = join(here, '..');

let made = null;
/** The bundle of the pinned commits, built once per process. */
export function engineBundle() {
  made ??= bundle();
  return made;
}

/** A new engine in a VM of its own. */
export async function newEngine(options = {}) {
  return WebEngine.create({
    factory: new LuaFactory(),
    bundle: engineBundle(),
    host: readFileSync(join(web, 'src', 'lua', 'host.lua'), 'utf8'),
    ...options,
  });
}

const SKIPPED = new Set(['.git', 'node_modules', 'target']);

/** The files under `dir` as `<root>/<relative path>` -> bytes, without the folders no map reads. */
export function readTree(dir, root, out = {}) {
  for (const name of readdirSync(dir).sort()) {
    if (SKIPPED.has(name)) continue;
    const path = join(dir, name);
    const st = statSync(path);
    if (st.isDirectory()) readTree(path, `${root}/${name}`, out);
    else if (st.isFile()) out[`${root}/${name}`] = readFileSync(path);
  }
  return out;
}

/**
 * The worker of the page in this process: the same `startWorker`, the messages copied as between a page and a worker
 * (a transferred buffer is gone on the sending side).
 */
export async function inProcessWorker() {
  const { startWorker } = await import('../src/worker.js');
  const worker = { onmessage: null, onerror: null, postMessage: null, terminate() {} };
  const scope = {
    fetch: globalThis.fetch,
    onmessage: null,
    postMessage: (message, transfer = []) => {
      const data = structuredClone(message, { transfer });
      queueMicrotask(() => worker.onmessage?.({ data }));
    },
  };
  worker.postMessage = (message, transfer = []) => {
    const data = structuredClone(message, { transfer });
    queueMicrotask(() => scope.onmessage?.({ data }));
  };
  startWorker(scope, { factory: () => new LuaFactory() });
  return worker;
}

/** A client on an engine that is started, with the bundle of the pinned commits. */
export async function startedClient() {
  const { WorkerClient } = await import('../src/client.js');
  const client = new WorkerClient(await inProcessWorker());
  await client.init({ bundle: engineBundle(), host: readFileSync(join(web, 'src', 'lua', 'host.lua'), 'utf8') });
  return client;
}

export const FIXTURE = join(here, 'fixtures', 'sample');

/** The fixture project: path relative to its root -> bytes. */
export function fixtureFiles() {
  const tree = readTree(FIXTURE, '');
  return Object.fromEntries(Object.entries(tree).map(([k, v]) => [k.slice(1), new Uint8Array(v)]));
}
