"""Tests of check-appkit-pin.py: every way the appkit pin of Cargo.lock can be wrong is refused, and only a tag that
still names the pinned commit passes. The remote is never asked: `remote_commit` and `subprocess.run` are replaced.

    python -m unittest discover -s tools -p "test_*.py"
"""

import importlib.util
import io
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

SPEC = importlib.util.spec_from_file_location("check_appkit_pin", Path(__file__).with_name("check-appkit-pin.py"))
pin = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pin)

SHA = "deadf4705b4a192c7a5744d9015d59b9d23b2075"
OTHER = "56df6b11ba66e1d9dd918a7284709bfb84e36dcd"
URL = "https://github.com/WKDSteve/appkit"


def lock(*sources):
    """A Cargo.lock with one appkit package per source, and one package from crates.io."""
    names = ["appkit-core", "appkit-hub", "appkit-testkit"]
    blocks = [
        '[[package]]\nname = "serde"\nversion = "1.0.0"\n'
        'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
    ]
    for name, source in zip(names, sources):
        blocks.append(f'[[package]]\nname = "{name}"\nversion = "0.0.0"\nsource = "{source}"\n')
    return "version = 4\n\n" + "\n".join(blocks)


def tagged(tag="v0.59.0", sha=SHA, url=URL):
    return f"git+{url}?tag={tag}#{sha}"


class Pinned(unittest.TestCase):
    def test_a_tag_and_one_commit_is_a_pin(self):
        pins, problems = pin.pinned(pin.appkit_packages(lock(tagged(), tagged())))
        self.assertEqual(problems, [])
        self.assertEqual(pins, {("v0.59.0", SHA)})

    def test_a_branch_a_rev_or_no_tag_is_refused(self):
        for query in ("branch=main", "rev=deadf47", ""):
            source = f"git+{URL}{'?' + query if query else ''}#{SHA}"
            _, problems = pin.pinned(pin.appkit_packages(lock(source)))
            self.assertEqual(len(problems), 1, query)
            self.assertIn("not by a tag", problems[0])

    def test_another_spelling_of_the_repository_is_refused(self):
        _, problems = pin.pinned(pin.appkit_packages(lock(tagged(url=URL + ".git"))))
        self.assertEqual(len(problems), 1)
        self.assertIn("not a source of", problems[0])

    def test_packages_on_two_commits_are_refused(self):
        _, problems = pin.pinned(pin.appkit_packages(lock(tagged(), tagged(tag="v0.60.0", sha=OTHER))))
        self.assertTrue(any("different commits" in p for p in problems))


class Main(unittest.TestCase):
    def run_main(self, text, now=SHA, error=None):
        with tempfile.TemporaryDirectory() as folder:
            path = os.path.join(folder, "Cargo.lock")
            with open(path, "w", encoding="utf-8") as f:
                f.write(text)
            out = io.StringIO()
            side = error if error else None
            with mock.patch.object(pin, "remote_commit", return_value=now, side_effect=side), redirect_stdout(out):
                code = pin.main(["--lock", path])
        return code, out.getvalue()

    def test_a_tag_that_still_names_the_commit_passes(self):
        code, out = self.run_main(lock(tagged()))
        self.assertEqual(code, 0, out)
        self.assertIn("appkit-core at v0.59.0", out)

    def test_a_moved_tag_fails(self):
        code, out = self.run_main(lock(tagged()), now=OTHER)
        self.assertEqual(code, 1)
        self.assertIn("the tag was moved", out)

    def test_a_tag_that_is_gone_fails(self):
        code, out = self.run_main(lock(tagged()), now=None)
        self.assertEqual(code, 1)
        self.assertIn("is not in", out)

    def test_a_lock_without_appkit_fails(self):
        code, out = self.run_main(lock())
        self.assertEqual(code, 1)
        self.assertIn("looked at nothing", out)

    def test_no_answer_from_the_remote_is_no_pass(self):
        code, out = self.run_main(lock(tagged()), error=RuntimeError("git ls-remote failed: offline"))
        self.assertEqual(code, 2)
        self.assertIn("could not run", out)

    def test_a_missing_lock_is_no_pass(self):
        out = io.StringIO()
        with redirect_stdout(out):
            code = pin.main(["--lock", os.path.join(tempfile.gettempdir(), "no-such-folder-x", "Cargo.lock")])
        self.assertEqual(code, 2)


class Remote(unittest.TestCase):
    def test_the_peeled_commit_of_an_annotated_tag_wins(self):
        answer = subprocess.CompletedProcess(
            [], 0, stdout=f"{OTHER}\trefs/tags/v0.59.0\n{SHA}\trefs/tags/v0.59.0^{{}}\n", stderr=""
        )
        with mock.patch.object(pin.subprocess, "run", return_value=answer):
            self.assertEqual(pin.remote_commit("v0.59.0"), SHA)

    def test_a_lightweight_tag_is_read(self):
        answer = subprocess.CompletedProcess([], 0, stdout=f"{SHA}\trefs/tags/v0.59.0\n", stderr="")
        with mock.patch.object(pin.subprocess, "run", return_value=answer):
            self.assertEqual(pin.remote_commit("v0.59.0"), SHA)

    def test_a_failing_git_raises(self):
        answer = subprocess.CompletedProcess([], 128, stdout="", stderr="fatal: unable to access")
        with mock.patch.object(pin.subprocess, "run", return_value=answer):
            with self.assertRaises(RuntimeError):
                pin.remote_commit("v0.59.0")


if __name__ == "__main__":
    unittest.main()
