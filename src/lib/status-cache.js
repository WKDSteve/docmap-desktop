// A tiny in-memory cache over the `map_status` command, keyed by map_dir.
//
// `render()` calls this for every project on every render, and `select()`
// calls it again for the one project it opens right after — without a
// cache that is repeated IPC + disk I/O (main.rs reads module_map.json) for
// data that only changes at two moments: a successful `generate`, or the
// project being removed. Unbounded is fine here (see PERFORMANCE.md's
// "Eviction pragmatisch wählen") — one entry per project in the workspace,
// realistically tens at most; a removed project just leaves an unused entry
// that is never read again.

const cache = new Map();

/** The in-flight or resolved `map_status` result for `mapDir`, fetched once. */
export function mapStatus(invoke, mapDir) {
  let entry = cache.get(mapDir);
  if (!entry) {
    entry = invoke("map_status", { mapDir }).catch((e) => {
      // Don't let a failed fetch poison retries — the next caller should
      // get a fresh attempt, not a cached rejection forever.
      cache.delete(mapDir);
      throw e;
    });
    cache.set(mapDir, entry);
  }
  return entry;
}

/** Call after anything that changes what `mapDir` contains on disk. */
export function invalidate(mapDir) {
  cache.delete(mapDir);
}
