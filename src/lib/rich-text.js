// The few catalog sentences that carry markup (<strong>, <code>, <em>, a line
// break, the grey detail line of a placeholder), turned into DOM nodes without
// `innerHTML` (Trusted Types would refuse the assignment).
//
// Only the tags below exist. Anything else that looks like a tag (the catalog
// has `gate:<name>` and `snapshot <name>` as plain text) stays text, and a
// character reference is decoded to the character it names, so a value
// escaped with `escapeHtml` and put through `fill` shows exactly as typed.

const TOKEN = /<(?:(\/)?(strong|code|em)|(br)|(span class="detail")|(\/span))>/g;
const ENTITIES = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'", "&nbsp;": " " };

const decode = (s) => s.replace(/&(?:amp|lt|gt|quot|#39|nbsp);/g, (m) => ENTITIES[m]);

/**
 * Markup to a tree: `{ text }`, `{ br: true }` or `{ tag, cls?, kids }`.
 * Pure, so it is tested without a DOM.
 */
export function parseRich(markup) {
  const root = { kids: [] };
  const stack = [root];
  const top = () => stack[stack.length - 1];
  const text = (s) => {
    if (s) top().kids.push({ text: decode(s) });
  };
  let at = 0;
  for (const m of String(markup).matchAll(TOKEN)) {
    text(markup.slice(at, m.index));
    at = m.index + m[0].length;
    const [, slash, simple, br, span, closeSpan] = m;
    if (br) top().kids.push({ br: true });
    else if (simple && !slash) {
      const node = { tag: simple, kids: [] };
      top().kids.push(node);
      stack.push(node);
    } else if (span) {
      const node = { tag: "span", cls: "detail", kids: [] };
      top().kids.push(node);
      stack.push(node);
    } else {
      const want = closeSpan ? "span" : simple;
      // A closing tag that closes nothing open is text, not an error.
      if (top().tag === want) stack.pop();
      else text(m[0]);
    }
  }
  text(markup.slice(at));
  return root.kids;
}

function build(doc, nodes) {
  return nodes.map((n) => {
    if (n.text !== undefined) return doc.createTextNode(n.text);
    if (n.br) return doc.createElement("br");
    const el = doc.createElement(n.tag);
    if (n.cls) el.className = n.cls;
    // A command, a path or a file name is not prose: a page translator
    // would turn `docs/map` or `nvim` into words of the other language.
    if (n.tag === "code") el.setAttribute("translate", "no");
    el.append(...build(doc, n.kids));
    return el;
  });
}

/** Replace the content of `el` with the markup, built from DOM nodes. */
export function renderRich(el, markup, doc = el.ownerDocument) {
  el.replaceChildren(...build(doc, parseRich(markup ?? "")));
}

/** Text as markup text: what `renderRich` shows as exactly `text`. */
export function escapeHtml(text) {
  return String(text).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}
