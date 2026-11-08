import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { closeOnBackdrop, isOutside } from "./backdrop.js";

// A stand-in for a <dialog> at 100..300 x 100..200: the events and the box
// are all the behaviour looks at.
function fakeDialog() {
  const node = new EventTarget();
  node.open = true;
  node.closed = 0;
  node.close = () => {
    node.open = false;
    node.closed += 1;
  };
  node.getBoundingClientRect = () => ({ left: 100, right: 300, top: 100, bottom: 200 });
  return node;
}

function mouse(node, type, x, y, target = node) {
  const e = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(e, {
    clientX: { value: x },
    clientY: { value: y },
    target: { value: target },
  });
  node.dispatchEvent(e);
}

const click = (node, down, up, target) => {
  mouse(node, "mousedown", ...down, target);
  mouse(node, "click", ...up, target);
};

test("a point is outside only beyond the edges of the box", () => {
  const box = { left: 10, right: 20, top: 10, bottom: 20 };
  assert.equal(isOutside(box, 15, 15), false);
  assert.equal(isOutside(box, 10, 20), false);
  assert.equal(isOutside(box, 9, 15), true);
  assert.equal(isOutside(box, 15, 21), true);
});

test("a press and a release beside the dialog close it, after a cancel event", () => {
  const node = fakeDialog();
  let cancels = 0;
  node.addEventListener("cancel", () => (cancels += 1));
  closeOnBackdrop(node);
  click(node, [50, 50], [50, 50]);
  assert.equal(cancels, 1);
  assert.equal(node.closed, 1);
});

test("a click inside the box, or one that starts inside, keeps the dialog open", () => {
  const node = fakeDialog();
  closeOnBackdrop(node);
  click(node, [150, 150], [150, 150]);
  // A text selection that starts in the dialog and ends beside it.
  click(node, [150, 150], [50, 50]);
  // A press beside it that is released inside.
  click(node, [50, 50], [150, 150]);
  assert.equal(node.closed, 0);
});

test("a click on a control of the dialog is not a click on the backdrop", () => {
  const node = fakeDialog();
  closeOnBackdrop(node);
  // A child that sticks out of the box still has itself as the target.
  click(node, [50, 50], [50, 50], { tagName: "BUTTON" });
  assert.equal(node.closed, 0);
});

test("a cancel handler that says no keeps the dialog open", () => {
  const node = fakeDialog();
  node.addEventListener("cancel", (e) => e.preventDefault());
  closeOnBackdrop(node);
  click(node, [50, 50], [50, 50]);
  assert.equal(node.closed, 0);
});

test("the returned function takes the behaviour away", () => {
  const node = fakeDialog();
  const stop = closeOnBackdrop(node);
  stop();
  click(node, [50, 50], [50, 50]);
  assert.equal(node.closed, 0);
});

test("every dialog of the window gets it", () => {
  const main = readFileSync(new URL("../main.js", import.meta.url), "utf8");
  assert.match(main, /document\.querySelectorAll\("dialog"\)\.forEach\(\(?\w+\)? => closeOnBackdrop\(/);
});
