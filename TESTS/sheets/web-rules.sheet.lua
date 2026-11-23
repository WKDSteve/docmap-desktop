-- The web-platform rules in the real window (WebView2): what scripts/e2e/web-rules.mjs checks in Chromium, asked of the
-- engine the app really ships with. Only what needs no keystroke; the dialogs that ask before typed input is thrown
-- away are checked by the Chromium script and by the hand list of the task (a script-set value is not input).
--   testing sheets step web-rules --headed
-- Needs node, tauri-driver and msedgedriver (testing.nvim, engine "webview"). Not part of the CI: the window needs a
-- desktop. NOT YET RUN: written without a desktop at hand; the first run on a machine with one decides whether the
-- expressions hold in WebView2.
return {
	id = "docmap-desktop.web-rules",
	title = "docmap-desktop: the web-platform rules hold in the real window",
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
			expect = "[...document.querySelectorAll('code')].every((c) => c.getAttribute('translate') === 'no')"
				.. " && document.getElementById('brand').translate === false",
			note = "code and the brand name are not translated",
		},
		{
			expect = "[...document.querySelectorAll('input:not([type=checkbox]):not([type=radio]), textarea, select')]"
				.. ".every((e) => parseFloat(getComputedStyle(e).fontSize) >= 16)",
			note = "every field is 16px or more",
		},
		{
			expect = "['url-input', 'ws-new', 'editor-cmd', 'repo-filter']"
				.. ".every((id) => document.getElementById(id).spellcheck === false)",
			note = "no spellcheck on URL, name, command and filter fields",
		},
		{
			expect = "/tabular-nums/.test(getComputedStyle(document.getElementById('rules-chat-egress')).fontVariantNumeric)",
			note = "changing numbers use tabular figures",
		},
		{
			expect = "(() => { document.getElementById('ws-new').closest('dialog').showModal();"
				.. " document.getElementById('ws-create').click();"
				.. " const f = document.getElementById('ws-new');"
				.. " const ok = f.getAttribute('aria-invalid') === 'true' && document.activeElement === f;"
				.. " f.closest('dialog').close(); return ok; })()",
			note = "an empty workspace name marks its field and takes the focus",
		},
	},
	judge = {
		"The window looks as it did before; the fields in Settings and in the feedback dialog read comfortably at their new size",
	},
}
