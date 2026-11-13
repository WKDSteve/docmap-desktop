# Viewing

The window: what it shows before you pick anything, what it shows after, and
the two panes that read the disk rather than the map.

## The workspace overview

With nothing selected, the main pane lists every project in the workspace at
once, ranked by what wants doing. It is where the app opens, and the picker's
first row — **All projects** — is how you get back to it.

Each row says one sentence about that project's map: *no map yet*, *made by
an older engine*, *changed since it was made*, *up to date*, or *nothing
found, though not everything was checked* — the honest version of "up to
date" when the freshness walk hit its file limit.

**Why that order, and not newest-changes-first.** Because the obvious order
was measured and it lost. Over 54 repositories, 30 with a generated map, 28
were "changed since made" and 27 "made by an older engine" — both fire on
nearly everything, so neither is useful as a count. What separates them is
what they point at: for 17 of those 28 the newest file was a `.gitignore`
touched in one sweep, and excluding generated files changed the number by
exactly zero. A map written by an older engine is missing something concrete,
and regenerating it gets that back. So it leads.

- **Module:** `src/lib/overview.js`, `src-tauri/src/freshness.rs`
- **Usercmds:** View → the overview, or the **All projects** row
- **Config:** Settings → Behaviour → Start with the workspace overview
- **Docs:** [USAGE.md](../USAGE.md#all-projects--the-first-screen)

## The map view

The generated page, embedded. It is the engine's artifact unchanged — this
app is a window in front of it, not a second renderer.

- **Module:** `src/main.js`, `src-tauri/src/server.rs`

## A local server per map

A small HTTP server so the embedded page's own `/api/*` calls resolve. The
page is the same document a browser would open; giving it a real origin is
what lets the parts of it that expect one keep working. That includes the
History tab (`commits`, `commit/<sha>`) and the Hierarchy tab's **Compare
maps** (`shape/<sha>` against the map on disk, `shape/<sha>/<sha>` between
two commits); every sha is checked before it reaches the engine.

- **Module:** `src-tauri/src/server.rs`

## The project bar

A slim bar above the main pane, for a selected project: the **Map / Files /
Statistics** switch on the left and the search box in the middle. It is the
app's own because the page's header belongs to the generated document, which
this window cannot touch. The workspace overview has no bar.

- **Module:** `src/main.js`, `src/index.html`
- **Usercmds:** View → Files on disk, View → Statistics
- **Docs:** [USAGE.md](../USAGE.md#the-project-bar-search-and-statistics)

## Search — text, file names, and what the map shows

One box with a *scope*: a folder of the project (a `grep` for text, or a `find`
for file names) or the **View**, which searches what the map shows by reading
`module_map.json` — the page itself is a separate document this window cannot
read into. Plain substring, no regular expressions; bounded by results, files
and time, and it says which stopped it. The folder cannot leave the project.
A text or file hit opens in the editor at its line; a map hit sends the map
there.

- **Module:** `src-tauri/src/search.rs`, `src/lib/finder.js`, `src/main.js`
- **Usercmds:** `Ctrl+K`
- **Docs:** [USAGE.md](../USAGE.md#the-project-bar-search-and-statistics)

## Project statistics

Files and lines per language, split into code, comments, documentation, data
and blank — counted from the disk when asked, not from the map. A comment-only
line is a comment; code with a trailing comment is code. Generated and vendored
folders are skipped, binaries and very large files counted but not read.

- **Module:** `src-tauri/src/stats.rs`, `src/lib/stats.js`
- **Usercmds:** View → Statistics
- **Docs:** [USAGE.md](../USAGE.md#the-project-bar-search-and-statistics)

## Files on disk

A pane in the app rather than a tab in the map, and that fork was decided by
what the data is: a file tree baked into the artifact would be a snapshot,
wrong the moment somebody adds a file, inside a document whose whole claim is
that it is byte-deterministic. Read live it is always right — and the program
that can read it live is the same one that can open a file for you.

It reads **one directory per call**. A monorepo is tens of thousands of files
and a reader opens a dozen folders; walking everything to draw one level is
work nobody asked for and a window that stalls.

- **Module:** `src-tauri/src/filetree.rs`
- **Usercmds:** View → Files on disk (`Ctrl+Shift+F`)
- **Docs:** [USAGE.md](../USAGE.md#files-on-disk)

## Skipped directories are listed, not hidden

`node_modules` is genuinely on the disk, and a tree that omits it is lying
about the disk to make the map look consistent. So four notes appear, and
they answer two opposite surprises:

| Note | Means |
| --- | --- |
| *its own repository — not scanned* | A nested checkout; the scan stops here |
| *not scanned* | Skipped by name in every repository — `node_modules`, `target`, `dist` and a dozen more |
| *ignored by git — but still mapped* | Your `.gitignore` covers it, and the map walked it anyway |
| *not in git* | On disk, never committed — and therefore in the map like anything else |

The first two explain a folder that is on screen and **not** in the map; the
last two explain the reverse. *ignored by git* is the one nothing else in the
window could tell you: the scan does not read `.gitignore`, because a
repository can reasonably ignore a directory the map should still describe.

The two git notes are read from `git status` once per directory listed, not
once per file. **A project that is not a git repository shows neither** —
absence of git is not evidence about a file, and calling every file in a
non-repository "not in git" would be confidently wrong about all of them.

- **Module:** `src-tauri/src/filetree.rs`
- **Docs:** [USAGE.md](../USAGE.md#files-on-disk)

## Open a file where an entity lives

The map's own right-click menu carries **Open in editor** beside *Open
source*, with the path and — on a function — its line. Offered only when the
map is embedded here, because a browser cannot start an editor and a menu
item that does nothing is worse than an absent one.

**The path is resolved and bounds-checked before anything opens.** It arrives
repo-relative, and the message comes from a document this app embeds but did
not author — a map from an older engine, or one somebody else produced.
`../../` in a path is the difference between opening a file and opening any
file, so a resolved path that does not start with the project root is
refused.

- **Module:** `src/main.js`
- **Config:** Settings → Editor — a command template with `{file}` and `{line}`, split into arguments *before* substitution so a path with a space stays one argument. Empty is a real answer: it hands the file to the desktop.
- **Docs:** [USAGE.md](../USAGE.md#opening-a-file-where-an-entity-lives)

## Notes over panels that cannot have data

Three panels of the generated page can be empty for a reason that is not the
project's. A note explains which, instead of leaving a blank pane to be read
as a verdict — two of them permanently, because this app's engine cannot ever
produce that data no matter how it is configured.

- **Module:** `src/main.js`
- **Docs:** [USAGE.md](../USAGE.md#the-note-that-appears-over-some-panels)

## Telemetry and Loaded panels

The live half, whenever a Neovim session collected it: call-count telemetry
read out of `runtime-analysis.nvim`'s cache, with its snapshots switchable.
Static map always; live panels when the data exists.

- **Module:** `src-tauri/src/telemetry.rs`
- **Config:** Settings → Telemetry
