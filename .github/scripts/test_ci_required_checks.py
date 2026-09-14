"""Exercise the actual required-check shell gates without GitHub runners."""

from pathlib import Path
import re
import subprocess
import unittest


WORKFLOWS = Path(__file__).resolve().parents[1] / "workflows"


def job(workflow, name):
    source = (WORKFLOWS / workflow).read_text()
    match = re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [\w-]+:\n|\Z)", source, re.M | re.S)
    if match is None:
        raise AssertionError(f"Missing job {name}")
    return match.group(1)


def first_script(block):
    match = re.search(r"^        run: \|\n((?:          .*\n|\n)+)", block, re.M)
    if match is None:
        raise AssertionError("Missing gate script")
    return "\n".join(line[10:] for line in match.group(1).splitlines())


def succeeds(script, **env):
    return subprocess.run(["bash", "-e", "-c", script], env=env,
                          capture_output=True, text=True).returncode == 0


class RequiredCheckTests(unittest.TestCase):
    def test_build_gates_pass_only_success_or_intentional_skip(self):
        for name in ["web", "winget-xtask-build", "rust-clippy", "launcher-build",
                     "linux-build", "macos-build", "windows-build", "windows-validation"]:
            block = job("scryer.yml", name)
            self.assertIn("    if: always()", block)
            script = first_script(block)
            for required, result, expected in [
                ("false", "skipped", True), ("true", "success", True),
                ("true", "skipped", False), ("true", "failure", False),
                ("true", "cancelled", False), ("false", "failure", False),
                ("", "skipped", False),
            ]:
                with self.subTest(job=name, required=required, result=result):
                    self.assertEqual(succeeds(script, CLASSIFICATION="success",
                                              REQUIRED=required, RESULT=result), expected)
            self.assertFalse(succeeds(script, CLASSIFICATION="failure",
                                      REQUIRED="false", RESULT="skipped"))

    def test_required_names_are_preserved(self):
        names = {
            "web": "web", "winget-xtask-build": "winget-xtask-build",
            "rust-clippy": "rust-clippy", "rust-nextest": "rust-nextest",
            "launcher-build": "launcher-${{ matrix.arch }}",
            "linux-build": "linux-${{ matrix.arch }}-${{ matrix.variant }}",
            "macos-build": "macos-${{ matrix.arch }}-${{ matrix.variant }}",
            "windows-build": "windows-${{ matrix.arch }}",
        }
        for key, name in names.items():
            self.assertIn(f"    name: {name}\n", job("scryer.yml", key))
        self.assertIn("    name: CodeQL (${{ matrix.language }})\n", job("codeql.yml", "analyze"))

    def test_nextest_gate_still_handles_docs_and_web_only(self):
        script = first_script(job("scryer.yml", "rust-nextest"))
        for full, matrix, targets, expected in [
            ("false", "skipped", "skipped", True),
            ("true", "success", "success", True),
            ("true", "success", "skipped", False),
            ("true", "failure", "skipped", False),
            ("true", "success", "cancelled", False),
        ]:
            self.assertEqual(succeeds(script, CHANGES_RESULT="success", FULL=full,
                                      MATRIX_RESULT=matrix, TARGET_RESULT=targets), expected)

    def test_codeql_noop_is_success_but_missing_classification_fails(self):
        script = first_script(job("codeql.yml", "analyze"))
        for classification, enabled, expected in [
            ("success", "true", True), ("success", "false", True),
            ("success", "", False), ("failure", "false", False),
            ("cancelled", "false", False),
        ]:
            self.assertEqual(succeeds(script, CLASSIFICATION=classification,
                                      ENABLED=enabled), expected)


if __name__ == "__main__":
    unittest.main()
