from __future__ import annotations

import json
import contextlib
import io
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock


CHECKS = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(CHECKS))

from mutation_guard import (  # noqa: E402
    apply_mutation,
    main,
    mutation_command,
    mutation_compile_command,
    mutation_compile_timeout,
    select_entries,
    verify_baseline,
)
from verify_mutation_reports import (  # noqa: E402
    declared_command,
    declared_compile_command,
    verify_reports,
)


class MutationSelectionTests(unittest.TestCase):
    def test_exact_test_command_still_gets_prefix_compile_binding(self) -> None:
        entry = {
            "file": "crates/liminal-tui/src/belief.rs",
            "test_command": "cargo test -p liminal-tui exact -- --exact",
        }
        self.assertEqual(
            declared_command(entry, {}, ["baseline"]),
            ["cargo", "test", "-p", "liminal-tui", "exact", "--", "--exact"],
        )
        self.assertEqual(
            declared_compile_command(
                entry, {"crates/liminal-tui/": "cargo test -p liminal-tui --no-run"}
            ),
            ["cargo", "test", "-p", "liminal-tui", "--no-run"],
        )

    def test_compile_command_uses_longest_matching_prefix(self) -> None:
        entry = {"file": "crates/liminald/src/lib.rs"}
        data = {"mutation_compile_commands": {
            "crates/": "cargo test --workspace --no-run",
            "crates/liminald/": "cargo test -p liminald --no-run",
        }}
        self.assertEqual(
            mutation_compile_command(entry, data),
            ["cargo", "test", "-p", "liminald", "--no-run"],
        )
        self.assertEqual(
            mutation_compile_timeout(
                entry,
                {"mutation_compile_timeout_seconds": {"crates/": 150, "crates/liminald/": 90}},
                180,
            ),
            90,
        )

    def test_scoped_baseline_uses_the_scoped_timeout(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "source.txt").write_text("guard = true\n")
            manifest = root / "mutations.json"
            manifest.write_text(json.dumps({
                "test_command": "full-suite",
                "timeout_seconds": 90,
                "mutation_test_commands": {"source": "scoped-suite"},
                "mutation_timeout_seconds": {"source": 3},
                "mutations": [{
                    "id": "scoped",
                    "file": "source.txt",
                    "invariant": "scoped timeout",
                    "find": "true",
                    "replace": "false",
                }],
            }))
            with (
                mock.patch("mutation_guard.verify_baseline", return_value=None) as baseline,
                mock.patch("mutation_guard.apply_mutation", return_value=("killed", "done")),
            ):
                with contextlib.redirect_stdout(io.StringIO()):
                    result = main([
                        "--manifest", str(manifest),
                        "--root", str(root),
                        "--assert-min", "1",
                    ])

            self.assertEqual(result, 0)
            self.assertEqual(baseline.call_args_list, [
                mock.call(root.resolve(), ["full-suite"], 90),
                mock.call(
                    root.resolve(),
                    ["scoped-suite"],
                    3.0,
                    require_one_test=False,
                ),
            ])

    def test_scoped_command_uses_longest_matching_prefix(self) -> None:
        entry = {"file": "crates/liminald/src/lib.rs"}
        data = {
            "mutation_test_commands": {
                "crates/": "cargo test --workspace",
                "crates/liminald/": "cargo test -p liminald -p liminal-cli",
            }
        }

        self.assertEqual(
            mutation_command(entry, data, ["fallback"]),
            ["cargo", "test", "-p", "liminald", "-p", "liminal-cli"],
        )

    def test_entry_command_overrides_prefix_and_exact_preflight_cannot_run_zero_tests(self) -> None:
        entry = {"file": "crates/liminald/src/lib.rs", "test_command": "exact test -- --exact"}
        data = {"mutation_test_commands": {"crates/liminald/": "package test"}}
        self.assertEqual(mutation_command(entry, data, ["fallback"]), ["exact", "test", "--", "--exact"])
        self.assertIn(
            "did not execute exactly one",
            verify_baseline(pathlib.Path.cwd(), ["/usr/bin/true"], 1, require_one_test=True),
        )

    def test_shards_are_disjoint_and_cover_every_entry(self) -> None:
        entries = [{"id": f"m-{index}"} for index in range(43)]
        shards = [select_entries(entries, index, 7) for index in range(1, 8)]

        flattened = [entry["id"] for shard in shards for entry in shard]
        self.assertEqual(sorted(flattened), sorted(entry["id"] for entry in entries))
        self.assertEqual(len(flattened), len(set(flattened)))
        self.assertLessEqual(max(map(len, shards)) - min(map(len, shards)), 1)

    def test_shard_assignment_does_not_depend_on_manifest_order(self) -> None:
        entries = [{"id": value} for value in ("delta", "alpha", "charlie", "bravo")]
        forwards = [entry["id"] for entry in select_entries(entries, 1, 2)]
        backwards = [entry["id"] for entry in select_entries(list(reversed(entries)), 1, 2)]
        self.assertEqual(forwards, backwards)


class MutationLifecycleTests(unittest.TestCase):
    def test_compile_failure_is_invalid_not_killed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            target = root / "source.txt"
            target.write_text("guard = true\n")
            failed_compile = subprocess.CompletedProcess(["compile"], 1, "", "error")
            with mock.patch("mutation_guard._run", return_value=failed_compile) as run:
                status, _detail = apply_mutation(
                    {"id": "invalid", "file": "source.txt", "find": "true", "replace": "false"},
                    root,
                    ["test"],
                    1,
                    compile_command=["compile"],
                )
            self.assertEqual(status, "invalid")
            self.assertEqual(run.call_count, 1)
            self.assertEqual(target.read_text(), "guard = true\n")

    def test_scoped_survival_falls_back_to_full_suite_before_verdict(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            target = root / "source.txt"
            target.write_text("guard = true\n")
            scoped = subprocess.CompletedProcess(["scoped"], 0, "", "")
            full = subprocess.CompletedProcess(["full"], 1, "", "")
            with mock.patch("mutation_guard._run", side_effect=[scoped, full]) as run:
                status, detail = apply_mutation(
                    {"id": "fallback", "file": "source.txt", "find": "true", "replace": "false"},
                    root,
                    ["scoped"],
                    1,
                    fallback_command=["full"],
                )

            self.assertEqual(status, "killed")
            self.assertIn("full workspace fallback", detail)
            self.assertEqual(run.call_count, 2)
            self.assertEqual(target.read_text(), "guard = true\n")

    def test_timeout_kills_process_tree_before_restoring_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            target = root / "source.txt"
            target.write_text("guard = true\n")
            pid_file = root / "child.pid"
            child = (
                "import os,pathlib,signal,time;"
                "signal.signal(signal.SIGTERM,signal.SIG_IGN);"
                f"pathlib.Path({str(pid_file)!r}).write_text(str(os.getpid()));"
                "time.sleep(60)"
            )
            parent = (
                "import subprocess,sys,time;"
                f"subprocess.Popen([sys.executable,'-c',{child!r}]);"
                "time.sleep(60)"
            )
            status, _detail = apply_mutation(
                {"id": "tree", "file": "source.txt", "find": "true", "replace": "false"},
                root,
                [sys.executable, "-c", parent],
                0.5,
            )

            self.assertEqual(status, "timeout")
            self.assertEqual(target.read_text(), "guard = true\n")
            pid = int(pid_file.read_text())
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.01)
            else:
                os.kill(pid, signal.SIGKILL)
                self.fail(f"child process {pid} survived mutation timeout")

    def test_sigterm_restores_source_and_kills_process_tree(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            target = root / "source.txt"
            target.write_text("guard = true\n")
            pid_file = root / "child.pid"
            child = (
                "import os,pathlib,signal,time;"
                "signal.signal(signal.SIGTERM,signal.SIG_IGN);"
                f"pathlib.Path({str(pid_file)!r}).write_text(str(os.getpid()));"
                "time.sleep(60)"
            )
            parent = (
                "import subprocess,sys,time;"
                f"subprocess.Popen([sys.executable,'-c',{child!r}]);"
                "time.sleep(60)"
            )
            tree_script = root / "tree.py"
            tree_script.write_text(parent)
            manifest = root / "mutations.json"
            manifest.write_text(json.dumps({
                "test_command": "false",
                "mutations": [{
                    "id": "terminate",
                    "file": "source.txt",
                    "invariant": "termination restores source",
                    "find": "true",
                    "replace": "false",
                }],
            }))
            process = subprocess.Popen([
                sys.executable,
                str(CHECKS / "mutation_guard.py"),
                "--manifest", str(manifest),
                "--root", str(root),
                "--skip-baseline",
                "--test-cmd", f"{sys.executable} {tree_script}",
            ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            deadline = time.monotonic() + 5
            while not pid_file.exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertTrue(pid_file.exists(), "mutation child did not start")
            process.terminate()
            process.wait(timeout=5)
            self.assertEqual(target.read_text(), "guard = true\n")
            self.assert_process_gone(int(pid_file.read_text()))

    def assert_process_gone(self, pid: int) -> None:
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return
            time.sleep(0.01)
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            return
        self.fail(f"child process {pid} survived process cleanup")


class MutationReportTests(unittest.TestCase):
    def test_aggregate_requires_exactly_once_killed_coverage(self) -> None:
        entries = [{"id": "one"}, {"id": "two"}]
        reports = [
            {"schema_version": 1, "commit_sha": "abc", "baseline": "passed", "selected_ids": ["one"], "results": [{"id": "one", "status": "killed"}]},
            {"schema_version": 1, "commit_sha": "abc", "baseline": "passed", "selected_ids": ["two"], "results": [{"id": "two", "status": "killed"}]},
        ]
        self.assertEqual(verify_reports(entries, reports), [])

        duplicate = reports + [{"schema_version": 1, "commit_sha": "abc", "baseline": "passed", "selected_ids": ["two"], "results": [{"id": "two", "status": "killed"}]}]
        errors = verify_reports(entries, duplicate)
        self.assertTrue(any("duplicate" in error for error in errors))

        survived = [{"schema_version": 1, "commit_sha": "abc", "baseline": "passed", "selected_ids": ["one", "two"], "results": [{"id": "one", "status": "killed"}, {"id": "two", "status": "survived"}]}]
        errors = verify_reports(entries, survived)
        self.assertTrue(any("two" in error and "survived" in error for error in errors))

        missing = [{"schema_version": 1, "commit_sha": "abc", "baseline": "passed", "selected_ids": ["one"], "results": [{"id": "one", "status": "killed"}]}]
        errors = verify_reports(entries, missing)
        self.assertTrue(any("missing" in error and "two" in error for error in errors))

        swapped = [
            {"schema_version": 1, "commit_sha": "abc", "baseline": "passed", "selected_ids": ["one"], "results": [{"id": "two", "status": "killed"}]},
            {"schema_version": 1, "commit_sha": "abc", "baseline": "passed", "selected_ids": ["two"], "results": [{"id": "one", "status": "killed"}]},
        ]
        errors = verify_reports(entries, swapped)
        self.assertTrue(any("do not exactly match" in error for error in errors))

        errors = verify_reports(entries, reports, expected_commit_sha="different")
        self.assertTrue(any("checked-out commit" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
