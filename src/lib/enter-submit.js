// Enter that submits a text field, without sending text the user is still
// composing. With an IME (Japanese, Chinese, Korean) Enter confirms the
// current conversion candidate; a handler that submits on every Enter sends
// half-converted text. Guide: modern-web-guidance `ime-safe-enter-submit`.
//
// None of these fields sits in a <form> (their buttons are `type="button"`),
// so there is no `requestSubmit()` to call: the handler runs the action the
// button runs.

/**
 * Is this keydown the Enter that confirms an IME conversion?
 *
 * `isComposing` is the standard signal; Safari delivers `compositionend`
 * before the confirming keydown, so it is already false there. Such a keydown
 * still reports `keyCode` 229 (VK_PROCESSKEY), which is checked as well. Only
 * meaningful under an `Enter` gate: virtual keyboards use 229 for plain keys.
 */
export function imeComposing(ev) {
  return ev.isComposing === true || ev.keyCode === 229;
}

/**
 * Run `submit` when Enter is pressed in `el`, unless the key belongs to an IME.
 * Enter's default action is always cancelled (implicit submission, newline).
 */
export function onEnterSubmit(el, submit) {
  el.addEventListener("keydown", (ev) => {
    if (ev.key !== "Enter") return;
    ev.preventDefault();
    if (imeComposing(ev)) return;
    submit();
  });
}
