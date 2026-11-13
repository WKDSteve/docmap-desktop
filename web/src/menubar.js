// The menu bar of the web variant, drawn in the page: the same menu the desktop app hands to the operating system
// (src-tauri/src/menu.rs, read at build time by menu-model.mjs), with the labels and the state the page sends through
// `set_menu`. A click emits the id of the item, as the native menu does.
//
// No accelerators: the keys the app uses (Ctrl+N, Ctrl+G, F5, Ctrl+0, ...) belong to the browser in a tab, and most of
// them cannot be taken. An item that needs a program behind the page is shown switched off, with the reason.

/** What `LOCALE_ID` is in menu.rs: the prefix of a language item's id. */
export const LOCALE_ID = 'menu.view.lang:';

/** The items of the menu that have nothing behind them in a browser. */
export const WEB_UNAVAILABLE = new Set([
  'menu.file.reveal',
  'menu.file.copy_path',
  'menu.file.export',
  'menu.project.generate_full',
  'menu.view.stats',
  'menu.help.usage',
  'menu.help.engine',
  'menu.help.docs_search',
  'menu.help.settings_folder',
  'menu.help.repo',
  'menu.help.license',
  'menu.help.feedback',
]);

/** Whether an item shows a check mark (menu.rs, `is_checked`). @param {string} id @param {any} state */
export function isChecked(id, state) {
  switch (id) {
    case 'menu.view.theme.system':
      return state.theme !== 'light' && state.theme !== 'dark';
    case 'menu.view.theme.light':
      return state.theme === 'light';
    case 'menu.view.theme.dark':
      return state.theme === 'dark';
    case 'menu.view.files':
      return Boolean(state.files);
    case 'menu.view.stats':
      return Boolean(state.stats);
    case 'menu.view.sidebar':
      return Boolean(state.sidebar);
    case 'menu.view.sidebar_auto':
      return Boolean(state.sidebarAuto);
    default:
      return false;
  }
}

/**
 * The menu as rows to draw: what each open menu lists, with its label, state and whether it can be chosen.
 * @param {import('../menu-model.mjs').MenuGroup[]} model
 * @param {{ labels: Record<string, string>, hasProject: boolean, state: any }} args what `set_menu` got
 */
export function menuRows(model, args) {
  const label = (/** @type {string} */ id) => args.labels?.[id] ?? id;
  /** @typedef {{ kind: 'item' | 'check' | 'head' | 'separator', id?: string, label?: string, enabled?: boolean, checked?: boolean, unavailable?: boolean, depth: number }} Row */
  /** @param {import('../menu-model.mjs').MenuNode[]} nodes @param {number} depth @returns {Row[]} */
  const rows = (nodes, depth) =>
    nodes.flatMap((n) => {
      if (n.type === 'separator') return [{ kind: /** @type {const} */ ('separator'), depth }];
      if (n.type === 'locales') {
        return (args.state?.locales ?? []).map((/** @type {{ code: string, label: string }} */ loc) => ({
          kind: /** @type {const} */ ('check'),
          id: LOCALE_ID + loc.code,
          label: loc.label,
          enabled: true,
          checked: loc.code === args.state?.locale,
          depth,
        }));
      }
      if (n.type === 'sub') return [{ kind: /** @type {const} */ ('head'), label: label(n.id), depth }, ...rows(n.items, depth + 1)];
      const unavailable = WEB_UNAVAILABLE.has(n.id);
      if (n.type === 'check') {
        return [{ kind: /** @type {const} */ ('check'), id: n.id, label: label(n.id), enabled: !unavailable, checked: isChecked(n.id, args.state ?? {}), unavailable, depth }];
      }
      return [{ kind: /** @type {const} */ ('item'), id: n.id, label: label(n.id), enabled: !unavailable && (!n.needsProject || args.hasProject), unavailable, depth }];
    });
  return model.map((group) => ({ id: group.id, label: label(group.id), rows: rows(group.items, 0) }));
}

/**
 * Draw the bar into `host` and keep it current.
 * @param {HTMLElement} host
 * @param {import('../menu-model.mjs').MenuGroup[]} model
 * @param {{ onChoose(id: string): void, unavailableText(): string }} hooks
 */
export function createMenubar(host, model, hooks) {
  const doc = host.ownerDocument;
  /** @type {{ labels: Record<string, string>, hasProject: boolean, state: any }} */
  let args = { labels: {}, hasProject: false, state: {} };
  /** @type {string | null} */
  let openId = null;

  host.setAttribute('role', 'menubar');

  const close = () => {
    if (openId === null) return;
    openId = null;
    draw();
  };

  /** @param {string} id @param {boolean} focusFirst */
  const open = (id, focusFirst) => {
    openId = id;
    draw();
    if (focusFirst) /** @type {HTMLElement | null} */ (host.querySelector('.web-menu [role^="menuitem"]:not([disabled])'))?.focus();
  };

  function draw() {
    const focused = /** @type {HTMLElement | null} */ (doc.activeElement)?.dataset?.menu ?? null;
    host.replaceChildren();
    for (const group of menuRows(model, args)) {
      const wrap = doc.createElement('div');
      wrap.className = 'web-menu-group';
      const top = doc.createElement('button');
      top.type = 'button';
      top.className = 'web-menu-top';
      top.dataset.menu = group.id;
      top.setAttribute('role', 'menuitem');
      top.setAttribute('aria-haspopup', 'menu');
      top.setAttribute('aria-expanded', String(openId === group.id));
      top.textContent = group.label;
      top.addEventListener('click', () => (openId === group.id ? close() : open(group.id, false)));
      top.addEventListener('pointerenter', () => {
        if (openId !== null && openId !== group.id) open(group.id, false);
      });
      top.addEventListener('keydown', (ev) => {
        if (ev.key === 'ArrowDown' || ev.key === 'Enter' || ev.key === ' ') {
          ev.preventDefault();
          open(group.id, true);
        }
      });
      wrap.append(top);
      if (openId === group.id) {
        const list = doc.createElement('ul');
        list.className = 'web-menu';
        list.setAttribute('role', 'menu');
        list.setAttribute('aria-label', group.label);
        for (const row of group.rows) {
          const li = doc.createElement('li');
          li.setAttribute('role', 'none');
          if (row.kind === 'separator') {
            li.className = 'web-menu-sep';
            li.setAttribute('role', 'separator');
          } else if (row.kind === 'head') {
            li.className = 'web-menu-head';
            li.textContent = row.label ?? '';
          } else {
            const b = doc.createElement('button');
            b.type = 'button';
            b.className = 'web-menu-item';
            b.style.setProperty('--depth', String(row.depth));
            b.setAttribute('role', row.kind === 'check' ? 'menuitemcheckbox' : 'menuitem');
            if (row.kind === 'check') b.setAttribute('aria-checked', String(Boolean(row.checked)));
            b.disabled = !row.enabled;
            b.textContent = row.label ?? '';
            if (row.unavailable) b.title = hooks.unavailableText();
            b.addEventListener('click', () => {
              close();
              hooks.onChoose(/** @type {string} */ (row.id));
            });
            li.append(b);
          }
          list.append(li);
        }
        list.addEventListener('keydown', (ev) => {
          const items = /** @type {HTMLElement[]} */ ([...list.querySelectorAll('button:not([disabled])')]);
          const at = items.indexOf(/** @type {HTMLElement} */ (doc.activeElement));
          if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp') {
            ev.preventDefault();
            const step = ev.key === 'ArrowDown' ? 1 : -1;
            items[(at + step + items.length) % items.length]?.focus();
          } else if (ev.key === 'Escape') {
            ev.preventDefault();
            ev.stopPropagation();
            close();
            /** @type {HTMLElement | null} */ (host.querySelector(`[data-menu="${group.id}"]`))?.focus();
          }
        });
        wrap.append(list);
      }
      host.append(wrap);
    }
    if (focused) /** @type {HTMLElement | null} */ (host.querySelector(`[data-menu="${focused}"]`))?.focus();
  }

  doc.addEventListener('pointerdown', (ev) => {
    if (openId !== null && !host.contains(/** @type {Node} */ (ev.target))) close();
  });
  // A click into the map (a frame) never reaches this document; the window losing focus is what tells.
  doc.defaultView?.addEventListener('blur', close);

  draw();
  return {
    /** @param {{ labels: Record<string, string>, hasProject: boolean, state: any }} next */
    update(next) {
      args = { labels: next.labels ?? {}, hasProject: Boolean(next.hasProject), state: next.state ?? {} };
      draw();
    },
    /** The language the page shows, as the last `set_menu` said. */
    locale: () => String(args.state?.locale ?? 'en'),
  };
}
