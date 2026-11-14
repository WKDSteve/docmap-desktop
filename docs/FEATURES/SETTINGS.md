# Settings

Two dialogs, and the split between them is the whole reason there are two.

## Settings — seven sections, belonging to this machine

**File → Settings…** (`Ctrl+,`): Appearance (theme and interface language),
Behaviour (project order, whether the app starts on the overview), Engine
(the binary and an optional grammar directory), Telemetry, GitHub traffic
(where the digest is, and whether the selected project shows it), Editor (the
command used to open a file), Neovim (the binary and config directory behind
the Neovim tab of Add project).

**Theme has three states, and *System* is one of them** — a two-way toggle
can only ever leave you pinned to a choice you made once, with no way to hand
the decision back to the OS.

**Order projects by** is the same control and the same setting as the
dropdown over the project list; changing either moves both. It is repeated
here because on the sidebar it reads as a view of *this* list rather than as
something the app remembers, and it is remembered.

**Start with the workspace overview** is the other half of the *don't show
this again* checkbox on the overview itself. One stored answer, two places to
give it — without this, the switch was findable exactly once: by the person
who had already decided to stop seeing the thing it lives in.

- **Module:** `src/main.js`
- **Config:** `workspace.json`
- **Usercmds:** File → Settings… (`Ctrl+,`)
- **Docs:** [USAGE.md](../USAGE.md#settings)

## Project settings — belonging to the repository

**Project → Project settings…**, and the distinction is why it is a second
dialog: **Settings belongs to this machine, Project settings belongs to the
repository.** Where your engine binary lives is a fact about this computer.
Whether a repository vendors a copy of something, or is worth reading as Go
only, is a fact about the repository — and storing that in the machine's
settings would apply one project's answer to every project in the list.

**Every field is optional, and empty means "the engine decides"** — which is
not the same as "the default". The engine reads a `.docmap.json` in the
repository itself, so an empty field leaves whatever the repository states
about itself alone. A filled one overrides it, because a flag beats a config
file: you are answering for this machine, the file is answering for everyone.

- **Module:** `src/main.js`
- **Usercmds:** Project → Project settings…
- **Docs:** [USAGE.md](../USAGE.md#project-settings)

## Scope — languages and excluded paths

**Languages** is a tick list of the backends this engine actually has, read
from the engine rather than from a list in this app — which is why a newer
engine shows more of them without an update here, and why each entry can say
whether its grammar loaded.

**Nothing ticked means all of them.** That is not an empty selection quietly
meaning "read nothing": a project that has never been narrowed already reads
everything, and the untouched dialog has to mean what the untouched project
means.

Three grammar states, not two, kept apart here as everywhere else: loaded,
wanted and missing (*module tree only*), and **needs no grammar** — full
fidelity rather than a degradation. Assembly is the one.

**Excluded paths** is one path per line, relative to the project root, and it
is a path rather than a pattern: vendored and generated folders are already
skipped by the engine wherever they appear, so the shape worth having is the
other one — *this path, in this repository*. **Add folder…** refuses a
directory outside the project rather than storing it, because a path that can
never match is a setting that appears to do something and does not.

- **Module:** `src/main.js`
- **Config:** per project; `--languages=`, `--exclude=` to the engine
- **Docs:** [USAGE.md](../USAGE.md#scope)

## Layout, source links and generating

**Sources** is where the code is, relative to the project — one folder or
several. Left empty the engine finds them itself, which is right for almost
every tree; it exists for the ones where that search is a wager you could not
correct, like `lua/` beside `src/`, or sources under `packages/`. Alongside
it: the output directory, and the repository URL and branch that turn map
entries into source links.

These four flags — `--source=`, `--out-dir=`, `--repo-url=`, `--branch=` —
were in the engine before this window existed, and the dialog simply never
grew into them. Until it did, a repository whose map does not live in
`docs/map` was unusable here, and every map generated in this window came out
with no source links at all.

- **Module:** `src/main.js`
- **Docs:** [USAGE.md](../USAGE.md#layout)

## Where things live

Nothing is stored inside a project you add.

| File | Holds |
| --- | --- |
| `workspace.json` | The settings, and which workspace is active |
| `workspaces/<name>.json` | One workspace's project list |
| `repos/` | Repositories cloned by the URL tab |
| `rules-trust.json` | The rulesets whose code this machine trusts: real path, content hash, who and when |

All of it in the OS's own per-app config directory — not beside the
executable, so an installed copy and a portable one find the same list on the
same machine, and an installed app never needs write access to its own
install directory.

- **Usercmds:** Help → Open the settings folder — the folder rather than a single file, because there are several now and which one you need depends on what went wrong
- **Docs:** [USAGE.md](../USAGE.md#where-things-live)
