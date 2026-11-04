// What the Neovim-config import says when it is done.
//
// The tab used to report "X of Y project(s) were added." and nothing else, in
// the status line, after closing the dialog. When X was 0 (everything already
// present, or nothing found) that read as "nothing happened". Every outcome
// now has its own sentence, and failures name the projects they hit.

import { t } from "./i18n.js";
import { fillParams, errorText } from "./errors.js";

/**
 * The sentence for one `import_from_nvim_config` result
 * (`{ found, added: Project[], already_present, errors: {name, error}[] }`).
 * `changed` says whether the project list grew, which is what decides
 * whether the dialog may close.
 */
export function nvimImportSummary(res) {
  const found = Number(res?.found) || 0;
  const added = Array.isArray(res?.added) ? res.added.length : 0;
  const present = Number(res?.already_present) || 0;
  const errors = Array.isArray(res?.errors) ? res.errors : [];

  let text;
  if (found === 0) {
    text = t("add.nvim.none");
  } else if (added === 0 && errors.length === 0) {
    text = fillParams(t("add.nvim.allPresent"), { found });
  } else {
    text = fillParams(t("add.nvim.done"), { added, found, present });
  }
  if (errors.length > 0) {
    const names = errors
      .map((f) => (f.error ? `${f.name} (${errorText(f.error)})` : f.name))
      .join("; ");
    text += " " + fillParams(t("add.nvim.failed"), { n: errors.length, names });
  }
  return { text, changed: added > 0 };
}
