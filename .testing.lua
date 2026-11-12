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
        },
      },
    },
  },
}
