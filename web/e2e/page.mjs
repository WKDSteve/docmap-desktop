// What the browser tests of the web variant share: the built file, the fixture project as a fake folder, and the watch
// on what the page must never do (a request, a violation of its policy; the kit's, appkit-web/e2e).

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { violations, watchPage as watchKitPage } from 'appkit-web/e2e';

const here = dirname(fileURLToPath(import.meta.url));
export const FILE = resolve(here, '..', 'dist', 'docmap.html');
export const FIXTURE = resolve(here, '..', 'test', 'fixtures', 'sample');

export function pageUrl() {
  if (!existsSync(FILE)) throw new Error(`${FILE} is not there: run "pnpm build" first`);
  return pathToFileURL(FILE).href;
}

/** The files of a folder of the disk: path relative to it -> text. */
export function filesOf(dir = FIXTURE) {
  /** @type {Record<string, string>} */
  const files = {};
  const walk = (/** @type {string} */ at) => {
    for (const name of readdirSync(at)) {
      const path = join(at, name);
      if (statSync(path).isDirectory()) walk(path);
      else files[relative(dir, path).replaceAll('\\', '/')] = readFileSync(path, 'utf8');
    }
  };
  walk(dir);
  return files;
}

/** Watch the page from now on (appkit-web/e2e); the page itself is `docmap.html`. */
export function watchPage(page) {
  return watchKitPage(page, { pageUrl: pageUrl() });
}

export { violations };

/**
 * A `showDirectoryPicker` that hands out folders made of `folders` (name of a folder -> its files, path -> text), read
 * only like the page asks for them: `values()` yields file and folder handles, `getFile()` gives size, time and bytes,
 * `getDirectoryHandle`/`getFileHandle` find an entry. The picker gives the folder named in `window.__pick` (the first
 * one by default).
 *
 * The test is the other program that works in a folder: `window.__disk.write(folder, path, text)` changes a file as an
 * editor would (new text, a time a minute from now), `__disk.permission(folder, state)` is the browser forgetting or keeping the
 * permission (it holds across a reload of the page), and `__disk.asked` counts how often the page asked for one.
 *
 * A real handle comes back from IndexedDB as a handle of the same folder. The structured clone of this fake keeps only
 * its data (`kind`, `name`, `path`), so what the page reads from IndexedDB is made a handle of the folder at that
 * `path` again.
 */
export async function fakeFolders(page, folders) {
  await page.addInitScript((tree) => {
    // Only the page itself has a picker; the frame of a map (an init script runs there too) has no storage to ask.
    if (window.top !== window) return;
    const enc = new TextEncoder();
    const root = { kind: 'directory', name: '', children: new Map() };
    let clock = 1_700_000_000_000;
    const tick = () => (clock += 5000);
    const domError = (name, message) => Object.assign(new Error(message), { name });
    const walkTo = (segments, create) => {
      let at = root;
      for (const part of segments) {
        let next = at.children.get(part);
        if (!next) {
          if (!create) return null;
          next = { kind: 'directory', name: part, children: new Map() };
          at.children.set(part, next);
        }
        if (next.kind !== 'directory') return null;
        at = next;
      }
      return at;
    };
    const put = (folder, path, text, time = tick()) => {
      const parts = [folder, ...path.split('/')];
      const dir = walkTo(parts.slice(0, -1), true);
      const name = parts[parts.length - 1];
      dir.children.set(name, { kind: 'file', name, bytes: enc.encode(text), time });
    };
    // The files a folder starts with are old (2023): older than any map the page makes of them.
    for (const [folder, files] of Object.entries(tree)) {
      walkTo([folder], true);
      for (const [path, text] of Object.entries(files)) put(folder, path, text);
    }

    // What the browser remembers of the permissions across a reload: a test sets it with `__disk.permission`, and the
    // next load of the page starts from it (the order of init scripts is not defined, so a second script could not).
    const permissions = new Map(Object.entries(JSON.parse(sessionStorage.getItem('__fakePermissions') ?? '{}')));
    const disk = { asked: 0, picked: 0 };
    const topOf = (path) => path.split('/')[0];
    const fileHandle = (parent, node) => ({
      kind: 'file',
      name: node.name,
      getFile: async () => {
        const now = parent.children.get(node.name);
        if (!now || now.kind !== 'file') throw domError('NotFoundError', 'gone');
        const bytes = now.bytes;
        return { size: bytes.byteLength, lastModified: now.time, arrayBuffer: async () => bytes.slice().buffer };
      },
    });
    // Only `kind`, `name` and `path` are data, the rest is not enumerable: like a real handle, this one can be put into
    // IndexedDB (structured clone copies the enumerable data and nothing else; a function among them would throw).
    const dirHandle = (node, path) => {
      const members = {
        __node: node,
        queryPermission: async () => (walkTo(path.split('/'), false) ? (permissions.get(topOf(path)) ?? 'granted') : 'prompt'),
        requestPermission: async () => {
          // As in Chrome: the question needs a click of the person right before it.
          if (!navigator.userActivation?.isActive) throw domError('SecurityError', 'User activation is required to request permissions.');
          disk.asked += 1;
          if (permissions.get(topOf(path)) !== 'denied') window.__disk.permission(topOf(path), 'granted');
          return permissions.get(topOf(path)) ?? 'granted';
        },
        isSameEntry: async (other) => other?.__node === node,
        async *values() {
          if ((permissions.get(topOf(path)) ?? 'granted') !== 'granted') throw domError('NotAllowedError', 'no permission');
          if (!walkTo(path.split('/'), false)) throw domError('NotFoundError', 'gone');
          for (const child of [...node.children.values()]) {
            yield child.kind === 'file' ? fileHandle(node, child) : dirHandle(child, `${path}/${child.name}`);
          }
        },
        getFileHandle: async (name) => {
          const child = node.children.get(name);
          if (!child) throw domError('NotFoundError', name);
          if (child.kind !== 'file') throw domError('TypeMismatchError', name);
          return fileHandle(node, child);
        },
        getDirectoryHandle: async (name) => {
          const child = node.children.get(name);
          if (!child) throw domError('NotFoundError', name);
          if (child.kind !== 'directory') throw domError('TypeMismatchError', name);
          return dirHandle(child, `${path}/${name}`);
        },
      };
      return Object.defineProperties(
        { kind: 'directory', name: node.name, path },
        Object.fromEntries(Object.entries(members).map(([key, value]) => [key, { value }])),
      );
    };
    const handleAt = (path) => {
      const node = walkTo(path.split('/'), false);
      return node ? dirHandle(node, path) : null;
    };

    window.__disk = Object.assign(disk, {
      // A file written now is a minute newer than anything the page did so far (its map included).
      write: (folder, path, text) => put(folder, path, text, Date.now() + 60_000),
      remove: (folder, path) => {
        const parts = [folder, ...path.split('/')];
        walkTo(parts.slice(0, -1), false)?.children.delete(parts[parts.length - 1]);
      },
      removeFolder: (folder) => root.children.delete(folder),
      permission: (folder, state) => {
        permissions.set(folder, state);
        sessionStorage.setItem('__fakePermissions', JSON.stringify(Object.fromEntries(permissions)));
      },
    });
    window.showDirectoryPicker = async () => {
      disk.picked += 1;
      const name = window.__pick ?? [...root.children.keys()][0];
      const handle = handleAt(name);
      if (!handle) throw domError('AbortError', 'nothing to pick');
      return handle;
    };

    // What the page reads back from IndexedDB is plain data where a handle was: made a handle again, by its path.
    const revive = (value, depth = 0) => {
      if (!value || typeof value !== 'object' || depth > 8) return value;
      if (value.kind === 'directory' && typeof value.path === 'string' && typeof value.values !== 'function') {
        return handleAt(value.path) ?? dirHandle({ kind: 'directory', name: value.name, children: new Map() }, value.path);
      }
      if (Array.isArray(value)) return value.map((v) => revive(v, depth + 1));
      if (Object.getPrototypeOf(value) === Object.prototype) {
        return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, revive(v, depth + 1)]));
      }
      return value;
    };
    const original = Object.getOwnPropertyDescriptor(IDBRequest.prototype, 'result');
    Object.defineProperty(IDBRequest.prototype, 'result', {
      configurable: true,
      get() {
        const value = original.get.call(this);
        const source = this.source;
        if (!source || !source.transaction || source.transaction.db.name !== 'docmap-web') return value;
        return revive(value);
      },
    });
  }, folders);
}

/** Open the page with the fixture project as the folder `name` and add it as a project. */
export async function openProject(page, { name = 'sample', files = filesOf() } = {}) {
  await fakeFolders(page, { [name]: files });
  await page.goto(pageUrl());
  await addProject(page, name);
}

/** Pick the folder `name` with the page's own button. */
export async function addProject(page, name) {
  await page.evaluate((n) => {
    window.__pick = n;
  }, name);
  await page.locator('#add').click();
}

/** The frame the map is shown in. */
export const mapFrame = (page) => page.frameLocator('#map');
