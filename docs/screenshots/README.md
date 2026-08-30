# Screenshot provenance

The refreshed live-field image is supporting evidence for the current TUI
contract. It is a terminal capture of the Rust TUI reading the local SQLite
ledger, not a camera frame, raw audio capture, or synthetic demo render.
Mode-specific captures are omitted until they can be made without unrelated
desktop content.

Capture date: 2026-08-29 (America/Vancouver)

Refreshed capture source: `eb4b9ea` (`Refresh live TUI documentation and
launcher reliability`). The live-field image was captured from the equivalent
working tree immediately before that commit. Terminal: macOS Terminal,
halfblock compatibility renderer.

| Image | Surface | Launch / evidence | Sensor state | Live/demo | Raw media |
|---|---|---|---|---|---|
| [`liminal-live-field-live.png`](liminal-live-field-live.png) | `1 LIVE FIELD` | `scripts/run-liminal.sh`; direct macOS Terminal capture | Camera, microphone, Wi-Fi, and Bluetooth-derived observations were visible in this capture; a separate doctor probe recorded a Keychain-unavailable Bluetooth startup path | Live | Never captured or persisted |

The two older images, [`liminal-tui-terminal-live.png`](liminal-tui-terminal-live.png)
and [`liminal-tui-art-demo.png`](liminal-tui-art-demo.png), are retained as
historical renderer evidence. The latter is explicitly synthetic demo output;
neither is used as the README hero or as proof of live sensor delivery.
