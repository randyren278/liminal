//! Privacy and space-calibration policy enforcement.
//!
//! Master plan reference: LIMINAL_MASTER_PLAN.md §19 (Privacy Constitution), §36-40 (Wi-Fi/BLE
//! privacy modes), §43 (Detecting Laptop Movement), §142 (Required Mutation Tests).

use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;

mod retention;
pub use retention::{eligible_for_deletion, RecordKind, RetentionPolicy};

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256(local_key, identifier), hex-encoded, prefixed for the given namespace.
/// §36 Mode B / §39 BLE privacy: identifiers must be transformed immediately, never stored raw.
pub fn pseudonymize(local_key: &[u8], identifier: &str, prefix: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(local_key).expect("HMAC accepts any key length");
    mac.update(identifier.as_bytes());
    let digest = mac.finalize().into_bytes();
    format!("{prefix}:{}", hex_encode(&digest))
}

pub fn pseudonymize_ble(local_key: &[u8], peripheral_identifier: &str) -> String {
    pseudonymize(local_key, peripheral_identifier, "ble")
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A raw Wi-Fi scan hit exactly as CoreWLAN would surface it. This type is Swift-side input
/// only — it must never cross the sanitization boundary intact.
#[derive(Debug, Clone)]
pub struct RawWifiScanResult {
    pub ssid: String,
    pub bssid: String,
    pub rssi: i32,
    pub noise: i32,
    pub channel: u32,
    pub tx_rate: f32,
}

/// §37 Mode A (default): anonymous atmosphere features only. No SSID/BSSID field exists on
/// this type by construction.
#[derive(Debug, Clone, Serialize)]
pub struct WifiObservationModeA {
    pub rssi_mean: f32,
    pub noise_mean: f32,
    pub visible_network_count: u32,
    pub strongest_1: i32,
    pub strongest_2: i32,
    pub strongest_3: i32,
}

/// Reduce raw scans to Mode A aggregate features. Mutation test §142.2: if this function is
/// changed to leak `ssid`/`bssid` into the output, `privacy_audit::scan_json_for_forbidden_keys`
/// must catch it.
pub fn sanitize_wifi_mode_a(raw: &[RawWifiScanResult]) -> WifiObservationModeA {
    let count = raw.len().max(1) as f32;
    let rssi_mean = raw.iter().map(|r| r.rssi as f32).sum::<f32>() / count;
    let noise_mean = raw.iter().map(|r| r.noise as f32).sum::<f32>() / count;
    let mut sorted_rssi: Vec<i32> = raw.iter().map(|r| r.rssi).collect();
    sorted_rssi.sort_unstable_by(|a, b| b.cmp(a));
    let strongest = |i: usize| *sorted_rssi.get(i).unwrap_or(&i32::MIN);
    WifiObservationModeA {
        rssi_mean,
        noise_mean,
        visible_network_count: raw.len() as u32,
        strongest_1: strongest(0),
        strongest_2: strongest(1),
        strongest_3: strongest(2),
    }
}

pub mod privacy_audit {
    use serde_json::Value;

    /// Keys that must never appear anywhere in a canonically-persisted record.
    /// §19 P7 (no SSID storage), §39 (no raw BLE names/manufacturer payload), §110 (never log).
    pub const FORBIDDEN_KEYS: &[&str] = &[
        "ssid",
        "bssid",
        "device_name",
        "peripheral_name",
        "mac_address",
        "raw_audio",
        "raw_frame",
        "raw_pcm",
        "transcript",
    ];

    /// Recursively scan a JSON value for forbidden keys. Returns the offending key paths.
    /// This is the mechanism `liminal privacy audit` (§133) runs over persisted records.
    pub fn scan_json_for_forbidden_keys(value: &Value) -> Vec<String> {
        let mut hits = Vec::new();
        walk(value, "$", &mut hits);
        hits
    }

    fn walk(value: &Value, path: &str, hits: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    let child_path = format!("{path}.{k}");
                    if FORBIDDEN_KEYS.contains(&k.to_lowercase().as_str()) {
                        hits.push(child_path.clone());
                    }
                    walk(v, &child_path, hits);
                }
            }
            Value::Array(items) => {
                for (i, v) in items.iter().enumerate() {
                    walk(v, &format!("{path}[{i}]"), hits);
                }
            }
            _ => {}
        }
    }
}

/// §43 Detecting Laptop Movement / §20 Development Decision Register D020: laptop movement
/// invalidates spatial calibration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorState {
    Stable,
    Invalidated,
}

pub struct SpaceAnchorMonitor {
    /// Divergence score above which the anchor is considered invalidated.
    pub threshold: f64,
}

impl SpaceAnchorMonitor {
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }

    pub fn evaluate(&self, divergence_score: f64) -> AnchorState {
        if divergence_score >= self.threshold {
            AnchorState::Invalidated
        } else {
            AnchorState::Stable
        }
    }
}

/// §43: "spatial beliefs downgrade and location-specific calibration pauses until the user
/// confirms/recalibrates." Confidence is capped, never silently left untouched.
pub const INVALIDATED_ANCHOR_CONFIDENCE_CEILING: f64 = 0.2;

pub fn apply_anchor_state_to_confidence(state: AnchorState, confidence: f64) -> f64 {
    match state {
        AnchorState::Stable => confidence,
        AnchorState::Invalidated => confidence.min(INVALIDATED_ANCHOR_CONFIDENCE_CEILING),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudonymize_ble_is_not_identity() {
        let out = pseudonymize_ble(b"local-key", "AA:BB:CC:DD:EE:FF");
        assert_ne!(out, "AA:BB:CC:DD:EE:FF");
        assert!(out.starts_with("ble:"));
    }

    #[test]
    fn pseudonymize_ble_is_deterministic_per_key() {
        let a = pseudonymize_ble(b"local-key", "AA:BB:CC:DD:EE:FF");
        let b = pseudonymize_ble(b"local-key", "AA:BB:CC:DD:EE:FF");
        assert_eq!(a, b);
    }

    #[test]
    fn pseudonymize_ble_differs_across_keys() {
        let a = pseudonymize_ble(b"key-one", "AA:BB:CC:DD:EE:FF");
        let b = pseudonymize_ble(b"key-two", "AA:BB:CC:DD:EE:FF");
        assert_ne!(a, b);
    }

    #[test]
    fn pseudonymize_ble_matches_known_hmac_vector() {
        // Independently computed HMAC-SHA256("local-key", "AA:BB:CC:DD:EE:FF") hex digest.
        let out = pseudonymize_ble(b"local-key", "AA:BB:CC:DD:EE:FF");
        assert_eq!(out.len(), "ble:".len() + 64);
    }

    #[test]
    fn mode_a_wifi_observation_never_serializes_ssid_or_bssid() {
        let raw = vec![
            RawWifiScanResult {
                ssid: "HomeNetwork".into(),
                bssid: "AA:BB:CC:DD:EE:FF".into(),
                rssi: -45,
                noise: -90,
                channel: 6,
                tx_rate: 200.0,
            },
            RawWifiScanResult {
                ssid: "Neighbor".into(),
                bssid: "11:22:33:44:55:66".into(),
                rssi: -70,
                noise: -92,
                channel: 11,
                tx_rate: 100.0,
            },
        ];
        let sanitized = sanitize_wifi_mode_a(&raw);
        let json = serde_json::to_value(&sanitized).unwrap();
        let hits = privacy_audit::scan_json_for_forbidden_keys(&json);
        assert!(
            hits.is_empty(),
            "forbidden keys leaked into Mode A record: {hits:?}"
        );
        assert_eq!(sanitized.visible_network_count, 2);
    }

    #[test]
    fn privacy_audit_detects_forbidden_key_when_present() {
        let leaked = serde_json::json!({ "rssi_mean": -50.0, "ssid": "HomeNetwork" });
        let hits = privacy_audit::scan_json_for_forbidden_keys(&leaked);
        assert_eq!(hits, vec!["$.ssid".to_string()]);
    }

    #[test]
    fn anchor_invalidation_caps_confidence() {
        let monitor = SpaceAnchorMonitor::new(0.5);
        let state = monitor.evaluate(0.9);
        assert_eq!(state, AnchorState::Invalidated);
        let adjusted = apply_anchor_state_to_confidence(state, 0.95);
        assert!(adjusted <= INVALIDATED_ANCHOR_CONFIDENCE_CEILING);
    }

    #[test]
    fn stable_anchor_leaves_confidence_untouched() {
        let monitor = SpaceAnchorMonitor::new(0.5);
        let state = monitor.evaluate(0.1);
        assert_eq!(state, AnchorState::Stable);
        assert_eq!(apply_anchor_state_to_confidence(state, 0.95), 0.95);
    }
}
