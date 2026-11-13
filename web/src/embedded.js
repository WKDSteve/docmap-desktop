// The parts the single-file page carries inside itself: the worker's script, the WASM of the Lua VM, the Lua sources of
// the engine and the host. Each is gzip-compressed and base64-encoded in a `<script type="application/octet-stream">`
// of the page (never fetched: the CSP forbids every request) and unpacked here. As in tasks-desktop's
// ui/src/web/embedded.ts.

/** The ids of the elements that hold the parts. */
export const PART_IDS = /** @type {const} */ (['worker', 'wasm', 'bundle', 'host']);

/** @type {((text: string) => Uint8Array) | null} `Uint8Array.fromBase64` where the browser has it, else null. */
const nativeFromBase64 = /** @type {any} */ (Uint8Array).fromBase64 ?? null;

/** base64 -> bytes through `atob`. @param {string} text */
export function base64ByAtob(text) {
  const binary = atob(text.replace(/\s+/g, ''));
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/**
 * base64 -> bytes: `Uint8Array.fromBase64` where there is one, else `atob`. Both skip the line breaks the packer puts
 * into the text.
 * @param {string} text
 * @param {((text: string) => Uint8Array) | null} [fromBase64] for a test: null takes the way of a browser without it
 */
export function base64Bytes(text, fromBase64 = nativeFromBase64) {
  return fromBase64 ? fromBase64(text) : base64ByAtob(text);
}

/** gzip -> bytes. @param {Uint8Array} bytes */
export async function gunzip(bytes) {
  const source = new ReadableStream({
    start(controller) {
      controller.enqueue(bytes);
      controller.close();
    },
  });
  return new Uint8Array(await new Response(source.pipeThrough(new DecompressionStream('gzip'))).arrayBuffer());
}

/** gzip + base64 -> bytes. @param {string} base64 */
export function unpack(base64) {
  return gunzip(/** @type {Uint8Array<ArrayBuffer>} */ (base64Bytes(base64)));
}

/**
 * Unpack the parts. `read(id)` gives the text of the element `id`, or null when it is not there. A missing part is an
 * error that names it: the page cannot start without any of them.
 * @param {(id: string) => string | null} read
 */
export async function loadParts(read) {
  /** @param {string} id */
  const raw = (id) => {
    const text = read(id);
    if (text === null || !/\S/.test(text)) throw new Error(`the page lacks its part "${id}"`);
    return text;
  };
  const decoder = new TextDecoder();
  // The text of each part is read (and a missing one named) before any of them is unpacked.
  const texts = PART_IDS.map((id) => /** @type {[string, string]} */ ([id, raw(id)]));
  const bytes = Object.fromEntries(await Promise.all(texts.map(async ([id, text]) => [id, await unpack(text)])));
  const parsed = JSON.parse(decoder.decode(bytes.bundle));
  if (!parsed || typeof parsed.modules !== 'object' || typeof parsed.engine?.commit !== 'string') {
    throw new Error('the part "bundle" is not an engine bundle');
  }
  return {
    worker: decoder.decode(bytes.worker),
    /** @type {ArrayBuffer} */
    wasm: bytes.wasm.buffer,
    bundle: /** @type {import('./engine.js').EngineBundle} */ (parsed),
    host: decoder.decode(bytes.host),
  };
}

/**
 * The worker, started from a blob. A classic one: a module worker from a blob does not start on a page opened from
 * disk. The address of the blob stays: the browser fetches the script after the constructor returns.
 * @param {string} source
 * @param {{ Blob: typeof Blob, URL: Pick<typeof URL, 'createObjectURL'>, Worker: new (url: string, options?: object) => Worker }} host
 */
export function startBlobWorker(source, host) {
  const url = host.URL.createObjectURL(new host.Blob([source], { type: 'text/javascript' }));
  return new host.Worker(url, { type: 'classic' });
}
