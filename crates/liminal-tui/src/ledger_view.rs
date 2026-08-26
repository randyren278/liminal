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

use liminal_ledger::{Event, SqliteLedger};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct LedgerSnapshot {
    pub total_event_count: usize,
    /// (x, y, confidence) for each joint in the most recent `camera` stream observation, if any.
    pub latest_camera_joints: Vec<(f64, f64, f64)>,
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

/// Opens the ledger read-and-write (SQLite requires a real connection either way; `liminald` may
/// be writing concurrently) and returns a snapshot, or `None` if the DB doesn't exist yet or a
/// read failed for any reason (e.g. a transient lock while `liminald` is mid-write) -- a TUI
/// panel should degrade to "no data yet" rather than crash on either.
///
/// Reopens the connection on every call rather than holding one open across ticks: at this
/// skeleton's poll rate (a few times a second) the overhead is negligible, and it avoids holding
/// a lock that could contend with `liminald`'s writer. Revisit if/when polling frequency or event
/// volume grows enough to matter.
pub fn read_ledger_snapshot(db_path: &Path) -> Option<LedgerSnapshot> {
    if !db_path.exists() {
        return None;
    }
    let ledger = SqliteLedger::open(db_path, i64::MAX).ok()?;
    let events = ledger.events().ok()?;
    Some(LedgerSnapshot {
        total_event_count: events.len(),
        latest_camera_joints: extract_latest_camera_joints(&events),
    })
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
    fn read_ledger_snapshot_returns_none_for_a_missing_database() {
        let path = std::env::temp_dir().join(format!("nonexistent-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(read_ledger_snapshot(&path), None);
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

        let snapshot = read_ledger_snapshot(&path).unwrap();
        assert_eq!(snapshot.total_event_count, 1);
        assert_eq!(snapshot.latest_camera_joints, vec![(0.3, 0.4, 0.7)]);

        std::fs::remove_file(&path).unwrap();
    }
}
