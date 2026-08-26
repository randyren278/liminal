# Liminal

> Machine perception at the edge of reality.

Liminal is a project to let an ordinary MacBook develop a persistent,
uncertainty-aware perception and memory of the physical place around it,
using only hardware already built into the Mac — no external sensors. The
full product, research, and privacy specification lives in
[`LIMINAL_MASTER_PLAN.md`](LIMINAL_MASTER_PLAN.md); this README tracks what
is actually built, right now, against that plan.

## Status

This repository is early. `liminal doctor` (Swift) reads real camera/
microphone/Wi-Fi/Bluetooth capability and current permission state — it
does not open a capture session, tap audio, or scan Wi-Fi/BLE, so it
requests no permission prompts of its own. Everything else is the
hardware-independent canonical-state layer the rest of the system will be
built on: the Rust crates that own privacy policy, epistemic-layer
boundaries, the append-only event ledger, the wire contract to the future
full Swift sensor app, and a CLI to inspect all of it.

Per the plan's own [README Claims Policy](LIMINAL_MASTER_PLAN.md#145-readme-claims-policy),
every capability below is labeled `WORKING`, `EXPERIMENTAL`, or `PLANNED` —
never claimed beyond what is tested.

| Capability | Status | Evidence |
|---|---|---|
| Epistemic layer boundary (OBSERVED/INFERRED/INTERPRETED/IMAGINED, agent-role write boundary) | WORKING | `crates/liminal-schema`, mutation-guarded |
| Sensorium profile data model (sensor states, capability schema) | WORKING | `crates/liminal-schema/src/sensorium.rs` |
| Occupancy event segmentation (hysteresis + gap merging) | WORKING | `crates/liminal-memory` |
| BLE/Wi-Fi pseudonymization + Mode A privacy sanitization | WORKING | `crates/liminal-policy`, mutation-guarded |
| Space-anchor invalidation (confidence downgrade on divergence) | WORKING | `crates/liminal-policy`, mutation-guarded |
| Retention policy (§85 tiers, pure eligibility function) | WORKING | `crates/liminal-policy/src/retention.rs`, mutation-guarded |
| Append-only event ledger with hash-chain integrity | WORKING | `crates/liminal-ledger` |
| Erase-cascade (privacy delete invalidates dependents) | WORKING | `crates/liminal-ledger`, mutation-guarded |
| Sensor-gap acknowledgment (belief can't bridge an outage) | WORKING | `crates/liminal-ledger`, mutation-guarded |
| SQLite-backed ledger persistence (migration, crash recovery) | WORKING | `crates/liminal-ledger` (`SqliteLedger`), mutation-guarded |
| IPC wire envelope (Swift↔Rust contract) + schema-version validation | WORKING | `crates/liminal-ipc` |
| CLI: privacy audit, event browsing, append-order event history | WORKING | `crates/liminal-cli` (`liminal` binary) |
| Sensorium discovery (`liminal doctor`): camera/audio/Wi-Fi/Bluetooth capability + permission state, no capture | WORKING | `app/Liminal` (`liminal-doctor` binary) |
| Vision organ (`liminal-capture`): camera capture + 2D body pose extraction + IPC envelope emission, zero raw frames persisted | EXPERIMENTAL — builds and unit-tests clean, but real-camera capture has not yet been confirmed by a human running it and granting the permission prompt | `app/Liminal` (`liminal-capture` binary) |
| `liminal-tui` mode skeleton + real terminal image/video rendering (Kitty/Sixel via `ratatui-image`) | WORKING | `crates/liminal-tui` |
| Camera/audio/Wi-Fi/BLE capture organs, `liminald`, fusion | PLANNED | see [ROADMAP.md](ROADMAP.md) |

## Repository layout

```text
crates/liminal-schema/    epistemic layers, claims, agent-role write boundary, sensorium profile model
crates/liminal-memory/    occupancy event segmentation (hysteresis, gap merging)
crates/liminal-policy/    HMAC pseudonymization, Wi-Fi Mode A sanitization, space-anchor policy, retention tiers
crates/liminal-ledger/    hash-chain event log (in-memory + SQLite-backed), provenance graph, erase cascade, sensor-gap guard
crates/liminal-ipc/       Protocol Buffers wire envelope + schema-version validation
crates/liminal-cli/       `liminal` binary: privacy audit, event browsing, append-order event history
crates/liminal-tui/       `liminal-tui` binary: PRIMARY interface (mode skeleton + real terminal image rendering)
proto/                    liminal.proto — the IPC wire contract
app/Liminal/              Swift package: LiminalCore (testable) + liminal-doctor (Sensorium probe)
checks/                   mutation guard, coverage gate, docs gate (see docs/ARCHITECTURE.md)
```

## Development

```bash
cargo test --workspace              # unit tests
cargo clippy --workspace --all-targets -- -D warnings
python3 checks/mutation_guard.py --manifest checks/mutations.json --assert-min 11

cd app/Liminal && swift test        # Swift unit tests (JSON schema, hashing)
cd app/Liminal && swiftformat --lint .
cd app/Liminal && swift run liminal-doctor --json   # real hardware probe, no permission prompts

cargo run -p liminal-tui            # the TUI itself -- run this in a real terminal to see it
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for how the pieces fit
together, and [`AUDIT.md`](AUDIT.md) / [`ROADMAP.md`](ROADMAP.md) for what's
built versus what's next.
