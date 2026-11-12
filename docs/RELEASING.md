# Releasing

Cutting a release of this app, start to finish. Written after doing it for
`v0.1.0`, so every step below is one that was actually taken rather than one
that ought to work.

## Table of content

- [What a release is here](#what-a-release-is-here)
- [Before tagging](#before-tagging)
- [Tagging](#tagging)
- [What the workflow does](#what-the-workflow-does)
- [Checking the draft](#checking-the-draft)
- [Publishing](#publishing)
- [A draft ages, and it ages fast](#a-draft-ages-and-it-ages-fast)
- [If something is wrong](#if-something-is-wrong)
- [Two things worth knowing before you publish](#two-things-worth-knowing-before-you-publish)
- [Security decisions](#security-decisions)

---

## What a release is here

A pushed tag matching `v*`, and nothing else. `.github/workflows/release.yml`
does the rest: it builds the Windows installers and attaches them to a
GitHub Release **as a draft**.

Pushes, tags and the rolling build run on the self-hosted Windows runner of the author, by choice; a pull
request runs `ci.yml` on `windows-latest` and never on the self-hosted runner. Without a running runner a push
or a tag stays "queued" and nothing turns red; start the runner first. Only Windows is built: there is no Linux
or macOS installer in a release.

The draft is the point, and it is deliberate — the workflow says so where it
sets it:

> Draft, not published outright: a build succeeding on every platform is not
> the same guarantee as someone having opened the result, and a draft lets
> that happen before the release goes public.

So the last step is a person opening the app. Nothing automates that, and
nothing should.

---

## Before tagging

**CI has to be green on `main`.** A release built from a red pipeline is the
thing a release exists to avoid — and this is not hypothetical here: `v0.1.0`
was held up because `main` had been failing on macOS for weeks without anyone
noticing, in a way that had nothing to do with the code being released.

```bash
gh run list --branch main --limit 1
```

If it is not green, find out why before anything else:

```bash
gh run view <run-id> --json jobs --jq '.jobs[] | "\(.conclusion)  \(.name)"'
gh run view <run-id> --log-failed | tail -25
```

**Refresh the engine first, and check that it actually built.** The Windows
installer bundles whatever `docmap.nvim`'s rolling
`standalone-latest` release holds *at build time*, so an app release with a
stale engine ships features that talk to flags the bundled engine does not
have. That is not hypothetical either: cutting `v0.2.0` found
`standalone-latest` five weeks of engine work behind — 58 commits, including
the `--exclude=`/`--languages=` flags that this app's own **Project
settings…** dialog passes.

```bash
gh -R WKDSteve/docmap.nvim release view standalone-latest --json publishedAt
gh -R WKDSteve/docmap.nvim workflow run release-engine.yml
```

Then watch it, because *both* platform jobs have to pass — the publish step
is skipped if either fails, and the rolling tag then still holds the old
engine while nothing says so:

```bash
gh -R WKDSteve/docmap.nvim run list --workflow=release-engine.yml --limit 1
```

Rebuilding the engine is also the only thing that runs its `standalone` gate
on a clean machine, which is why it is where two Neovim-only API calls were
caught in one afternoon. Expect it to find something if the engine has moved
much since the last release.

**The version in the manifests is the version you tag.** `src-tauri/tauri.conf.json`
and `src-tauri/Cargo.toml` both carry it, and they must agree with each other
and with the tag. Bump them in a commit of their own before tagging, so the
tag points at a tree that already claims that number. The workflow checks it
(`check`, below) and stops before building anything when they differ.

---

## Tagging

An annotated tag, with a message that says what is in the release. It is the
first thing anyone reads on the Releases page.

```bash
git tag -a v0.2.0 -F -
```

…then type the message and end with `Ctrl+D`. Or keep it in a file and pass
`-F notes.md`.

```bash
git push origin v0.2.0
```

Pushing the tag is what starts the build. Nothing before that point is
public.

---

## What the workflow does

Three steps, in order: the job `check`, then the job `build` (Windows only), then the job `publish`, all on
the self-hosted runner. One run per tag at a time (`concurrency`): a tag deleted and set again waits for the
run it replaces.

`check` stops the run, before anything is built, when the tag is not `vX.Y.Z`, when the tag, `tauri.conf.json`
and `Cargo.toml` do not name one version, when the commit is not on `main` or has no green `ci` run, or when a
*published* release of the tag exists already (a published release is never cut again). A draft of the tag
that is there is reused: its files and notes are replaced by this cut.

`build` runs the Tauri CLI itself (`npx @tauri-apps/cli@2.12.4 build`, the version is pinned in both
workflows) and uploads the installers as an artifact. It does not use `tauri-apps/tauri-action`: on the
self-hosted runner the action's CLI does not find `cargo` ("cargo metadata … os error 2"), while the same CLI
started from the job's own shell does. The Rust toolchain is the one of the runner machine (`rustup` with
clippy and rustfmt), not an action. Shell steps use Git Bash by its full path, because `shell: bash` is the
WSL launcher on that machine. `publish` is the action of the same name in appkit's release-kit: it makes the
draft (titled `docmap-desktop vX.Y.Z`, with `.github/release-notes.md`, where `{tag}` becomes the tag), puts the
installers on it, and adds the install script, stamped for this tag, and `SHA256SUMS` over all of them.

| Platform | Produces | Engine bundled |
|---|---|---|
| Windows | `_x64-setup.exe` | yes |

One file, which is the number to expect (no MSI: `bundle.targets` is NSIS only, installed per user). Listed from the real run rather than from the workflow, because Tauri
decides which bundles it makes (`bundle.targets`). `publish` expects exactly one (`expect: 1`): a build that
made fewer leaves the number short and nothing is published. On top come `install.ps1` and `SHA256SUMS`. (The
first releases also carried Linux packages (`.deb`, `.rpm`, `.AppImage`) and macOS images (`.dmg`,
`.app.tar.gz`), nine files in all; they were built on hosted runners and are gone with them.)

The engine sidecar is downloaded from `docmap.nvim`'s own `standalone-latest` rolling release and staged where
`tauri.windows.conf.json` expects it (the local action `.github/actions/engine-sidecar`, which `release.yml`
and `rolling.yml` share; it still accepts the Linux triple, but nothing calls it with that). `docmap.nvim` does
not publish a macOS build of the engine, and no macOS build of this app is made now.

**Which engine the check actually exercises.** Since 2026-08-20 the bundled
one wins over anything on `PATH`, so opening the installed app verifies the
engine that shipped in it — which is the point of the step. Before that
change it did not: an older `docmap.exe` on the tester's `PATH` silently
took precedence, and the app looked correct while running a two-day-old
engine with four languages instead of twenty-three. If you want to confirm
which one is in use, the sidebar says `(bundled)` or `(found on PATH)`.

Watch it:

```bash
gh run list --workflow=release.yml --limit 1
gh run view <run-id> --json jobs --jq '.jobs[] | "\(.conclusion // .status)  \(.name)"'
```

The draft Release appears when `publish` runs, after the build has finished, and with every file at once.
A draft that is not there yet means the build is still running, or that it failed (read the `build` job, then
tag again after deleting the draft, if there is one). If nothing happens at all, the runner is not running:
`pwsh $env:NVIM_CONFIG_DIR\scripts\runners.ps1 status docmap-desktop`.

---

## The scripts, and the rolling build

Both channels carry `install.ps1`, stamped for the channel (the release-kit of appkit, v0.23.1 and later, which
also runs on a Windows runner; [its README](https://github.com/WKDSteve/appkit/tree/main/release-kit)). There
is no `install.sh` any more. Why a script and not only the
installer: a file the browser downloads is marked as coming from the internet and SmartScreen stops an unsigned
setup with that mark (on a managed workstation it may not be run at all); a file the script downloads has no
mark, and the script checks it against `SHA256SUMS` of the same release and runs nothing that does not match.
That proves the installer is whole and the one of the release, not who made the release. The Windows script runs
the NSIS setup silently (`/S`, for the current user).

**Downloads are anonymous.** `install.ps1` fetches the release files without a token; `GH_TOKEN` (or
`GITHUB_TOKEN`) is only used when set, e.g. against rate limits. The `published` checks of the workflows get the
token of the run.

The rolling build ([`rolling.yml`](../.github/workflows/rolling.yml)) is the pre-release `latest`: after every
green `CI` run on `main` it builds the NSIS setup of exactly that commit (with the engine bundled), under the
fixed name `docmap-desktop-windows-x86_64-setup.exe`, and replaces the files of `latest` with the script and
`SHA256SUMS`. It is for trying the newest state, not a release. Only the tip of `main` is built.

**Tauri smoke.** Before the installer goes up, the rolling build opens the real window of exactly that build
(`scripts/e2e/tauri_smoke.mjs`: title, rendered page, size, a screenshot as the artifact `tauri-smoke-windows`). It
needs `tauri-driver` (`cargo install tauri-driver --locked`) and an `msedgedriver` of the installed WebView2 on the
runner's PATH, installed once (the driver from `msedgedriver.microsoft.com/<version>/edgedriver_win64.zip`, its
Authenticode signature checked; the WebView2 version is `pv` under the EdgeUpdate client
`{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`). After a WebView2 update the driver has to follow, or the smoke fails at
the session. A missing tool is a reported skip (exit 77), a warning on the run; a failed check stops the rolling
build.

**The kit is pinned in one place.** `tools/release-kit.pin` names the tag and the commit; the `uses:` lines of
both workflows are that commit, and `tools/check-release-kit-pin.py` (CI job `tokens`) fails when they differ or
when the tag was moved. To move to another kit tag: change the pin file and the `uses:` lines together. The
design tokens have their own pin (`tools/appkit-ui.pin`).

---

## Checking the draft

```bash
gh release view v0.2.0 --web
```

Drafts are visible only to people with write access; there is nothing public
yet.

Download the installer for the platform you are on:

```bash
gh release download v0.2.0 --pattern "*-setup.exe" --dir .
```

The install scripts cannot fetch a draft by themselves (the files of a draft download only for people who may
write to the repository, logged in). Download the installer, `SHA256SUMS` and the script into one folder and run
`.\install.ps1 -File .\docmap-desktop_0.2.0_x64-setup.exe`: the sums are found beside it. The download path
(no `-File`, see above) is tried once after publishing, with the line of the notes.

If you cannot install it right away, you can still look inside the Windows
installer without installing or starting anything. It is an NSIS archive, so
7-Zip unpacks it, and the bundled engine answers on its own:

```bash
7z x -o"$TMP/x" docmap-desktop_0.6.0_x64-setup.exe
cd "$TMP/x" && DOCMAP_TS_DIR="$PWD/grammars" ./docmap.exe --capabilities
```

That prints the engine's `build.commit` (compare it with `docmap.nvim`'s
`main`) and how many grammars loaded. **Do not run `docmap-desktop.exe` for a
version check** - it has no `--version`, it opens the window. This proves which
engine shipped; it proves nothing about the window, which is why it does not
replace the walk-through below.

Install it, open it, and walk through the things a build cannot check:

- a project selects and its map loads;
- **Generate map** runs and the status bar reports it;
- **File → Settings…** opens and the theme switch takes;
- **Help → About** names an engine and a build.

That last one is worth reading rather than glancing at. It says which engine
the installed app found and, when the engine is a bundled build, the commit
it came from and whether that commit describes it.

---

## Publishing

```bash
gh release edit v0.2.0 --draft=false
```

Or on the release page: **Edit**, then **Publish release** at the bottom.

It is then at `/releases/latest`.

**Until something is published, the public parts were broken, and they said so
out loud** (written while the repository was still public). Checked on 2026-08-21, with `v0.1.0` and `v0.2.0` sitting in the
Releases page as drafts and nothing public:

```
$ curl -sI .../releases/latest            # 302, to the releases list
$ curl -s .../badge/v/release/...         # "release: no releases or repo not found"
```

The README's *Download the latest release* link and its version badge are the
first two things a visitor sees, and a repository with three tags and two
drafts renders as a repository with no releases at all. That is not an
argument against the draft step — it is an argument for finishing it.

---

## A draft ages, and it ages fast

`v0.2.0` was cut on 2026-08-20 with nine green assets and never published.
By the next morning **22 desktop commits and 17 engine commits** had landed
on top of it. Publishing it then would have shipped a version nobody would
ever install, and the walkthrough below — install it, open it, click four
things — would have been done on software that was already history.

So: **the check is part of the cut, not a task for later.** If a draft
cannot be opened within a day or so of building it, the honest move is to
let it go stale on purpose and re-cut from `main` when there is time to
check it.

What to do with the number is a smaller question with a clear answer.
`v0.2.0` was never public, so 0.2.0 was technically free — but the handover
notes and the build log already described what was *in* it, and a tag pointing at
a different tree than the prose written about it is worse than a gap in the
sequence. The draft was deleted, the tag left standing as a point in the
history, and the next release took the next number.

---

## If something is wrong

Do not publish it. Delete the draft and the tag together, fix, and start
over:

```bash
gh release delete v0.2.0 --yes --cleanup-tag
```

Then commit the fix, re-tag, and push. While nothing is published this costs
nothing. A public release that gets withdrawn is already in somebody's
download history, which is why the draft step exists at all.

**Re-cutting a version is for the case where nobody has seen it.** `v0.6.0` was
published, an hour later a review of it found a real defect, and the tag was cut
again on a newer commit (`gh release delete v0.6.0 --yes --cleanup-tag`, tag the
new commit, push, wait for four green jobs, publish). That was defensible only
because the tag was an hour old and the author was the only person who knew the
repository; anyone who had installed the first build still has it, and *Help ->
About* is how to tell the two apart (the engine commit differs). Once anyone else
could have it, a fix is a patch release.

---

## Two things worth knowing before you publish

**There is no macOS or Linux installer now, and the macOS menu bar was never checked.**
Releases carry Windows files only (see the top). If a macOS
build comes back, mind what the menu bar's design record (kept with the author's notes) says: the tree needs a
documented macOS variant before shipping there — on that platform the first submenu is the
*application* menu and carries About, Preferences and Quit by convention, and
File does not own Quit. The tests that would catch a wrong tree cannot run on
macOS either: `muda::Menu` refuses to be created off the main thread and
Rust's test harness runs every test on a worker, so those three are gated off
that platform. A test that cannot run is not evidence. (The earlier drafts carried `.dmg` files;
`gh release delete-asset v0.2.0 <name>.dmg --yes` was how to drop them before publishing.)

**A draft is visible only to people who may write to the repository.** Publishing is what makes a release
reach `/releases/latest` and the install scripts.

**The engine has its own release, and it rolls.** `standalone-latest` is a
moving tag in `docmap.nvim`. Two installers of this app built a month
apart therefore bundle different engines, and that is by design — which is
exactly why About reports the engine's own commit rather than assuming the
app's version implies it.

## Security decisions

- **No updater.** The hub (hub-desktop) updates the apps, with the checksums of the release; a second way to update would be a second key to look after.
- **No code signing.** The installers are unsigned: this is private use, and a certificate costs money every year. Windows may warn on first start.
- **Logging.** The app writes no log file. Messages go to the console of a debug run, and they name no file contents. Keep it that way: paths and contents of a project are private data.
- **macOS and Linux** are not published (see the table above); the code keeps building for them, nothing promises they run.

Enforced in code and CI: the window's CSP allows no inline script, the asset protocol allows no file until the app names it (`tauri.conf.json`), the server of a map only answers its own page, and `cargo deny` and `cargo audit` run in CI.
