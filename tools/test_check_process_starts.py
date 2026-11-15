"""Tests of tools/check-process-starts.py: `python -m unittest discover tools`."""

import importlib.util
import os
import pathlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "check_process_starts", pathlib.Path(__file__).with_name("check-process-starts.py")
)
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


def lines(text):
    return [number for number, _ in check.findings(text)]


class Findings(unittest.TestCase):
    def test_each_way_to_start_a_process_is_found(self):
        for line in [
            "let c = Command::new(program);",
            "std::process::Command::new(\"git\")",
            "tokio::process::Command::new(nvim).spawn()",
            "command.creation_flags(0x0800_0000);",
            "    .creation_flags (FLAGS)",
            "CreateProcessW(null(), line, ...)",
            "use windows_sys::Win32::UI::Shell::ShellExecuteW;",
        ]:
            with self.subTest(line=line):
                self.assertEqual(lines("fn f() {\n" + line + "\n}\n"), [2])

    def test_the_helper_and_other_names_pass(self):
        text = (
            "let start = Start::new(\"app\", program);\n"
            "child::spawn(&start)?;\n"
            "pub fn creation_flags(window: Window) -> u32 {\n"
            "let flags = creation_flags(window);\n"
            "let c = MyCommand::new();\n"
            "Subcommand::newer();\n"
        )
        self.assertEqual(lines(text), [])

    def test_a_comment_line_is_not_code(self):
        self.assertEqual(lines("// Command::new(x)\n    /// `.creation_flags(1)`\n"), [])

    def test_a_test_module_ends_the_look(self):
        text = (
            "fn f() { Command::new(a); }\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    fn g() { Command::new(b); }\n"
            "}\n"
        )
        self.assertEqual(lines(text), [1])

    def test_a_cfg_test_before_something_else_than_a_module_does_not_end_the_look(self):
        text = "#[cfg(test)]\nuse std::fs;\nfn f() { Command::new(a); }\n#[cfg(test)]\n"
        self.assertEqual(lines(text), [3])

    def test_an_indented_cfg_test_does_not_end_the_look(self):
        text = "    #[cfg(test)]\n    fn only_in_tests() {}\nfn f() { Command::new(a); }\n"
        self.assertEqual(lines(text), [3])


class LookedAt(unittest.TestCase):
    def test_tests_benches_examples_and_build_scripts_are_skipped(self):
        for path in [
            "crates/a/tests/run.rs",
            "tests/it.rs",
            "crates/a/benches/b.rs",
            "examples/demo.rs",
            "crates/a/build.rs",
        ]:
            self.assertFalse(check.looked_at(path), path)
        for path in ["src/main.rs", "crates/a/src/bin/tool.rs", "crates/a/src/tests_support.rs"]:
            self.assertTrue(check.looked_at(path), path)


class Allowed(unittest.TestCase):
    def allow(self, text):
        with tempfile.TemporaryDirectory() as folder:
            path = os.path.join(folder, "allow")
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(text)
            return check.read_allow(path)

    def test_the_file_is_read_with_comments(self):
        allowed, problems = self.allow(
            "# why\n\nsrc/child.rs 2 the helper itself\nsrc/old.rs 1 moves later # soon\n"
        )
        self.assertEqual(problems, [])
        self.assertEqual(
            allowed, {"src/child.rs": (2, "the helper itself"), "src/old.rs": (1, "moves later")}
        )

    def test_a_missing_file_means_no_exceptions(self):
        self.assertEqual(check.read_allow(os.path.join("no", "such", "file")), ({}, []))

    def test_a_line_without_count_or_reason_and_a_double_entry_are_problems(self):
        for text in ["src/a.rs\n", "src/a.rs 2\n", "src/a.rs two reasons\n", "a.rs 1 x\na.rs 1 y\n"]:
            with self.subTest(text=text):
                self.assertEqual(len(self.allow(text)[1]), 1)

    def test_a_start_outside_the_list_is_reported_with_its_line(self):
        found = {"src/run.rs": [(7, "Command::new(x)")]}
        self.assertEqual(
            check.check(found, {}), ["src/run.rs:7: starts a process itself: Command::new(x)"]
        )

    def test_a_listed_file_with_its_count_passes(self):
        found = {"src/child.rs": [(1, "a"), (2, "b")]}
        self.assertEqual(check.check(found, {"src/child.rs": (2, "the helper")}), [])

    def test_more_or_fewer_starts_than_listed_fail(self):
        found = {"src/child.rs": [(1, "a"), (2, "b")]}
        for count in [1, 3]:
            with self.subTest(count=count):
                problems = check.check(found, {"src/child.rs": (count, "the helper")})
                self.assertIn(f"the allow file says {count}", problems[0])
                self.assertEqual(len(problems), 3)

    def test_an_entry_for_a_file_without_a_start_fails(self):
        problems = check.check({}, {"src/gone.rs": (1, "old")})
        self.assertEqual(len(problems), 1)
        self.assertIn("remove the entry", problems[0])


if __name__ == "__main__":
    unittest.main()
