#!/usr/bin/env python3
"""Fail when a colour is written in the stylesheets of src/ outside the tokens of appkit-ui.

The rules are appkit's own: tools/check-color-literals.py is its file, kept verbatim (from appkit b8c24c5, the same
copy as in tasks-desktop and rules-desktop) so a newer one replaces it without a merge. That file lets one token
file hold colours; this window has two, the copy of the kit's tokens and the plain-colour fallback generated from
it, and tools/check-appkit-tokens.py checks both against the kit. Every other tracked .css file under src/ is read
with the kit's rules.

Exit 0: clean. Exit 1: the places are listed (file, line, literal). Exit 2: git could not be asked, or no file was
read.
"""

import importlib.util
import os
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOTS = ["src"]
ALLOWED = {"src/appkit-tokens.css", "src/palette.css", "src/appkit-tokens-fallback.css"}


def kit_check():
    spec = importlib.util.spec_from_file_location("check_color_literals", HERE / "check-color-literals.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def findings(check, paths, read):
    """The (path, line, literal) of the colours in the files to check, and how many files were read."""
    found, count = [], 0
    for path in paths:
        if path in ALLOWED or not check.checked(path, ROOTS, ""):
            continue
        text = read(path)
        if text is None:
            continue
        count += 1
        found += [(path, line, literal) for line, literal in check.problems(path, text)]
    return found, count


def main():
    check = kit_check()
    # The kit's file asks git in the current folder. Run from anywhere else, it would read that folder's repository
    # (and pass it, if it has a clean src/): this script checks this repository, wherever it is started.
    os.chdir(HERE)
    top, paths = check.tracked_files()

    def read(path):
        try:
            with open(os.path.join(top, path), encoding="utf-8") as handle:
                return handle.read()
        except (OSError, UnicodeDecodeError):
            return None

    found, count = findings(check, paths, read)
    if count == 0:
        print("no tracked .css file under src/: nothing was checked", file=sys.stderr)
        return 2
    for path, line, literal in found:
        print(f"{path}:{line}: {literal}")
    if found:
        print(f"{len(found)} colour literal(s) outside the tokens: use a token of appkit-ui", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
