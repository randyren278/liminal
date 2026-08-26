# Architecture

Full rationale: [`LIMINAL_MASTER_PLAN.md`](../LIMINAL_MASTER_PLAN.md). This
document tracks the architecture of what is actually built.

## Target runtime shape (revised 2026-08-26 — TUI-primary)

The master plan's original D014 ("native visual app is first-class") is
superseded, at the user's direction — see `ROADMAP.md`'s "Update
2026-08-26" section for the full rationale. Swift no longer owns a windowed
app; it's a **headless capture daemon**. The Rust TUI is the primary
interface, rendering real bitmap/video via the Kitty graphics protocol
(confirmed supported by the user's terminal, Ghostty 1.3.1) through
`ratatui-image`, not ASCII-art approximation. Swift and Rust still talk
over a Unix domain socket via Protocol Buffers (§15) exactly as before;
raw camera frames and raw microphone PCM still never cross that boundary —
only this diagram's shape changed, not the privacy posture.

```mermaid
flowchart TB
    subgraph Swift["Liminal capture daemon (Swift, headless, no window)"]
        Doctor["liminal-doctor: Sensorium probe (built, no capture)"]
        Sensors["liminal-capture: camera + 2D pose (built, EXPERIMENTAL -- unverified by a human yet)"]
        Extract["Audio/Wi-Fi/BLE feature extraction (not built)"]
    end
    subgraph Rust["liminald (Rust) -- ingest-only skeleton, built"]
        IPC["liminal-ipc: wire envelope, schema-version validation"]
        Ledger["liminal-ledger: hash-chain events (in-memory + SQLite), erase cascade"]
        Policy["liminal-policy: pseudonymization, privacy audit, space anchor, retention"]
        Schema["liminal-schema: epistemic layers, agent-role boundary, sensorium profile"]
        Memory["liminal-memory: occupancy segmentation (not wired to ingest yet)"]
    end
    CLI["liminal-cli: privacy audit, event browsing, event history"]
    TUI["liminal-tui: PRIMARY interface -- mode skeleton + real bitmap render built; wiring to Ledger not done"]

    Doctor -.->|"SensoriumProfile JSON (same shape, not yet wired)"| Schema
    Sensors -->|"real IPC envelope over Unix socket -- verified end-to-end with a manual test client"| IPC
    Extract -.->|"not built yet"| IPC
    IPC --> Ledger
    Ledger --> Policy
    Ledger --> Schema
    Ledger -.-> Memory
    Ledger --> CLI
    Ledger -.-> TUI
```

## What exists today

The canonical-state core, its wire contract, and a first Swift probe exist
so far:

- **`crates/liminal-schema`** — the four epistemic layers (OBSERVED,
  INFERRED, INTERPRETED, IMAGINED) and the hard boundary between them: no
  IMAGINED artifact may back a factual claim, and each agent role
  (Archivist, Ethnographer, Skeptic, Cartographer, Poet) can only author
  claims in its permitted layer. Also carries `SensoriumProfile` (§22): the
  data shape a future Swift probe will report hardware capabilities into.
- **`crates/liminal-memory`** — Observation → Event segmentation (§58):
  hysteresis-based occupancy transitions with gap merging, over a synthetic
  probability time series. Not yet wired to a live belief stream.
- **`crates/liminal-policy`** — HMAC-SHA256 pseudonymization for BLE
  identifiers, Wi-Fi Mode A sanitization (aggregate features only, no
  SSID/BSSID field exists on the output type), a recursive privacy-audit
  scanner for forbidden keys, space-anchor divergence handling that caps
  belief confidence when the laptop appears to have moved, and the §85
  retention-tier eligibility function (pure decision logic, not wired to an
  actual deletion scheduler yet).
- **`crates/liminal-ledger`** — an append-only, BLAKE3 hash-chained event
  log with two implementations: an in-memory `Ledger` (the original) and a
  SQLite-backed `SqliteLedger` (persisted, with a migration and
  crash-recovery test). A separate in-memory `ProvenanceGraph` cascades
  invalidation from an erased source to every downstream
  Event/Episode/Pattern/Interpretation; it is not connected to
  `SqliteLedger`'s persisted storage (see "Known architectural gap" below).
  A sensor-gap guard refuses to record belief across an unacknowledged
  sensor outage, on both ledger implementations.
- **`crates/liminal-ipc`** — the Swift↔Rust wire contract: a Protocol
  Buffers envelope (§15) matching the exact field list the plan specifies,
  with schema-version rejection (§119) so a mismatched build can't silently
  desync. No transport (reconnect, dedup, backpressure) yet — that's a
  `liminald` runtime concern, not this crate's.
- **`crates/liminal-cli`** — the `liminal` binary's data-layer-only
  subcommands: `privacy audit` (scans a `SqliteLedger`'s stored records for
  forbidden keys), `events list`/`show`, and `events history <id>` (walks
  `SqliteLedger`'s `previous_hash` chain back to genesis — an append-order/
  integrity view, explicitly NOT a provenance/derivation query; it was
  originally named `explain` and framed as §62 provenance drilldown, then
  renamed after review caught that mismatch — see the gap note below).
- **`crates/liminald`** — ROADMAP item 3: an ingest-only daemon skeleton.
  Prepares the §15 socket path (0700 directory, 0600 socket file — one of
  its two mutation-guarded invariants), binds a `UnixListener`, and for each
  connection decodes length-delimited `liminal-ipc` envelopes, validates
  their schema version, and persists them via
  `SqliteLedger::append_observation_with_features` (a new method added
  alongside this crate, since the existing `append_observation` discarded
  everything but `stream_id`/`ts_us` — real sensor features need to survive
  ingest). No fusion, no belief frames — just validate and persist. Verified
  end-to-end for real: `crates/liminald/examples/send_test_envelope.rs`
  connects over a real Unix socket and the resulting SQLite row was
  inspected directly (not just asserted in a test) during development.
- **`crates/liminal-tui`** — the primary interface (2026-08-26 pivot, above):
  a mode skeleton (SPECTRAL/BELIEF/MEMORY/FIELD NOTES/REFERENCE, §72) with
  `ratatui-image` proven to render real animated bitmap output over the
  terminal's graphics protocol (Kitty/Sixel, auto-detected; halfblock
  fallback otherwise) — the demo panel is an explicitly-labeled synthetic
  pattern, not a sensor feed, per §146's Demo Honesty rule. Not yet wired
  to `liminal-ledger` or any real sensor data.
- **`app/Liminal`** — a Swift package: `LiminalCore` (a testable library —
  the `SensoriumProfile`/`SensorState` Codable types mirroring
  `liminal-schema`'s Rust types field-for-field, plus hashing) and
  `liminal-doctor` (the §21/§117 `liminal doctor` / `liminal doctor --json`
  CLI, a thin wrapper over `LiminalCore`'s hardware probes). It reads real
  camera format/resolution, microphone sample rate, Wi-Fi interface state,
  and Bluetooth authorization/power state on the machine it runs on — but
  it never opens an `AVCaptureSession`, taps microphone audio, scans Wi-Fi
  networks, or starts a BLE scan, so it requests zero permission prompts.
  See "TCC and unsigned CLI binaries" below before building anything past
  this probe. The package also has `liminal-capture` (ROADMAP item 2, §120
  Vision Organ): requests camera authorization explicitly (§90), then uses
  `VisionCaptureCoordinator` (`AVCaptureSession` + `VNDetectHumanBodyPoseRequest`)
  to extract 2D body pose per frame and emit it as a `liminal-ipc` envelope —
  over the Unix socket at `/tmp/liminal-$UID/core.sock` if something is
  listening (nothing is yet — that's item 3), else printed to stdout. Zero
  raw frames are ever written to disk. The pose-extraction and envelope/
  framing logic is unit-tested (including a real Unix-socket round-trip
  test using an in-process POSIX listener); the actual camera capture path
  is EXPERIMENTAL — it builds and the logic around it is tested, but a
  human has not yet run it and confirmed real pose data comes out the other
  end, since granting the camera permission prompt requires a human present
  at the keyboard.

Everything else in the master plan — Sensorium discovery's live acceptance
mode, the full permission shell, `liminald` (item 3), passive acoustics,
Wi-Fi/BLE scanning (items 5-6), calibration, fusion, field-note agents — is
`PLANNED`. See [`ROADMAP.md`](../ROADMAP.md) for the proposed build order
and [`AUDIT.md`](../AUDIT.md) for the full feature-by-feature status.

## TCC and unsigned CLI binaries

`liminal-doctor` is an unsigned Swift Package Manager executable, not a
signed `.app` bundle. On macOS, an unsigned CLI tool invoked from a
terminal does not get its own TCC (privacy permission) identity — the OS
attributes camera/microphone/Bluetooth authorization checks to whatever
process hosts it (observed directly during development: `AVCaptureDevice
.authorizationStatus(for: .audio)` returned `available` because the host
terminal already had microphone access granted, not because
`liminal-doctor` itself had been granted anything).

This is fine for `liminal-doctor`'s current scope (it only ever *reads*
authorization state, never requests it), but it means the next milestone —
actually requesting camera/microphone/Bluetooth access and having macOS
prompt the user by *Liminal's* name, not the terminal's — requires
Liminal.app to be a properly code-signed application bundle with its own
bundle identifier and Info.plist usage-description strings (§90/§118).
Building that permission shell as another SPM executable would silently
attribute every grant to the terminal instead of to Liminal, which is
exactly the "no hidden sensing" violation §3/§14 exist to prevent.

## Known architectural gap: two disconnected provenance mechanisms

`liminal-ledger` currently has two ways to answer "what does this claim
depend on":

1. `ProvenanceGraph` — an explicit dependency DAG (`add_node(id,
   depends_on)`) with cascading erase-invalidation. Built for and only
   exercised by that crate's own tests; nothing persists a `depends_on`
   edge into SQLite.
2. `SqliteLedger`'s `previous_hash` chain — every persisted `Event` already
   links to its predecessor. `liminal-cli`'s `events history <id>` walks
   this chain, which is real persisted data, but it's the GLOBAL APPEND
   ORDER (an integrity mechanism, §87), not a derivation graph: an
   unrelated event from a different stream, appended between two related
   ones, appears in the history exactly as if it were evidence. It is not
   the branching dependency graph `ProvenanceGraph` models (an Event can
   only have one predecessor in the chain; the plan's real provenance
   model, §62, is Observation → Event → Episode → Pattern →
   Interpretation, a many-to-one fan-in `ProvenanceGraph` is built for).

These are not yet reconciled. Building Episodes/Patterns/Interpretations
(out of scope for every task built so far) will need one coherent,
persisted provenance mechanism — deciding whether that's a
`depends_on`-edges table replacing `ProvenanceGraph`, or something else, is
a design decision for whoever picks up that work, not something to guess at
here.

## Why these crates first

The master plan's fixed development order (§177) starts with the
constitution and sensor discovery, but the constitution's actual
enforcement mechanism — the privacy and epistemic invariants — has no
hardware dependency at all. Building and mutation-testing that layer first
means every later crate (fusion, memory, agents) is built on top of
boundaries that are already proven to hold, rather than proven later by
inspection.

## Testing

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 checks/mutation_guard.py --manifest checks/mutations.json --assert-min 12
python3 checks/coverage_gate.py --manifest checks/mutations.json --report target/lcov.info
```

CI (`.github/workflows/ci.yml`) runs all four on every push and pull
request, plus `checks/docs_gate.py` against this file and the README.
