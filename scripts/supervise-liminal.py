#!/usr/bin/env python3
"""Start and supervise the complete local Liminal terminal runtime."""
from __future__ import annotations

import argparse
import os
import pathlib
import shutil
import signal
import subprocess
import sys
import tempfile
import time


GRACE_SECONDS = 5.0
STARTUP_SECONDS = 180.0


def _alive(pid: int) -> bool:
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def _reap(pid: int) -> bool:
    try:
        waited, _status = os.waitpid(pid, os.WNOHANG)
    except ChildProcessError:
        return not _alive(pid)
    return waited == pid


def _stop(process: subprocess.Popen | None) -> None:
    if process is None:
        return
    pid = process.pid
    if process.poll() is not None or not _alive(pid):
        _reap(pid)
        return
    try:
        process.terminate()
    except ProcessLookupError:
        _reap(pid)
        return
    deadline = time.monotonic() + GRACE_SECONDS
    while time.monotonic() < deadline:
        if process.poll() is not None or _reap(pid) or not _alive(pid):
            return
        time.sleep(0.05)
    try:
        process.kill()
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        pass


def _socket_id(path: pathlib.Path) -> str | None:
    try:
        stat = path.stat()
    except FileNotFoundError:
        return None
    return f"{stat.st_dev}:{stat.st_ino}"


def _cleanup_socket_candidate(
    initial_id: str | None, current_id: str | None, daemon_started: bool
) -> str | None:
    """Claim only a socket inode that appeared or changed after our daemon started."""
    if not daemon_started or current_id == initial_id:
        return None
    return current_id


def _log_text(path: pathlib.Path) -> str:
    try:
        return path.read_text(errors="replace")
    except OSError:
        return ""


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Start liminald, optional live sensor capture, and the TUI in one terminal."
    )
    parser.add_argument("--root", type=pathlib.Path, required=True, help=argparse.SUPPRESS)
    parser.add_argument("--no-capture", action="store_true")
    args = parser.parse_args(argv)
    root = args.root.resolve()
    run_dir = pathlib.Path(tempfile.gettempdir()) / f"liminal-run-{os.getpid()}"
    daemon_log_path = run_dir / "liminald.log"
    capture_log_path = run_dir / "liminal-capture.log"
    socket_path = pathlib.Path(f"/tmp/liminal-{os.getuid()}/core.sock")
    initial_socket_id = _socket_id(socket_path)

    termination_signal = 0
    daemon: subprocess.Popen | None = None
    capture: subprocess.Popen | None = None
    tui: subprocess.Popen | None = None
    owned_socket_id: str | None = None
    daemon_log = None
    capture_log = None

    def terminate(signum, _frame) -> None:
        nonlocal termination_signal
        termination_signal = signum
        for process in (tui, capture, daemon):
            if process is not None and process.poll() is None:
                try:
                    process.terminate()
                except ProcessLookupError:
                    pass

    # No managed process exists before these handlers are installed.
    for handled in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(handled, terminate)

    def interrupted() -> bool:
        return termination_signal != 0

    def wait_ready(process: subprocess.Popen, predicate, label: str, log_path: pathlib.Path) -> bool:
        deadline = time.monotonic() + STARTUP_SECONDS
        while time.monotonic() < deadline and not interrupted():
            if predicate():
                return True
            if process.poll() is not None:
                print(f"{label} exited during startup:\n{_log_text(log_path)[:16000]}", file=sys.stderr)
                return False
            time.sleep(0.1)
        if not interrupted():
            print(f"Timed out waiting for {label}:\n{_log_text(log_path)[:16000]}", file=sys.stderr)
        return False

    try:
        run_dir.mkdir(parents=True)
        print("Liminal runtime", flush=True)
        print(f"  logs: {run_dir}", flush=True)

        daemon_log = daemon_log_path.open("w")
        daemon = subprocess.Popen(
            ["cargo", "run", "--quiet", "-p", "liminald"],
            cwd=root,
            stdout=daemon_log,
            stderr=subprocess.STDOUT,
        )
        daemon_marker = f"liminald: listening on {socket_path}"
        if not wait_ready(
            daemon,
            lambda: socket_path.is_socket() and daemon_marker in _log_text(daemon_log_path),
            f"liminald at {socket_path}",
            daemon_log_path,
        ):
            return 128 + termination_signal if interrupted() else 1
        owned_socket_id = _socket_id(socket_path)

        if args.no_capture:
            print("  capture: disabled (--no-capture)", flush=True)
        else:
            capture_log = capture_log_path.open("w")
            capture = subprocess.Popen(
                [
                    "swift", "run", "--package-path", "app/Liminal",
                    "--configuration", "debug", "liminal-capture",
                ],
                cwd=root,
                stdout=capture_log,
                stderr=subprocess.STDOUT,
            )
            print(
                f"  capture: starting (permission prompts and sensor logs are in {capture_log_path})",
                flush=True,
            )

        if interrupted():
            return 128 + termination_signal
        print("  tui:     starting (q or Esc quits and cleans up all processes)\n", flush=True)
        tui = subprocess.Popen(["cargo", "run", "--quiet", "-p", "liminal-tui"], cwd=root)
        if interrupted() and tui.poll() is None:
            tui.terminate()
        return_code = tui.wait()
        return 128 + termination_signal if interrupted() else return_code
    finally:
        cleanup_socket_id = owned_socket_id or _cleanup_socket_candidate(
            initial_socket_id, _socket_id(socket_path), daemon is not None
        )
        _stop(tui)
        _stop(capture)
        _stop(daemon)
        for handle in (capture_log, daemon_log):
            if handle is not None:
                handle.close()
        if cleanup_socket_id is not None and _socket_id(socket_path) == cleanup_socket_id:
            socket_path.unlink(missing_ok=True)
        shutil.rmtree(run_dir, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
