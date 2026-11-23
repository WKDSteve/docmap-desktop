// The pure half of the statistics view: from the backend's counts to the
// handful of numbers a reader wants, and the proportions behind the bars.

/**
 * @typedef {{total: number, code: number, comment: number, blank: number}} Lines
 * @typedef {{files: number, lines: Lines}} Totals
 * @typedef {{name: string, kind: string, files: number, lines: Lines, bytes: number}} Lang
 * @typedef {{languages: Lang[], code: Totals, docs: Totals, data: Totals, other: Totals,
 *            files: number, bytes: number, largest: {path: string, lines: number}[],
 *            truncated: boolean}} Stats
 */

/** A percentage with one decimal, or `0` when there is nothing to divide. */
export function percent(part, whole) {
  if (!whole) return 0;
  return Math.round((part / whole) * 1000) / 10;
}

/**
 * The headline numbers.
 *
 * `lines` is everything that was measured; `blank` is summed across the three
 * kinds because "how much of this is empty space" is one question. The
 * comment ratio is over *source* lines only — a comment is a property of code,
 * and a Markdown file has none to count.
 *
 * @param {Stats} s
 */
export function headline(s) {
  const lines = s.code.lines.total + s.docs.lines.total + s.data.lines.total;
  const blank = s.code.lines.blank + s.docs.lines.blank + s.data.lines.blank;
  const source = s.code.lines.code + s.code.lines.comment;
  return {
    files: s.files,
    lines,
    code: s.code.lines.code,
    codeFiles: s.code.files,
    comment: s.code.lines.comment,
    commentRatio: percent(s.code.lines.comment, source),
    docs: s.docs.lines.code,
    docsFiles: s.docs.files,
    data: s.data.lines.code,
    dataFiles: s.data.files,
    blank,
    other: s.other.files,
  };
}

/**
 * The composition bar: what share of all measured lines each kind of line is.
 * Segments sum to 100 (within rounding) and empty ones are left out, so a
 * project with no Markdown has no empty sliver in its bar.
 *
 * @param {Stats} s
 * @returns {{key: "code"|"comment"|"docs"|"data"|"blank", lines: number, percent: number}[]}
 */
export function composition(s) {
  const h = headline(s);
  const parts = [
    { key: "code", lines: h.code },
    { key: "comment", lines: h.comment },
    { key: "docs", lines: h.docs },
    { key: "data", lines: h.data },
    { key: "blank", lines: h.blank },
  ];
  return parts
    .filter((p) => p.lines > 0)
    .map((p) => ({ ...p, percent: percent(p.lines, h.lines) }));
}

/**
 * Languages for the table, each with its share of the measured lines — the
 * widest first, which the backend already sorts, so this only adds the share
 * and a width relative to the largest (the longest bar fills its cell).
 *
 * @param {Stats} s
 */
export function languageRows(s) {
  const total = headline(s).lines;
  const top = s.languages.length ? s.languages[0].lines.total : 0;
  return s.languages.map((l) => ({
    ...l,
    share: percent(l.lines.total, total),
    width: percent(l.lines.total, top),
  }));
}

/**
 * Bytes as a short human size, the number in the reader's locale (a German
 * reader gets `3,0 MB`, not `3.0 MB`).
 *
 * @param {number} n
 * @param {string} [locale]  BCP 47 tag; the UI language
 */
export function formatBytes(n, locale = "en") {
  if (!Number.isFinite(n) || n < 0) return "";
  const whole = new Intl.NumberFormat(locale, { maximumFractionDigits: 0 });
  const one = new Intl.NumberFormat(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  if (n < 1024) return `${whole.format(n)} B`;
  if (n < 1024 * 1024) return `${whole.format(Math.round(n / 1024))} KB`;
  if (n < 1024 * 1024 * 1024) return `${one.format(n / (1024 * 1024))} MB`;
  return `${one.format(n / (1024 * 1024 * 1024))} GB`;
}
