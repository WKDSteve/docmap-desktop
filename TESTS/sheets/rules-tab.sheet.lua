-- The Rules tab in the real window (WebView2) with the bundled engine, at the window's minimum size (900x600):
--   testing sheets step rules-tab --headed
-- The project is the fixture of this repository (TESTS/fixtures/rules: a `.rules.json`, a ruleset of seven rules,
-- three of them automated, one a Lua predicate). It is added through the app's own command, because the folder
-- dialog of "Add project" is a native window no WebDriver reaches; where the checkout lies is read off the engine the
-- app found (the sidecar beside its own program).
--
-- Needs a real engine in src-tauri/binaries (a release build without one has nothing to ask), node, tauri-driver
-- and msedgedriver (testing.nvim, engine "webview"); without them it is a skip. Not part of the CI: the window needs
-- a desktop. No model is asked anywhere in it: Start runs the engine on this machine. The waits are long because an
-- engine call takes seconds on Windows and minutes on a machine that is busy.
-- (A sheet is loaded without the `table` library, so the lines are joined by hand.)
local ADD = "(window.__rulesSeed ||= (async () => {"
	.. "  const invoke = window.__TAURI__.core.invoke;"
	.. "  const info = await invoke('engine_info');"
	.. "  const at = String(info.path || '').replace(/\\\\/g, '/');"
	.. "  const cut = at.indexOf('/src-tauri/');"
	.. "  if (cut < 0) throw new Error('the engine is not the one of this checkout: ' + at);"
	.. "  await invoke('add_project', { root: at.slice(0, cut) + '/TESTS/fixtures/rules/project' });"
	.. "  location.reload();"
	.. "  return true;"
	.. "})())"

local function row(id)
	return "document.querySelector('.rules-row[data-rule=\"" .. id .. "\"]')"
end

return {
	id = "docmap-desktop.rules-tab",
	title = "docmap-desktop: the Rules tab lists, selects and runs the automated rules; a predicate needs trust",
	task = "docmap-desktop/rules-tab-list-and-mechanical-run",
	engine = "webview",
	app = "window",
	sizes = { { cols = 900, rows = 600 } },
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
			note = "the project is in the picker",
		},
		{
			expect = "(() => { const s = document.querySelector('#projects');"
				.. " s.value = [...s.options].find((o) => o.textContent.startsWith('project')).value;"
				.. " s.dispatchEvent(new Event('change')); return true; })()",
			note = "select it",
		},
		{ wait = "!document.querySelector('#projbar').hidden", timeout = 15000 },
		{ click = ".viewtabs [data-pane='rules']" },
		{
			wait = "document.querySelectorAll('.rules-row').length === 7",
			timeout = 240000,
			note = "the engine listed the seven rules (it takes seconds on Windows)",
		},
		{
			expect = "/7 .*3 .*4 /.test(document.querySelector('#rules-state').textContent)",
			note = "7 rules: 3 automated, 4 manual",
		},
		{
			expect = "document.querySelectorAll('.rules-row.auto').length === 3"
				.. " && document.querySelectorAll('.rules-row.manual').length === 4",
		},
		{
			expect = "[...document.querySelectorAll('.rules-row .rules-lane')].every((e) => /^[◆○] \\S/.test(e.textContent))",
			note = "the lane is a shape and a word on every row",
		},
		{
			expect = "document.querySelector('.rules-trust-row.untrusted') !== null",
			note = "the ruleset brings code and is not trusted",
		},
		{ expect = "document.documentElement.scrollWidth <= window.innerWidth", note = "no sideways scroll at 900" },
		{ shot = "list-light" },

		-- the selection: a group's checkbox, the filter, a range
		{ click = ".rules-chip-filter.manual" },
		{ expect = "document.querySelectorAll('.rules-row').length === 4", note = "only the manual ones are shown" },
		{ click = ".rules-group-head input[type=checkbox]" },
		{
			expect = "/ 0 /.test(document.querySelector('#rules-sel-auto').textContent)"
				.. " && !/ 0 /.test(document.querySelector('#rules-sel-manual').textContent)",
			note = "ticking a group under the manual filter selects manual rules and nothing hidden",
		},
		{ click = ".rules-chips .rules-chip-filter" },
		{ expect = "document.querySelectorAll('.rules-row').length === 7", note = "all shown again" },
		{ click = "#rules-by-family" },
		{
			expect = "[...document.querySelectorAll('.rules-group-name')].map((e) => e.textContent).join() === 'M,N,P,Q'",
			note = "by family, sorted",
		},
		{ click = ".rules-row[data-rule='P-01'] input", note = "one rule by its own checkbox" },
		{ click = "#rules-gates button", note = "the gate of .rules.json selects the rest of its families" },
		{
			expect = "/ 3 /.test(document.querySelector('#rules-sel-auto').textContent)"
				.. " && !document.querySelector('#rules-start').disabled",
			note = "three automated selected, Start is on",
		},

		-- the mechanical run, predicate not trusted
		{ click = "#rules-start" },
		{
			wait = row("Q-02")
				.. ".querySelector('.rules-status') !== null && !document.querySelector('#rules-start').disabled",
			timeout = 240000,
			note = "the run came back",
		},
		{ expect = row("Q-01") .. ".querySelector('.rules-status.fail') !== null", note = "the grep found the word" },
		{ expect = row("Q-02") .. ".querySelector('.rules-status.pass') !== null" },
		{
			expect = row("P-01")
				.. ".querySelector('.rules-status.error') !== null"
				.. " && document.querySelector('#rules-list').textContent.includes('predicate not trusted')",
			note = "the untrusted predicate is an error row that says why, not an absent one",
		},
		{
			expect = "[...document.querySelectorAll('.rules-finding-file')].some((b) => b.textContent === 'lua/a.lua:3')",
			note = "the finding names file and line",
		},
		{
			expect = "[...document.querySelectorAll('.rules-agent')].filter((e) => e.textContent).length >= 1"
				.. " && [...document.querySelectorAll('.rules-mech')].filter((e) => e.textContent).length === 3",
			note = "manual rows carry their note in the agent column, results only in the mechanical one",
		},
		{ expect = "!document.querySelector('#proj-rules').hidden", note = "the sidebar counts the project's rules" },
		{ shot = "run-untrusted-light" },

		-- trusting: never the default, and then the predicate runs
		{ click = ".rules-trust-row button" },
		{ wait = "document.querySelector('#trust-predicates li') !== null", timeout = 240000 },
		{
			expect = "document.querySelector('#trust-go').disabled && document.activeElement.id === 'trust-cancel'",
			note = "Trust is off and the focus is on Do not trust",
		},
		{ click = "#trust-predicates button", note = "Show code" },
		{
			wait = "/lua_predicate/.test(document.querySelector('#trust-predicates pre').textContent)",
			timeout = 240000,
			note = "the code of the rule is shown",
		},
		{ shot = "trust-dialog-light" },
		{ click = "#trust-seen" },
		{ expect = "!document.querySelector('#trust-go').disabled" },
		{ click = "#trust-go" },
		{
			wait = "document.querySelector('.rules-trust-row.trusted') !== null",
			timeout = 240000,
			note = "trusted now",
		},
		{ click = "#rules-start" },
		{
			wait = row("P-01") .. ".querySelector('.rules-status.pass') !== null",
			timeout = 240000,
			note = "the predicate ran and passed",
		},

		-- the same in the dark
		{ expect = "(document.documentElement.setAttribute('data-theme', 'dark'), true)" },
		{ shot = "run-trusted-dark" },
		{ click = ".rules-trust-row button", note = "Revoke trust" },
		{ wait = "document.querySelector('.rules-trust-row.untrusted') !== null", timeout = 240000 },
		{ shot = "list-dark" },
	},
	judge = {
		"At 900x600 the list is readable: titles wrap instead of ending after two words, nothing is cut off sideways",
		"Automated and manual rules can be told apart without colour (shape and word)",
		"The selection bar keeps the two counts apart and Start says how many automated rules it runs",
		"A mechanical result and the note 'left for an agent run' sit in different columns",
		"The trust dialog shows the folder, the rule with code and its code; its buttons are in view",
		"Light and dark both read well",
	},
}
