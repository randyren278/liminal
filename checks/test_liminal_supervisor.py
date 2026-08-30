from __future__ import annotations

import importlib.util
import pathlib
import unittest


SUPERVISOR_PATH = pathlib.Path(__file__).resolve().parents[1] / "scripts/supervise-liminal.py"
SPEC = importlib.util.spec_from_file_location("liminal_supervisor", SUPERVISOR_PATH)
assert SPEC is not None and SPEC.loader is not None
SUPERVISOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUPERVISOR)


class SocketOwnershipTests(unittest.TestCase):
    def test_claims_socket_created_after_daemon_start(self) -> None:
        self.assertEqual(SUPERVISOR._cleanup_socket_candidate(None, "new", True), "new")

    def test_claims_replacement_of_stale_socket(self) -> None:
        self.assertEqual(SUPERVISOR._cleanup_socket_candidate("stale", "new", True), "new")

    def test_does_not_claim_preexisting_active_socket(self) -> None:
        self.assertIsNone(SUPERVISOR._cleanup_socket_candidate("active", "active", True))

    def test_does_not_claim_socket_without_started_daemon(self) -> None:
        self.assertIsNone(SUPERVISOR._cleanup_socket_candidate(None, "other", False))


if __name__ == "__main__":
    unittest.main()
