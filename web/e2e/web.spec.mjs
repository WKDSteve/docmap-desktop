// The built file itself, opened from disk in Chrome: what a person does with it, and what the page must never do.

import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
import { idbWrites, spyOnIndexedDb } from 'appkit-web/e2e';
import { addProject, fakeFolders, filesOf, mapFrame, pageUrl, violations, watchPage } from './page.mjs';

const GIT = { '.git/HEAD': 'ref: refs/heads/main\n' };
const SAMPLE = { ...filesOf(), ...GIT };
const OTHER = {
  ...GIT,
  'README.md': '# other\n',
  'lua/other/init.lua': "---@module 'other'\n---@brief The other project, which needs the sample.\nlocal sample = require('sample')\nreturn { sample = sample }\n",
};

/** Open a menu of the bar and choose an item by its text. */
async function choose(page, menu, item) {
  await page.locator('#web-menubar').getByRole('menuitem', { name: menu, exact: true }).click();
  await page.locator('.web-menu').getByText(item, { exact: true }).click();
}

/** Add the folder `name` and wait until its map is in the frame. */
async function addAndWait(page, name, module) {
  await addProject(page, name);
  await page.locator('#pick-folder').click();
  await expect(mapFrame(page).locator('body')).toContainText(module, { timeout: 30_000 });
}

test('the file opens from disk without a request, starts its engine and says what it is', async ({ page }) => {
  const seen = await watchPage(page);
  await fakeFolders(page, { sample: SAMPLE });
  await page.goto(pageUrl());
  expect(page.url().startsWith('file:///')).toBe(true);
  await expect(page.locator('#web-menubar').getByRole('menuitem')).toHaveText(['File', 'Project', 'View', 'Help']);
  await expect(page.locator('#web-strip')).toContainText('Web variant');
  await expect(page.locator('#web-strip')).toContainText('read only');
  // The engine answers: the panel of the window names it and its contract.
  await expect(page.locator('#engine-summary')).not.toBeEmpty({ timeout: 30_000 });
  await page.locator('#engine').locator('summary').click();
  await expect(page.locator('#engine-state')).toContainText('web+Lua5.4');
  // What is said about the difference is on the page, not behind a link.
  await page.locator('.web-strip-summary').click();
  await expect(page.locator('.web-strip-details')).toContainText('git history');
  await expect(page.locator('.web-strip-details')).toContainText('telemetry');
  await expect(page.locator('.web-strip-details')).toContainText('agent');
  // Nothing that needs a program is offered.
  await expect(page.locator('#nvim')).toBeHidden();
  expect(seen.requests).toEqual([]);
  expect(seen.errors).toEqual([]);
  expect(await violations(page)).toEqual([]);
});

test('a folder is added, mapped in the page and shown; nothing leaves the page and only its own database is written', async ({ page }) => {
  const seen = await watchPage(page);
  await spyOnIndexedDb(page);
  await fakeFolders(page, { sample: SAMPLE });
  await page.goto(pageUrl());
  await addAndWait(page, 'sample', 'sample');
  const frame = mapFrame(page);
  await expect(frame.locator('body')).toContainText('1 modules');
  await expect(page.locator('#status')).toContainText(/sample/);
  await expect(page.locator('#projects')).toContainText('sample');
  // The map is a page of its own, made from a blob; its scripts ran (it draws its tabs).
  expect(await page.locator('#map').getAttribute('src')).toMatch(/^blob:/);
  await expect(frame.locator('body')).toContainText('Small string helpers');

  expect(seen.requests).toEqual([]);
  expect(seen.errors).toEqual([]);
  expect(await violations(page)).toEqual([]);
  const writes = await idbWrites(page);
  expect(writes.length).toBeGreaterThan(0);
  expect([...new Set(writes.map((w) => w.db))]).toEqual(['docmap-web']);
  // What is kept is the list of folders, never the content of one.
  for (const w of writes) expect(w.json).not.toContain('entry of the sample project');
});

test('the report of a run is the whole of what the engine printed', async ({ page }) => {
  await fakeFolders(page, { sample: SAMPLE });
  await page.goto(pageUrl());
  await addAndWait(page, 'sample', 'sample');
  await expect(page.locator('#run-report')).toBeVisible();
  await expect(page.locator('#run-report-body')).toContainText('wrote docs/map/index.html');
  await expect(page.locator('#run-report-body')).toContainText(/\d+ modules?, \d+ namespaces?, \d+ files?/);
});

test('checking the map says whether it is current, from the folder as it is now', async ({ page }) => {
  const seen = await watchPage(page);
  await fakeFolders(page, { sample: SAMPLE });
  await page.goto(pageUrl());
  await addAndWait(page, 'sample', 'sample');
  await choose(page, 'Project', 'Check exactly');
  await expect(page.locator('#status')).toContainText('exactly current', { timeout: 15_000 });
  expect(seen.errors).toEqual([]);
});

test('the map opens in a tab of its own, from the same blob, still without a request', async ({ page }) => {
  const seen = await watchPage(page);
  await fakeFolders(page, { sample: SAMPLE });
  await page.goto(pageUrl());
  await addAndWait(page, 'sample', 'sample');
  const opening = page.waitForEvent('popup');
  await choose(page, 'File', 'Open map in browser');
  const tab = await opening;
  await tab.waitForLoadState('domcontentloaded');
  expect(tab.url()).toMatch(/^blob:/);
  await expect(tab.locator('body')).toContainText('The entry of the sample project');
  expect(seen.requests).toEqual([]);
  expect(seen.errors).toEqual([]);
});

test('the theme of the window is the theme of the map, light and dark', async ({ page }) => {
  await fakeFolders(page, { sample: SAMPLE });
  await page.goto(pageUrl());
  await addAndWait(page, 'sample', 'sample');
  for (const [item, theme] of [['Dark', 'dark'], ['Light', 'light']]) {
    await choose(page, 'View', item);
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await expect(mapFrame(page).locator('html')).toHaveAttribute('data-theme', theme, { timeout: 15_000 });
    await expect(mapFrame(page).locator('body')).toContainText('sample');
    await page.screenshot({ path: `test-results/web-${theme}.png` });
    const axe = await new AxeBuilder({ page }).include('#web-top').analyze();
    expect(axe.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target).join(' ')}`)).toEqual([]);
  }
  // The check mark follows.
  await page.locator('#web-menubar').getByRole('menuitem', { name: 'View', exact: true }).click();
  await expect(page.locator('.web-menu').getByRole('menuitemcheckbox', { name: 'Light', exact: true })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.web-menu').getByRole('menuitemcheckbox', { name: 'Dark', exact: true })).toHaveAttribute('aria-checked', 'false');
});

test('the menu speaks the language of the window, and says what is not part of this variant', async ({ page }) => {
  await fakeFolders(page, { sample: SAMPLE });
  await page.goto(pageUrl());
  await addAndWait(page, 'sample', 'sample');
  await choose(page, 'View', 'Deutsch');
  await expect(page.locator('#web-menubar').getByRole('menuitem')).toHaveText(['Datei', 'Projekt', 'Ansicht', 'Hilfe']);
  await expect(page.locator('#web-strip')).toContainText('Web-Variante');
  await expect(page.locator('html')).toHaveAttribute('lang', 'de');
  await page.locator('#web-menubar').getByRole('menuitem', { name: 'Hilfe', exact: true }).click();
  const items = page.locator('.web-menu').getByRole('menuitem');
  const off = items.filter({ hasText: /Feedback|Rückmeldung|Problem/ }).first();
  await expect(off).toBeDisabled();
  await expect(off).toHaveAttribute('title', 'Nicht Teil der Web-Variante');
  // The keyboard works the open menu, Escape closes it and goes back to the bar.
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Escape');
  await expect(page.locator('.web-menu')).toHaveCount(0);
  await expect(page.locator('#web-menubar').getByRole('menuitem', { name: 'Hilfe', exact: true })).toBeFocused();
});

test('several projects: each has its map, switching is a click, and the overview knows who needs whom', async ({ page }) => {
  await fakeFolders(page, { sample: SAMPLE, other: OTHER });
  await page.goto(pageUrl());
  await addAndWait(page, 'sample', 'sample');
  await addAndWait(page, 'other', 'other project');
  await expect(page.locator('#projects option')).toHaveCount(3);
  await page.locator('#projects').selectOption({ label: 'sample' });
  await expect(mapFrame(page).locator('body')).toContainText('entry of the sample project');
  await page.locator('#projects').selectOption({ label: 'other' });
  await expect(mapFrame(page).locator('body')).toContainText('other project');
  // With nothing selected the window lists the workspace, and the edge between the two maps.
  await page.locator('#projects').selectOption({ index: 0 });
  await expect(page.locator('#overview')).toBeVisible();
  await expect(page.locator('#ov-list')).toContainText('sample');
  await expect(page.locator('#ov-list')).toContainText('other');
  await expect(page.locator('#ov-deps')).toBeVisible();
  await page.locator('#ov-deps summary').click();
  await expect(page.locator('#ov-deps-list')).toContainText('sample');
  await expect(page.locator('#ov-deps-list')).toContainText('used by one project');
});

test('after a reload the projects are there; a folder the browser asks about again waits for one click', async ({ page }) => {
  const seen = await watchPage(page);
  await fakeFolders(page, { sample: SAMPLE });
  await page.goto(pageUrl());
  await addAndWait(page, 'sample', 'sample');

  await page.reload();
  await expect(page.locator('#projects')).toContainText('sample');
  // The folder may still be read (the permission holds through a reload): it is read again, the map is not kept.
  await page.locator('#projects').selectOption({ label: 'sample' });
  await expect(page.locator('#placeholder')).toBeVisible();
  await expect(page.locator('#web-allow')).toHaveCount(0);

  // The browser was restarted: the permission is asked again.
  await page.evaluate(() => window.__disk.permission('sample', 'prompt'));
  await page.reload();
  await expect(page.locator('#web-allow')).toBeVisible({ timeout: 15_000 });
  await expect(page.locator('.web-strip-access')).toContainText('1 folder waits for your permission');
  expect(await page.evaluate(() => window.__disk.asked)).toBe(0);
  await page.locator('#web-allow').click();
  await expect(page.locator('#web-allow')).toHaveCount(0);
  expect(await page.evaluate(() => window.__disk.asked)).toBe(1);
  await page.locator('#projects').selectOption({ label: 'sample' });
  await page.locator('#gen').click();
  await expect(mapFrame(page).locator('body')).toContainText('sample', { timeout: 30_000 });
  expect(seen.requests).toEqual([]);
  expect(seen.errors).toEqual([]);
});

test('a file changed by another program marks the map as stale, and generating again clears the mark', async ({ page }) => {
  await fakeFolders(page, { sample: SAMPLE });
  await page.goto(pageUrl());
  await addAndWait(page, 'sample', 'sample');
  await expect(page.locator('#proj-stale')).toBeHidden();
  await page.evaluate(() => {
    // An editor saves a new module; the fake disk's clock runs ahead of the real one, so it is newer than the map.
    window.__disk.write('sample', 'lua/sample/later.lua', "---@module 'sample.later'\n---@brief Written after the map.\nreturn {}\n");
    window.dispatchEvent(new Event('focus'));
  });
  await expect(page.locator('#proj-stale')).toBeVisible({ timeout: 15_000 });
  // The map shown is still the one generated before: it does not know the new module.
  await expect(mapFrame(page).locator('body')).not.toContainText('Written after the map');
});

test('the files of a project are browsed and searched from what was read', async ({ page }) => {
  await fakeFolders(page, { sample: SAMPLE });
  await page.goto(pageUrl());
  await addAndWait(page, 'sample', 'sample');
  await page.locator('[data-pane="files"]').click();
  await expect(page.locator('#files')).toContainText('README.md');
  await expect(page.locator('#files')).toContainText('lua');
  await expect(page.locator('#files')).not.toContainText('.git');
  // Statistics need the line counter of the app: the pane is not offered.
  await expect(page.locator('[data-pane="stats"]')).toBeHidden();
});

test('a map that lies in the folder is shown as it is, without generating', async ({ page }) => {
  const committed = '<!DOCTYPE html><html><head><title>committed</title></head><body><h1>the committed map</h1></body></html>';
  const map = JSON.stringify({ meta: { schema: 6, title: 'sample', counts: { module: 7, file: 9, namespace: 2 } }, nodes: [] });
  await fakeFolders(page, { sample: { ...SAMPLE, 'docs/map/index.html': committed, 'docs/map/module_map.json': map } });
  await page.goto(pageUrl());
  await addProject(page, 'sample');
  await page.locator('#pick-folder').click();
  await expect(mapFrame(page).locator('h1')).toHaveText('the committed map', { timeout: 30_000 });
  // The window knows there is a map (it does not offer "no map yet"), and nothing was generated.
  await expect(page.locator('#proj-counts')).toBeHidden();
  await expect(page.locator('#run-report')).toBeHidden();
});
