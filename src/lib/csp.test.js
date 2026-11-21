import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// The browser test (scripts/e2e/trusted-types.mjs) proves the policy is
// enforced; this keeps it from being edited out where only `node --test` runs.
const conf = JSON.parse(readFileSync(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
const directives = Object.fromEntries(
  conf.app.security.csp.split(";").map((d) => d.trim().split(/\s+(.*)/s).slice(0, 2))
);

test("the window requires Trusted Types for scripts and creates no policy", () => {
  assert.equal(directives["require-trusted-types-for"], "'script'");
  // Nothing in src/ needs a policy (DOM APIs build every node), so none is allowed.
  assert.equal(directives["trusted-types"], "'none'");
});

test("the window still allows no inline script", () => {
  assert.equal(directives["script-src"], "'self'");
});
