// scripts/e2e/rules-run-fixture.mjs -- a project with a finished agent run, for the sheet of the run view's decisions
// (TESTS/sheets/rules-run-decide.sheet.lua).
//
//   node scripts/e2e/rules-run-fixture.mjs
//
// Copies TESTS/fixtures/rules (the project and its ruleset) to src-tauri/target/sheet-fixtures/rules-decide, fresh
// on every call: the sheet accepts and waives there, and the engine writes .rules-verdicts.json and
// .rules-waivers.json into that copy, never into the tracked fixture. Under the build folder and not the temp folder,
// for the reason rules/tab.rs gives (the standalone engine does not come back from a project below
// AppData\Local\Temp on Windows).
//
// The run is made the way the app makes one, without a model: the engine of this checkout (src-tauri/binaries)
// plans the selection (`rules/plan`), a canned answer goes through `rules/validate`, and the record is written in
// the shape of agent/store.rs (`.rules-proposals/<run>/run.json`), with the request key the app computes
// (sha256 of system, prompt, backend and model joined by NUL). Three proposals: M-01 "no violation found" and M-02 a
// violation, both current, and M-03 with a key of another request, which the app reads as "the files changed":
// outdated, so it cannot be accepted. N-01 has no scope and is not sent.
//
// Exit 0: written. Exit 1: no engine, or the engine did not plan or validate as expected (what it said is printed).
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const dest = join(root, 'src-tauri', 'target', 'sheet-fixtures', 'rules-decide');
const project = join(dest, 'project');
const RUN = 'r-sheet-decide';
const MODEL = 'local-example';
const SELECTION = ['M-01', 'M-02', 'M-03', 'N-01'];

const fail = (why) => {
  console.error(`rules-run-fixture: ${why}`);
  process.exit(1);
};

const bins = join(root, 'src-tauri', 'binaries');
const engine = existsSync(bins)
  ? readdirSync(bins)
      .filter((n) => n.startsWith('docmap-'))
      .map((n) => join(bins, n))
      .find((p) => statSync(p).size > 0)
  : undefined;
if (!engine) fail(`no engine in ${bins}: stage one first (scripts/build-install.ps1 -BuildOnly does)`);

rmSync(dest, { recursive: true, force: true });
mkdirSync(dest, { recursive: true });
cpSync(join(root, 'TESTS', 'fixtures', 'rules'), dest, { recursive: true });

/** One route of the engine over the copy, its JSON answer. No vault and no trust of whoever runs this. */
function api(route, flags) {
  const r = spawnSync(engine, [project, `--api=${route}`, ...flags], {
    encoding: 'utf8',
    env: { ...process.env, RULES_VAULT: '', RULES_TRUST: '' },
    timeout: 240_000,
  });
  let answer;
  try {
    answer = JSON.parse(r.stdout);
  } catch {
    fail(`${route} did not answer with JSON (exit ${r.status}): ${r.stderr || r.stdout}`);
  }
  if (answer.available !== true) fail(`${route}: ${answer.reason}`);
  return answer;
}

const sha256 = (...parts) => createHash('sha256').update(parts.join('\0')).digest('hex');

const plan = api('rules/plan', [`--select=${SELECTION.join(',')}`]);
const batches = Array.isArray(plan.batches) ? plan.batches : [];
if (batches.length !== 1) fail(`expected one request, the plan has ${batches.length}`);
const batch = batches[0];
const ids = batch.rules.map((r) => r.id);
if (ids.join(',') !== 'M-01,M-02,M-03') fail(`expected M-01, M-02 and M-03 in the request, got ${ids}`);
const key = sha256(batch.system, batch.prompt, 'ollama', MODEL);

// What a model could have answered, checked against the tree as every answer is.
const answer = {
  results: [
    { rule: 'M-01', verdict: 'compliant', checked: ['lua/a.lua'] },
    {
      rule: 'M-02',
      verdict: 'violation',
      checked: ['lua/a.lua'],
      findings: [{ file: 'lua/a.lua', line: 3, quote: 'M.word = "FORBIDDEN"', why: 'a word, not a question' }],
    },
    { rule: 'M-03', verdict: 'compliant', checked: ['lua/a.lua'] },
  ],
};
const answerFile = join(dest, 'answer.json');
writeFileSync(answerFile, JSON.stringify(answer));

const now = Date.now();
const proposals = ids.map((id) => {
  const p = api('rules/validate', [`--select=${id}`, `--answer=${answerFile}`]).proposal;
  if (!p || typeof p !== 'object') fail(`rules/validate gave no proposal for ${id}`);
  p.proposed = true;
  p.verified = false;
  return {
    ruleId: id,
    batch: batch.id,
    // M-03 was answered for another request: the app reads that as "the files changed since"
    key: id === 'M-03' ? sha256('an earlier request') : key,
    ruleHash: null,
    status: p.status,
    proposal: p,
    error: null,
    createdMs: now,
  };
});

const usage = { promptTokens: 1200, completionTokens: 180 };
const run = {
  id: RUN,
  title: 'M-01, M-02, M-03 +1 (sheet)',
  notes: '',
  root: project.replaceAll('\\', '/'),
  selection: SELECTION.join(','),
  refresh: false,
  backend: 'ollama',
  model: MODEL,
  local: true,
  state: 'done',
  createdMs: now,
  updatedMs: now,
  finishedMs: now,
  batches: [
    {
      id: batch.id,
      rules: ids,
      files: (batch.files || []).length,
      bytes: batch.bytes,
      tokens: batch.tokens,
      key,
      state: 'done',
      fromCache: false,
      startedMs: now,
      finishedMs: now,
      usage,
      error: null,
    },
  ],
  proposals,
  skipped: Array.isArray(plan.skipped) ? plan.skipped : [],
  usage,
  error: null,
};
const runDir = join(project, '.rules-proposals', RUN);
mkdirSync(runDir, { recursive: true });
writeFileSync(join(runDir, 'run.json'), JSON.stringify(run, null, 2));
rmSync(answerFile);
console.log(`rules-run-fixture: ${project} with run ${RUN} (${proposals.map((p) => `${p.ruleId} ${p.status}`).join(', ')})`);
