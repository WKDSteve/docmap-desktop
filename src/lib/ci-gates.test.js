import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

// The formatting gate lives in a YAML file and a README paragraph, neither of
// which a compiler looks at: dropping the step, gating it to one platform or
// letting the README's local mirror drift would all leave every other check
// green. These tests pin the three places that have to agree.

const ROOT = fileURLToPath(new URL("../../", import.meta.url));
const read = (rel) => readFileSync(path.join(ROOT, rel), "utf8").replace(/\r\n/g, "\n");

// The `- name:` blocks of the one job, in file order.
function steps(yml) {
  const out = [];
  let cur = null;
  for (const line of yml.split("\n")) {
    const m = /^ {6}- (?:name|uses): (.*)$/.exec(line);
    if (m) {
      cur = { head: m[1], body: line + "\n" };
      out.push(cur);
    } else if (cur) {
      cur.body += line + "\n";
    }
  }
  return out;
}

test("ci.yml runs cargo fmt --check on every platform, before clippy", () => {
  const all = steps(read(".github/workflows/ci.yml"));
  const fmt = all.findIndex((s) => /^\s*run: cargo fmt --check\s*$/m.test(s.body));
  const clippy = all.findIndex((s) => /cargo clippy/.test(s.body));
  assert.ok(fmt >= 0, "no step runs `cargo fmt --check`");
  assert.ok(clippy >= 0, "no step runs clippy");
  assert.ok(fmt < clippy, "the format check must come before clippy: it is the cheaper failure");
  assert.match(all[fmt].body, /working-directory: src-tauri/, "must run in the crate");
  assert.doesNotMatch(all[fmt].body, /^\s*if:/m, "an `if:` would exempt a platform from the gate");
});

test("the toolchain comes from the runner and the workflow says it needs clippy and rustfmt", () => {
  // The self-hosted runner has no toolchain step (the action runs the WSL bash there); the machine's
  // stable toolchain with both components is what the gates use.
  const yml = read(".github/workflows/ci.yml");
  assert.doesNotMatch(code(yml), /rust-toolchain/, "a toolchain action would run `shell: bash` (WSL) on the runner");
  const m = /\(The toolchain is the stable one of the runner machine, with ([^)]*)\)/.exec(yml.replace(/\n\s*#\s*/g, " "));
  assert.ok(m, "the comment naming the runner toolchain's components is gone");
  assert.match(m[1], /\brustfmt\b/);
  assert.match(m[1], /\bclippy\b/);
});

test("rustfmt.toml pins Unix line endings for every checkout", () => {
  assert.match(read("src-tauri/rustfmt.toml"), /^newline_style\s*=\s*"Unix"\s*$/m);
});

test("the README's test section names the local mirror of both Rust gates", () => {
  const readme = read("README.md");
  const at = readme.indexOf("### Tests");
  // `slice(-1)` of a missing heading is the last character, which is "shorter
  // than the file" and made this guard pass on exactly the case it is for.
  assert.ok(at >= 0, "no Tests section: was the heading renamed?");
  const section = readme.slice(at);
  assert.match(section, /cargo fmt --check/);
  assert.match(section, /cargo clippy --locked --all-targets -- -D warnings/);
});

/** The YAML without its comment lines: a comment that mentions a key is not the key. */
const code = (yml) => yml.replace(/^\s*#.*$/gm, "");

test("no gate can fail without failing the run: no continue-on-error, no matrix exclusion", () => {
  const yml = read(".github/workflows/ci.yml");
  // A step that may fail is not a gate. The one allowed kind says so in its name: "(informative)" (the legacy-scan
  // of the web platform), it never decides the run and no Rust or Node gate may carry the key.
  let inSteps = 0;
  for (const s of steps(yml)) {
    if (!/continue-on-error/.test(code(s.body))) continue;
    inSteps += 1;
    assert.match(s.body, /name:\s*[^\n]*\(informative\)/, "a step that may fail must be named (informative): it is not a gate");
  }
  // Not on a job either: every use of the key sits in a step counted above
  assert.equal((code(yml).match(/continue-on-error/g) ?? []).length, inSteps, "continue-on-error on a job makes the whole job optional");
  assert.doesNotMatch(code(yml), /^\s*exclude:/m, "a matrix exclusion exempts a platform from every gate");
  // Per step as well, for the Rust gates named in the findings.
  const all = steps(yml);
  for (const re of [/cargo fmt --check/, /cargo clippy/, /cargo test/, /node --test/]) {
    const i = all.findIndex((s) => re.test(code(s.body)));
    assert.ok(i >= 0, `no step runs ${re}`);
    assert.doesNotMatch(code(all[i].body), /continue-on-error/, `the ${re} step is allowed to fail`);
    assert.doesNotMatch(code(all[i].body), /^\s*if:/m, `the ${re} step is skipped on some platform`);
  }
});

test("only a push reaches the self-hosted runner; every other event gets windows-latest", () => {
  // A pull request from a fork must never run its own code on the author's machine. Every job picks its runner
  // with one matrix expression: self-hosted only when the event is `push`, otherwise the hosted VM.
  const yml = code(read(".github/workflows/ci.yml"));
  const all = [...yml.matchAll(/^\s*runs-on: (.*)$/gm)].map((m) => m[1].trim());
  // A job may also name a throwaway VM of GitHub outright (the Linux lint): it never reaches the author's machine.
  // Anything else goes through the matrix.
  const HOSTED = ["ubuntu-latest"];
  const fixed = all.filter((r) => r !== "${{ matrix.runs-on }}");
  for (const r of fixed) assert.ok(HOSTED.includes(r), `a job runs on ${r}, not on the event's runner`);
  const runsOn = all.filter((r) => r === "${{ matrix.runs-on }}");
  assert.ok(runsOn.length >= 3, "the jobs of ci.yml are gone");
  const includes = [...yml.matchAll(/^\s*include: (.*)$/gm)].map((m) => m[1].trim());
  assert.equal(includes.length, runsOn.length, "every job needs the runner matrix");
  for (const inc of includes) {
    assert.equal(inc, includes[0], "the jobs pick their runner with different expressions");
    const m = /^\$\{\{ fromJSON\(github\.event_name == 'push' && '(.*)' \|\| '(.*)'\) \}\}$/.exec(inc);
    assert.ok(m, `the runner matrix is not keyed on a push: ${inc}`);
    const [onPush, otherwise] = [JSON.parse(m[1]), JSON.parse(m[2])];
    assert.deepEqual(onPush.map((e) => e["runs-on"]), [["self-hosted", "windows"]]);
    assert.deepEqual(otherwise.map((e) => e["runs-on"]), ["windows-latest"]);
    for (const e of [...onPush, ...otherwise]) {
      assert.match(e.shell, /bash/, "a leg without a bash shell");
      assert.match(e.leg ?? "", /^[a-z-]+$/, "a leg without a short name: the job name would be the shell string");
    }
  }
  const names = [...yml.matchAll(/^ {4}name: (.*)$/gm)].map((m) => m[1].trim());
  assert.equal(names.length, all.length, "every job needs a name");
  const legs = names.filter((n) => /^[a-z]+ \(\$\{\{ matrix\.leg \}\}\)$/.test(n));
  assert.equal(legs.length, runsOn.length, `a job of the matrix does not show the leg: ${names.join(", ")}`);
  for (const n of names.filter((x) => !legs.includes(x))) {
    assert.ok(HOSTED.includes(/^[a-z]+ \((.*)\)$/.exec(n)?.[1]), `job name ${n} does not show its runner`);
  }
  const shells = [...yml.matchAll(/^\s*shell: (.*)$/gm)].map((m) => m[1].trim());
  for (const s of shells) assert.equal(s, "${{ matrix.shell }}", `a job pins the shell ${s} for both runners`);
});

test("ci.yml hands no credential of the runner to the code it checks", () => {
  // `gh auth token` on the self-hosted runner is the owner's full login; nothing in a step may read it.
  const yml = code(read(".github/workflows/ci.yml"));
  assert.doesNotMatch(yml, /gh auth token/);
  assert.doesNotMatch(yml, /pull_request_target/);
  assert.match(yml, /^permissions:\n {2}contents: read$/m, "the token of the run must be read-only");
});

test("the LF checkout the fmt step relies on is there", () => {
  // `newline_style = "Unix"` makes a CRLF file a *failure*; what keeps the
  // Windows runner green is that git checks files out with LF.
  assert.match(read(".gitattributes"), /^\*\s+text=auto\s+eol=lf\s*$/m);
});

test("the comment on the floating toolchain names both rustfmt and clippy", () => {
  const yml = read(".github/workflows/ci.yml");
  const at = yml.indexOf("floating `stable`");
  assert.ok(at >= 0, "the comment on the toolchain choice is gone");
  const end = yml.indexOf("- name: stage a placeholder engine sidecar", at);
  assert.ok(end > at, "the step after the comment is gone: the slice would be the rest of the file");
  const comment = yml.slice(at, end);
  assert.match(comment, /rustfmt/);
  assert.match(comment, /clippy/);
});

test("no workflow uses Swatinem/rust-cache: on the self-hosted runner it deletes files of the shared ~/.cargo", () => {
  // Its save step removes ~/.cargo/credentials.toml, prunes registry/src, registry/cache and git/checkouts to the
  // crates of this one project (the other Rust repos of the runner start from nothing) and, by default,
  // deletes every program that was in ~/.cargo/bin when it started (rustup.exe and the `cargo install` tools:
  // 2026-10-27). It is built for throw-away runners.
  for (const name of ["ci.yml", "rolling.yml", "release.yml"]) {
    assert.doesNotMatch(code(read(`.github/workflows/${name}`)), /rust-cache/, `${name} uses rust-cache`);
  }
});

test("ci.yml holds the window (src/) to a size budget with the shared tool", () => {
  const all = steps(read(".github/workflows/ci.yml"));
  const step = all.find((s) => /check-dist-size\.py src --budget \d+/.test(s.body));
  assert.ok(step, "no step measures src/ with tools/check-dist-size.py");
  assert.doesNotMatch(step.body, /^\s*if:/m, "an `if:` would exempt a run from the budget");
  // The budget is the measure plus about a tenth: not so loose that growth
  // passes unseen, not so tight that the next small change fails.
  const budget = Number(/--budget (\d+)/.exec(step.body)[1]);
  const kb = (function size(dir) {
    let n = 0;
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      n += e.isDirectory() ? size(p) : e.name.endsWith(".map") ? 0 : statSync(p).size;
    }
    return n;
  })(path.join(ROOT, "src")) / 1024;
  assert.ok(kb <= budget, `src/ is ${kb.toFixed(0)} kB, over its budget of ${budget} kB`);
  assert.ok(budget <= kb * 1.25, `the budget ${budget} kB is more than a quarter above the ${kb.toFixed(0)} kB measured`);
});

test("the web job runs the two Chromium checks of the window and does not read a skip as a pass", () => {
  const all = steps(read(".github/workflows/ci.yml"));
  for (const script of ["web-rules.mjs", "trusted-types.mjs"]) {
    const step = all.find((s) => s.body.includes(`scripts/e2e/${script}`));
    assert.ok(step, `no CI step runs scripts/e2e/${script}`);
    assert.match(step.body, /E2E_BROWSER_CHANNEL: chrome/, `${script} needs the Chrome of the runner`);
    // Exit 77 means "a prerequisite is missing": in CI that is a failure, not a green step.
    assert.match(step.body, /\|\| *\{[^}]*exit 1/s, `${script}: a skip (77) must fail the step`);
  }
});
