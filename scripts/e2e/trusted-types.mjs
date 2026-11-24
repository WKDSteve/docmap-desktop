// scripts/e2e/trusted-types.mjs -- the window under the CSP of tauri.conf.json, in a real browser.
//
//   node scripts/e2e/trusted-types.mjs
//
// The unit tests read sources; this one runs the page. src/ is served with the policy of `app.security.csp` as a header,
// `window.__TAURI__` is a stub that answers every command with an empty value, and the page must start, draw the empty
// screen (the placeholder, the catalog sentences with <strong>/<code>) and switch language without one CSP or
// Trusted Types violation. It also proves the policy is enforced here: a string assigned to innerHTML throws, and no
// policy can be created. A rule that is only written down passes either way.
//
// Needs Playwright with a Chromium (the CI image and the cloud sandbox have one). Exit 0 = passed, 1 = failed,
// 77 = a prerequisite is missing (a reported skip, never a green run).
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

const browser = await chromium.launch(process.env.E2E_BROWSER_CHANNEL ? { channel: process.env.E2E_BROWSER_CHANNEL } : {});
let code = 0;
try {
  const page = await browser.newPage();
  const problems = [];
  page.on('console', (m) => /trusted ?type|content security policy/i.test(m.text()) && problems.push(m.text()));
  page.on('pageerror', (e) => /trusted ?type/i.test(String(e)) && problems.push(String(e)));
  await page.addInitScript(STUB);
  await page.goto(url);
  await page.waitForFunction(() => document.querySelector('#placeholder h2')?.textContent, null, { timeout: 15000 }).catch(() => {
    problems.push('the empty screen never drew its placeholder');
  });

  // The language switch runs applyLocale over every [data-i18n] element, the sentences with markup included.
  const switched = await page.evaluate(() => {
    const sel = document.querySelector('#lang, select[data-lang], #prefs-lang');
    if (!sel) return false;
    sel.value = [...sel.options].map((o) => o.value).find((v) => v !== sel.value) ?? sel.value;
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    return true;
  });
  if (!switched) console.log('note: no language control found; applyLocale ran once at start only');
  await page.waitForTimeout(300);

  const strong = await page.evaluate(() => document.querySelectorAll('[data-i18n] strong, [data-i18n] code').length);
  if (strong === 0) problems.push('no catalog sentence with <strong>/<code> was built');

  // What the page itself did, before the probes below provoke violations on purpose.
  problems.push(...(await page.evaluate(() => window.__violations)));
  const own = problems.length;

  const enforced = await page.evaluate(() => {
    const out = {};
    try {
      document.createElement('div').innerHTML = '<b>x</b>';
      out.innerHTML = 'accepted';
    } catch (e) {
      out.innerHTML = e.name;
    }
    try {
      window.trustedTypes.createPolicy('docmap', { createHTML: (s) => s });
      out.policy = 'created';
    } catch (e) {
      out.policy = e.name;
    }
    return out;
  });
  if (enforced.innerHTML !== 'TypeError') problems.push(`a string was accepted by innerHTML (${enforced.innerHTML}): Trusted Types is not enforced`);
  if (enforced.policy === 'created') problems.push('a policy could be created: the CSP names one that the window needs none of');

  problems.length = own;
  if (enforced.innerHTML !== 'TypeError') problems.push(`a string was accepted by innerHTML (${enforced.innerHTML}): Trusted Types is not enforced`);
  if (enforced.policy === 'created') problems.push('a policy could be created: the CSP names one that the window needs none of');
  if (problems.length) {
    console.error(problems.map((p) => `  - ${p}`).join('\n'));
    code = 1;
  } else console.log('ok: the window runs under the CSP of tauri.conf.json; Trusted Types is enforced');
} finally {
  await browser.close();
  server.close();
}
process.exit(code);
