# docmap-desktop

A Tauri 2 desktop workspace for the module maps docmap.nvim generates, plus a web variant of the same window
(one HTML file opened from disk, published as docmap-web). What a user sees is in [README.md](README.md) and
[docs/](docs/); this file holds what an agent working in the repository must know.

## Layout

- `src-tauri/` — the Rust side (commands, the engine sidecar, processes, git/gh). Every process goes through
  `src-tauri/src/proc.rs` (appkit_core::child) or appkit_core::launch; `tools/process-starts.allow` names the rest.
- `src/` — the window: `index.html`, `main.js` (needs `window.__TAURI__`, not unit-tested), the helpers in `src/lib/`
  with their `*.test.js`. Plain JS, no bundler and no package manager for the desktop app.
- `web/` — the web variant (docmap-web): `web/src` is its shell, `web/build.mjs` packs `src/` + engine into
  `web/dist/docmap.html` (esbuild, `target: 'es2022'`, a meta CSP that forbids every request). pnpm.
- `TESTS/sheets/` — testing.nvim sheets; `.testing.lua` says how the apps start.
- `docs/` — user documentation; [docs/WEB.md](docs/WEB.md) for the web variant, [docs/PROCESSES.md](docs/PROCESSES.md)
  for every program the window starts.

## Checks (what `.github/workflows/ci.yml` runs)

- Rust, in `src-tauri/`: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test`
  (CI stages placeholder sidecar/grammars files first, see ci.yml).
- Frontend: `node --check` on `src/*.js` and `src/lib/*.js`, then `node --test src/lib/*.test.js`.
- Tools: `python tools/check-colors.py` (no colour outside the appkit tokens), `python tools/check-process-starts.py`,
  `python -m unittest discover -s tools -p "test_*.py"`, plus the pin checks of appkit and release-kit.
- Web, in `web/`: `pnpm install --frozen-lockfile`, `pnpm test`, `pnpm build`, `pnpm e2e` (needs the engine sources
  of `engine.lock`, see the job `web`).

Commits: conventional style with a scope (`fix(import): …`, `build(engine): …`), a body that says why.

## Web platform
- Browser Support: Baseline Widely available (`.mwgrc`, `browserslist` in `web/package.json`). Newer features only behind feature detection with a fallback; polyfills only via dynamic `import()`.
- Before writing or changing HTML, CSS or client-side JS, use the `modern-web-guidance` skill (`search` → `retrieve` → verify). Not for Rust, backend, CI or build config.
- For open-ended modernisation: `npx -y modern-web-guidance@latest legacy-scan <ui-dir> --json` (`src/`, `web/src`), check each finding against `keepWhen`/`dropWhen`.
- Rules: rules-vault ruleset `web` (plus the app rules `WEB-*` in ruleset `native`).
- `src/main.js` still assigns `innerHTML` in about 30 places. That is known and tracked in tasks-vault as
  `docmap-desktop/innerhtml-durch-dom-apis-und-trusted-types`; do not add new ones (rule WEB-01) — build nodes with
  DOM APIs (`createElement`, `textContent`, `replaceChildren`) — and do not refactor the existing ones outside that task.
