-- .testing.lua -- what the test sheets of this repository run against (testing.nvim, docs/SHEETS.md there). The sheets
-- in TESTS/sheets only name an app; how it starts is here, under testing.nvim's trust gate. docmap-desktop has no Lua
-- spec.

return {
	sheets = {
		apps = {
			-- The release build of this checkout in the WebView2 of its own window, started by tauri-driver (engine
			-- "webview"). Everything it writes goes to a folder of the run: its config directory (projects, workspaces,
			-- clones; DOCMAP_DESKTOP_CONFIG_DIR), the page's storage (WEBVIEW2_USER_DATA_FOLDER) and the Neovim folders it
			-- reads (LOCALAPPDATA). Never the user's own.
			["window"] = {
				build = { "cargo", "tauri", "build", "--no-bundle", "--", "--locked" },
				-- A release rebuild alone takes about 8 minutes and more on a busy machine: 30 minutes per build step
				-- (testing.nvim: build_timeout_ms, default 10).
				build_timeout_ms = 1800000,
				run = { "src-tauri/target/release/docmap-desktop.exe" },
				env = {
					DOCMAP_DESKTOP_CONFIG_DIR = "${temp}/config",
					WEBVIEW2_USER_DATA_FOLDER = "${temp}/webview",
					LOCALAPPDATA = "${temp}/local",
					-- The rulesets of a sheet are the fixture's, never a vault or a trusted folder of whoever runs it.
					RULES_VAULT = "",
					RULES_TRUST = "",
				},
			},
			-- The same window, after a ruleset of the size of a real one is written below src-tauri/target
			-- (scripts/e2e/rules-scale-fixture.mjs): 332 rules, one file per manual rule, for the Rules tab at scale.
			["window-scale"] = {
				build = {
					{ "cargo", "tauri", "build", "--no-bundle", "--", "--locked" },
					{ "node", "scripts/e2e/rules-scale-fixture.mjs" },
				},
				build_timeout_ms = 1800000,
				run = { "src-tauri/target/release/docmap-desktop.exe" },
				env = {
					DOCMAP_DESKTOP_CONFIG_DIR = "${temp}/config",
					WEBVIEW2_USER_DATA_FOLDER = "${temp}/webview",
					LOCALAPPDATA = "${temp}/local",
					RULES_VAULT = "",
					RULES_TRUST = "",
				},
			},
			-- The same window, after a Lua project with two commits of a committed map is written below src-tauri/target
			-- (scripts/e2e/map-history-fixture.mjs, with the engine of this checkout): the map's views and the comparison
			-- of maps in the window's frame.
			["window-maps"] = {
				build = {
					{ "cargo", "tauri", "build", "--no-bundle", "--", "--locked" },
					{ "node", "scripts/e2e/map-history-fixture.mjs" },
				},
				build_timeout_ms = 1800000,
				run = { "src-tauri/target/release/docmap-desktop.exe" },
				env = {
					DOCMAP_DESKTOP_CONFIG_DIR = "${temp}/config",
					WEBVIEW2_USER_DATA_FOLDER = "${temp}/webview",
					LOCALAPPDATA = "${temp}/local",
					RULES_VAULT = "",
					RULES_TRUST = "",
				},
			},
			-- The same window, after a fresh copy of the rules fixture with a finished agent run in it is made below
			-- src-tauri/target (scripts/e2e/rules-run-fixture.mjs, with the engine of this checkout): the sheet of the run
			-- view's decisions writes verdicts and waivers there, never into the tracked fixture.
			["window-decide"] = {
				build = {
					{ "cargo", "tauri", "build", "--no-bundle", "--", "--locked" },
					{ "node", "scripts/e2e/rules-run-fixture.mjs" },
				},
				build_timeout_ms = 1800000,
				run = { "src-tauri/target/release/docmap-desktop.exe" },
				env = {
					DOCMAP_DESKTOP_CONFIG_DIR = "${temp}/config",
					WEBVIEW2_USER_DATA_FOLDER = "${temp}/webview",
					LOCALAPPDATA = "${temp}/local",
					RULES_VAULT = "",
					RULES_TRUST = "",
				},
			},
			-- The same window over a Lua project with functions and a telemetry store of known content, both in the run's
			-- own folder: what the page of the map shows of the runtime data in the detail of a function, reached
			-- through the window's server (`/api/telemetry`, the engine as a sidecar). The project has a map with
			-- functions (telemetry-fixture.mjs, grammars of this checkout); `prepare` writes the workspace that lists
			-- it and the store, with the real runtime-analysis.nvim under Neovim (telemetry-prepare.mjs; needs
			-- Neovim and the checkouts of lib.nvim and runtime-analysis.nvim). The engine reads the store below
			-- the nvim-data folder below the run's LOCALAPPDATA (`%LOCALAPPDATA%/nvim-data`), which is in the run's folder.
			["window-telemetry"] = {
				build = {
					{ "cargo", "tauri", "build", "--no-bundle", "--", "--locked" },
					{ "node", "scripts/e2e/telemetry-fixture.mjs" },
				},
				build_timeout_ms = 1800000,
				prepare = { "node", "scripts/e2e/telemetry-prepare.mjs", "${temp}" },
				run = { "src-tauri/target/release/docmap-desktop.exe" },
				env = {
					DOCMAP_DESKTOP_CONFIG_DIR = "${temp}/config",
					WEBVIEW2_USER_DATA_FOLDER = "${temp}/webview",
					LOCALAPPDATA = "${temp}/local",
					RULES_VAULT = "",
					RULES_TRUST = "",
				},
			},
			-- The window started from outside: `--open-at <project> <file>:<line>` (what tasks-desktop's "open in DocMap"
			-- runs). The window reads it once at its start, so the project is known before it starts: the fixture
			-- of the map views (map-history-fixture.mjs) is built, and `prepare` writes a workspace that lists it
			-- into the run's config folder (open-at-config.mjs). Three apps, one start each: what is asked is the
			-- command line, and the window takes it only once.
			["window-open-at"] = {
				build = {
					{ "cargo", "tauri", "build", "--no-bundle", "--", "--locked" },
					{ "node", "scripts/e2e/map-history-fixture.mjs" },
				},
				build_timeout_ms = 1800000,
				prepare = { "node", "scripts/e2e/open-at-config.mjs", "${temp}/config" },
				run = {
					"src-tauri/target/release/docmap-desktop.exe",
					"--open-at=project|lua/app/core/init.lua:3",
				},
				env = {
					DOCMAP_DESKTOP_CONFIG_DIR = "${temp}/config",
					WEBVIEW2_USER_DATA_FOLDER = "${temp}/webview",
					LOCALAPPDATA = "${temp}/local",
					RULES_VAULT = "",
					RULES_TRUST = "",
				},
			},
			-- The same, for a project the app does not know: said in the status line, nothing opens.
			["window-open-at-unknown"] = {
				build = {
					{ "cargo", "tauri", "build", "--no-bundle", "--", "--locked" },
					{ "node", "scripts/e2e/map-history-fixture.mjs" },
				},
				build_timeout_ms = 1800000,
				prepare = { "node", "scripts/e2e/open-at-config.mjs", "${temp}/config" },
				run = {
					"src-tauri/target/release/docmap-desktop.exe",
					"--open-at=no-such-project|lua/app/core/init.lua:3",
				},
				env = {
					DOCMAP_DESKTOP_CONFIG_DIR = "${temp}/config",
					WEBVIEW2_USER_DATA_FOLDER = "${temp}/webview",
					LOCALAPPDATA = "${temp}/local",
					RULES_VAULT = "",
					RULES_TRUST = "",
				},
			},
			-- And for a path that leaves the repository (a `..` segment): refused, said, nothing opens.
			["window-open-at-escape"] = {
				build = {
					{ "cargo", "tauri", "build", "--no-bundle", "--", "--locked" },
					{ "node", "scripts/e2e/map-history-fixture.mjs" },
				},
				build_timeout_ms = 1800000,
				prepare = { "node", "scripts/e2e/open-at-config.mjs", "${temp}/config" },
				run = {
					"src-tauri/target/release/docmap-desktop.exe",
					"--open-at=project|../outside/secret.lua:1",
				},
				env = {
					DOCMAP_DESKTOP_CONFIG_DIR = "${temp}/config",
					WEBVIEW2_USER_DATA_FOLDER = "${temp}/webview",
					LOCALAPPDATA = "${temp}/local",
					RULES_VAULT = "",
					RULES_TRUST = "",
				},
			},
		},
	},
}
