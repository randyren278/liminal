//! Deterministic first-pass fusion for the operator view.
//!
//! This is intentionally a transparent heuristic, not a trained occupancy model. It combines
//! only signals that have a defensible relationship to presence and exposes coverage and
//! disagreement so the CUI never presents a confident guess as a measurement.

use crate::ledger_view::TelemetrySnapshot;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BeliefState {
    Unknown,
    Stable,
    Contested,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeliefSnapshot {
    pub occupancy_probability: f64,
    pub confidence: f64,
    pub disagreement: f64,
    pub observed_modalities: u8,
    pub sensor_health: f64,
    pub state: BeliefState,
}

pub fn derive_belief(telemetry: &TelemetrySnapshot) -> BeliefSnapshot {
    let mut weighted_sum = 0.0;
    let mut total_weight = 0.0;
    let mut signals = Vec::new();

    if let Some(presence) = telemetry.camera_presence {
        weighted_sum += presence * 0.65;
        total_weight += 0.65;
        signals.push(presence);
    }
    if let Some(vad) = telemetry.audio_vad {
        // Acoustic activity is supporting evidence only; it cannot establish a person exists.
        weighted_sum += vad.clamp(0.0, 1.0) * 0.20;
        total_weight += 0.20;
        signals.push(vad.clamp(0.0, 1.0));
    }
    if let Some(clusters) = telemetry.bluetooth_cluster_count {
        let proximity = (clusters / 4.0).clamp(0.0, 1.0);
        weighted_sum += proximity * 0.15;
        total_weight += 0.15;
        signals.push(proximity);
    }

    if total_weight == 0.0 {
        return BeliefSnapshot {
            occupancy_probability: 0.5,
            confidence: 0.0,
            disagreement: 1.0,
            observed_modalities: 0,
            sensor_health: 0.0,
            state: BeliefState::Unknown,
        };
    }
    let probability = weighted_sum / total_weight;
    let min = signals.iter().copied().fold(1.0, f64::min);
    let max = signals.iter().copied().fold(0.0, f64::max);
    let disagreement = max - min;
    BeliefSnapshot {
        occupancy_probability: probability,
        confidence: (total_weight / 1.0 * (1.0 - disagreement * 0.5)).clamp(0.0, 1.0),
        disagreement,
        observed_modalities: signals.len() as u8,
        sensor_health: 1.0,
        state: if disagreement > 0.45 {
            BeliefState::Contested
        } else {
            BeliefState::Stable
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_sensor_evidence_is_unknown_and_has_zero_confidence() {
        let belief = derive_belief(&TelemetrySnapshot::default());
        assert_eq!(belief.occupancy_probability, 0.5);
        assert_eq!(belief.confidence, 0.0);
        assert_eq!(belief.observed_modalities, 0);
    }

    #[test]
    fn camera_presence_is_stronger_than_supporting_modalities() {
        let belief = derive_belief(&TelemetrySnapshot {
            camera_presence: Some(1.0),
            audio_vad: Some(0.0),
            ..TelemetrySnapshot::default()
        });
        assert!(belief.occupancy_probability > 0.7);
        assert_eq!(belief.observed_modalities, 2);
        assert!(belief.disagreement > 0.0);
        assert_eq!(belief.state, BeliefState::Contested);
    }

    #[test]
    fn bluetooth_participates_and_agreeing_modalities_are_stable() {
        let belief = derive_belief(&TelemetrySnapshot {
            camera_presence: Some(0.5),
            audio_vad: Some(0.5),
            bluetooth_cluster_count: Some(2.0),
            ..TelemetrySnapshot::default()
        });
        assert_eq!(belief.observed_modalities, 3);
        assert_eq!(belief.occupancy_probability, 0.5);
        assert_eq!(belief.disagreement, 0.0);
        assert_eq!(belief.state, BeliefState::Stable);
        assert_eq!(belief.sensor_health, 1.0);
    }

    #[test]
    fn supporting_modalities_are_clamped_to_their_valid_probability_range() {
        let belief = derive_belief(&TelemetrySnapshot {
            audio_vad: Some(2.0),
            bluetooth_cluster_count: Some(-4.0),
            ..TelemetrySnapshot::default()
        });
        assert_eq!(belief.observed_modalities, 2);
        assert!(belief.occupancy_probability > 0.5);
        assert!(belief.occupancy_probability < 0.6);
        assert_eq!(belief.state, BeliefState::Contested);
    }

    #[test]
    fn wifi_alone_does_not_create_an_occupancy_belief() {
        let belief = derive_belief(&TelemetrySnapshot {
            wifi_rssi_mean: Some(-45.0),
            wifi_network_count: Some(8.0),
            ..TelemetrySnapshot::default()
        });
        assert_eq!(belief.confidence, 0.0);
        assert_eq!(belief.observed_modalities, 0);
    }
}
