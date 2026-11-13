# The web variant

docmap-desktop as one HTML file, `docmap.html`, opened from disk in Chrome or Edge. No installation, no program, no
server and no request to anywhere: the window is the page of the app as it is, the engine of docmap.nvim runs inside
the page, and a project is a folder you pick in the browser. It exists for a machine on which nothing may be
installed or run; where the app can be installed, the app does more.

## Table of content

- [Getting it](#getting-it)
- [Using it](#using-it)
- [What is different from the app](#what-is-different-from-the-app)
- [Limits](#limits)
- [What the page can and cannot reach](#what-the-page-can-and-cannot-reach)
- [How it is built](#how-it-is-built)
- [Building and testing](#building-and-testing)

## Getting it

The built page lies in the repository `WKDSteve/docmap-web` with its checksum (`SHA256SUMS`) and the notices of what
it bundles:

```
git clone https://github.com/WKDSteve/docmap-web
```

Open `docmap.html` from that folder in Chrome or Edge (`file:///C:/repos/docmap-web/docmap.html`). `git pull` brings
a newer one. The page says its build under **Help > About**.

To build it yourself, see [Building and testing](#building-and-testing): the result is `web/dist/docmap.html`.

## Using it

1. **Add project…**, then **Choose a folder…**: the browser's own folder dialog opens. The page asks for the folder
   read only.
2. A folder that is a checkout (it has a `.git`) becomes a project. Any other folder lists the checkouts inside it
   with a tick box each, as in the app.
3. A project without a map is generated at once. A project whose folder already holds a map (`docs/map`, or the
   folder set in its project settings) shows that map as it is; **Generate map** makes a new one from the sources.
4. Projects and workspaces are remembered by the browser (IndexedDB of this page). After a restart of the browser it
   asks for each folder again: the strip under the menu bar then shows **Allow access**, and one click reads the
   folders. Generating a map asks too, so that click is often all it takes.

The menu bar at the top is the menu of the app, drawn in the page. A folder that changed on disk is read again when
you come back to the tab and every half minute; a map older than its sources gets the mark it has in the app.

## What is different from the app

The strip under the menu bar says this on the page itself (**What is different here**).

| | The app | The web variant |
|---|---|---|
| A project | a path on disk | a folder picked in the browser, read only |
| A generated map | written into the project (`docs/map`) | kept in memory of the tab; nothing is written into the folder, and it is gone when the tab closes |
| Function-level facts (functions, calls, complexity) | with the tree-sitter grammars | not in a map generated here: the page carries no parser. A map that lies in the folder has what it was generated with |
| History, checklist status | from git, through the local server | not there: no git. The map's own text for a page opened from disk applies |
| Telemetry, loaded modules | from a Neovim session's data | not there: those files are outside the folder |
| Rule runs with an agent | through ai-desktop | not there |
| Statistics pane | yes | not offered |
| Open in editor, reveal in file manager, copy path | yes | not offered: the browser does not tell a page where a folder lies |
| Add from a URL (clone), from a Neovim config, GitHub lists, traffic | yes | not offered: no git, no network |
| Documentation links, feedback, export of a diagram | yes | switched off in the menu |
| Keyboard shortcuts of the menu | yes | none: the keys belong to the browser in a tab |
| Engine | the sidecar, replaceable in the settings | the commit of `engine.lock`, inside the page |

Every command the window sends has an answer in the page or a refusal that names it (`Not part of the web variant:
<command>`); `web/test/backend.test.mjs` holds the list of commands of `src/main.js` against that.

## Limits

- **Browser:** the File System Access API, so Chrome or Edge. Another browser shows a line that says so.
- **Size of a project:** up to 20 000 files and 256 MiB are read; a larger folder is refused as a whole (half a
  project would be a wrong map). A single file over 4 MiB is left out and the strip says how many were.
- **What is read:** everything the engine walks. The folders it never walks (`node_modules`, `target`, `dist`,
  `build`, `.git`, ... the list of `core/scan.lua`) are not read at all.
- **Memory:** every project that was opened stays loaded until the tab closes.
- **Names:** a file or folder whose name could not be a path segment (a separator in it, a trailing dot or space)
  is left out and counted.

## What the page can and cannot reach

- The page carries a Content-Security-Policy that forbids every request (`connect-src 'none'`, `default-src 'none'`).
  Scripts and styles are inline, the engine's worker and the map's frame are blobs the page makes itself, fonts and
  the icon are data in the file. The build refuses a page whose shell names an address or loads a file, and the
  browser tests open the built file and count its requests (none).
- A folder is opened read only. The page has no code that writes to one.
- What is stored: the list of projects with the browser's handles of their folders, in the IndexedDB database
  `docmap-web`. No content of a folder is stored; a generated map is not stored either.
- Every HTML file opened from disk shares the origin `file://`, and with it IndexedDB. Another local page can
  therefore read this list (the names of the folders and their handles, which still need your permission after a
  restart of the browser). Keep the page in a folder of its own, and do not open HTML files you do not trust from
  disk in the same browser profile.
- The map is a page generated from your project and shown in a frame. It runs its own scripts, under the same
  policy: it cannot reach the network either.

## How it is built

| Part | Where |
|---|---|
| The page of the app, unchanged | `src/` (two places in `src/main.js` ask the web variant by name) |
| The bridge: `window.__TAURI__` answered in the browser | `web/src/shell.js`, `web/src/backend.js` |
| The menu bar, from the table in `src-tauri/src/menu.rs` | `web/menu-model.mjs`, `web/src/menubar.js` |
| Reading a picked folder | `web/src/folder-read.js` |
| The engine: docmap.nvim's standalone pipeline on Lua 5.4 in WebAssembly (wasmoon), in a worker | `web/src/engine.js`, `web/src/lua/host.lua`, `web/src/worker.js` |
| The Lua sources, read from git at the commits of `engine.lock` | `web/engine-bundle.mjs` |
| Files of a project inside the VM: tree, search, languages, freshness | `web/src/vmfs.js` |
| Packing into one file, and the check that its shell loads nothing | `web/build.mjs` with `appkit-web/pack` |

The pattern is the one of tasks-desktop's web variant (`tasks.html`), with the parts of appkit's `web-kit`.

The engine in the page is the engine of the sidecar: the same `standalone/vim_shim.lua` and `lua/docmap/core`. For
the same folder it writes the same bytes as the sidecar does without grammars;
`web/test/sidecar-compare.test.mjs` compares them in CI.

## Building and testing

Needs Node 22 or later, pnpm, git, and checkouts of docmap.nvim and lib.nvim that have the commits of `engine.lock`
(`REPOS_DIR` names the folder both lie in, or `DOCMAP_ENGINE_DIR` and `LIB_NVIM_DIR` name each).

```
cd web
pnpm install --frozen-lockfile
pnpm test          # the engine in the VM, the commands of the window, the build (node --test)
pnpm build         # web/dist/docmap.html and SHA256SUMS
pnpm e2e           # the built file, opened from disk in the Chrome of the machine (Playwright)
```

`DOCMAP_WEB_SIDECAR=<path to the docmap binary> pnpm test` also runs the comparison with the sidecar.

CI runs all of it on every push (job `web` of `ci.yml`). `web.yml` builds on a tag (`v*`) or by hand, runs the same
tests and commits `docmap.html`, `SHA256SUMS` and `THIRD-PARTY-NOTICES.md` to `WKDSteve/docmap-web`.

To move the engine of the page, move `engine.lock` (its header says how) and, when the engine needs a newer lib.nvim,
its `lib_commit`.
