#!/usr/bin/env python3
"""Prove the test suite actually guards the invariants it claims to guard.

A green suite is not evidence. An agent that writes both the code and the tests
can produce a suite that passes no matter what the code does. This closes that
hole: for each entry in the manifest, deliberately break one invariant in place,
run the suite, and require it to go RED. A mutation the suite does not catch
(SURVIVED) means that invariant is unguarded -- the test is theatre.

The mutated file is always restored, including on failure, and the restore is
verified byte-for-byte before the process exits.

Language-agnostic: the mutation is a textual find/replace and the suite is
whatever `--test-cmd` names.

    mutation_guard.py --manifest checks/mutations.yaml --assert-min 12
    mutation_guard.py --manifest checks/mutations.json --test-cmd "npm test"
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import pathlib
import re
import shlex
import signal
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from manifest import ManifestError, load, mutations  # noqa: E402

# A mutation that removes a guard rail can leave a test waiting on something
# that will now never happen. Without a bound the suite hangs, the whole job is
# killed by the CI timeout, and the log shows no verdict at all -- so cap each
# run well above the honest suite runtime and report the hang as its own class.
DEFAULT_TIMEOUT_SECONDS = 120.0
DEFAULT_TEST_COMMAND = "python -m pytest -q -x"
PROCESS_GROUP_GRACE_SECONDS = 1.0


ANSI = re.compile(r"\x1b\[[0-9;]*[a-zA-Z]")


class ProcessCleanupError(RuntimeError):
    """The mutation suite's process tree could not be proven terminated."""


def _process_group_members(group_id: int) -> set[int]:
    """Return live, non-zombie members of one POSIX process group."""
    try:
        result = subprocess.run(
            ["ps", "-axo", "pid=,pgid=,stat="],
            check=False,
            capture_output=True,
            text=True,
            timeout=PROCESS_GROUP_GRACE_SECONDS,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ProcessCleanupError("could not inspect mutation process group") from error
    if result.returncode != 0:
        raise ProcessCleanupError(
            f"process-group inspection failed with exit {result.returncode}"
        )
    members = set()
    for line in result.stdout.splitlines():
        fields = line.split()
        if len(fields) < 3:
            if fields:
                raise ProcessCleanupError("process-group inspection was malformed")
            continue
        pid, pgid, state = fields[:3]
        try:
            parsed_pid = int(pid)
            parsed_pgid = int(pgid)
        except ValueError as error:
            raise ProcessCleanupError("process-group inspection was malformed") from error
        if parsed_pgid == group_id and not state.startswith("Z"):
            members.add(parsed_pid)
    return members


def _signal_process_group(group_id: int, sig: signal.Signals) -> None:
    try:
        os.killpg(group_id, sig)
        return
    except ProcessLookupError:
        return
    except PermissionError:
        members = _process_group_members(group_id)
    for pid in members:
        try:
            if os.getpgid(pid) == group_id:
                os.kill(pid, sig)
        except (ProcessLookupError, PermissionError):
            pass


def _stop_process_group(process: subprocess.Popen) -> None:
    """Terminate a suite and its descendants before restoring mutated source."""
    group_id = process.pid
    _signal_process_group(group_id, signal.SIGTERM)
    try:
        process.communicate(timeout=PROCESS_GROUP_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        pass
    _signal_process_group(group_id, signal.SIGKILL)
    try:
        process.communicate(timeout=PROCESS_GROUP_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        for stream in (process.stdout, process.stderr):
            if stream is not None:
                stream.close()
    try:
        process.wait(timeout=PROCESS_GROUP_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=PROCESS_GROUP_GRACE_SECONDS)

    deadline = time.monotonic() + PROCESS_GROUP_GRACE_SECONDS
    while _process_group_members(group_id):
        if time.monotonic() >= deadline:
            raise ProcessCleanupError("mutation process group survived SIGKILL")
        time.sleep(0.01)


def _run(command: list[str], root: pathlib.Path, timeout: float):
    with tempfile.TemporaryDirectory(prefix="liminal-mutation-pycache-") as cache:
        environment = os.environ.copy()
        environment["PYTHONPYCACHEPREFIX"] = cache
        process = subprocess.Popen(
            command,
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=environment,
            start_new_session=True,
        )
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except BaseException:
            _stop_process_group(process)
            raise
        if _process_group_members(process.pid):
            _stop_process_group(process)
        return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)


def _tail(output: str, lines: int = 15, width: int = 200) -> str:
    """Readable tail of a suite's output.

    Test runners emit colour codes and progress lines that are megabytes of
    escape sequences on one line. Unfiltered, they bury the actual failure the
    operator needs to read.
    """
    kept = []
    for raw in output.strip().splitlines():
        line = ANSI.sub("", raw).rstrip()
        if not line.strip() or not line.strip(" ·.-_✓✔✗✕×"):
            continue
        kept.append(line if len(line) <= width else line[:width] + " ...")
    return "\n".join(f"    {line}" for line in kept[-lines:]) or "    (no output)"


def apply_mutation(
    entry: dict,
    root: pathlib.Path,
    command: list[str],
    timeout: float,
    compile_command: list[str] | None = None,
    compile_timeout: float | None = None,
    fallback_command: list[str] | None = None,
    fallback_timeout: float | None = None,
) -> tuple[str, str]:
    """Apply one mutation, run the suite, restore the file.

    Returns (status, detail) where status is one of:
      "killed"   -- find matched once, mutation applied, suite went red (good).
      "survived" -- find matched once, mutation applied, suite stayed green
                    (the invariant is unguarded by any test).
      "timeout"  -- the suite neither passed nor failed within the timeout. A
                    test is waiting on something the mutation prevents. That is
                    a non-deterministic suite defect, not a kill: bound the wait
                    so the test fails fast.
      "invalid"  -- the mutation does not compile. Compiler failures are not
                    accepted as behavioral quality evidence.
      "stale"    -- find did not match exactly once. The manifest text no longer
                    matches the source (usually because a legitimate change
                    edited the surrounding lines). That is a manifest
                    maintenance problem, not evidence the invariant is
                    unguarded, and must never be conflated with "survived".
    """
    target = root / entry["file"]
    if not target.is_file():
        return "stale", (
            f"{entry['file']} does not exist -- the manifest points at a file that "
            f"was moved or deleted; update or remove this entry")

    original = target.read_text()
    count = original.count(entry["find"])
    if count != 1:
        return "stale", (
            f"'find' matched {count} times in {entry['file']} (need exactly 1) -- "
            f"manifest entry no longer matches the current source; update find/replace")

    mutated = original.replace(entry["find"], entry["replace"], 1)
    try:
        target.write_text(mutated)
        if compile_command:
            compile_limit = compile_timeout or timeout
            try:
                compile_result = _run(compile_command, root, compile_limit)
            except subprocess.TimeoutExpired:
                return "invalid", (
                    f"mutant compile check did not finish within {compile_limit:.0f}s -- INVALID"
                )
            if compile_result.returncode != 0:
                return "invalid", (
                    f"mutant did not compile (exit {compile_result.returncode}) -- INVALID\n"
                    + _tail(compile_result.stdout or compile_result.stderr or "")
                )
        try:
            result = _run(command, root, timeout)
        except subprocess.TimeoutExpired:
            return "timeout", (
                f"suite did not finish within {timeout:.0f}s -- a test is blocking on a "
                f"condition this mutation prevents; give that wait a bound so it fails "
                f"fast instead of hanging")
        if result.returncode == 0 and fallback_command and fallback_command != command:
            fallback_limit = fallback_timeout or timeout
            try:
                fallback = _run(fallback_command, root, fallback_limit)
            except subprocess.TimeoutExpired:
                return "timeout", (
                    f"full fallback suite did not finish within {fallback_limit:.0f}s after the "
                    "scoped suite stayed green")
            if fallback.returncode != 0:
                return "killed", (
                    "scoped suite stayed green; full workspace fallback exited "
                    f"{fallback.returncode} -- KILLED")
            return "survived", (
                "scoped suite and full workspace fallback both exited 0 -- SURVIVED")
        if result.returncode == 0:
            return "survived", "suite exited 0 (stayed green) -- SURVIVED"
        return "killed", f"suite exited {result.returncode} -- KILLED"
    finally:
        target.write_text(original)
        if target.read_text() != original:
            raise ManifestError(
                f"failed to restore {entry['file']} to its original content after "
                f"mutation {entry['id']!r}; on-disk content does not match the "
                f"pre-mutation snapshot")


def verify_baseline(
    root: pathlib.Path,
    command: list[str],
    timeout: float,
    *,
    require_one_test: bool = False,
) -> str | None:
    """Require the unmutated suite to be green.

    Without this, a suite that is already red 'kills' every mutation and the
    whole gate reports PASS while proving nothing. This is the single cheapest
    way to stop the guard from being gamed.
    """
    try:
        result = _run(command, root, timeout)
    except subprocess.TimeoutExpired:
        return f"the unmutated suite did not finish within {timeout:.0f}s"
    if result.returncode != 0:
        return ("the unmutated suite is already failing (exit "
                f"{result.returncode}); every mutation would 'kill' trivially and the "
                "gate would prove nothing. Fix the suite first.\n"
                + _tail(result.stdout or result.stderr or ""))
    if require_one_test and not re.search(
        r"test result: ok\. 1 passed; 0 failed;", result.stdout or ""
    ):
        return "the exact scoped command did not execute exactly one passing test"
    return None


def mutation_command(
    entry: dict,
    data: dict,
    default: list[str],
    *,
    allow_scoped: bool = True,
) -> list[str]:
    """Select the narrowest declared test command that owns the mutated file."""
    if not allow_scoped:
        return default
    if entry.get("test_command"):
        return shlex.split(entry["test_command"])
    matches = [
        (prefix, command)
        for prefix, command in data.get("mutation_test_commands", {}).items()
        if entry["file"].startswith(prefix)
    ]
    if not matches:
        return default
    _prefix, command = max(matches, key=lambda item: len(item[0]))
    return shlex.split(command)


def mutation_timeout(entry: dict, data: dict, default: float) -> float:
    """Select the narrowest declared timeout that owns the mutated file."""
    if "timeout_seconds" in entry:
        timeout = entry["timeout_seconds"]
        if isinstance(timeout, bool) or not isinstance(timeout, (int, float)):
            raise ManifestError("mutation timeouts must be numbers")
        timeout = float(timeout)
        if not math.isfinite(timeout) or timeout <= 0:
            raise ManifestError("mutation timeouts must be positive and finite")
        return timeout
    matches = [
        (prefix, timeout)
        for prefix, timeout in data.get("mutation_timeout_seconds", {}).items()
        if entry["file"].startswith(prefix)
    ]
    timeout = default if not matches else max(matches, key=lambda item: len(item[0]))[1]
    if isinstance(timeout, bool) or not isinstance(timeout, (int, float)):
        raise ManifestError("mutation timeouts must be numbers")
    timeout = float(timeout)
    if not math.isfinite(timeout) or timeout <= 0:
        raise ManifestError("mutation timeouts must be positive and finite")
    return timeout


def mutation_compile_command(entry: dict, data: dict) -> list[str] | None:
    """Select the narrowest compile-only command for the mutated source."""
    if entry.get("compile_command"):
        return shlex.split(entry["compile_command"])
    matches = [
        (prefix, command)
        for prefix, command in data.get("mutation_compile_commands", {}).items()
        if entry["file"].startswith(prefix)
    ]
    if not matches:
        return None
    _prefix, command = max(matches, key=lambda item: len(item[0]))
    return shlex.split(command)


def mutation_compile_timeout(entry: dict, data: dict, default: float) -> float:
    """Select the narrowest compile-only timeout for the mutated source."""
    if "compile_timeout_seconds" in entry:
        timeout = entry["compile_timeout_seconds"]
    else:
        matches = [
            (prefix, timeout)
            for prefix, timeout in data.get("mutation_compile_timeout_seconds", {}).items()
            if entry["file"].startswith(prefix)
        ]
        timeout = default if not matches else max(matches, key=lambda item: len(item[0]))[1]
    if isinstance(timeout, bool) or not isinstance(timeout, (int, float)):
        raise ManifestError("mutation compile timeouts must be numbers")
    timeout = float(timeout)
    if not math.isfinite(timeout) or timeout <= 0:
        raise ManifestError("mutation compile timeouts must be positive and finite")
    return timeout


def select_entries(entries: list[dict], shard_index: int, shard_count: int) -> list[dict]:
    """Return one deterministic, balanced, one-based shard of manifest entries."""
    if shard_count < 1 or shard_index < 1 or shard_index > shard_count:
        raise ManifestError("shard must satisfy 1 <= index <= count")
    return [
        entry for position, entry in enumerate(sorted(entries, key=lambda item: item["id"]))
        if position % shard_count == shard_index - 1
    ]


def _parse_shard(value: str) -> tuple[int, int]:
    try:
        index, count = (int(part) for part in value.split("/", 1))
    except (TypeError, ValueError) as error:
        raise argparse.ArgumentTypeError("shard must be INDEX/COUNT") from error
    if count < 1 or index < 1 or index > count:
        raise argparse.ArgumentTypeError("shard must satisfy 1 <= INDEX <= COUNT")
    return index, count


def _write_report(
    path: pathlib.Path | None,
    selected: list[dict],
    rows: list[dict],
    shard: tuple[int, int] | None,
    manifest_sha256: str,
    commit_sha: str,
    baseline: str,
    baseline_command: list[str],
    scoped_baseline_commands: list[list[str]],
) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps({
        "schema_version": 1,
        "commit_sha": commit_sha,
        "manifest_sha256": manifest_sha256,
        "baseline": baseline,
        "baseline_command": baseline_command,
        "scoped_baseline_commands": scoped_baseline_commands,
        "selected_ids": [entry["id"] for entry in selected],
        "shard": ({"index": shard[0], "count": shard[1]} if shard else None),
        "results": rows,
    }, indent=2) + "\n")
    temporary.replace(path)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--manifest", type=pathlib.Path, required=True)
    parser.add_argument("--root", type=pathlib.Path, default=None,
                        help="repo root the suite runs in (default: manifest's repo)")
    parser.add_argument("--test-cmd", default=None,
                        help=f"suite command (default: manifest 'test_command', "
                             f"else {DEFAULT_TEST_COMMAND!r})")
    parser.add_argument("--timeout", type=float, default=None,
                        help=f"per-run seconds (default: manifest 'timeout_seconds', "
                             f"else {DEFAULT_TIMEOUT_SECONDS:.0f})")
    parser.add_argument("--assert-min", type=int, default=0,
                        help="fail unless at least this many mutations ran. This is the "
                             "ratchet: raise it as invariants are added, never lower it")
    parser.add_argument("--skip-baseline", action="store_true",
                        help="do not verify the unmutated suite is green first. Only for "
                             "CI where a prior job already proved it")
    parser.add_argument("--only", action="append", default=[],
                        help="run only this mutation ID; repeat for a focused rerun")
    parser.add_argument("--file-prefix", default=None,
                        help="run only mutations whose source path starts with this prefix")
    parser.add_argument("--shard", type=_parse_shard, default=None,
                        help="run deterministic shard INDEX/COUNT (one based)")
    parser.add_argument("--report", type=pathlib.Path, default=None,
                        help="write an atomic JSON result report after every mutation")
    args = parser.parse_args(argv)

    try:
        data = load(args.manifest)
        entries = mutations(data, args.manifest)
        if args.only:
            requested = set(args.only)
            known = {entry["id"] for entry in entries}
            if unknown := requested - known:
                raise ManifestError("unknown mutation ID(s): " + ", ".join(sorted(unknown)))
            entries = [entry for entry in entries if entry["id"] in requested]
        if args.file_prefix:
            entries = [
                entry for entry in entries if entry["file"].startswith(args.file_prefix)
            ]
        if args.shard:
            entries = select_entries(entries, *args.shard)
        if not entries:
            raise ManifestError("selection contains no mutations")
    except ManifestError as error:
        print(f"MANIFEST ERROR: {error}", file=sys.stderr)
        return 1

    root = (args.root or args.manifest.resolve().parent).resolve()
    while not (root / ".git").exists() and root != root.parent and args.root is None:
        root = root.parent
    command = shlex.split(args.test_cmd or data.get("test_command") or DEFAULT_TEST_COMMAND)
    timeout = args.timeout or data.get("timeout_seconds") or DEFAULT_TIMEOUT_SECONDS
    manifest_sha256 = hashlib.sha256(args.manifest.read_bytes()).hexdigest()
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=root, check=False, capture_output=True, text=True
    )
    commit_sha = commit.stdout.strip() if commit.returncode == 0 else "unknown"
    try:
        entry_commands = {
            entry["id"]: mutation_command(
                entry, data, command, allow_scoped=args.test_cmd is None
            )
            for entry in entries
        }
        entry_timeouts = {
            entry["id"]: (
                args.timeout if args.timeout is not None
                else mutation_timeout(entry, data, timeout)
            )
            for entry in entries
        }
        entry_compile_commands = {
            entry["id"]: mutation_compile_command(entry, data) for entry in entries
        }
        entry_compile_timeouts = {
            entry["id"]: mutation_compile_timeout(entry, data, entry_timeouts[entry["id"]])
            for entry in entries
        }
    except ManifestError as error:
        print(f"MANIFEST ERROR: {error}", file=sys.stderr)
        return 1

    print(f"root:    {root}")
    print(f"baseline suite: {' '.join(command)}")
    print(f"mutations:      {len(entries)} selected")
    if args.shard:
        print(f"shard:          {args.shard[0]}/{args.shard[1]}")
    print()

    baseline_status = "skipped"
    if not args.skip_baseline:
        problem = verify_baseline(root, command, timeout)
        if problem:
            print(f"FAIL: {problem}", file=sys.stderr)
            return 1
        print("baseline: unmutated full workspace suite is green", flush=True)
        distinct_scoped = {
            (tuple(entry_commands[entry["id"]]), entry_timeouts[entry["id"]])
            for entry in entries if entry_commands[entry["id"]] != command
        }
        for scoped_command, scoped_timeout in sorted(distinct_scoped):
            problem = verify_baseline(
                root,
                list(scoped_command),
                scoped_timeout,
                require_one_test="--exact" in scoped_command,
            )
            if problem:
                print(
                    f"FAIL: scoped mutation suite {' '.join(scoped_command)} is not green: "
                    f"{problem}", file=sys.stderr
                )
                return 1
        baseline_status = "passed"
        print(f"baseline: {len(distinct_scoped)} scoped suite(s) are green\n", flush=True)

    rows = []
    survived = stale = timed_out = invalid = 0
    for entry in entries:
        effective_command = entry_commands[entry["id"]]
        effective_timeout = entry_timeouts[entry["id"]]
        print(
            f"RUN       {entry['id']:32s} command={' '.join(effective_command)} "
            f"timeout={effective_timeout:.0f}s",
            flush=True,
        )
        started = time.monotonic()
        try:
            status, detail = apply_mutation(
                entry,
                root,
                effective_command,
                effective_timeout,
                compile_command=entry_compile_commands[entry["id"]],
                compile_timeout=entry_compile_timeouts[entry["id"]],
                fallback_command=command,
                fallback_timeout=effective_timeout,
            )
        except ManifestError as error:
            print(f"MANIFEST ERROR on {entry['id']}: {error}", file=sys.stderr, flush=True)
            return 1
        if status == "survived":
            survived += 1
        elif status == "stale":
            stale += 1
        elif status == "timeout":
            timed_out += 1
        elif status == "invalid":
            invalid += 1
        rows.append({
            "id": entry["id"],
            "file": entry["file"],
            "status": status,
            "detail": detail,
            "invariant": entry["invariant"],
            "duration_seconds": round(time.monotonic() - started, 3),
            "command": effective_command,
            "compile_command": entry_compile_commands[entry["id"]],
            "compile_timeout_seconds": entry_compile_timeouts[entry["id"]],
            "timeout_seconds": effective_timeout,
        })
        _write_report(
            args.report,
            entries,
            rows,
            args.shard,
            manifest_sha256,
            commit_sha,
            baseline_status,
            command,
            [list(scoped_command) for scoped_command, _ in sorted(distinct_scoped)]
            if not args.skip_baseline else [],
        )
        # Flush per mutation: CI captures stdout through a pipe, so without this
        # a run that dies part way through shows no verdicts at all.
        print(f"{status.upper():9s} {entry['id']:32s} {entry['file']}", flush=True)

    print()
    print(f"{'id':32s} {'file':28s} verdict")
    print("-" * 76)
    for row in rows:
        verdict = row["status"].upper()
        print(f"{row['id']:32s} {row['file']:28s} {verdict}")
        if verdict != "KILLED":
            print(f"  invariant: {row['invariant']}")
            print(f"  {row['detail']}")

    total = len(rows)
    killed = total - survived - stale - timed_out - invalid
    print()
    print(f"{total} mutations run, {killed} killed, {survived} survived, "
          f"{stale} stale, {timed_out} timed out, {invalid} invalid", flush=True)

    if total < args.assert_min:
        print(f"FAIL: only {total} mutations ran, expected at least {args.assert_min} -- "
              f"the ratchet was lowered or entries were removed", file=sys.stderr)
        return 1
    if stale:
        print(f"FAIL: {stale} manifest entry(ies) no longer match the current source -- "
              f"update find/replace. This is NOT evidence of an unguarded invariant",
              file=sys.stderr)
        return 1
    if timed_out:
        print(f"FAIL: {timed_out} mutation(s) hung the suite instead of failing it -- "
              f"bound the blocking wait so the invariant is proven by a fast failure",
              file=sys.stderr)
        return 1
    if invalid:
        print(f"FAIL: {invalid} mutant(s) did not compile; compile failures are not "
              f"quality evidence", file=sys.stderr)
        return 1
    if survived:
        print(f"FAIL: {survived} mutation(s) survived -- the invariant is unguarded by any "
              f"test. Write the missing test, do not delete the mutation", file=sys.stderr)
        return 1
    print("PASS: all mutations killed")
    return 0


def _interrupt_on_termination(signum, _frame):
    raise KeyboardInterrupt(f"received signal {signum}")


if __name__ == "__main__":  # pragma: no cover - CLI wrapper
    handled = (signal.SIGTERM, signal.SIGHUP)
    previous = {sig: signal.getsignal(sig) for sig in handled}
    for sig in handled:
        signal.signal(sig, _interrupt_on_termination)
    try:
        raise SystemExit(main())
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)
