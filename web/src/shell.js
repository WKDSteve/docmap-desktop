// The shell of the web variant: everything the page of the app (src/main.js) finds in place when it starts.
//
// Imported before src/main.js, so it runs first: it puts the markup of the window (the body of src/index.html, carried
// in the script) into the document, draws the menu bar and the strip under it, and installs `window.__TAURI__`: the
// bridge src/main.js talks to in the desktop app, answered here by backend.js. The engine starts in a worker in the
// background; a command that comes before it is ready waits for it.

import { grantAccess, idbKeyValue, permissionOf, pickFolder, pickerSupport } from 'appkit-web/folder';
import { createBackend } from './backend.js';
import { HIDDEN } from './hidden.js';
import { WorkerClient } from './client.js';
import { loadParts, startBlobWorker } from './embedded.js';
import { createMenubar } from './menubar.js';
import { text } from './texts.js';

/* global __WEB_BODY__, __WEB_MENU__, __WEB_INFO__ */

/** The name of the page's own IndexedDB: every local HTML file shares the origin `file://`, so each app has its own. */
export const DATABASE = 'docmap-web';
/** The id under which the browser remembers where the picker was last opened. */
export const PICKER_ID = 'docmap-web-project';

/** @type {{ version: string, build: string | null, engine: { commit: string, lib_commit: string },
 *   contract: { commit: string, schema: number, routes: string[] } }} */
const INFO = __WEB_INFO__;

const doc = document;
doc.documentElement.classList.add('web');
const mount = /** @type {HTMLElement} */ (doc.getElementById('app'));
mount.insertAdjacentHTML(
  'beforebegin',
  '<div id="web-top"><nav id="web-menubar" aria-label="Menu"></nav><div id="web-strip"></div></div>' + __WEB_BODY__,
);
mount.remove();

for (const selector of HIDDEN.elements) {
  for (const node of doc.querySelectorAll(selector)) node.setAttribute('data-web-hide', '');
}
for (const selector of HIDDEN.sectionsOf) {
  doc.querySelector(selector)?.closest('.prefs-section')?.setAttribute('data-web-hide', '');
}

const strip = /** @type {HTMLElement} */ (doc.getElementById('web-strip'));
/** @type {{ waiting: { id: string, name: string, state: string }[], skipped: { id: string, name: string, skipped: unknown[] }[] }} */
let notice = { waiting: [], skipped: [] };
/** @type {string | null} */
let failed = null;
let detailsOpen = false;

/** @type {ReturnType<typeof createBackend> | null} */
let backend = null;

const menubar = createMenubar(/** @type {HTMLElement} */ (doc.getElementById('web-menubar')), __WEB_MENU__, {
  onChoose: (id) => backend?.emit('menu', id),
  unavailableText: () => text(menubar.locale(), 'menu.unavailable'),
});

/** @param {string} tag @param {string} cls @param {string} [content] */
function el(tag, cls, content) {
  const node = doc.createElement(tag);
  node.className = cls;
  if (content !== undefined) node.textContent = content;
  return node;
}

function drawStrip() {
  const locale = menubar.locale();
  doc.documentElement.lang = locale;
  strip.replaceChildren();
  const about = el('details', 'web-strip-about');
  about.open = detailsOpen;
  about.addEventListener('toggle', () => {
    detailsOpen = about.open;
  });
  const summary = el('summary', 'web-strip-summary');
  summary.append(el('strong', 'web-strip-label', text(locale, 'strip.label')));
  summary.append(el('span', 'web-strip-text', text(locale, 'strip.summary')));
  summary.append(el('span', 'web-strip-more', text(locale, 'strip.more')));
  about.append(summary);
  about.append(el('p', 'web-strip-details', text(locale, 'strip.details')));
  strip.append(about);

  if (failed !== null) {
    strip.append(el('p', 'web-strip-bad', `${text(locale, 'start.failed')}: ${failed}`));
  }
  if (pickerSupport(window) !== 'ok') {
    strip.append(el('p', 'web-strip-bad', text(locale, 'picker.missing')));
  }
  const waiting = notice.waiting.filter((w) => w.state !== 'gone');
  const gone = notice.waiting.filter((w) => w.state === 'gone');
  if (waiting.length > 0) {
    const row = el('p', 'web-strip-access');
    row.append(
      el('span', '', waiting.length === 1 ? text(locale, 'access.one') : text(locale, 'access.many', { n: waiting.length })),
    );
    const allow = /** @type {HTMLButtonElement} */ (el('button', 'web-strip-allow', text(locale, 'access.allow')));
    allow.type = 'button';
    allow.id = 'web-allow';
    allow.addEventListener('click', async () => {
      allow.disabled = true;
      // The click is the gesture the browser wants for its question; nothing is awaited before the first one.
      const granted = await backend?.allowAccess().catch(() => 0);
      if (granted) backend?.emit('web:folders-changed', []);
    });
    row.append(allow);
    strip.append(row);
  }
  if (gone.length > 0) {
    strip.append(el('p', 'web-strip-bad', text(locale, 'access.gone', { names: gone.map((g) => g.name).join(', ') })));
  }
  for (const s of notice.skipped) {
    const n = s.skipped.length;
    strip.append(
      el('p', 'web-strip-note', n === 1 ? text(locale, 'skipped.one', { name: s.name }) : text(locale, 'skipped.many', { n, name: s.name })),
    );
  }
}
drawStrip();

async function start() {
  const parts = await loadParts((id) => doc.getElementById(id)?.textContent ?? null);
  const client = new WorkerClient(startBlobWorker(parts.worker, { Blob, URL, Worker }));
  await client.init({ wasm: parts.wasm, bundle: parts.bundle, host: parts.host });
  return createBackend({
    client,
    kv: idbKeyValue(window.indexedDB, DATABASE),
    pick: () => pickFolder(/** @type {any} */ (window), { id: PICKER_ID, mode: 'read' }),
    permission: {
      of: (handle) => permissionOf(handle, 'read'),
      grant: (handle) => grantAccess(handle, 'read'),
    },
    now: () => Date.now(),
    info: {
      version: INFO.version,
      build: INFO.build,
      engine: parts.bundle.engine,
      contract: INFO.contract,
      platform: navigator.platform || 'browser',
    },
    ui: {
      setMenu: (args) => {
        const before = menubar.locale();
        menubar.update(/** @type {any} */ (args));
        if (menubar.locale() !== before) drawStrip();
      },
      setTitle: (title) => {
        doc.title = title;
      },
      setZoom: (factor) => {
        doc.body.style.setProperty('zoom', String(factor));
      },
      download: (name, contents) => {
        const url = URL.createObjectURL(new Blob([contents], { type: 'text/plain' }));
        const a = doc.createElement('a');
        a.setAttribute('href', url);
        a.setAttribute('download', name);
        doc.body.append(a);
        a.click();
        a.remove();
        setTimeout(() => URL.revokeObjectURL(url), 60_000);
      },
      openUrl: (url) => {
        window.open(url, '_blank', 'noopener');
      },
      blobUrl: (html) => URL.createObjectURL(new Blob([html], { type: 'text/html' })),
      revokeUrl: (url) => URL.revokeObjectURL(url),
      notice: (state) => {
        notice = /** @type {typeof notice} */ (state);
        drawStrip();
      },
    },
  });
}

const ready = start().then(
  (made) => {
    backend = made;
    made.publishNotice().catch(() => {});
    // A folder that changed on disk is read again when the person comes back to the tab, and every half minute.
    // One look at a time (a look walks every loaded folder), and none while nobody sees the tab.
    let looking = false;
    const look = async () => {
      if (looking || doc.visibilityState === 'hidden') return;
      looking = true;
      try {
        const changed = await made.refreshChanged().catch(() => /** @type {string[]} */ ([]));
        if (changed.length > 0) made.emit('web:folders-changed', changed);
      } finally {
        looking = false;
      }
    };
    window.addEventListener('focus', look);
    setInterval(look, 30_000);
    return made;
  },
  (err) => {
    failed = String(err instanceof Error ? err.message : err);
    drawStrip();
    throw err;
  },
);
// Every command waits on `ready`; this keeps a failed start from also being reported as an unhandled rejection.
ready.catch(() => {});

/** What a command fails with while the engine could not start. */
const notStarted = () => ({ key: 'web.start', params: {}, detail: failed, message: `The engine in the page did not start — ${failed}` });

/** @template T @param {(b: ReturnType<typeof createBackend>) => T | Promise<T>} use @returns {Promise<T>} */
const withBackend = (use) => ready.then(use, () => Promise.reject(notStarted()));

/** @type {any} */ (window).__TAURI__ = {
  core: {
    invoke: (/** @type {string} */ command, /** @type {object} */ args) => withBackend((b) => b.invoke(command, args)),
    // The app turns a path into an address of its own protocol; here a map only ever comes from `serve_project`.
    convertFileSrc: () => '',
  },
  dialog: {
    open: (/** @type {object} */ options) => withBackend((b) => b.dialog.open(options)),
    save: (/** @type {object} */ options) => withBackend((b) => b.dialog.save(options)),
  },
  event: {
    listen: (/** @type {string} */ name, /** @type {(event: { payload: any }) => void} */ listener) =>
      withBackend((b) => b.listen(name, listener)),
  },
};

/** What src/main.js asks only in the web variant. */
/** @type {any} */ (window).__DOCMAP_WEB__ = {
  /** @param {string} base @param {string} theme @param {string | null} tab */
  mapUrl: (base, theme, tab) => backend?.mapUrl(base, theme, tab) ?? '',
};
