// A dialog that holds what somebody typed does not close on a slip: Escape
// and a click on the backdrop ask first. A dialog without input closes as
// before.
//
// Two ways to ask, one function (`request`). A browser that knows `closedby`
// gets `closedby="none"` while the dialog is dirty: it then ignores Escape
// by itself and this module answers the key. A browser without it fires the
// `cancel` event for Escape, as `closeOnBackdrop` (backdrop.js) does for the
// click, and the handler here keeps the dialog open until the question is
// answered. Both paths end in the same question.

/** True when the engine has the `closedBy` property on `<dialog>`. */
export function supportsClosedBy(doc = globalThis.document) {
  const ctor = doc?.defaultView?.HTMLDialogElement ?? globalThis.HTMLDialogElement;
  return Boolean(ctor && "closedBy" in ctor.prototype);
}

/**
 * Guard `dialog` against closing with unsaved input.
 *
 * @param {HTMLDialogElement} dialog
 * @param {object} opts
 * @param {() => boolean} opts.ask         the question: true to discard
 * @param {() => boolean} [opts.hasInput]  true while there is something to
 *                                          lose; the default is "anything
 *                                          was typed since the dialog opened"
 * @param {boolean} [opts.closedBy]        feature-detected by default
 * @returns {{ touch(): void, reset(): void, dirty(): boolean, request(): boolean }}
 */
export function guardDirty(dialog, { ask, hasInput = () => true, closedBy = supportsClosedBy() }) {
  let touched = false;
  const dirty = () => touched && hasInput();

  const sync = () => {
    if (closedBy) dialog.closedBy = dirty() ? "none" : "closerequest";
  };
  const touch = () => {
    touched = true;
    sync();
  };
  const reset = () => {
    touched = false;
    sync();
  };
  // Closes, or asks and then closes. Returns whether the dialog closed.
  const request = () => {
    if (dirty() && !ask()) return false;
    dialog.close();
    return true;
  };

  // `isTrusted` is false for a value the program sets and a dispatched
  // event, so only a person's input counts.
  const onInput = (e) => {
    if (e.isTrusted) touch();
  };
  dialog.addEventListener("input", onInput);
  dialog.addEventListener("change", onInput);
  // `close` is fired as a task of its own, after the dialog is closed. When the dialog was opened again and typed
  // into before that task ran (a slow machine), the late event belongs to the earlier opening: it must not wipe what
  // was typed since, or Escape would close the dialog without the question.
  dialog.addEventListener("close", () => {
    if (!dialog.open) reset();
  });
  dialog.addEventListener("cancel", (e) => {
    if (!dirty()) return;
    // Keep it open; `request` closes it when the answer is yes.
    e.preventDefault();
    request();
  });
  dialog.addEventListener("keydown", (e) => {
    // Only reached with closedby="none": otherwise Escape is a `cancel`.
    if (closedBy && e.key === "Escape" && dirty()) {
      e.preventDefault();
      request();
    }
  });
  return { touch, reset, dirty, request };
}
