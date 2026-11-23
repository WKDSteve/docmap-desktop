// scripts/e2e/web-rules.mjs -- the web-platform rules of the window, in a real browser.
//
//   node scripts/e2e/web-rules.mjs
//
// What src/lib/web-rules.test.js can only read as text, this runs: reduced motion, translate="no", field size and
// spellcheck, tabular figures, the 24px targets, errors tied to their field, the feedback button while it works, and
// the dialogs that ask before typed input is thrown away (real keystrokes, real Escape). Same harness as
// trusted-types.mjs: src/ under the CSP of tauri.conf.json, `window.__TAURI__` a stub.
//
// Needs Playwright with a Chromium. Exit 0 = passed, 1 = failed, 77 = a prerequisite is missing.
import { createServer } from 'node:http';
import { readFileSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { extname, join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const src = join(root, 'src');
const SKIP = 77;
const fail = (why) => {
  console.error(`FAIL: ${why}`);
  process.exit(1);
};

let chromium;
for (const from of [import.meta.url, join(root, 'web', 'package.json')]) {
  for (const name of ['playwright', 'playwright-core', '@playwright/test']) {
    try {
      chromium ??= createRequire(from)(name).chromium;
    } catch {
      /* next */
    }
  }
}
if (!chromium) {
  console.log('SKIP: no Playwright');
  process.exit(SKIP);
}

const csp = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8')).app.security.csp;
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.ttf': 'font/ttf', '.woff2': 'font/woff2' };

const server = createServer((req, res) => {
  const path = join(src, decodeURIComponent(new URL(req.url, 'http://x').pathname).replace(/^\/+$/, 'index.html'));
  if (!path.startsWith(src) || !existsSync(path) || path === src) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { 'content-type': TYPES[extname(path)] ?? 'application/octet-stream', 'content-security-policy': csp });
  res.end(readFileSync(path));
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const url = `http://127.0.0.1:${server.address().port}/index.html`;

const STUB = () => {
  const none = async () => (Math.random() < 2 ? [] : null);
  class Channel {
    set onmessage(_) {}
  }
  window.__TAURI__ = {
    core: { invoke: none, convertFileSrc: (p) => p, Channel },
    dialog: { open: none, save: none },
    event: { listen: async () => () => {} },
  };
  window.__violations = [];
  document.addEventListener('securitypolicyviolation', (e) => window.__violations.push(`${e.violatedDirective}: ${e.sample ?? e.blockedURI}`));
};


const browser = await chromium.launch();
let code = 0;
const problems = [];
const check = (ok, what) => {
  if (!ok) problems.push(what);
};
try {
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
  // open_feedback takes a while, so the busy state can be looked at.
  await page.addInitScript(STUB);
  await page.addInitScript(() => {
    const base = window.__TAURI__.core.invoke;
    window.__calls = [];
    window.__TAURI__.core.invoke = async (cmd, args) => {
      window.__calls.push(cmd);
      if (cmd === 'open_feedback') await new Promise((r) => setTimeout(r, 600));
      return base(cmd, args);
    };
  });
  const asked = [];
  let answer = false;
  page.on('dialog', async (d) => {
    asked.push(d.message());
    await (answer ? d.accept() : d.dismiss());
  });
  await page.goto(url);
  await page.waitForFunction(() => document.querySelector('#placeholder h2')?.textContent, null, { timeout: 15000 });

  // ACC-03: with the preference there is no transition, without it there is one.
  const motion = async (mode) => {
    await page.emulateMedia({ reducedMotion: mode });
    return page.evaluate(() => {
      document.body.classList.add('sb-auto');
      return getComputedStyle(document.getElementById('sidebar')).transitionDuration;
    });
  };
  check((await motion('reduce')).split(',').every((d) => parseFloat(d) === 0), 'the sidebar still animates with prefers-reduced-motion: reduce');
  check((await motion('no-preference')).split(',').some((d) => parseFloat(d) > 0), 'the sidebar does not animate without the preference');
  await page.evaluate(() => document.body.classList.remove('sb-auto'));

  // HTML-05: nothing that is code is translated.
  const codes = await page.evaluate(() => [...document.querySelectorAll('code')].filter((c) => c.getAttribute('translate') !== 'no').length);
  check(codes === 0, `${codes} <code> element(s) without translate="no"`);
  check(await page.evaluate(() => document.getElementById('brand').translate === false), 'the brand name is translatable');

  // FORM-05: 16px at least, no spellcheck where it is noise.
  const fields = await page.evaluate(() =>
    [...document.querySelectorAll('input:not([type=checkbox]):not([type=radio]), textarea, select')]
      .filter((e) => e.type !== 'hidden')
      .map((e) => [e.id || e.className, parseFloat(getComputedStyle(e).fontSize)])
      .filter(([, px]) => px < 16),
  );
  check(fields.length === 0, `fields under 16px: ${JSON.stringify(fields)}`);
  for (const id of ['url-input', 'ws-new', 'editor-cmd', 'repo-filter']) {
    check(await page.evaluate((i) => document.getElementById(i).spellcheck === false, id), `#${id} is spell-checked`);
  }

  // CSS-09 and ACC-04.
  const style = await page.evaluate(() => {
    const note = getComputedStyle(document.getElementById('rules-chat-egress'));
    const fold = document.createElement('button');
    fold.className = 'rules-fold';
    fold.textContent = '▸';
    document.body.append(fold);
    const r = fold.getBoundingClientRect();
    fold.remove();
    return { nums: note.fontVariantNumeric, wrap: getComputedStyle(document.querySelector('h2')).textWrapMode, balance: CSS.supports('text-wrap', 'balance') ? getComputedStyle(document.querySelector('h2')).textWrap : 'balance', w: r.width, h: r.height };
  });
  check(/tabular-nums/.test(style.nums), `the rules note has ${style.nums}, not tabular figures`);
  check(/balance/.test(style.balance), `headings wrap as ${style.balance}`);
  check(style.w >= 24 && style.h >= 24, `.rules-fold is ${style.w} x ${style.h}`);

  // FORM-06: an empty summary and an empty workspace name mark their field.
  const marked = async (open, click, field, problem) => {
    await page.evaluate((id) => document.getElementById(id).showModal(), open);
    await page.click(click);
    const s = await page.evaluate(([f, p]) => ({
      invalid: document.getElementById(f).getAttribute('aria-invalid'),
      desc: document.getElementById(f).getAttribute('aria-describedby'),
      focus: document.activeElement.id,
      text: document.getElementById(p).textContent.trim(),
    }), [field, problem]);
    check(s.invalid === 'true' && s.desc === problem && s.focus === field && s.text !== '', `${field}: ${JSON.stringify(s)}`);
    await page.keyboard.type('x');
    check(await page.evaluate((f) => !document.getElementById(f).hasAttribute('aria-invalid'), field), `${field} stays marked after typing`);
    await page.fill(`#${field}`, '');
    await page.evaluate((id) => document.getElementById(id).close(), open);
  };
  await marked('fbbox', '#fb-go', 'fb-subject', 'fb-problem');
  await marked('wsbox', '#ws-create', 'ws-new', 'ws-problem');

  // FORM-06: the feedback button while the report opens.
  await page.evaluate(() => document.getElementById('fbbox').showModal());
  await page.fill('#fb-subject', 'a report');
  const label = await page.textContent('#fb-go');
  await page.click('#fb-go');
  const busy = await page.evaluate(() => ({ disabled: document.getElementById('fb-go').disabled, text: document.getElementById('fb-go').textContent, status: document.getElementById('fb-status').textContent.trim(), shown: !document.getElementById('fb-status').hidden }));
  check(busy.disabled && busy.text === label && busy.shown && busy.status !== '', `feedback button while busy: ${JSON.stringify(busy)}`);
  await page.waitForFunction(() => !document.getElementById('fbbox').open, null, { timeout: 5000 });
  check((await page.evaluate(() => window.__calls.filter((c) => c === 'open_feedback').length)) === 1, 'open_feedback was not called exactly once');

  // FORM-07: Escape on a clean dialog closes it; with typed input it asks, no keeps it, yes closes it.
  const dialogs = [['fbbox', '#fb-subject'], ['wsbox', '#ws-new'], ['runbox', '#run-title'], ['scopebox', '#scope-source']];
  for (const [id, field] of dialogs) {
    asked.length = 0;
    await page.evaluate((i) => document.getElementById(i).showModal(), id);
    await page.keyboard.press('Escape');
    check(!(await page.evaluate((i) => document.getElementById(i).open, id)) && asked.length === 0, `${id}: a clean dialog did not close without a question`);
    await page.evaluate((i) => document.getElementById(i).showModal(), id);
    await page.focus(field);
    await page.keyboard.type('typed');
    answer = false;
    await page.keyboard.press('Escape');
    check((await page.evaluate((i) => document.getElementById(i).open, id)) && asked.length === 1, `${id}: Escape did not ask, or closed after no (${asked.length} questions)`);
    // The backdrop goes through the same question.
    await page.mouse.click(2, 2);
    check((await page.evaluate((i) => document.getElementById(i).open, id)) && asked.length === 2, `${id}: the backdrop did not ask, or closed after no (${asked.length} questions)`);
    answer = true;
    await page.keyboard.press('Escape');
    check(!(await page.evaluate((i) => document.getElementById(i).open, id)) && asked.length === 3, `${id}: yes did not close the dialog`);
  }
  // Settings: only a command not yet stored is input.
  asked.length = 0;
  await page.evaluate(() => document.getElementById('prefsbox').showModal());
  await page.keyboard.press('Escape');
  check(!(await page.evaluate(() => document.getElementById('prefsbox').open)) && asked.length === 0, 'prefsbox: a clean dialog asked');
  await page.evaluate(() => document.getElementById('prefsbox').showModal());
  await page.focus('#editor-cmd');
  await page.keyboard.type('code -g');
  answer = false;
  await page.keyboard.press('Escape');
  check((await page.evaluate(() => document.getElementById('prefsbox').open)) && asked.length === 1, 'prefsbox: Escape did not ask about the editor command');

  if (problems.length) {
    console.error(problems.map((p) => `  - ${p}`).join('\n'));
    code = 1;
  } else console.log('ok: the web-platform rules hold in the running window');
} finally {
  await browser.close();
  server.close();
}
process.exit(code);
