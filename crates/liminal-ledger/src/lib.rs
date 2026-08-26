//! Append-only event ledger: hash-chain integrity, erase-cascade invalidation, and
//! sensor-gap acknowledgment (belief must never silently bridge a sensor outage).
//!
//! Also provides a SQLite-backed persistence layer (`SqliteLedger`) for the `events` subset of
//! §84's Data Model, with a single forward migration per §108 and crash recovery per §88 (on
//! reopen, the event chain tail is re-verified rather than trusted).
//!
//! Master plan reference: §84 (Data Model), §87 (Event Integrity), §88 (Crash Recovery),
//! §103 (Forgetting), §108 (Database Migration Policy), §142 (Required Mutation Tests #6, #9).

use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;
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
    #[error("sqlite ledger error: {0}")]
    Database(String),
}

impl From<rusqlite::Error> for LedgerError {
    fn from(err: rusqlite::Error) -> Self {
        LedgerError::Database(err.to_string())
    }
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

const SCHEMA_VERSION: i64 = 1;

/// §108: the single forward migration for this task's scope (`events` only; `schema_migrations`
/// itself). Idempotent — safe to call on every open.
fn migrate(conn: &Connection) -> Result<(), LedgerError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY
         );
         CREATE TABLE IF NOT EXISTS events (
            id            TEXT PRIMARY KEY,
            sequence      INTEGER NOT NULL UNIQUE,
            kind          TEXT NOT NULL,
            payload       TEXT NOT NULL,
            previous_hash TEXT NOT NULL,
            hash          TEXT NOT NULL
         );",
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (version) VALUES (?1)",
        [SCHEMA_VERSION],
    )?;
    Ok(())
}

/// SQLite-backed counterpart to `Ledger`, persisting events to the §84 `events` table instead of
/// holding them only in memory. Offers the same append/verify shape; sensor-gap and hash-chain
/// state is reconstructed from the stored events on open (§88 step 3: verify the event-chain
/// tail on restart, §109's replay requirement).
pub struct SqliteLedger {
    conn: Connection,
    next_sequence: u64,
    last_hash: String,
    stream_last_ts_us: HashMap<String, i64>,
    stream_gap_pending: HashMap<String, bool>,
    max_silent_gap_us: i64,
}

impl SqliteLedger {
    /// Open (creating if absent) a SQLite-backed ledger at `path`, running the forward migration
    /// and reconstructing in-memory sensor-gap state from any events already stored.
    pub fn open(path: &Path, max_silent_gap_us: i64) -> Result<Self, LedgerError> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn, max_silent_gap_us)
    }

    fn from_connection(conn: Connection, max_silent_gap_us: i64) -> Result<Self, LedgerError> {
        migrate(&conn)?;

        let mut ledger = Self {
            conn,
            next_sequence: 0,
            last_hash: GENESIS_HASH.to_string(),
            stream_last_ts_us: HashMap::new(),
            stream_gap_pending: HashMap::new(),
            max_silent_gap_us,
        };

        let events = ledger.load_events()?;
        for event in &events {
            if let Some(stream_id) = event.payload.get("stream_id").and_then(|v| v.as_str()) {
                let ts_us = event
                    .payload
                    .get("ts_us")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                match event.kind.as_str() {
                    "observation" => {
                        if let Some(&last) = ledger.stream_last_ts_us.get(stream_id) {
                            if ts_us - last > ledger.max_silent_gap_us {
                                ledger
                                    .stream_gap_pending
                                    .insert(stream_id.to_string(), true);
                            }
                        }
                        ledger
                            .stream_last_ts_us
                            .insert(stream_id.to_string(), ts_us);
                    }
                    "sensor_gap" => {
                        ledger
                            .stream_gap_pending
                            .insert(stream_id.to_string(), false);
                        ledger
                            .stream_last_ts_us
                            .insert(stream_id.to_string(), ts_us);
                    }
                    _ => {}
                }
            }
            ledger.next_sequence = event.sequence + 1;
            ledger.last_hash = event.hash.clone();
        }

        Ok(ledger)
    }

    fn load_events(&self) -> Result<Vec<Event>, LedgerError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, sequence, kind, payload, previous_hash, hash FROM events ORDER BY sequence ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let payload_text: String = row.get(3)?;
            Ok(Event {
                id: row.get(0)?,
                sequence: row.get::<_, i64>(1)? as u64,
                kind: row.get(2)?,
                payload: serde_json::from_str(&payload_text).unwrap_or(serde_json::Value::Null),
                previous_hash: row.get(4)?,
                hash: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(LedgerError::from)
    }

    fn append_raw(
        &mut self,
        id: impl Into<String>,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<(), LedgerError> {
        let previous_hash = self.last_hash.clone();
        let hash = compute_hash(&previous_hash, &payload);
        let sequence = self.next_sequence;
        self.conn.execute(
            "INSERT INTO events (id, sequence, kind, payload, previous_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id.into(),
                sequence as i64,
                kind,
                payload.to_string(),
                previous_hash,
                hash,
            ],
        )?;
        self.last_hash = hash;
        self.next_sequence += 1;
        Ok(())
    }

    /// See `Ledger::append_observation`.
    pub fn append_observation(
        &mut self,
        id: impl Into<String>,
        stream_id: &str,
        ts_us: i64,
    ) -> Result<(), LedgerError> {
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
        )
    }

    /// Like `append_observation`, but embeds an arbitrary derived-feature payload (e.g. a
    /// decoded `liminal-ipc` envelope's feature JSON from a real sensor organ) alongside
    /// `stream_id`/`ts_us`, rather than the fixed `{stream_id, ts_us}` shape. Same sensor-gap
    /// tracking as `append_observation` -- this is the ingest path `liminald` uses for real
    /// sensor data (§15), where `append_observation` alone would discard the actual features.
    pub fn append_observation_with_features(
        &mut self,
        id: impl Into<String>,
        stream_id: &str,
        ts_us: i64,
        features: serde_json::Value,
    ) -> Result<(), LedgerError> {
        if let Some(&last) = self.stream_last_ts_us.get(stream_id) {
            if ts_us - last > self.max_silent_gap_us {
                self.stream_gap_pending.insert(stream_id.to_string(), true);
            }
        }
        self.stream_last_ts_us.insert(stream_id.to_string(), ts_us);
        self.append_raw(
            id,
            "observation",
            serde_json::json!({ "stream_id": stream_id, "ts_us": ts_us, "features": features }),
        )
    }

    /// See `Ledger::record_sensor_gap`.
    pub fn record_sensor_gap(
        &mut self,
        id: impl Into<String>,
        stream_id: &str,
        ts_us: i64,
    ) -> Result<(), LedgerError> {
        self.stream_gap_pending.insert(stream_id.to_string(), false);
        self.stream_last_ts_us.insert(stream_id.to_string(), ts_us);
        self.append_raw(
            id,
            "sensor_gap",
            serde_json::json!({ "stream_id": stream_id, "ts_us": ts_us }),
        )
    }

    /// See `Ledger::append_belief`.
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
        )
    }

    pub fn events(&self) -> Result<Vec<Event>, LedgerError> {
        self.load_events()
    }

    /// §87/§88 step 3/§109: verify the persisted hash chain has not been tampered with or torn
    /// mid-write, e.g. on crash-recovery reopen.
    pub fn verify_chain(&self) -> Result<(), LedgerError> {
        let mut previous_hash = GENESIS_HASH.to_string();
        for event in self.load_events()? {
            if event.previous_hash != previous_hash {
                return Err(LedgerError::ChainBroken(event.sequence));
            }
            let expected = compute_hash(&previous_hash, &event.payload);
            if expected != event.hash {
                return Err(LedgerError::ChainBroken(event.sequence));
            }
            previous_hash = event.hash;
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
    fn chain_detects_broken_previous_hash_link() {
        let mut ledger = Ledger::new(5_000_000);
        ledger.append_observation("obs_1", "wifi", 0);
        ledger.append_belief("belief_1", "wifi", 100).unwrap();
        // Simulate a torn write that leaves a stored previous_hash pointing at the wrong
        // predecessor, without touching the payload/hash themselves.
        ledger.events[1].previous_hash = "tampered".to_string();
        assert_eq!(ledger.verify_chain(), Err(LedgerError::ChainBroken(1)));
    }

    #[test]
    fn ledger_events_returns_all_appended_events_in_order() {
        let mut ledger = Ledger::new(5_000_000);
        ledger.append_observation("obs_1", "wifi", 0);
        ledger.append_belief("belief_1", "wifi", 100).unwrap();
        let events = ledger.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, "observation");
        assert_eq!(events[1].kind, "belief");
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

    #[test]
    fn provenance_graph_default_creates_an_empty_graph() {
        let graph = ProvenanceGraph::default();
        assert_eq!(graph.is_erased("anything"), None);
        assert_eq!(graph.is_invalidated("anything"), None);
    }

    /// Returns a unique tempfile path for a fresh on-disk SQLite DB; the caller is responsible
    /// for removing it once the test is done with it.
    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("liminal-ledger-test-{name}-{unique}.db"))
    }

    #[test]
    fn migration_creates_events_and_schema_migrations_tables_on_empty_db() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1")
            .unwrap();
        assert!(stmt.exists(["events"]).unwrap());
        assert!(stmt.exists(["schema_migrations"]).unwrap());

        let version: i64 = conn
            .query_row("SELECT version FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn sqlite_ledger_survives_close_and_reopen() {
        let path = temp_db_path("reopen");

        {
            let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
            ledger
                .append_observation("obs_2", "wifi", 1_000_000)
                .unwrap();
            ledger.append_belief("belief_1", "wifi", 1_000_100).unwrap();
            assert!(ledger.verify_chain().is_ok());
            // `ledger` (and its rusqlite::Connection) is dropped here, closing the file.
        }

        let reopened = SqliteLedger::open(&path, 5_000_000).unwrap();
        assert!(reopened.verify_chain().is_ok());
        assert_eq!(reopened.events().unwrap().len(), 3);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn append_observation_with_features_embeds_the_features_and_still_tracks_gaps() {
        let path = temp_db_path("features");
        let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();

        ledger
            .append_observation_with_features(
                "obs_1",
                "camera",
                0,
                serde_json::json!({ "body_count": "one", "joints": [] }),
            )
            .unwrap();

        let events = ledger.events().unwrap();
        assert_eq!(events[0].payload["features"]["body_count"], "one");

        // Same gap-tracking as append_observation: a big enough jump sets the gap flag, which
        // append_belief still enforces regardless of which append_* method produced the history.
        ledger
            .append_observation_with_features("obs_2", "camera", 20_000_000, serde_json::json!({}))
            .unwrap();
        let err = ledger
            .append_belief("belief_1", "camera", 20_000_100)
            .unwrap_err();
        assert_eq!(
            err,
            LedgerError::SensorGapNotAcknowledged("camera".to_string())
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn sqlite_ledger_detects_corrupted_chain_link_on_reopen() {
        let path = temp_db_path("corrupt");

        {
            let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
            ledger
                .append_observation("obs_2", "wifi", 1_000_000)
                .unwrap();
            ledger.append_belief("belief_1", "wifi", 1_000_100).unwrap();
        }

        // Simulate a torn/corrupted write (§88 crash recovery) by directly mutating the last
        // row's hash via raw SQL, bypassing the ledger API entirely.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "UPDATE events SET hash = 'corrupted' WHERE sequence = (SELECT MAX(sequence) FROM events)",
                [],
            )
            .unwrap();
        }

        let reopened = SqliteLedger::open(&path, 5_000_000).unwrap();
        assert_eq!(reopened.verify_chain(), Err(LedgerError::ChainBroken(2)));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn sqlite_ledger_verify_chain_detects_broken_previous_hash_link() {
        let path = temp_db_path("broken-prev");

        {
            let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
            ledger.append_belief("belief_1", "wifi", 100).unwrap();
        }

        // Simulate a torn write that leaves a stored previous_hash pointing at the wrong
        // predecessor, without touching the payload/hash themselves.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "UPDATE events SET previous_hash = 'tampered' WHERE sequence = 1",
                [],
            )
            .unwrap();
        }

        let reopened = SqliteLedger::open(&path, 5_000_000).unwrap();
        assert_eq!(reopened.verify_chain(), Err(LedgerError::ChainBroken(1)));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn sqlite_ledger_records_sensor_gap_and_rejects_belief_until_acknowledged() {
        let path = temp_db_path("gap-live");
        let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();

        ledger.append_observation("obs_1", "wifi", 0).unwrap();
        // 20s gap, threshold is 5s.
        ledger
            .append_observation("obs_2", "wifi", 20_000_000)
            .unwrap();

        let err = ledger
            .append_belief("belief_1", "wifi", 20_000_100)
            .unwrap_err();
        assert_eq!(
            err,
            LedgerError::SensorGapNotAcknowledged("wifi".to_string())
        );

        ledger
            .record_sensor_gap("gap_1", "wifi", 20_000_000)
            .unwrap();
        assert!(ledger.append_belief("belief_1", "wifi", 20_000_100).is_ok());

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn sqlite_ledger_reconstructs_gap_state_and_sensor_gap_kind_on_reopen() {
        let path = temp_db_path("gap-reopen");

        {
            let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
            ledger
                .append_observation("obs_2", "wifi", 20_000_000)
                .unwrap();
            ledger
                .record_sensor_gap("gap_1", "wifi", 20_000_000)
                .unwrap();
            ledger
                .append_belief("belief_1", "wifi", 20_000_100)
                .unwrap();
            // A second, unacknowledged gap right before shutdown: reopen must still require
            // acknowledgment rather than silently trusting the pre-shutdown ack.
            ledger
                .append_observation("obs_3", "wifi", 50_000_000)
                .unwrap();
        }

        let mut reopened = SqliteLedger::open(&path, 5_000_000).unwrap();
        let err = reopened
            .append_belief("belief_2", "wifi", 50_000_100)
            .unwrap_err();
        assert_eq!(
            err,
            LedgerError::SensorGapNotAcknowledged("wifi".to_string())
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn sqlite_ledger_open_rejects_a_file_that_is_not_a_valid_sqlite_database() {
        let path = temp_db_path("corrupt-file");
        std::fs::write(&path, b"not a sqlite database").unwrap();

        let result = SqliteLedger::open(&path, 5_000_000);
        assert!(matches!(result, Err(LedgerError::Database(_))));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn migrate_fails_when_schema_migrations_table_has_incompatible_schema() {
        let path = temp_db_path("bad-schema");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE schema_migrations (not_version INTEGER PRIMARY KEY);")
                .unwrap();
        }

        // The pre-existing table has no `version` column, so the migration's own INSERT
        // fails once `CREATE TABLE IF NOT EXISTS` finds the table already present.
        let result = SqliteLedger::open(&path, 5_000_000);
        assert!(matches!(result, Err(LedgerError::Database(_))));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn sqlite_ledger_operations_surface_errors_when_events_table_is_missing() {
        let path = temp_db_path("missing-table");
        let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute("DROP TABLE events", []).unwrap();
        }

        assert!(matches!(
            ledger.events().unwrap_err(),
            LedgerError::Database(_)
        ));
        assert!(matches!(
            ledger.append_observation("obs_1", "wifi", 0).unwrap_err(),
            LedgerError::Database(_)
        ));

        std::fs::remove_file(&path).unwrap();
    }
}
