// The page's side of the worker (worker.js): one promise per request, matched by id.

/**
 * @typedef {{ postMessage(message: any, transfer?: Transferable[]): void, onmessage: ((event: { data: any }) => void) | null,
 *   onerror?: ((event: any) => void) | null, terminate?(): void }} WorkerLike
 */

export class WorkerClient {
  /** @param {WorkerLike} worker */
  constructor(worker) {
    this.worker = worker;
    this.nextId = 1;
    /** @type {Map<number, { resolve(value: any): void, reject(error: Error): void }>} */
    this.waiting = new Map();
    /** @type {{ resolve(ms: number): void, reject(error: Error): void } | null} */
    this.starting = null;
    this.closed = false;
    worker.onmessage = (event) => this.receive(event.data);
    worker.onerror = (event) => this.fail(new Error(`the worker failed: ${event?.message ?? 'unknown error'}`));
  }

  /** @param {any} msg */
  receive(msg) {
    if (msg.type === 'ready') {
      this.starting?.resolve(msg.ms);
      this.starting = null;
      return;
    }
    if (msg.type === 'error') {
      this.starting?.reject(new Error(msg.message));
      this.starting = null;
      return;
    }
    const waiting = this.waiting.get(msg.id);
    if (!waiting) return;
    this.waiting.delete(msg.id);
    if (msg.ok) waiting.resolve(msg.value);
    else waiting.reject(new Error(msg.message));
  }

  /** @param {Error} error */
  fail(error) {
    this.starting?.reject(error);
    this.starting = null;
    for (const waiting of this.waiting.values()) waiting.reject(error);
    this.waiting.clear();
  }

  /**
   * Start the engine. Resolves with the milliseconds it took.
   * @param {{ wasm?: ArrayBuffer, bundle: object, host: string }} parts
   * @returns {Promise<number>}
   */
  init(parts) {
    return new Promise((resolve, reject) => {
      this.starting = { resolve, reject };
      this.worker.postMessage({ type: 'init', ...parts }, parts.wasm ? [parts.wasm] : []);
    });
  }

  /**
   * @param {string} type
   * @param {object} [args]
   * @param {Transferable[]} [transfer]
   * @returns {Promise<any>}
   */
  ask(type, args = {}, transfer = []) {
    if (this.closed) return Promise.reject(new Error('the engine is closed'));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      this.worker.postMessage({ type, id, args }, transfer);
    });
  }

  close() {
    this.closed = true;
    this.fail(new Error('the engine is closed'));
    this.worker.terminate?.();
  }
}
