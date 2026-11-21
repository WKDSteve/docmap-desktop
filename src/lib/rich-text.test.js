import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { parseRich, renderRich, escapeHtml } from "./rich-text.js";
import { t, setLocale, keys, LOCALES } from "./i18n.js";

/** The text a tree shows, tags dropped. */
const flat = (nodes) => nodes.map((n) => n.text ?? (n.br ? "\n" : flat(n.kids))).join("");

/** Just enough of a document to build into, and to read back as markup. */
function fakeDoc() {
  const mk = (tag) => ({
    tag,
    className: "",
    children: [],
    append(...c) {
      this.children.push(...c);
    },
    replaceChildren(...c) {
      this.children = c;
    },
  });
  return { createElement: mk, createTextNode: (text) => ({ text }) };
}
const dump = (n) => (n.text !== undefined ? JSON.stringify(n.text) : `${n.tag}${n.className ? "." + n.className : ""}[${n.children.map(dump).join(",")}]`);

test("the tags of the catalog become elements", () => {
  assert.deepEqual(parseRich("a <strong>b</strong> <code>c</code><br>d"), [
    { text: "a " },
    { tag: "strong", kids: [{ text: "b" }] },
    { text: " " },
    { tag: "code", kids: [{ text: "c" }] },
    { br: true },
    { text: "d" },
  ]);
  assert.deepEqual(parseRich('<span class="detail">x</span>'), [{ tag: "span", cls: "detail", kids: [{ text: "x" }] }]);
});

test("nothing but those tags becomes an element: it stays text", () => {
  for (const evil of [
    "<img src=x onerror=alert(1)>",
    "<script>alert(1)</script>",
    '<a href="javascript:0">x</a>',
    '<span class="detail" onclick="x()">x</span>',
    "<strong onclick=x>y</strong>",
    "<STRONG>y</STRONG>",
  ]) {
    const tree = parseRich(evil);
    assert.equal(flat(tree), evil, evil);
    const doc = fakeDoc();
    const el = doc.createElement("p");
    renderRich(el, evil, doc);
    assert.ok(el.children.every((c) => c.text !== undefined), `an element came out of ${evil}`);
  }
});

test("a placeholder in angle brackets is text, a stray closing tag too", () => {
  assert.equal(flat(parseRich("gate:<name>")), "gate:<name>");
  assert.equal(flat(parseRich("a</code>b")), "a</code>b");
});

test("character references are decoded once, and escapeHtml round-trips", () => {
  assert.equal(flat(parseRich("&lt;name&gt; &amp; &quot;x&quot;")), '<name> & "x"');
  for (const v of ["<b>x</b>", "a & b", "&lt;", "C:\\a<b>\\c", "<code>"]) {
    assert.equal(flat(parseRich(escapeHtml(v))), v);
    assert.equal(parseRich(escapeHtml(v)).every((n) => n.text !== undefined), true, v);
  }
});

test("renderRich builds nodes with the document it is given", () => {
  const doc = fakeDoc();
  const el = doc.createElement("p");
  renderRich(el, 'x<br><span class="detail">y</span> <strong>z</strong>', doc);
  assert.equal(el.children.map(dump).join(" "), '"x" br[] span.detail["y"] " " strong["z"]');
  renderRich(el, "", doc);
  assert.deepEqual(el.children, []);
  renderRich(el, null, doc);
  assert.deepEqual(el.children, []);
});

test("every markup tag the catalog uses, in every language, is one renderRich knows", () => {
  const seen = new Set();
  for (const loc of LOCALES) {
    setLocale(loc.code ?? loc);
    for (const k of keys()) {
      for (const m of t(k).matchAll(/<\/?([a-zA-Z][\w:-]*)[^>]*>/g)) seen.add(m[0]);
    }
  }
  const known = new Set(["<strong>", "</strong>", "<code>", "</code>", "<em>", "</em>", "<br>"]);
  // `<name>` is the one placeholder in angle brackets (a plain-text example).
  const unknown = [...seen].filter((tag) => !known.has(tag) && tag !== "<name>");
  assert.deepEqual(unknown, [], "a catalog tag renderRich would show as text");
});

// main.js is not unit-tested: the sinks are held out by reading its source.
test("main.js assigns no HTML: no innerHTML, outerHTML, insertAdjacentHTML, document.write", () => {
  const src = readFileSync(new URL("../main.js", import.meta.url), "utf8")
    .replace(/\r\n/g, "\n")
    .split("\n")
    .filter((l) => !/^\s*(\/\/|\*|\/\*)/.test(l));
  const hits = src.filter((l) => /\.(innerHTML|outerHTML)\s*[+\-]?=(?!=)|insertAdjacentHTML\(|document\.write\(|\.srcdoc\s*=/.test(l));
  assert.deepEqual(hits, [], "a Trusted Types sink in main.js (rule WEB-01)");
});
