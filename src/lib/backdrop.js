// A modal <dialog> closes when the pointer is pressed and released on the
// backdrop beside it, as it does for Escape. The same behaviour as
// `appkit-ui/backdrop` (appkit, ui-kit/src/backdrop.ts), written again here
// because this window has no bundler to take the kit's TypeScript.
//
// The backdrop is part of the dialog element for the events: a click on it
// has the dialog itself as its target, so the click counts only when it
// lies outside the box of the dialog (a click on the padding of the box is
// inside). Both the press and the release must be on the backdrop: a text
// selection that starts in the dialog and ends outside it must not close it.

/** True when the point lies outside the box: on the backdrop, not on the dialog. */
export function isOutside(rect, x, y) {
  return x < rect.left || x > rect.right || y < rect.top || y > rect.bottom;
}

/**
 * Make a dialog close on a click beside it. It goes the way Escape goes: a
 * `cancel` event first (a handler that calls `preventDefault()` keeps the
 * dialog open), then `close()`, so the dialog's `close` handlers run as they
 * do for Escape. Returns a function that takes the behaviour away again.
 */
export function closeOnBackdrop(node) {
  let pressedOutside = false;
  const outside = (e) =>
    e.target === node && isOutside(node.getBoundingClientRect(), e.clientX, e.clientY);
  const onDown = (e) => {
    pressedOutside = outside(e);
  };
  const onClick = (e) => {
    const start = pressedOutside;
    pressedOutside = false;
    if (!start || !outside(e) || !node.open) return;
    const cancel = new Event("cancel", { cancelable: true });
    node.dispatchEvent(cancel);
    if (!cancel.defaultPrevented) node.close();
  };
  node.addEventListener("mousedown", onDown);
  node.addEventListener("click", onClick);
  return () => {
    node.removeEventListener("mousedown", onDown);
    node.removeEventListener("click", onClick);
  };
}
