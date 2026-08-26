#!/usr/bin/env python3
"""Hold every production-critical module to its own coverage floor.

A repo-wide `fail_under` is an average: a thoroughly covered leaf module can
hide a weakly covered control-plane one, and the control plane is exactly where
an untested branch becomes a security or correctness bug. This gate re-checks
the modules that carry auth, data mutation, money, and external I/O
individually.

A module named in the manifest but absent from the coverage report is a
FAILURE, not a skip -- otherwise renaming or deleting a module silently retires
its floor, which is the easiest way for coverage to rot without anyone noticing.

Reads whichever report the stack produces:
  * coverage.py JSON      (`coverage json -o coverage.json`)
  * istanbul JSON summary (`--coverage-reporters=json-summary`)
  * LCOV                  (`.info`; Go via gcov2lcov, Rust via llvm-cov, JS, C++)

    coverage_gate.py --manifest checks/mutations.yaml --report coverage.json
    coverage_gate.py --manifest checks/checks.json --report lcov.info --floor 90
"""
from __future__ import annotations

import argparse
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from manifest import ManifestError, load  # noqa: E402

DEFAULT_FLOOR = 95.0


def _parse_lcov(text: str) -> dict[str, float]:
    """Branch coverage per file, falling back to line coverage where a file has
    no branches at all (a module of straight-line code is not a gap)."""
    percentages: dict[str, float] = {}
    name = None
    found = hit = line_found = line_hit = 0
    for raw in text.splitlines():
        record = raw.strip()
        if record.startswith("SF:"):
            name = record[3:]
            found = hit = line_found = line_hit = 0
        elif record.startswith("BRF:"):
            found = int(record[4:] or 0)
        elif record.startswith("BRH:"):
            hit = int(record[4:] or 0)
        elif record.startswith("LF:"):
            line_found = int(record[3:] or 0)
        elif record.startswith("LH:"):
            line_hit = int(record[3:] or 0)
        elif record == "end_of_record" and name:
            total, covered = (found, hit) if found else (line_found, line_hit)
            percentages[name] = 100.0 * covered / total if total else 100.0
            name = None
    return percentages


def _parse_json(report: dict) -> dict[str, float]:
    """Normalize coverage.py and istanbul JSON into {path: percent}."""
    files = report.get("files")
    if isinstance(files, dict):  # coverage.py
        return {name: entry["summary"]["percent_covered"] for name, entry in files.items()}

    # istanbul json-summary: top level is {"total": {...}, "<path>": {...}}
    percentages: dict[str, float] = {}
    for name, entry in report.items():
        if name == "total" or not isinstance(entry, dict):
            continue
        branches = entry.get("branches", {})
        lines = entry.get("lines", {})
        # A file with zero branches is fully exercised by its line coverage;
        # istanbul reports pct 100 for an empty branch set, so prefer branches
        # only when there is something there to miss.
        source = branches if branches.get("total") else lines
        if "pct" in source:
            percentages[name] = float(source["pct"])
    return percentages


def read_report(path: pathlib.Path) -> dict[str, float]:
    try:
        text = path.read_text()
    except OSError as error:
        raise ManifestError(f"cannot read {path}: {error}") from error
    if path.suffix in (".info", ".lcov"):
        return _parse_lcov(text)
    try:
        return _parse_json(json.loads(text))
    except json.JSONDecodeError as error:
        raise ManifestError(
            f"{path} is neither LCOV (.info/.lcov) nor valid JSON: {error}") from error
    except (KeyError, TypeError) as error:
        raise ManifestError(
            f"{path} is JSON but not a coverage.py or istanbul report: {error}") from error


def resolve(module: str, percentages: dict[str, float]) -> float | None:
    """Match a manifest path against a report key.

    Reports disagree about absolute vs relative and about leading './', so fall
    back to a unique suffix match. An ambiguous suffix is treated as unmatched
    rather than guessed -- silently gating the wrong file is worse than failing.
    """
    if module in percentages:
        return percentages[module]
    wanted = module.lstrip("./")
    exact = [value for key, value in percentages.items() if key.lstrip("./") == wanted]
    if len(exact) == 1:
        return exact[0]
    suffix = [value for key, value in percentages.items()
              if key.replace("\\", "/").endswith("/" + wanted)]
    return suffix[0] if len(suffix) == 1 else None


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--manifest", type=pathlib.Path, required=True)
    parser.add_argument("--report", type=pathlib.Path, required=True,
                        help="coverage.json, coverage-summary.json, or lcov.info")
    parser.add_argument("--floor", type=float, default=None,
                        help=f"percent (default: manifest 'coverage_floor', "
                             f"else {DEFAULT_FLOOR})")
    args = parser.parse_args(argv)

    try:
        data = load(args.manifest)
        percentages = read_report(args.report)
    except ManifestError as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1

    critical = data.get("critical_modules") or []
    if not critical:
        print(f"FAIL: {args.manifest} lists no 'critical_modules'. Name the modules that "
              f"carry auth, data mutation, money, or external I/O -- an empty list is a "
              f"gate that passes by doing nothing", file=sys.stderr)
        return 1

    floor = args.floor if args.floor is not None else float(
        data.get("coverage_floor", DEFAULT_FLOOR))

    missing: list[str] = []
    below: list[tuple[str, float]] = []
    for module in critical:
        percent = resolve(module, percentages)
        if percent is None:
            missing.append(module)
            continue
        if percent < floor:
            below.append((module, percent))
        print(f"{percent:6.1f}%  {module}")

    # CI captures both streams into one log; without this the failure block
    # below lands above the per-module lines it is explaining.
    sys.stdout.flush()

    if missing:
        print(f"\nFAIL: {len(missing)} critical module(s) absent from {args.report} -- "
              f"update 'critical_modules' if this was a rename, do not let the floor "
              f"lapse:", file=sys.stderr)
        for module in missing:
            print(f"  {module}", file=sys.stderr)
        return 1

    if below:
        print(f"\nFAIL: {len(below)} critical module(s) below {floor:.1f}%:",
              file=sys.stderr)
        for module, percent in below:
            print(f"  {percent:6.1f}%  {module}", file=sys.stderr)
        return 1

    print(f"\nPASS: {len(critical)} critical modules at or above {floor:.1f}%")
    return 0


if __name__ == "__main__":  # pragma: no cover - CLI wrapper
    raise SystemExit(main())
