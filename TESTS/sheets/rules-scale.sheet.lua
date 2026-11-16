-- The Rules tab in the real window (WebView2) with a ruleset of the size of a real one, at the window's minimum size:
--   testing sheets step rules-scale --headed
-- The project is written by scripts/e2e/rules-scale-fixture.mjs (the app `window-scale` of .testing.lua runs it after
-- the build): the fixture of rules-tab.sheet.lua plus 325 manual rules in 13 families, one file per rule, as the
-- author's own ruleset has them. It is added through the app's own command (the folder dialog is a native window no
-- WebDriver reaches); where the checkout lies is read off the engine the app found.
--
-- What rules-tab.sheet.lua shows on seven rules, this holds for hundreds: the list comes, nothing runs out of the
-- window sideways at 900x600, the manual filter and a family's box select only manual rules, a click with Shift
-- selects a range, Start with a mixed selection runs only the automated rules and sends nothing, and Enter in the
-- trust dialog does not trust. A person still judges the look (the task's acceptance list); "changed" after an edit
-- of the ruleset needs a file written during the run, which a sheet cannot do.
--
-- Needs a real engine in src-tauri/binaries, node, tauri-driver and msedgedriver (testing.nvim, engine "webview");
-- without them it is a skip. Not part of the CI: the window needs a desktop. No model is asked.
-- (A sheet is loaded without the `table` library, so the lines are joined by hand.)
local ADD = "(window.__rulesSeed ||= (async () => {"
	.. "  const invoke = window.__TAURI__.core.invoke;"
	.. "  const info = await invoke('engine_info');"
	.. "  const at = String(info.path || '').replace(/\\\\/g, '/');"
	.. "  const cut = at.indexOf('/src-tauri/');"
	.. "  if (cut < 0) throw new Error('the engine is not the one of this checkout: ' + at);"
	.. "  const fixture = '/src-tauri/target/sheet-fixtures/rules-scale/project';"
	.. "  await invoke('add_project', { root: at.slice(0, cut) + fixture });"
	.. "  location.reload();"
	.. "  return true;"
	.. "})())"

local function row(id)
	return "document.querySelector('.rules-row[data-rule=\"" .. id .. "\"]')"
end

local function box(id)
	return row(id) .. ".querySelector('input[type=checkbox]')"
end

local CHOSEN = "[...document.querySelectorAll('.rules-row input[type=checkbox]')].filter((b) => b.checked)"
local NO_SIDEWAYS = "document.documentElement.scrollWidth <= window.innerWidth"
	.. " && document.querySelector('#rules-list').scrollWidth <= document.querySelector('#rules-list').clientWidth + 1"

return {
	id = "docmap-desktop.rules-scale",
	title = "docmap-desktop: the Rules tab with 332 rules at 900x600: list, filter, range, mixed run, trust by Enter",
	task = "docmap-desktop/rules-tab-in-der-installierten-app-ansehen",
	engine = "webview",
	app = "window-scale",
	sizes = { { cols = 900, rows = 600 } },
	steps = {
		{
			wait = "document.querySelector('#engine-summary').textContent.trim() !== ''",
			timeout = 30000,
			note = "the window is up and the engine was looked for",
		},
		{ wait = ADD, timeout = 30000, note = "the project is added, the page reloads" },
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
			wait = "document.querySelectorAll('.rules-row').length === 332",
			timeout = 240000,
			note = "the engine listed all 332 rules (the first read of a large ruleset takes seconds)",
		},
		{ expect = NO_SIDEWAYS, note = "nothing runs out of the window sideways at 900x600" },
		{ shot = "list-light" },

		-- the manual filter and a family's box
		{ click = ".rules-chip-filter.manual" },
		{
			expect = "document.querySelectorAll('.rules-row').length === 329",
			note = "only the manual rules are shown (332 less the three automated)",
		},
		{ click = ".rules-group-head input[type=checkbox]" },
		{
			expect = CHOSEN
				.. ".length > 0 && "
				.. CHOSEN
				.. ".every((b) => b.closest('.rules-row').classList.contains('manual'))",
			note = "a family's box under the manual filter selects manual rules only",
		},
		{ expect = NO_SIDEWAYS, note = "still nothing sideways with a selection bar" },
		{ click = ".rules-group-head input[type=checkbox]", note = "and clears them again" },
		{ expect = CHOSEN .. ".length === 0" },
		{ click = ".rules-chips .rules-chip-filter" },
		{ expect = "document.querySelectorAll('.rules-row').length === 332", note = "all shown again" },

		-- a range by Shift: WebDriver clicks without modifiers, so the click with Shift is dispatched on the box
		{ click = "#rules-by-family" },
		{ click = ".rules-row[data-rule='B-03'] input", note = "the anchor" },
		{
			expect = "("
				.. box("B-12")
				.. ".dispatchEvent(new MouseEvent('click', { shiftKey: true, bubbles: true })), true)",
			note = "Shift and a click on B-12",
		},
		{
			wait = CHOSEN
				.. ".map((b) => b.closest('.rules-row').dataset.rule).join(',') === "
				.. "'B-03,B-04,B-05,B-06,B-07,B-08,B-09,B-10,B-11,B-12'",
			timeout = 5000,
			note = "the ten rules from the anchor to the click are chosen, nothing else",
		},

		-- a mixed selection: the automated rules run, the manual ones are left for an agent run, nothing is sent
		{
			expect = "(" .. row("Q-02") .. ".scrollIntoView({ block: 'center' }), true)",
			note = "the row away from the selection bar, which sticks to the bottom of the list",
		},
		{ click = ".rules-row[data-rule='Q-02'] input", note = "one automated rule to the ten manual ones" },
		{ expect = "!document.querySelector('#rules-start').disabled", note = "Start is on" },
		{ click = "#rules-start" },
		{
			wait = row("Q-02")
				.. ".querySelector('.rules-status') !== null && !document.querySelector('#rules-start').disabled",
			timeout = 240000,
			note = "the run came back",
		},
		{
			expect = row("Q-02") .. ".querySelector('.rules-status.pass') !== null",
			note = "the automated rule has a result",
		},
		{
			expect = row("B-05")
				.. ".querySelector('.rules-mech').textContent === ''"
				.. " && "
				.. row("B-05")
				.. ".querySelector('.rules-agent').textContent !== ''",
			note = "a manual rule has no mechanical result, only the note in the agent column",
		},
		{ expect = NO_SIDEWAYS, note = "nothing sideways with results in the rows" },
		{ shot = "mixed-run-light" },

		-- Enter in the trust dialog does not trust
		{ click = ".rules-trust-row button" },
		{ wait = "document.querySelector('#trust-predicates li') !== null", timeout = 240000 },
		{
			expect = "document.querySelector('#trust-go').disabled && document.activeElement.id === 'trust-cancel'",
			note = "Trust is off and the focus is on Do not trust",
		},
		{ press = "Enter" },
		{
			wait = "!document.querySelector('#trustbox').open",
			timeout = 5000,
			note = "Enter closes the dialog through Do not trust",
		},
		{
			expect = "document.querySelector('.rules-trust-row.trusted') === null",
			note = "and trusts nothing",
		},

		-- the same list in the dark
		{ expect = "(document.documentElement.setAttribute('data-theme', 'dark'), true)" },
		{ expect = NO_SIDEWAYS },
		{ shot = "list-dark" },
	},
	judge = {
		"With 332 rules at 900x600 the list is readable: families are grouped, long titles wrap, nothing is cut off sideways",
		"Automated and manual rules can be told apart without colour (shape and word)",
		"After the mixed run the mechanical result and the note for the agent sit in different columns",
		"Light and dark both read well",
	},
}
