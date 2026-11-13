// Reads the folder of a project (a directory handle of the File System Access API) into memory: what the engine walks,
// and nothing else. Read only: the web variant never writes into a folder.
//
// The folders left out are the engine's own list (`VENDOR_DIRS` in docmap.nvim's core/scan.lua): what it never walks is
// never read, so a `node_modules` costs nothing. A name that comes from a folder is not trusted: one that could leave
// the root (a separator, a dot segment, a NUL) is never made a path. A file over the limit is left out and reported;
// a folder over the budget is refused as a whole, because half a project is a wrong map.

/**
 * @typedef {{ kind: 'file', name: string, getFile(): Promise<{ size: number, lastModified?: number,
 *   arrayBuffer(): Promise<ArrayBuffer> }> }} FileHandleLike
 * @typedef {{ kind: 'directory', name: string, values(): AsyncIterable<FileHandleLike | DirHandleLike> }} DirHandleLike
 * @typedef {{ path: string, reason: 'too-large' | 'unreadable' | 'name' | 'too-deep' }} Skipped
 * @typedef {{ files: Record<string, Uint8Array>, mtimes: Record<string, number>, skipped: Skipped[], bytes: number }} Loaded
 */

/** The folders the engine never walks (docmap.nvim, core/scan.lua, `VENDOR_DIRS`), at any depth. */
export const VENDOR_DIRS = new Set([
  'node_modules',
  'vendor',
  'bower_components',
  'target',
  'dist',
  'build',
  'out',
  '.next',
  '.nuxt',
  '.svelte-kit',
  '__pycache__',
  '.venv',
  'venv',
  '.git',
  '.deps',
  '.claude',
]);

export const LIMITS = Object.freeze({
  /** Files of one project. */
  files: 20_000,
  /** Bytes of one project. */
  bytes: 256 * 1024 * 1024,
  /** Bytes of one file; a larger one is left out and reported. */
  fileBytes: 4 * 1024 * 1024,
  /** Levels of folders below the root. */
  depth: 32,
});

export class FolderTooLarge extends Error {
  /** @param {'files' | 'bytes'} what @param {number} limit */
  constructor(what, limit) {
    super(`the folder holds more than ${limit} ${what}`);
    this.name = 'FolderTooLarge';
    this.what = what;
    this.limit = limit;
  }
}

/**
 * Whether `name` is one segment of a path and nothing else. Also refuses what Windows rewrites (a name ending in a dot
 * or a space names another file there).
 * @param {unknown} name
 */
export function safeName(name) {
  return (
    typeof name === 'string' &&
    name !== '' &&
    name !== '.' &&
    name !== '..' &&
    !/[/\\\0]/.test(name) &&
    !/[. ]$/.test(name)
  );
}

/**
 * The files below `dir`, as path relative to it -> bytes, with the time each was last changed (milliseconds).
 * @param {DirHandleLike} dir
 * @param {Partial<typeof LIMITS>} [limits]
 * @returns {Promise<Loaded>}
 */
export async function readFolder(dir, limits = {}) {
  const max = { ...LIMITS, ...limits };
  /** @type {Loaded} */
  const out = { files: {}, mtimes: {}, skipped: [], bytes: 0 };
  let count = 0;
  /** @param {DirHandleLike} at @param {string} prefix @param {number} depth */
  const walk = async (at, prefix, depth) => {
    /** @type {(FileHandleLike | DirHandleLike)[]} */
    const entries = [];
    for await (const entry of at.values()) entries.push(entry);
    // The order of a folder is the browser's; sorted, the same folder reads the same way every time.
    entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    for (const entry of entries) {
      const path = prefix + entry.name;
      if (!safeName(entry.name)) {
        out.skipped.push({ path: prefix + String(entry.name).replace(/[/\\\0]/g, '?'), reason: 'name' });
        continue;
      }
      if (entry.kind === 'directory') {
        if (VENDOR_DIRS.has(entry.name)) continue;
        if (depth >= max.depth) {
          out.skipped.push({ path, reason: 'too-deep' });
          continue;
        }
        await walk(entry, `${path}/`, depth + 1);
        continue;
      }
      if (entry.kind !== 'file') continue;
      let file;
      try {
        file = await entry.getFile();
      } catch {
        out.skipped.push({ path, reason: 'unreadable' });
        continue;
      }
      if (file.size > max.fileBytes) {
        out.skipped.push({ path, reason: 'too-large' });
        continue;
      }
      count += 1;
      if (count > max.files) throw new FolderTooLarge('files', max.files);
      let bytes;
      try {
        bytes = new Uint8Array(await file.arrayBuffer());
      } catch {
        out.skipped.push({ path, reason: 'unreadable' });
        count -= 1;
        continue;
      }
      // The size that counts is the one read: a file may have grown since its size was asked.
      out.bytes += bytes.length;
      if (out.bytes > max.bytes) throw new FolderTooLarge('bytes', max.bytes);
      out.files[path] = bytes;
      out.mtimes[path] = typeof file.lastModified === 'number' ? file.lastModified : 0;
    }
  };
  await walk(dir, '', 0);
  return out;
}

/**
 * What tells one state of the folder from another without reading a file: every path with its size and time. The
 * same walk as `readFolder`, so a change the engine would not see does not count.
 * @param {DirHandleLike} dir
 * @param {Partial<typeof LIMITS>} [limits]
 * @returns {Promise<string>}
 */
export async function folderSignature(dir, limits = {}) {
  const max = { ...LIMITS, ...limits };
  /** @type {string[]} */
  const lines = [];
  /** @param {DirHandleLike} at @param {string} prefix @param {number} depth */
  const walk = async (at, prefix, depth) => {
    for await (const entry of at.values()) {
      if (!safeName(entry.name)) continue;
      const path = prefix + entry.name;
      if (entry.kind === 'directory') {
        if (!VENDOR_DIRS.has(entry.name) && depth < max.depth) await walk(entry, `${path}/`, depth + 1);
      } else if (entry.kind === 'file') {
        if (lines.length >= max.files) return;
        try {
          const file = await entry.getFile();
          lines.push(`${path}\0${file.size}\0${file.lastModified ?? 0}`);
        } catch {
          lines.push(`${path}\0?`);
        }
      }
    }
  };
  await walk(dir, '', 0);
  return lines.sort().join('\n');
}

/**
 * The folders directly inside `dir` and whether each is a git checkout (has an entry `.git`): what the window lists
 * when the folder picked is not a repository itself. Hidden folders are left out, as in the desktop app.
 * @param {DirHandleLike & { getDirectoryHandle?: Function, getFileHandle?: Function }} dir
 * @returns {Promise<{ isGit: boolean, subfolders: { name: string, isGit: boolean, handle: DirHandleLike }[] }>}
 */
export async function inspectFolder(dir) {
  /** @param {any} at */
  const hasGit = async (at) => {
    for (const probe of ['getDirectoryHandle', 'getFileHandle']) {
      try {
        await at[probe]('.git');
        return true;
      } catch {
        // not there as this kind
      }
    }
    return false;
  };
  const subfolders = [];
  for await (const entry of dir.values()) {
    if (entry.kind !== 'directory' || !safeName(entry.name) || entry.name.startsWith('.')) continue;
    subfolders.push({ name: entry.name, isGit: await hasGit(entry), handle: entry });
  }
  subfolders.sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  return { isGit: await hasGit(dir), subfolders };
}
