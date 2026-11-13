// A folder as the File System Access API hands it out, for the tests in Node: `values()`, `getFile()`, the permission,
// and a way to change it as another program would.

/**
 * @param {string} name
 * @param {Record<string, string | Uint8Array>} files path -> content
 * @param {{ permission?: string, start?: number }} [options]
 */
export function fakeFolder(name, files, options = {}) {
  const enc = new TextEncoder();
  let clock = options.start ?? 1_700_000_000_000;
  const tick = () => (clock += 5000);
  const state = { permission: options.permission ?? 'granted', asked: 0, gone: false, reads: 0 };
  const root = { kind: 'directory', name, children: new Map() };
  const dirOf = (segments, create) => {
    let at = root;
    for (const part of segments) {
      let next = at.children.get(part);
      if (!next) {
        if (!create) return null;
        next = { kind: 'directory', name: part, children: new Map() };
        at.children.set(part, next);
      }
      at = next;
    }
    return at;
  };
  const put = (path, content) => {
    const parts = path.split('/');
    const leaf = parts.pop();
    const bytes = typeof content === 'string' ? enc.encode(content) : content;
    dirOf(parts, true).children.set(leaf, { kind: 'file', name: leaf, bytes, time: tick() });
  };
  for (const [path, content] of Object.entries(files)) put(path, content);

  const fileHandle = (node) => ({
    kind: 'file',
    name: node.name,
    getFile: async () => {
      if (node.unreadable) throw new Error('unreadable');
      state.reads += 1;
      return { size: node.size ?? node.bytes.length, lastModified: node.time, arrayBuffer: async () => node.bytes.slice().buffer };
    },
  });
  const dirHandle = (node) => ({
    kind: 'directory',
    name: node.name,
    __node: node,
    queryPermission: async () => {
      if (state.gone) throw Object.assign(new Error('gone'), { name: 'NotFoundError' });
      return state.permission;
    },
    requestPermission: async () => {
      state.asked += 1;
      if (state.permission === 'prompt') state.permission = 'granted';
      return state.permission;
    },
    isSameEntry: async (other) => other?.__node === node,
    async *values() {
      if (state.permission !== 'granted') throw Object.assign(new Error('no permission'), { name: 'NotAllowedError' });
      for (const child of [...node.children.values()]) yield child.kind === 'file' ? fileHandle(child) : dirHandle(child);
    },
    getDirectoryHandle: async (child) => {
      const found = node.children.get(child);
      if (!found || found.kind !== 'directory') throw Object.assign(new Error(child), { name: 'NotFoundError' });
      return dirHandle(found);
    },
    getFileHandle: async (child) => {
      const found = node.children.get(child);
      if (!found || found.kind !== 'file') throw Object.assign(new Error(child), { name: 'NotFoundError' });
      return fileHandle(found);
    },
  });
  return {
    handle: dirHandle(root),
    state,
    /** Another program writes a file (new content, newer time). */
    write: put,
    remove: (path) => {
      const parts = path.split('/');
      const leaf = parts.pop();
      dirOf(parts, false)?.children.delete(leaf);
    },
    /** A raw entry, for the cases a real folder cannot be made to hold on every system (odd names, sizes). */
    raw: (path, node) => {
      const parts = path.split('/');
      parts.pop();
      dirOf(parts, true).children.set(node.name, node);
    },
  };
}
