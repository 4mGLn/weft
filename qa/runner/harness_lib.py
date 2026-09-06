"""Pure configuration, planning, status, and report helpers for Weft QA."""

from __future__ import annotations

import json
from collections import Counter
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

STATUSES = ("PASS", "FAIL", "BLOCKED", "SKIPPED")


class HarnessConfigError(ValueError):
    """Raised when declarative QA configuration is malformed."""


@dataclass(frozen=True)
class SuitePlan:
    name: str
    config: Mapping[str, Any]


def utc_now() -> datetime:
    return datetime.now(timezone.utc)


def isoformat_utc(value: datetime) -> str:
    return value.astimezone(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def load_config(path: Path) -> dict[str, Any]:
    try:
        config = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise HarnessConfigError(f"cannot load QA config: {error}") from error
    if config.get("schemaVersion") != 1 or not isinstance(config.get("profiles"), dict) or not isinstance(config.get("suites"), dict):
        raise HarnessConfigError("qa.config.json must contain schemaVersion 1, profiles, and suites")
    for name, suite in config["suites"].items():
        if not isinstance(suite, dict) or not isinstance(suite.get("description"), str):
            raise HarnessConfigError(f"suite {name!r} must define a description")
        command = suite.get("command")
        if not isinstance(command, list) or not command or not all(isinstance(item, str) for item in command):
            raise HarnessConfigError(f"suite {name!r} must define a non-empty command array")
        for field in ("requires", "requiresVariables", "requiredCommands", "requiredAnyCommands", "platforms"):
            values = suite.get(field, [])
            if not isinstance(values, list) or not all(isinstance(item, str) for item in values):
                raise HarnessConfigError(f"suite {name!r} {field} must be a string array")
        if suite.get("kind", "deterministic") not in ("deterministic", "observation"):
            raise HarnessConfigError(f"suite {name!r} has an invalid kind")
        timeout = suite.get("timeoutSeconds", 300)
        if not isinstance(timeout, int) or isinstance(timeout, bool) or timeout < 1:
            raise HarnessConfigError(f"suite {name!r} timeoutSeconds must be positive")
        if "optional" in suite and not isinstance(suite["optional"], bool):
            raise HarnessConfigError(f"suite {name!r} optional must be boolean")
        codes = suite.get("skipExitCodes", [])
        if not isinstance(codes, list) or not all(isinstance(code, int) and not isinstance(code, bool) for code in codes):
            raise HarnessConfigError(f"suite {name!r} skipExitCodes must be integers")
    for name, suites in config["profiles"].items():
        if not isinstance(suites, list) or not suites or not all(isinstance(item, str) and item in config["suites"] for item in suites):
            raise HarnessConfigError(f"profile {name!r} must reference known suites")
    return config


def resolve_plan(config: Mapping[str, Any], selected: Sequence[str]) -> list[SuitePlan]:
    suites = config["suites"]
    ordered: list[SuitePlan] = []
    permanent: set[str] = set()
    temporary: set[str] = set()
    def visit(name: str) -> None:
        if name not in suites:
            raise HarnessConfigError(f"unknown suite {name!r}")
        if name in permanent:
            return
        if name in temporary:
            raise HarnessConfigError(f"suite dependency cycle includes {name!r}")
        temporary.add(name)
        for dependency in suites[name].get("requires", []):
            visit(dependency)
        temporary.remove(name)
        permanent.add(name)
        ordered.append(SuitePlan(name, suites[name]))
    for name in selected:
        visit(name)
    return ordered


def render_command(command: Sequence[str], variables: Mapping[str, str]) -> list[str]:
    try:
        return [item.format_map(variables) for item in command]
    except KeyError as error:
        raise HarnessConfigError(f"unknown command variable {error.args[0]!r}") from error


def summarize_results(results: Iterable[Mapping[str, Any]]) -> dict[str, Any]:
    rows = list(results)
    counts = Counter(row.get("status") for row in rows)
    if any(status not in STATUSES for status in counts):
        raise HarnessConfigError("unknown suite status")
    verdict = "FAIL" if counts["FAIL"] else "BLOCKED" if counts["BLOCKED"] else "SKIPPED" if rows and counts["SKIPPED"] == len(rows) else "PASS"
    return {"pass": counts["PASS"], "fail": counts["FAIL"], "blocked": counts["BLOCKED"], "skipped": counts["SKIPPED"], "total": len(rows), "verdict": verdict}


def exit_code_for(summary: Mapping[str, Any]) -> int:
    return 1 if summary["fail"] else 2 if summary["blocked"] or summary["verdict"] == "SKIPPED" else 0


def write_report(report: Mapping[str, Any], root: Path) -> tuple[Path, Path]:
    directory = root / report["runLabel"]
    directory.mkdir(parents=True, exist_ok=True)
    latest = directory / "latest.json"
    history = directory / f"{report['startedAt'].replace(':', '-').replace('.', '-')}.md"
    latest.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    summary = report["summary"]
    lines = [f"# Weft QA Report — {report['runLabel']}", "", f"Result: **{summary['verdict']}**", "", "| Suite | Kind | Status | Duration | Note |", "| --- | --- | --- | ---: | --- |"]
    for row in report["results"]:
        note = str(row.get("note", "")).replace("|", "\\|").replace("\n", "<br>")
        lines.append(f"| {row['name']} | {row.get('kind', 'deterministic')} | {row['status']} | {row.get('durationMs', 0)} ms | {note} |")
    history.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return latest, history
