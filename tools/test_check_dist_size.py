import importlib.util
import os
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("check_dist_size", os.path.join(HERE, "check-dist-size.py"))
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


class CheckDistSize(unittest.TestCase):
    def make(self, root, name, size):
        path = os.path.join(root, name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(b"x" * size)

    def test_a_folder_within_the_budget_passes_and_one_over_it_fails(self):
        with tempfile.TemporaryDirectory(prefix="dist-size-") as root:
            self.make(root, "app.js", 6 * 1024)
            self.make(root, "assets/style.css", 2 * 1024)
            ok, message = mod.check(root, 10)
            self.assertTrue(ok, message)
            ok, message = mod.check(root, 7)
            self.assertFalse(ok)
            self.assertIn("over budget", message)
            self.assertIn("app.js", message)

    def test_source_maps_do_not_count(self):
        with tempfile.TemporaryDirectory(prefix="dist-size-") as root:
            self.make(root, "app.js", 1024)
            self.make(root, "app.js.map", 500 * 1024)
            self.assertTrue(mod.check(root, 2)[0])

    def test_a_single_file_is_measured_as_it_is(self):
        with tempfile.TemporaryDirectory(prefix="dist-size-") as root:
            self.make(root, "page.html", 3 * 1024)
            self.assertTrue(mod.check(os.path.join(root, "page.html"), 4)[0])
            self.assertFalse(mod.check(os.path.join(root, "page.html"), 2)[0])

    def test_nothing_to_measure_is_an_error_not_a_small_page(self):
        with tempfile.TemporaryDirectory(prefix="dist-size-") as root:
            ok, message = mod.check(root, 100)
            self.assertFalse(ok)
            self.assertIn("holds no file", message)
            self.assertFalse(mod.check(os.path.join(root, "missing"), 100)[0])


if __name__ == "__main__":
    unittest.main()
