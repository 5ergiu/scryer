import copy
import unittest
from unittest.mock import patch

from windows_ci import WINDOWS_TESTS, filterset, main, validate_inventory


def inventory():
    return {"rust-suites": {
        package: {"package-name": package, "testcases": {
            f"tests::{name}": {"filter-match": {"status": "matches"},
                               "ignored": name == "credential_manager_round_trip"}
            for name in names
        }} for package, names in WINDOWS_TESTS.items()
    }}


class WindowsCiTests(unittest.TestCase):
    def test_complete_windows_inventory_is_accepted(self):
        validate_inventory(inventory())

    def test_missing_windows_test_is_rejected(self):
        data = inventory()
        data["rust-suites"].pop("scryer")
        with self.assertRaisesRegex(ValueError, "Missing"):
            validate_inventory(data)

    def test_unrelated_selected_test_is_rejected(self):
        data = inventory()
        data["rust-suites"]["scryer"]["testcases"]["tests::unrelated"] = {
            "filter-match": {"status": "matches"}, "ignored": False,
        }
        with self.assertRaisesRegex(ValueError, "Unexpected"):
            validate_inventory(data)

    def test_duplicate_test_is_rejected(self):
        data = inventory()
        data["rust-suites"]["duplicate"] = copy.deepcopy(data["rust-suites"]["scryer"])
        with self.assertRaisesRegex(ValueError, "duplicate"):
            validate_inventory(data)

    def test_ignoring_a_regression_is_rejected(self):
        data = inventory()
        test = next(iter(data["rust-suites"]["scryer"]["testcases"].values()))
        test["ignored"] = True
        with self.assertRaisesRegex(ValueError, "ignored status"):
            validate_inventory(data)

    def test_filter_is_scoped_to_windows_tests(self):
        expression = filterset()
        for package, names in WINDOWS_TESTS.items():
            self.assertIn(f"package(={package})", expression)
            for name in names:
                self.assertIn(name, expression)
        self.assertNotIn("all()", expression)

    def test_clippy_is_native_and_strict(self):
        with patch("sys.argv", ["windows_ci.py", "clippy"]), patch("sys.platform", "win32"), patch(
            "windows_ci.subprocess.run"
        ) as run:
            main()
        command = run.call_args.args[0]
        self.assertEqual(command[-3:], ["--", "-D", "warnings"])
        self.assertIn("x86_64-pc-windows-msvc", command)
        self.assertIn("--all-targets", command)
        self.assertIn("--all-features", command)
        self.assertTrue(run.call_args.kwargs["check"])

    def test_non_windows_host_is_rejected(self):
        with patch("sys.argv", ["windows_ci.py", "tests"]), patch("sys.platform", "darwin"):
            with self.assertRaisesRegex(SystemExit, "natively on Windows"):
                main()


if __name__ == "__main__":
    unittest.main()
