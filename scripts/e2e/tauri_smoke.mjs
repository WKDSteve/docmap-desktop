// scripts/e2e/tauri_smoke.mjs -- tier 3: the REAL docmap-desktop window through msedgedriver (Windows) or tauri-driver
// (W3C WebDriver over plain HTTP, no client library).
//
//   node scripts/e2e/tauri_smoke.mjs [path\to\docmap-desktop.exe]
//
// Needs an `msedgedriver` whose version matches the installed WebView2 on Windows, `tauri-driver` (cargo install
// tauri-driver --locked) on Linux (override: $TESTING_TAURI_DRIVER, $TESTING_MSEDGEDRIVER). Exit 0 = passed,
// 1 = failed, 77 = a prerequisite is missing (a reported skip, never a green run). Windows and Linux only (no macOS
// driver).
//
// No window but the app's own: every program starts with `windowsHide` (a runner step has no console, so a console
// program would get a new, visible one), and on Windows the session goes straight to msedgedriver with the switch
// `enable-logging` excluded. msedgedriver passes `--enable-logging` to WebView2 by default, and with it
// msedgewebview2.exe allocates a console of its own (its parent, the app, is a GUI program without one): the window
// "...\msedgewebview2.exe" that took the focus on the runner. tauri-driver 2.1 cannot carry `excludeSwitches`, it
// replaces `ms:edgeOptions` wholesale; on Windows it does nothing but that mapping, which `nativeCapabilities` does
// here.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const SKIP = 77;
const skip = (why) => {
  console.log(`SKIP: ${why}`);
  process.exit(SKIP);
};
const which = (name) => {
  const r = spawnSync(process.platform === 'win32' ? 'where' : 'which', [name], {
    encoding: 'utf8',
    windowsHide: true,
  });
  return r.status === 0 ? r.stdout.split(/\r?\n/)[0].trim() : null;
};

// What tauri-driver 2.1 makes of `tauri:options` on Windows, plus the one switch it cannot exclude.
const nativeCapabilities = (application) => ({
  'ms:edgeChromium': true,
  browserName: 'webview2',
  'ms:edgeOptions': { binary: application, args: [], excludeSwitches: ['enable-logging'] },
});

const windows = process.platform === 'win32';
const app = resolve(process.argv[2] ?? resolve(root, 'src-tauri/target/release/docmap-desktop.exe'));
if (process.platform === 'darwin') skip('Tauri has no WebDriver on macOS');
if (!existsSync(app)) skip(`app binary missing: ${app} (cargo tauri build)`);
const driver = windows ? null : process.env.TESTING_TAURI_DRIVER || which('tauri-driver');
const native = windows ? process.env.TESTING_MSEDGEDRIVER || which('msedgedriver') : null;
if (!windows && !driver) skip('tauri-driver not found (cargo install tauri-driver --locked)');
if (windows && !native) skip('msedgedriver not found (version of the installed WebView2)');

const outDir = resolve(process.env.DOCMAP_E2E_OUT ?? resolve(root, 'src-tauri/target/e2e'));
const port = 4500 + Math.floor(Math.random() * 400);
const wd = `http://127.0.0.1:${port}`;
const proc = windows
  ? spawn(native, [`--port=${port}`, '--host=127.0.0.1'], { stdio: 'ignore', windowsHide: true })
  : spawn(driver, ['--port', String(port)], { stdio: 'ignore', windowsHide: true });
const failures = [];
const check = (cond, msg) => {
  console.log(`${cond ? '  ok   ' : '  FAIL '}${msg}`);
  if (!cond) failures.push(msg);
};
const call = async (method, path, body) => {
  const r = await fetch(wd + path, {
    method,
    // a hung app or driver must end the run, not the job's whole time limit
    signal: AbortSignal.timeout(30000),
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(`${method} ${path}: ${JSON.stringify(j.value)}`);
  return j.value;
};

let session;
try {
  for (let i = 0; i < 50; i++) {
    try {
      if ((await fetch(`${wd}/status`)).ok) break;
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  const created = await call('POST', '/session', {
    capabilities: { alwaysMatch: windows ? nativeCapabilities(app) : { 'tauri:options': { application: app } } },
  });
  session = created.sessionId;
  const s = `/session/${session}`;
  await new Promise((r) => setTimeout(r, 2000));

  check((await call('GET', `${s}/title`)).includes('docmap'), 'the window title names docmap');
  const body = await call('POST', `${s}/execute/sync`, {
    script: 'return document.body ? document.body.innerText.length : -1;',
    args: [],
  });
  check(body > 0, `the page is rendered (${body} characters of text)`);
  const rect = await call('GET', `${s}/window/rect`);
  check(rect.width > 300 && rect.height > 200, `the window has a size (${rect.width}x${rect.height})`);

  mkdirSync(outDir, { recursive: true });
  const png = await call('GET', `${s}/screenshot`);
  writeFileSync(resolve(outDir, 'docmap_window.png'), Buffer.from(png, 'base64'));
  console.log(`screenshot: ${resolve(outDir, 'docmap_window.png')}`);
} catch (e) {
  failures.push(String(e));
  console.log(`  FAIL ${e}`);
} finally {
  if (session) await call('DELETE', `/session/${session}`).catch(() => {});
  proc.kill();
}
process.exit(failures.length ? 1 : 0);
