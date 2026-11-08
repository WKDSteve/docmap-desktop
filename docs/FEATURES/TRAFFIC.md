# GitHub traffic

What GitHub reports about a repository — views, clones, where visitors come
from — shown next to the project it belongs to. **Read-only, and nothing in
this app talks to GitHub or ever sees a token.** The numbers are collected by
[github_stats.nvim](https://github.com/WKDSteve/github_stats.nvim), which
also keeps them past the 14 days GitHub itself reports; this app reads the
small file that plugin publishes and shows what is in it. With the plugin
absent, nothing in the window changes.

## The traffic line — views and clones, 7 / 30 / 90 days

Under the selected project in the sidebar: views and clones over the last 7,
30 and 90 complete days, a trend arrow (the last 7 days against the 7 before),
and, on hover, the uniques. A second, quieter line says how old the data is —
and says so plainly when it is more than three days old, because a number that
is three weeks stale and looks current is the one way this line can lie.

**Absent is never zero.** A project with no GitHub remote shows nothing (not an
error), a machine without the plugin shows nothing, and a repository the plugin
does not track says exactly that. "Uniques" is the sum of the daily uniques, not
distinct visitors, and is never called visitors.

**The details dialog** (*Traffic details…*) lists the pages GitHub reported as most
looked at, with the span the stored numbers cover. It once also drew a chart per
series and a referrer list; both were dropped — the pages are what is worth reading.
A page that is a file in the project opens in the editor.

- **Module:** `src-tauri/src/traffic.rs`, `src/lib/traffic.js`, `src/main.js`
- **Config:** `traffic_dir`, `traffic_asked_dir` in `workspace.json`; per project `traffic_hidden`
- **Docs:** [USAGE.md](../USAGE.md#github-traffic)

A row on the **All projects** overview carries the 30-day views figure too, where
there is one; a row without numbers is untouched, and the overview's own order —
by what needs doing — does not change.

## The fifth sort order — most traffic

**Most traffic** puts the projects with the most views in the last 30 days first,
clones as the tie-break. It answers the question a list of many projects is the
one place to ask: which of my projects is anyone looking at.

A project with no numbers — no GitHub remote, no plugin, not tracked — comes
**after every project that has some, zero included.** "Nobody looked" and "we
cannot tell whether anybody looked" are not the same rank. Like the two
freshness orders it reads its data first (one call for the whole list) rather
than sorting by whatever happens to be known.

- **Module:** `src/main.js`, `src/lib/traffic.js`
- **Config:** shared with Settings → Behaviour
- **Docs:** [USAGE.md](../USAGE.md#the-project-picker-and-how-to-sort-it)

## Finding the digest — a fixed chain, never a search

The plugin writes one JSON file per repository (the *digest*) and a pointer
(`root.json`) to a local, per-machine folder. This app finds it through one
chain, first hit wins: **a folder you chose**, then **`root.json` at the plugin's
default place**, then **the answer Neovim gave when asked**. Nothing else — it
never searches the disk — and if nothing qualifies, Settings says what was
looked at and why it did not count.

**Ask Neovim** starts one `nvim --headless` and asks the *loaded plugin* where it
writes (`require("github_stats.digest").digest_dir()` — a small module that
needs neither the plugin's dashboard nor `ui.nvim`). It never runs on a render.
It replaces reading the path out of your installation spec, which is Lua code
(`opts` may be a function).
The process is bounded: `traffic::run_with_timeout` kills the one child it started
after 30 s (`ASK_TIMEOUT`) and reports it, so a configuration that blocks cannot
hang the button. A `digest_dir` read from `root.json` is used only when absolute.

The chosen folder is read by the app's own process only; it is **not** added to
any Tauri fs scope, so the window can ask for numbers but never gets a path it
could read on its own.

- **Module:** `src-tauri/src/traffic.rs`, `src-tauri/src/traffic_cmds.rs` (`traffic_*` commands)
- **Config:** Settings → GitHub traffic
- **Docs:** [USAGE.md](../USAGE.md#github-traffic)

## Project → repository, reported not assumed

A project's GitHub repository is its `repo_url` when it has one (a URL-imported
project already does — no process), otherwise its `origin` remote, read once with
`git remote get-url origin` and remembered. Three URL forms are understood, and
**only host `github.com`** — `https://github.com/o/n`, `git@github.com:o/n.git`,
`ssh://git@github.com/o/n` — and the host is taken from after the last `@`, so
`https://github.com@evil.example/o/n` is *evil.example*. Outcomes are shown as
what they are: no remote (nothing), not tracked, unreadable, or newer than this
app understands.

**Opt-out, per project.** *Show GitHub traffic for the selected project*
(Settings) turns it off for one repository — for the private one you track in
the plugin and still do not want on screen or in a screenshot. Off means nothing
is read for it, not even its remote.

- **Module:** `src-tauri/src/traffic.rs`
- **Config:** `traffic_hidden` in the project list
- **Docs:** [USAGE.md](../USAGE.md#github-traffic)

## Reading files that are not yours

The digest is text written by another program from data a website supplied — a
referrer is whatever it sent. So it is read with a **2 MiB cap**, decoded
lossily rather than refused over one bad byte, refused with its own message when
its `schema` is newer than this app knows, and every number is clamped rather
than trusted. A digest that is broken is `unreadable` with the reason; it never
takes the window down. Everything shown is written with `textContent`, never
markup.

- **Module:** `src-tauri/src/traffic.rs`
- **Docs:** [github_stats.nvim's `DIGEST.md`](https://github.com/WKDSteve/github_stats.nvim/blob/main/docs/FEATURES/DIGEST.md) is the file contract
