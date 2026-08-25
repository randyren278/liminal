//! The persisted `SensoriumProfile` — a snapshot of what sensing capability the current
//! machine actually has, per sensor.
//!
//! Master plan reference: LIMINAL_MASTER_PLAN.md §3 (Hardware Constitution — sensor state
//! enum), §22 (Sensorium Profile Schema).

use serde::{Deserialize, Serialize};

/// The state of a single sensor, per §3. Liminal must never assume a sensor's capability;
/// it discovers and records this state at runtime instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorState {
    Unknown,
    Probing,
    Available,
    Degraded,
    Denied,
    Busy,
    Unsupported,
    Failed,
    DisabledByUser,
}

/// Camera organ state, per §22.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraProfile {
    pub state: SensorState,
    pub device_id_hash: String,
    pub selected_resolution: (u32, u32),
    pub selected_fps: u32,
    pub depth_data: bool,
}

/// Shared shape for `audio_input` and `audio_output`, per §22.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioProfile {
    pub state: SensorState,
    pub sample_rate: u32,
    pub channels: u32,
}

/// Wi-Fi radio atmosphere organ state, per §22.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WifiProfile {
    pub state: SensorState,
    pub aggregate_rssi: bool,
    pub aggregate_noise: bool,
    pub scanning: bool,
    pub stable_ap_ids: bool,
    pub csi: bool,
}

/// Bluetooth proximity atmosphere organ state, per §22.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BluetoothProfile {
    pub state: SensorState,
    pub scan_rssi: bool,
}

/// The full persisted Sensorium snapshot for a machine, per §22.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensoriumProfile {
    pub schema_version: u32,
    pub machine_profile_id: String,
    pub created_at: String,
    pub camera: CameraProfile,
    pub audio_input: AudioProfile,
    pub audio_output: AudioProfile,
    pub wifi: WifiProfile,
    pub bluetooth: BluetoothProfile,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const SECTION_22_EXAMPLE: &str = r#"
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
    "#;

    #[test]
    fn section_22_example_round_trips_structurally() {
        let profile: SensoriumProfile = serde_json::from_str(SECTION_22_EXAMPLE).unwrap();
        let round_tripped = serde_json::to_value(&profile).unwrap();
        let original: Value = serde_json::from_str(SECTION_22_EXAMPLE).unwrap();
        assert_eq!(round_tripped, original);
    }

    #[test]
    fn each_sensor_state_variant_serializes_to_its_snake_case_string() {
        let variants = [
            (SensorState::Unknown, "unknown"),
            (SensorState::Probing, "probing"),
            (SensorState::Available, "available"),
            (SensorState::Degraded, "degraded"),
            (SensorState::Denied, "denied"),
            (SensorState::Busy, "busy"),
            (SensorState::Unsupported, "unsupported"),
            (SensorState::Failed, "failed"),
            (SensorState::DisabledByUser, "disabled_by_user"),
        ];

        for (state, expected) in variants {
            let profile = SensoriumProfile {
                schema_version: 1,
                machine_profile_id: "machine:test".to_string(),
                created_at: "2026-08-25T00:00:00Z".to_string(),
                camera: CameraProfile {
                    state,
                    device_id_hash: "hash".to_string(),
                    selected_resolution: (1280, 720),
                    selected_fps: 10,
                    depth_data: false,
                },
                audio_input: AudioProfile {
                    state: SensorState::Available,
                    sample_rate: 48000,
                    channels: 1,
                },
                audio_output: AudioProfile {
                    state: SensorState::Available,
                    sample_rate: 48000,
                    channels: 2,
                },
                wifi: WifiProfile {
                    state: SensorState::Available,
                    aggregate_rssi: true,
                    aggregate_noise: true,
                    scanning: true,
                    stable_ap_ids: false,
                    csi: false,
                },
                bluetooth: BluetoothProfile {
                    state: SensorState::Available,
                    scan_rssi: true,
                },
            };

            let value = serde_json::to_value(&profile).unwrap();
            assert_eq!(value["camera"]["state"], json!(expected));
        }
    }
}
