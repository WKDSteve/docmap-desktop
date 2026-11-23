-- `docmap-desktop --open-at no-such-project lua/app/core/init.lua:3` in the real window: the project is not one the
-- app knows, so the window says so in its status line and opens nothing. Nothing is searched for on the disk, nothing
-- is added. Run with
--   testing sheets step open-at-unknown --headed
-- The app `window-open-at-unknown` (.testing.lua) has the fixture project in its workspace, which is the point: the
-- name of the request is matched against the known projects and nothing else. The accepted case is open-at.sheet.lua.
return {
	id = "docmap-desktop.open-at-unknown",
	title = "docmap-desktop: --open-at a project the app does not know says so and opens nothing",
	task = "docmap-desktop/sheet-fuer-open-at-im-echten-fenster",
	engine = "webview",
	app = "window-open-at-unknown",
	sizes = { { cols = 1280, rows = 800 } },
	steps = {
		{
			wait = "/no-such-project/.test(document.getElementById('status').textContent)",
			timeout = 60000,
			note = "the status line names the project that is not known",
		},
		{
			expect = "document.getElementById('map').hidden",
			note = "no map is shown",
		},
		{
			expect = "![...document.querySelector('#projects').options].some((o) => /no-such-project/.test(o.textContent))",
			note = "nothing was added: the project of the request is not in the picker",
		},
		{ shot = "unknown" },
	},
	judge = { "The message is readable and says what to do (add the project first)" },
}
