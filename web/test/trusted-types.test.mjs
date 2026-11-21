// The one Trusted Types policy: what it lets through, and that it lets nothing else.

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { POLICY_NAME, createTrust } from '../src/trusted-types.js';

const MARKUP = '<div id="a"></div>';

/** A `trustedTypes` that wraps what a policy returns, the way the browser's does. */
function fakeWindow() {
  const policies = [];
  return {
    policies,
    trustedTypes: {
      createPolicy(name, rules) {
        const policy = {
          name,
          createHTML: (s) => ({ kind: 'html', value: rules.createHTML(s) }),
          createScriptURL: (s) => ({ kind: 'url', value: rules.createScriptURL(s) }),
        };
        policies.push(policy);
        return policy;
      },
    },
  };
}
const fakeURL = () => {
  let n = 0;
  return { createObjectURL: () => `blob:null/${++n}` };
};

test('there is one policy, with the name the CSP allows', () => {
  const win = fakeWindow();
  createTrust(win, { URL: fakeURL(), markup: MARKUP });
  assert.deepEqual(win.policies.map((p) => p.name), [POLICY_NAME]);
  assert.equal(POLICY_NAME, 'docmap');
});

test('the markup of the build passes, any other string is refused', () => {
  const trust = createTrust(fakeWindow(), { URL: fakeURL(), markup: MARKUP });
  assert.deepEqual(trust.markup(MARKUP), { kind: 'html', value: MARKUP });
  for (const other of ['<img src=x onerror=alert(1)>', MARKUP + ' ', '', '<script>1</script>']) {
    assert.throws(() => trust.markup(other), TypeError, other);
  }
});

test('a worker address passes only as a blob the policy made itself', () => {
  const win = fakeWindow();
  const trust = createTrust(win, { URL: fakeURL(), markup: MARKUP });
  assert.deepEqual(trust.workerUrl(new Blob(['1'])), { kind: 'url', value: 'blob:null/1' });
  assert.deepEqual(trust.workerUrl(new Blob(['2'])), { kind: 'url', value: 'blob:null/2' });
  // The policy object itself is not reachable from the page, but its rule still holds for any string handed to it.
  const [policy] = win.policies;
  for (const url of ['https://evil.example/w.js', 'blob:null/99', 'data:text/javascript,1', 'javascript:1']) {
    assert.throws(() => policy.createScriptURL(url), TypeError, url);
  }
});

test('a browser without Trusted Types gets the plain strings', () => {
  const trust = createTrust({}, { URL: fakeURL(), markup: MARKUP });
  assert.equal(trust.markup(MARKUP), MARKUP);
  assert.equal(trust.workerUrl(new Blob(['1'])), 'blob:null/1');
});
