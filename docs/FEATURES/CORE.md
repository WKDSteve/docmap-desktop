# Core

The workspace model: what a project is here, how several of them are kept
together, and the one question a single repository cannot answer about
itself.

## Adding a project

Three ways in, all landing on the same thing — a directory on disk plus the
map directory that belongs to it. A local path, a GitHub URL (cloned into the
app's own `repos/`), or the Neovim tab, which adds the Neovim config itself
and reads its plugin list rather than making you name each one. The Lua that
asks the config for that list (`NVIM_EXPORT_LUA`) ships with this app: it used
to be a script inside the config, and when that was deleted there the tab
failed on every machine with an error nobody connected to it.

Nothing is stored inside the project you add. Everything the app knows lives
in its own config directory, so adding a repository never writes to it.

- **Module:** `src/main.js` (the add dialog), `src-tauri/src/github.rs` (the URL tab, via the GitHub CLI)
- **Usercmds:** File → Add project…
- **Docs:** [USAGE.md](../USAGE.md#adding-a-project)

## Bulk import from a parent folder

The Folder tab's single button only ever added the directory you picked. If
that directory is not itself a repository, `inspect_folder` says so and
lists its immediate subdirectories instead — each flagged for a `.git` entry
and for already being in the workspace — and the dialog swaps the button for
a checklist. **Add selected** calls `import_many`, which adds every checked
path under one lock-read-write of `workspace.json` — not one per directory —
so a folder of thirty-two plugins costs the same one write as a folder of
one; a single bad entry still does not fail the rest of the batch.

No auto-generate loop follows a bulk add — the same restraint
`import_from_nvim_config` already applies, for the same reason: thirty
sequential engine runs would block the window, and **Generate the
out-of-date ones** already exists to catch every project left without a map.

- **Module:** `src-tauri/src/import.rs` (`inspect_folder`, `import_many`), `src-tauri/src/nvim.rs` (`import_from_nvim_config`) and `src-tauri/src/workspace.rs` (`add_project`; all three wrap `add_one`, the in-memory add neither does its own disk I/O for), `src/main.js` (the checklist)
- **Usercmds:** File → Add project… → Folder → pick a non-repository directory
- **Docs:** [USAGE.md](../USAGE.md#adding-a-project)

## Auto-generate on add

A project added without a map gets one generated immediately — and only then.
It never overwrites an existing map, which is what separates it from every
other generate command.

- **Module:** `src/main.js`
- **Config:** none — it fires exactly when there is nothing to lose

## Workspaces

Named project lists, switched as a unit. One `workspaces/<name>.json` each,
with `workspace.json` recording which is active and the settings shared
across all of them.

A workspace name is typed by a person and then becomes a path, so it is
reduced to alphanumerics, `-`, `_` and space before it is used as one.

- **Module:** `src/main.js`, `src/lib/last-selection.js` (which project a workspace was left on)
- **Docs:** [USAGE.md](../USAGE.md#workspaces)

## The project picker

A native `<select>`, deliberately: it already answers `↓`/`↑`, `Home`/`End`,
`Enter` and type-ahead the way the platform does, and none of that can drift.
A hand-rolled list would have to re-earn all of it.

`Delete` is the one key it does not answer, on purpose — removal lives on
**File → Remove from workspace**, which also names what it removes. A bare
Delete key over a list of repositories was the more frightening of the two.

- **Module:** `src/index.html`, `src/main.js`
- **Keymaps:** the platform's own `<select>` behaviour; right-click opens the same per-project commands as a context menu
- **Docs:** [USAGE.md](../USAGE.md#the-project-picker-and-how-to-sort-it)

## Five sort orders, named for the question

Each order is named for what it answers rather than the field it sorts on,
because nobody sorts by a timestamp. The fifth, **Most traffic**, reads GitHub
traffic (see [TRAFFIC](TRAFFIC.md)). The control is the same setting as
**Order projects by** in Settings — changing either moves both.

- **Module:** `src/main.js`
- **Config:** `workspace.json`, shared with Settings → Behaviour
- **Docs:** [USAGE.md](../USAGE.md#the-project-picker-and-how-to-sort-it)

## Cross-project dependencies — who would find out if I changed this

The workspace-level answer no single repository has. Every map records
`requires_external`: a module required from outside that repository, about
which the engine can say nothing more, because it never saw where that module
lives. Several maps together can, and a workspace is the only place several
exist.

**The matching is exact, not clever**, and that was measured rather than
assumed: across 30 generated maps there were 1,820 declared module names and
not one claimed by two repositories, so a hit is a fact. The obvious
fallback — walking down to the longest declared prefix — was written for that
measurement and resolved *zero* additional names, so it is not in the app. Of
1,175 external requires, 852 resolved and 323 did not; the 323 are the answer
working rather than failing, since `telescope`, `fzf-lua` and friends are not
in your workspace and have no map to be found in.

Two numbers, not one: *used by five projects* and *in 197 places* answer
different questions, and the interesting cases are where they disagree — one
project reaching for something sixty times is a coupling, twenty projects
reaching once each is a convention.

Nothing here asks the engine anything. It is all read from artifacts on disk,
so it works in a workspace whose engine is not configured at all.

- **Module:** `src-tauri/src/deps.rs` (computed from the artifacts), `src/lib/deps.js` (shaping it into the direction people ask it in)
- **Docs:** [USAGE.md](../USAGE.md#what-depends-on-what)

## The dependency matrix

The same edges as a picture instead of a list — **View as matrix…** in the
Dependencies panel opens a dialog of its own, off the default path, rather
than folding a grid into the overview. Rows require columns, a cell is
call-site count (shaded by how heavy — never fully opaque, so even the
darkest cell keeps its number legible), and pointing at a cell names the
modules behind it. Asks the engine nothing a second time: it reads the same
`workspace_deps` result the panel already fetched.

An adjacency matrix rather than a node-link graph, on purpose — no layout
algorithm to write and maintain for a workspace-sized win a grid already
delivers, and a grid stays readable at the sizes this app actually sees
where a tangle of crossing arrows would not.

- **Module:** `src/main.js` (`renderMatrix`), `src/index.html` (`#matrixbox`)
- **Usercmds:** the Dependencies panel under the workspace overview → View as matrix…

## Language detection per project

What a directory is written in, counted before anything else runs, so a
project is described by its contents rather than by what was assumed about
it.

- **Module:** `src-tauri/src/languages.rs`, presented and cached by `src/lib/languages.js`

## Project icons

A project's own icon, found by following conventions somebody else already
established, rather than asking the user to supply one.

- **Module:** `src-tauri/src/icon.rs`
