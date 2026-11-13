// The words of the web variant's own parts (the strip under the menu bar), in the languages of the window. What a
// command fails with is said by the page (src/lib/errors.js), so those sentences are in its catalog (src/lib/i18n.js,
// the keys `web.*`); what only the shell of the web variant shows is here.

const TEXTS = {
  en: {
    'strip.label': 'Web variant',
    'strip.summary': 'read only, maps are kept in memory',
    'strip.more': 'What is different here',
    'strip.details':
      'This page runs in the browser, without a program behind it. It reads the folders you pick and writes nothing into them: ' +
      'a map you generate lives in this tab until you close it; a map that already lies in the folder is shown as it is. ' +
      'Not part of it: the git history (History, checklist status), telemetry and loaded modules of a Neovim session, ' +
      'rule runs with an agent, statistics, opening a file in an editor, clone from a URL, GitHub lists and traffic, ' +
      'the documentation links, and exporting a diagram. A map generated here has no facts per function (functions, ' +
      'calls, complexity): they need a parser the page does not carry. A map that lies in the folder has them.',
    'access.one': '1 folder waits for your permission: the browser asks again after it was restarted.',
    'access.many': '{n} folders wait for your permission: the browser asks again after it was restarted.',
    'access.allow': 'Allow access',
    'access.gone': 'not found any more: {names}',
    'skipped.one': '1 file of {name} was left out (too large, unreadable or an unusable name).',
    'skipped.many': '{n} files of {name} were left out (too large, unreadable or an unusable name).',
    'picker.missing':
      'This browser cannot open a folder: the page needs the File System Access API (Chrome or Edge).',
    'menu.unavailable': 'Not part of the web variant',
    'start.failed': 'The engine did not start',
  },
  de: {
    'strip.label': 'Web-Variante',
    'strip.summary': 'nur lesend, Karten bleiben im Speicher',
    'strip.more': 'Was hier anders ist',
    'strip.details':
      'Diese Seite läuft im Browser, ohne ein Programm dahinter. Sie liest die Ordner, die du wählst, und schreibt nichts hinein: ' +
      'Eine erzeugte Karte lebt in diesem Tab, bis du ihn schließt; eine Karte, die schon im Ordner liegt, wird gezeigt, wie sie ist. ' +
      'Nicht dabei: der Git-Verlauf (History, Status der Checkliste), Telemetrie und geladene Module einer Neovim-Sitzung, ' +
      'Regel-Läufe mit einem Agenten, die Statistik, das Öffnen einer Datei im Editor, Klonen von einer URL, GitHub-Listen und Traffic, ' +
      'die Links zur Dokumentation und der Export eines Diagramms. Eine hier erzeugte Karte hat keine Angaben je Funktion ' +
      '(Funktionen, Aufrufe, Komplexität): Die brauchen einen Parser, den die Seite nicht mitbringt. Eine Karte, die im ' +
      'Ordner liegt, hat sie.',
    'access.one': '1 Ordner wartet auf deine Erlaubnis: Der Browser fragt nach seinem Neustart wieder.',
    'access.many': '{n} Ordner warten auf deine Erlaubnis: Der Browser fragt nach seinem Neustart wieder.',
    'access.allow': 'Zugriff erlauben',
    'access.gone': 'nicht mehr gefunden: {names}',
    'skipped.one': '1 Datei von {name} wurde ausgelassen (zu groß, nicht lesbar oder ein unbrauchbarer Name).',
    'skipped.many': '{n} Dateien von {name} wurden ausgelassen (zu groß, nicht lesbar oder ein unbrauchbarer Name).',
    'picker.missing':
      'Dieser Browser kann keinen Ordner öffnen: Die Seite braucht die File System Access API (Chrome oder Edge).',
    'menu.unavailable': 'Nicht Teil der Web-Variante',
    'start.failed': 'Die Engine ist nicht gestartet',
  },
};

/** The languages this file has. */
export const LOCALES = Object.keys(TEXTS);

/**
 * @param {string} locale
 * @param {keyof typeof TEXTS['en']} key
 * @param {Record<string, string | number>} [vars]
 */
export function text(locale, key, vars = {}) {
  const table = /** @type {Record<string, Record<string, string>>} */ (TEXTS)[locale] ?? TEXTS.en;
  const template = table[key] ?? TEXTS.en[key] ?? key;
  return template.replace(/\{([a-z]+)\}/g, (whole, name) => (name in vars ? String(vars[name]) : whole));
}

/** Every key, for the test that holds the two languages to the same set. */
export const keysOf = (/** @type {string} */ locale) => Object.keys(/** @type {any} */ (TEXTS)[locale] ?? {});
