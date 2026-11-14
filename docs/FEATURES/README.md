# Features

docmap-desktop is a workspace over the module maps
[docmap.nvim](https://github.com/WKDSteve/docmap.nvim)
generates: add several projects, look at any of them, switch between them
quickly. It analyses nothing itself — the engine does that, and anything here
that started rebuilding it would be the wrong turn this program is defined
against.

This folder is the catalogue, grouped by theme. It is the same
[`FEATURES_FORMAT`](https://github.com/WKDSteve/docmap.nvim/blob/main/docs/FEATURES_FORMAT.md)
shape the Neovim siblings use — one `##` per feature, a run of
`- **Key:** value` bullets after the summary — even though nothing here is
Lua. The format reads Markdown, not a language.

- **[CORE](CORE.md)** — the workspace model: projects, workspaces, the
  picker, and the cross-project dependency answer a single repository cannot
  give.
- **[GENERATION](GENERATION.md)** — running the engine: the four generate
  commands and their four different guarantees, the staleness mark, and how
  the engine is found.
- **[VIEWING](VIEWING.md)** — the window: the overview screen, the map view
  and the server behind it, the project bar with search and statistics, the
  files pane, the Rules tab with its mechanical run, and opening a file where
  an entity lives.
- **[SETTINGS](SETTINGS.md)** — global and per-project settings, and where
  everything is stored.
- **[TRAFFIC](TRAFFIC.md)** — GitHub traffic from `github_stats.nvim`'s digest: the
  sidebar line, the fifth sort order, how the digest is found, and how a file
  that is not yours is read.
- **[DESKTOP](DESKTOP.md)** — the parts that make it a desktop program rather
  than a page: menu bar, keyboard and mouse behaviour, export, feedback,
  interface language, and the look it shares with the other desktop apps.

Reference documentation lives one level up and is not repeated here:
[`USAGE.md`](../USAGE.md) is the button-by-button manual and
[`RELEASING.md`](../RELEASING.md) the release procedure.

> **Only theme files belong in this folder.** The Features tab's parser reads
> every `##` here as a feature, so a document that uses `##` for its own
> structure — a plan, a decision record, an essay — is counted as features
> that do not exist. Such documents do not belong in this repository's `docs/`
> at all; they are kept with the author's working notes.
