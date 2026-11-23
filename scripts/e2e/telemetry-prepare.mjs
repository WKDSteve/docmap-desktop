// scripts/e2e/telemetry-prepare.mjs -- the `prepare` step of the app `window-telemetry` (.testing.lua): the config
// folder that knows the fixture project (telemetry-fixture.mjs) and the telemetry store of the sheet, both in the run's
// own folder.
//
//   node scripts/e2e/telemetry-prepare.mjs <run dir>
//
// `<run dir>/config/workspace.json` lists the project (open-at-config.mjs writes the same shape for another fixture);
// `<run dir>/local/nvim-data/runtime-analysis.nvim/...` is the store, written by a headless Neovim with the real
// runtime-analysis.nvim (telemetry-store.lua) with LOCALAPPDATA moved into the run dir, which is where the engine of
// the window looks (its `stdpath("state")`). Needs Neovim on the PATH and the checkouts of lib.nvim and
// runtime-analysis.nvim (DOCMAP_SHEET_RTP, or $REPOS_DIR).
//
// Exit 0: written. Exit 1: no run dir, no fixture project, or Neovim failed (what it said is printed).
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, realpathSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const fail = (why) => {
  console.error(`telemetry-prepare: ${why}`);
  process.exit(1);
};

const run = process.argv[2];
if (!run) fail('usage: node scripts/e2e/telemetry-prepare.mjs <run dir>');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const project = join(root, 'src-tauri', 'target', 'sheet-fixtures', 'telemetry', 'project');
if (!existsSync(join(project, 'docs', 'map', 'module_map.json'))) {
  fail(`no fixture project at ${project}: run scripts/e2e/telemetry-fixture.mjs first`);
}

const canonical = realpathSync.native(project).replace(/^\\\\\?\\/, '').replace(/\\/g, '/');
mkdirSync(join(run, 'config'), { recursive: true });
writeFileSync(
  join(run, 'config', 'workspace.json'),
  JSON.stringify(
    { projects: [{ id: canonical, name: 'project', root: canonical, map_dir: `${canonical}/docs/map` }] },
    null,
    2
  ) + '\n'
);

const local = join(run, 'local');
mkdirSync(local, { recursive: true });
const r = spawnSync(
  'nvim',
  ['--headless', '-u', 'NONE', '-l', join(root, 'scripts', 'e2e', 'telemetry-store.lua'), run],
  { cwd: root, encoding: 'utf8', env: { ...process.env, LOCALAPPDATA: local }, timeout: 120_000 }
);
if (r.error || r.status !== 0) fail(`nvim: ${r.error ?? ''}${r.stdout ?? ''}${r.stderr ?? ''}`);
console.log(`telemetry-prepare: ${canonical}; ${r.stdout.trim()}`);
