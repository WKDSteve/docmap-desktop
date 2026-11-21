// The one Trusted Types policy of the page (CSP: `require-trusted-types-for 'script'; trusted-types docmap`).
//
// Two sinks of the shell take more than a string under that rule: the markup it writes at start, and the address of
// the engine's worker. The policy does not turn "a string" into one of them. Each of its two functions accepts one
// thing only, and the page has no way to hand it anything else:
//   - the markup: the constant the build baked in, compared with `===`;
//   - the worker address: a blob URL that `workerUrl` itself made from a Blob a moment earlier.
// Where the browser has no Trusted Types the strings pass as they are, and nothing is enforced either.

export const POLICY_NAME = 'docmap';

/**
 * @param {{ trustedTypes?: { createPolicy(name: string, rules: object): any } }} win
 * @param {{ URL: Pick<typeof URL, 'createObjectURL'>, markup: string }} given `markup`: the one markup the page writes
 */
export function createTrust(win, { URL, markup }) {
  /** Blob URLs made by `workerUrl`, so the policy can tell them from every other string. @type {Set<string>} */
  const made = new Set();
  const policy = win.trustedTypes?.createPolicy(POLICY_NAME, {
    /** @param {string} html */
    createHTML(html) {
      if (html !== markup) throw new TypeError('trusted types: this markup is not the one the build made');
      return html;
    },
    /** @param {string} url */
    createScriptURL(url) {
      if (!made.has(url)) throw new TypeError('trusted types: this address is not a blob the page just made');
      return url;
    },
  });
  return {
    /** The page's own markup, as the sink wants it. @param {string} html */
    markup: (html) => (policy ? policy.createHTML(html) : html),
    /** A Blob as the address a worker may start from. @param {Blob} blob */
    workerUrl(blob) {
      const url = URL.createObjectURL(blob);
      made.add(url);
      return policy ? policy.createScriptURL(url) : url;
    },
  };
}
