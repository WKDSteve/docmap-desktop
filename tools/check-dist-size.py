#!/usr/bin/env python3
"""Fails when what a build ships is above its budget.

    python tools/check-dist-size.py <folder or file> --budget <kB>

A folder counts as the sum of its files (source maps left out): a small page that is served from the binary or opened
from disk is parsed at every start, so its size is a cost of every start, not only of the first load. Without a number
it only grows. The budget is the measured size plus about a tenth; when it is used up, find what grew (the output
names the five largest files) before raising it, and write the reason where the number stands.

`--budget` is in kB of 1024 bytes. The exit code is 1 over the budget or when there is nothing to measure (an empty
folder or a missing path is an error, not a small page).
"""

from __future__ import annotations

import argparse
import os
import sys


def files_of(path: str) -> list[tuple[str, int]]:
    """(relative name, bytes) of a file, or of every file of a folder except source maps."""
    if os.path.isfile(path):
        return [(os.path.basename(path), os.path.getsize(path))]
    out: list[tuple[str, int]] = []
    for dirpath, _dirs, names in os.walk(path):
        for name in names:
            if name.endswith(".map"):
                continue
            full = os.path.join(dirpath, name)
            out.append((os.path.relpath(full, path).replace(os.sep, "/"), os.path.getsize(full)))
    return out


def check(path: str, budget_kb: float) -> tuple[bool, str]:
    if not os.path.exists(path):
        return False, f"error: {path} does not exist (was the page built?)"
    files = files_of(path)
    if not files:
        return False, f"error: {path} holds no file: a page without files is an error, not a small page"
    total = sum(size for _name, size in files)
    kb = total / 1024
    largest = ", ".join(f"{name} {size / 1024:.0f} kB" for name, size in sorted(files, key=lambda f: -f[1])[:5])
    if kb > budget_kb:
        return False, f"over budget: {path} is {kb:.1f} kB, budget {budget_kb:g} kB ({len(files)} files; largest: {largest})"
    return True, f"ok: {path} is {kb:.1f} kB of {budget_kb:g} kB ({len(files)} files)"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("path", help="the built folder or file")
    parser.add_argument("--budget", type=float, required=True, help="budget in kB (1024 bytes)")
    args = parser.parse_args(argv)
    ok, message = check(args.path, args.budget)
    print(message, file=sys.stdout if ok else sys.stderr)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
