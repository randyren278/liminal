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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentedEvent {
    pub kind: OccupancyState,
    pub start_ts_us: i64,
    pub end_ts_us: i64,
}

/// A deterministic group of adjacent segmented Events. This is a structural memory record, not
/// a claim about what happened; callers must preserve the source event indices when displaying
/// or persisting it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    pub event_indices: Vec<usize>,
    pub start_ts_us: i64,
    pub end_ts_us: i64,
}

/// A recurrence bucket over Episodes. The signature is intentionally coarse and descriptive;
/// it is not an interpretation or a calibrated behavioral label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pattern {
    pub signature: String,
    pub episode_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryReplay {
    pub events: Vec<SegmentedEvent>,
    pub episodes: Vec<Episode>,
    pub patterns: Vec<Pattern>,
}

const EPISODE_GAP_US: i64 = 30 * 60 * 1_000_000;

/// Replay the bounded Event stream into Episodes and coarse recurrence Patterns. Input order is
/// normalized by timestamp, making a replay of the same persisted Events deterministic.
pub fn replay_memory(events: &[SegmentedEvent]) -> MemoryReplay {
    let mut ordered = events.to_vec();
    ordered.sort_by_key(|event| (event.start_ts_us, event.end_ts_us));
    let mut episodes: Vec<Episode> = Vec::new();
    for (index, event) in ordered.iter().enumerate() {
        if let Some(previous) = episodes.last_mut() {
            if event.kind == ordered[previous.event_indices[0]].kind
                && event.start_ts_us - previous.end_ts_us <= EPISODE_GAP_US
            {
                previous.event_indices.push(index);
                previous.end_ts_us = event.end_ts_us;
                continue;
            }
        }
        episodes.push(Episode {
            event_indices: vec![index],
            start_ts_us: event.start_ts_us,
            end_ts_us: event.end_ts_us,
        });
    }

    let mut pattern_groups = std::collections::BTreeMap::<String, Vec<usize>>::new();
    for (episode_index, episode) in episodes.iter().enumerate() {
        let event = &ordered[episode.event_indices[0]];
        let duration_bucket = (episode.end_ts_us - episode.start_ts_us).div_euclid(60_000_000);
        let signature = format!("{:?}:{}min", event.kind, duration_bucket);
        pattern_groups
            .entry(signature)
            .or_default()
            .push(episode_index);
    }
    let patterns = pattern_groups
        .into_iter()
        .map(|(signature, episode_indices)| Pattern {
            signature,
            episode_indices,
        })
        .collect();
    MemoryReplay {
        events: ordered,
        episodes,
        patterns,
    }
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

/// One operator- or trial-labeled sample for evaluating an occupancy predictor.
///
/// Labels are deliberately external to the live sensor path: this type must never be populated
/// by treating a sensor feature as ground truth. A future field-trial tool can construct these
/// samples from explicit human annotations or another approved reference instrument.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationSample {
    pub predicted_probability: f64,
    pub observed_occupied: bool,
}

/// Bounded metrics for a labeled calibration set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationReport {
    pub sample_count: usize,
    pub accuracy: f64,
    pub brier_score: f64,
    pub positive_precision: f64,
    pub positive_recall: f64,
}

/// Score predictions against explicit occupancy labels without changing the predictor.
///
/// Probabilities are clamped to `[0, 1]` at the evaluation boundary. The 0.5 decision threshold
/// is intentionally explicit and matches the report's accuracy/precision/recall semantics. Empty
/// sets return zero-valued metrics rather than NaN so a UI can safely display "no labels yet".
pub fn evaluate_calibration(samples: &[CalibrationSample]) -> CalibrationReport {
    if samples.is_empty() {
        return CalibrationReport {
            sample_count: 0,
            accuracy: 0.0,
            brier_score: 0.0,
            positive_precision: 0.0,
            positive_recall: 0.0,
        };
    }

    let mut correct = 0usize;
    let mut squared_error = 0.0;
    let mut true_positive = 0usize;
    let mut predicted_positive = 0usize;
    let mut actual_positive = 0usize;

    for sample in samples {
        let probability = sample.predicted_probability.clamp(0.0, 1.0);
        let predicted_occupied = probability >= 0.5;
        if predicted_occupied == sample.observed_occupied {
            correct += 1;
        }
        let label = f64::from(sample.observed_occupied);
        squared_error += (probability - label).powi(2);
        if predicted_occupied {
            predicted_positive += 1;
        }
        if sample.observed_occupied {
            actual_positive += 1;
        }
        if predicted_occupied && sample.observed_occupied {
            true_positive += 1;
        }
    }

    CalibrationReport {
        sample_count: samples.len(),
        accuracy: correct as f64 / samples.len() as f64,
        brier_score: squared_error / samples.len() as f64,
        positive_precision: if predicted_positive == 0 {
            0.0
        } else {
            true_positive as f64 / predicted_positive as f64
        },
        positive_recall: if actual_positive == 0 {
            0.0
        } else {
            true_positive as f64 / actual_positive as f64
        },
    }
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
    fn empty_calibration_set_is_safe_to_display() {
        assert_eq!(
            evaluate_calibration(&[]),
            CalibrationReport {
                sample_count: 0,
                accuracy: 0.0,
                brier_score: 0.0,
                positive_precision: 0.0,
                positive_recall: 0.0,
            }
        );
    }

    #[test]
    fn memory_replay_is_deterministic_and_groups_recurrent_events() {
        let events = vec![
            SegmentedEvent {
                kind: OccupancyState::Occupied,
                start_ts_us: 0,
                end_ts_us: 60_000_000,
            },
            SegmentedEvent {
                kind: OccupancyState::Occupied,
                start_ts_us: 10 * 60_000_000,
                end_ts_us: 11 * 60_000_000,
            },
            SegmentedEvent {
                kind: OccupancyState::Occupied,
                start_ts_us: 2 * 60 * 60_000_000,
                end_ts_us: 3 * 60 * 60_000_000,
            },
        ];

        let first = replay_memory(&events);
        let second = replay_memory(&events);
        assert_eq!(first, second);
        assert_eq!(first.episodes.len(), 2);
        assert_eq!(first.patterns.len(), 2);
        assert_eq!(first.episodes[0].event_indices, vec![0, 1]);
    }

    #[test]
    fn calibration_report_scores_labeled_predictions() {
        let report = evaluate_calibration(&[
            CalibrationSample {
                predicted_probability: 0.9,
                observed_occupied: true,
            },
            CalibrationSample {
                predicted_probability: 0.2,
                observed_occupied: false,
            },
            CalibrationSample {
                predicted_probability: 0.8,
                observed_occupied: false,
            },
            CalibrationSample {
                predicted_probability: 0.4,
                observed_occupied: true,
            },
        ]);

        assert_eq!(report.sample_count, 4);
        assert_eq!(report.accuracy, 0.5);
        assert_eq!(report.positive_precision, 0.5);
        assert_eq!(report.positive_recall, 0.5);
        assert!((report.brier_score - 0.2625).abs() < f64::EPSILON);
    }

    #[test]
    fn calibration_clamps_out_of_range_predictions_before_scoring() {
        let report = evaluate_calibration(&[
            CalibrationSample {
                predicted_probability: 4.0,
                observed_occupied: true,
            },
            CalibrationSample {
                predicted_probability: -2.0,
                observed_occupied: false,
            },
        ]);

        assert_eq!(report.accuracy, 1.0);
        assert_eq!(report.brier_score, 0.0);
    }

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
