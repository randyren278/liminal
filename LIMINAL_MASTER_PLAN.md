# LIMINAL
## Machine Perception at the Edge of Reality
### Complete Product, Research, Architecture, UX, Privacy, Testing, and Development Specification

**Document type:** Development constitution / master implementation plan  
**Status:** Foundational specification  
**Product name:** **Liminal**  
**Primary thesis:** An ordinary MacBook can develop a persistent, uncertainty-aware perception and memory of the physical place around it using only hardware already built into the Mac.  
**Hardware constraint:** **No external sensing hardware. No ESP32, CSI dongle, UWB anchor, LiDAR, depth camera, external microphone array, Raspberry Pi, router modification, iPhone/Continuity Camera, Apple Watch, or other accessory may be required.**  
**Platform:** macOS, Apple Silicon preferred  
**Primary UI:** Native visual application + engineering/operator TUI  
**Core languages:** Swift + Rust; Python is research/offline only  
**Network posture:** Local-first and network-off by default  
**Raw media posture:** Raw camera frames and microphone audio are ephemeral by default and are not written to disk  
**Core epistemic rule:** `OBSERVED ≠ INFERRED ≠ INTERPRETED ≠ IMAGINED`

---

# 0. One-Sentence Product Definition

> **Liminal lets an ordinary MacBook perceive a room through the senses it already has—vision, acoustics, Wi-Fi, and Bluetooth—and turns those incomplete signals into a persistent, uncertain memory of physical space.**

# 1. What Liminal Is

Liminal is simultaneously:

- an ambient-computing system
- a machine-perception experiment
- a local sensor-fusion runtime
- an uncertainty visualization
- a spatial memory engine
- an agentic ethnography experiment
- a privacy research artifact
- generative software art

It asks:

> **What can a normal laptop know about a physical place without any added hardware?**

And then:

> **What happens when the machine is required to show the difference between what it sensed, what it inferred, what it believes, and what it imagined?**

The project is not trying to hide uncertainty. **Uncertainty is the medium.**

# 2. What Liminal Is Not

Liminal must never be described as any of the following unless future experimental evidence genuinely supports the claim:

- “Wi-Fi vision through walls”
- “DensePose from a MacBook's Wi-Fi”
- “radar using your laptop”
- “whole-house mapping from Wi-Fi”
- “camera-free person identification”
- “precise through-wall localization”
- “emotion recognition”
- “mind reading”
- “identity recognition”
- “a surveillance system”
- “an accurate simulation of human culture”
- “a conscious room”
- “a digital twin with ground-truth geometry”

The stock MacBook Wi-Fi interface publicly exposes aggregate WLAN telemetry such as RSSI, noise, channel, transmit rate, and scans via CoreWLAN. It does **not** expose the raw per-subcarrier Channel State Information used by specialized Wi-Fi sensing research. CSI extraction frameworks such as Nexmon target patched firmware on supported hardware such as Raspberry Pi variants; that violates Liminal's hardware constitution.

Therefore:

> **Wi-Fi in Liminal is an environmental perturbation sensor, not a camera replacement.**

# 3. Hardware Constitution

Development must assume exactly one ordinary MacBook and no additional sensor hardware.

The application must discover the actual capabilities of the user's Mac at runtime. It must never assume:

- exact MacBook model
- exact camera model
- camera depth support
- microphone channel count
- speaker channel topology
- sample rate
- Bluetooth state
- Wi-Fi chipset/band
- access-point count
- Location permission
- camera permission
- microphone permission
- Bluetooth permission
- stable BLE identifiers
- specific CPU/GPU performance

Every sensor has one state:

```text
UNKNOWN
PROBING
AVAILABLE
DEGRADED
DENIED
BUSY
UNSUPPORTED
FAILED
DISABLED_BY_USER
```

No missing sensor is fatal. Liminal adapts its Sensorium to what is actually available.

# 4. The Sensorium

Canonical built-in sensing modalities:

1. **Vision**
2. **Passive acoustics**
3. **Active acoustics**
4. **Wi-Fi radio atmosphere**
5. **Bluetooth proximity atmosphere**
6. **Machine context**

Only the first five may contribute to physical-place belief. Machine context exists only to explain sensor quality and runtime state.

# 5. Capability Matrix

| Modality | macOS API | Canonical data | V0.1 use | Precision claim |
|---|---|---|---|---|
| Camera | AVFoundation + Vision | pose, motion, scene features | teacher/reference + optional live sensor | strong only inside field of view |
| 2D body pose | Vision | joints + confidence | occupancy/motion ground truth | supported |
| 3D body pose | Vision | camera-relative joints if supported | optional | never assumed |
| Microphone | AVAudioEngine | PCM in memory | passive acoustic features | environmental, not semantic |
| Speaker | AVAudioEngine | controlled output | optional active probe | environment-change sensing |
| Wi-Fi | CoreWLAN | RSSI/noise/channel/rate/scans | coarse RF atmosphere | no CSI |
| BLE | CoreBluetooth | advertisements + RSSI | proximity clusters | no identity |
| System context | ProcessInfo/app state | thermal/load/sleep | explain degradation | not physical sensing |

# 6. External Hardware Ban

The following are explicitly forbidden as required dependencies:

```text
ESP32 / ESP8266
Raspberry Pi
Nexmon CSI hardware
Wi-Fi monitor-mode adapters
UWB anchors
mmWave radar
external microphone arrays
external cameras
depth cameras
LiDAR
Kinect / RealSense
Arduino
Bluetooth beacons deployed for Liminal
custom routers
SDR / HackRF / USRP
iPhone sensor relay / Continuity Camera
Apple Watch sensor relay
HomePod sensor relay
```

Development may mention them only in research comparisons. No feature may be marked **working** if any is required.

# 7. Primary Product Metaphor

Liminal has two conceptual systems.

## 7.1 SPECTER — live machine Sensorium

Answers:

> **What does the Mac think is happening right now?**

## 7.2 MEMORY — historical place model

Answers:

> **What does the Mac think has happened here over time?**

SPECTER is an internal subsystem, not the product name.

# 8. Epistemic Layers

Every piece of Liminal knowledge belongs to exactly one layer.

## 8.1 OBSERVED

Direct sensor-derived measurement with no semantic leap beyond the measurement.

Examples:

```text
Vision returned one body pose.
Wi-Fi RSSI changed from -43 dBm to -57 dBm.
BLE pseudonym B17 appeared at RSSI -61.
Acoustic residual energy increased 14%.
Left-region optical-flow energy increased.
```

## 8.2 INFERRED

Probabilistic model output derived from observations.

Examples:

```text
occupancy likely
movement likely
entry/exit likely
space anchor likely changed
proximity cluster returned
```

## 8.3 INTERPRETED

Agent-generated higher-level meaning that cites evidence.

Examples:

```text
This time window appears to be a recurring use period.
The left region is used more frequently in evenings.
This proximity cluster often co-occurs with occupancy.
```

## 8.4 IMAGINED

Explicitly artistic/speculative transformation.

Examples:

```text
“On Sundays, the room learns to expect company.”
generated memory sculpture
dream sequence
poetic narration
counterfactual scene
```

### Hard boundary

No IMAGINED artifact may become evidence for OBSERVED, INFERRED, or INTERPRETED claims. No INTERPRETED claim may silently become OBSERVED fact.

# 9. Canonical Pipeline

```text
SENSORS
   ↓
FEATURES
   ↓
OBSERVATIONS
   ↓
BELIEF
   ↓
EVENTS
   ↓
EPISODES
   ↓
PATTERNS
   ↓
AGENT INTERPRETATION
   ↓
ART
```

Every arrow is explicit and preserves provenance.

# 10. Runtime Architecture

Liminal is not one giant Python process.

```text
┌─────────────────────────────────────────────────────────┐
│                    Liminal.app                          │
│                  Swift / SwiftUI                        │
│                                                         │
│ Permissions  Sensors  Vision  Audio  Native Renderer   │
└─────────────────────────┬───────────────────────────────┘
                          │ derived observations only
                          ▼
                    Unix domain socket
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│                      liminald                           │
│                         Rust                            │
│ ingest → ledger → calibration → fusion → memory → API  │
└───────────────┬───────────────────┬─────────────────────┘
                │                   │
                ▼                   ▼
        SQLite event store      Artifact store
                │
                ▼
┌─────────────────────────────────────────────────────────┐
│                    liminal TUI                          │
│                         Rust                            │
└─────────────────────────────────────────────────────────┘

Optional offline:
┌─────────────────────────────────────────────────────────┐
│                    liminal-lab                          │
│                        Python                           │
│ statistics / acoustics / training / notebooks          │
└─────────────────────────────────────────────────────────┘
```

# 11. Why Swift Owns Sensors

Swift owns protected/native Mac sensing because AVFoundation, Vision, AVAudioEngine, CoreWLAN, CoreBluetooth, and macOS TCC integrate naturally there. The sensor process performs media feature extraction **before** crossing the process boundary.

**Raw frames and raw continuous microphone PCM must not be sent to `liminald` in normal operation.**

# 12. Why Rust Owns Canonical State

Rust owns:

- append-only event ledger
- belief state
- sensor fusion
- memory hierarchy
- event segmentation
- calibration/model metadata
- privacy policies
- canonical configuration
- CLI/TUI
- replay/crash recovery
- integrity checks
- local API
- agent orchestration metadata

Canonical truth must not live in Swift view state or notebooks.

# 13. Python Scope

Python is allowed for exploratory DSP, feature analysis, model training, statistics, benchmark evaluation, plotting, and experimental notebooks.

Python is **not** the sole implementation of canonical privacy, event state, deletion, pause/stop, retention, or runtime authority.

# 14. Process Model

## `liminald`

Per-user background daemon. Owns canonical state, IPC, DB, fusion, memory, TUI/API, replay and retention. It must not directly access camera/microphone.

## `Liminal.app`

Foreground GUI. Owns TCC permissions, hardware enumeration, media capture, CoreWLAN/CoreBluetooth collection, local feature extraction and the visual art experience.

When it exits, protected sensing stops.

## `liminal`

CLI/TUI client. Owns inspection, diagnostics, calibration controls, privacy operations, event/memory browsing and pause/resume.

# 15. IPC

Use Unix domain sockets under:

```text
/tmp/liminal-$UID/
```

Permissions:

```text
0700 directory
0600 socket
```

Socket:

```text
/tmp/liminal-$UID/core.sock
```

Transport: **Protocol Buffers**, length-delimited frames.

Libraries:

- SwiftProtobuf
- Rust `prost`

Every message includes:

```text
schema_version
message_id
sensor_stream_id
monotonic_sequence
captured_at_utc_us
captured_at_mono_ns
payload
```

Raw video and continuous raw microphone PCM are absent from the normal IPC schema.

# 16. Clock Model

All sensor capture timestamps originate in `Liminal.app`, which maintains wall-clock UTC and a monotonic clock. `liminald` stores both capture and receive timestamps. Clock discontinuities emit `CLOCK_DISCONTINUITY` and temporarily reduce confidence.

# 17. Storage Locations

Canonical data:

```text
~/Library/Application Support/Liminal/
├── liminal.db
├── config.toml
├── models/
├── exports/
├── debug-captures/
└── migrations/
```

Caches:

```text
~/Library/Caches/Liminal/
```

Logs:

```text
~/Library/Logs/Liminal/
```

Secrets: **macOS Keychain only**.

# 18. Secrets

Keychain entries:

```text
pseudonym_hmac_key
optional_sensitive_field_key
cloud_agent_credentials_reference
debug_export_key
```

Secrets never exist in repository, config, SQLite plaintext, or logs.

# 19. Privacy Constitution

Privacy is architectural, not a future feature.

### P1 — Raw video is ephemeral

Default persistent retention: `0 seconds`.

### P2 — Raw microphone audio is ephemeral

Default persistent retention: `0 seconds`.

### P3 — No speech transcription

Core does not transcribe, summarize speech, identify speakers, or recognize names from audio.

### P4 — No face recognition

Liminal may infer that a body exists; it must not identify the person.

### P5 — No sensitive-trait inference

Never infer race, ethnicity, religion, sexual orientation, health, political ideology, disability, or other sensitive traits.

### P6 — No Bluetooth identity resolution

Display `Recurring proximity cluster B17`, never a person's/device's human-readable identity.

### P7 — No SSID storage by default

Wi-Fi names are unnecessary to the thesis.

### P8 — Local-first

Core perception/memory works with network disconnected.

### P9 — Cloud agents never receive raw sensor media

Only derived structured records may be sent with explicit opt-in.

### P10 — Visible sensing

Liminal.app visibly shows sensing state in addition to macOS's system indicators.

# 20. Debug Capture

Raw capture is a separate manual mode.

Requirements:

- manually initiated
- explicit GUI confirmation
- default maximum 30 seconds
- red banner + countdown
- stored only under `debug-captures/`
- default expiry 24 hours
- user can erase immediately
- agent cannot enable it
- normal export excludes it

CLI requests GUI confirmation:

```bash
liminal debug-capture start --seconds 30
```

# 21. Sensorium Discovery

First executable milestone:

```bash
liminal doctor
```

First GUI:

```text
LIMINAL — DISCOVERING SENSORIUM

Camera
━━━━━━━━━━━━━━━━━━━━━━ AVAILABLE

Vision 2D Pose
━━━━━━━━━━━━━━━━━━━━━━ AVAILABLE

Vision 3D Pose
━━━━━━━━━━━━━━━━━━━━━━ PROBING

Microphone
━━━━━━━━━━━━━━━━━━━━━━ AVAILABLE

Speaker Output
━━━━━━━━━━━━━━━━━━━━━━ AVAILABLE

Wi-Fi RSSI/Noise
━━━━━━━━━━━━━━━━━━━━━━ AVAILABLE

Wi-Fi Stable AP IDs
━━━━━━━━━━━━━━━━━━━━━━ NEEDS OPTIONAL LOCATION PERMISSION

Wi-Fi CSI
░░░░░░░░░░░░░░░░░░░░ UNSUPPORTED

Bluetooth
━━━━━━━━━━━━━━━━━━━━━━ AVAILABLE
```

This becomes persisted `SensoriumProfile` data.

# 22. Sensorium Profile Schema

```json
{
  "schema_version": 1,
  "machine_profile_id": "machine:...",
  "created_at": "...",
  "camera": {
    "state": "available",
    "device_id_hash": "...",
    "selected_resolution": [1280, 720],
    "selected_fps": 10,
    "depth_data": false
  },
  "audio_input": {
    "state": "available",
    "sample_rate": 48000,
    "channels": 1
  },
  "audio_output": {
    "state": "available",
    "sample_rate": 48000,
    "channels": 2
  },
  "wifi": {
    "state": "available",
    "aggregate_rssi": true,
    "aggregate_noise": true,
    "scanning": true,
    "stable_ap_ids": false,
    "csi": false
  },
  "bluetooth": {
    "state": "available",
    "scan_rssi": true
  }
}
```

Example numeric values are never hardcoded as assumptions.

# 23. Vision Organ

Frameworks:

- AVFoundation
- Vision

Default capture target:

- 1280×720 when available
- 10 FPS inference target
- adaptive downsampling under thermal pressure

Do not process 60 FPS just because hardware can.

# 24. Vision Features

Persist only derived features.

## Body count

```text
0
1
2+
unknown
```

## 2D body pose

Store normalized joints:

```text
joint_name
x
y
confidence
```

Initial joint confidence floor: `0.25`, configurable and experimentally validated.

## 3D pose

Use only if the Vision request succeeds on the target system. Always label camera-relative. Never call it room-global position without calibration and validation.

## Optical flow / motion

Persist region summaries:

```text
left_motion_energy
center_motion_energy
right_motion_energy
global_camera_motion
```

## Visual scene anchor

Low-dimensional scene fingerprint used only to detect laptop movement/major rearrangement, not identity.

# 25. Camera Teacher Mode

The camera is a calibration teacher.

```text
camera pose/count
       │
       ▼
temporary labels
       │
       ▼
nonvisual model
```

After calibration, the user can disable vision and evaluate what acoustics/Wi-Fi/BLE retain.

# 26. Passive Acoustic Organ

Framework: **AVAudioEngine**.

Raw audio processed in memory.

Feature windows:

```text
20 ms low-level frame
1 s aggregate
5 s aggregate
```

# 27. Passive Audio Features

Allowed persistent features:

- RMS energy
- peak level
- broad-band spectral energies
- spectral centroid
- spectral rolloff
- spectral flatness
- zero-crossing rate
- onset count
- modulation energy
- coarse reverberant change
- voice-activity probability only
- nonsemantic acoustic anomaly score

MFCC persistence is disabled by default to reduce speech-information retention.

# 28. Voice Activity

Voice activity may exist only as:

```text
speech_like_activity_probability
```

Use it to suppress active probes and inform privacy UI. Never use it for transcription, speaker identity, or content analysis.

# 29. Active Acoustic Organ

Opt-in experiment using:

```text
MacBook speaker → room → MacBook microphone
```

to estimate changes in room response.

Published room-acoustic research demonstrates that changing humans/objects can alter measurable impulse responses, but those studies may use richer microphone arrays than a Mac exposes. Therefore V0.1 target is **environmental state change**, not centimeter localization.

# 30. Active Probe Safety

Default: **disabled**.

Restrictions:

- bounded level
- bounded duration
- visible indicator
- no probe during speech-like activity
- no probe during media playback unless overridden
- configurable quiet hours
- immediate stop
- no ultrasonic marketing claim

# 31. Active Probe Waveform

Initial research waveform: exponential sine sweep.

Default candidate:

```text
start_frequency_hz = 300
end_frequency_hz   = 12000
duration_ms        = 750
fade_ms            = 20
output_gain        = conservative calibrated level
```

Effective band must be measured on actual hardware and narrowed automatically if unreliable.

# 32. Acoustic Calibration

1. verify quiet-enough condition
2. user declares baseline state
3. record N probe repetitions
4. estimate repeatability
5. derive baseline RIR features
6. reject unstable setup
7. persist features, not raw PCM

Initial N: `20`.

# 33. RIR Feature Vector

Persist:

- direct/early energy
- early/late energy ratio
- band-limited decay statistics
- coarse RT60 estimate only when stable
- correlation with baseline
- spectral residual
- impulse-response distance
- temporal stability
- per-channel values if multiple channels are exposed

# 34. Wi-Fi Organ

Framework: **CoreWLAN**.

Supported inputs:

- aggregate RSSI
- aggregate noise
- channel
- PHY state
- transmit rate
- transmit power if available
- cached/active scans

CSI is explicitly `UNSUPPORTED_BY_DESIGN` unless Apple later exposes a supported API.

# 35. Wi-Fi Sampling

Current interface telemetry target: up to `2 Hz` initially.

Network scan interval: `30–60 seconds` initially.

Scan rate adapts downward under battery/thermal pressure and can be disabled.

# 36. Wi-Fi Privacy Modes

## Mode A — anonymous atmosphere (default)

Store:

- current RSSI/noise
- visible-network count
- RSSI histogram
- channel histogram
- strongest anonymous RSSI values
- RF change score

No SSID/BSSID persistence.

## Mode B — stable local pseudonyms (optional)

If modern macOS requires Location permission for stable SSID/BSSID information, request it only after explicit user enablement. Transform identifiers immediately:

```text
HMAC-SHA256(local_key, identifier)
```

Persist only pseudonym such as `wifi_ap:7a31`.

# 37. Wi-Fi Feature Vector

Per 5-second window:

```text
current_rssi_mean
current_rssi_std
noise_mean
snr
tx_rate_mean
rssi_delta_from_baseline
visible_network_count
rss_histogram_bins
channel_histogram
strongest_1
strongest_2
strongest_3
rf_novelty_score
```

This is an atmosphere signal, not geometry.

# 38. Bluetooth Organ

Framework: **CoreBluetooth**.

Run as central scanner. Use discovery RSSI. Allow duplicate discoveries when active sampling is enabled. Do not connect to arbitrary devices.

# 39. Bluetooth Privacy

Never display/store human-readable device names by default. Never persist raw manufacturer payload by default.

Canonical pseudonym:

```text
ble:<HMAC(local_key, peripheral.identifier)>
```

Because identifiers can rotate or be unstable, UI calls them **proximity clusters**, not persons or definitive devices.

# 40. Bluetooth Feature Vector

Per cluster:

```text
first_seen
last_seen
sample_count
rssi_mean
rssi_std
rssi_slope
appearance_frequency
recurrence_score
continuity_score
```

Global:

```text
cluster_count
new_cluster_count
departed_cluster_count
near_cluster_count
ble_activity_score
```

# 41. Machine Context

Machine context explains sensing quality.

Store:

- power/battery state when available
- app foreground/background
- sleep/wake
- thermal state
- permission state
- camera/microphone availability
- Wi-Fi/Bluetooth off/on
- dropped messages
- processing latency

Machine context never directly implies human behavior.

# 42. Space Profiles

V0.1 supports one calibrated space at a time.

```text
space_id
display_name
created_at
sensorium_profile_id
visual_anchor_hash
acoustic_baseline_id
wifi_baseline_id
ble_baseline_id
calibration_version
state
```

States:

```text
UNINITIALIZED
CALIBRATING
READY
STALE
RECALIBRATION_REQUIRED
ARCHIVED
```

# 43. Detecting Laptop Movement

Signals:

- global optical flow
- visual anchor mismatch
- acoustic baseline divergence
- RF baseline discontinuity
- BLE field discontinuity
- sleep/wake + scene change

If threshold crossed:

```text
SPACE_ANCHOR_INVALIDATED
```

Then spatial beliefs downgrade and location-specific calibration pauses until the user confirms/recalibrates.

# 44. Calibration Flow

## Stage 1 — Empty baseline

Target: 5 minutes.

## Stage 2 — seated / low movement

Target: 5 minutes.

## Stage 3 — walking

Target: 5 minutes.

## Stage 4 — entry/exit

Target: 10 repetitions.

## Stage 5 — door/object perturbation

Optional, 10 repetitions.

## Stage 6 — natural collection

Minimum: 30 minutes. Recommended: 24 hours.

Camera acts as temporary label source.

# 45. Calibration Labels

Initial classes:

```text
EMPTY
OCCUPIED_STATIONARY
OCCUPIED_MOVING
ENTRY_OR_EXIT
ENVIRONMENT_CHANGED
UNKNOWN
```

Do not require multi-person data for V0.1.

# 46. Spatial Output Classes

Stock-MacBook nonvisual sensing does not promise continuous XY coordinates.

V0.1 language:

```text
UNKNOWN
LEFT / CENTER / RIGHT
NEAR / MID / FAR
```

A sector is enabled only if held-out validation meets its own threshold. Otherwise `POSITION: UNKNOWN`.

# 47. Calibration Model

V0.1 starts interpretable:

- logistic regression
- probability calibration
- Hidden Markov Model / temporal smoothing
- change-point detection

V0.2 may test gradient boosting, temporal convolution, small transformer, or ONNX-exported models only if held-out data justifies complexity.

# 48. Training/Validation Split

Initial split:

```text
60% train/calibration
20% validation
20% held-out
```

Split by contiguous time blocks to reduce temporal leakage.

# 49. Vision-Off Evaluation

Defining experiment:

1. calibrate with camera teacher
2. freeze model
3. disable vision
4. run scripted + natural test
5. evaluate nonvisual inference against separately captured/revealed ground truth
6. publish confusion matrix

Required metrics:

- balanced accuracy
- precision/recall per class
- false occupancy rate
- false empty rate
- transition delay
- calibration drift
- unknown rate

Liminal must proudly display when it does not know.

# 50. Sensor Observation Schema

```json
{
  "observation_id": "obs_...",
  "schema_version": 1,
  "space_id": "space_...",
  "sensor": "wifi",
  "captured_at": "...",
  "window_ms": 5000,
  "kind": "rf_state",
  "features": {},
  "quality": 0.87,
  "uncertainty": 0.13,
  "raw_retained": false,
  "source_version": "sensor-wifi/0.1.0"
}
```

# 51. Belief Frame

Produced at 1 Hz initially.

```json
{
  "belief_id": "belief_...",
  "space_id": "space_...",
  "timestamp": "...",
  "occupancy": {
    "empty": 0.11,
    "stationary": 0.18,
    "moving": 0.62,
    "unknown": 0.09
  },
  "coarse_position": {
    "left": 0.18,
    "center": 0.55,
    "right": 0.17,
    "unknown": 0.10
  },
  "modalities": {
    "vision": 0.0,
    "acoustic": 0.72,
    "wifi": 0.31,
    "ble": 0.63
  },
  "evidence": ["obs_1", "obs_2"],
  "model_version": "fusion-v3"
}
```

# 52. Fusion

V0.1 uses modality-specific calibrated likelihoods, health weighting, room-specific reliability, temporal smoothing, and explicit UNKNOWN.

Conceptually:

```text
P(state | evidence)
∝ P(state) × L_audio × L_wifi × L_ble × L_vision(if enabled)
```

Poor-quality sensors contribute less. Contradiction increases uncertainty rather than forcing consensus.

# 53. Disagreement Is First-Class

Example:

```text
VISION       empty: 0.94
ACOUSTIC     occupied: 0.78
BLE          occupied: 0.69
WIFI         weak evidence
```

Output:

```text
CONTESTED PRESENCE

Vision contradicts nonvisual modalities.
Possible causes:
• person outside camera field
• acoustic environmental change
• recurring BLE cluster nearby
• sensor/model error
```

# 54. Belief Confidence

Confidence includes:

- classifier calibration
- modality agreement
- sensor quality
- training support density
- calibration freshness
- drift/OOD status
- missing modalities

A class probability may be high while epistemic confidence remains low.

# 55. OOD Detection

V0.1:

- robust z-scores / Mahalanobis distance
- sensor baseline divergence
- visual anchor mismatch
- RF novelty
- acoustic novelty

High OOD should produce UNKNOWN rather than forced confidence.

# 56. Memory Hierarchy

```text
Observation
    ↓
Belief Frame
    ↓
Event
    ↓
Episode
    ↓
Pattern
    ↓
Interpretation
    ↓
Artwork
```

# 57. Event

Bounded state/change segment.

Examples:

```text
occupancy_transition
movement_interval
proximity_arrival
proximity_departure
acoustic_shift
rf_shift
space_anchor_change
sensor_outage
```

# 58. Event Segmentation

Use hysteresis, minimum durations, change-point detection, and gap merging.

Initial occupancy defaults:

```text
enter threshold = occupied P >= 0.70 for 3 consecutive seconds
exit threshold  = empty P >= 0.80 for 5 consecutive seconds
```

Calibration may tune them.

# 59. Episode

Merge related Events into a meaningful interval while remaining INFERRED.

Example:

```text
19:03 proximity cluster arrived
19:04 occupancy became moving
19:08 occupancy stationary
19:42 occupancy empty
19:43 proximity cluster departed

→ probable_occupied_session 19:03–19:43
```

No identity claim.

# 60. Pattern

Statistical recurrence, e.g. evening occupancy or recurring proximity coincidence.

Requirements:

- minimum occurrence count
- confidence/statistics
- supporting Episode IDs

Initial minimum: 3 occurrences, visibly marked low-sample.

# 61. Ritual

A Ritual is an **interpretation** of a recurring Pattern, not a sensor fact.

Example:

```text
Pattern:
occupancy 19:00–21:00 on 11 of 14 Sundays

Ethnographer:
“This appears to function as a recurring gathering period.”

Skeptic:
“The evidence cannot establish who gathers or why.”
```

# 62. Provenance Graph

```text
Interpretation
      ↓
Pattern
      ↓
Episodes
      ↓
Events
      ↓
Belief Frames
      ↓
Observations
      ↓
Sensors
```

Every high-level claim can be drilled down to evidence.

# 63. Agent Layer

Agents operate **after deterministic sensor fusion**. They do not decide raw occupancy.

Initial agents:

1. Archivist
2. Ethnographer
3. Skeptic
4. Cartographer
5. Poet

# 64. Archivist Agent

Mission: state what evidence supports with minimal interpretation.

Must cite evidence, uncertainty and time window. Forbidden from identity, motives, emotions or invented causes.

# 65. Ethnographer Agent

Mission: propose behavioral/place interpretations of recurring patterns.

Must:

- cite evidence
- state sample size
- include alternative explanation
- avoid identity/sensitive traits
- distinguish routine from meaning

Claims enter `PENDING_INTERPRETATION` until reviewed.

# 66. Skeptic Agent

Mission: attack interpretations.

Checks:

- unsupported leap
- blind spots
- counter-hypotheses
- small sample
- correlation/causation
- contradictory periods

Verdicts:

```text
SUPPORTED
CONTESTED
REJECTED
INSUFFICIENT_EVIDENCE
```

# 67. Cartographer Agent

Maintains machine-space description without pretending to know ground-truth geometry.

May describe field regions, recurring motion zones, coarse sectors, acoustic state regions, and anchor stability. May not invent room dimensions.

# 68. Poet Agent

Transforms approved memory into art. All output is stamped `IMAGINED` and cannot become evidence.

# 69. Agent Runtime Tiers

## Tier 0 — no LLM

Deterministic summaries. Core works entirely offline.

## Tier 1 — local LLM

Optional local model receives structured Events/Patterns only.

## Tier 2 — cloud model

Explicit opt-in. Receives only policy-selected derived records; never raw media.

# 70. Agent Data Contract

Agents receive structured records such as:

```json
{
  "space": "studio",
  "period": "2026-08-24",
  "events": [{
    "type": "occupied_session",
    "start": "19:02",
    "end": "19:41",
    "confidence": 0.83,
    "evidence": ["event_..."]
  }],
  "sensor_limitations": ["vision_disabled", "wifi_csi_unavailable"]
}
```

# 71. Agent Prompt-Injection Boundary

Sensor-derived text is untrusted data, never instruction.

Interpretation agent tool set:

```text
read_events
read_patterns
read_evidence
write_interpretation
write_artifact
```

No shell by default. Agent cannot alter privacy, permissions, retention or epistemic rules.

# 72. Native Visual Experience

Liminal.app is an instrument, not a SaaS dashboard.

Primary modes:

```text
SPECTRAL
BELIEF
MEMORY
FIELD NOTES
```

Calibration/debug adds:

```text
REFERENCE
```

# 73. SPECTRAL Mode

No raw camera image.

### Acoustic layer

- interference contours
- wave surfaces
- residual-energy clouds
- reverberation tails
- probe rings

### Wi-Fi layer

- slow contour field
- signal/noise tension
- network-density background texture
- RF-change ripples

### BLE layer

- pseudonymous orbiting points
- opacity from confidence
- halo from uncertainty
- recurring clusters leave faint traces

Never render a human silhouette from RF/BLE alone as if measured.

# 74. BELIEF Mode

Fused hypothesis view.

Example:

```text
UNKNOWN PRESENCE
occupancy: 0.78
epistemic confidence: 0.61
position: contested
```

Visual grammar:

- translucent volume = belief
- opacity = probability
- edge jitter = uncertainty
- ghost/split outline = modality disagreement

# 75. MEMORY Mode

Timeline scrubber:

- minutes/hours/days
- belief trails
- recurring proximity traces
- sensor-blind intervals
- Pattern overlays
- Episode selection
- evidence drilldown
- day-to-day compare

Any path-like rendering is labeled `BELIEF TRAIL`, not ground-truth movement.

# 76. FIELD NOTES Mode

Display agents as epistemic cards:

```text
ARCHIVIST
11 of the last 14 Sunday evenings contained an occupied session.

ETHNOGRAPHER
This appears to function as a recurring gathering window.

SKEPTIC
The system cannot establish whether the same people were present or why.

POET
“On Sundays, the room learns to expect company.”
```

# 77. REFERENCE Mode

Calibration/debug only. May show camera view, pose skeleton, motion regions, teacher labels, alignment, FPS and latency.

Always display:

```text
REFERENCE / CAMERA ACTIVE
```

# 78. Visual Rendering Stack

Native stack:

- SwiftUI shell
- Metal / MetalKit spectral renderer
- Core Animation where useful

Avoid Electron.

# 79. Visual Coordinate Model

V0.1 uses camera-relative 2.5D field:

```text
x: -1 left → +1 right
y: -1 bottom → +1 top
depth: near / mid / far / unknown
```

Nonvisual localization uses only coarse sectors when validated.

# 80. TUI

Binary:

```bash
liminal
```

Views:

```text
HOME
SENSORIUM
LIVE
EVENTS
MEMORY
AGENTS
CALIBRATION
PRIVACY
DIAGNOSTICS
```

# 81. TUI Home

```text
╭─ LIMINAL ───────────────────────────────────────────────────────────╮
│ SPACE studio                 STATE ● SENSING                        │
│ VISION OFF                   NONVISUAL BELIEF 0.73                  │
│ MEMORY 11d 04h               ANCHOR STABLE                         │
╰─────────────────────────────────────────────────────────────────────╯

 SENSORIUM
 Camera       ○ disabled
 Acoustic     ● healthy
 Wi-Fi        ● healthy
 Bluetooth    ● healthy

 CURRENT BELIEF

 OCCUPIED_MOVING        0.73
 EMPTY                  0.11
 UNKNOWN                0.16

 POSITION               contested

 RECENT

 09:41:12  BLE cluster B17 arrived
 09:41:14  acoustic field shifted
 09:41:17  occupancy transitioned

 [S] Sensorium  [E] Events  [M] Memory  [P] Privacy  [Q] Pause
```

# 82. CLI Contract

```bash
liminal status
liminal doctor
liminal sensorium
liminal start
liminal pause
liminal resume
liminal calibrate
liminal calibrate status
liminal vision on
liminal vision off
liminal acoustic probe
liminal acoustic baseline
liminal wifi status
liminal bluetooth status
liminal events list
liminal events show <id>
liminal memory today
liminal memory range <start> <end>
liminal explain <claim-id>
liminal agents status
liminal agents run archivist
liminal privacy status
liminal privacy erase --range ...
liminal export --range ...
liminal debug
```

CLI writes never bypass policy.

# 83. Menu Bar

States:

```text
● sensing
◐ degraded
○ paused
! permission required
× error
```

Menu:

```text
Open Liminal
Pause sensing
Vision on/off
Active acoustic probes on/off
Privacy status
Quit
```

No hidden sensing.

# 84. Data Model

Primary SQLite tables:

```text
schema_migrations
sensorium_profiles
spaces
sensor_streams
observations
belief_frames
events
episodes
patterns
interpretations
interpretation_reviews
artifacts
calibrations
model_versions
agent_runs
privacy_actions
system_events
```

# 85. Retention Defaults

```text
high-resolution derived observations: 7 days
belief frames:                       30 days
events:                              1 year
episodes:                            1 year
patterns:                            indefinite
interpretations:                     indefinite
raw camera:                          never
raw continuous audio:                never
debug raw captures:                  24 hours
```

User may shorten retention.

# 86. Data Growth Budget

Default goal: `< 100 MB/day`.

If exceeded, diagnostics identify the source and downsample high-frequency derived data before touching durable Events/Patterns.

# 87. Event Integrity

Canonical events include:

```text
id
sequence
timestamp
type
payload
previous_hash
hash
```

Hash chain:

```text
BLAKE3(previous_hash || canonical_event_payload)
```

This is integrity/replay, not a blockchain.

# 88. Crash Recovery

On restart:

1. open DB
2. verify migrations
3. verify event-chain tail
4. restore space/model/calibration
5. insert sensor gap
6. wait for sensor app
7. resume derived state

Never fabricate continuity across downtime.

# 89. Sleep/Wake

Sleep emits `SYSTEM_SLEEP`; wake emits `SYSTEM_WAKE`.

On wake:

- reprobe hardware
- reset temporal filters
- recheck space anchor
- temporarily increase uncertainty
- never bridge sleep as occupancy

# 90. Permission Model

Request camera, microphone, Bluetooth and optional Location only at relevant feature use. Before the OS prompt, explain exactly what the permission enables, what is stored, and what is never stored.

# 91. Permission Denial

Denial is valid configuration.

If camera is denied:

- continue nonvisual mode
- explain reduced calibration ability
- do not repeatedly nag
- provide Settings guidance only when requested

Same philosophy applies to microphone, Bluetooth, and optional Location.

# 92. Wi-Fi Location Permission

Modern macOS may gate SSID/BSSID behind Location privilege. Liminal does **not require** stable AP identity.

Default Mode A uses anonymous aggregate atmosphere data when available.

Mode B:

- explicit user enablement
- request Location only then
- pseudonymize immediately
- never persist geographic location
- never include location coordinates in place belief

# 93. Performance Budget

Hardware model is unknown; performance is adaptive.

Initial targets:

- belief update p95 < 500 ms
- visual canvas target 60 FPS
- Vision target 10 FPS adaptive
- `liminald` memory target < 500 MB
- GUI memory target < 1 GB
- no sustained serious/critical thermal state caused by Liminal
- zero network requirement for core operation

Thermal degradation order:

```text
Vision 10 FPS → 5 → 2
visual effect density ↓
Wi-Fi scan rate ↓
background agent jobs pause
active acoustic probes pause
```

Machine stability wins over sensing fidelity.

# 94. Battery Policy

Profiles:

```text
FULL
BALANCED
LOW_POWER
PAUSED
```

Default behavior:

- charger → BALANCED/FULL according to user preference
- battery → BALANCED
- low battery → LOW_POWER
- LOW_POWER disables active acoustics and reduces Vision/scans

# 95. Core Research Questions

**RQ1:** Can stock MacBook passive acoustics distinguish empty vs occupied room state after room-specific calibration?

**RQ2:** Does active acoustic probing add measurable signal beyond passive audio?

**RQ3:** Does aggregate Wi-Fi telemetry add useful independent signal?

**RQ4:** Do BLE recurrence/proximity features improve transition confidence?

**RQ5:** How much performance survives when vision is disabled?

**RQ6:** How quickly does calibration decay when laptop/room geometry changes?

**RQ7:** Can sensor disagreement predict model error?

**RQ8:** Can long-term place memory discover meaningful recurrence without identifying people?

**RQ9:** Can agent interpretation remain epistemically honest when forced to cite machine evidence?

# 96. Experiment Record

Every experiment contains:

```text
hypothesis
session IDs
sensorium profile
space profile
train split
validation split
held-out split
metrics
baseline
result
confidence/failure cases
decision
```

No feature becomes a product claim from one cool demo.

# 97. Required Baselines

For room-state inference compare:

1. majority baseline
2. passive audio only
3. Wi-Fi only
4. BLE only
5. active acoustics only
6. audio + Wi-Fi
7. audio + Wi-Fi + BLE
8. all nonvisual
9. vision teacher/reference

This makes the contribution of every sense visible.

# 98. Initial Promotion Thresholds

For nonvisual binary EMPTY vs OCCUPIED, a provisional “useful” gate is:

```text
balanced accuracy >= 0.75
false occupied <= 0.15
false empty <= 0.15
```

This is a project gate, not a guaranteed capability. If missed, the UI says `EXPERIMENTAL / LOW CONFIDENCE`.

Coarse position has a separate validation gate and may remain disabled.

# 99. False Positive Philosophy

A confident wrong belief is worse than UNKNOWN.

Optimize calibration and honest uncertainty, not maximum forced classification. Unknown rate is a published metric.

# 100. Artistic Philosophy

Liminal's art comes from epistemology, not fake capabilities.

Powerful moments include:

- modalities disagreeing
- a presence forming and dissolving
- memory fading
- a recurring cluster returning
- vision becoming blind
- room calibration breaking
- an interpretation being challenged
- the machine admitting uncertainty

# 101. Visual Grammar for Epistemic Layers

Persistent differentiation:

```text
OBSERVED      sharp / thin / high-frequency marks
INFERRED      translucent probabilistic volumes
INTERPRETED   annotated narrative overlays
IMAGINED      unconstrained generative transformations
```

A screenshot must make the layer identifiable.

# 102. Memory Decay

Detailed sensor-derived information does not survive forever.

```text
DAY 1       high-resolution derived observations
DAY 7       observations + events
MONTH 3     events + episodes + patterns
YEAR 1      patterns + selected episodes + interpretations
```

Raw media is not part of this hierarchy.

# 103. Forgetting

Example:

```bash
liminal privacy erase --range "2026-08-24T09:00..11:00"
```

Deletion algorithm:

1. select source Observations/Beliefs in range
2. identify dependent Events/Episodes/Patterns/Interpretations
3. delete private source content
4. invalidate or recompute dependents
5. retain only a privacy tombstone stating a deletion occurred
6. do not preserve deleted content in logs/artifacts

# 104. Export Modes

```text
SUMMARY
RESEARCH
ART
DEBUG
```

SUMMARY: Events, Patterns, interpretations.

RESEARCH: derived features, labels, model/version metadata; no raw media unless explicitly selected.

ART: renderings/narration, no hidden identifiers.

DEBUG: manual explicit selection only.

# 105. Anonymous Dataset Export

Future research export removes:

- SSID/BSSID
- raw BLE identifiers
- names
- raw video
- raw speech/audio
- account names
- user paths

May include:

- sensor model metadata
- anonymized space type chosen by user
- feature windows
- occupancy labels
- uncertainty
- calibration protocol

# 106. Repository Structure

```text
liminal/
│
├── app/
│   └── Liminal/
│       ├── App/
│       ├── Permissions/
│       ├── Sensors/
│       │   ├── Camera/
│       │   ├── Audio/
│       │   ├── WiFi/
│       │   └── Bluetooth/
│       ├── Vision/
│       ├── Features/
│       ├── IPC/
│       ├── Renderer/
│       ├── Views/
│       └── MenuBar/
│
├── crates/
│   ├── liminal-core/
│   ├── liminal-ledger/
│   ├── liminal-schema/
│   ├── liminal-fusion/
│   ├── liminal-memory/
│   ├── liminal-agents/
│   ├── liminal-policy/
│   ├── liminal-api/
│   ├── liminal-cli/
│   └── liminal-tui/
│
├── proto/
│   └── liminal.proto
│
├── python/
│   └── liminal_lab/
│       ├── acoustics/
│       ├── calibration/
│       ├── models/
│       ├── evaluation/
│       └── notebooks/
│
├── research/
│   └── experiments/
│
├── fixtures/
│   ├── observations/
│   ├── sensorium/
│   └── replay/
│
├── tests/
│   ├── integration/
│   ├── privacy/
│   ├── mutation/
│   ├── chaos/
│   └── acceptance/
│
├── docs/
│   ├── CONSTITUTION.md
│   ├── SENSORIUM.md
│   ├── PRIVACY.md
│   ├── EPISTEMOLOGY.md
│   ├── ARCHITECTURE.md
│   ├── ACOUSTICS.md
│   ├── CALIBRATION.md
│   ├── MEMORY.md
│   ├── AGENTS.md
│   ├── TESTING.md
│   └── RESEARCH.md
│
├── scripts/
│   ├── install.sh
│   ├── uninstall.sh
│   └── smoke.sh
│
├── Cargo.toml
├── Makefile
└── README.md
```

# 107. Configuration

Path:

```text
~/Library/Application Support/Liminal/config.toml
```

Example:

```toml
[space]
active = "studio"

[vision]
enabled = true
target_fps = 10
persist_raw = false

[audio]
passive_enabled = true
persist_raw = false

[acoustic_probe]
enabled = false
quiet_hours_start = "22:00"
quiet_hours_end = "08:00"

[wifi]
enabled = true
scan_interval_seconds = 45
stable_pseudonyms = false

[bluetooth]
enabled = true

[belief]
update_hz = 1

[retention]
observations_days = 7
belief_frames_days = 30
events_days = 365
debug_capture_hours = 24

[agents]
mode = "off"
```

Config cannot turn on normal raw media persistence.

# 108. Database Migration Policy

- numbered migrations
- forward-only for released versions
- empty-DB migration test
- previous-version fixture migration test
- backup before destructive migration
- restore path tested
- schema version included in diagnostics

# 109. Event Replay Requirement

From canonical event log plus model/version artifacts, replay must reconstruct spaces, sensor states, calibration metadata, Events, Episodes, Patterns, and interpretation metadata.

If an old model cannot be retained, an explicit migration invalidates dependent derived data rather than silently recomputing with a different model.

# 110. Logging

Structured logs only.

Never log:

- raw audio
- raw frames
- SSID/BSSID
- BLE names
- secrets

Canonical fields:

```text
timestamp
level
component
event_id
sensor_stream
latency_ms
error_code
```

# 111. Diagnostics Metrics

Local-only by default:

- sensor FPS
- audio underruns
- IPC depth
- observation rate
- belief latency
- dropped messages
- Wi-Fi scan latency
- BLE discovery rate
- DB size
- CPU/memory
- thermal state
- confidence distribution
- UNKNOWN rate

# 112. Stable Error Codes

```text
LIM-CAM-001 permission denied
LIM-CAM-002 device unavailable
LIM-AUD-001 permission denied
LIM-AUD-002 input unavailable
LIM-AUD-003 probe calibration unstable
LIM-WIFI-001 interface unavailable
LIM-WIFI-002 scan failed
LIM-BLE-001 permission denied
LIM-BLE-002 bluetooth off
LIM-IPC-001 schema mismatch
LIM-DB-001 integrity failure
LIM-CAL-001 insufficient calibration
LIM-SPACE-001 anchor invalidated
LIM-AGENT-001 policy violation
```

Never rely solely on error strings.

# 113. Sensor Health Score

Each sensor reports availability, freshness, latency, stability/noise and calibration relevance. Derived health is `0.0–1.0` and fusion weight decays toward zero when stale.

# 114. Master Build Ledger

| Ledger | Deliverable | Exit condition |
|---|---|---|
| **L0** | Constitution | scope/privacy/claims frozen |
| **L1** | Repo + CI | clean build/tests on target Mac |
| **L2** | Sensorium probe | actual hardware capabilities enumerated |
| **L3** | Native permission shell | all TCC flows robust |
| **L4** | IPC/event spine | Swift→Rust derived event flow |
| **L5** | Vision organ | pose/motion features; zero raw persistence |
| **L6** | Passive acoustics | privacy-safe features |
| **L7** | Active acoustics | opt-in calibrated probe |
| **L8** | Wi-Fi organ | aggregate atmosphere features |
| **L9** | BLE organ | pseudonymous proximity clusters |
| **L10** | Space/calibration | repeatable wizard |
| **L11** | Nonvisual baseline | classifier + held-out evaluation |
| **L12** | Fusion | multimodal belief + UNKNOWN |
| **L13** | Event/memory engine | Observation→Pattern |
| **L14** | Native Spectral Canvas | visual machine reality |
| **L15** | TUI | operator/research control |
| **L16** | Field Notes agents | Archivist/Ethnographer/Skeptic |
| **L17** | Historical memory UI | timeline + evidence drilldown |
| **L18** | Privacy hardening | retention/delete/audit |
| **L19** | Chaos/recovery | sleep/crash/permission failure |
| **L20** | 7-day field trial | stability + research report |
| **L21** | Vision-off demo | canonical nonvisual experiment |
| **L22** | Poet/art layer | explicit IMAGINED output |
| **L23** | 30-day memory trial | Patterns + contested interpretation |
| **L24** | Release packaging | reproducible install/uninstall |

# 115. L0 — Constitution

Create:

```text
docs/CONSTITUTION.md
docs/PRIVACY.md
docs/EPISTEMOLOGY.md
docs/CLAIMS.md
```

Freeze:

- no extra hardware
- no CSI claim
- raw-media policy
- epistemic layers
- identity ban
- sensitive-trait ban
- local-first posture
- agents cannot alter privacy policy

CI verifies README cannot contradict these statuses.

# 116. L1 — Repository and CI

Swift:

```text
swiftformat
swiftlint
xcodebuild test
```

Rust:

```text
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo llvm-cov
```

Python:

```text
ruff
pytest
```

Cross-language:

- protobuf compatibility
- DB migrations
- event replay
- privacy tests

Mutation guards protect privacy and epistemic boundaries.

# 117. L2 — Sensorium Probe

`liminal doctor --json` must report actual hardware/permissions, not mocks.

Probe:

- cameras + formats
- Vision 2D support
- Vision 3D request support
- audio input format
- audio output format
- Wi-Fi interface/metrics/scan
- Bluetooth state/auth
- TCC states

# 118. L3 — Permission Shell

Onboarding sequence:

1. explain camera → enable/skip → OS prompt
2. explain microphone → enable/skip → OS prompt
3. explain Bluetooth → enable/skip
4. explain enhanced Wi-Fi Location → only if selected

Acceptance:

- allow
- deny
- restricted
- revoke while open
- restart after decision
- missing usage descriptions caught in build checks

# 119. L4 — IPC/Event Spine

Start with synthetic Swift observations.

Requirements:

- reconnect
- deduplicate by message ID
- detect sequence gaps
- reject schema mismatch
- bounded queues
- backpressure
- no unbounded memory growth

# 120. L5 — Vision Organ

Implement capture, body pose/count, motion regions, scene anchor, optional 3D pose.

Live acceptance scenarios:

- empty
- one person
- partial body
- low light
- camera occluded
- camera busy
- laptop moved
- multiple bodies where detector supports them

Exit: after a normal one-hour session, raw video files on disk = 0.

# 121. L6 — Passive Acoustics

Implement audio tap, features, VAD probability, persistence schema and latency metrics.

Test:

- silence
- typing
- speech
- music
- fan
- footsteps/movement
- chair/object movement

Exit: no PCM files and no speech transcript in canonical storage.

# 122. L7 — Active Acoustics

Implement manual probe, calibration, feature extraction, repeatability score, quiet-hours and VAD suppression.

Do not schedule automatic probes until manual experiments show repeatable value.

Exit: 20 baseline repetitions produce quantified stability and no safety/UX violation.

# 123. L8 — Wi-Fi Organ

Implement RSSI, noise, channel, rate, anonymous scan histograms, Mode A, optional Mode B.

Test:

- Wi-Fi off
- disconnected
- connected
- Location denied
- scan failure
- AP changes
- laptop movement

Exit: Mode A DB contains zero raw SSID/BSSID.

# 124. L9 — BLE Organ

Implement authorization, scan, duplicate events, HMAC pseudonyms, RSSI smoothing and recurrence.

Test Bluetooth off, no devices, rotating IDs, bursty ads, authorization revocation.

Exit: UI never displays discovered human-readable device names by default.

# 125. L10 — Calibration

Wizard persists:

- protocol version
- sensorium profile
- start/end
- teacher labels
- feature stats
- train/val/test boundaries
- model hash

Exit: repeated baseline in same room gives comparable feature distributions within documented tolerance.

# 126. L11 — Nonvisual Baseline

Required report:

```text
class distribution
confusion matrix
balanced accuracy
precision/recall
UNKNOWN rate
transition latency
sensor ablations
failure examples
```

No README claim “detects people without a camera” until held-out evidence meets defined threshold. Before then say: **models changes in the room from nonvisual signals**.

# 127. L12 — Fusion

Implement reliability weights, sensor health decay, temporal smoothing, OOD, UNKNOWN and contradiction.

Acceptance: synthetic contradictory sensor feeds produce `CONTESTED`, not arbitrary consensus.

# 128. L13 — Memory Engine

Implement Observation → Belief → Event → Episode → Pattern before LLM agents.

Exit: 24-hour replay deterministically reproduces Events/Episodes/Patterns.

# 129. L14 — Spectral Canvas

First compelling visual milestone.

Requirements:

- acoustic field
- RF field
- BLE clusters
- fused belief volume
- uncertainty
- modality toggles
- vision-off mode
- target 60 FPS
- no raw camera required

Exit: a screen recording communicates “machine perception” without needing a camera view.

# 130. L15 — TUI

Must include health, sensor rates, live beliefs, events, calibration, privacy, pause, vision toggle and diagnostics.

All critical operations except TCC prompts/debug raw confirmation must be possible from TUI/CLI.

# 131. L16 — Field Notes Agents

Flow:

```text
Archivist draft
      ↓
Skeptic review
      ↓
Ethnographer optional interpretation
      ↓
Skeptic review
      ↓
store
```

Every stored claim has evidence IDs + epistemic layer + confidence/caveat.

# 132. L17 — Historical UI

Implement:

- timeline/day view
- Event drilldown
- provenance graph
- Pattern visualization
- contested interpretations
- date comparison

Exit: user can navigate from interpretation to original Observation IDs.

# 133. L18 — Privacy Hardening

Implement:

- retention worker
- erase
- export
- HMAC-key reset semantics
- privacy audit command
- debug-capture expiration
- network-off verification

Command:

```bash
liminal privacy audit
```

must inspect DB/files for prohibited media/identifiers.

# 134. L19 — Chaos and Recovery

Automated scenarios:

- kill GUI
- kill daemon
- sleep/wake
- camera revoked
- mic revoked
- Wi-Fi disabled
- Bluetooth disabled
- disk temporarily full
- DB locked
- malformed IPC
- clock jump
- corrupted event tail
- thermal degradation simulation

Exit: no silent fabricated continuity or corrupt memory.

# 135. L20 — Seven-Day Field Trial

Conditions:

- one stock MacBook
- one room
- no external hardware
- normal daily use

Collect:

- uptime
- DB growth
- CPU/memory
- thermal state
- sensor availability
- belief coverage
- UNKNOWN rate
- detected events
- sampled false events
- privacy audit

# 136. L21 — Vision-Off Demo

Canonical technical demo:

1. show stock MacBook + Sensorium
2. camera ON for calibration
3. freeze calibration
4. camera OFF
5. leave room
6. return
7. sit
8. move
9. perform a door/object perturbation
10. show nonvisual beliefs
11. reveal reference labels afterward
12. show errors honestly

Technical demo must not edit out failures. Art film may be edited separately but is not benchmark evidence.

# 137. L22 — Poet / Art Layer

Poet receives only approved Events/Episodes/Patterns/Field Notes.

May output:

- short text
- narration
- scene titles
- visual parameters/prompts

Everything stamped `IMAGINED` and excluded from factual evidence.

# 138. L23 — Thirty-Day Trial

Questions:

- do stable Patterns emerge?
- are they explainable?
- how often does calibration stale?
- do agents overinterpret?
- does Skeptic reduce unsupported claims?
- which modalities matter over weeks?
- does memory feel meaningful without identity?

Primary artwork:

> **30 Days in a Room, as Remembered by a MacBook**

# 139. L24 — Packaging

Installer must:

- install/build `liminald`
- install CLI/TUI
- install/open Liminal.app
- create data dirs
- register a user LaunchAgent for `liminald` only
- never auto-grant protected permissions

Uninstaller:

- stop daemon
- remove LaunchAgent
- remove binaries/app
- offer preserve/delete data
- never silently erase memory

# 140. launchd Boundary

Only `liminald` runs as background LaunchAgent. Camera/mic/Wi-Fi enhanced permission access lives in the visible GUI process.

This is deliberate: the daemon may report `SENSOR APP OFFLINE`, but may not attempt hidden camera/mic capture.

# 141. Testing Strategy

## Unit

Feature math, HMAC, fusion, segmentation, retention, agent schemas.

## Contract

Protobuf, config, DB schema.

## Integration

Swift sensor simulator → Rust daemon.

## Acceptance

Actual Mac camera/mic/Wi-Fi/BLE.

## Privacy

Filesystem/DB scanning for prohibited data.

## Mutation

Deliberately weaken boundaries and prove tests fail.

## Chaos

Process/sensor failures.

# 142. Required Mutation Tests

1. write a raw camera frame → privacy suite must fail
2. persist SSID in Mode A → fail
3. remove HMAC from BLE ID → fail
4. let Poet artifact feed Pattern → fail
5. let interpretation agent write OBSERVED claim → fail
6. remove erase-dependent invalidation → fail
7. ignore space-anchor movement → fail
8. enable low-confidence sector localization → fail
9. bridge belief through sensor gap → fail
10. auto-enable camera after user disabled it → fail

The suite is meaningful only if these mutations turn it red.

# 143. Real-Mac Acceptance

CI cannot fake target hardware truth.

Create a live acceptance command such as:

```bash
liminal doctor --live
```

that verifies camera frames, Vision request, microphone samples, speaker output path, CoreWLAN metrics, BLE scan and permission states.

Store only derived acceptance results.

# 144. Research Log

Every experiment:

```text
research/experiments/YYYY-MM-DD-name/
├── README.md
├── hypothesis.md
├── config.json
├── metrics.json
├── result.md
└── plots/
```

No undocumented “seemed to work” claims.

# 145. README Claims Policy

Every feature claim is one of:

```text
WORKING
EXPERIMENTAL
PLANNED
INTENTIONALLY_UNSUPPORTED
```

WORKING requires production wiring, automated tests and live Mac acceptance where hardware dependent.

# 146. Demo Honesty

Never present:

```text
coarse estimated presence
```

as:

```text
tracked person through wall
```

Never render RF perturbation as a measured human body. The art may be surreal; the engineering claim must remain exact.

# 147. Failure Cases the UI Should Celebrate

```text
I DO NOT KNOW
SENSORS DISAGREE
THIS ROOM HAS CHANGED
VISION IS BLIND
RADIO EVIDENCE IS WEAK
THIS MEMORY IS CONTESTED
```

These are part of the artistic identity.

# 148. Anthropology Guardrails

Allowed questions:

- when is the space active?
- where are recurrent coarse activity regions?
- what recurring signals co-occur?
- how do routines shift over time?
- what interpretations are possible?

Forbidden:

- demographic inference
- mental-health diagnosis
- relationship inference as fact
- sexual-behavior inference
- political/religious inference
- identity resolution
- secret monitoring

# 149. Consent Model

Initial assumption: personal experimental installation in a space where operator has permission.

Before sensing, show:

```text
Liminal uses this Mac's camera, microphone, Wi-Fi and Bluetooth
to model environmental changes.

By default:
• video is not saved
• microphone audio is not saved
• speech is not transcribed
• people are not identified
• network/device names are not retained
```

# 150. Multi-Person Ethics

When others are present:

- operator obtains appropriate consent
- sensing indicator remains visible
- vision/mic can be disabled instantly
- Guest Mode is available
- no identity tracking

Public installations require a dedicated consent/signage design.

# 151. Guest Privacy Mode

Effects:

- camera disabled
- active probes disabled
- agents paused
- stable BLE continuity optionally disabled
- only coarse environmental features remain
- memory records reduced-sensing state

# 152. Network Posture

`liminald` uses Unix socket only in V0.1. No TCP server, LAN discovery or cloud sync.

Cloud interpretation is a separately visible optional outbound adapter.

# 153. Agent Audit

Every agent run records:

```text
agent_name
model/provider
input event IDs
input byte count
network mode
output claims
evidence IDs
tokens/cost if available
timestamp
```

No invisible interpretation.

# 154. Model Versioning

```text
model_id
training_session_ids
feature_schema_version
algorithm
parameters_hash
metrics
created_at
status
```

Statuses:

```text
candidate
active
retired
invalidated
```

Laptop movement may invalidate spatially dependent models.

# 155. Sensor Ablation UI

Show counterfactual dependence:

```text
All sensors          occupancy 0.82
minus Bluetooth      occupancy 0.77
minus Wi-Fi          occupancy 0.80
acoustic only        occupancy 0.69
```

Scientific and artistic.

# 156. Counterfactual Belief View

Modes:

```text
ALL
NO VISION
ACOUSTIC ONLY
RADIO ONLY
PROXIMITY ONLY
```

The same moment can be re-evaluated with modalities withheld.

# 157. Ground Truth

Ground truth sources:

1. camera teacher
2. manual annotation
3. scripted experiment steps

Ground truth is stored separately from prediction and never leaked into held-out inference.

# 158. Manual Annotation

Buttons/commands:

```text
EMPTY
SEATED
MOVING
ENTRY
EXIT
OTHER
```

Annotations are research labels, not sensor Observations.

# 159. Visualization Recording

Built-in recording may capture rendered canvas. If REFERENCE camera mode is visible, user must receive an explicit warning before recording.

# 160. Signature Public Demo

Title:

> **What does your room look like when the camera is off?**

Sequence:

1. show ordinary MacBook
2. explicitly show no external hardware
3. Sensorium discovery
4. camera teaches room briefly
5. camera goes OFF
6. SPECTRAL canvas remains active
7. person leaves/returns/moves
8. fields change
9. BELIEF forms and fades
10. sensors disagree at least once
11. show memory timeline
12. show Field Notes
13. end:

> **Liminal does not show what happened. It shows what the machine believes happened—and how uncertain it is.**

# 161. Signature Long-Term Artwork

> **30 Days in a Room, as Remembered by a MacBook**

Inputs:

- one stock MacBook
- no extra sensors
- one room
- 30 days

Outputs:

- spectral film
- temporal traces
- recurring Patterns
- contested Field Notes
- visible blind spots
- explicit imagined layer

Failures remain in the artwork.

# 162. Future — Dreams

Not V0.1.

A nightly process may transform selected Episodes/Patterns into explicitly IMAGINED visual/textual dreams. Dreams never modify factual evidence. Dream salience may propose questions, but factual memory changes only from new evidence.

# 163. Future — Hephaestus Integration

Hephaestus could later optimize non-safety perception harnesses:

Allowed targets:

- fusion weights
- feature subsets
- event segmentation
- calibration strategy
- model selection
- agent prompts

Protected Laws:

- no raw-media default
- no identity
- no sensitive traits
- epistemic separation
- user pause/erase
- no-extra-hardware constitution

# 164. Future — Hera Tie-In

Hera's conceptual DNA appears in provenance, contradiction, retrieval, memory usefulness and archiving.

Possible integration: export selected Field Notes to Hera as human-readable durable knowledge.

Liminal's canonical physical evidence remains in Liminal.

# 165. Future — Iris Tie-In

Iris could become an operator interface:

```text
“what did Liminal notice today?”
“pause Liminal”
“show contested memories”
“why does Liminal think the room is occupied?”
```

Iris must not receive raw sensor media by default. Liminal remains the canonical authority.

# 166. Explicit V0.1 Non-Goals

Do not derail the project with:

- whole-house maps
- Gaussian-splat room reconstruction
- general world model
- Vision Pro
- iOS app
- distributed multi-room nodes
- cloud backend/accounts
- social features
- person identity
- emotion detection
- CSI hacks
- external sensors
- custom hardware
- Kubernetes
- blockchain

The weirdness must come from the stock-MacBook constraint.

# 167. V0.1 Definition

```text
✓ stock MacBook only
✓ Sensorium discovery
✓ camera teacher
✓ raw video not persisted
✓ passive audio features
✓ raw audio not persisted
✓ Wi-Fi aggregate features
✓ BLE pseudonymous features
✓ calibration wizard
✓ held-out nonvisual evaluation
✓ multimodal belief with UNKNOWN
✓ sensor disagreement
✓ Events/Episodes/Patterns
✓ native SPECTRAL canvas
✓ TUI
✓ provenance drilldown
✓ privacy erase
✓ 7-day stability trial
✓ vision-off demo
```

# 168. V0.2 Definition

```text
✓ active acoustics proven useful or explicitly rejected
✓ Archivist
✓ Skeptic
✓ Ethnographer
✓ 30-day memory
✓ recurring Patterns
✓ contested interpretations
✓ ablation UI
✓ stable export
✓ privacy audit
```

# 169. V0.3 Definition

```text
✓ Poet
✓ richer MEMORY renderer
✓ multiple Space Profiles
✓ optional local LLM
✓ model drift handling
✓ calibration-refresh suggestions
✓ research dataset export
```

# 170. V1.0 Definition

> **A stock MacBook can run Liminal for 30 days in a normal room without external sensors, retain no raw camera/audio by default, survive ordinary sleep/wake and sensor failures, produce a reproducible uncertainty-aware history of the space, demonstrate independently measured nonvisual room-state inference after vision calibration, and let a user trace every interpretation back to sensor evidence.**

# 171. North-Star Metrics

Technical:

```text
uptime
sensor availability
belief latency
UNKNOWN rate
classification accuracy
false occupancy
false empty
calibration decay
DB growth
CPU/memory
thermal impact
```

Epistemic:

```text
claims with evidence = 100%
imagined artifacts mislabeled as fact = 0
identity claims = 0
sensitive-trait claims = 0
```

Privacy:

```text
raw video files in normal mode = 0
raw audio files in normal mode = 0
raw SSIDs in default DB = 0
raw BLE names in default DB = 0
```

# 172. Documentation Deliverables

```text
README.md
docs/CONSTITUTION.md
docs/ARCHITECTURE.md
docs/SENSORIUM.md
docs/PRIVACY.md
docs/EPISTEMOLOGY.md
docs/ACOUSTICS.md
docs/WIFI.md
docs/BLUETOOTH.md
docs/CALIBRATION.md
docs/MEMORY.md
docs/AGENTS.md
docs/TESTING.md
docs/RESEARCH.md
docs/OPERATIONS.md
docs/CLAIMS.md
```

# 173. README First Screen

```text
                         LIMINAL

              machine perception at the
                    edge of reality

Liminal lets an ordinary MacBook perceive a room through
vision, acoustics, Wi-Fi and Bluetooth, then build a
persistent memory of what it believes happened.

No added sensors.
No raw camera archive.
No raw microphone archive.
No pretending uncertainty is truth.

[ SPECTRAL SCREENSHOT ]
```

# 174. README Claim Status Table

| Capability | Status | Evidence |
|---|---|---|
| Vision pose | Working | live acceptance |
| Passive acoustic features | Working | live tests |
| Active acoustic occupancy signal | Experimental | experiment receipt |
| Wi-Fi RF perturbation | Working | feature capture |
| Camera-free occupancy classification | Experimental until threshold | benchmark |
| Through-wall tracking | Intentionally not claimed | unsupported |
| Identity | Intentionally absent | constitution |

# 175. Technical Grounding / Sources

These sources establish the feasibility boundaries used by this plan.

## Apple CoreWLAN

Apple documents `CWInterface` access to aggregate RSSI, aggregate noise, scanning, channel, transmit rate and related WLAN state.

https://developer.apple.com/documentation/corewlan/cwinterface

## Apple Vision Human Pose

Apple Vision provides human body pose requests and 3D body pose requests. Depth data can improve 3D results where available; Liminal does not assume the built-in Mac camera provides depth.

https://developer.apple.com/documentation/vision/vndetecthumanbodyposerequest

https://developer.apple.com/documentation/vision/detecthumanbodypose3drequest

## AVAudioEngine

Apple exposes real-time input and output nodes suitable for capture/rendering.

https://developer.apple.com/documentation/avfaudio/avaudioengine

## Media Permission

Camera/microphone require explicit authorization and usage-description configuration on macOS.

https://developer.apple.com/documentation/bundleresources/requesting-authorization-for-media-capture-on-macos

## CoreBluetooth

Peripheral discovery callbacks provide advertisement information and RSSI.

https://developer.apple.com/documentation/corebluetooth/cbcentralmanagerdelegate/centralmanager(_:diddiscover:advertisementdata:rssi:)

## Wi-Fi SSID/BSSID Privacy

Apple Developer Technical Support has documented Location privilege requirements for SSID/BSSID access on modern macOS; stable AP identity is therefore optional in Liminal.

https://developer.apple.com/forums/thread/759044

https://developer.apple.com/forums/thread/732431

## Room Acoustics Research

SoundCam demonstrates that room impulse responses vary with human position and can support human-related inference in research environments. Its richer microphone setup means Liminal treats this as evidence that acoustic signal exists, **not** proof that stock MacBook localization will match the paper.

https://arxiv.org/abs/2311.03517

## CSI Boundary

Nexmon work depends on patched firmware/hardware combinations such as Raspberry Pi variants, illustrating why stock CoreWLAN data must not be marketed as CSI sensing.

https://github.com/seemoo-lab/nexmon

https://github.com/seemoo-lab/nexmon_csi

# 176. Development Decision Register

Resolved unless new evidence forces revision:

- **D001** — Product name is Liminal.
- **D002** — No external sensing hardware.
- **D003** — Swift owns protected/native sensors.
- **D004** — Rust owns canonical state/TUI.
- **D005** — Python is research/offline only.
- **D006** — Raw camera/audio are not persisted by default.
- **D007** — Wi-Fi CSI is not available/required.
- **D008** — Nonvisual V0.1 target is room-state inference, not precise 3D tracking.
- **D009** — Vision acts as calibration teacher.
- **D010** — UNKNOWN is first-class.
- **D011** — Epistemic layers are invariant semantics.
- **D012** — No identity recognition.
- **D013** — No sensitive-trait inference.
- **D014** — Native visual app is first-class.
- **D015** — TUI is first-class.
- **D016** — Cloud is optional and derived-data-only.
- **D017** — Agents operate after deterministic belief/memory.
- **D018** — Interpretations require evidence.
- **D019** — Art cannot become factual evidence.
- **D020** — Laptop movement invalidates spatial calibration.

# 177. Fixed Development Order

```text
CONSTITUTION
     ↓
REAL SENSORIUM DISCOVERY
     ↓
PERMISSIONS
     ↓
IPC + EVENT SPINE
     ↓
VISION TEACHER
     ↓
PASSIVE ACOUSTICS
     ↓
WI-FI
     ↓
BLUETOOTH
     ↓
OPTIONAL ACTIVE ACOUSTICS
     ↓
SPACE CALIBRATION
     ↓
HELD-OUT NONVISUAL BASELINE
     ↓
FUSION + UNKNOWN
     ↓
EVENTS / EPISODES / PATTERNS
     ↓
SPECTRAL CANVAS
     ↓
TUI
     ↓
AGENT FIELD NOTES
     ↓
HISTORICAL MEMORY
     ↓
PRIVACY HARDENING
     ↓
7-DAY FIELD TRIAL
     ↓
VISION-OFF DEMO
     ↓
30-DAY ARTWORK
```

Do not build agents before sensors. Do not build a fake room map before calibration proves spatial information. Do not build generative dreams before factual memory works. Do not claim through-wall perception because a visualization looks convincing.

# 178. Final Product Principle

Every implementation decision must be tested against this sentence:

> **Liminal is compelling because it reveals the difference between reality and machine perception, not because it pretends the difference does not exist.**

The project succeeds when someone looks at the screen and thinks:

> “That is not a camera view of the room.”

> “That is what the machine can sense.”

Then looks at a historical memory and realizes:

> “And it has been remembering this place.”

That is Liminal.
