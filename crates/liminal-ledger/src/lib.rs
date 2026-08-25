//! Append-only event ledger: hash-chain integrity, erase-cascade invalidation, and
//! sensor-gap acknowledgment (belief must never silently bridge a sensor outage).
//!
//! Master plan reference: §87 (Event Integrity), §88 (Crash Recovery), §103 (Forgetting),
//! §142 (Required Mutation Tests #6, #9).

use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Event {
    pub id: String,
    pub sequence: u64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub previous_hash: String,
    pub hash: String,
}

fn compute_hash(previous_hash: &str, payload: &serde_json::Value) -> String {
    let canonical = payload.to_string();
    let mut hasher = blake3::Hasher::new();
    hasher.update(previous_hash.as_bytes());
    hasher.update(canonical.as_bytes());
    hasher.finalize().to_hex().to_string()
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LedgerError {
    #[error("hash chain broken at sequence {0}")]
    ChainBroken(u64),
    #[error("unknown provenance node `{0}`")]
    UnknownNode(String),
    #[error(
        "belief for stream `{0}` cannot be recorded: an unacknowledged sensor gap exists; \
         record a SensorGap event before resuming belief"
    )]
    SensorGapNotAcknowledged(String),
}

const GENESIS_HASH: &str = "0";

pub struct Ledger {
    events: Vec<Event>,
    next_sequence: u64,
    stream_last_ts_us: HashMap<String, i64>,
    stream_gap_pending: HashMap<String, bool>,
    max_silent_gap_us: i64,
}

impl Ledger {
    pub fn new(max_silent_gap_us: i64) -> Self {
        Self {
            events: Vec::new(),
            next_sequence: 0,
            stream_last_ts_us: HashMap::new(),
            stream_gap_pending: HashMap::new(),
            max_silent_gap_us,
        }
    }

    fn append_raw(&mut self, id: impl Into<String>, kind: &str, payload: serde_json::Value) {
        let previous_hash = self
            .events
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_else(|| GENESIS_HASH.to_string());
        let hash = compute_hash(&previous_hash, &payload);
        self.events.push(Event {
            id: id.into(),
            sequence: self.next_sequence,
            kind: kind.to_string(),
            payload,
            previous_hash,
            hash,
        });
        self.next_sequence += 1;
    }

    /// Record a raw sensor observation on a stream. Detects (but does not reject) a silent
    /// gap — the gap must be explicitly acknowledged via `record_sensor_gap` before any belief
    /// can be recorded for this stream again.
    pub fn append_observation(&mut self, id: impl Into<String>, stream_id: &str, ts_us: i64) {
        if let Some(&last) = self.stream_last_ts_us.get(stream_id) {
            if ts_us - last > self.max_silent_gap_us {
                self.stream_gap_pending.insert(stream_id.to_string(), true);
            }
        }
        self.stream_last_ts_us.insert(stream_id.to_string(), ts_us);
        self.append_raw(
            id,
            "observation",
            serde_json::json!({ "stream_id": stream_id, "ts_us": ts_us }),
        );
    }

    /// §89 / §142.9: explicitly acknowledge a sensor outage. Required before belief can resume
    /// for the affected stream.
    pub fn record_sensor_gap(&mut self, id: impl Into<String>, stream_id: &str, ts_us: i64) {
        self.stream_gap_pending.insert(stream_id.to_string(), false);
        self.stream_last_ts_us.insert(stream_id.to_string(), ts_us);
        self.append_raw(
            id,
            "sensor_gap",
            serde_json::json!({ "stream_id": stream_id, "ts_us": ts_us }),
        );
    }

    /// Record a belief derived from a stream. Fails if the stream has an unacknowledged sensor
    /// gap — belief must never silently bridge an outage (§89, §142.9).
    pub fn append_belief(
        &mut self,
        id: impl Into<String>,
        stream_id: &str,
        ts_us: i64,
    ) -> Result<(), LedgerError> {
        if *self.stream_gap_pending.get(stream_id).unwrap_or(&false) {
            return Err(LedgerError::SensorGapNotAcknowledged(stream_id.to_string()));
        }
        self.append_raw(
            id,
            "belief",
            serde_json::json!({ "stream_id": stream_id, "ts_us": ts_us }),
        );
        Ok(())
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// §87/§109: verify the hash chain has not been tampered with, e.g. after replay.
    pub fn verify_chain(&self) -> Result<(), LedgerError> {
        let mut previous_hash = GENESIS_HASH.to_string();
        for event in &self.events {
            if event.previous_hash != previous_hash {
                return Err(LedgerError::ChainBroken(event.sequence));
            }
            let expected = compute_hash(&previous_hash, &event.payload);
            if expected != event.hash {
                return Err(LedgerError::ChainBroken(event.sequence));
            }
            previous_hash = event.hash.clone();
        }
        Ok(())
    }
}

/// §103 Forgetting: erase source content and cascade invalidation to every derived node
/// (Observation -> Event -> Episode -> Pattern -> Interpretation), per the provenance graph.
#[derive(Debug, Clone)]
struct ProvenanceNode {
    depends_on: Vec<String>,
    erased: bool,
    invalidated: bool,
}

pub struct ProvenanceGraph {
    nodes: HashMap<String, ProvenanceNode>,
}

impl ProvenanceGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, id: impl Into<String>, depends_on: Vec<String>) {
        self.nodes.insert(
            id.into(),
            ProvenanceNode {
                depends_on,
                erased: false,
                invalidated: false,
            },
        );
    }

    pub fn is_invalidated(&self, id: &str) -> Option<bool> {
        self.nodes.get(id).map(|n| n.invalidated)
    }

    pub fn is_erased(&self, id: &str) -> Option<bool> {
        self.nodes.get(id).map(|n| n.erased)
    }

    /// §103 steps 2-4: erase source content, then invalidate every direct or transitive
    /// dependent. Mutation test §142.6: skipping the cascade must be caught by a test that
    /// erases a source Observation and asserts its downstream Pattern is invalidated.
    pub fn erase(&mut self, id: &str) -> Result<(), LedgerError> {
        if !self.nodes.contains_key(id) {
            return Err(LedgerError::UnknownNode(id.to_string()));
        }
        self.nodes.get_mut(id).unwrap().erased = true;
        self.nodes.get_mut(id).unwrap().invalidated = true;

        // Bounded by node count: a correct fixpoint converges in at most one pass per node
        // (the longest possible dependency chain), so this can never hang even if a single
        // pass fails to mark every eligible node invalidated.
        for _ in 0..=self.nodes.len() {
            let mut changed = false;
            let invalidated_ids: Vec<String> = self
                .nodes
                .iter()
                .filter(|(_, n)| n.invalidated)
                .map(|(k, _)| k.clone())
                .collect();
            for node in self.nodes.values_mut() {
                if node.invalidated {
                    continue;
                }
                if node
                    .depends_on
                    .iter()
                    .any(|dep| invalidated_ids.contains(dep))
                {
                    node.invalidated = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        Ok(())
    }
}

impl Default for ProvenanceGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_verifies_on_untampered_ledger() {
        let mut ledger = Ledger::new(5_000_000);
        ledger.append_observation("obs_1", "wifi", 0);
        ledger.append_belief("belief_1", "wifi", 100).unwrap();
        assert!(ledger.verify_chain().is_ok());
    }

    #[test]
    fn chain_detects_tampered_payload() {
        let mut ledger = Ledger::new(5_000_000);
        ledger.append_observation("obs_1", "wifi", 0);
        ledger.append_belief("belief_1", "wifi", 100).unwrap();
        // Simulate tampering with a stored payload without recomputing the hash chain.
        ledger.events[0].payload = serde_json::json!({ "stream_id": "wifi", "ts_us": 999_999 });
        assert_eq!(ledger.verify_chain(), Err(LedgerError::ChainBroken(0)));
    }

    #[test]
    fn belief_rejected_across_unacknowledged_sensor_gap() {
        let mut ledger = Ledger::new(5_000_000);
        ledger.append_observation("obs_1", "wifi", 0);
        ledger.append_observation("obs_2", "wifi", 20_000_000); // 20s gap, threshold is 5s
        let err = ledger
            .append_belief("belief_1", "wifi", 20_000_100)
            .unwrap_err();
        assert_eq!(
            err,
            LedgerError::SensorGapNotAcknowledged("wifi".to_string())
        );
    }

    #[test]
    fn belief_allowed_after_gap_is_acknowledged() {
        let mut ledger = Ledger::new(5_000_000);
        ledger.append_observation("obs_1", "wifi", 0);
        ledger.append_observation("obs_2", "wifi", 20_000_000);
        ledger.record_sensor_gap("gap_1", "wifi", 20_000_000);
        assert!(ledger.append_belief("belief_1", "wifi", 20_000_100).is_ok());
    }

    #[test]
    fn belief_allowed_when_no_gap_occurred() {
        let mut ledger = Ledger::new(5_000_000);
        ledger.append_observation("obs_1", "wifi", 0);
        ledger.append_observation("obs_2", "wifi", 1_000_000);
        assert!(ledger.append_belief("belief_1", "wifi", 1_000_100).is_ok());
    }

    #[test]
    fn erase_cascades_through_the_full_provenance_chain() {
        let mut graph = ProvenanceGraph::new();
        graph.add_node("obs_1", vec![]);
        graph.add_node("event_1", vec!["obs_1".to_string()]);
        graph.add_node("episode_1", vec!["event_1".to_string()]);
        graph.add_node("pattern_1", vec!["episode_1".to_string()]);
        graph.add_node("interpretation_1", vec!["pattern_1".to_string()]);
        // Unrelated branch must survive.
        graph.add_node("obs_2", vec![]);
        graph.add_node("pattern_2", vec!["obs_2".to_string()]);

        graph.erase("obs_1").unwrap();

        assert_eq!(graph.is_erased("obs_1"), Some(true));
        for id in ["event_1", "episode_1", "pattern_1", "interpretation_1"] {
            assert_eq!(
                graph.is_invalidated(id),
                Some(true),
                "{id} should be invalidated"
            );
        }
        assert_eq!(graph.is_invalidated("obs_2"), Some(false));
        assert_eq!(graph.is_invalidated("pattern_2"), Some(false));
    }

    #[test]
    fn erase_unknown_node_errors() {
        let mut graph = ProvenanceGraph::new();
        assert_eq!(
            graph.erase("nope"),
            Err(LedgerError::UnknownNode("nope".to_string()))
        );
    }
}
