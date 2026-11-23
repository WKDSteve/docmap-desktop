-- `docmap-desktop --open-at project ../outside/secret.lua:1` in the real window: a path with a `..` segment leaves
-- the repository, so it is refused: the window says so in its status line and opens no node. Run with
--   testing sheets step open-at-escape --headed
-- The app `window-open-at-escape` (.testing.lua) has the fixture project in its workspace; the project of the request
-- is a known one, only the path is the problem. The accepted case is open-at.sheet.lua.
return {
	id = "docmap-desktop.open-at-escape",
	title = "docmap-desktop: --open-at a path with .. is refused and opens no node",
	task = "docmap-desktop/sheet-fuer-open-at-im-echten-fenster",
	engine = "webview",
	app = "window-open-at-escape",
	sizes = { { cols = 1280, rows = 800 } },
	steps = {
		{
			wait = "/outside.secret/.test(document.getElementById('status').textContent)",
			timeout = 60000,
			note = "the status line names the path that was refused",
		},
		{
			expect = "document.getElementById('map').hidden",
			note = "no map is shown: no node was opened",
		},
		{ shot = "escape" },
	},
	judge = { "The message says only paths relative to a repository are accepted" },
}
