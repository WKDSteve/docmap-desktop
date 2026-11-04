"""Tests of tools/check-appkit-tokens.py: python -m unittest discover -s tools -p "test_*.py"."""

import contextlib
import importlib.util
import io
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("check_appkit_tokens", HERE / "check-appkit-tokens.py")
tokens = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tokens)

KIT = """/* a comment with --fake: light-dark(#111, #222); in it */
:root {
  --bg: light-dark(#f7f7f5, #15171a);
  --text: light-dark(#1b1d20, #e8e9ea);
  --accent-soft: color-mix(in srgb, var(--accent) 14%, transparent);
  --backdrop: rgb(0 0 0 / 0.45);
  --radius: 6px;
}
"""


def git(cwd, *args):
    subprocess.run(["git", "-C", str(cwd), *args], check=True, capture_output=True)


def quiet(argv):
    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        return tokens.main(argv)


class Pin(unittest.TestCase):
    def test_reads_tag_and_commit(self):
        text = "# comment\ntag = v1.2.3\ncommit = " + "a" * 40 + "\n"
        self.assertEqual(tokens.read_pin(text), ("v1.2.3", "a" * 40))

    def test_refuses_a_branch_or_a_short_commit(self):
        with self.assertRaises(ValueError):
            tokens.read_pin("tag = main\ncommit = " + "a" * 40)
        with self.assertRaises(ValueError):
            tokens.read_pin("tag = v1.2.3\ncommit = abc1234")
        with self.assertRaises(ValueError):
            tokens.read_pin("tag v1.2.3")

    def test_the_repository_pin_is_readable(self):
        tag, _ = tokens.read_pin((HERE / "appkit-ui.pin").read_text(encoding="utf-8"))
        self.assertRegex(tag, r"^v\d+\.\d+\.\d+$")


class LsRemote(unittest.TestCase):
    def test_the_peeled_commit_of_an_annotated_tag_wins(self):
        out = "1111\trefs/tags/v1.0.0\n2222\trefs/tags/v1.0.0^{}\n"
        self.assertEqual(tokens.commit_of_tag(out, "v1.0.0"), "2222")

    def test_a_lightweight_tag(self):
        self.assertEqual(tokens.commit_of_tag("3333\trefs/tags/v1.0.0\n", "v1.0.0"), "3333")

    def test_another_tag_is_no_answer(self):
        self.assertIsNone(tokens.commit_of_tag("4444\trefs/tags/v1.0.10\n", "v1.0.1"))


class Fallback(unittest.TestCase):
    def test_pairs_skip_comments_and_derived_tokens(self):
        self.assertEqual(
            tokens.pairs(KIT), [("--bg", "#f7f7f5", "#15171a"), ("--text", "#1b1d20", "#e8e9ea")]
        )

    def test_no_pair_is_an_error_not_an_empty_fallback(self):
        with self.assertRaises(ValueError):
            tokens.pairs(":root { --radius: 6px; }")

    def test_a_light_dark_the_fallback_cannot_copy_is_an_error_not_a_token_left_out(self):
        for value in ("light-dark(rgb(255 229 138), rgb(107 90 0))", "color-mix(in srgb, light-dark(#111, #222) 50%, red)"):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "--mark"):
                tokens.pairs(KIT.replace("--radius: 6px;", f"--mark: {value};"))

    def test_the_backdrop_gets_its_own_copy_of_the_tokens_it_reads(self):
        css = tokens.fallback(KIT, "v9.9.9")
        self.assertIn("  ::backdrop {\n    --backdrop: rgb(0 0 0 / 0.45);\n  }\n}\n", css)

    def test_a_kit_without_the_backdrop_token_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "--backdrop"):
            tokens.fallback(KIT.replace("--backdrop", "--shade"), "v9.9.9")

    def test_light_by_default_dark_by_system_or_by_choice(self):
        css = tokens.fallback(KIT, "v9.9.9")
        self.assertIn("@supports not (color: light-dark(", css)
        light, rest = css.split("@media (prefers-color-scheme: dark)")
        self.assertIn("--bg:   #f7f7f5;", light)
        guarded, forced = rest.split(":root[data-theme='dark']")
        self.assertIn(":root:not([data-theme='light'])", guarded)
        for block in (guarded, forced):
            self.assertIn("#15171a", block)
            self.assertNotIn("#f7f7f5", block)
        self.assertIn("appkit v9.9.9", css)

    def test_the_palette_wins_and_adds_its_own_tokens(self):
        palette = ":root {\n  --bg: light-dark(#f6f6f9, #131317);\n  --chart-2: light-dark(#111111, #eeeeee);\n}\n"
        css = tokens.fallback(KIT, "v9.9.9", palette)
        self.assertIn("--bg:      #f6f6f9;", css)
        self.assertIn("--bg:      #131317;", css)
        self.assertNotIn("#f7f7f5", css)
        self.assertIn("--text:    #1b1d20;", css)
        self.assertIn("--chart-2: #111111;", css)
        self.assertLess(css.index("--text:"), css.index("--chart-2:"))
        self.assertIn("and src/palette.css", css)

    def test_without_a_palette_the_kit_alone(self):
        css = tokens.fallback(KIT, "v9.9.9")
        self.assertIn("--bg:   #f7f7f5;", css)
        self.assertNotIn("palette", css)

    def test_a_palette_the_fallback_cannot_read_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "src/palette.css"):
            tokens.fallback(KIT, "v9.9.9", ":root { --bg: #ffffff; }\n")

    def test_the_repository_fallback_is_the_generated_one(self):
        # Offline: a hand edit of the fallback is caught here before CI asks GitHub.
        root = HERE.parent
        tag, _ = tokens.read_pin((root / tokens.PIN).read_text(encoding="utf-8"))
        copy = (root / tokens.COPY).read_text(encoding="utf-8")
        palette = tokens.read(root / tokens.PALETTE)
        want = tokens.fallback(copy, tag, palette)
        self.assertEqual(tokens.drift(tokens.FALLBACK, want, (root / tokens.FALLBACK).read_text(encoding="utf-8")), [])


class EndToEnd(unittest.TestCase):
    """main() against a local clone made here: a tag, a pin, a copy and a fallback."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = pathlib.Path(self.tmp.name)
        self.kit, self.app = base / "appkit", base / "app"
        (self.kit / "ui-kit/src").mkdir(parents=True)
        (self.kit / tokens.KIT_PATH).write_bytes(KIT.encode())
        git(self.kit, "init", "-q")
        git(self.kit, "add", ".")
        git(self.kit, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "kit")
        git(self.kit, "-c", "user.name=t", "-c", "user.email=t@t", "tag", "-a", "v1.0.0", "-m", "v1.0.0")
        self.commit = subprocess.run(
            ["git", "-C", str(self.kit), "rev-parse", "HEAD"], check=True, capture_output=True, text=True
        ).stdout.strip()
        (self.app / "tools").mkdir(parents=True)
        (self.app / "src").mkdir()
        self.pin(self.commit)
        self.assertEqual(self.run_check("--write"), 0)

    def tearDown(self):
        self.tmp.cleanup()

    def pin(self, commit, tag="v1.0.0"):
        (self.app / tokens.PIN).write_text(f"tag = {tag}\ncommit = {commit}\n", encoding="utf-8")

    def run_check(self, *extra):
        return quiet(["--appkit", str(self.kit), "--root", str(self.app), *extra])

    def test_in_step(self):
        self.assertEqual(self.run_check(), 0)

    def test_a_new_palette_is_drift_until_written(self):
        (self.app / tokens.PALETTE).write_text(":root {\n  --bg: light-dark(#010101, #020202);\n}\n", encoding="utf-8")
        self.assertEqual(self.run_check(), 1)
        self.assertEqual(self.run_check("--write"), 0)
        self.assertEqual(self.run_check(), 0)
        self.assertIn("#010101", (self.app / tokens.FALLBACK).read_text(encoding="utf-8"))

    def test_crlf_in_the_working_copy_is_no_drift(self):
        path = self.app / tokens.COPY
        path.write_bytes(path.read_bytes().replace(b"\n", b"\r\n"))
        self.assertEqual(self.run_check(), 0)

    def test_an_edited_copy_is_drift(self):
        path = self.app / tokens.COPY
        path.write_text(path.read_text().replace("#f7f7f5", "#ffffff"), encoding="utf-8")
        self.assertEqual(self.run_check(), 1)

    def test_a_missing_copy_is_drift(self):
        (self.app / tokens.COPY).unlink()
        self.assertEqual(self.run_check(), 1)

    def test_an_edited_fallback_is_drift(self):
        path = self.app / tokens.FALLBACK
        path.write_text(path.read_text().replace("#15171a", "#000000", 1), encoding="utf-8")
        self.assertEqual(self.run_check(), 1)

    def test_a_moved_tag_is_drift(self):
        self.pin("b" * 40)
        self.assertEqual(self.run_check(), 1)

    def test_a_tag_that_is_gone_is_drift(self):
        self.pin(self.commit, tag="v2.0.0")
        self.assertEqual(self.run_check(), 1)

    def test_write_repairs_drift(self):
        (self.app / tokens.FALLBACK).write_text("edited", encoding="utf-8")
        self.assertEqual(self.run_check("--write"), 0)
        self.assertEqual(self.run_check(), 0)

    def test_a_kit_the_fallback_cannot_follow_is_an_error_not_a_traceback(self):
        (self.kit / tokens.KIT_PATH).write_bytes(KIT.replace("#15171a", "rgb(21 23 26)").encode())
        git(self.kit, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qam", "rgb")
        git(self.kit, "-c", "user.name=t", "-c", "user.email=t@t", "tag", "-a", "v1.1.0", "-m", "v1.1.0")
        commit = subprocess.run(
            ["git", "-C", str(self.kit), "rev-parse", "HEAD"], check=True, capture_output=True, text=True
        ).stdout.strip()
        self.pin(commit, tag="v1.1.0")
        self.assertEqual(self.run_check(), 2)

    def test_no_clone_is_unreachable_not_a_pass(self):
        self.assertEqual(quiet(["--appkit", str(self.app / "nowhere"), "--root", str(self.app)]), 2)

    def test_no_pin_is_an_error_not_a_pass(self):
        (self.app / tokens.PIN).unlink()
        self.assertEqual(self.run_check(), 2)


if __name__ == "__main__":
    unittest.main()
