"""Static tests of scripts/e2e/tauri_smoke.mjs: the smoke test opens no window but the app's own.

On a self-hosted runner a job step has no console, so every console program the smoke starts would get a new,
visible one, and msedgedriver's default switch `--enable-logging` makes msedgewebview2.exe allocate a console of its
own. Both took the focus of whoever sat at the machine.

    python -m unittest discover -s tools -p "test_*.py"
"""

import re
import unittest
from pathlib import Path

SMOKE = Path(__file__).resolve().parent.parent / "scripts" / "e2e" / "tauri_smoke.mjs"


def calls(source, name):
    """The argument text of every call `name(...)`, parentheses balanced."""
    found = []
    for m in re.finditer(r"\b" + re.escape(name) + r"\(", source):
        depth, i = 1, m.end()
        while depth and i < len(source):
            depth += {"(": 1, ")": -1}.get(source[i], 0)
            i += 1
        found.append(source[m.end() : i - 1])
    return found


class SmokeOpensNoWindow(unittest.TestCase):
    def setUp(self):
        self.source = SMOKE.read_text(encoding="utf-8")

    def test_every_program_starts_hidden(self):
        starts = calls(self.source, "spawn") + calls(self.source, "spawnSync")
        self.assertGreaterEqual(len(starts), 3, "the smoke starts where/which and a driver")
        for args in starts:
            self.assertIn("windowsHide: true", args, f"a start without windowsHide: {args.strip()[:80]}")

    def test_webview2_gets_no_enable_logging(self):
        body = re.search(r"const nativeCapabilities = \(application\) => \((.*?)\n\}\);", self.source, re.S)
        self.assertIsNotNone(body, "nativeCapabilities is defined")
        self.assertRegex(body.group(1), r"excludeSwitches:\s*\[[^\]]*'enable-logging'")
        self.assertRegex(self.source, r"alwaysMatch:\s*windows\s*\?\s*nativeCapabilities\(app\)")

    def test_windows_goes_straight_to_msedgedriver(self):
        # tauri-driver 2.1 replaces ms:edgeOptions, so excludeSwitches would not reach msedgedriver through it
        self.assertRegex(self.source, r"windows\s*\?\s*spawn\(native,")
        self.assertNotIn("--native-driver", self.source)


if __name__ == "__main__":
    unittest.main()
