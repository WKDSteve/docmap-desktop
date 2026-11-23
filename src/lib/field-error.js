// A message about one field: shown in the dialog's problem line, tied to the
// field (aria-invalid, aria-describedby) and the field gets the focus, so a
// keyboard or screen-reader user lands where the fix is.

/**
 * @param {HTMLElement} field    the control that is wrong
 * @param {HTMLElement} problem  the dialog's message element (has an id)
 * @param {string} text
 */
export function showFieldError(field, problem, text) {
  problem.textContent = text;
  problem.hidden = false;
  field.setAttribute("aria-invalid", "true");
  field.setAttribute("aria-describedby", problem.id);
  field.focus();
}

/** Takes the mark off again; the message element is hidden by the caller's own rule. */
export function clearFieldError(field, problem) {
  field.removeAttribute("aria-invalid");
  field.removeAttribute("aria-describedby");
  if (problem) problem.hidden = true;
}
