import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import path from "node:path";

// This repository is public. The vaults its author keeps notes and rules in are
// not, and neither are the ids of the rules written there: a fixture named
// after a real vault, or a comment citing a private rule by its id, tells a
// stranger how the private side is laid out and cannot be resolved by them
// anyway. Fixtures carry neutral names. A name that was committed once stays
// in the history; this keeps the current tree clean.
//
// This file is scanned like every other one: there is no exemption for it, so
// it must not contain what it looks for. The patterns are therefore assembled
// at run time, and the sample strings the tests plant are assembled the same
// way.

const VAULT_NAMES = [
  ["wk", "dbook"],
  ["tasks", "-vault"],
  ["rules", "-vault"],
].map((p) => p.join(""));
// Any rule id of the shape LETTERS-DIGITS, whatever the family: a new family
// in the private rules needs no change here. What is allowed is a short list
// of real identifiers that look the same (encodings, hashes, licences).
const ID = /\b([A-Z]{2,5})-\d{1,3}\b/g;
const ID_ALLOWED = new Set(["UTF", "SHA", "ISO", "GPL", "LGPL", "BSD", "AES", "MPL"]);
const VAULT_RE = new RegExp(VAULT_NAMES.join("|"), "i");

const ROOT = fileURLToPath(new URL("../../", import.meta.url));
// Not scanned: lockfiles (machine-written, no prose). Binary files are
// recognised by content, not by extension.
const SKIP_FILES = new Set(["Cargo.lock", "package-lock.json"]);

/** Every private name or rule id in one line, as the matched text. */
function privateHits(line) {
  const out = [];
  const v = VAULT_RE.exec(line);
  if (v) out.push(v[0]);
  for (const m of line.matchAll(ID)) if (!ID_ALLOWED.has(m[1])) out.push(m[0]);
  return out;
}

/** The tracked files of a work tree: `git ls-files`, so a new top-level folder is covered without touching any list. */
function trackedFiles(root) {
  const raw = execFileSync("git", ["ls-files", "-z"], { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return raw.split("\0").filter(Boolean);
}

/** Findings as `path:line: text` over every tracked text file of `root`. */
function scanTree(root) {
  const hits = [];
  for (const rel of trackedFiles(root)) {
    if (SKIP_FILES.has(path.basename(rel))) continue;
    const full = path.join(root, rel);
    if (!existsSync(full)) continue; // tracked but deleted in the work tree
    const buf = readFileSync(full);
    if (buf.subarray(0, 4096).includes(0)) continue; // binary
    buf
      .toString("utf8")
      .split(/\r?\n/)
      .forEach((l, i) => {
        if (privateHits(l).length) hits.push(`${rel}:${i + 1}: ${l.trim().slice(0, 100)}`);
      });
  }
  return hits;
}

test("no tracked text names a private vault or cites a private rule id", () => {
  const files = trackedFiles(ROOT);
  assert.ok(files.length > 50, `expected the whole tracked tree, found ${files.length} files`);
  assert.deepEqual(scanTree(ROOT), [], "private names or rule ids in a public repository");
});

test("privateHits: every id family is caught, real identifiers are not", () => {
  // Lower case in the source, so this file's own text stays clean.
  for (const id of ["app-01", "prin-09", "lua-12", "nrel-13", "err-3", "perf-100", "sec-7", "xp-02", "go-5"].map((x) => x.toUpperCase())) {
    assert.deepEqual(privateHits(`// per ${id} here`), [id], id);
  }
  for (const ok of ["UTF-8", "UTF-16", "SHA-256", "ISO-8859", "GPL-3", "MPL-2"]) assert.deepEqual(privateHits(`uses ${ok}`), [], ok);
  assert.equal(privateHits("see " + VAULT_NAMES[0].toUpperCase() + "s").length, 1, "vault names match case-insensitively");
  assert.equal(privateHits("see the " + VAULT_NAMES[1]).length, 1);
  assert.deepEqual(privateHits("an ordinary comment, a-1 and X-1"), []);
});

// The sabotage cases of the review: the files the old fixed list never
// scanned. Each is planted in a throwaway git repository, so the real tree is
// never touched, and each must be reported.
test("scanTree reaches files outside the old positive list (build script, platform config, source, new folders)", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "private-names-"));
  try {
    const git = (...a) => execFileSync("git", a, { cwd: dir, stdio: "pipe" });
    git("init", "-q");
    const id = (fam, n) => `${fam}-${n}`.toUpperCase();
    const planted = {
      "src-tauri/build.rs": `// ${VAULT_NAMES[0]} ${id("nrel", 13)}\n`,
      "src-tauri/tauri.windows.conf.json": `{ "note": "# ${VAULT_NAMES[0]}" }\n`,
      "src/main.js": `// per ${id("prin", "09")} and ${id("lua", 12)}\n`,
      ".gitignore": `# ${id("err", "01")}\n`,
      "brand-new-top-level/notes.txt": `${VAULT_NAMES[1]}\n`,
      "clean/readme.md": "nothing private, only UTF-8 and SHA-256\n",
    };
    for (const [rel, text] of Object.entries(planted)) {
      mkdirSync(path.dirname(path.join(dir, rel)), { recursive: true });
      writeFileSync(path.join(dir, rel), text);
      git("add", "--", rel);
    }
    // Untracked and binary files are not part of the scan.
    writeFileSync(path.join(dir, "untracked.txt"), VAULT_NAMES[0]);
    writeFileSync(path.join(dir, "blob.bin"), Buffer.concat([Buffer.from([0, 1, 2]), Buffer.from(VAULT_NAMES[0])]));
    git("add", "--", "blob.bin");

    const found = scanTree(dir).map((h) => h.split(":")[0]);
    assert.deepEqual(found.sort(), [".gitignore", "brand-new-top-level/notes.txt", "src-tauri/build.rs", "src-tauri/tauri.windows.conf.json", "src/main.js"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("this file is tracked and scanned like any other: no self-exemption, and its own text is clean", () => {
  const src = readFileSync(fileURLToPath(import.meta.url), "utf8");
  assert.deepEqual(privateHits(src), [], "the test file quotes a private name or id");
  assert.ok(trackedFiles(ROOT).includes("src/lib/private-names.test.js"), "scanTree covers this file");
});
