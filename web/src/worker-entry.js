// The entry of the worker's script: the VM loader and the worker. Bundled into one classic script (build.mjs).

import { LuaFactory } from 'wasmoon';
import { startWorker } from './worker.js';

startWorker(/** @type {any} */ (self), { LuaFactory });
