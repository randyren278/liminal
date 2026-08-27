//! Reads real data out of `liminal-ledger`'s SQLite store for display -- this is what makes
//! REFERENCE mode show something other than the roadmap-item-1 demo pattern once a real sensor
//! organ (today: `liminal-capture`'s Vision organ) has written data.
//!
//! Correction to the original ROADMAP.md wording for this item: it described "the live camera
//! frame reference view" with a pose overlay. That would require a raw camera frame to exist in
//! the ledger, which §120's exit criterion (zero raw video files) and the whole architecture's
//! Swift->Rust contract (derived features only, §14) both forbid -- no raw frame is ever
//! persisted or transmitted for this to render. What's shown instead is a real skeleton derived
//! purely from the stored `PoseObservation` JSON (joint x/y/confidence) -- genuine sensor-derived
//! data, just not pixels. That's consistent with this project's own epistemic-layer distinction
//! (§101: OBSERVED marks are sharp/thin, not photographic).

use crate::belief::BeliefState;
use liminal_ledger::{Event, SqliteLedger, DEFAULT_MAX_SILENT_GAP_US};
use liminal_memory::{
    evaluate_calibration, replay_memory, segment_occupancy, CalibrationSample, SegmentedEvent,
};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct LedgerSnapshot {
    pub total_event_count: usize,
    pub sensor_gap_count: usize,
    /// Streams whose latest persisted state still blocks daemon fusion.
    pub pending_gap_streams: Vec<String>,
    pub belief_count: usize,
    /// Number of persisted Tier-0 agent runs available to FIELD NOTES.
    pub agent_run_count: usize,
    /// Newest persisted Tier-0 outputs, bounded for a terminal card.
    pub agent_runs: Vec<AgentRunSummary>,
    /// Number of observations per sensor stream, in stable display order.
    pub stream_event_counts: BTreeMap<String, usize>,
    /// Newest timestamped observation per stream, used to prevent stale data looking live.
    pub latest_observation_timestamps: BTreeMap<String, i64>,
    /// The most recently appended event's kind and stream, if one exists.
    pub latest_event: Option<(String, String)>,
    /// The newest persisted record, including explicit derivation sources for drill-down.
    pub latest_record: Option<LatestRecord>,
    /// A bounded newest-first record index for read-only historical inspection in the CUI.
    pub recent_records: Vec<LatestRecord>,
    /// (x, y, confidence) for each joint in the most recent `camera` stream observation, if any.
    pub latest_camera_joints: Vec<(f64, f64, f64)>,
    /// The latest derived feature values available for each live organ. Missing values mean that
    /// the organ has not emitted an observation yet, not that the sensor measured zero.
    pub telemetry: TelemetrySnapshot,
    /// Most recent daemon-produced fusion result, if one has been persisted.
    pub persisted_belief: Option<PersistedBelief>,
    pub recent_observations: Vec<RecentObservation>,
    /// Compact full-ledger history, newest day first. Buckets count observations only; they do
    /// not imply continuous coverage between observations.
    pub historical_buckets: Vec<HistoricalBucket>,
    /// Confirmed occupancy sessions projected from daemon fusion beliefs. This is read-only and
    /// remains empty until the hysteresis thresholds are actually met.
    pub occupancy_events: Vec<SegmentedEvent>,
    pub episode_count: usize,
    pub pattern_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestRecord {
    pub id: String,
    pub kind: String,
    pub stream: Option<String>,
    pub timestamp_us: Option<i64>,
    pub provenance_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentObservation {
    pub stream: String,
    pub timestamp_us: i64,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalBucket {
    pub day_index: i64,
    pub observation_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TelemetrySnapshot {
    pub camera_presence: Option<f64>,
    pub audio_rms: Option<f64>,
    pub audio_centroid_hz: Option<f64>,
    pub audio_vad: Option<f64>,
    pub wifi_rssi_mean: Option<f64>,
    pub wifi_noise_mean: Option<f64>,
    pub wifi_network_count: Option<f64>,
    pub bluetooth_cluster_count: Option<f64>,
    pub bluetooth_mean_rssi: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersistedBelief {
    pub occupancy_probability: f64,
    pub confidence: f64,
    pub disagreement: f64,
    pub observed_modalities: u8,
    pub sensor_health: f64,
    pub state: BeliefState,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunSummary {
    pub role: String,
    pub layer: String,
    pub status: String,
    pub evidence_count: usize,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationReportView {
    pub labels_total: usize,
    pub matched_labels: usize,
    pub unmatched_labels: usize,
    pub accuracy: f64,
    pub brier_score: f64,
    pub positive_precision: f64,
    pub positive_recall: f64,
}

/// Pure extraction: given the full event list, find the most recent `camera`-stream observation
/// and pull its joints back out of the JSON shape `liminald::ingest_envelope` stored them in
/// (`{"stream_id", "ts_us", "features": {"joints": [{"x","y","confidence"}, ...]}}`).
pub fn extract_latest_camera_joints(events: &[Event]) -> Vec<(f64, f64, f64)> {
    events
        .iter()
        .rev()
        .find(|e| e.payload.get("stream_id").and_then(|v| v.as_str()) == Some("camera"))
        .and_then(|e| e.payload.get("features"))
        .and_then(|f| f.get("joints"))
        .and_then(|j| j.as_array())
        .map(|joints| {
            joints
                .iter()
                .filter_map(|j| {
                    let x = j.get("x")?.as_f64()?;
                    let y = j.get("y")?.as_f64()?;
                    let confidence = j.get("confidence")?.as_f64()?;
                    Some((x, y, confidence))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn latest_features<'a>(events: &'a [Event], stream: &str) -> Option<&'a serde_json::Value> {
    events
        .iter()
        .rev()
        .find(|event| {
            event.kind == "observation"
                && event.payload.get("stream_id").and_then(|v| v.as_str()) == Some(stream)
        })
        .and_then(|event| event.payload.get("features"))
}

fn number(features: Option<&serde_json::Value>, key: &str) -> Option<f64> {
    features?.get(key)?.as_f64()
}

fn average_rssi(features: Option<&serde_json::Value>) -> Option<f64> {
    let clusters = features?.get("clusters")?.as_array()?;
    if clusters.is_empty() {
        return None;
    }
    Some(
        clusters
            .iter()
            .filter_map(|cluster| cluster.get("rssi")?.as_f64())
            .sum::<f64>()
            / clusters.len() as f64,
    )
}

fn record_summary(event: &Event, ledger: &SqliteLedger) -> LatestRecord {
    LatestRecord {
        id: event.id.clone(),
        kind: event.kind.clone(),
        stream: event
            .payload
            .get("stream_id")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        timestamp_us: event
            .payload
            .get("ts_us")
            .and_then(|value| value.as_i64())
            .or_else(|| {
                event
                    .payload
                    .get("timestamp_us")
                    .and_then(|value| value.as_i64())
            }),
        provenance_sources: ledger.provenance_sources(&event.id).unwrap_or_default(),
    }
}

pub fn extract_telemetry(events: &[Event]) -> TelemetrySnapshot {
    let audio = latest_features(events, "microphone");
    let wifi = latest_features(events, "wifi");
    let bluetooth = latest_features(events, "bluetooth");
    TelemetrySnapshot {
        camera_presence: latest_features(events, "camera").and_then(|features| {
            match features.get("body_count")?.as_str()? {
                "zero" => Some(0.0),
                "one" => Some(1.0),
                "two_or_more" => Some(1.0),
                _ => None,
            }
        }),
        audio_rms: number(audio, "rms"),
        audio_centroid_hz: number(audio, "spectral_centroid_hz"),
        audio_vad: number(audio, "voice_activity_probability"),
        wifi_rssi_mean: number(wifi, "rssi_mean"),
        wifi_noise_mean: number(wifi, "noise_mean"),
        wifi_network_count: number(wifi, "visible_network_count"),
        bluetooth_cluster_count: number(bluetooth, "cluster_count"),
        bluetooth_mean_rssi: average_rssi(bluetooth),
    }
}

pub fn extract_latest_belief(events: &[Event]) -> Option<PersistedBelief> {
    let belief_event = events.iter().rev().find(|event| {
        event.kind == "belief"
            && event.payload.get("stream_id").and_then(|v| v.as_str()) == Some("fusion")
    })?;
    let features = belief_event.payload.get("features")?;
    Some(PersistedBelief {
        occupancy_probability: features.get("occupancy_probability")?.as_f64()?,
        confidence: features.get("confidence")?.as_f64()?,
        disagreement: features.get("disagreement")?.as_f64()?,
        observed_modalities: features.get("observed_modalities")?.as_u64()? as u8,
        sensor_health: features.get("sensor_health")?.as_f64()?,
        state: match features.get("state")?.as_str()? {
            "stable" => BeliefState::Stable,
            "contested" => BeliefState::Contested,
            "unknown" => BeliefState::Unknown,
            _ => return None,
        },
        evidence_ids: belief_event
            .payload
            .get("derived_from")?
            .as_array()?
            .iter()
            .filter_map(|value| value.as_str().map(ToString::to_string))
            .collect(),
    })
}

pub fn extract_recent_observations(events: &[Event], limit: usize) -> Vec<RecentObservation> {
    events
        .iter()
        .filter_map(|event| {
            let stream = event.payload.get("stream_id")?.as_str()?.to_string();
            let timestamp_us = event.payload.get("ts_us")?.as_i64()?;
            Some(RecentObservation {
                stream,
                timestamp_us,
                kind: event.kind.clone(),
            })
        })
        .rev()
        .take(limit)
        .collect()
}

pub fn extract_latest_observation_timestamps(events: &[Event]) -> BTreeMap<String, i64> {
    let mut latest: BTreeMap<String, i64> = BTreeMap::new();
    for observation in events.iter().filter(|event| event.kind == "observation") {
        let Some(stream) = observation
            .payload
            .get("stream_id")
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        let Some(timestamp_us) = observation
            .payload
            .get("ts_us")
            .and_then(|value| value.as_i64())
        else {
            continue;
        };
        latest
            .entry(stream.to_string())
            .and_modify(|current| *current = (*current).max(timestamp_us))
            .or_insert(timestamp_us);
    }
    latest
}

/// Estimate recent observation rates from persisted timestamps. A rate is only shown for a
/// stream with a non-zero observed span; a single observation is represented as zero rather than
/// pretending that one sample establishes a cadence.
pub fn extract_recent_observation_rates(
    observations: &[RecentObservation],
) -> BTreeMap<String, f64> {
    let mut ranges = BTreeMap::<String, (i64, i64, usize)>::new();
    for observation in observations {
        let entry = ranges.entry(observation.stream.clone()).or_insert((
            observation.timestamp_us,
            observation.timestamp_us,
            0,
        ));
        entry.0 = entry.0.min(observation.timestamp_us);
        entry.1 = entry.1.max(observation.timestamp_us);
        entry.2 += 1;
    }
    ranges
        .into_iter()
        .map(|(stream, (oldest, newest, count))| {
            let span_seconds = (newest - oldest).max(0) as f64 / 1_000_000.0;
            let rate = if span_seconds > 0.0 {
                count as f64 / span_seconds
            } else {
                0.0
            };
            (stream, rate)
        })
        .collect()
}

const DAY_US: i64 = 86_400_000_000;

/// Summarize the full timestamped observation history into bounded day buckets. The day index is
/// an epoch-day value for deterministic grouping; callers should display relative age rather than
/// turning this into an unverified calendar claim. Only the latest `max_days` buckets are kept.
pub fn extract_historical_buckets(events: &[Event], max_days: usize) -> Vec<HistoricalBucket> {
    let mut counts = BTreeMap::<i64, usize>::new();
    for event in events {
        if event.kind != "observation" {
            continue;
        }
        if let Some(timestamp_us) = event.payload.get("ts_us").and_then(|value| value.as_i64()) {
            *counts.entry(timestamp_us.div_euclid(DAY_US)).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .rev()
        .take(max_days)
        .map(|(day_index, observation_count)| HistoricalBucket {
            day_index,
            observation_count,
        })
        .collect()
}

/// Project only daemon-owned fusion beliefs into the memory crate's hysteresis segmenter. Raw
/// observations, render fallbacks, and other belief streams are deliberately excluded so the
/// CUI cannot turn an uncalibrated display guess into a confirmed occupancy Event.
pub fn extract_occupancy_events(events: &[Event]) -> Vec<SegmentedEvent> {
    let mut samples: Vec<(i64, f64)> = events
        .iter()
        .filter(|event| {
            event.kind == "belief"
                && event.payload.get("stream_id").and_then(|v| v.as_str()) == Some("fusion")
        })
        .filter_map(|event| {
            let timestamp_us = event.payload.get("ts_us")?.as_i64()?;
            let probability = event
                .payload
                .get("features")?
                .get("occupancy_probability")?
                .as_f64()?;
            Some((timestamp_us, probability))
        })
        .collect();
    samples.sort_unstable_by_key(|(timestamp_us, _)| *timestamp_us);
    segment_occupancy(&samples, &Default::default())
}

/// Prefer explicitly materialized structural records when an operator has run `memory replay`;
/// otherwise return `None` so the CUI can continue showing the deterministic read-only replay.
pub fn extract_persisted_memory_counts(events: &[Event]) -> Option<(usize, usize)> {
    let episode_count = events
        .iter()
        .filter(|event| event.kind == "episode")
        .count();
    let pattern_count = events
        .iter()
        .filter(|event| event.kind == "pattern")
        .count();
    (episode_count > 0 || pattern_count > 0).then_some((episode_count, pattern_count))
}

pub fn extract_agent_runs(events: &[Event], limit: usize) -> Vec<AgentRunSummary> {
    events
        .iter()
        .rev()
        .filter(|event| event.kind == "agent_run")
        .filter_map(|event| {
            let output = event.payload.get("output")?;
            Some(AgentRunSummary {
                role: event.payload.get("agent_name")?.as_str()?.to_string(),
                layer: output.get("layer")?.as_str()?.to_string(),
                status: output.get("status")?.as_str()?.to_string(),
                evidence_count: event.payload.get("evidence_ids")?.as_array()?.len(),
                text: output.get("text")?.as_str()?.to_string(),
            })
        })
        .take(limit)
        .collect()
}

pub fn read_calibration_report_checked(
    db_path: &Path,
    labels_path: &Path,
) -> Result<Option<CalibrationReportView>, String> {
    let text = std::fs::read_to_string(labels_path)
        .map_err(|error| format!("read labels {}: {error}", labels_path.display()))?;
    let labels: Vec<(i64, bool)> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(line_number, line)| {
            let value: serde_json::Value = serde_json::from_str(line)
                .map_err(|error| format!("parse labels line {}: {error}", line_number + 1))?;
            let timestamp = value
                .get("ts_us")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| {
                    format!("labels line {} is missing integer ts_us", line_number + 1)
                })?;
            let occupied = value
                .get("occupied")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| {
                    format!(
                        "labels line {} is missing boolean occupied",
                        line_number + 1
                    )
                })?;
            Ok((timestamp, occupied))
        })
        .collect::<Result<_, String>>()?;
    let ledger = SqliteLedger::open(db_path, DEFAULT_MAX_SILENT_GAP_US)
        .map_err(|error| error.to_string())?;
    let beliefs: Vec<(i64, f64)> = ledger
        .events()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|event| {
            event.kind == "belief"
                && event
                    .payload
                    .get("stream_id")
                    .and_then(|value| value.as_str())
                    == Some("fusion")
        })
        .filter_map(|event| {
            Some((
                event.payload.get("ts_us")?.as_i64()?,
                event
                    .payload
                    .get("features")?
                    .get("occupancy_probability")?
                    .as_f64()?,
            ))
        })
        .collect();
    let mut used_beliefs = vec![false; beliefs.len()];
    let mut samples = Vec::new();
    for (label_ts, occupied) in &labels {
        let nearest = beliefs
            .iter()
            .enumerate()
            .filter(|(index, (belief_ts, _))| {
                !used_beliefs[*index] && label_ts.abs_diff(*belief_ts) <= 500_000
            })
            .min_by_key(|(_, (belief_ts, _))| label_ts.abs_diff(*belief_ts));
        if let Some((index, (_, probability))) = nearest {
            used_beliefs[index] = true;
            samples.push(CalibrationSample {
                predicted_probability: *probability,
                observed_occupied: *occupied,
            });
        }
    }
    let metrics = evaluate_calibration(&samples);
    Ok(Some(CalibrationReportView {
        labels_total: labels.len(),
        matched_labels: samples.len(),
        unmatched_labels: labels.len() - samples.len(),
        accuracy: metrics.accuracy,
        brier_score: metrics.brier_score,
        positive_precision: metrics.positive_precision,
        positive_recall: metrics.positive_recall,
    }))
}

/// Opens the ledger read-and-write (SQLite requires a real connection either way; `liminald` may
/// be writing concurrently) and returns a snapshot, or `None` if the DB doesn't exist yet or a
/// read failed for any reason (e.g. a transient lock while `liminald` is mid-write) -- a TUI
/// panel should degrade to "no data yet" rather than crash on either.
///
/// Reopens the connection on every call rather than holding one open across ticks: at this
/// skeleton's poll rate (a few times a second) the overhead is negligible, and it avoids holding
/// a lock that could contend with `liminald`'s writer. Revisit if/when polling frequency or event
/// volume grows enough to matter.
pub fn read_ledger_snapshot_checked(db_path: &Path) -> Result<Option<LedgerSnapshot>, String> {
    if !db_path.exists() {
        return Ok(None);
    }
    let ledger = SqliteLedger::open(db_path, DEFAULT_MAX_SILENT_GAP_US)
        .map_err(|error| error.to_string())?;
    let events = ledger.events().map_err(|error| error.to_string())?;
    let mut stream_event_counts = BTreeMap::new();
    let sensor_gap_count = events
        .iter()
        .filter(|event| event.kind == "sensor_gap")
        .count();
    let pending_gap_streams = ledger.unacknowledged_sensor_gap_streams();
    let belief_count = events.iter().filter(|event| event.kind == "belief").count();
    let agent_run_count = events
        .iter()
        .filter(|event| event.kind == "agent_run")
        .count();
    let agent_runs = extract_agent_runs(&events, 5);
    for event in &events {
        if event.kind == "observation" {
            if let Some(stream_id) = event.payload.get("stream_id").and_then(|v| v.as_str()) {
                *stream_event_counts
                    .entry(stream_id.to_string())
                    .or_insert(0) += 1;
            }
        }
    }
    let latest_event = events.last().and_then(|event| {
        event
            .payload
            .get("stream_id")
            .and_then(|value| value.as_str())
            .map(|stream| (event.kind.clone(), stream.to_string()))
    });
    let recent_records: Vec<LatestRecord> = events
        .iter()
        .rev()
        .take(32)
        .map(|event| record_summary(event, &ledger))
        .collect();
    let latest_record = recent_records.first().cloned();
    let occupancy_events = extract_occupancy_events(&events);
    let memory_replay = replay_memory(&occupancy_events);
    let (episode_count, pattern_count) = extract_persisted_memory_counts(&events)
        .unwrap_or((memory_replay.episodes.len(), memory_replay.patterns.len()));
    let persisted_belief = extract_latest_belief(&events).map(|mut belief| {
        if let Some(belief_event) = events.iter().rev().find(|event| {
            event.kind == "belief"
                && event.payload.get("stream_id").and_then(|v| v.as_str()) == Some("fusion")
        }) {
            if let Ok(sources) = ledger.provenance_sources(&belief_event.id) {
                if !sources.is_empty() {
                    belief.evidence_ids = sources;
                }
            }
        }
        belief
    });
    Ok(Some(LedgerSnapshot {
        total_event_count: events.len(),
        sensor_gap_count,
        pending_gap_streams,
        belief_count,
        agent_run_count,
        agent_runs,
        stream_event_counts,
        latest_observation_timestamps: extract_latest_observation_timestamps(&events),
        latest_event,
        latest_record,
        recent_records,
        latest_camera_joints: extract_latest_camera_joints(&events),
        telemetry: extract_telemetry(&events),
        persisted_belief,
        // Keep enough history for the CUI's stepped local windows without loading an unbounded
        // timeline into every redraw.
        recent_observations: extract_recent_observations(&events, 4096),
        historical_buckets: extract_historical_buckets(&events, 30),
        episode_count,
        pattern_count,
        occupancy_events,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Builds an `Event` with the exact payload shape
    /// `SqliteLedger::append_observation_with_features` produces
    /// (`{"stream_id", "ts_us", "features"}`) -- `extract_latest_camera_joints` only reads the
    /// payload, so a hand-built `Event` with irrelevant chain fields zeroed is a fine substitute
    /// for round-tripping through a real ledger in every test.
    fn event(stream_id: &str, features: serde_json::Value) -> Event {
        Event {
            id: "id".to_string(),
            sequence: 0,
            kind: "observation".to_string(),
            payload: serde_json::json!({ "stream_id": stream_id, "ts_us": 0, "features": features }),
            previous_hash: "0".to_string(),
            hash: "irrelevant".to_string(),
        }
    }

    fn observation(stream_id: &str, timestamp_us: i64) -> Event {
        let mut event = event(stream_id, serde_json::json!({}));
        event.payload["ts_us"] = serde_json::json!(timestamp_us);
        event
    }

    fn belief(timestamp_us: i64, probability: f64, stream: &str) -> Event {
        Event {
            id: format!("belief-{timestamp_us}"),
            sequence: 0,
            kind: "belief".to_string(),
            payload: serde_json::json!({
                "stream_id": stream,
                "ts_us": timestamp_us,
                "features": { "occupancy_probability": probability }
            }),
            previous_hash: "0".to_string(),
            hash: "irrelevant".to_string(),
        }
    }

    #[test]
    fn extract_occupancy_events_segments_only_fusion_beliefs() {
        let events = vec![
            belief(0, 0.9, "fusion"),
            belief(3_000_000, 0.9, "fusion"),
            belief(6_000_000, 0.9, "fusion"),
            belief(9_000_000, 0.9, "render-fallback"),
        ];

        let occupancy_events = extract_occupancy_events(&events);
        assert_eq!(occupancy_events.len(), 1);
        assert_eq!(occupancy_events[0].start_ts_us, 0);
        assert_eq!(occupancy_events[0].end_ts_us, 6_000_000);
    }

    #[test]
    fn extract_latest_camera_joints_returns_empty_for_no_events() {
        assert_eq!(extract_latest_camera_joints(&[]), vec![]);
    }

    #[test]
    fn extract_latest_camera_joints_ignores_non_camera_streams() {
        let events = vec![event("wifi", serde_json::json!({}))];
        assert_eq!(extract_latest_camera_joints(&events), vec![]);
    }

    #[test]
    fn extract_latest_camera_joints_reads_the_most_recent_camera_observation() {
        let events = vec![
            event(
                "camera",
                serde_json::json!({ "joints": [{"name":"nose","x":0.1,"y":0.2,"confidence":0.9}] }),
            ),
            event("wifi", serde_json::json!({})),
            event(
                "camera",
                serde_json::json!({ "joints": [{"name":"nose","x":0.5,"y":0.6,"confidence":0.8}] }),
            ),
        ];
        // The second camera observation is the most recent one overall.
        assert_eq!(extract_latest_camera_joints(&events), vec![(0.5, 0.6, 0.8)]);
    }

    #[test]
    fn extract_latest_camera_joints_handles_missing_or_malformed_features_gracefully() {
        let events = vec![event(
            "camera",
            serde_json::json!({ "no_joints_here": true }),
        )];
        assert_eq!(extract_latest_camera_joints(&events), vec![]);
    }

    #[test]
    fn extract_telemetry_reads_latest_derived_values_without_raw_identifiers() {
        let events = vec![
            event(
                "microphone",
                serde_json::json!({
                    "rms": 0.12,
                    "spectral_centroid_hz": 1200.0,
                    "voice_activity_probability": 0.8
                }),
            ),
            event(
                "wifi",
                serde_json::json!({
                    "rssi_mean": -51.0,
                    "noise_mean": -92.0,
                    "visible_network_count": 4
                }),
            ),
            event(
                "bluetooth",
                serde_json::json!({
                    "cluster_count": 2,
                    "clusters": [{"pseudonym":"ble:one","rssi":-40},{"pseudonym":"ble:two","rssi":-60}]
                }),
            ),
        ];
        assert_eq!(
            extract_telemetry(&events),
            TelemetrySnapshot {
                camera_presence: None,
                audio_rms: Some(0.12),
                audio_centroid_hz: Some(1200.0),
                audio_vad: Some(0.8),
                wifi_rssi_mean: Some(-51.0),
                wifi_noise_mean: Some(-92.0),
                wifi_network_count: Some(4.0),
                bluetooth_cluster_count: Some(2.0),
                bluetooth_mean_rssi: Some(-50.0),
            }
        );
    }

    #[test]
    fn extract_latest_belief_reads_only_the_daemons_fusion_record() {
        let mut belief = event(
            "fusion",
            serde_json::json!({
                "occupancy_probability": 0.8,
                "confidence": 0.65,
                "disagreement": 0.1,
                "observed_modalities": 2,
                "sensor_health": 1.0,
                "state": "stable",
                "model": "transparent-v1"
            }),
        );
        belief.payload["derived_from"] = serde_json::json!(["observation-1"]);
        belief.kind = "belief".to_string();
        assert_eq!(
            extract_latest_belief(&[belief]),
            Some(PersistedBelief {
                occupancy_probability: 0.8,
                confidence: 0.65,
                disagreement: 0.1,
                observed_modalities: 2,
                sensor_health: 1.0,
                state: BeliefState::Stable,
                evidence_ids: vec!["observation-1".to_string()],
            })
        );
    }

    #[test]
    fn historical_buckets_are_newest_first_and_bounded() {
        let events = vec![
            Event {
                payload: serde_json::json!({"stream_id":"camera","ts_us":0,"features":{}}),
                ..event("camera", serde_json::json!({}))
            },
            Event {
                payload: serde_json::json!({"stream_id":"wifi","ts_us":DAY_US,"features":{}}),
                ..event("wifi", serde_json::json!({}))
            },
            Event {
                payload: serde_json::json!({"stream_id":"camera","ts_us":DAY_US + 1,"features":{}}),
                ..event("camera", serde_json::json!({}))
            },
        ];

        assert_eq!(
            extract_historical_buckets(&events, 2),
            vec![
                HistoricalBucket {
                    day_index: 1,
                    observation_count: 2,
                },
                HistoricalBucket {
                    day_index: 0,
                    observation_count: 1,
                },
            ]
        );
    }

    #[test]
    fn persisted_memory_counts_override_transient_replay_when_present() {
        let events = vec![
            Event {
                kind: "episode".to_string(),
                ..event("episode-1", serde_json::json!({}))
            },
            Event {
                kind: "pattern".to_string(),
                ..event("pattern-1", serde_json::json!({}))
            },
            Event {
                kind: "pattern".to_string(),
                ..event("pattern-2", serde_json::json!({}))
            },
        ];
        assert_eq!(extract_persisted_memory_counts(&events), Some((1, 2)));
        assert_eq!(extract_persisted_memory_counts(&[]), None);
    }

    #[test]
    fn read_ledger_snapshot_returns_none_for_a_missing_database() {
        let path = std::env::temp_dir().join(format!("nonexistent-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(read_ledger_snapshot_checked(&path).unwrap(), None);
    }

    #[test]
    fn checked_snapshot_reports_an_existing_invalid_database() {
        let path = std::env::temp_dir().join(format!("invalid-{}.db", std::process::id()));
        std::fs::write(&path, b"not sqlite").unwrap();

        let error = read_ledger_snapshot_checked(&path).unwrap_err();
        assert!(!error.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn read_ledger_snapshot_reads_real_data_from_a_real_sqlite_ledger() {
        let path = std::env::temp_dir().join(format!("ledger-view-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger
                .append_observation_with_features(
                    "obs_1",
                    "camera",
                    0,
                    serde_json::json!({ "joints": [{"name":"nose","x":0.3,"y":0.4,"confidence":0.7}] }),
                )
                .unwrap();
        }

        let snapshot = read_ledger_snapshot_checked(&path).unwrap().unwrap();
        assert_eq!(snapshot.total_event_count, 1);
        assert_eq!(snapshot.stream_event_counts.get("camera"), Some(&1));
        assert_eq!(
            snapshot.latest_event,
            Some(("observation".to_string(), "camera".to_string()))
        );
        assert_eq!(snapshot.latest_camera_joints, vec![(0.3, 0.4, 0.7)]);
        assert_eq!(snapshot.telemetry, TelemetrySnapshot::default());
        assert_eq!(snapshot.recent_observations.len(), 1);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn read_ledger_snapshot_exposes_pending_gaps_and_agent_runs() {
        let path =
            std::env::temp_dir().join(format!("ledger-view-state-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let mut ledger = SqliteLedger::open(&path, 10).unwrap();
            ledger.append_observation("obs_1", "camera", 0).unwrap();
            ledger
                .append_observation("obs_2", "camera", DEFAULT_MAX_SILENT_GAP_US + 1)
                .unwrap();
            ledger
                .append_derived_record(
                    "agent_1",
                    "agent_run",
                    serde_json::json!({"schema":"liminal.agent_run.v1"}),
                    &["obs_2".to_string()],
                )
                .unwrap();
        }

        let snapshot = read_ledger_snapshot_checked(&path).unwrap().unwrap();
        assert_eq!(snapshot.pending_gap_streams, vec!["camera".to_string()]);
        assert_eq!(snapshot.agent_run_count, 1);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn read_ledger_snapshot_uses_persisted_provenance_edges_for_belief_evidence() {
        let path = std::env::temp_dir().join(format!(
            "ledger-view-provenance-test-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "camera", 0).unwrap();
            ledger
                .append_belief_with_features_and_evidence(
                    "belief_1",
                    "fusion",
                    1,
                    serde_json::json!({
                        "occupancy_probability": 0.8,
                        "confidence": 0.7,
                        "disagreement": 0.1,
                        "observed_modalities": 1,
                        "sensor_health": 1.0,
                        "state": "stable"
                    }),
                    &["obs_1".to_string()],
                )
                .unwrap();
        }

        let snapshot = read_ledger_snapshot_checked(&path).unwrap().unwrap();
        assert_eq!(
            snapshot.persisted_belief.unwrap().evidence_ids,
            vec!["obs_1".to_string()]
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn calibration_report_matches_only_fusion_beliefs_and_keeps_unmatched_labels() {
        let db_path = std::env::temp_dir().join(format!(
            "ledger-view-calibration-db-{}.db",
            std::process::id()
        ));
        let labels_path = std::env::temp_dir().join(format!(
            "ledger-view-calibration-labels-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&labels_path);
        {
            let mut ledger = SqliteLedger::open(&db_path, i64::MAX).unwrap();
            ledger
                .append_belief_with_features_and_evidence(
                    "belief_1",
                    "fusion",
                    1_000_000,
                    serde_json::json!({
                        "occupancy_probability": 0.9,
                        "confidence": 0.8,
                        "disagreement": 0.0,
                        "observed_modalities": 2,
                        "sensor_health": 1.0,
                        "state": "stable"
                    }),
                    &[],
                )
                .unwrap();
            ledger
                .append_belief_with_features_and_evidence(
                    "belief_2",
                    "render-fallback",
                    2_000_000,
                    serde_json::json!({"occupancy_probability": 0.0}),
                    &[],
                )
                .unwrap();
        }
        std::fs::write(
            &labels_path,
            "{\"ts_us\":1000100,\"occupied\":true}\n{\"ts_us\":9000000,\"occupied\":false}\n",
        )
        .unwrap();

        let report = read_calibration_report_checked(&db_path, &labels_path)
            .unwrap()
            .unwrap();
        assert_eq!(report.labels_total, 2);
        assert_eq!(report.matched_labels, 1);
        assert_eq!(report.unmatched_labels, 1);
        assert_eq!(report.accuracy, 1.0);

        std::fs::remove_file(db_path).unwrap();
        std::fs::remove_file(labels_path).unwrap();
    }

    #[test]
    fn recent_observations_are_newest_first_and_limited() {
        let events = vec![
            event("wifi", serde_json::json!({})),
            event("camera", serde_json::json!({})),
            event("microphone", serde_json::json!({})),
        ];
        let recent = extract_recent_observations(&events, 2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].stream, "microphone");
        assert_eq!(recent[1].stream, "camera");
    }

    #[test]
    fn recent_observation_rates_use_timestamp_spans_and_do_not_invent_single_sample_cadence() {
        let observations = vec![
            RecentObservation {
                stream: "camera".to_string(),
                timestamp_us: 2_000_000,
                kind: "observation".to_string(),
            },
            RecentObservation {
                stream: "camera".to_string(),
                timestamp_us: 1_000_000,
                kind: "observation".to_string(),
            },
            RecentObservation {
                stream: "wifi".to_string(),
                timestamp_us: 2_000_000,
                kind: "observation".to_string(),
            },
        ];
        let rates = extract_recent_observation_rates(&observations);
        assert_eq!(rates.get("camera"), Some(&2.0));
        assert_eq!(rates.get("wifi"), Some(&0.0));
    }

    #[test]
    fn latest_observation_timestamps_select_the_newest_per_stream() {
        let events = vec![
            observation("camera", 20),
            observation("camera", 10),
            observation("wifi", 30),
        ];
        let timestamps = extract_latest_observation_timestamps(&events);
        assert_eq!(timestamps.get("camera"), Some(&20));
        assert_eq!(timestamps.get("wifi"), Some(&30));
    }
}
