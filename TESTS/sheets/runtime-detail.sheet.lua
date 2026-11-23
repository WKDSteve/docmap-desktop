-- The runtime data in the detail of a function, in the real window (WebView2): the page the app serves itself
-- (`serve_project`, http://127.0.0.1:<port>) in the iframe `#map` asks `/api/telemetry` of the window's server, which
-- asks the engine sidecar (`--api=telemetry`). docmap.nvim checks the page itself against its own server
-- (TESTS/sheets/served-runtime-detail.sheet.lua); this sheet is the way through the window: server.rs, the sidecar and
-- the frame. Run with
--   testing sheets step runtime-detail --headed
-- The app `window-telemetry` (.testing.lua) has a Lua project with a map that has functions
-- (scripts/e2e/telemetry-fixture.mjs) in its workspace and a telemetry store of known content in the run's own folder
-- (scripts/e2e/telemetry-prepare.mjs, telemetry-store.lua: the real runtime-analysis.nvim). The functions:
--   lua/look#M.start               13 calls, 3 in the last week, timed, 1 error, a caller, argument fingerprints
--   lua/look/store.lua#M.load      wrapped, never called: 0 calls
--   lua/look/store.lua#M.old_load  not wrapped: no row at all, "not measured"
--
-- Checked: the numbers of a measured function, the arguments only as fingerprints, and that "not measured" and
-- "measured, never called" are two different answers. The look of the page: abnahme-laufzeitdetail-im-funktionsdetail
-- (by eye). Needs tauri-driver, msedgedriver, Neovim and the plugin checkouts for the store; not part of the CI.
-- (A sheet is loaded without the `table` library and without `ipairs`.)

---The block of a function in the detail pane, as an expression.
---@param key string
---@return string
local function block(key)
	return "document.querySelector('#detail .fn[data-fn=\"" .. key .. "\"]')"
end

---The runtime part of a function's block, as an expression for its text ('' when there is none).
---@param key string
---@return string
local function rt(key)
	local b = block(key)
	return "(" .. b .. " && " .. b .. ".querySelector('.fn-rt') ? " .. b .. ".querySelector('.fn-rt').textContent : '')"
end

---Open the tree at a node (and a function) inside the frame, through the page's own link.
---@param id string
---@param fn string
---@return string
local function goto_fn(id, fn)
	return "(location.hash = '#tab=tree&id=" .. id .. "&fn=" .. fn .. "', true)"
end

local START = "lua/look#M.start"
local LOAD = "lua/look/store.lua#M.load"
local OLD = "lua/look/store.lua#M.old_load"

return {
	id = "docmap-desktop.runtime-detail",
	title = "docmap-desktop: the runtime data in the function detail, measured, never called and not measured",
	task = "docmap-desktop/sheet-laufzeitdetail-im-echten-fenster-fixture-mit",
	engine = "webview",
	app = "window-telemetry",
	sizes = { { cols = 1280, rows = 800 } },
	steps = {
		{
			wait = "document.querySelector('#engine-summary').textContent.trim() !== ''",
			timeout = 60000,
			note = "the window is up and the engine was looked for",
		},
		{
			wait = "[...document.querySelectorAll('#projects option')].some((o) => o.textContent.startsWith('project'))",
			timeout = 60000,
			note = "the project of the workspace is in the picker",
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
			timeout = 240000,
			note = "the map is in the frame, served by the app",
		},
		{ frame = "#map", note = "into the page" },
		{ wait = "document.querySelector('#tree .row') !== null", timeout = 60000, note = "the page is up" },

		-- A function the store measured.
		{ expect = goto_fn("lua%2Flook", "lua%2Flook%23M.start"), note = "the function is opened" },
		{ wait = block(START) .. " !== null", timeout = 30000, note = "the detail shows the function" },
		{
			wait = rt(START) .. ".length > 0",
			timeout = 60000,
			note = "the runtime block arrives: the window's server asked the engine",
		},
		{
			expect = rt(START) .. ".includes('13 calls, 3 in the last week')",
			note = "calls in total and in the last week are two numbers",
		},
		{
			expect = "/duration: mean [0-9.]+ ms, slowest [0-9.]+ ms \\(3 timed\\)/.test(" .. rt(START) .. ")",
			note = "the duration of the timed calls",
		},
		{ expect = rt(START) .. ".includes('errors: 1')", note = "the one error" },
		{
			expect = "/top callers: .*telemetry-store\\.lua:[0-9]+.* \u{00d7}3/.test(" .. rt(START) .. ")",
			note = "the callers, with their count",
		},
		{
			expect = "/top arguments: .*<string:5:[0-9a-f]+>.* \u{00d7}2/.test(" .. rt(START) .. ")",
			note = "the arguments as fingerprints (length and digest), with their count",
		},
		{
			expect = "!" .. rt(START) .. ".includes('alpha') && !" .. rt(START) .. ".includes('boom')",
			note = "never the argument text itself",
		},
		{ shot = "measured" },

		-- A function wrapped and never called, and one that was never measured.
		{
			expect = goto_fn("lua%2Flook%2Fstore.lua", "lua%2Flook%2Fstore.lua%23M.load"),
			note = "the module with the other two",
		},
		{
			wait = rt(LOAD) .. ".length > 0 && " .. rt(OLD) .. ".length > 0",
			timeout = 60000,
			note = "both blocks got their runtime part",
		},
		{
			expect = rt(LOAD) .. ".includes('wrapped, never called')",
			note = "measured and never called",
		},
		{
			expect = "!" .. rt(LOAD) .. ".includes('Not measured') && !" .. rt(LOAD) .. ".includes(' 0 calls')",
			note = "which is neither the 'not measured' answer nor a zero count",
		},
		{
			expect = rt(OLD) .. ".includes('Not measured')",
			note = "a function the store has no row for is 'not measured'",
		},
		{ shot = "unmeasured" },
	},
	judge = {
		"The runtime lines of the function detail read as part of the function, light and not in the way",
		"'Not measured' and 'wrapped, never called' look like two different things",
	},
}
