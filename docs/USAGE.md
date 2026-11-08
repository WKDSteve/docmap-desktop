# Using the app

`README.md` says what the program is for. This is the second thing: what each
button, pane and indicator actually does, so using it does not mean reading
`main.js` first.

Rewritten 2026-08-19 from an inventory of what the app actually has, rather
than patched where it had gone false. Everything below was checked against
the code, not recalled.

## Table of content

- [The one thing to know first](#the-one-thing-to-know-first)
- [Adding a project](#adding-a-project)
- [Workspaces](#workspaces)
- [The window](#the-window)
- [The project bar, search and statistics](#the-project-bar-search-and-statistics)
- [Files on disk](#files-on-disk)
- [Opening a file where an entity lives](#opening-a-file-where-an-entity-lives)
- [Generate, Generate all, Generate the out-of-date ones](#generate-generate-all-generate-the-out-of-date-ones)
- [The staleness mark](#the-staleness-mark)
- [GitHub traffic](#github-traffic)
- [Settings](#settings)
- [Project settings](#project-settings)
- [The engine indicator](#the-engine-indicator)
- [The note that appears over some panels](#the-note-that-appears-over-some-panels)
- [Keyboard navigation](#keyboard-navigation)
- [Where things live](#where-things-live)
- [The menu bar](#the-menu-bar)
- [Sending feedback](#sending-feedback)
- [Limits of a rules run through ai-desktop](#limits-of-a-rules-run-through-ai-desktop)
- [What this app is not documenting](#what-this-app-is-not-documenting)

---

## The one thing to know first

**A generated map is a snapshot of the engine that wrote it.** The map is a
self-contained HTML document produced by `docmap.nvim`'s engine at
the moment you pressed Generate — it is not rendered live by this window.

The consequence catches everyone once: **updating the app does not change
an existing map, and updating the engine does not either.** A page feature
that shipped after your map was written arrives by *regenerating that
project*, not by installing a newer anything. If a map looks older than the
release notes say it should, the answer is **Project → Regenerate and
reload** (`F5`).

What updating the app *does* change is this window: the sidebar, the menu,
the panes, the settings. Those are ours. Everything inside the map is the
engine's, frozen at generation time.

## Adding a project

**File → Add project…** (`Ctrl+N`) opens one dialog with three tabs, because
the three are alternatives rather than steps.

**Folder** — pick any directory on this machine. It does not need a
`docs/map` already. The project is identified by its **canonical, resolved
path**, so adding the same directory twice (even by a different relative
route to it) is a no-op rather than a duplicate entry.

If the project already has a map, that map opens immediately. If it does
not, and an engine is configured, generation starts automatically — but
**only because there was nothing to overwrite**. See
[Generate](#generate-generate-all-generate-the-out-of-date-ones) for why
that condition matters and does not extend to a project that already has
one.

**Pick a folder that is not itself a repository — `$REPOS_DIR` holding
thirty-two plugins, say — and a checklist replaces the single button**: one
row per immediate subdirectory, name-sorted, each marked whether it has a
`.git` entry and whether it is already in the workspace (ticked and
un-editable, so re-scanning a folder you have mostly added already does not
ask you to click past what is already there). Everything with `.git` is
pre-ticked; **Select all** / **Select none** cover the rest, including a
plain folder with no `.git` — not everything worth mapping is a checkout.
**Add selected** adds them the same way the single-folder path does, one at
a time so that one bad entry does not fail the rest, and reports how many of
how many were added. Like the Neovim tab, added projects are **not**
auto-generated in bulk — thirty-two sequential engine runs would block the
window for no reason when [Generate the out-of-date
ones](#generate-generate-all-generate-the-out-of-date-ones) already covers
exactly this case afterwards.

**Neovim** — the Neovim config itself plus every personal plugin it
declares, added in one step instead of one folder picker per plugin. It runs
your config headless and asks it which plugins are enabled and checked out on
this machine — the same `plugins.personal.core.export.projects()` list
`:MyPlugins` and `:DocMapAll` read, the real, currently-active list rather than
a guess at a policy table — then adds the config directory and each plugin
exactly as the Folder tab would. Plugins that are not checked out here are
skipped, not cloned. Nothing else about the config is read, and nothing is
changed. The Lua it runs is part of this app, so the config needs no script
of its own; a config without that module gets an error that says so. It waits
at most 60 seconds: a configuration that stops to ask something ends in an
error saying so, and the dialog is usable again.

The result is written into the tab itself, not only the status line: how
many were found, how many newly added, how many were already in the list, and
which failed, with the reason per project. "Nothing found" and "everything is
already in the list" are sentences of their own, and the dialog stays open
for them (and for an error); it closes only when the list grew. Already
present is not a failure. Like the folder checklist, the added projects are
not generated in bulk.

This tab needs two things resolved, shown in the **Neovim** section of
Settings: the `nvim` binary (found on `PATH`, or **Locate nvim…**) and the
config directory to run it against (the OS-conventional location by default
— `%LOCALAPPDATA%\nvim` on Windows, `~/.config/nvim` elsewhere — or
**Locate config…**). A configured path that stops existing falls back to
searching again rather than failing later with a raw error.

**URL** — a repository URL, cloned shallow (history is not needed, only the
current tree) and added the same way. Clones land in a `repos/` directory
inside the app's own config directory, not somewhere temporary, so
reopening the app later still finds them. A URL that was already cloned is
reused rather than re-cloned.

This app holds no credentials: whatever a plain `git clone` of that URL
needs on this machine (an HTTPS credential helper, an SSH agent) is exactly
what runs here, unchanged — with one exception: git is never allowed to *ask*
(`GIT_TERMINAL_PROMPT=0`), because there is no terminal to answer on, and a clone
gives up after 15 minutes. A clone that fails shows git's own error, not a
guess at what went wrong.

Only the forms a hosting page gives you are accepted: `https://`, `http://`,
`ssh://`, `git://` and `user@host:path`. A URL that starts with `-`, names a
remote helper (`ext::…`, `fd::…`), points at a local path or contains a space
is refused before git is started, and git itself is limited to those same
transports. The folder in `repos/` is the last part of the URL followed by
`-` and eight hex digits of a hash of the URL, e.g. `foo-c2edc6b0` for
`https://github.com/a/foo`: two repositories of the same name
(`github.com/a/foo` and `github.com/b/foo`) get two folders and become two
projects. The project itself is named after the repository, `foo`, not after
its folder; when a project named `foo` is already listed, the second one is
named `b/foo` (owner and name), so the two can be told apart.

One repository is cloned once, however its URL is spelled: `https://`,
`http://`, `ssh://`, `git://` and `git@host:owner/name` all name the same
repository, as do a trailing `/` or `.git`, a user or token in the URL, and
the host in any case. On GitHub, GitLab, Bitbucket and Codeberg the owner and
name are compared in any case too (`github.com/Owner/Name` is
`github.com/owner/name`); on other hosts their case counts. A port counts
only where it is not the usual one (an ssh port never counts). A folder is
reused only when its `origin` is this repository, so a URL that happens to
get the same hash as another gets a folder of its own (`foo-c2edc6b0-2`)
instead of opening the other one. A clone made by an earlier version, in a
folder named only `foo`, is still found and used where it is, but only for
the repository its `origin` points at; any other `foo` gets a folder of its
own.
The last part of the URL must be a plain name (letters, digits, `.`, `_`,
`-`). A name that ends in `.` or that Windows opens as a device (`CON`,
`PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, with or without an
extension) is refused on every platform.
An IPv6 address in a URL (`https://[::1]/repo.git`) is fine.

The URL tab can also list **your own GitHub repositories** to pick from,
behind a button rather than on open — listing them is a network call
against your account, and a dialog that made one just for being looked at
would be doing something you did not ask for. It goes through the **GitHub
CLI** (`gh`), so this app never holds a token. Without `gh`, or without
being signed in, the list says which of the two it is and the URL field
beside it keeps working.

## Workspaces

A workspace is **a set of projects and nothing else**. Not a theme, not a
language, not a zoom level, not the engine path — those are properties of
this machine's eyes and this machine's disk, and carrying them per
workspace would change your lighting when you switch project sets.

**File → Workspaces…** (`Ctrl+Shift+W`) opens the dashboard: every
workspace, with its project count, and the controls to create, rename,
delete and switch.

* **Switching creates.** There is no separate *New* verb, because switching
  to a name that does not exist is the same action.
* **Deleting refuses the last one**, and says what it removes: the list,
  never a repository on disk.
* **Switching clears the selection**, the map, and the cached freshness and
  page counts — a selection from the old workspace would leave the sidebar
  naming one project while the map showed another.
* **Each workspace remembers what it was left on**, and lands there when you
  arrive. A workspace you have never opened lands on nothing: the picker says
  *Pick a project…* rather than naming one you did not choose. An
  installation from before this had one remembered project; it belongs to
  whichever workspace was open then, and is filed under that one on the next
  start rather than dropped.

**When the dashboard appears at startup**: whenever there is more than one
workspace. With a single workspace you never see it — a chooser with one
row is a click in front of the thing you wanted. The **Don't show this
again** checkbox exists for the reader who has several and still wants to
land in the last one.

**The window title names the workspace only when there is more than one**,
which is the only time the answer is news. With one workspace the title
stays `<project> — docmap`.

Your existing project list was migrated in place the first time this
version read it, into a workspace called `Default`. You were not asked,
because a feature whose first act is losing your project list is not a
feature.

## The window

| Part | What it is |
|---|---|
| **Sidebar** | The project picker, the per-project detail block, and the Engine and Neovim panels. Hidden with **View → Sidebar** (`Ctrl+B`). The pin in its corner (or **View → Auto-hide sidebar**) lets it fold to a thin edge and open as an overlay while the pointer is on it. |
| **Project bar** | Above the main pane for a selected project: the **Map / Files / Statistics** switch and the search box — see below. |
| **Main pane** | The generated map, embedded. Everything inside it is `docmap.nvim`'s surface, not this app's. With nothing selected it shows the whole workspace instead — see below. |
| **Files pane** | The live file tree — see below. **View → Files on disk** (`Ctrl+Shift+F`). |
| **Statistics pane** | What the project is made of, counted now. **View → Statistics**. |
| **Status bar** | Spans the bottom, carrying the project path and the progress of anything long-running. |

**Zoom** is in **View** (`Ctrl+plus`, `Ctrl+-`, `Ctrl+0`). The map is a
dense page and this is the single most useful thing a menu bar adds to it.

### All projects — the first screen

With nothing selected, the main pane lists **every project in the workspace
at once**, ranked by what wants doing. It is where the app opens, and the
picker's first row — **All projects** — is how you get back to it after
opening a map.

Each row says the project's name and one sentence about its map:

| It says | It means |
|---|---|
| **no map yet** | Nothing has been generated for it. Nothing to read, and nothing behind either. |
| **made by an older engine** | There is a map, but a newer engine is installed now. The page is built at generation time, so anything the engine learned since is simply not in it. |
| **changed since it was made** | Files in the repository are newer than the map. |
| **up to date** | Nothing found. |
| **nothing found, though not everything was checked** | The freshness walk hit its file limit, so this is the honest version of *up to date*. |

**Why that order, and not newest-changes-first.** Because the obvious order
was measured and it lost. Over the author's own tree — 54 repositories, 30
with a generated map — 28 were "changed since made" and 27 were "made by an
older engine". Both fire on nearly everything, so neither is useful as a
count; what separates them is *what they point at*. For 17 of those 28 the
newest file was a `.gitignore` touched in one sweep, and excluding generated
files changed the number by exactly zero. A map written by an older engine,
by contrast, is missing something concrete and regenerating it gets that
back. So it leads.

When any project is behind the engine, a button above the list offers to
remake just those. That is the one bulk action the menu does not already
have: **Generate the out-of-date ones** compares modification times, and a
map built by an older engine is not out of date by that measure — it is
exactly as old as its sources and still missing what the engine can do now.

### What depends on what

Under the list, collapsed, is the one question a single repository cannot
answer: **who would find out if I changed this.**

Every map records `requires_external` — a module that was required from
outside this repository. The engine can say nothing more about it, because
it never saw where that module lives. Several maps can, and the workspace is
the only place several exist. So the names are matched against the modules
other projects *declare*, and each row reads: this project, used by that many
others, in that many places, and which of its modules they reach for.

**The matching is exact, not clever.** Measured over 30 generated maps: 1 820
declared module names, and not one claimed by two repositories — so a hit is
a fact. The obvious fallback, walking down to the longest declared prefix,
was written for that measurement and resolved *zero* additional names, so it
is not in the app. Of 1 175 external requires, 852 resolved and 323 did not.

The 323 are the answer working rather than failing: `telescope`, `fzf-lua`,
`which-key` and friends are not in your workspace and have no map to be found
in. They get their own line, because "this leans on things you do not have
open" is the other half of the question.

**Two numbers, not one.** *Used by five projects* and *in 197 places* answer
different questions, and the interesting cases are where they disagree: one
project reaching for something sixty times is a coupling, twenty projects
reaching once each is a convention.

Nothing here asks the engine anything — it is all read from the artifacts on
disk, so it works in a workspace whose engine is not configured at all.

**View as matrix…**, at the bottom of the panel, opens the same edges as a
grid instead of a list: rows require columns, and a cell is the call-site
count between them, shaded by how heavy — the darkest cell never quite
reaches full opacity, so its number stays readable. Point at a cell for
which modules. Nothing is asked again to open it; the dialog reads the same
data the panel already fetched.

It is a matrix rather than a node-and-arrow graph on purpose: this app ships
no charting library, a force-directed layout would be the one thing that
made writing one worthwhile, and a matrix reads at a glance at the sizes a
real workspace has (measured: 30 projects, 49 edges) where a tangle of
crossing arrows would not.

### The project picker, and how to sort it

A native `<select>`, so it already answers Arrow, `Home`/`End`, `Enter` and
type-ahead the way your platform does. Beside it is a sort control with five
orders, and each is **named for the question rather than the field**, because
nobody sorts by a timestamp:

| Order | Answers |
|---|---|
| **Name** | Alphabetical. The default, and the one you use to find a project you can already name. |
| **Needs regenerating** | Which maps have fallen behind their code — the staleness mark, gathered into an order. |
| **Least recently generated** | Which have you left alone the longest. |
| **Added** | The order you added them in. |
| **Most traffic** | Which projects is anyone looking at: most GitHub views in the last 30 days first (see [GitHub traffic](#github-traffic)). A project with no traffic data comes after every project that has some. |

**The last two are not the same question**, which is the reason both exist.
Staleness cannot answer for a tree that did not move: a repository nobody has
touched stays un-stale forever however old its map is, and those are exactly
the ones worth finding.

Sorting by staleness **measures first** rather than reading whatever happens
to be known — before that was noticed, a fresh window sorted by one entry and
produced alphabetical order while claiming to sort by staleness. It takes a
moment on a long list, and a sort control that appears to work and does not
is worse than one that makes you wait.

Under the picker is the detail block for the selected project: its
languages, its **Map outdated** chip when the sources are newer than the map
(click it for the files that changed; hover for what to do about it), its
GitHub traffic, and the right-click menu (see
[The menu bar](#the-menu-bar)). A project that ships an icon by one of the
conventions other tools already use shows it there, in five places worth
looking and no more: a `manifest.json`'s `icons` array (the W3C standard for
this exact question), `apple-touch-icon.png`, a favicon (`.svg` before `.png`
before `.ico`), an Android launcher icon, an iOS app icon set. **Nothing
matches for most repositories** — a Neovim plugin has no icon and is not
supposed to — and nothing is exactly what they get. An absent icon is a real
answer, not a missing one, so nothing is invented to fill the space.

## The project bar, search and statistics

With a project selected, a slim bar sits above the main pane. It is this
app's, not the map's: the page's own header belongs to the generated document,
which this window cannot touch, so what the window adds sits on top of it. The
workspace overview has no project to search and shows no bar.

**Left: the view.** **Map**, **Files** and **Statistics** — which of the three
fills the main pane. *Files* is the same pane **View → Files on disk** opens;
the map keeps its page underneath, so coming back costs no reload.

**Middle: search.** One box, three questions, chosen by its **scope**:

| Scope | Asks | Answered by |
|---|---|---|
| **Folder**, *Text* | Which lines under this folder contain the words — a `grep`. | Walking the disk. |
| **Folder**, *File names* | Which files have these words in their path — a `find`. | Walking the disk. |
| **View** | What the map shows: names, paths, summaries, signatures, parameters, documentation, features. | `module_map.json` next to the page. |

Focusing the box opens the panel below it. Its first row is the **scope**: the
folder (the project root until you change it — type a path, or **Choose…**, which
opens inside the project rather than wherever the system dialog last was) and a
selector for *Folder* or *View*. A folder outside the project is refused, and so
is `..`. Matching is a plain, case-insensitive substring (tick **Match case** to
change that); there are no regular expressions, because a pattern can be made to
run for a minute and the box promises something simpler. For file names every
word has to be somewhere in the path, so `lua init` finds `lua/foo/init.lua`.

Results appear as you type, after a short pause. **Enter** searches at once,
**↓** moves into the list, **Esc** closes. `Ctrl+K` focuses the box from
anywhere in the window except inside the map (key events do not cross out of the
embedded page). Skipped folders (`node_modules`, `target`, a nested checkout…) and
the map directory are not searched; binary and very large files are passed over;
a search stops after 300 results, a few seconds or forty thousand files and says
which. A file with many matches shows only its first few (and says so). File-name
search ranks every match in the tree before cutting to 300, so the file actually
called `config.lua` beats the twenty files under `config/`.

**Clicking a result** goes where the result lives. A text or file match opens in
your editor at that line. A *View* match sends the map to it — the Index → Tree
view with that module selected — and **Open file** on the row opens its source
instead. Documentation pages have no place in the map and simply open.

**Statistics** counts the project when asked — it opens every file, so it is not
done for thirty projects at start-up. Files and lines per language, and the lines
split into **code**, **comments**, **documentation** (Markdown, reStructuredText,
plain text), **data and config** (JSON, YAML, TOML…) and **blank**. A line that
is only a comment is a comment; code with a trailing comment is code. A Python
docstring is code, because counting it as a comment would need to know the
language better than a line counter does. Generated and vendored folders and the
map are left out; very large and binary files are counted as files and not
read. **Count again** refreshes it. A count ends by itself after twenty seconds or
a gigabyte of reading and says that its numbers are a lower bound.

## Files on disk

**View → Files on disk** (`Ctrl+Shift+F`) is a pane in the app, not a tab
in the map — and that fork was decided by what the data is. A file tree
baked into the artifact would be a snapshot, wrong the moment somebody adds
a file, inside a document whose whole claim is that it is byte-deterministic.
Read live it is always right, and the program that can read it live is the
same one that can open a file for you.

It reads **one directory per call**. A monorepo is tens of thousands of
files and a reader opens a dozen folders; walking everything to draw one
level is work nobody asked for and a window that stalls.

**Skipped directories are listed, not hidden**, and say why. `node_modules`
is genuinely on the disk, and a tree that omits it is lying about the disk
to make the map look consistent. The same goes for a nested checkout —
which is the reason half a tree can be missing from a map, and the one
thing a reader would otherwise have no way to find out.

**Four notes, and they answer two opposite surprises.**

| Note | Means |
|---|---|
| *its own repository — not scanned* | A nested checkout. The scan stops here. |
| *not scanned* | This tool skips this folder by name, in every repository — `node_modules`, `target`, `dist` and a dozen more. |
| *ignored by git — but still mapped* | Your `.gitignore` covers it, **and the map walked it anyway**. |
| *not in git* | On disk, never committed — and therefore in the map like anything else. |

The first two explain a folder that is on screen and **not** in the map. The
last two explain the reverse, and *ignored by git* is the one nothing else in
this window could tell you: **the scan does not read `.gitignore`**, because
a repository can quite reasonably ignore a directory the map should still
describe. So a folder you ignore in git is mapped anyway — and that line is
what tells you before you go looking for a bug.

At most one note per row, in that order: a folder that is not scanned at all
makes what git thinks of it beside the point.

The two git notes are read from `git status` once per directory listed, not
once per file. **A project that is not a git repository shows neither** —
absence of git is not evidence about a file, and calling every file in a
non-repository "not in git" would be confidently wrong about all of them.

Clicking a file opens it wherever the **Editor** setting says. Coming back
to the map does not reload it.

## Opening a file where an entity lives

The map's own right-click menu has **Open in editor** beside *Open source*,
carrying the path and — on a function — its line.

It is offered only when the map is embedded here, because a browser cannot
start an editor and a menu item that does nothing is worse than an absent
one.

The path is **resolved and bounds-checked before anything opens**. It
arrives repo-relative, which is what the artifact stores, and the message
comes from a document this app embeds but does not author — a map generated
by an older engine, or one somebody else produced. `../../` in a path is
the difference between opening a file and opening any file, so a resolved
path that does not start with the project root is refused. The check is on
the path's *shape* first — a drive, a UNC path (`\\host\share\…`) or a leading
separator is refused before the disk is touched, so a hostile map cannot make
Windows contact a server of its choosing — and on the resolved location second.
The same rule covers the search box's folder and the file tree. So do the project icon (every file it considers, including a web manifest's `icons`) and the local server behind the embedded map: a `docs/map` that is a symlink or junction, or a file in it that links out of it, is not served, because the page it holds is the repository's own and runs next to this server. A linked `docs/map` — or a `module_map.json` / `index.html` in it that is a link out of the project or to another machine — is read as **no map** by everything else too (the staleness check, the changed-files list, search over the map, the dependency graph, the project's map status): reading it would follow the link to wherever it points. A link that stays inside the project still works. *Generate* and *Check* refuse a project whose map directory is a link, or whose `index.html`, `module_map.json`, `overview.md` or `coverage.svg` is a link that leaves the project, goes to another machine or has a target that is not there (it cannot be checked, and an engine would create the target wherever the link says), with a message instead of writing through it (an engine from `docmap.nvim` commit `e472c5c` on does the same on its side: it checks every component of its output path and does not follow links out of the project when it reads the source tree, `.docmap.json` or the map for *Check*; an older engine does follow them). The map directory (Project settings → Map directory) must be one the engine accepts, and is refused when you save it otherwise: it lies inside the project (no `..`, no absolute path) and its names consist of letters, digits, `-`, `_` and `.` only, with no `.` name and, on Windows, no name that ends in a dot. A value saved before that was checked (`../maps`, `docs/my map`) is not handed to the engine: *Generate* and *Check* name it and send you to Project settings. Path components that name a drive (`C:x`) or a stream are refused on Windows, and a link target counts as local only if it is a drive or a volume — not a share, not an NT-namespace spelling of one, and not a target with a root and no drive (`\Device\…`). On Windows a file or folder whose name ends in a dot or a space (which Win32 reads as a different name) is skipped by every walk and by the folder picker, and a path with such a component is refused (so the project icon, which is handed on without its `\\?\` prefix, can never be one). The work the link walk may do for one path, and for one project icon lookup in all, is bounded: a link to a very deep tree, a long chain of links or a manifest with thousands of icons costs a fixed amount and then gives up. A name Win32 would rewrite is refused wherever it comes from - the path asked for, or the target of a link on the way - and a file of more than 8 MiB is never taken for the project icon (the window reads the whole file into memory to show it). The same holds when you pick a folder in *Add project*: the links in it that lead to another machine are not followed (a junction to another drive is, and still lists), and a `.git` that is a link counts as a `.git` without being followed.

Configure it in **Settings → Editor**: a command template with `{file}` and
`{line}` substituted. The template is split into arguments *before*
substitution, so a path with a space in it stays one argument.

**Leaving it empty hands the file to the desktop — for documents only.** What a
search hit, a changed file or a map's own "open" points at is text from a
repository somebody else wrote, and "open" on an executable or a script *runs*
it. So with no editor command, only plain documents and source files whose
open is to show them (`.md`, `.txt`, `.json`, `.toml`, `.yaml`, `.lua`, `.rs`,
`.go`, `.css`, and similar) open through the desktop's association. Anything
else — `.js`, `.py`, `.sh`, `.bat`, `.ps1`, `.html`, `.exe`, a file with no
extension, anything marked executable — is **shown in the file manager**
instead. Set an editor command to open every kind in the editor.

## Generate, Generate all, Generate the out-of-date ones

Four different guarantees, not four speeds of one thing:

| | Runs on | Overwrites an existing map? |
|---|---|---|
| Auto-generate (on adding a new project) | the one just added | never — only fires when there is no map yet |
| **Project → Generate map** (`Ctrl+G`) | the selected project | yes, that one |
| **Project → Regenerate and reload** (`F5`) | the selected project | yes, and reloads the view |
| **Project → Generate all** (`Ctrl+Shift+G`) | every project in the workspace | yes, all of them |
| **Project → Generate the out-of-date ones** | every project whose sources have moved on | yes, those |

**Generate all** is a menu item rather than a button: the sidebar keeps
exactly one command, and this is the rare one. It is the one place this app
writes to disk without being asked about that specific project, and that is
the point of a command with that name.

It runs projects **one after another, not in parallel**: each run is its
own CPU-bound process, and starting a dozen at once would not finish
sooner, only make the machine harder to use while it happened. Progress is
counted in the status bar (`Generating 3/12…`) for exactly that reason — a
multi-minute run with no visible progress looks identical to a hang. One
project failing does not stop the rest; the status line names every failure
once the batch finishes.

**Generate the out-of-date ones** measures each project rather than reading
the freshness cache. The cache only holds projects that have been opened,
so acting on it would skip exactly the ones this command exists for.

**Generate map (full)** adds `lua-language-server`'s type detail — the
`@class`/`@alias` information behind the Types panel. It needs
`lua-language-server` on `PATH`; without it the run fails and says exactly
that, and the ordinary Generate still produces a complete map apart from
that detail.

Either way, the engine's own report is shown: what was written, the counts and
the coverage as they are; the findings (drift findings such as a dead README
link or a Lua file without `---@module`) grouped by check, a few examples each
and how many more there are, with **Show all** for the rest. After a run that
wrote a map it is a panel docked under the map (**Engine report**, scrolls,
closes with × or Esc; the map above it gets shorter); after a failed run, and for **Check exactly**, it is under the
message in the middle of the view. Both scroll when the report is long. A
vault with findings is still *generated*: the map is written and the status bar
says how many errors and warnings the engine found. "Failed" is kept for a run
that did not write a map (no such directory, a refused output directory, a
crashed backend) — the window asks the engine for `--lenient`, so error-severity
findings in the tree do not turn into a non-zero exit.

## The staleness mark

The selected project carries a **Map outdated** chip when its sources have
moved on since its map was built — one keyword rather than a sentence: either
the map is behind or it is not. **Click it** for the files that changed since
the map was made (newest first, capped, with the true total; click a row to open
the file, or **Generate a new map** from the dialog). Hover for what to do about
it. The mark is what **Generate the out-of-date ones** acts on,
and it is why the mark is shown for the selected project rather than as a
count on the picker: a number behind a click is a number nobody reads.

It is a comparison of modification times, not a re-analysis. It answers
"something changed since this was written", not "the map would come out
different" — the exact question needs a `--check` run the app does not have
a command for yet.

## Settings

**File → Settings…** (`Ctrl+,`), in seven sections.

| Section | What it holds |
|---|---|
| **Appearance** | Theme and interface language. |
| **Behaviour** | The order the project list is in, and whether the app starts on the workspace overview. |
| **Engine** | Where the engine binary is, and optionally a directory of compiled tree-sitter grammars. |
| **Telemetry** | Whether `runtime-analysis.nvim` collects for the selected project, and the snapshots it has taken. |
| **GitHub traffic** | Where the traffic digest is read from, and whether the selected project shows it — see [GitHub traffic](#github-traffic). |
| **Editor** | The command used to open a file — see [above](#opening-a-file-where-an-entity-lives). |
| **Neovim** | The `nvim` binary and the config directory behind the Neovim tab of Add project. |

**Theme** has three states, and *System* is one of them: a two-way toggle
can only ever leave you pinned to a choice you made once, with no way to
hand the decision back to the OS.

**Order projects by** is the same control as the dropdown over the project
list, and the same setting — changing either moves both. It is repeated
here because on the sidebar it reads as a view of *this* list rather than as
something the app remembers, and it is remembered.

**Start with the workspace overview** is the other half of the *don't show
this again* checkbox on the overview itself. One stored answer, two places
to give it: the overview phrases it as the negative because that is what
somebody looking at it wants to say, and a preference list reads better
when every line is a thing that is on. Without this, the switch was
findable exactly once — by the person who had already decided to stop
seeing the thing it lives in.

**Language** changes this window and nothing else. The generated map is a
separate artifact with its own translation — English is the source
language; German ships because the author can tell when it is wrong, which
is the bar for listing a locale unmarked. `?i18n=debug` marks any string
still falling back to English, so an unfinished locale is countable rather
than merely embarrassing.

Error messages follow the language too. What the app says went wrong is
in the window's language; what *something else* said is shown after a
dash exactly as it was said — the operating system's error, `git`'s,
`gh`'s or Neovim's output, a path or a project name. So a German window
reads *C:/x lässt sich nicht lesen — Access is denied. (os error 5)* on
an English Windows. An error this window has no wording for (a newer
backend than the window) is shown in English, marked `‹en›`.

Theme, language and zoom live in `localStorage` rather than in the
workspace file, because they are properties of *this machine's eyes*.

**Telemetry** is worth two precise notes. Switching it takes effect **from
the next Neovim session** — nothing in this window runs your plugin, and
the switch is a flag the plugin reads when it next starts. And a telemetry
namespace is a *plugin name*, so this applies only to a project that
registers telemetry under its own. The snapshots listed are captures taken
with `:RATelemetry snapshot <name>`, never automatically; to compare two,
the map's own **Analysis → Telemetry** panel is the place, since it can
hold two at once.

## GitHub traffic

**Optional, read-only, and this app never talks to GitHub or sees a token.**
[github_stats.nvim](https://github.com/WKDSteve/github_stats.nvim) collects
the traffic GitHub reports for a repository — views, clones, referrers, popular
paths — and keeps it past the 14 days GitHub itself reports. After each fetch it
writes a small file (the *digest*) to a local folder; this window reads that
file. **Without the plugin, nothing here appears and nothing changes.**

**The line.** Under the selected project: `GitHub · 7 / 30 / 90 d — 41 / 210 /
655 views · 19 / 88 / 300 clones ↑ +12.5 %`. Views and clones over the last 7,
30 and 90 *complete* days, and the trend of the last 7 against the 7 before.
Hover for the uniques (a sum of the daily uniques — not distinct visitors). A
quieter line under it says **how old the data is**, and says so when it is more
than three days old: the digest only refreshes while Neovim runs the plugin.

**On the overview.** On the first screen (**All projects**) a row that has traffic
data carries one more figure under its counts — *210 views / 30 d* — and a row
that has none is exactly as it was. The overview's own order is unchanged: it
ranks by what needs doing, and traffic is a fact about a project, not a task.

**What it says when there is nothing to show — and what it does not say:**

| Situation | What you see |
|---|---|
| A digest for this repository | The line above. |
| The project has no GitHub remote | **Nothing.** Not an error. |
| The plugin is not installed, or has never run here | **Nothing** in the sidebar. Settings says what was looked at. |
| A GitHub repository the plugin does not track | *"owner/name is not tracked by github_stats.nvim."* |
| The digest is broken | *"The traffic digest could not be read: …"* with the reason. |
| The digest is from a newer plugin | *"…is newer than this app understands."* |
| You opted this project out | **Nothing.** |

None of these is ever shown as `0`. Absent is not zero, and the same rule holds
inside the plugin: a page that is not in GitHub's top 10 is *unknown*, not
*zero views*.

**How it finds the digest.** A fixed chain, first hit wins, never a search of
your disk: a folder you chose (**Settings → GitHub traffic → Choose folder…**),
then `root.json` at the plugin's default place (`stdpath("data")/github_stats.nvim`
— on Windows `%LOCALAPPDATA%\nvim-data\github_stats.nvim`), then the answer
**Ask Neovim** stored. The default is usually all it takes. **Ask Neovim** starts
Neovim once, headless, and asks the loaded plugin where it writes — for when you
moved `digest_dir` or run Neovim under another `NVIM_APPNAME`; it needs the
`nvim` binary from [Settings → Neovim](#settings) and never runs on its own. It
waits at most 30 seconds: a Neovim configuration that stops to ask something (an
update prompt, a debugger waiting for a client) ends in an error saying so, and the
button is usable again.
**Look again** forgets what was remembered (each project's remote, each parsed
digest) and reads afresh. The panel shows the folder that was found, how many
repositories it holds and when it last changed — or, if none qualified, each
folder that was looked at and why it did not count. A `digest_dir` that
`root.json` names must be an absolute path; a relative one is ignored.

**Which repository a project is.** Its `repo_url` if it has one (a project
imported from a URL does), otherwise its `origin` remote, read once and
remembered. Only `github.com`. A project whose origin is anywhere else has no
traffic to show.

**Opting a project out.** **Show GitHub traffic for the selected project**
(Settings → GitHub traffic) turns it off for one repository, for the private one
you track in the plugin and still do not want on screen or in a screenshot. Off
means nothing is read for it — not even its remote. It is stored with the
project, in this app; the plugin has no notion of the app.

**The files are not yours.** A digest is written from data a website supplied,
so it is read with a size cap, refused with a message if it is from a newer
schema, and shown as text — never as markup. A broken digest costs its own line
and nothing else.

## Project settings

**Project → Project settings…**, and the distinction from the section above
is the whole reason it is a second dialog: **Settings belongs to this
machine, Project settings belongs to the repository.** Where your engine
binary lives is a fact about this computer. Whether a repository vendors a
copy of something, or is worth reading as Go only, is a fact about the
repository — and storing that in the machine's settings would apply one
project's answer to every project in the list.

Five groups, because the engine takes more than the two flags this dialog
started with. It always did: `--source=`, `--out-dir=`, `--repo-url=` and
`--branch=` were there before this window existed, and the dialog simply
never grew into them — so a repository whose map does not live in
`docs/map` was unusable here, and every map generated in this window came
out with no source links at all.

**Every field is optional, and empty means "the engine decides".** That is
not the same as "the default": the engine reads a `.docmap.json` in the
repository itself, so an empty field leaves whatever the repository states
about itself alone. A filled one overrides it, because a flag beats a config
file — you are answering for this machine, the file is answering for
everyone.

### Scope

**Languages** is a tick list of the backends this engine actually has, read
from the engine itself rather than from a list in this app — which is why a
newer engine shows more of them without an update here, and why each entry
can say whether its grammar loaded. **Nothing ticked means all of them.**
That is not an empty selection quietly meaning "read nothing": a project
that has never been narrowed already reads everything, and the untouched
dialog has to mean the same thing the untouched project does.

Three grammar states, not two, and they stay apart here as they do
everywhere else: a grammar loaded, a grammar wanted and missing (*module
tree only* — a complete tree with no function-level data), and **needs no
grammar**, which is full fidelity rather than a degradation. Assembly is
the one.

**Excluded paths** is one path per line, relative to the project root, and a
path excludes that file or folder and everything under it. It is a path and
not a pattern: vendored and generated folders — `node_modules`, `target`,
`dist`, `build`, `.venv` and a dozen more — are already skipped by the
engine wherever they appear, with nothing to configure, so the shape worth
having is the other one. *This path, in this repository.*

**Add folder…** picks one and stores it relative to the root. A directory
outside the project is refused rather than stored, because a path that can
never match is a setting that appears to do something and does not.

### Layout

**Sources** is where the code is, relative to the project — one folder, or
several separated by commas. Left empty the engine finds them itself, which
is right for almost every tree. It exists for the ones where that search is
a wager you could not correct: a repository with `lua/` beside `src/`, or
sources under `packages/`. `exclude` and `languages` were already
correctable and this was not, which was an arbitrary place to stop.

**Map directory** is where the map is written inside the project, relative
to its folder. Empty means `docs/map`. It is refused on saving when the
engine would refuse it: a path outside the project (`..`, an absolute path or
a drive), a name with anything but letters, digits, `-`, `_` and `.` in it (a
space or an umlaut, say), a `.` name, or on Windows a name ending in a dot.
An older value that breaks this (say `../maps`) is named when the dialog opens,
and no map is read from there until it is changed.
Change it and **this window follows
the map to its new home** — the picker, the freshness mark and the overview
row all read the new location immediately. That mattered enough to name:
the alternative is a project that generates into one directory, reads from
another, succeeds at both, and shows you the old map.

### Source links

**Repository URL** is what the *view source* links in the generated page
point at, and **Branch** is which branch they point at (empty means `main`).

Without a URL the page still works and simply has no links out. That was
the one visible difference between a map made in this window and the same
map made by the same engine in CI, and nothing here explained it — the CI
job passes `--repo-url` and this app did not.

### Generating

**Always generate this project fully** makes plain *Generate* do what
*Generate map (full)* does: add the `lua-language-server` enrichment behind
the Types panel. It needs that tool installed, takes longer, and gains a
non-Lua project nothing — which is exactly why it is a choice per project
rather than a setting for this machine. *Generate all* honours it too, and
never offered the choice at all before.

### How they reach the engine

Every one of these becomes a flag, and they are looked up **by the app, not
by each button** — so *Generate*, *Generate all*, *Generate map (full)* and
**Check exactly** all honour them. That last one matters most: a check that
asked without the project's own settings would compare the committed map
against a map nobody would ever write, and report the project stale forever.

`--full` is the one deliberate exception: **Check exactly** does not mirror
it, because enrichment adds detail the committed artifact may legitimately
not contain.

## The engine indicator

**Engine**, the collapsible panel at the bottom of the sidebar. It answers
one question — can this app actually generate anything right now — and says
so in one word beside the header even while collapsed: `ready`,
`no grammars`, or `not found`.

The engine is `docmap.nvim`'s own standalone binary, a separate
program this app runs as a subprocess. The verdict stays visible because it
decides whether the next action works, and a fact behind a click is a fact
nobody reads.

**Three places it can come from, in this order:**

1. **A path you set** under **Settings → Engine**. Setting one is an act of
   intent, so it wins over everything.
2. **The engine installed beside this app.** The Windows installer puts it next
   to the program, together with its grammars — nothing to set up, and it is
   the exact build this version was tested against. (Linux packages are no
   longer built; a Linux build from source can stage the engine the same way.)
3. **An engine on your `PATH`**, if there is no bundled one (a macOS build, or
   a build without it).

**The bundled one beats `PATH`, and that order changed on 2026-08-20 after
it bit.** `PATH` used to win. Measured right after v0.2.0 shipped: the app
was using a `docmap.exe` from two days earlier — four languages, an older
schema — while its own installed copy read twenty-three, and nothing said
so. A binary on `PATH` is somebody's leftover as often as it is their
intention, and this program cannot tell the two apart.

A path you set that stops existing is not remembered as broken: the app
falls back to the list above rather than failing later with a raw OS error.

**Contract** is the row that says whether the engine found is the one this
app was checked against. Every build of the app carries `engine.lock`: the
docmap.nvim commit it was checked against, the artifact schema that
commit writes and the routes the window asks it for. The row compares the
engine's own answer with those lines:

- `schema 6, as checked` — in step.
- **This engine writes schema 7; this app was checked against 6** — the
  engine is newer. Its maps may lack a field this window reads (a schema bump
  is allowed to remove one), so update the app.
- **This engine writes schema 5…** — the engine is older. Update the engine,
  then generate the maps again.
- **This engine does not answer …** — a route the Telemetry, Loaded,
  Checklist or Commits panel needs is missing; that panel stays empty.

An engine too old to report its schema has no Contract row; the **Reads**
row already says it cannot be asked.

**Grammars…** points at a directory of compiled tree-sitter grammars and is
optional. Without one, generation still succeeds — a complete module tree,
correctly. With one, the same run also gets function-level detail
(signatures, call graphs, parameters). `no grammars` in the summary reports
that difference; it is not a problem to fix.

**When one is missing, the panel says where it looked.** Under the verdict
it names the directory the engine is given, lists what that directory
actually holds, and gives the file name a missing grammar would have —
`zig.dll`, `.so` or `.dylib`. It lists the directory's contents rather than
computing which paths the engine would probe: the resolution order belongs
to the engine, and a second copy of that rule here could disagree with it
while looking authoritative. Three other answers replace it when they apply:
the configured directory is gone, it is empty, or there is none at all.
Settings shows the same sentence, because that is where the button that
fixes it lives.

**There is no download button, and that is a decision.** Fetching grammars
would mean pulling native shared libraries from a rolling release tag with
no published checksum and handing them to a program that `dlopen`s them —
fine in CI, a silent update channel for unverified executable code in an
installed app. Naming the missing file costs nothing and touches no network.

**Not every language needs one.** The engine reads assembly without a
grammar at all, because GAS, NASM and ARM are a fork rather than dialects
and a line scanner is the instrument that is right across all three. The
panel keeps "needs no parser" and "wanted a parser and could not find it"
apart, so a backend at full fidelity is never reported as broken.

The panel opens itself automatically whenever the engine is not found — the
one state worth interrupting you for — and otherwise stays however you left
it.

## The note that appears over some panels

The generated page reports which panel it is showing, and three of them can
be empty for a reason that is not the project's. A note explains which
instead of leaving a blank pane to be read as a verdict.

**Two of them are permanent** — the app's engine cannot ever produce that
data, no matter how it is configured:

| Panel | Why this app can't | What it needs instead |
|---|---|---|
| Analysis → Telemetry | Telemetry exists only if real code ran inside a live Neovim session with `runtime-analysis.nvim` collecting it. This window can switch collection **on** (Settings → Telemetry) but cannot run your plugin code, so it cannot make the data exist. | Switch collection on here if it is off, then open the project in Neovim and use it — the panel reads whatever was collected there. |
| Hierarchy → Types | Type data comes from `lua-language-server`. The engine this app runs is `docmap.nvim`'s Neovim-free build, which has no equivalent — not "not installed", structurally absent. | Run `:DocMap full` inside Neovim, then reopen the project here — the map it writes already carries the type data. |

**The third is temporary, and says so.** Hierarchy → **Calls** and **Module
Calls** draw nothing for a project whose languages have no call extraction
in the engine yet — four of its twenty-three backends produce a call graph
and the rest do not. That is the same blank pane a project with genuinely no
calls in it produces, which is exactly why it needs a sentence: the note
says the engine has not got there yet, and that nothing about those
languages makes it impossible.

Only when the answer is *known to be no*. A project whose languages the
engine has not been asked about, or an engine too old to carry the field,
shows nothing — a guess would be wrong for precisely the reader who most
needs it to be right.

Every other panel needs no explanation and shows none.

## Keyboard navigation

The project picker is a native `<select>`, so it answers `↓`/`↑`,
`Home`/`End`, `Enter` and type-ahead in whatever way your platform does —
which is more than a hand-rolled list would, and none of it can drift.

`Delete` is the one key it does not answer, deliberately: removal lives on
**File → Remove from workspace**, which also names what it removes. A bare
Delete key over a list of repositories was always the more frightening of
the two.

Right-clicking the detail block under the picker opens the same per-project
commands as a context menu, including **Reveal in file manager** and **Copy
project path** — the latter because the path is on screen and
unselectable, which is the whole reason it exists.

## Where things live

Nothing is stored inside a project you add.

| File | Holds |
|---|---|
| `workspace.json` | The settings, and which workspace is active. |
| `workspaces/<name>.json` | One workspace's project list. |
| `repos/` | Repositories cloned by the URL tab. |

All of it in the OS's own per-app config directory — not beside the
executable, so an installed copy and a portable one find the same list on
the same machine, and an installed app never needs write access to its own
install directory.

**Help → Open the settings folder** opens it. The folder rather than a
single file, because there are now several and which one you need depends
on what went wrong.

A workspace name is typed by a person and becomes a path, so it is reduced
to alphanumerics, `-`, `_` and space before it is used as one.

## The menu bar

Everything this window does, in one place. The sidebar keeps the two things
a menu is wrong for — the project you are looking at, and the verdicts that
decide whether the next action works.

| Menu | Item | Key |
|---|---|---|
| **File** | Add project… | `Ctrl+N` |
| | Open map in browser | `Ctrl+Shift+O` |
| | Reveal in file manager | |
| | Copy project path | |
| | Export current view… | `Ctrl+E` |
| | Remove from workspace | `Delete` |
| | Workspaces… | `Ctrl+Shift+W` |
| | Settings… | `Ctrl+,` |
| **Project** | Generate map | `Ctrl+G` |
| | Generate all | `Ctrl+Shift+G` |
| | Generate the out-of-date ones | |
| | Regenerate and reload | `F5` |
| | Generate map (full) | |
| **View** | Theme · Language | |
| | Zoom in / out / Actual size | `Ctrl+plus` `Ctrl+-` `Ctrl+0` |
| | Files on disk | `Ctrl+Shift+F` |
| | Statistics | |
| | Sidebar | `Ctrl+B` |
| | Auto-hide sidebar | |
| **Help** | Usage · What the engine is · Search the documentation… · Open the settings folder | |
| | GitHub repository · License: MIT | |
| | Send feedback… | |
| | About docmap | |

Project items are greyed when nothing is selected. **Search the
documentation…** asks for words and opens GitHub's search over the README and
`docs/` of this repository in your browser; the window builds that address
itself, so the words can only ever be the search. Every dialog closes with
Escape or with a click beside it, and the name **docmap** at the top of the
sidebar opens the workspaces. The menu's design record
(kept with the author's notes, not in this repository) has the rule every one
of these had to pass, and the review that checked them all against it.

### Export the current view

**File → Export current view…** (`Ctrl+E`) saves the diagram the map is
showing as a standalone SVG, wherever you choose.

The map is a separate document from a separate origin, so this window
cannot read into it — it *asks*, and the page answers. The page takes no
instructions through that channel, only questions about itself: a host that
could tell an embedded page what to do is a different kind of program than
one that can ask it what it is showing.

Only Hierarchy draws a diagram. On any other tab this says so rather than
writing a file of the last diagram that happened to be drawn.

### About

**Help → About docmap** answers "which versions am I running", including
the engine's own build: the commit it was built from, when, and whether
that tree was clean. A binary built from a modified tree carries a commit
that does not describe it, so About says so rather than quoting a sha that
would send a reader to the wrong diff.

There is a copy button, because the whole point of the block is a bug
report, and the links to the repository and its license.

## Sending feedback

**Help → Send feedback…** builds a report and opens it on GitHub in your
browser. It does not post anything: you land on GitHub's own form with the
text already in it, read it, and press Submit yourself.

That is not caution for its own sake. This app holds no credentials of its
own — the same reason cloning goes through whatever `git clone` already
needs on your machine. And filing to a public tracker is publishing, which
is not something a dialog should do on your behalf while you are looking at
a button.

**Attach version and platform** is ticked by default because almost every
report needs it and almost nobody remembers, and the exact text is shown
before anything opens — it is the one part of the report you did not type.

## Limits of a rules run through ai-desktop

A rules run sends file excerpts through ai-desktop on this machine. Two things
this app cannot promise, whatever it shows:

- **Cancelling stops the waiting, not the provider.** This app drops the call at
  once and ignores its answer, but ai-desktop may finish it upstream: a closed
  socket on this machine does not reach a cloud provider. Text already sent has
  been sent, and the call can still be billed.
- **The destination shown follows the default prefixes.** `claude-` goes to
  Anthropic, `gpt-`, `o1-` and `o3-` to OpenAI, `gemini-` to Google, anything else
  to Ollama. A custom model in ai-desktop's `config.json` or the
  `AI_DESKTOP_PREFIXES_*` variables can send a model somewhere else, and this app
  does not see them, so the allow-list (`agent.backends` in `.rules.json`) checks
  the destination from the prefixes. Do not use a model name that ai-desktop routes
  differently in a project whose allow-list has to hold.

## What this app is not documenting

Everything the generated page itself shows — the module tree, the Analysis
tabs, the Findings tab, the Checklist panel and its `@ref`/`@verified`
syntax, what Telemetry and Loaded need to show real data — is
`docmap.nvim`'s own surface, not this app's. This app is one more
place that page can run (the project's roadmap calls it *the fourth host*); it does
not change what the page means.

| Question | Where |
|---|---|
| What each tab and Analysis panel shows | [`docmap.nvim` — WORKFLOW.md](https://github.com/WKDSteve/docmap.nvim/blob/main/docs/WORKFLOW.md) |
| Which languages the engine reads, and how fully | [`docmap.nvim` — languages.md](https://github.com/WKDSteve/docmap.nvim/blob/main/docs/languages.md) |
| The checklist ledger's syntax and states | [`docmap.nvim` — checklist_format.md](https://github.com/WKDSteve/docmap.nvim/blob/main/docs/checklist_format.md) |
| How the map is built, stage by stage | [`docmap.nvim` — pipeline.md](https://github.com/WKDSteve/docmap.nvim/blob/main/docs/pipeline.md) |
| Talking to a project's map from an agent | [`docmap.nvim` — mcp.md](https://github.com/WKDSteve/docmap.nvim/blob/main/docs/mcp.md) |
