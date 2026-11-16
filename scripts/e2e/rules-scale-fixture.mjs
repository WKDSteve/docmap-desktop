// scripts/e2e/rules-scale-fixture.mjs -- a project with a ruleset of the size of a real one, for the sheet of the Rules
// tab at scale (TESTS/sheets/rules-scale.sheet.lua).
//
//   node scripts/e2e/rules-scale-fixture.mjs
//
// The ruleset of the author has several hundred rules, almost all manual, one file per rule. This writes such a
// ruleset next to a copy of TESTS/fixtures/rules (its project and its rules: the automated ones and the predicate that
// needs trust) to src-tauri/target/sheet-fixtures/rules-scale, fresh on every call. Under the build folder and not the
// temp folder, for the reason rules/tab.rs gives (the standalone engine does not come back from a project below
// AppData\Local\Temp on Windows).
//
// 13 families of 25 manual rules each (325), one Markdown file per rule; every fifth title is long enough to wrap.
// With the fixture's 7 rules the tab lists 332.
//
// Exit 0: written.
import { cpSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const source = join(root, 'TESTS', 'fixtures', 'rules');
const dest = join(root, 'src-tauri', 'target', 'sheet-fixtures', 'rules-scale');

// Not the letters of the fixture's own families (M, N, P, Q).
export const FAMILIES = ['B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'L', 'R', 'S', 'T'];
export const PER_FAMILY = 25;

rmSync(dest, { recursive: true, force: true });
mkdirSync(dest, { recursive: true });
cpSync(join(source, 'project'), join(dest, 'project'), { recursive: true });
cpSync(join(source, 'ruleset'), join(dest, 'ruleset'), { recursive: true });

const many = join(dest, 'ruleset', 'many');
mkdirSync(many, { recursive: true });
for (const family of FAMILIES) {
  for (let n = 1; n <= PER_FAMILY; n += 1) {
    const id = `${family}-${String(n).padStart(2, '0')}`;
    const title =
      n % 5 === 0
        ? `rule ${id} of the family ${family}, with a title as long as the long ones of a real ruleset, which has to wrap in the list instead of pushing its columns out of the window`
        : `rule ${id} of the family ${family}`;
    const text = [
      `# Family ${family}`,
      '',
      `## Part ${Math.ceil(n / 10)}`,
      '',
      `#### \`${id}\` — ${title}`,
      '',
      '```rule',
      `id = "${id}",`,
      `severity = "${n % 3 === 0 ? 'critical' : 'recommended'}",`,
      '```',
      '',
      'Read the code and decide.',
      '',
    ].join('\n');
    writeFileSync(join(many, `${id}.md`), text);
  }
}
console.log(`rules-scale-fixture: ${FAMILIES.length * PER_FAMILY} rules written below ${dest}`);
