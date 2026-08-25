# Liminal

> Machine perception at the edge of reality.

Liminal is a project to let an ordinary MacBook develop a persistent,
uncertainty-aware perception and memory of the physical place around it,
using only hardware already built into the Mac — no external sensors. The
full product, research, and privacy specification lives in
[`LIMINAL_MASTER_PLAN.md`](LIMINAL_MASTER_PLAN.md); this README tracks what
is actually built, right now, against that plan.

## Status

This repository is early. Nothing here talks to a camera, microphone,
Wi-Fi radio, or Bluetooth radio yet. What exists is the hardware-independent
canonical-state layer the rest of the system will be built on: the Rust
crates that own privacy policy, epistemic-layer boundaries, and the
append-only event ledger.

Per the plan's own [README Claims Policy](LIMINAL_MASTER_PLAN.md#145-readme-claims-policy),
every capability below is labeled `WORKING`, `EXPERIMENTAL`, or `PLANNED` —
never claimed beyond what is tested.

| Capability | Status | Evidence |
|---|---|---|
| Epistemic layer boundary (OBSERVED/INFERRED/INTERPRETED/IMAGINED, agent-role write boundary) | WORKING | `crates/liminal-schema`, mutation-guarded |
| BLE/Wi-Fi pseudonymization + Mode A privacy sanitization | WORKING | `crates/liminal-policy`, mutation-guarded |
| Space-anchor invalidation (confidence downgrade on divergence) | WORKING | `crates/liminal-policy`, mutation-guarded |
| Append-only event ledger with hash-chain integrity | WORKING | `crates/liminal-ledger` |
| Erase-cascade (privacy delete invalidates dependents) | WORKING | `crates/liminal-ledger`, mutation-guarded |
| Sensor-gap acknowledgment (belief can't bridge an outage) | WORKING | `crates/liminal-ledger`, mutation-guarded |
| Sensorium discovery, camera/audio/Wi-Fi/BLE organs, fusion, TUI, native app | PLANNED | see [ROADMAP.md](ROADMAP.md) |

## Repository layout

```text
crates/liminal-schema/    epistemic layers, claims, agent-role write boundary
crates/liminal-policy/    HMAC pseudonymization, Wi-Fi Mode A sanitization, space-anchor policy
crates/liminal-ledger/    hash-chain event log, provenance graph, erase cascade, sensor-gap guard
checks/                   mutation guard, coverage gate, docs gate (see docs/ARCHITECTURE.md)
```

## Development

```bash
cargo test --workspace              # unit tests
cargo clippy --workspace --all-targets -- -D warnings
python3 checks/mutation_guard.py --manifest checks/mutations.json --assert-min 7
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for how the pieces fit
together, and [`AUDIT.md`](AUDIT.md) / [`ROADMAP.md`](ROADMAP.md) for what's
built versus what's next.
