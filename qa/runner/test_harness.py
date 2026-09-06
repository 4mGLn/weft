import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

import harness
from harness_lib import HarnessConfigError


class HarnessLifecycleTests(unittest.TestCase):
    def test_required_option_blocks_before_execution(self):
        with self.assertRaises(HarnessConfigError):
            harness.command_for({"command": ["make"], "requiresVariables": ["version"]}, {"version": ""})

    @patch("harness.shutil.which", return_value=None)
    def test_optional_missing_capability_is_skipped(self, _which):
        result = harness._preflight({"optional": True, "requiredCommands": ["but"]}, {})
        self.assertEqual(result, ("SKIPPED", "missing required commands: but"))

    @patch("harness.shutil.which", return_value=None)
    def test_missing_alternative_capability_blocks(self, _which):
        result = harness._preflight({"requiredAnyCommands": ["docker", "podman"]}, {})
        self.assertEqual(result, ("BLOCKED", "no supported command available: docker, podman"))

    @unittest.skipUnless(os.name == "posix", "process-group cleanup is POSIX-specific")
    def test_timeout_terminates_process_group(self):
        with tempfile.TemporaryDirectory() as directory:
            child_pid = Path(directory) / "child.pid"
            command = [
                sys.executable,
                "-c",
                "import pathlib,subprocess,sys,time; "
                "child=subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)']); "
                f"pathlib.Path({str(child_pid)!r}).write_text(str(child.pid)); time.sleep(60)",
            ]
            result = harness.run_suite("timeout", {"description": "", "command": command, "timeoutSeconds": 1}, {})
            self.assertEqual((result["status"], result["exitCode"]), ("FAIL", 124))
            pid = int(child_pid.read_text(encoding="utf-8"))
            for _ in range(20):
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.05)
            else:
                self.fail("timeout left a descendant process running")


if __name__ == "__main__":
    unittest.main()
