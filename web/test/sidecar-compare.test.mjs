// The golden comparison with the sidecar: the engine in the VM writes the same bytes as the binary the desktop app runs.
//
//   DOCMAP_WEB_SIDECAR=<path to docmap-x86_64-pc-windows-msvc.exe> node --test test/sidecar-compare.test.mjs
//
// The sidecar is a rolling release (`standalone-latest`), so it is not the commit of engine.lock. What is compared is
// therefore the host, not the pin: the Lua sources are bundled at the commit the binary says it was built from
// (`--capabilities`, `build.commit`; the checkout of docmap.nvim must have it), and both map the same folder. The
// binary runs without grammars (no DOCMAP_TS_DIR), which is the state of the page: no parser. Without the variable the
// test is skipped (a machine without the binary); CI sets it in the job that downloads the engine.

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { LuaFactory } from 'wasmoon';
import { bundle, checkout, readLock } from '../engine-bundle.mjs';
import { WebEngine } from '../src/engine.js';
import { FIXTURE, fixtureFiles, web } from './helpers.mjs';

const SIDECAR = process.env.DOCMAP_WEB_SIDECAR;
const ROOT = '/w/p1/sample';

/** Run the binary without a parser, as the page is. */
function sidecar(args, cwd) {
  const env = { ...process.env };
  for (const name of Object.keys(env)) if (name.startsWith('DOCMAP_TS_')) delete env[name];
  return execFileSync(SIDECAR, args, { cwd, env, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] });
}

test('the engine in the VM writes what the sidecar writes, byte for byte', { skip: SIDECAR ? false : 'DOCMAP_WEB_SIDECAR is not set' }, async () => {
  const caps = JSON.parse(sidecar(['--capabilities'], tmpdir()));
  const short = caps.build?.commit;
  assert.match(String(short), /^[0-9a-f]{7,40}$/, 'the binary names the commit it was built from');
  const engineDir = checkout('DOCMAP_ENGINE_DIR', 'docmap.nvim');
  const commit = execFileSync('git', ['-C', engineDir, 'rev-parse', `${short}^{commit}`], { encoding: 'utf8' }).trim();
  const lock = { ...readLock(), commit };

  const engine = await WebEngine.create({
    factory: new LuaFactory(),
    bundle: bundle({ lock }),
    host: readFileSync(join(web, 'src', 'lua', 'host.lua'), 'utf8'),
  });
  // What the two say about themselves: the same schema, the same routes, the same languages.
  const own = JSON.parse(engine.capabilities().payload);
  assert.equal(own.schema, caps.schema);
  assert.deepEqual(own.routes, caps.routes);
  assert.deepEqual(own.languages.map((l) => l.name), caps.languages.map((l) => l.name));

  const work = mkdtempSync(join(tmpdir(), 'docmap-web-compare-'));
  try {
    // The folder has the name it has in the VM: the title of a map is the name of its folder.
    const dir = join(work, 'sample');
    cpSync(FIXTURE, dir, { recursive: true });
    sidecar([dir.replaceAll('\\', '/'), '--lenient'], work);

    engine.mount(Object.fromEntries(Object.entries(fixtureFiles()).map(([rel, bytes]) => [`${ROOT}/${rel}`, bytes])));
    const run = engine.generate({ root: ROOT, argv: ['--lenient'] });
    assert.equal(run.code, 0, run.payload);

    for (const file of ['module_map.json', 'overview.md', 'index.html']) {
      const native = readFileSync(join(dir, 'docs', 'map', file));
      const inVm = Buffer.from(engine.fs.readFile(`${ROOT}/docs/map/${file}`));
      // Compared as text first: a difference then shows as lines, not as two megabytes of numbers.
      if (!native.equals(inVm)) assert.equal(inVm.toString('utf8'), native.toString('utf8'), `${file} differs from the sidecar's`);
    }

    // The routes: what reads the folder answers the same; what needs git is unavailable on both sides (the binary
    // asks a git that finds no repository, the page has no git to ask).
    for (const route of caps.routes) {
      // The routes of the rules (a newer engine has them) are answered by rules.nvim, which the binary carries and
      // the page does not: rule runs are not part of the web variant. There the page must say "not available".
      if (route.startsWith('rules/')) {
        const rules = engine.api({ root: ROOT, route });
        if (rules.status === 'ok') assert.notEqual(JSON.parse(rules.payload).available, true, `${route} has no engine in the page`);
        continue;
      }
      let native;
      try {
        native = JSON.parse(sidecar([dir.replaceAll('\\', '/'), `--api=${route}`], work));
      } catch {
        // A route that takes a parameter, or one the binary refuses for a folder without git: nothing to compare.
        continue;
      }
      const answer = engine.api({ root: ROOT, route });
      if (answer.status !== 'ok') continue;
      const inVm = JSON.parse(answer.payload);
      assert.equal(inVm.available, native.available, `${route}: available`);
      if (native.available !== false) assert.deepEqual(inVm, native, route);
    }
  } finally {
    rmSync(work, { recursive: true, force: true });
    engine.close();
  }
});
