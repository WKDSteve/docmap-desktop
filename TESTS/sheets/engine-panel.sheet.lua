-- The engine panel of the sidebar in the real window (WebView2), with an empty workspace of the run:
--   testing sheets step engine-panel --headed
-- Needs node, tauri-driver and msedgedriver (testing.nvim, engine "webview"); without them it is a skip. Not part of
-- the CI: the window needs a desktop.
return {
  id = "docmap-desktop.engine-panel",
  title = "docmap-desktop: the engine panel opens and closes, its verdict readable while collapsed",
  task = "docmap-desktop/live-test-installed-app",
  engine = "webview",
  app = "window",
  sizes = { { cols = 1400, rows = 900 } },
  steps = {
    {
      wait = "document.querySelector('#engine-summary').textContent.trim() !== ''",
      timeout = 30000,
      note = "the window is up and the engine was looked for",
    },
    { expect = "!document.querySelector('#engine').open", note = "collapsed at the start" },
    {
      expect = "/ready|grammars|not found/.test(document.querySelector('#engine-summary').textContent)",
      note = "the verdict is in the summary line while collapsed",
    },
    { shot = "collapsed" },
    { click = "#engine summary", note = "the summary opens it" },
    { expect = "document.querySelector('#engine').open" },
    {
      expect = "document.querySelector('#engine-state').children.length > 0",
      note = "the facts are rows",
    },
    { shot = "open" },
    { focus = "#engine summary" },
    { press = "Enter", note = "the keyboard closes it again" },
    { expect = "!document.querySelector('#engine').open" },
  },
  judge = {
    "The summary line is readable while the panel is collapsed",
    "The facts of the open panel read as rows (binary, grammars, what it reads)",
  },
}
