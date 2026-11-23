-- scripts/e2e/telemetry-store.lua -- the telemetry store of the sheet TESTS/sheets/runtime-detail.sheet.lua, written
-- by the real runtime-analysis.nvim:
--
--   nvim --headless -u NONE -l scripts/e2e/telemetry-store.lua <run dir>
--
-- (run by telemetry-prepare.mjs, the `prepare` step of the app `window-telemetry`, with `LOCALAPPDATA` inside the run
-- dir: the engine the window starts reads the store below `stdpath("state")`, which on Windows is
-- `%LOCALAPPDATA%\nvim-data`). The script refuses to write anywhere else: a fixture must never land in the store of
-- whoever runs it.
--
-- What the store holds, for the namespace of the fixture project (telemetry-fixture.mjs):
--   look.start        13 calls (3 of them today), one raised an error, timed, a caller, fingerprinted arguments
--   look.store.load   wrapped, never called: `calls == 0`
--   look.store.old_load is not wrapped: no row, "not measured"
-- luacheck: globals vim
---@diagnostic disable: undefined-global
local run_dir = arg[1]
assert(run_dir and run_dir ~= "", "usage: telemetry-store.lua <run dir>")

local NAMESPACE = "docmap-sheet-telemetry"

---Put the checkouts of the plugins on the runtimepath: DOCMAP_SHEET_RTP, or lib.nvim and runtime-analysis.nvim next
---to each other (`$REPOS_DIR`, else the folder above this repository's checkout of the family).
local function add_rtp()
	local given = vim.env.DOCMAP_SHEET_RTP
	if given and given ~= "" then
		vim.o.runtimepath = given .. "," .. vim.o.runtimepath
		return
	end
	local base = vim.env.REPOS_DIR
	if not base or base == "" then
		base = "E:/repos"
	end
	vim.o.runtimepath = base .. "/lib.nvim," .. base .. "/runtime-analysis.nvim," .. vim.o.runtimepath
end
add_rtp()

local function die(why)
	io.stderr:write("telemetry-store: " .. why .. "\n")
	os.exit(1)
end

local paths = require("runtime-analysis.telemetry.paths")
local store_dir = vim.fs.normalize(paths.dir())
local run = vim.fs.normalize(run_dir)
if store_dir:lower():sub(1, #run) ~= run:lower() then
	die("the telemetry store " .. store_dir .. " is not below " .. run .. " (LOCALAPPDATA)")
end

local telemetry = require("runtime-analysis.telemetry")
local store = require("runtime-analysis.telemetry.store")
store.clear(NAMESPACE, { dir = store_dir })

local looks = {
	start = function(value)
		if value == "boom" then
			error("fixture failure")
		end
		return value
	end,
}
local stores = {
	load = function(key)
		return key
	end,
}
local inst = telemetry.new({ namespace = NAMESPACE, persist = true })
inst.wrap(looks, "look", { module_id = "look" })
inst.wrap(stores, "look.store", { module_id = "look.store" })
inst.start({ profile_args = true, time = true, errors = true, call_tree = true })
local function call_start(value)
	-- a Lua closure between, so the caller the store records is this file and not `[C]` (pcall)
	return pcall(function()
		local result = looks.start(value)
		return result
	end)
end
call_start("alpha")
call_start("alpha")
call_start("boom")
inst.flush()
inst.stop()

-- topped up: a total that differs from the week, so the sheet can tell "calls" from "this week"
local data = assert(store.load(NAMESPACE, { dir = store_dir }), "the fixture store was not written")
data.functions["look.start"].calls = data.functions["look.start"].calls + 10
assert(store.save(NAMESPACE, data, { dir = store_dir }), "the fixture store could not be saved")
io.stdout:write("telemetry-store: " .. store.data_path(NAMESPACE, { dir = store_dir }) .. "\n")
