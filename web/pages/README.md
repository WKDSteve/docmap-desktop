# docmap-web

The window of [docmap-desktop](https://github.com/WKDSteve/docmap-desktop) as a single HTML file that Chrome or Edge
opens from disk (`file://`). No install, no program, no server: the file makes no network request (its policy
forbids it), the engine of [docmap.nvim](https://github.com/WKDSteve/docmap.nvim) runs inside the page, and a project
is a folder you pick in the browser.

This repository holds the built page, its sum and the notices of what it bundles. The sources live in
docmap-desktop (`web/`); its workflow `web.yml` writes here, this README included. Do not edit the files by hand.

## Using it

```
git clone https://github.com/WKDSteve/docmap-web.git
```

1. Check the file: `Get-FileHash docmap.html` must match the line in `SHA256SUMS`.
2. Open `file:///C:/repos/docmap-web/docmap.html` in Chrome or Edge (adjust the path).
3. Choose **Add project…**, then **Choose a folder…**, and pick a repository (or a folder that holds several: the
   page lists the checkouts inside it).
4. A project without a map is generated at once; a map that already lies in the folder (`docs/map`) is shown as it
   is, and **Generate map** makes a new one. The page reads the folder and writes nothing into it: a generated map
   lives in the tab until you close it.
5. After a restart of the browser the strip under the menu bar shows **Allow access**: one click, no picker.
6. A folder that changed on disk is read again when the tab gets the focus back, or within 30 seconds.

Update later with `git pull`. The page says its build under **Help > About**.

## What it is not

There is no program behind the page, so some things of the app are missing, and the page says so itself (the strip
under the menu bar, **What is different here**): function-level facts in a map generated here (the page has no
parser), the git history, telemetry of a Neovim session, rule runs with an agent, statistics, opening a file in your
editor, cloning from a URL and anything from GitHub.

The whole comparison, the limits (20 000 files and 256 MiB per project) and what the page can and cannot reach are in
[docs/WEB.md](https://github.com/WKDSteve/docmap-desktop/blob/main/docs/WEB.md) of docmap-desktop.

## Licenses

The page is MIT (`LICENSE`). What it bundles from others, with their notices: `THIRD-PARTY-NOTICES.md`.
