-- The run dialog in the real window (WebView2) with the bundled engine, at the window's minimum size (900x600):
--   testing sheets step rules-run-dialog --headed
-- What it shows is what the engine plans for the fixture project (TESTS/fixtures/rules: three manual rules with a
-- scope go into one request, a fourth has none and is not sent). The address of ai-desktop is pointed at a port
-- nobody listens on, so the dialog is seen in the state "ai-desktop does not answer": Start stays off, and **this
-- sheet never starts a run and never reaches a model**. A run itself is tested against a stand-in in Rust
-- (agent/runner.rs) and looked at in the preview harness; in the real window with a real model it is a hand check.
--
-- Needs a real engine in src-tauri/binaries, node, tauri-driver and msedgedriver; without them it is a skip. Not
-- part of the CI. The waits are long because an engine call takes seconds on Windows and minutes on a busy machine.
-- (A sheet is loaded without the `table` library, so the lines are joined by hand.)
local ADD = "(window.__rulesSeed ||= (async () => {"
	.. "  const invoke = window.__TAURI__.core.invoke;"
	.. "  const info = await invoke('engine_info');"
	.. "  const at = String(info.path || '').replace(/\\\\/g, '/');"
	.. "  const cut = at.indexOf('/src-tauri/');"
	.. "  if (cut < 0) throw new Error('the engine is not the one of this checkout: ' + at);"
	.. "  await invoke('agent_url', { set: 'http://127.0.0.1:9' });"
	.. "  await invoke('add_project', { root: at.slice(0, cut) + '/TESTS/fixtures/rules/project' });"
	.. "  location.reload();"
	.. "  return true;"
	.. "})())"

return {
	id = "docmap-desktop.rules-run-dialog",
	title = "docmap-desktop: the run dialog says what would be sent and where, and does not start by itself",
	task = "docmap-desktop/rules-run-dialog-and-run-window",
	engine = "webview",
	app = "window",
	sizes = { { cols = 900, rows = 600 } },
	steps = {
		{
			wait = "document.querySelector('#engine-summary').textContent.trim() !== ''",
			timeout = 30000,
			note = "the window is up and the engine was looked for",
		},
		{ wait = ADD, timeout = 30000, note = "ai-desktop is pointed at a dead port, the fixture project is added" },
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
			expect = "document.querySelector('#rules-agent').disabled && document.querySelector('#rules-runs').hidden",
			note = "nothing selected: no agent run to offer, and the project has no runs",
		},
		{ click = ".rules-chip-filter.manual" },
		{ click = ".rules-group-head input[type=checkbox]", note = "the four manual rules of the file" },
		{
			expect = "!document.querySelector('#rules-agent').disabled"
				.. " && / 4 /.test(document.querySelector('#rules-agent').textContent)",
			note = "the button says how many manual rules a run would be about",
		},
		{ shot = "bar-light" },

		{ click = "#rules-agent" },
		{ wait = "document.querySelector('#runbox').open", timeout = 5000 },
		{
			expect = "document.activeElement.id === 'run-cancel' && document.querySelector('#run-go').disabled",
			note = "the focus is on Do not start, Start is off",
		},
		{
			expect = "/M-01, M-02, M-03 \\+1 \\(\\d{4}-\\d\\d-\\d\\d\\)/.test(document.querySelector('#run-title').value)",
			note = "the title defaults to the selection and the day",
		},
		{
			wait = "document.querySelector('#run-preview-state').textContent === ''"
				.. " && document.querySelectorAll('#run-batch-rows tr').length === 1",
			timeout = 240000,
			note = "the engine planned one request (nothing was sent for that)",
		},
		{
			expect = "document.querySelector('#run-skipped').textContent.includes('N-01')"
				.. " && document.querySelector('#run-skipped').textContent.includes('no scope')",
			note = "the rule that is not sent is listed with its reason",
		},
		{
			expect = "document.querySelector('#run-go').disabled && !document.querySelector('#run-refused').hidden",
			note = "no model named: Start is off and the dialog says why",
		},
		{
			expect = "(() => { const m = document.querySelector('#run-model'); m.value = 'local-example';"
				.. " m.dispatchEvent(new Event('change')); return true; })()",
			note = "name the project's local model",
		},
		{
			wait = "document.querySelector('#run-egress').textContent.includes('local-example')",
			timeout = 240000,
			note = "the egress line names the model",
		},
		{
			expect = "!document.querySelector('#run-egress').classList.contains('leaves')",
			note = "a local model: the line says the files stay on this machine",
		},
		{
			wait = "/127\\.0\\.0\\.1:9/.test(document.querySelector('#run-service').textContent)",
			timeout = 60000,
			note = "ai-desktop did not answer, and the dialog names the address it tried",
		},
		{
			expect = "document.querySelector('#run-go').disabled && !document.querySelector('#run-refused').hidden",
			note = "with ai-desktop unreachable Start stays off",
		},
		{ shot = "dialog-light" },
		{
			expect = "(() => { const m = document.querySelector('#run-model'); m.value = 'claude-example';"
				.. " m.dispatchEvent(new Event('change')); return true; })()",
			note = "a cloud model the project does not allow",
		},
		{
			wait = "document.querySelector('#run-preview-state').textContent === ''"
				.. " && /ollama/.test(document.querySelector('#run-refused').textContent)",
			timeout = 240000,
			note = "refused in the project's own terms: agent.backends allows ollama only",
		},
		{ expect = "document.querySelector('#run-go').disabled" },
		{ expect = "(document.documentElement.setAttribute('data-theme', 'dark'), true)" },
		{ shot = "dialog-refused-dark" },
		{ click = "#run-cancel" },
		{
			expect = "!document.querySelector('#runbox').open && document.querySelector('#rules-runs').hidden",
			note = "closed, and no run exists",
		},
	},
	judge = {
		"The egress line is the line of the dialog that cannot be missed, and it says the files stay on this machine",
		"The two buttons are in view at 900x600 without scrolling to them",
		"The table of requests and the list of what is not sent are readable",
		"A refused model says why in words a person can act on",
		"Light and dark both read well",
	},
}
