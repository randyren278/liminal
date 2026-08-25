# Feature Audit

Per `full-reign` Phase 2. Every feature that exists in this repository,
checked against: is it wired up, is it tested, is it mutation-guarded, is it
documented, and does it actually work (verified by running it in this
session, not by reading the code).

This audit is short because the repository is young: as of this commit, the
entire implementation is the hardware-independent canonical-state core
described in `docs/ARCHITECTURE.md`. Nothing from the Swift/sensor/GUI side
of the master plan exists yet.

## `crates/liminal-schema` — epistemic layers & agent-role boundary

| Check | Result |
|---|---|
| Wired | Yes — standalone crate, no dead code, all public items exercised by tests |
| Tested | Yes — 7 unit tests |
| Mutation-guarded | Yes — 2 of the crate's 2 core invariants (imagined-evidence boundary, agent-role layer boundary) |
| Documented | Yes — module doc comments cite plan §§ directly; covered in `docs/ARCHITECTURE.md` |
| Actually works | Verified: `cargo test -p liminal-schema` → 7 passed, ran in this session |

**Gap:** only `Claim`/`Evidence`/`AgentRole` exist. The plan's richer types
(`Observation`, `BeliefFrame`, `Event`, `Episode`, `Pattern`,
`Interpretation` — §50–§61) are not yet modeled. See ROADMAP item 1.

## `crates/liminal-policy` — privacy & space-anchor policy

| Check | Result |
|---|---|
| Wired | Yes |
| Tested | Yes — 8 unit tests |
| Mutation-guarded | Yes — 3 of 3 core invariants (BLE HMAC, Wi-Fi Mode A key leakage, space-anchor invalidation) |
| Documented | Yes |
| Actually works | Verified: `cargo test -p liminal-policy` → 8 passed, ran in this session |

**Gap:** `pseudonymize` covers BLE; Wi-Fi Mode B stable pseudonyms (§36) are
not implemented (Mode A anonymous aggregate is the default and only mode
today, which is spec-compliant but incomplete). No `liminal privacy audit`
CLI surface yet — the scanning function exists as a library call only.

## `crates/liminal-ledger` — event ledger & provenance

| Check | Result |
|---|---|
| Wired | Yes |
| Tested | Yes — 7 unit tests |
| Mutation-guarded | Yes — 2 of 2 core invariants (erase cascade, sensor-gap bridging) |
| Documented | Yes |
| Actually works | Verified: `cargo test -p liminal-ledger` → 7 passed, ran in this session |

**Gap:** no persistence (SQLite, §84) — the ledger and provenance graph are
in-memory only. No crash-recovery/replay (§88) yet since there is nothing to
recover from disk. `ProvenanceGraph` and `Ledger` are separate types that
don't yet share state (a real erase would need to invalidate ledger events,
not just a standalone graph).

## Everything else in the master plan

Sensorium discovery (L2), the native permission shell (L3), the Swift
sensor organs (L5–L9, camera/audio/Wi-Fi/BLE), space calibration (L10), the
nonvisual baseline (L11), fusion (L12), the Spectral Canvas (L14), the TUI
(L15), field-note agents (L16), the historical memory UI (L17), the 7- and
30-day field trials (L20, L23), and packaging (L24) are **not started**.
None of this is dead code to flag — it simply does not exist yet. See
`ROADMAP.md` for what's next and why the Swift/hardware side is out of
autonomous reach in this environment.

## Repository hygiene

- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean, zero warnings.
- No secrets, no `.env`, no committed credentials.
- `.gitignore` excludes `target/`, Xcode build artifacts, `Package.resolved`.
- Git history so far: 3 commits on branch full-reign/2026-08-25, `main` untouched.
