-- The comparison of maps (Hierarchy, ± Compare maps) in the real window: the page in the frame `#map` asks the app's
-- server for `/api/commits` and `/api/shape/<sha>`, which the app hands to the engine (server.rs). Run with
--   testing sheets step map-compare --headed
-- The project is scripts/e2e/map-history-fixture.mjs's (app `window-maps`): two commits with a committed map. Its
-- OLDER commit has `app.core.cache` and no `app.util`, so against the map on disk one node is added
-- (lua/app/util.lua), one lost (lua/app/core/cache.lua), one require gained and one lost; app.log is the rest.
--
-- Needs the engine of docmap.nvim f1d8c36 or later (engine.lock): before it, `commits` was refused on Windows (a git
-- --format with a % through cmd.exe) and `shape/<sha>` stopped at "module 'docmap.core.diff' not found".
-- Needs tauri-driver and msedgedriver; not part of the CI (a desktop).
-- (A sheet is loaded without the `table` library, so the lines are joined by hand.)
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

local BAR = "document.getElementById('hshape')"
---The four counts of the bar, joined: "+1 node|−1 node|+1 require|−1 require".
local COUNTS = "[...document.querySelectorAll('#hshape .sd-count')].map(e => e.textContent).join('|')"

---The box of a node, as an expression.
---@param id string
---@return string
local function box(id)
	return "document.querySelector('.hnode[data-key=\"" .. id .. "\"]')"
end

return {
	id = "docmap-desktop.map-compare",
	title = "docmap-desktop: comparing the map with an older commit in the window's frame",
	task = "docmap-desktop/kartenvergleich-und-seitenansichten-als-webview-blatt-mit-fr",
	engine = "webview",
	app = "window-maps",
	sizes = { { cols = 1280, rows = 800 } },
	steps = {
		{
			wait = "document.querySelector('#engine-summary').textContent.trim() !== ''",
			timeout = 30000,
			note = "the window is up and the engine was looked for",
		},
		{ wait = ADD, timeout = 30000, note = "the fixture project is added, the page reloads" },
		{
			wait = "[...document.querySelectorAll('#projects option')].some((o) => o.textContent.startsWith('project'))",
			timeout = 30000,
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
		{ frame = "#map" },
		{ wait = "document.querySelector('button[data-tab=\"hierarchy\"]') !== null", timeout = 30000 },
		{ click = "button[data-tab='hierarchy']", note = "the Hierarchy tab" },
		{ focus = "#hshapebtn", note = "± Compare maps, from the keyboard" },
		{ press = "Enter" },
		{
			wait = "!" .. BAR .. ".hidden && document.querySelectorAll('#hshape-a option').length >= 2",
			timeout = 30000,
			note = "the bar lists the commits the app's server reported",
		},
		{
			expect = "(s => { const o = [...s.options].find(o => /older/.test(o.textContent));"
				.. " s.value = o.value; s.dispatchEvent(new Event('change', { bubbles: true })); return true; })"
				.. "(document.getElementById('hshape-a'))",
			note = "the older commit",
		},
		{
			wait = COUNTS .. " === '+1 node|−1 node|+1 require|−1 require'",
			timeout = 60000,
			note = "the bar counts nodes and requires (the engine answered shape/<sha> through the app)",
		},
		{
			expect = box("lua/app/util.lua")
				.. " !== null && "
				.. box("lua/app/util.lua")
				.. ".classList.contains('sd-added')",
			note = "the added node is framed",
		},
		{
			expect = "document.querySelectorAll('.hnode.sd-lost, .hnode .sd-badge').length > 0",
			note = "the place a node was lost from is marked",
		},
		{
			expect = "document.querySelectorAll('.hnode.sd-same').length > 0",
			note = "the rest is dimmed",
		},
		{ shot = "compare-light" },
		{ focus = "#hshape-end" },
		{ press = "Enter", note = "End comparison, from the keyboard" },
		{
			wait = BAR .. ".hidden && document.querySelectorAll('.sd-added, .sd-lost, .sd-same').length === 0",
			note = "every mark is gone",
		},
	},
	judge = {
		"The added node, the lost one and the dimmed rest are easy to tell apart (compare-light), and nothing jumps",
	},
}
