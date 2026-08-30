#!/usr/bin/env python3
"""Require mutation shard reports to cover every declared invariant exactly once."""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import shlex
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from manifest import ManifestError, load, mutations  # noqa: E402


def declared_command(
    entry: dict, configured: dict[str, str], baseline: list[str]
) -> list[str]:
    if entry.get("test_command"):
        return shlex.split(entry["test_command"])
    matches = [
        (prefix, command) for prefix, command in configured.items()
        if entry["file"].startswith(prefix)
    ]
    return shlex.split(max(matches, key=lambda item: len(item[0]))[1]) if matches else baseline


def declared_compile_command(
    entry: dict, configured: dict[str, str]
) -> list[str] | None:
    if entry.get("compile_command"):
        return shlex.split(entry["compile_command"])
    matches = [
        (prefix, command) for prefix, command in configured.items()
        if entry["file"].startswith(prefix)
    ]
    return shlex.split(max(matches, key=lambda item: len(item[0]))[1]) if matches else None


def verify_reports(
    entries: list[dict],
    reports: list[dict],
    expected_manifest_sha256: str | None = None,
    expected_commit_sha: str | None = None,
    expected_baseline_command: list[str] | None = None,
    expected_commands: dict[str, list[str]] | None = None,
    expected_compile_commands: dict[str, list[str] | None] | None = None,
) -> list[str]:
    expected = {entry["id"] for entry in entries}
    observed: dict[str, list[str]] = {}
    selected: dict[str, int] = {}
    errors = []
    valid_reports = []
    for report in reports:
        if not isinstance(report, dict):
            errors.append("every report must be a JSON object")
            continue
        report_selected = report.get("selected_ids", [])
        report_results = report.get("results", [])
        if not isinstance(report_selected, list) or not all(
            isinstance(item, str) for item in report_selected
        ):
            errors.append("every report selected_ids field must be a list of strings")
            continue
        if not isinstance(report_results, list) or not all(
            isinstance(item, dict) for item in report_results
        ):
            errors.append("every report results field must be a list of objects")
            continue
        valid_reports.append(report)
        selected_set = {
            mutation_id for mutation_id in report_selected if isinstance(mutation_id, str)
        }
        result_ids = [
            result.get("id") for result in report_results
            if isinstance(result.get("id"), str)
        ]
        if len(selected_set) != len(report_selected) or set(result_ids) != selected_set \
                or len(result_ids) != len(set(result_ids)):
            errors.append("a shard's completed results do not exactly match its selection")
        for mutation_id in report.get("selected_ids", []):
            if isinstance(mutation_id, str):
                selected[mutation_id] = selected.get(mutation_id, 0) + 1
        for result in report.get("results", []):
            mutation_id = result.get("id")
            status = result.get("status")
            if isinstance(mutation_id, str) and isinstance(status, str):
                observed.setdefault(mutation_id, []).append(status)

    manifest_hashes = {report.get("manifest_sha256") for report in valid_reports}
    commit_shas = {report.get("commit_sha") for report in valid_reports}
    if expected_manifest_sha256 is not None and manifest_hashes != {expected_manifest_sha256}:
        errors.append("reports do not all match the current manifest hash")
    if expected_commit_sha is not None and commit_shas != {expected_commit_sha}:
        errors.append("reports do not all match the checked-out commit")
    elif len(commit_shas) != 1 or None in commit_shas or "unknown" in commit_shas:
        errors.append("reports do not all come from one known commit")
    if any(report.get("baseline") != "passed" for report in valid_reports):
        errors.append("every shard report must record a passed baseline")
    if any(report.get("schema_version") != 1 for report in valid_reports):
        errors.append("every shard report must use schema version 1")
    if expected_baseline_command is not None and any(
        report.get("baseline_command") != expected_baseline_command for report in valid_reports
    ):
        errors.append("reports do not use the manifest's baseline command")
    if expected_commands is not None:
        for report in valid_reports:
            for result in report.get("results", []):
                mutation_id = result.get("id")
                if mutation_id in expected_commands and result.get("command") != expected_commands[mutation_id]:
                    errors.append(f"{mutation_id} did not use its manifest-declared command")
    if expected_compile_commands is not None:
        for report in valid_reports:
            for result in report.get("results", []):
                mutation_id = result.get("id")
                if mutation_id in expected_compile_commands and result.get(
                    "compile_command"
                ) != expected_compile_commands[mutation_id]:
                    errors.append(
                        f"{mutation_id} did not use its manifest-declared compile command"
                    )
    selected_missing = sorted(expected - selected.keys())
    selected_unknown = sorted(selected.keys() - expected)
    selected_duplicates = sorted(key for key, count in selected.items() if count != 1)
    if selected_missing:
        errors.append("shard selections omit mutation(s): " + ", ".join(selected_missing))
    if selected_unknown:
        errors.append("shard selections contain unknown mutation(s): " + ", ".join(selected_unknown))
    if selected_duplicates:
        errors.append("shard selections overlap on mutation(s): " + ", ".join(selected_duplicates))
    missing = sorted(expected - observed.keys())
    unknown = sorted(observed.keys() - expected)
    duplicates = sorted(key for key, values in observed.items() if len(values) != 1)
    if missing:
        errors.append("missing mutation result(s): " + ", ".join(missing))
    if unknown:
        errors.append("unknown mutation result(s): " + ", ".join(unknown))
    if duplicates:
        errors.append("duplicate mutation result(s): " + ", ".join(duplicates))
    for mutation_id in sorted(expected & observed.keys()):
        if len(observed[mutation_id]) == 1 and observed[mutation_id][0] != "killed":
            errors.append(f"{mutation_id} reported {observed[mutation_id][0]}, not killed")
    if set(observed) != set(selected):
        errors.append("completed results do not exactly match the declared shard selections")
    return errors


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=pathlib.Path, required=True)
    parser.add_argument("reports", nargs="+", type=pathlib.Path)
    args = parser.parse_args(argv)
    try:
        data = load(args.manifest)
        entries = mutations(data, args.manifest)
        reports = [json.loads(path.read_text()) for path in args.reports]
    except (ManifestError, OSError, json.JSONDecodeError) as error:
        print(f"REPORT ERROR: {error}", file=sys.stderr)
        return 1
    manifest_hash = hashlib.sha256(args.manifest.read_bytes()).hexdigest()
    root = args.manifest.resolve().parent
    while root != root.parent and not (root / ".git").exists():
        root = root.parent
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=root, check=False, capture_output=True, text=True
    )
    if commit.returncode != 0:
        print("REPORT ERROR: could not resolve checked-out commit", file=sys.stderr)
        return 1
    baseline_command = shlex.split(data.get("test_command", "python -m pytest -q -x"))
    configured = data.get("mutation_test_commands", {})
    configured_compile = data.get("mutation_compile_commands", {})
    expected_commands = {}
    expected_compile_commands = {}
    for entry in entries:
        expected_commands[entry["id"]] = declared_command(
            entry, configured, baseline_command
        )
        expected_compile_commands[entry["id"]] = declared_compile_command(
            entry, configured_compile
        )
    errors = verify_reports(
        entries,
        reports,
        manifest_hash,
        commit.stdout.strip(),
        baseline_command,
        expected_commands,
        expected_compile_commands,
    )
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print(f"PASS: {len(entries)} mutation results present exactly once and all killed")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
