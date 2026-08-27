#!/usr/bin/env bash
set -euo pipefail

# Start the complete local Liminal runtime in one terminal:
#   liminald (background) -> liminal-capture (background) -> liminal-tui (foreground)
# The TUI owns the terminal; the other processes write to logs in a temporary directory.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_DIR="${TMPDIR:-/tmp}/liminal-run-$$"
DAEMON_LOG="$RUN_DIR/liminald.log"
CAPTURE_LOG="$RUN_DIR/liminal-capture.log"
DAEMON_PID=""
CAPTURE_PID=""
CAPTURE_ENABLED=1

usage() {
    printf 'Usage: %s [--no-capture]\n\n' "$0"
    printf 'Starts liminald, optional live Swift sensor capture, and the TUI in one terminal.\n'
}

for arg in "$@"; do
    case "$arg" in
        --no-capture) CAPTURE_ENABLED=0 ;;
        -h|--help) usage; exit 0 ;;
        *) printf 'Unknown option: %s\n' "$arg" >&2; usage >&2; exit 2 ;;
    esac
done

mkdir -p "$RUN_DIR"

cleanup() {
    status=$?
    trap - EXIT INT TERM
    stop_child() {
        local pid="$1"
        if [[ -z "$pid" ]] || ! kill -0 "$pid" 2>/dev/null; then
            return
        fi
        kill "$pid" 2>/dev/null || true
        for _ in $(seq 1 50); do
            if ! kill -0 "$pid" 2>/dev/null; then
                wait "$pid" 2>/dev/null || true
                return
            fi
            sleep 0.1
        done
        kill -KILL "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    }
    stop_child "$CAPTURE_PID"
    stop_child "$DAEMON_PID"
    socket_path="/tmp/liminal-$(id -u)/core.sock"
    if [[ -S "$socket_path" ]] && ! lsof -t "$socket_path" >/dev/null 2>&1; then
        rm -f "$socket_path"
    fi
    rm -rf "$RUN_DIR"
    exit "$status"
}
trap cleanup EXIT INT TERM

cd "$ROOT_DIR"
# The TUI owns a full-screen color surface. It clears NO_COLOR in its own process too, but doing
# this here keeps the daemon/capture launcher output and the TUI consistent in color-capable shells.
if [[ "${LIMINAL_COLOR:-}" != "0" && "${LIMINAL_COLOR:-}" != "off" && "${LIMINAL_COLOR:-}" != "false" ]]; then
    unset NO_COLOR
fi
printf 'Liminal runtime\n'
printf '  logs: %s\n' "$RUN_DIR"

cargo run --quiet -p liminald >"$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!

socket_path="/tmp/liminal-$(id -u)/core.sock"
# Swift/Rust builds can take longer than the old ten-second startup window on a
# clean checkout. Keep waiting for the daemon while still failing promptly if
# it exits during startup.
for _ in $(seq 1 300); do
    if [[ -S "$socket_path" ]]; then break; fi
    if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
        printf 'liminald exited before opening %s:\n' "$socket_path" >&2
        sed -n '1,120p' "$DAEMON_LOG" >&2
        exit 1
    fi
    sleep 0.1
done
if [[ ! -S "$socket_path" ]]; then
    printf 'Timed out waiting for liminald at %s\n' "$socket_path" >&2
    sed -n '1,120p' "$DAEMON_LOG" >&2
    exit 1
fi

if [[ "$CAPTURE_ENABLED" -eq 1 ]]; then
    swift run --package-path app/Liminal --configuration debug liminal-capture >"$CAPTURE_LOG" 2>&1 &
    CAPTURE_PID=$!
    for _ in $(seq 1 20); do
        if ! kill -0 "$CAPTURE_PID" 2>/dev/null; then
            printf 'liminal-capture exited during startup:\n' >&2
            sed -n '1,160p' "$CAPTURE_LOG" >&2
            exit 1
        fi
        sleep 0.1
    done
    printf '  capture: running (permission prompts and sensor logs are in %s)\n' "$CAPTURE_LOG"
else
    printf '  capture: disabled (--no-capture)\n'
fi

printf '  tui:     starting (q or Esc quits and cleans up all processes)\n'
printf '\n'
cargo run --quiet -p liminal-tui
