// What has nothing behind it in a browser is not offered: the shell marks these controls of src/index.html, and
// web.css hides what carries the mark. The Neovim panel; the two ways to add a project that need a program (the plugin
// list of a Neovim config, a URL to clone); the statistics pane; the links that open GitHub; and in the settings the
// choice of an engine and of grammars, telemetry, traffic, the editor and Neovim.
//
// No DOM here, so a test in Node can hold the list to the ids that exist in src/index.html.

export const HIDDEN = {
  /** Selectors of single controls. */
  elements: [
    '#nvim',
    '#pick-engine',
    '#pick-grammars',
    '[data-pane="pane-nvim"]',
    '[data-pane="pane-url"]',
    '[data-pane="stats"]',
    '#proj-traffic-detail',
    '#about-repo',
    '#about-license',
  ],
  /** A section of the settings, named by a control inside it. */
  sectionsOf: ['#tel-state', '#traffic-state', '#editor-cmd', '#prefs-nvim-state'],
};
