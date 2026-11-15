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
      -- The same window, after a fresh copy of the rules fixture with a finished agent run in it is made below
      -- src-tauri/target (scripts/e2e/rules-run-fixture.mjs, with the engine of this checkout): the sheet of the run
      -- view's decisions writes verdicts and waivers there, never into the tracked fixture.
      ["window-decide"] = {
        build = {
          { "cargo", "tauri", "build", "--no-bundle", "--", "--locked" },
          { "node", "scripts/e2e/rules-run-fixture.mjs" },
        },
        run = { "src-tauri/target/release/docmap-desktop.exe" },
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
