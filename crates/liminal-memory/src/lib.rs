//! Belief → Event segmentation: turns a stream of occupancy-probability samples into bounded
//! occupancy sessions using hysteresis, minimum durations, and gap merging.
//!
//! Master plan reference: LIMINAL_MASTER_PLAN.md §56 (Memory Hierarchy), §57 (Event),
//! §58 (Event Segmentation).

use serde::{Deserialize, Serialize};

/// Confirmed occupancy classification. `Unknown` is the state before enough samples have been
/// observed to confirm either `Occupied` or `Empty` (§58: segmentation starts with no belief).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OccupancyState {
    Unknown,
    Empty,
    Occupied,
}

/// A bounded occupancy session, §57's `occupancy_transition` Event kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentedEvent {
    pub kind: OccupancyState,
    pub start_ts_us: i64,
    pub end_ts_us: i64,
}

/// §58 thresholds, tunable per "Calibration may tune them." Defaults are the master plan's
/// initial occupancy defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentationConfig {
    /// Minimum `occupied_probability` to count a sample toward an enter run.
    pub enter_threshold: f64,
    /// How long an enter run must sustain, in microseconds, before Occupied is confirmed.
    pub enter_duration_us: i64,
    /// Minimum empty-confidence (i.e. `occupied_probability <= 1.0 - exit_confidence`) to count
    /// a sample toward an exit run.
    pub exit_confidence: f64,
    /// How long an exit run must sustain, in microseconds, before Empty is confirmed.
    pub exit_duration_us: i64,
    /// Two confirmed Occupied events separated by a gap no larger than this are merged into one,
    /// per §58's "gap merging" (e.g. a person stepping briefly out of sensor range mid-session).
    pub gap_merge_us: i64,
}

impl Default for SegmentationConfig {
    fn default() -> Self {
        Self {
            enter_threshold: 0.70,
            enter_duration_us: 3_000_000,
            exit_confidence: 0.80,
            exit_duration_us: 5_000_000,
            gap_merge_us: 5_000_000,
        }
    }
}

/// Segments an ordered sequence of `(timestamp_us, occupied_probability)` samples into
/// Occupied `SegmentedEvent`s using §58 hysteresis.
///
/// A run of samples meeting `enter_threshold` must sustain for `enter_duration_us` before an
/// Occupied session is confirmed; a run meeting `exit_confidence` must sustain for
/// `exit_duration_us` before it ends. An event's `start_ts_us`/`end_ts_us` mark the actual onset
/// of the run (the first/last sample past the threshold), not the later moment the transition
/// was confirmed. A duration exactly equal to the configured minimum counts as sustained (`>=`).
/// If the stream ends mid-Occupied-session, the event closes at the last observed sample.
pub fn segment_occupancy(
    samples: &[(i64, f64)],
    config: &SegmentationConfig,
) -> Vec<SegmentedEvent> {
    let mut events = Vec::new();
    let mut confirmed_state = OccupancyState::Unknown;
    let mut event_start_ts: Option<i64> = None;
    let mut pending_run_start: Option<i64> = None;

    for &(ts, probability) in samples {
        let wants_occupied = probability >= config.enter_threshold;
        let wants_empty = probability <= 1.0 - config.exit_confidence;

        match confirmed_state {
            OccupancyState::Occupied => {
                if wants_empty {
                    let run_start = *pending_run_start.get_or_insert(ts);
                    if ts - run_start >= config.exit_duration_us {
                        events.push(SegmentedEvent {
                            kind: OccupancyState::Occupied,
                            start_ts_us: event_start_ts.expect("Occupied implies event started"),
                            end_ts_us: run_start,
                        });
                        confirmed_state = OccupancyState::Empty;
                        event_start_ts = None;
                        pending_run_start = None;
                    }
                } else {
                    pending_run_start = None;
                }
            }
            OccupancyState::Empty | OccupancyState::Unknown => {
                if wants_occupied {
                    let run_start = *pending_run_start.get_or_insert(ts);
                    if ts - run_start >= config.enter_duration_us {
                        confirmed_state = OccupancyState::Occupied;
                        event_start_ts = Some(run_start);
                        pending_run_start = None;
                    }
                } else {
                    pending_run_start = None;
                }
            }
        }
    }

    if confirmed_state == OccupancyState::Occupied {
        if let (Some(start), Some(&(last_ts, _))) = (event_start_ts, samples.last()) {
            events.push(SegmentedEvent {
                kind: OccupancyState::Occupied,
                start_ts_us: start,
                end_ts_us: last_ts,
            });
        }
    }

    merge_gaps(events, config.gap_merge_us)
}

/// §58 gap merging: collapse consecutive same-kind events separated by a gap no larger than
/// `gap_merge_us` into a single event spanning both.
fn merge_gaps(events: Vec<SegmentedEvent>, gap_merge_us: i64) -> Vec<SegmentedEvent> {
    let mut merged: Vec<SegmentedEvent> = Vec::with_capacity(events.len());
    for event in events {
        if let Some(last) = merged.last_mut() {
            if event.kind == last.kind && event.start_ts_us - last.end_ts_us <= gap_merge_us {
                last.end_ts_us = event.end_ts_us;
                continue;
            }
        }
        merged.push(event);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_transition_produces_one_occupied_event() {
        let samples = [
            (0, 0.05),
            (1_000_000, 0.05),
            (2_000_000, 0.95),
            (3_000_000, 0.95),
            (4_000_000, 0.95),
            (5_000_000, 0.95),
            (6_000_000, 0.95),
            (7_000_000, 0.95),
        ];
        let events = segment_occupancy(&samples, &SegmentationConfig::default());
        assert_eq!(
            events,
            vec![SegmentedEvent {
                kind: OccupancyState::Occupied,
                start_ts_us: 2_000_000,
                end_ts_us: 7_000_000,
            }]
        );
    }

    #[test]
    fn flicker_below_hysteresis_window_produces_no_event() {
        // Probability spikes above 0.70 for only 1 consecutive second, then drops back:
        // never sustains the required 3 seconds, so no transition is confirmed.
        let samples = [
            (0, 0.10),
            (1_000_000, 0.90),
            (2_000_000, 0.90),
            (3_000_000, 0.10),
            (4_000_000, 0.10),
        ];
        let events = segment_occupancy(&samples, &SegmentationConfig::default());
        assert!(events.is_empty());
    }

    #[test]
    fn brief_gap_inside_occupied_session_is_merged() {
        // Fast thresholds (1s enter/exit) keep the sample count small while still exercising two
        // full hysteresis cycles: a real exit is confirmed and a real re-entry follows 3 seconds
        // later. That gap is well inside gap_merge_us, so the two sessions merge into one.
        let config = SegmentationConfig {
            enter_duration_us: 1_000_000,
            exit_duration_us: 1_000_000,
            ..SegmentationConfig::default()
        };
        let samples = [
            (0, 0.9),
            (1_000_000, 0.9), // enter confirmed, start = 0
            (2_000_000, 0.9),
            (3_000_000, 0.1),
            (4_000_000, 0.1), // exit confirmed, end = 3s
            (5_000_000, 0.1),
            (6_000_000, 0.9),
            (7_000_000, 0.9), // enter confirmed, start = 6s (gap of 3s since last end)
            (8_000_000, 0.9),
            (9_000_000, 0.1),
            (10_000_000, 0.1), // exit confirmed, end = 9s
        ];
        let events = segment_occupancy(&samples, &config);
        assert_eq!(
            events,
            vec![SegmentedEvent {
                kind: OccupancyState::Occupied,
                start_ts_us: 0,
                end_ts_us: 9_000_000,
            }]
        );
    }

    #[test]
    fn exact_threshold_sustained_for_exact_duration_counts_as_met() {
        // Probability sits at exactly 0.70 (the enter threshold) for exactly 3.0 seconds of
        // samples. This crate treats both the probability threshold and the duration threshold
        // as inclusive (`>=`), so this counts as a confirmed transition.
        let samples = [
            (0, 0.70),
            (1_000_000, 0.70),
            (2_000_000, 0.70),
            (3_000_000, 0.70),
        ];
        let events = segment_occupancy(&samples, &SegmentationConfig::default());
        assert_eq!(
            events,
            vec![SegmentedEvent {
                kind: OccupancyState::Occupied,
                start_ts_us: 0,
                end_ts_us: 3_000_000,
            }]
        );
    }
}
