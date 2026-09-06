#!/usr/bin/env python3
"""Weft's additive, declarative local QA runner."""

from __future__ import annotations

import argparse
import datetime as dt
import os
import platform
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Mapping

from harness_lib import HarnessConfigError, exit_code_for, load_config as load_validated_config, render_command, resolve_plan, summarize_results, write_report as write_structured_report

ROOT = Path(__file__).resolve().parents[2]
CONFIG = ROOT / "qa" / "qa.config.json"
OUTPUT_LIMIT = 24_000


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run declarative Weft QA profiles.")
    parser.add_argument("action", choices=("run", "list", "self-test"), nargs="?", default="run")
    parser.add_argument("--profile", default="smoke")
    parser.add_argument("--suite", action="append", default=[])
    parser.add_argument("--version", default="")
    parser.add_argument("--keep-going", action="store_true")
    parser.add_argument("--report-dir", default="qa/reports")
    return parser.parse_args()


def load_config() -> dict:
    return load_validated_config(CONFIG)


def command_for(suite: Mapping[str, Any], variables: Mapping[str, str]) -> list[str]:
    missing = [name for name in suite.get("requiresVariables", []) if not variables.get(name)]
    if missing:
        raise HarnessConfigError(f"missing required options: {', '.join('--' + name for name in missing)}")
    return render_command(suite["command"], variables)


def _terminate(process: subprocess.Popen[str]) -> None:
    if process.poll() is not None:
        return
    try:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGTERM)
        else:
            process.terminate()
        process.wait(timeout=5)
    except (ProcessLookupError, subprocess.TimeoutExpired):
        if process.poll() is None:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()


def _preflight(suite: Mapping[str, Any], variables: Mapping[str, str]) -> tuple[str, str] | None:
    if suite.get("platforms") and platform.system().lower() not in suite["platforms"]:
        return "SKIPPED", f"platform {platform.system().lower()} is not applicable"
    required = [render_command([item], variables)[0] for item in suite.get("requiredCommands", [])]
    missing = [item for item in required if shutil.which(item) is None]
    if missing:
        return ("SKIPPED" if suite.get("optional") else "BLOCKED", f"missing required commands: {', '.join(missing)}")
    alternatives = [render_command([item], variables)[0] for item in suite.get("requiredAnyCommands", [])]
    if alternatives and not any(shutil.which(item) for item in alternatives):
        return ("SKIPPED" if suite.get("optional") else "BLOCKED", f"no supported command available: {', '.join(alternatives)}")
    return None


def run_suite(name: str, suite: Mapping[str, Any], variables: Mapping[str, str]) -> dict:
    started = time.monotonic()
    try:
        command = command_for(suite, variables)
    except HarnessConfigError as error:
        return {"name": name, "kind": suite.get("kind", "deterministic"), "status": "BLOCKED", "satisfiesDependencies": False, "note": str(error), "durationMs": 0}
    preflight = _preflight(suite, variables)
    if preflight:
        return {"name": name, "kind": suite.get("kind", "deterministic"), "status": preflight[0], "satisfiesDependencies": False, "note": preflight[1], "durationMs": 0}
    try:
        process = subprocess.Popen(command, cwd=ROOT, text=True, errors="replace", stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, start_new_session=(os.name == "posix"))
        output, _ = process.communicate(timeout=suite.get("timeoutSeconds", 300))
        if process.returncode == 0:
            status, note = "PASS", ""
        elif process.returncode in suite.get("skipExitCodes", []):
            status, note = "SKIPPED", f"unavailable coverage (exit {process.returncode})"
        else:
            status, note = "FAIL", f"command exited with {process.returncode}"
        exit_code = process.returncode
    except OSError as error:
        return {"name": name, "kind": suite.get("kind", "deterministic"), "status": "BLOCKED", "satisfiesDependencies": False, "note": f"cannot start command: {error}", "durationMs": round((time.monotonic() - started) * 1000)}
    except subprocess.TimeoutExpired:
        _terminate(process)
        output, _ = process.communicate()
        status, note, exit_code = "FAIL", f"timed out after {suite.get('timeoutSeconds', 300)} seconds", 124
    except KeyboardInterrupt:
        _terminate(process)
        output, _ = process.communicate()
        status, note, exit_code = "BLOCKED", "interrupted by user", 143
    return {"name": name, "kind": suite.get("kind", "deterministic"), "status": status,
            "satisfiesDependencies": status == "PASS", "note": note, "command": command,
            "exitCode": exit_code, "durationMs": round((time.monotonic() - started) * 1000), "outputTail": output[-OUTPUT_LIMIT:]}


def self_test() -> int:
    return subprocess.run(
        [sys.executable, "-m", "unittest", "discover", "-s", "qa/runner", "-p", "test_*.py", "-v"],
        cwd=ROOT,
        check=False,
    ).returncode


def main() -> int:
    args = parse_args()
    try:
        config = load_config()
        if args.action == "self-test":
            return self_test()
        if args.action == "list":
            for name, suites in config["profiles"].items():
                print(f"{name}: {', '.join(suites)}")
            return 0
        selected = args.suite or config["profiles"].get(args.profile, [])
        if not selected:
            raise HarnessConfigError(f"unknown or empty QA profile: {args.profile}")
        plan = resolve_plan(config, selected)
        variables = {"version": args.version, "release_version": args.version.removeprefix("v")}
        results = []
        satisfied: dict[str, bool] = {}
        stop_reason = ""
        for item in plan:
            missing = [name for name in item.config.get("requires", []) if not satisfied.get(name, False)]
            if missing:
                result = {"name": item.name, "kind": item.config.get("kind", "deterministic"), "status": "SKIPPED", "satisfiesDependencies": False, "note": f"dependency did not satisfy prerequisites: {', '.join(missing)}", "durationMs": 0}
            elif stop_reason:
                result = {"name": item.name, "kind": item.config.get("kind", "deterministic"), "status": "SKIPPED", "satisfiesDependencies": False, "note": stop_reason, "durationMs": 0}
            else:
                result = run_suite(item.name, item.config, variables)
            results.append(result)
            satisfied[item.name] = bool(result["satisfiesDependencies"])
            print(f"{item.name}: {result['status']} ({result['note']})")
            if result["status"] in ("FAIL", "BLOCKED") and not args.keep_going:
                stop_reason = f"stopped after {item.name} {result['status'].lower()}"
        summary = summarize_results(results)
        report = {"schemaVersion": 1, "runLabel": "custom" if args.suite else args.profile,
                  "startedAt": dt.datetime.now(dt.UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z"),
                  "selectedSuites": selected, "executionPlan": [item.name for item in plan],
                  "summary": summary, "results": results}
        write_structured_report(report, ROOT / args.report_dir)
        return exit_code_for(summary)
    except HarnessConfigError as error:
        print(f"HARNESS ERROR: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
