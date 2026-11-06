// axe on the window of the app in a real Chrome, in light and dark, also with a hover, a focus and the dialogs open. The
// page is the app's own markup and CSS with the stubbed bridge of tools/preview, so this proves structure, names and
// contrast, not what the commands do. The app itself stays without a package manager: Playwright and axe come at fixed
// versions from the folder tools/page-check, which nothing else reads.
//
//   pnpm --dir tools/page-check install --frozen-lockfile
//   node tools/page-check.mjs tools/page-check/node_modules

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { connect } from 'node:net';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const modules = process.argv[2];
if (!modules) {
  console.error('usage: node tools/page-check.mjs <node_modules>');
  process.exit(2);
}
const require = createRequire(join(resolve(modules), 'x.js'));
const { chromium } = require('@playwright/test');
const { AxeBuilder } = require('@axe-core/playwright');

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const python = process.platform === 'win32' ? 'python' : 'python3';
const server = spawn(python, ['tools/preview/preview.py'], { cwd: root, stdio: 'ignore' });
const URL = 'http://localhost:8731/tools/preview/preview.html';

const failed = [];
let browser;
try {
  for (let i = 0; ; i += 1) {
    try {
      await new Promise((ok, no) => {
        const socket = connect(8731, 'localhost', () => {
          socket.destroy();
          ok();
        });
        socket.on('error', no);
      });
      break;
    } catch (e) {
      if (i > 50) throw e;
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  browser = await chromium.launch({ channel: 'chrome' });
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  const problems = [];
  page.on('pageerror', (e) => problems.push(String(e)));
  await page.goto(URL);
  await page.waitForSelector('#sidebar');
  await page.waitForTimeout(800);

  const check = async (what) => {
    // The map is a page of its own, with its own checks in docmap.nvim.
    const result = await new AxeBuilder({ page }).exclude('iframe').analyze();
    const found = result.violations.flatMap((v) => v.nodes.map((n) => `${v.id}: ${n.target.join(' ')}`));
    console.log(`${found.length === 0 ? 'ok    ' : 'FAILED'} ${what}`);
    for (const line of found) console.log(`       ${line}`);
    failed.push(...found.map((line) => `${what}: ${line}`));
  };

  for (const theme of ['light', 'dark']) {
    await page.evaluate((t) => document.documentElement.setAttribute('data-theme', t), theme);
    await check(`${theme}: the window`);
    // The app may have opened one by itself (a first start): the window is looked at without it, each dialog below.
    await page.evaluate(() => document.querySelectorAll('dialog[open]').forEach((d) => d.close()));
    const button = page.locator('#sidebar button:visible').first();
    await button.hover();
    await check(`${theme}: a button under the pointer`);
    await button.focus();
    await check(`${theme}: a button with the focus`);
    await page.mouse.move(0, 0);
    for (const id of await page.locator('dialog').evaluateAll((all) => all.map((d) => d.id).filter(Boolean))) {
      const opened = await page.evaluate((name) => {
        const dialog = document.getElementById(name);
        try {
          dialog.showModal();
          return true;
        } catch {
          return false;
        }
      }, id);
      if (!opened) continue;
      await check(`${theme}: the dialog ${id}`);
      await page.evaluate((name) => document.getElementById(name).close(), id);
    }
  }
  if (problems.length > 0) {
    console.log(`FAILED errors of the page: ${problems.join('; ')}`);
    failed.push(...problems);
  }
} finally {
  await browser?.close();
  server.kill();
}
if (failed.length > 0) {
  console.error(`${failed.length} finding(s)`);
  process.exit(1);
}
console.log('no finding');
