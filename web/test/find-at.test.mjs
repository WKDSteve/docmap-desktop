// "Open the map at a file and line" is resolved twice: by `resolveAt` (src/lib/open-at.js, from module_map.json) in the
// window, and by `find.at` of docmap.nvim (`core/find.lua`, behind `gen_map.lua --at=` and `:DocMap here`). Two
// implementations of one rule: this holds both to the same cases (src/lib/open-at-cases.json, `at`), with the
// `find.lua` of the commit of engine.lock run in the Lua VM. Either side drifting turns this red.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { LuaFactory } from 'wasmoon';
import { checkout, gitFiles, readLock } from '../engine-bundle.mjs';
import { nodeFragment, resolveAt } from '../../src/lib/open-at.js';

const { at } = JSON.parse(readFileSync(new URL('../../src/lib/open-at-cases.json', import.meta.url), 'utf8'));

/** The map as the IR `find.at` reads: `order` and `nodes` by id, each with `source` and `functions`. */
function irLiteral(map) {
  const str = (s) => JSON.stringify(s); // a JSON string is a Lua string for these (ASCII) values
  const node = (n) =>
    `{id=${str(n.id)},source=${str(n.source)},functions={${(n.functions ?? [])
      .map((f) => `{name=${str(f.name)},line=${f.line}${f.line_end === undefined ? '' : `,line_end=${f.line_end}`}}`)
      .join(',')}}}`;
  return (
    `{order={${map.nodes.map((n) => str(n.id)).join(',')}},` +
    `nodes={${map.nodes.map((n) => `[${str(n.id)}]=${node(n)}`).join(',')}}}`
  );
}

/** What `gen_map.lua --at=` prints for a hit (`run_at` of core/cli.lua), or "none". */
const LINE = `
  local hit = find.at(ir, file, line, last)
  if not hit then return "none" end
  return table.concat({ hit.id, hit.key or "-", find.fragment(hit) }, "\\t")
`;

async function engineAnswers() {
  const lock = readLock();
  const dir = checkout('DOCMAP_ENGINE_DIR', 'docmap.nvim');
  const files = gitFiles(dir, lock.commit, ['lua/docmap/core/find.lua']);
  const source = files.get('lua/docmap/core/find.lua');
  assert.ok(source, `docmap.nvim ${lock.commit.slice(0, 12)} has no core/find.lua: engine.lock must name a commit with find.at`);
  const lua = await new LuaFactory().createEngine();
  try {
    lua.global.set('FIND_SRC', source.toString('utf8'));
    await lua.doString(`find = assert(load(FIND_SRC, "=core/find.lua"))(); ir = ${irLiteral(at.map)}`);
    const answers = [];
    for (const c of at.cases) {
      lua.global.set('file', c.file);
      lua.global.set('line', c.line);
      lua.global.set('last', c.end ?? undefined);
      answers.push(await lua.doString(`local find, ir, file, line, last = find, ir, file, line, last\n${LINE}`));
    }
    return answers;
  } finally {
    lua.global.close();
  }
}

/** The same line, from what the window computes. */
function windowAnswer(c) {
  const hit = resolveAt(at.map, c.file, c.line, c.end);
  return hit ? [hit.id, hit.key ?? '-', nodeFragment(hit)].join('\t') : 'none';
}

/** What the case says, as that line. */
function expected(c) {
  if (!c.want) return 'none';
  return [c.want.id, c.want.key ?? '-', nodeFragment(c.want)].join('\t');
}

test('find.at of the engine in engine.lock answers the cases of open-at-cases.json', async () => {
  const answers = await engineAnswers();
  at.cases.forEach((c, i) => assert.equal(answers[i], expected(c), `engine: ${JSON.stringify(c)}`));
});

test('resolveAt of the window answers the same cases, with the same fragment', () => {
  for (const c of at.cases) assert.equal(windowAnswer(c), expected(c), `window: ${JSON.stringify(c)}`);
});

test('the engine and the window agree on every case, line for line', async () => {
  const answers = await engineAnswers();
  at.cases.forEach((c, i) => assert.equal(windowAnswer(c), answers[i], `engine and window differ at ${JSON.stringify(c)}`));
});
