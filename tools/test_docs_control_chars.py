"""The documentation holds no control characters.

A backslash written through a shell is easily lost: `\\a` and `\\b` in `%LOCALAPPDATA%\\appkit-family\\bin` became the
bell and backspace characters in README.md, and the path read `%LOCALAPPDATA%ppkit-familyin` on GitHub. Every Markdown
file of the repository (outside `node_modules` and build output) may hold tab, line feed and carriage return, nothing
else below 0x20.

    python -m unittest discover -s tools -p "test_*.py"
"""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
SKIP = {"node_modules", "target", "dist", ".git", ".claude"}
CONTROL = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")


def markdown_files():
    for path in ROOT.rglob("*.md"):
        if SKIP.isdisjoint(path.relative_to(ROOT).parts):
            yield path


def control_chars(text):
    """(line, column, character) of every control character but tab, line feed and carriage return."""
    found = []
    for number, line in enumerate(text.split("\n"), start=1):
        for match in CONTROL.finditer(line):
            found.append((number, match.start() + 1, repr(match.group())))
    return found


class DocsHaveNoControlChars(unittest.TestCase):
    def test_the_detector_finds_what_a_lost_backslash_leaves(self):
        self.assertEqual(control_chars("a\x07ppkit\x08in"), [(1, 2, "'\\x07'"), (1, 8, "'\\x08'")])
        self.assertEqual(control_chars("tab\there\r\nnext"), [])

    def test_no_markdown_file_holds_one(self):
        files = list(markdown_files())
        self.assertTrue(any(p.name == "README.md" for p in files), "README.md was not looked at")
        bad = {}
        for path in files:
            found = control_chars(path.read_text(encoding="utf-8", errors="replace"))
            if found:
                bad[str(path.relative_to(ROOT))] = found
        self.assertEqual(bad, {})


if __name__ == "__main__":
    unittest.main()
