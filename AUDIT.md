# Feature Audit

Current-state audit for the full-reign build. Each feature is checked for
wiring, tests, mutation coverage where the invariant is safety-critical,
documentation, and a runnable proof path.

| Area | Wired and verified behavior | Evidence and remaining boundary |
|---|---|---|
| `liminal-schema` | Epistemic layers, claim/evidence boundaries, and agent-role restrictions | Rust tests and 2 mutation checks pass; richer provenance types remain future work |
| `liminal-policy` | BLE HMAC pseudonyms, Wi-Fi Mode A sanitization, space-anchor downgrade, retention decisions | Rust tests and 3 mutation checks pass; Wi-Fi Mode B is not implemented |
| `liminal-ledger` | Hash-chained in-memory and SQLite persistence with open-time crash recovery, migrations, explicit SQLite provenance edges, confirmation-gated erase cascade, sensor-gap state, derived observations, beliefs, and structural records | Rust tests and ledger mutation checks pass; automatic retention workers remain future work |
| `liminal-memory` | Hysteresis occupancy segmentation, gap merging, deterministic Event → Episode → Pattern replay, and offline labeled calibration scoring | Rust tests pass; replay is structural only and no human-labeled trial has been collected, so the heuristic and recurrence interpretation are not calibrated |
| `liminald` | Unix-socket ingest, schema/JSON/frame validation, per-stream gap detection, replay deduplication, bounded connection queue, freshness-weighted fusion with explicit stable/contested state | Rust tests and daemon mutation checks pass; contradictory modalities are contested, stale inputs decay out, and an existing unacknowledged historical gap suppresses new fusion beliefs |
| `liminal-cli` | Privacy audit, event listing/show, append-order history, explicit provenance-source lookup, calibration scoring, operator-only sensor-gap recovery, explicit memory replay, and Tier-0 agent runs | Rust tests pass; provenance is separate from hash-chain history, calibration reports never retune the live model, recovery appends acknowledgment events instead of deleting or bridging history, replay writes only structural records, and agent runs require structured evidence |
| `liminal-capture` | Real camera pose, microphone DSP, Wi-Fi Mode A, and Bluetooth coordinator emit derived IPC envelopes; sequences are independent and durable per stream | Swift tests/build and live launcher run pass; camera/microphone/Wi-Fi delivery observed on this Mac, and the Bluetooth radio independently reported three advertiser discoveries; derived Bluetooth emission still fails closed when the Keychain key is unavailable |
| `liminal-doctor` | Non-prompting capability probe plus bounded `--live --duration=N --json` acceptance | Swift tests/build pass; live acceptance reports derived counts/statuses only and never stores raw media |
| `liminal-tui` | SPECTRAL, BELIEF, MEMORY, FIELD NOTES, REFERENCE, and CALIBRATION modes; live SQLite polling; telemetry-driven bitmap field using all derived feature values; recent observation rates; derived pose skeleton; nonvisual vision-off path; pause/resume; diagnostics; displayed belief evidence IDs; bounded persisted Tier-0 drafts; historical record/provenance drilldown; explicit synthetic demo mode | 52 focused Rust tests pass; real terminal runs rendered live telemetry and the labeled demo bitmap, then exited cleanly |
| Runtime launcher | Starts daemon, capture, and TUI together; cleans up child processes and socket | `scripts/run-liminal.sh` live run added observations, created no new cross-stream gaps, and left no Liminal processes/socket |

## Safety and privacy proof

- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- `cargo fmt --all -- --check`, `git diff --check`, and the documentation gate
  pass.
- The mutation manifest contains 43 critical invariants. The latest full
  42-case run killed every prior invariant with zero survivors, and the new
  SQLite erase invariant independently killed in its scoped ledger/CLI guard;
  CI's ratchet now requires all 43 in the next full sweep.
- The ledger contains observations, derived features, and explicitly requested
  structural memory records only. No raw camera
  frames, PCM audio, SSIDs, or Bluetooth names are written by the capture
  path.

## Remaining plan work

The remaining master-plan items are human-labeled calibration and nonvisual
baseline trials, active acoustic probing, richer fusion health/OOD behavior,
full historical provenance navigation beyond the bounded 32-record
drill-down, durable retention and export workers, and seven-/thirty-day
trials. Recovery and privacy boundaries are implemented and verified, but
they do not substitute for those human-dependent trials.
These are intentionally not claimed by the current CUI or its synthetic/unit
test evidence.
