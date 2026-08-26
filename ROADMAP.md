# Roadmap

Ranked, falsifiable, scoped to what's actually reachable next. Ordered by
dependency, not just priority — later items build on earlier ones.

Every item below is **hardware-independent**: buildable and verifiable in
this environment without a camera, microphone, Wi-Fi radio, Bluetooth radio,
or a human granting a macOS permission prompt. That's a deliberate scoping
decision, explained in "What's deliberately not on this list" below — please
read that section before approving, since it's the part that determines what
"autonomous" can honestly mean for this project.

## 1. Memory-hierarchy schema + event segmentation

**Rationale:** the ledger currently stores raw `Event`s with no concept of
Belief → Event → Episode → Pattern (§56–§61 of the master plan). Nothing
above the ledger exists yet, so fusion and memory work has no data model to
target.

**Done-state:** a new `liminal-memory` crate implements the segmentation
hysteresis from §58 (enter: occupied P≥0.70 for 3 consecutive seconds; exit:
empty P≥0.80 for 5 consecutive seconds) as a pure function over a synthetic
probability time series. Unit tests cover: a clean transition, a flicker
that stays under the hysteresis window (must NOT trigger a transition), and
a gap that must be merged rather than split into two Events.

**Risk:** low — pure algorithm, no I/O.

## 2. IPC schema (proto/liminal.proto)

**Rationale:** this is the Swift↔Rust contract (§15). It's the one piece of
the sensor pipeline that's fully specifiable and testable without touching
a sensor — and every future Swift sensor organ needs a concrete target to
serialize into.

**Done-state:** `prost`-generated Rust types compile; an encode→decode
round-trip test is byte-identical; a fixture with a mismatched
`schema_version` is rejected with a typed error, matching §119's "reject
schema mismatch" requirement.

**Risk:** low.

## 3. SQLite-backed ledger persistence

**Rationale:** the ledger and provenance graph are in-memory only today.
Privacy erase, retention, and CLI browsing (§82) all require real storage —
this is the first item that unblocks several later ones at once.

**Done-state:** events survive a close/reopen cycle with `verify_chain()`
still passing; a corrupted tail row is rejected rather than silently
accepted (crash-recovery per §88); an empty-DB migration test and the first
forward migration both pass (§108).

**Risk:** medium — the schema decided here is expensive to change once
anything else depends on it. Worth getting review on before merging.

## 4. `liminal-cli` — the data-layer-only subset

**Rationale:** of the full CLI contract (§82), `privacy audit`, `events
list`/`show`, and `explain <claim-id>` need nothing but the SQLite store
from item 3 — no camera, no daemon socket, no TCC permission. It's the one
slice of the user-facing product that's honestly buildable right now.

**Done-state:** `liminal privacy audit` against a fixture DB seeded with a
deliberately leaked forbidden key exits non-zero and names the offending
record; against a clean DB it exits 0. `liminal explain <id>` walks the
provenance graph from item 3's storage back to source Observations.

**Risk:** low.

## 5. Retention policy as a pure function

**Rationale:** §85's decay tiers and §171's privacy metrics ("raw video
files in normal mode = 0", etc.) are product-level claims. The policy that
decides what's eligible for deletion should exist and be tested before it's
wired to a scheduler that actually touches disk.

**Done-state:** unit tests cover each retention tier (observations 7 days,
belief frames 30 days, events 1 year) with boundary timestamps exactly at,
one second before, and one second after the cutoff.

**Risk:** low.

## 6. `SensoriumProfile` data model

**Rationale:** §22 defines the exact JSON shape a Sensorium probe reports.
Modeling it now — without the live macOS probing — gives a future Swift
probe a concrete, tested target instead of an ad hoc struct invented at
build time.

**Done-state:** the literal JSON example from §22 round-trips through the
Rust type unchanged; every sensor state in §3's enum (`UNKNOWN` through
`DISABLED_BY_USER`) is representable.

**Risk:** low.

---

## What's deliberately not on this list

The master plan's fixed development order (§177) goes sensor discovery →
permissions → IPC → vision → acoustics → Wi-Fi → Bluetooth → calibration →
fusion → canvas → TUI → agents → field trials. Most of that is **Swift code
that drives real macOS hardware**, and I want to be direct about why it
isn't on this roadmap rather than attempt it and produce something that
looks done but isn't:

- **The Swift sensor organs and native app** (camera/Vision, AVAudioEngine,
  CoreWLAN, CoreBluetooth, the Spectral Canvas) need a human physically
  present to grant camera/microphone/Bluetooth/Location TCC permission
  prompts — macOS will not grant these to an unattended agent, and that's
  correct, not a limitation to route around. They also need code signing
  under an Apple Developer identity to run at all.
- **Any live acceptance test** (`liminal doctor --live`, §143) has the same
  dependency — it exists to prove real hardware truth, which by definition
  can't be faked in CI or by me alone.
- **The 7-day and 30-day field trials** (L20, L23) require the app running
  unattended in a real room on your machine. Starting one is your call, not
  something to schedule on my own initiative.
- **The nonvisual fusion classifier and its held-out evaluation** (L11)
  needs real calibration data captured from the sensors above. Building a
  classifier before that data exists would be exactly the premature
  complexity §47 warns against.

Once items 1–6 are in and a human has walked through the Swift
permission/capture setup on real hardware at least once, the natural next
roadmap is the Swift sensor organs themselves — but that's a decision for
after this slice lands, not a commitment to make now.

---

**Approve, cut, or reorder this list, then Phase 3 builds it one item at a
time — test-first, mutation-guarded, CI green before the next item starts.**
