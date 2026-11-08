// A small state store: the view state of the window in one place, with a way
// to react to a change instead of calling the renderers by hand after it.
//
// `state` is a plain-looking object: reading `state.selectedId` gives the
// value, assigning to it stores the value and tells the subscribers of that
// key, so the code that sets a value does not have to know who shows it. The
// store has no DOM and no timers, so what it promises is asserted in
// `store.test.js`.
//
// A subscriber is told only about a real change (`Object.is`), synchronously,
// after the value is stored (so it reads the new state), in the order it
// subscribed. A subscriber that throws does not stop the others; the first
// error is thrown once all have run, so a broken renderer is not silent.

/**
 * @template {Record<string, unknown>} S
 * @param {S} initial  the keys of the state; no key can be added later
 */
export function createStore(initial) {
  /** @type {Record<string, unknown>} */
  const values = { ...initial };
  /** @type {Map<string, Set<(value: any, old: any, key: string) => void>>} */
  const listeners = new Map();
  let depth = 0;
  /** @type {Map<string, [any, any]>} */
  let pending = new Map();

  function deliver(key, value, old) {
    let first = null;
    const run = (fn) => {
      try {
        fn();
      } catch (e) {
        if (first === null) first = { error: e };
      }
    };
    for (const fn of [...(listeners.get(key) ?? [])]) run(() => fn(value, old, key));
    if (first) throw first.error;
  }

  function changed(key, value, old) {
    if (depth > 0) {
      // Inside a batch: keep the first old value and the last new one.
      pending.set(key, [value, pending.has(key) ? pending.get(key)[1] : old]);
      return;
    }
    deliver(key, value, old);
  }

  /** @type {S} */
  const state = /** @type {any} */ ({});
  for (const key of Object.keys(initial)) {
    Object.defineProperty(state, key, {
      enumerable: true,
      get: () => values[key],
      set: (value) => {
        const old = values[key];
        if (Object.is(old, value)) return;
        values[key] = value;
        changed(key, value, old);
      },
    });
  }

  return {
    state,

    /** Assign several keys, then tell the subscribers (one call per changed key). */
    patch(partial) {
      this.batch(() => {
        for (const [key, value] of Object.entries(partial)) {
          if (!(key in values)) throw new Error(`unknown state key: ${key}`);
          state[key] = value;
        }
      });
    },

    /**
     * Run `fn`; the subscribers hear of the changes it made when it returns,
     * once per key, with the value before and after the whole batch (a key
     * that ends where it began is not reported).
     */
    batch(fn) {
      depth += 1;
      let failed = false;
      try {
        return fn();
      } catch (e) {
        failed = true;
        throw e;
      } finally {
        depth -= 1;
        if (depth === 0) {
          const done = pending;
          pending = new Map();
          let first = null;
          for (const [key, [value, old]] of done) {
            if (Object.is(value, old)) continue;
            try {
              deliver(key, value, old);
            } catch (e) {
              if (first === null) first = { error: e };
            }
          }
          // A throw here would replace the error of `fn`, which is the one that matters.
          if (first && !failed) throw first.error;
        }
      }
    },

    /**
     * Call `listener(value, old, key)` when `key` (or any of `keys`) changes.
     * Returns the function that ends the subscription.
     * @param {string|string[]} keys
     */
    subscribe(keys, listener) {
      const list = Array.isArray(keys) ? keys : [keys];
      for (const key of list) {
        if (!(key in values)) throw new Error(`unknown state key: ${key}`);
        if (!listeners.has(key)) listeners.set(key, new Set());
        listeners.get(key).add(listener);
      }
      return () => {
        for (const key of list) listeners.get(key)?.delete(listener);
      };
    },
  };
}
