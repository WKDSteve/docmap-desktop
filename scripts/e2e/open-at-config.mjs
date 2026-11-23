// scripts/e2e/open-at-config.mjs -- the configuration folder for the sheets of `--open-at` (TESTS/sheets/open-at*.sheet.lua):
// a workspace that already knows the fixture project of map-history-fixture.mjs.
//
//   node scripts/e2e/open-at-config.mjs <config-dir>
//
// The window reads `--open-at` once, at its start, and resolves it against the projects it knows then: a project added
// afterwards from the page comes too late. So the project is in the workspace before the window starts (the sheet
// app's `prepare` step), not added by the sheet. The folder is the run's own (`${temp}/config`), never the user's.
//
// `workspace.json` carries the project list inline, the form older versions wrote; the window moves it into the
// workspace file of its own on the first read (`read_workspace`). The id and the root are what `add_project` stores:
// the canonical path with forward slashes and without the `\\?\` prefix (`portable`).
//
// Exit 0: written. Exit 1: no config dir given, or the fixture project is not built (run map-history-fixture.mjs).
import { existsSync, mkdirSync, realpathSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const fail = (why) => {
  console.error(`open-at-config: ${why}`);
  process.exit(1);
};

const dir = process.argv[2];
if (!dir) fail('usage: node scripts/e2e/open-at-config.mjs <config-dir>');

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const project = join(root, 'src-tauri', 'target', 'sheet-fixtures', 'map-history', 'project');
if (!existsSync(join(project, 'docs', 'map', 'module_map.json'))) {
  fail(`no fixture project at ${project}: run scripts/e2e/map-history-fixture.mjs first`);
}

const canonical = realpathSync.native(project).replace(/^\\\\\?\\/, '').replace(/\\/g, '/');
mkdirSync(dir, { recursive: true });
writeFileSync(
  join(dir, 'workspace.json'),
  JSON.stringify(
    { projects: [{ id: canonical, name: 'project', root: canonical, map_dir: `${canonical}/docs/map` }] },
    null,
    2
  ) + '\n'
);
console.log(`open-at-config: ${join(dir, 'workspace.json')} knows ${canonical}`);
