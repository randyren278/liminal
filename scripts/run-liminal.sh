#!/usr/bin/env bash
set -euo pipefail

# Enter one signal-aware supervisor before starting any runtime child. This
# avoids a shell-to-supervisor handoff window where termination could leak a
# daemon, capture process, socket, or temporary logs.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ "${LIMINAL_COLOR:-}" != "0" && "${LIMINAL_COLOR:-}" != "off" \
    && "${LIMINAL_COLOR:-}" != "false" ]]; then
    unset NO_COLOR
fi

exec python3 "$ROOT_DIR/scripts/supervise-liminal.py" --root "$ROOT_DIR" "$@"
