-- The decisions of the run view in the real window (WebView2) with the bundled engine, at the window's minimum size
-- (900x600):
--   testing sheets step rules-run-decide --headed
-- The project is a fresh copy of TESTS/fixtures/rules with a finished agent run in it, made by the build step of the
-- app "window-decide" (.testing.lua: scripts/e2e/rules-run-fixture.mjs, below src-tauri/target, so what the engine
-- writes never lands in the tracked fixture). In the run: M-01 "no violation found" and M-02 a violation, both
-- current; M-03 answered for an earlier request (outdated: its files changed); N-01 not sent. The sheet accepts M-02,
-- waives M-01, rejects M-03 and looks at what each wrote, through the app's own reader of the two files, and at what
-- the Rules tab and the sidebar make of it. **No model is asked**: the run exists before the window opens, and
-- ai-desktop is pointed at a port nobody listens on.
--
-- Needs a real engine with `rules-write` in src-tauri/binaries, node, tauri-driver and msedgedriver; without them it
-- is a skip. Not part of the CI. The waits are long because an engine call takes seconds on Windows.
-- (A sheet is loaded without the `table` library, so the lines are joined by hand.)
local ADD = "(window.__rulesSeed ||= (async () => {"
	.. "  const invoke = window.__TAURI__.core.invoke;"
	.. "  const info = await invoke('engine_info');"
	.. "  const at = String(info.path || '').replace(/\\\\/g, '/');"
	.. "  const cut = at.indexOf('/src-tauri/');"
	.. "  if (cut < 0) throw new Error('the engine is not the one of this checkout: ' + at);"
	.. "  await invoke('agent_url', { set: 'http://127.0.0.1:9' });"
	.. "  const project = at.slice(0, cut) + '/src-tauri/target/sheet-fixtures/rules-decide/project';"
	.. "  await invoke('add_project', { root: project });"
	.. "  location.reload();"
	.. "  return true;"
	.. "})())"

-- What the two files hold now, as the Rules tab reads them.
local function judged(test)
	return "(async () => { const s = document.querySelector('#projects');"
		.. " const j = await window.__TAURI__.core.invoke('rules_judged', { id: s.value });"
		.. " return "
		.. test
		.. "; })()"
end

local function set(selector, value)
	return "(() => { const el = document.querySelector('"
		.. selector
		.. "'); el.value = '"
		.. value
		.. "';"
		.. " el.dispatchEvent(new Event('input')); return true; })()"
end

local DETAIL = "document.querySelector('#rules-run-detail')"

return {
	id = "docmap-desktop.rules-run-decide",
	title = "docmap-desktop: a proposal becomes a verdict, a waiver or nothing, one at a time;"
		.. " an outdated one cannot be accepted",
	task = "docmap-desktop/rules-run-accept-waive-reject",
	engine = "webview",
	app = "window-decide",
	sizes = { { cols = 900, rows = 600 } },
	steps = {
		{
			wait = "document.querySelector('#engine-summary').textContent.trim() !== ''",
			timeout = 30000,
			note = "the window is up and the engine was looked for",
		},
		{ wait = ADD, timeout = 30000, note = "the copy with the finished run is added, ai-desktop points nowhere" },
		{
			wait = "[...document.querySelectorAll('#projects option')].some((o) => o.textContent.startsWith('project'))",
			timeout = 30000,
		},
		{
			expect = "(() => { const s = document.querySelector('#projects');"
				.. " s.value = [...s.options].find((o) => o.textContent.startsWith('project')).value;"
				.. " s.dispatchEvent(new Event('change')); return true; })()",
			note = "select the project",
		},
		{ wait = "!document.querySelector('#projbar').hidden", timeout = 15000 },
		{ click = ".viewtabs [data-pane='rules']" },
		{ wait = "document.querySelectorAll('.rules-row').length === 7", timeout = 240000, note = "the list is there" },
		{
			expect = "/\\b4\\b/.test(document.querySelector('#proj-rules').textContent)"
				.. " && !document.querySelector('.rules-judged')",
			note = "nothing decided yet: the four manual rules are open, and no row shows a verdict",
		},
		{
			wait = "!document.querySelector('#rules-runs').hidden"
				.. " && document.querySelector('[data-fk=\"run:r-sheet-decide\"]')",
			timeout = 30000,
			note = "the prepared run is listed",
		},
		{ click = "[data-fk='run:r-sheet-decide']" },
		{
			wait = "!document.querySelector('#rules-run').hidden"
				.. " && document.querySelectorAll('#rules-run-queue li').length === 4",
			timeout = 30000,
			note = "the run view: three proposals and the rule that was not sent",
		},

		-- the outdated one
		{ click = "[data-fk='runrow:M-03']" },
		{
			wait = DETAIL .. ".querySelector('.rules-run-stale') !== null",
			timeout = 240000,
			note = "the engine planned the run again: M-03 is marked outdated, in words",
		},
		{
			expect = "document.querySelector('[data-fk=\"decide:accept\"]').disabled"
				.. " && !document.querySelector('[data-fk=\"decide:waive\"]').disabled"
				.. " && !document.querySelector('[data-fk=\"decide:reject\"]').disabled"
				.. " && document.querySelector('[data-fk=\"runrow:M-03\"] .rules-run-mark').textContent.startsWith('!')",
			note = "it cannot be accepted, it can still be waived or rejected, and its row says outdated",
		},
		{ shot = "outdated-light" },

		-- accept a current violation
		{ click = "[data-fk='runrow:M-02']" },
		{
			wait = "!document.querySelector('[data-fk=\"decide:accept\"]').disabled",
			timeout = 30000,
			note = "M-02 is current: Accept is on",
		},
		{
			expect = "[...document.querySelectorAll('#rules-run button')].every((b) => !/\\ball\\b/i.test(b.textContent))",
			note = "there is no button for all of them",
		},
		{ expect = set('[data-fk="decide:by"]', "A Person"), note = "the person who decides" },
		{ expect = set('[data-fk="decide:note"]', "read the module"), note = "and a note" },
		{ shot = "open-light" },
		{ click = "[data-fk='decide:accept']" },
		{
			wait = DETAIL .. ".querySelector('.rules-run-decided') && /A Person/.test(" .. DETAIL .. ".textContent)",
			timeout = 240000,
			note = "accepted: the detail says by whom and where it was written",
		},
		{
			expect = judged(
				"j.verdicts['M-02'] && j.verdicts['M-02'].by === 'A Person'"
					.. " && j.verdicts['M-02'].verdict === 'violation' && Object.keys(j.verdicts).length === 1"
			),
			note = ".rules-verdicts.json holds the one verdict, written by the engine",
		},
		{
			expect = "!document.querySelector('[data-fk=\"decide:accept\"]')",
			note = "a decided proposal offers nothing more",
		},

		-- waive a current answer's rule
		{ click = "[data-fk='runrow:M-01']" },
		{ wait = "document.querySelector('[data-fk=\"decide:waive\"]') !== null", timeout = 30000 },
		{ click = "[data-fk='decide:waive']" },
		{ wait = "document.querySelector('[data-fk=\"waive:reason\"]') !== null", timeout = 5000 },
		{ expect = set('[data-fk="decide:by"]', "A Person") },
		{ expect = set('[data-fk="waive:reason"]', "the module is replaced next month") },
		{ expect = set('[data-fk="waive:expires"]', "2099-12-31") },
		{ shot = "waive-light" },
		{ click = "[data-fk='waive:go']" },
		{
			wait = DETAIL .. ".querySelector('.rules-run-decided') !== null",
			timeout = 240000,
			note = "waived",
		},
		{
			expect = judged(
				"j.waivers['M-01'] && j.waivers['M-01'].expires === '2099-12-31' && !j.waivers['M-01'].expired"
			),
			note = ".rules-waivers.json holds the waiver",
		},

		-- reject the outdated one
		{ click = "[data-fk='runrow:M-03']" },
		{ wait = "document.querySelector('[data-fk=\"decide:reject\"]') !== null", timeout = 30000 },
		{ expect = set('[data-fk="decide:by"]', "A Person") },
		{ click = "[data-fk='decide:reject']" },
		{
			wait = DETAIL .. ".querySelector('.rules-run-decided') !== null",
			timeout = 30000,
			note = "rejected",
		},
		{
			expect = judged(
				"!j.verdicts['M-03'] && !j.waivers['M-03'] && Object.keys(j.verdicts).length === 1"
					.. " && Object.keys(j.waivers).length === 1"
			),
			note = "a rejection writes nothing into either file",
		},
		{
			expect = "['M-01', 'M-02', 'M-03'].every((id) =>"
				.. " document.querySelector('[data-fk=\"runrow:' + id + '\"] .rules-run-mark').textContent.trim() !== '')",
			note = "every row says what became of it",
		},
		{ shot = "decided-light" },

		-- the Rules tab and the sidebar
		{ click = "#rules-run-back" },
		{ wait = "document.querySelector('#rules-run').hidden", timeout = 5000 },
		{
			expect = "document.querySelector('.rules-row[data-rule=\"M-02\"] .rules-agent .rules-judged.verdict')"
				.. ".textContent.startsWith('✕')"
				.. " && document.querySelector('.rules-row[data-rule=\"M-02\"] .rules-mech').textContent === ''"
				.. " && document.querySelector('.rules-row[data-rule=\"M-01\"] .rules-agent .rules-judged.waived') !== null"
				.. " && !document.querySelector('.rules-row[data-rule=\"M-03\"] .rules-judged')",
			note = "the verdict and the waiver are in the Agent column, never in the mechanical one;"
				.. " the rejected rule shows nothing",
		},
		{
			expect = "/\\b2\\b/.test(document.querySelector('#proj-rules').textContent)",
			note = "the sidebar counts the two manual rules without a verdict or a waiver",
		},
		{ shot = "tab-light" },
		{ expect = "(document.documentElement.setAttribute('data-theme', 'dark'), true)" },
		{ shot = "tab-dark" },
		{ click = "[data-fk='run:r-sheet-decide']" },
		{ wait = "!document.querySelector('#rules-run').hidden", timeout = 30000 },
		{ click = "[data-fk='runrow:M-02']" },
		{ shot = "decided-dark" },
	},
	judge = {
		"Accept, Waive… and Reject are in the detail of one proposal, and nothing offers to decide several at once",
		"An outdated proposal says so in words, and its Accept is off with the reason next to it",
		"A decided proposal says who decided, when, and what was written where",
		"The verdict and the waiver show in the Agent column of the Rules tab, never in the mechanical one",
		"Nothing is told apart by colour alone; light and dark both read well",
		"At 900x600 the buttons and the fields of a waiver are reachable",
	},
}
