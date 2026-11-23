-- The window started from outside, `docmap-desktop --open-at project lua/app/core/init.lua:3` (what tasks-desktop's
-- "open in DocMap" runs), in the real window (WebView2). Run with
--   testing sheets step open-at --headed
-- The app `window-open-at` (.testing.lua) builds the fixture project of the map views and writes a workspace that
-- already lists it: the window reads the request once at its start, a project added from the page would come too
-- late. Nothing is added or selected here: what the sheet sees is what the start argument did.
--
-- Checked: the project is selected, the map is in the frame at the node that owns the file (`#tab=tree&id=…`; the
-- fixture is a Lua tree without grammars, so there is no function and no `fn=`), the page inside the frame carries
-- the same fragment, and a change of theme (which reloads the frame) keeps it. The refusals are open-at-unknown and
-- open-at-escape. Needs tauri-driver, msedgedriver and a real engine in src-tauri/binaries; not part of the CI.
-- (A sheet is loaded without the `table` library and without `ipairs`.)
local NODE = "lua%2Fapp%2Fcore"

return {
	id = "docmap-desktop.open-at",
	title = "docmap-desktop: --open-at shows the map at the node of the file, and a theme change keeps it",
	task = "docmap-desktop/sheet-fuer-open-at-im-echten-fenster",
	engine = "webview",
	app = "window-open-at",
	sizes = { { cols = 1280, rows = 800 } },
	steps = {
		{
			wait = "!document.getElementById('map').hidden"
				.. " && /^http:\\/\\/127\\.0\\.0\\.1:\\d+\\//.test(document.getElementById('map').src)",
			timeout = 90000,
			note = "the map is in the frame, served by the app, without a click",
		},
		{
			expect = "(() => { const s = document.querySelector('#projects');"
				.. " return s.options[s.selectedIndex].textContent.startsWith('project'); })()",
			note = "the project of the request is the selected one",
		},
		{
			expect = "document.getElementById('map').src.endsWith('#tab=tree&id=" .. NODE .. "')",
			note = "the frame is loaded at the node that owns lua/app/core/init.lua",
		},
		{ frame = "#map", note = "into the page" },
		{
			wait = "location.hash === '#tab=tree&id=" .. NODE .. "'",
			timeout = 30000,
			note = "the page carries the node in its own link",
		},
		{ frame = false },
		{
			expect = "!/not a project|not a path|no such|--open-at needs/i.test(document.getElementById('status').textContent)",
			note = "nothing was refused",
		},
		{ shot = "node-light" },

		-- the theme change reloads the frame: the node stays
		{
			expect = "(s => (s.value = 'dark', s.dispatchEvent(new Event('change')), true))(document.getElementById('theme'))",
			note = "the theme is dark",
		},
		{
			wait = "/[?&]theme=dark/.test(document.getElementById('map').src)",
			timeout = 15000,
			note = "the frame is reloaded with the theme",
		},
		{
			expect = "document.getElementById('map').src.endsWith('#tab=tree&id=" .. NODE .. "')",
			note = "and still at the node",
		},
		{ frame = "#map" },
		{
			wait = "document.documentElement.getAttribute('data-theme') === 'dark'"
				.. " && location.hash === '#tab=tree&id="
				.. NODE
				.. "'",
			timeout = 30000,
			note = "the page is dark, at the same node",
		},
		{ shot = "node-dark" },
		{ frame = false },
		{
			expect = "(s => (s.value = 'system', s.dispatchEvent(new Event('change')), true))(document.getElementById('theme'))",
			note = "the theme back to the system's",
		},
	},
	judge = {
		"The map opens by itself at the node of the file, the tree selects it, nothing else is in the way",
	},
}
