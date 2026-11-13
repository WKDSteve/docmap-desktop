# Desktop

The parts that make this a program rather than a page: a menu bar, the
platform's own input behaviour, and the things only a native host can do.

## The menu bar

Everything this window does, in one place — File, Project, View, Help. The
sidebar keeps the two things a menu is wrong for: the project you are looking
at, and the verdicts that decide whether the next action works.

Project items are greyed when nothing is selected.

- **Module:** `src-tauri/src/menu.rs`
- **Usercmds:** the full table in [USAGE.md](../USAGE.md#the-menu-bar)

## An auto-hiding sidebar

The sidebar can be **pinned** (the default) or fold to a thin edge and open as
an overlay while the pointer is on it, so the map gets the whole width and the
controls are one hover away. Toggled with the pin in its corner or **View →
Auto-hide sidebar**; remembered per machine. It also opens on keyboard focus,
and does not close while a dropdown inside it is in use. `Ctrl+B` still hides it
altogether.

- **Module:** `src/main.js`, `src/style.css`
- **Usercmds:** the pin, View → Auto-hide sidebar, View → Sidebar (`Ctrl+B`)
- **Docs:** [USAGE.md](../USAGE.md#the-window)

## Keyboard shortcuts

`Ctrl+N` add project · `Ctrl+Shift+O` open map in browser · `Ctrl+E` export ·
`Delete` remove from workspace · `Ctrl+Shift+W` workspaces · `Ctrl+,`
settings · `Ctrl+G` generate · `Ctrl+Shift+G` generate all · `F5` regenerate
and reload · `Ctrl+plus`/`Ctrl+-`/`Ctrl+0` zoom · `Ctrl+Shift+F` files on
disk · `Ctrl+B` sidebar · `Ctrl+K` search.

- **Module:** `src-tauri/src/menu.rs`
- **Docs:** [USAGE.md](../USAGE.md#the-menu-bar)

## Keyboard navigation, borrowed rather than built

The project picker is a native `<select>`, so `↓`/`↑`, `Home`/`End`, `Enter`
and type-ahead behave the way the platform does — more than a hand-rolled
list would answer, and none of it can drift.

`Delete` is the one key it does not answer, deliberately: removal lives on
**File → Remove from workspace**, which also names what it removes.

- **Module:** `src/index.html`, `src/main.js`
- **Docs:** [USAGE.md](../USAGE.md#keyboard-navigation)

## Context menu on the project block

Right-clicking the detail block under the picker opens the same per-project
commands, including **Reveal in file manager** and **Copy project path** —
the latter because the path is on screen and unselectable, which is the whole
reason it exists.

- **Module:** `src/main.js`
- **Docs:** [USAGE.md](../USAGE.md#keyboard-navigation)

## Export the current view

**File → Export current view…** (`Ctrl+E`) saves the diagram the map is
showing as a standalone SVG.

The map is a separate document from a separate origin, so this window cannot
read into it — **it asks, and the page answers.** The page takes no
instructions through that channel, only questions about itself: a host that
could tell an embedded page what to do is a different kind of program than
one that can ask it what it is showing.

Only Hierarchy draws a diagram. On any other tab this says so rather than
writing a file of the last diagram that happened to be drawn.

- **Module:** `src/main.js`
- **Usercmds:** File → Export current view… (`Ctrl+E`)
- **Docs:** [USAGE.md](../USAGE.md#export-the-current-view)

## About — including the engine's own build

**Help → About docmap** answers "which versions am I running", including the
engine's: the commit it was built from, when, and whether that tree was
clean. A binary built from a modified tree carries a commit that does not
describe it, so About says so rather than quoting a sha that would send a
reader to the wrong diff.

- **Module:** `src-tauri/src/deps.rs`, `src/main.js`
- **Usercmds:** Help → About docmap
- **Docs:** [USAGE.md](../USAGE.md#about)

## Sending feedback

Turning "this should be better" into a filed report, from inside the app.

- **Module:** `src-tauri/src/feedback.rs`
- **Usercmds:** Help → Send feedback…
- **Docs:** [USAGE.md](../USAGE.md#sending-feedback)

## Interface language

The app's own UI language, switchable from View → Language and from
Settings → Appearance. Error messages from the backend are translated too:
a failing command answers with a catalog key, named parameters and the
untranslated detail (the OS error, `git`'s or `gh`'s output), never with an
English sentence of its own.

- **Module:** `src/lib/i18n.js`, `src/lib/errors.js`,
  `src-tauri/src/apperr.rs`
- **Config:** Settings → Appearance → interface language

## One look with the other desktop apps

Colours, type and shapes come from appkit-ui, the kit tasks-desktop and
rules-desktop are built on, so the three windows look alike. This window has
no package manager, so the kit's token file is a checked-in copy pinned to
one tag; CI fails when the copy drifts from that tag or when a colour is
written anywhere but in the tokens. Light, dark and the explicit choice in
View → Theme behave as in the other apps.

Kept on purpose, not aligned:

- **JetBrains Mono, bundled** (with its OFL licence) in front of the kit's
  monospace stack: paths have to be legible, `Il1` and `O0` included.
- **The colours of the icon.** `palette.css`, generated by the tool of the
  app icons and loaded after the kit's tokens, sets them in the violet of
  docmap's icon (light and dark); the kit's blue is gone here.
- **No meaning colours.** The kit has green, amber and red; this window
  marks a state with a word and the accent.
- **The theme blocks stay duplicated, as a fallback.** The kit writes each
  colour as `light-dark()`; for a WebView without it,
  `appkit-tokens-fallback.css` (generated from the tokens and the palette) carries the same
  colours as plain values, system choice and `data-theme` included.
- **`--panel` and `--ink`** stay as names, as aliases of the kit's
  `--surface` and `--text`.

- **Module:** `src/appkit-tokens.css`, `src/palette.css`, `src/appkit-tokens-fallback.css`, `src/style.css`
- **Config:** `tools/appkit-ui.pin` (the tag); `tools/check-appkit-tokens.py`, `tools/check-colors.py`

## Busy buttons

One place for "disable, swap label, run, restore", instead of that pattern
hand-rolled at every call site that starts something slow.

- **Module:** `src/lib/busy-button.js`

## The web variant

The same window as one HTML file opened from disk, for a machine on which
nothing may be installed: no program, no server, no request. The page of the
app is taken as it is; a shell answers its commands in the browser, the
engine runs in a Lua VM in a worker, and a project is a folder picked in the
browser and read only. A map is generated in memory; one that lies in the
folder is shown as it is. What needs a program is not offered, and the page
says so in a strip under its menu bar. [`WEB.md`](../WEB.md) is the manual.

- **Module:** `web/src/shell.js` (the bridge and the strip), `web/src/backend.js` (the commands), `web/src/menubar.js`, `web/src/engine.js` with `web/src/lua/host.lua` and `web/src/worker.js` (the engine), `web/src/folder-read.js`, `web/src/vmfs.js`
- **Config:** `engine.lock` (`commit`, `lib_commit`: the sources in the page); `web/build.mjs` (the parts and the policy of the file, `pnpm build` writes `web/dist/docmap.html`); `.github/workflows/web.yml` publishes it to `WKDSteve/docmap-web`
- **Tests:** `web/test/` (Node: the engine in the VM, the commands, the build), `web/e2e/` (the built file opened from disk in Chrome)
- **Docs:** `docs/WEB.md`
