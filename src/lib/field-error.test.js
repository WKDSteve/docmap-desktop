import { test } from "node:test";
import assert from "node:assert/strict";

import { showFieldError, clearFieldError } from "./field-error.js";

function el(id = "") {
  const attrs = new Map();
  return {
    id,
    textContent: "",
    hidden: true,
    focused: false,
    attrs,
    setAttribute: (k, v) => attrs.set(k, v),
    removeAttribute: (k) => attrs.delete(k),
    focus() {
      this.focused = true;
    },
  };
}

test("an error marks the field, links the message and focuses the field", () => {
  const field = el();
  const problem = el("ws-problem");
  showFieldError(field, problem, "A name, please.");
  assert.equal(field.attrs.get("aria-invalid"), "true");
  assert.equal(field.attrs.get("aria-describedby"), "ws-problem");
  assert.equal(problem.textContent, "A name, please.");
  assert.equal(problem.hidden, false);
  assert.equal(field.focused, true);
});

test("clearing removes the mark and hides the message", () => {
  const field = el();
  const problem = el("p");
  showFieldError(field, problem, "x");
  clearFieldError(field, problem);
  assert.equal(field.attrs.has("aria-invalid"), false);
  assert.equal(field.attrs.has("aria-describedby"), false);
  assert.equal(problem.hidden, true);
  clearFieldError(field, null); // no message element: still fine
});
