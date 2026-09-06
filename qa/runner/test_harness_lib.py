import json
import tempfile
import unittest
from pathlib import Path

from harness_lib import HarnessConfigError, exit_code_for, load_config, render_command, resolve_plan, summarize_results


class HarnessLibraryTests(unittest.TestCase):
    def test_config_rejects_invalid_timeout_and_unknown_profile_suite(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "qa.config.json"
            path.write_text(json.dumps({"schemaVersion": 1, "profiles": {"smoke": ["missing"]}, "suites": {"suite": {"description": "x", "command": ["true"], "timeoutSeconds": 0}}}), encoding="utf-8")
            with self.assertRaises(HarnessConfigError):
                load_config(path)

    def test_dependencies_resolve_once_and_cycles_fail(self):
        config = {"suites": {"a": {"requires": ["b"]}, "b": {"requires": ["c"]}, "c": {}}}
        self.assertEqual([item.name for item in resolve_plan(config, ["a", "b"])], ["c", "b", "a"])
        config["suites"]["c"]["requires"] = ["a"]
        with self.assertRaises(HarnessConfigError):
            resolve_plan(config, ["a"])

    def test_rendering_and_status_exit_contract(self):
        self.assertEqual(render_command(["make", "VERSION={version}"], {"version": "v0.2.1"}), ["make", "VERSION=v0.2.1"])
        with self.assertRaises(HarnessConfigError):
            render_command(["{missing}"], {})
        self.assertEqual(exit_code_for(summarize_results([{"status": "PASS"}, {"status": "SKIPPED"}])), 0)
        self.assertEqual(exit_code_for(summarize_results([{"status": "BLOCKED"}])), 2)
        self.assertEqual(exit_code_for(summarize_results([{"status": "FAIL"}])), 1)


if __name__ == "__main__":
    unittest.main()
