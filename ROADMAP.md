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

## Update 2026-08-26: hardware phase, TUI-primary pivot

Items 1–6 above landed and merged. The user is now physically present at
the target Mac, which unblocks the hardware-dependent work this document
originally deferred (see the superseded section below, kept for the
record). Two things changed since then, both decided in conversation with
the user rather than unilaterally:

1. **`liminal doctor` (L2) is done and merged** — `app/Liminal`, a Swift
   package that reads real camera/microphone/Wi-Fi/Bluetooth capability and
   authorization state. It never opens a capture session, so it requests no
   permission prompts and needs no code signing to be useful.
2. **The master plan's native SwiftUI/Metal app (D014) is superseded by a
   TUI-primary architecture**, at the user's direction. Rationale: a
   dedicated `.app` bundle only matters for *branded* permission prompts and
   a polished menu-bar presence — neither is required to build or verify
   real sensor capture. Swift's role shrinks to a **headless capture
   daemon** (AVFoundation + Vision, AVAudioEngine, CoreWLAN, CoreBluetooth —
   still all real macOS APIs behind real TCC prompts, just with no window of
   its own); the Rust TUI (`liminal-tui`, already first-class per §80) becomes
   the **primary** interface, not a secondary operator tool, using
   `ratatui` + `ratatui-image` for real bitmap/video rendering (the user's
   terminal, Ghostty, supports the Kitty graphics protocol — confirmed, not
   assumed). This is a real architecture decision, recorded here and in
   `docs/ARCHITECTURE.md`, not a silent drift from the master plan's stated
   D014 — `LIMINAL_MASTER_PLAN.md` itself is left unedited as the original
   constitution; this file and `docs/ARCHITECTURE.md` track what's actually
   being built against it, same as always.

### New roadmap: hardware capture + TUI (in dependency order)

1. **`liminal-tui` skeleton** — a Rust binary crate using `ratatui`, wiring
   up the mode structure from §72 (SPECTRAL/BELIEF/MEMORY/FIELD NOTES) as
   navigable screens with placeholder content, plus a `ratatui-image`-based
   panel proven to render a real image in the user's terminal. Done-state:
   the user runs it and confirms an actual image (not ASCII art) renders.
2. **Vision organ (camera capture + 2D pose)** — extend `app/Liminal` with
   a capture daemon subcommand: `AVCaptureSession` + `VNDetectHumanBodyPoseRequest`,
   emitting body-count/pose/motion-region features as `liminal-ipc` protobuf
   envelopes over a Unix domain socket (§15/§119, socket path per §15). Zero
   raw frames persisted (§120 exit criterion). This is the first command that
   requests a real TCC prompt — the user grants it interactively.
3. **`liminald` skeleton** — a Rust daemon accepting the Unix socket
   connection from item 2, decoding envelopes, and appending them to
   `SqliteLedger` (already built). No fusion yet — just ingest and persist.
4. **DONE, with a correction from how it was originally scoped here.**
   Wire `liminal-tui` to `liminald`'s SQLite store — this item's original
   wording called for "the live camera frame reference view... and pose
   overlay," which would require a raw camera frame in the ledger. §120's
   exit criterion (zero raw video files) and the Swift→Rust contract (§14,
   derived features only) both forbid that — no raw frame ever exists to
   render. Built instead: REFERENCE mode renders a real skeleton from the
   most recent `liminal-capture` pose observation (derived joint data, not
   pixels — see `crates/liminal-tui/src/ledger_view.rs`), and MEMORY mode
   shows the real ingested event count. Both fall back to the item-1 demo
   pattern when no real data exists yet. Belief-latency budget (§93, p95 <
   500ms) not yet measured — there's no belief frame to time until fusion
   exists.
5. **DONE.** Passive acoustic organ — `AVAudioEngine` tap, §27 features
   (RMS, spectral centroid/rolloff/flatness, ZCR, VAD probability), same
   envelope/socket path as item 2. Real DSP bug caught and fixed by its own
   test: an unwindowed FFT biased the spectral centroid by >1.5kHz via
   sidelobe leakage; fixed with a Hann window, verified against synthetic
   tone/noise/silence buffers (23 tests). `voice_activity_probability` is
   an explicit heuristic, documented as such — not a trained model, and
   §28 doesn't require one. EXPERIMENTAL until a human runs it live.
6. **DONE.** Wi-Fi + Bluetooth organs — live `CWWiFiClient` scanning (Mode
   A aggregate only, `sanitizeWifiModeA` ported line-for-line from
   `liminal_policy::sanitize_wifi_mode_a` so both languages agree on what
   "Mode A" means) and `CBCentralManager` scanning with HMAC
   pseudonymization wired in for real, not just unit-tested against
   synthetic input. The HMAC key is Keychain-persisted (§18) rather than
   random-per-process, so recurring-cluster detection (§39/§40) actually
   means something across restarts. EXPERIMENTAL until a human runs it
   live — same reasoning as items 2 and 5.

**All six items on this roadmap slice are now built.** Fusion (§52,
combining these into one belief), calibration (§44), and the 7-/30-day
field trials remain explicitly **not** started — per §47, building a
classifier before real calibration data exists from items 2–6 is the
premature complexity the master plan itself warns against. That stays
future work until there's real data to justify it, and until a human has
run every organ above at least once to confirm they work as built.

---

## Superseded: original "what's deliberately not on this list" (2026-08-25)

Kept for the record — the reasoning was correct at the time (no human was
present to grant permissions or decide on the app-vs-TUI question) and the
update above supersedes it now that both conditions changed.

The master plan's fixed development order (§177) goes sensor discovery →
permissions → IPC → vision → acoustics → Wi-Fi → Bluetooth → calibration →
fusion → canvas → TUI → agents → field trials. Most of that is **Swift code
that drives real macOS hardware**, and the Swift sensor organs and native
app needed a human physically present to grant camera/microphone/
Bluetooth/Location TCC permission prompts, and code signing under an Apple
Developer identity to run at all. The 7-/30-day field trials and the
nonvisual fusion classifier (needs real calibration data, §47) remain
future work for the same underlying reasons as before.

---

**This update was discussed and directed by the user in conversation, not
presented as a batched proposal awaiting approval — build proceeds
directly per the "New roadmap" section above.**
