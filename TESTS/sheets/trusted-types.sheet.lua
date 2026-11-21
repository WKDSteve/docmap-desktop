-- Trusted Types in the real window (WebView2): the CSP of tauri.conf.json requires them, and the window must draw
-- everything it draws (the empty screen, the catalog sentences with <strong>/<code> after a language change) without
-- one violation. The same check runs in Chromium in CI-less form: node scripts/e2e/trusted-types.mjs. This sheet is
-- the one that says it of WebView2, where the policy of the final app is enforced by the real engine of the app.
--   testing sheets step trusted-types --headed
-- Needs node, tauri-driver and msedgedriver (testing.nvim, engine "webview"); without them it is a skip. Not part of
-- the CI: the window needs a desktop.
return {
	id = "docmap-desktop.trusted-types",
	title = "docmap-desktop: the window runs under Trusted Types without a violation",
	task = "docmap-desktop/innerhtml-durch-dom-apis-und-trusted-types",
	engine = "webview",
	app = "window",
	sizes = { { cols = 1400, rows = 900 } },
	steps = {
		{
			wait = "document.querySelector('#engine-summary').textContent.trim() !== ''",
			timeout = 30000,
			note = "the window is up and the engine was looked for",
		},
		{
			expect = "(() => { window.__violations = [];"
				.. " document.addEventListener('securitypolicyviolation', (e) =>"
				.. " window.__violations.push(e.violatedDirective + ': ' + (e.sample || e.blockedURI))); return true; })()",
			note = "violations are collected from here on",
		},
		{
			expect = "(() => { const s = document.getElementById('lang');"
				.. " s.value = [...s.options].map((o) => o.value).find((v) => v !== s.value);"
				.. " s.dispatchEvent(new Event('change')); return true; })()",
			note = "another language: every catalog sentence is drawn again",
		},
		{
			expect = "document.querySelectorAll('[data-i18n] strong, [data-i18n] code').length > 0",
			note = "the sentences with <strong> and <code> are built as nodes",
		},
		{
			expect = "!document.getElementById('placeholder').hidden"
				.. " && document.querySelector('#placeholder h2').textContent.trim() !== ''",
			note = "the empty screen's placeholder is drawn (no project in the run's workspace)",
		},
		{ expect = "window.__violations.length === 0", note = "the page itself caused no violation" },
		{
			expect = "(() => { try { document.createElement('div').innerHTML = 'x'; return false; }"
				.. " catch (e) { return e instanceof TypeError; } })()",
			note = "a string assigned to innerHTML is refused: the policy is enforced here",
		},
		{
			expect = "(() => { try { window.trustedTypes.createPolicy('docmap', { createHTML: (s) => s }); return false; }"
				.. " catch (e) { return true; } })()",
			note = "no policy can be created: the window needs none",
		},
	},
	judge = {
		"The window looks as it did before: the sentences with bold text and code in Settings read the same in both languages",
	},
}
