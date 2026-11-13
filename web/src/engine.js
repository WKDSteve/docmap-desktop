// The docmap.nvim engine in a Lua VM (wasmoon, Lua 5.4) for the web variant: what the sidecar of the desktop app
// answers (`docmap <root>`, `--api=<route>`, `--capabilities`), without a program and without a server.
//
// Nothing here knows where it runs. The caller hands in the VM factory (in the browser one whose WASM comes from the
// page itself, see worker.js; in a Node test the stock one), the Lua sources (engine-bundle.mjs) and the host
// (lua/host.lua), and fills the file system inside the VM with the folder of a project. The engine only ever sees
// that file system: a path is a path inside the VM, never one of the machine.
//
// The pattern is the one of tasks-desktop's ui/src/web/engine.ts (the pilot of the web variants).

/**
 * @typedef {{ engine: { commit: string, lib_commit: string }, modules: Record<string, string> }} EngineBundle
 * @typedef {{ stream: 'out' | 'err', text: string }} LogLine
 * @typedef {{ status: 'ok' | 'exit' | 'error', payload: string, log: LogLine[] }} Answer
 * @typedef {{
 *   stat(path: string): { mode: number, size: number, mtime: Date, ino: number, dev: number },
 *   lstat(path: string): { mode: number, size: number, mtime: Date, ino: number, dev: number },
 *   readdir(path: string): string[],
 *   readlink(path: string): string,
 *   mkdir(path: string): void,
 *   rmdir(path: string): void,
 *   unlink(path: string): void,
 *   chdir(path: string): void,
 *   cwd(): string,
 *   writeFile(path: string, data: string | Uint8Array): void,
 *   readFile(path: string): Uint8Array,
 * }} MemFs
 */

const S_IFMT = 0o170000;
/** @param {number} mode */
export const isDir = (mode) => (mode & S_IFMT) === 0o040000;
/** @param {number} mode */
export const isFile = (mode) => (mode & S_IFMT) === 0o100000;
/** @param {number} mode */
const isLink = (mode) => (mode & S_IFMT) === 0o120000;

/**
 * @param {MemFs} fs
 * @param {string} path
 */
export function mkdirTree(fs, path) {
  let at = '';
  for (const part of path.split('/').filter(Boolean)) {
    at += `/${part}`;
    try {
      fs.mkdir(at);
    } catch {
      if (!isDir(fs.stat(at).mode)) throw new Error(`not a folder: ${at}`);
    }
  }
}

/**
 * @param {MemFs} fs
 * @param {string} path
 */
export function removeTree(fs, path) {
  for (const name of fs.readdir(path)) {
    if (name === '.' || name === '..') continue;
    const child = `${path}/${name}`;
    if (isDir(fs.lstat(child).mode)) removeTree(fs, child);
    else fs.unlink(child);
  }
  fs.rmdir(path);
}

const encoder = new TextEncoder();

/**
 * One answer of a door of host.lua: `<status> <bytes of the payload>\n<payload><log>`. wasmoon hands a Lua string
 * over as text decoded from UTF-8, so the length (bytes in Lua) is counted in the encoded form.
 * @param {string} raw
 * @returns {Answer}
 */
export function parseAnswer(raw) {
  const nl = raw.indexOf('\n');
  const head = /^(ok|exit|error) (\d+)$/.exec(raw.slice(0, nl));
  if (nl < 0 || !head) throw new Error(`the engine answered without a head: ${raw.slice(0, 80)}`);
  const bytes = encoder.encode(raw.slice(nl + 1));
  const size = Number(head[2]);
  const decoder = new TextDecoder();
  const payload = decoder.decode(bytes.subarray(0, size));
  /** @type {LogLine[]} */
  const log = [];
  // The pieces of output, each `<o|e><bytes>:<text>` (host.lua, `capture`).
  let at = size;
  while (at < bytes.length) {
    const colon = bytes.indexOf(0x3a, at);
    const length = colon < 0 ? NaN : Number(decoder.decode(bytes.subarray(at + 1, colon)));
    if (!Number.isInteger(length) || length < 0 || colon + 1 + length > bytes.length) {
      throw new Error('the engine answered with a log this page cannot read');
    }
    log.push({
      stream: bytes[at] === 0x65 ? 'err' : 'out',
      text: decoder.decode(bytes.subarray(colon + 1, colon + 1 + length)),
    });
    at = colon + 1 + length;
  }
  return { status: /** @type {Answer['status']} */ (head[1]), payload, log };
}

export class WebEngine {
  /**
   * @param {any} lua
   * @param {MemFs} fs
   * @param {EngineBundle} bundle
   */
  constructor(lua, fs, bundle) {
    this.lua = lua;
    /** @type {MemFs} */
    this.fs = fs;
    this.bundle = bundle;
    // Taken once: every `global.get` makes a new JS wrapper with its own reference into the Lua registry.
    /** @type {Record<string, (request: string) => string>} */
    this.doors = {
      generate: lua.global.get('__web_generate'),
      api: lua.global.get('__web_api'),
      capabilities: lua.global.get('__web_capabilities'),
    };
  }

  /**
   * @param {{
   *   factory: { getLuaModule(): Promise<any>, createEngine(options: object): Promise<any> },
   *   bundle: EngineBundle,
   *   host: string,
   *   env?: Record<string, string>,
   * }} opts
   */
  static async create(opts) {
    const wasm = await opts.factory.getLuaModule();
    /** @type {MemFs} */
    const fs = wasm.module.FS;
    const lua = await opts.factory.createEngine({ openStandardLibs: true, injectObjects: false, enableProxy: false });
    /** @type {Record<string, string>} */
    const env = { HOME: '/home/web', ...(opts.env ?? {}) };
    mkdirTree(fs, env.HOME);
    const names = Object.keys(opts.bundle.modules).sort().join('\n');
    /** @param {string} path @param {boolean} follow */
    const statOf = (path, follow) => {
      try {
        const st = follow === false ? fs.lstat(path) : fs.stat(path);
        const kind = isDir(st.mode) ? 'directory' : isFile(st.mode) ? 'file' : isLink(st.mode) ? 'link' : 'other';
        return `${kind} ${st.size} ${Math.floor(st.mtime.getTime() / 1000)} ${st.ino} ${st.dev}`;
      } catch {
        return undefined;
      }
    };
    // Every answer to Lua is a string, a boolean or undefined (nil): numbers and tables would depend on how wasmoon
    // converts them, the host parses what it needs. Never null: wasmoon looks for a `.then` on every object it gets
    // back, and `typeof null` is 'object'.
    lua.global.set('__web', {
      module_names: () => names,
      source: (/** @type {string} */ name) => opts.bundle.modules[name],
      env: (/** @type {string} */ name) => env[name],
      stat: statOf,
      // Length-prefixed: a name may hold a newline on the system the folder comes from. The length is in bytes, as
      // Lua counts.
      readdir: (/** @type {string} */ path) => {
        try {
          return fs
            .readdir(path)
            .filter((n) => n !== '.' && n !== '..')
            .sort()
            .map((n) => `${encoder.encode(n).length}:${n}`)
            .join('');
        } catch {
          return undefined;
        }
      },
      readlink: (/** @type {string} */ path) => {
        try {
          return fs.readlink(path);
        } catch {
          return undefined;
        }
      },
      mkdir: (/** @type {string} */ path) => {
        try {
          fs.mkdir(path);
          return true;
        } catch {
          return false;
        }
      },
      chdir: (/** @type {string} */ path) => {
        try {
          fs.chdir(path);
          return true;
        } catch {
          return false;
        }
      },
      cwd: () => fs.cwd(),
    });
    lua.doStringSync(opts.host);
    return new WebEngine(lua, fs, opts.bundle);
  }

  /**
   * Put files into the VM's file system: path inside the VM -> content. Folders are made as needed.
   * @param {Record<string, string | Uint8Array>} files
   */
  mount(files) {
    for (const [path, content] of Object.entries(files)) {
      const dir = path.slice(0, path.lastIndexOf('/'));
      if (dir) mkdirTree(this.fs, dir);
      this.fs.writeFile(path, content);
    }
  }

  /**
   * `docmap <root> [flags]`: writes the map into the project's out_dir inside the VM.
   * @param {{ root: string, source?: string[], exclude?: string[], languages?: string[], repo_url?: string,
   *   branch?: string, out_dir?: string, argv?: string[] }} request
   * @returns {Answer & { code: number | null }} `code` is the exit code of the run (0: written), null when it broke.
   */
  generate(request) {
    const answer = parseAnswer(String(this.doors.generate(JSON.stringify(request))));
    const code = answer.status === 'error' ? null : Number(answer.payload);
    return { ...answer, code: Number.isInteger(code) ? code : null };
  }

  /**
   * `docmap <root> --api=<route>`: the JSON of one route, as the sidecar prints it.
   * @param {{ root: string, route: string, snapshot?: string, out_dir?: string }} request
   * @returns {Answer}
   */
  api(request) {
    return parseAnswer(String(this.doors.api(JSON.stringify(request))));
  }

  /** `docmap --capabilities`. @returns {Answer} */
  capabilities() {
    return parseAnswer(String(this.doors.capabilities('')));
  }

  close() {
    this.lua.global.close();
  }
}
