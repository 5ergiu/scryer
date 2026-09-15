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
    def test_builds_and_validation_remain_independent(self):
        def ancestors(name):
            found = set()
            pending = [name]
            while pending:
                block = job("scryer.yml", pending.pop())
                match = re.search(r"^    needs: (.+)$", block, re.M)
                dependencies = match.group(1).strip("[]").split(",") if match else []
                for dependency in dependencies:
                    dependency = dependency.strip()
                    if dependency not in found:
                        found.add(dependency)
                        pending.append(dependency)
            return found

        builds = {"launcher-build", "linux-build", "macos-build", "windows-build",
                  "winget-xtask-build"}
        validations = {"rust-clippy", "rust-nextest", "windows-validation"}
        for build in builds:
            self.assertFalse(ancestors(build) & {
                *validations, "rust-clippy-work", "rust-nextest-target", "windows-validation-work",
            })
        for validation in validations:
            self.assertFalse(ancestors(validation) & (builds | (validations - {validation})))

    def test_existing_jobs_short_circuit_without_wrappers(self):
        source = (WORKFLOWS / "scryer.yml").read_text()
        self.assertNotRegex(source, r"(?m)^  [\w-]+-work:")
        for name in ["web", "winget-xtask-build", "rust-clippy", "launcher-build",
                     "linux-build", "macos-build", "windows-build", "windows-validation"]:
            block = job("scryer.yml", name)
            self.assertIn("    if: always()", block)
            script = first_script(block)
            for required, ready, expected in [
                ("false", "false", True), ("false", "true", True),
                ("true", "true", True), ("true", "false", False),
                ("true", "", False), ("", "false", False),
            ]:
                with self.subTest(job=name, required=required, ready=ready):
                    self.assertEqual(succeeds(script, CLASSIFICATION="success",
                                              REQUIRED=required, READY=ready,
                                              GITHUB_OUTPUT="/dev/null"), expected)
            self.assertFalse(succeeds(script, CLASSIFICATION="failure",
                                      REQUIRED="false", READY="false", GITHUB_OUTPUT="/dev/null"))
            steps = re.split(r"(?m)^      - ", block)[1:]
            self.assertIn("id: scope", steps[0])
            for step in steps[1:]:
                condition = re.search(r"(?m)^        if: (.+)$", step)
                self.assertIsNotNone(condition, (name, step.splitlines()[0]))
                self.assertIn("steps.scope.outputs.run == 'true'", condition[1])

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
