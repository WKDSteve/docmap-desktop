-- The views of the map page in the real window (WebView2), light and dark: the page the app serves itself
-- (`serve_project`, http://127.0.0.1:<port>) in the iframe `#map`, a document of another origin, reached with the
-- step `frame` (testing.nvim 88d17ad). Run with
--   testing sheets step map-views --headed
-- The project is scripts/e2e/map-history-fixture.mjs's (app `window-maps` builds it): a small Lua tree with a
-- committed map. It is added through the app's own command, as in rules-tab.sheet.lua (the folder dialog is a native
-- window no WebDriver reaches).
--
-- Checked here: each view opens from the keyboard and draws, the link carries it, and what the page reports through
-- its channel reaches the window (the note under the map for the Types view). A theme change reloads the frame with
-- `?theme=`; the page must then be dark. Needs tauri-driver and msedgedriver; not part of the CI (a desktop).
-- (A sheet is loaded without the `table` library and without `ipairs`: lines are joined and lists walked by hand.)
local ADD = "(window.__mapSeed ||= (async () => {"
	.. "  const invoke = window.__TAURI__.core.invoke;"
	.. "  const info = await invoke('engine_info');"
	.. "  const at = String(info.path || '').replace(/\\\\/g, '/');"
	.. "  const cut = at.indexOf('/src-tauri/');"
	.. "  if (cut < 0) throw new Error('the engine is not the one of this checkout: ' + at);"
	.. "  const root = at.slice(0, cut) + '/src-tauri/target/sheet-fixtures/map-history/project';"
	.. "  await invoke('add_project', { root });"
	.. "  location.reload();"
	.. "  return true;"
	.. "})())"

---Something is drawn for the current view: the zooming diagram has shapes, or the panel that replaces it has text.
local DRAWN = "(() => { const a = document.getElementById('halt'); const s = document.getElementById('hsvg');"
	.. " return (a && !a.hidden && a.textContent.trim() !== '') || (s && s.querySelectorAll('*').length > 0); })()"

---The steps that open one view of the Hierarchy from the keyboard and check it, ending with a shot.
---@param view string
---@param theme string
---@return table[]
local function view_steps(view, theme)
	-- A Lua tree has no types: the Types view is empty, which is the case the window explains under the map.
	local drawn = view == "types" and "true" or DRAWN
	return {
		{ focus = "button.hview-btn[data-view='" .. view .. "']", note = view .. ": reached with the keyboard" },
		{ press = "Enter" },
		{
			wait = "/view=" .. view .. "/.test(location.hash) && " .. drawn,
			timeout = 15000,
			note = view .. ": the link carries the view and it draws",
		},
		{ shot = view .. "-" .. theme },
	}
end

local steps = {
	{
		wait = "document.querySelector('#engine-summary').textContent.trim() !== ''",
		timeout = 30000,
		note = "the window is up and the engine was looked for",
	},
	{ wait = ADD, timeout = 30000, note = "the fixture project is added, the page reloads" },
	{
		wait = "[...document.querySelectorAll('#projects option')].some((o) => o.textContent.startsWith('project'))",
		timeout = 30000,
		note = "the project is in the picker",
	},
	{
		expect = "(() => { const s = document.querySelector('#projects');"
			.. " s.value = [...s.options].find((o) => o.textContent.startsWith('project')).value;"
			.. " s.dispatchEvent(new Event('change')); return true; })()",
		note = "select it",
	},
	{
		wait = "!document.getElementById('map').hidden"
			.. " && /^http:\\/\\/127\\.0\\.0\\.1:\\d+\\//.test(document.getElementById('map').src)",
		timeout = 60000,
		note = "the map is in the frame, served by the app",
	},
	{ frame = "#map", note = "into the page" },
	{ wait = "document.querySelector('button[data-tab=\"hierarchy\"]') !== null", timeout = 30000 },
	{ click = "button[data-tab='hierarchy']", note = "the Hierarchy tab" },
	{ wait = "/tab=hierarchy/.test(location.hash)", timeout = 15000 },
}

-- Not Routes: it draws the route trees of web frameworks, which a Lua tree has none of (docmap.nvim's browser sheet
-- routes-view covers it); what this sheet adds is the page in the app's frame, and the views here show that.
local VIEWS = { "coupling", "rings", "layers", "types" }

---Append the steps of every view in `theme`.
---@param theme string
local function add_views(theme)
	for v = 1, #VIEWS do
		local more = view_steps(VIEWS[v], theme)
		for i = 1, #more do
			steps[#steps + 1] = more[i]
		end
	end
end
add_views("light")

local tail = {
	{ frame = false, note = "back to the window" },
	{
		wait = "!document.getElementById('context-note').hidden"
			.. " && document.getElementById('context-note').textContent.trim() !== ''",
		timeout = 15000,
		note = "the page reported the Types view through its channel and the window explains it under the map",
	},

	-- dark: the window reloads the frame with ?theme=dark
	{
		expect = "(s => (s.value = 'dark', s.dispatchEvent(new Event('change')), true))(document.getElementById('theme'))",
		note = "the theme is dark",
	},
	{
		wait = "/[?&]theme=dark/.test(document.getElementById('map').src)",
		timeout = 15000,
		note = "the frame is reloaded with the theme",
	},
	{ frame = "#map" },
	{
		wait = "document.documentElement.getAttribute('data-theme') === 'dark'"
			.. " && document.querySelector('button[data-tab=\"hierarchy\"]') !== null",
		timeout = 30000,
		note = "the page is dark",
	},
	{ click = "button[data-tab='hierarchy']" },
}
for i = 1, #tail do
	steps[#steps + 1] = tail[i]
end
add_views("dark")
steps[#steps + 1] = { frame = false }
steps[#steps + 1] = {
	expect = "(s => (s.value = 'system', s.dispatchEvent(new Event('change')), true))(document.getElementById('theme'))",
	note = "the theme back to the system's",
}

return {
	id = "docmap-desktop.map-views",
	title = "docmap-desktop: the views of the map in the window's frame, light and dark, from the keyboard",
	task = "docmap-desktop/kartenvergleich-und-seitenansichten-als-webview-blatt-mit-fr",
	engine = "webview",
	app = "window-maps",
	sizes = { { cols = 1280, rows = 800 } },
	steps = steps,
	judge = {
		"Each view (coupling, rings, layers, types) reads well in the window, light and dark:"
			.. " nothing cut off, no jump",
		"The note under the map for the Types view says why the view is what it is",
	},
}
