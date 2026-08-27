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
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;
use thiserror::Error;

/// §17 Storage Locations: canonical data lives under `~/Library/Application Support/Liminal/`.
/// Shared by every process that opens the real ledger (`liminald`, `liminal-tui`) so the path
/// can't drift between them.
pub fn default_db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join("Library/Application Support/Liminal")
        .join("liminal.db")
}

#[derive(Debug, Clone)]
pub struct Event {
    pub id: String,
    pub sequence: u64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub previous_hash: String,
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct LatestObservation {
    pub id: String,
    pub timestamp_us: i64,
    pub features: serde_json::Value,
}

fn compute_hash(previous_hash: &str, payload: &serde_json::Value) -> String {
    compute_serialized_hash(previous_hash, &payload.to_string())
}

fn compute_serialized_hash(previous_hash: &str, payload: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(previous_hash.as_bytes());
    hasher.update(payload.as_bytes());
    hasher.finalize().to_hex().to_string()
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LedgerError {
    #[error("hash chain broken at sequence {0}")]
    ChainBroken(u64),
    #[error("unknown provenance node `{0}`")]
    UnknownNode(String),
    #[error("unknown ledger event `{0}`")]
    UnknownEvent(String),
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

/// Runtime gap threshold used by `liminald` and operator recovery commands.
pub const DEFAULT_MAX_SILENT_GAP_US: i64 = 30_000_000;

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

    pub fn has_unacknowledged_sensor_gap(&self, stream_id: &str) -> bool {
        *self.stream_gap_pending.get(stream_id).unwrap_or(&false)
    }

    /// Return streams whose beliefs are currently blocked by an unacknowledged gap.
    pub fn unacknowledged_sensor_gap_streams(&self) -> Vec<String> {
        let mut streams: Vec<_> = self
            .stream_gap_pending
            .iter()
            .filter(|(_, pending)| **pending)
            .map(|(stream, _)| stream.clone())
            .collect();
        streams.sort();
        streams
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

    /// Record a derived belief with its explainable feature payload. The same gap guard as
    /// [`Ledger::append_belief`] applies.
    pub fn append_belief_with_features(
        &mut self,
        id: impl Into<String>,
        stream_id: &str,
        ts_us: i64,
        features: serde_json::Value,
    ) -> Result<(), LedgerError> {
        self.append_belief_with_features_and_evidence(id, stream_id, ts_us, features, &[])
    }

    /// Record a derived belief with explicit source observation IDs. The IDs are persisted at the
    /// top level of the belief payload so downstream consumers can distinguish provenance from
    /// append-order hash history.
    pub fn append_belief_with_features_and_evidence(
        &mut self,
        id: impl Into<String>,
        stream_id: &str,
        ts_us: i64,
        features: serde_json::Value,
        derived_from: &[String],
    ) -> Result<(), LedgerError> {
        if *self.stream_gap_pending.get(stream_id).unwrap_or(&false) {
            return Err(LedgerError::SensorGapNotAcknowledged(stream_id.to_string()));
        }
        self.append_raw(
            id,
            "belief",
            serde_json::json!({ "stream_id": stream_id, "ts_us": ts_us, "derived_from": derived_from, "features": features }),
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

const SCHEMA_VERSION: i64 = 2;

/// §108: forward migrations for the event store and its explicit derivation edges. Idempotent —
/// safe to call on every open.
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
         );
         CREATE TABLE IF NOT EXISTS provenance_edges (
            derived_id TEXT NOT NULL,
            source_id  TEXT NOT NULL,
            PRIMARY KEY (derived_id, source_id)
         );
         CREATE INDEX IF NOT EXISTS events_kind_stream_sequence
           ON events (kind, json_extract(payload, '$.stream_id'), sequence DESC);",
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
    stream_last_monotonic_sequence: HashMap<String, u64>,
    max_silent_gap_us: i64,
}

impl SqliteLedger {
    /// Open (creating if absent) a SQLite-backed ledger at `path`, running the forward migration
    /// and reconstructing in-memory sensor-gap state from any events already stored.
    pub fn open(path: &Path, max_silent_gap_us: i64) -> Result<Self, LedgerError> {
        let conn = Connection::open(path)?;
        // Sensor ingest is a live path. Return a bounded database-lock error instead of allowing
        // a stalled reader/writer to freeze the capture daemon indefinitely.
        conn.busy_timeout(Duration::from_millis(250))?;
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
            stream_last_monotonic_sequence: HashMap::new(),
            max_silent_gap_us,
        };

        ledger.rebuild_runtime_state()?;

        Ok(ledger)
    }

    fn rebuild_runtime_state(&mut self) -> Result<(), LedgerError> {
        self.next_sequence = 0;
        self.last_hash = GENESIS_HASH.to_string();
        self.stream_last_ts_us.clear();
        self.stream_gap_pending.clear();
        self.stream_last_monotonic_sequence.clear();

        let events = self.load_events()?;
        for event in &events {
            if let Some(stream_id) = event.payload.get("stream_id").and_then(|v| v.as_str()) {
                let ts_us = event
                    .payload
                    .get("ts_us")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                match event.kind.as_str() {
                    "observation" => {
                        if let Some(&last) = self.stream_last_ts_us.get(stream_id) {
                            if ts_us - last > self.max_silent_gap_us {
                                self.stream_gap_pending.insert(stream_id.to_string(), true);
                            }
                        }
                        self.stream_last_ts_us.insert(stream_id.to_string(), ts_us);
                        if let Some(sequence) = event
                            .payload
                            .get("features")
                            .and_then(|features| features.get("_monotonic_sequence"))
                            .and_then(|value| value.as_u64())
                        {
                            self.stream_last_monotonic_sequence
                                .entry(stream_id.to_string())
                                .and_modify(|last| *last = (*last).max(sequence))
                                .or_insert(sequence);
                        }
                    }
                    "sensor_gap" => {
                        self.stream_gap_pending.insert(stream_id.to_string(), false);
                        self.stream_last_ts_us.insert(stream_id.to_string(), ts_us);
                    }
                    _ => {}
                }
            }
            self.next_sequence = event.sequence + 1;
            self.last_hash = event.hash.clone();
        }

        // Crash recovery is part of opening the canonical store: never expose a reconstructed
        // ledger whose persisted chain has already failed integrity verification.
        self.verify_chain()
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
        self.append_raw_with_dependencies(id, kind, payload, &[])
    }

    fn append_raw_with_dependencies(
        &mut self,
        id: impl Into<String>,
        kind: &str,
        payload: serde_json::Value,
        source_ids: &[String],
    ) -> Result<(), LedgerError> {
        let id = id.into();
        let previous_hash = self.last_hash.clone();
        let hash = compute_hash(&previous_hash, &payload);
        let sequence = self.next_sequence;
        let transaction = self.conn.transaction()?;
        transaction.execute(
            "INSERT INTO events (id, sequence, kind, payload, previous_hash, hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                sequence as i64,
                kind,
                payload.to_string(),
                previous_hash,
                hash,
            ],
        )?;
        for source_id in source_ids {
            transaction.execute(
                "INSERT INTO provenance_edges (derived_id, source_id) VALUES (?1, ?2)",
                rusqlite::params![id, source_id],
            )?;
        }
        transaction.commit()?;
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
        let monotonic_sequence = features
            .get("_monotonic_sequence")
            .and_then(|value| value.as_u64());
        self.append_raw(
            id,
            "observation",
            serde_json::json!({ "stream_id": stream_id, "ts_us": ts_us, "features": features }),
        )?;
        if let Some(sequence) = monotonic_sequence {
            self.stream_last_monotonic_sequence
                .entry(stream_id.to_string())
                .and_modify(|last| *last = (*last).max(sequence))
                .or_insert(sequence);
        }
        Ok(())
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

    pub fn has_unacknowledged_sensor_gap(&self, stream_id: &str) -> bool {
        *self.stream_gap_pending.get(stream_id).unwrap_or(&false)
    }

    /// Return streams whose beliefs are currently blocked by an unacknowledged gap.
    pub fn unacknowledged_sensor_gap_streams(&self) -> Vec<String> {
        let mut streams: Vec<_> = self
            .stream_gap_pending
            .iter()
            .filter(|(_, pending)| **pending)
            .map(|(stream, _)| stream.clone())
            .collect();
        streams.sort();
        streams
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

    /// See `Ledger::append_belief_with_features`.
    pub fn append_belief_with_features(
        &mut self,
        id: impl Into<String>,
        stream_id: &str,
        ts_us: i64,
        features: serde_json::Value,
    ) -> Result<(), LedgerError> {
        self.append_belief_with_features_and_evidence(id, stream_id, ts_us, features, &[])
    }

    /// See `Ledger::append_belief_with_features_and_evidence`.
    pub fn append_belief_with_features_and_evidence(
        &mut self,
        id: impl Into<String>,
        stream_id: &str,
        ts_us: i64,
        features: serde_json::Value,
        derived_from: &[String],
    ) -> Result<(), LedgerError> {
        if *self.stream_gap_pending.get(stream_id).unwrap_or(&false) {
            return Err(LedgerError::SensorGapNotAcknowledged(stream_id.to_string()));
        }
        self.append_raw_with_dependencies(
            id,
            "belief",
            serde_json::json!({ "stream_id": stream_id, "ts_us": ts_us, "derived_from": derived_from, "features": features }),
            derived_from,
        )
    }

    /// Append a structural derived record (for example an Episode or Pattern) with explicit
    /// provenance edges. The caller owns the record schema; this method only provides the same
    /// atomic hash-chain plus source-edge write used by persisted beliefs.
    pub fn append_derived_record(
        &mut self,
        id: impl Into<String>,
        kind: &str,
        payload: serde_json::Value,
        derived_from: &[String],
    ) -> Result<(), LedgerError> {
        self.append_raw_with_dependencies(id, kind, payload, derived_from)
    }

    pub fn events(&self) -> Result<Vec<Event>, LedgerError> {
        self.load_events()
    }

    pub fn contains_event_id(&self, event_id: &str) -> Result<bool, LedgerError> {
        let found = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE id = ?1)",
            [event_id],
            |row| row.get(0),
        )?;
        Ok(found)
    }

    pub fn last_monotonic_sequence(&self, stream_id: &str) -> Option<u64> {
        self.stream_last_monotonic_sequence.get(stream_id).copied()
    }

    /// Read the newest feature-bearing observation for a stream without loading the full ledger.
    /// SQLite's JSON1 extension is bundled with the project, so this remains indexed by the
    /// existing event sequence ordering while keeping the daemon ingest path bounded by the
    /// number of participating streams.
    pub fn latest_observation(
        &self,
        stream_id: &str,
    ) -> Result<Option<LatestObservation>, LedgerError> {
        let mut statement = self.conn.prepare(
            "SELECT id, payload FROM events
             WHERE kind = 'observation' AND json_extract(payload, '$.stream_id') = ?1
             ORDER BY sequence DESC LIMIT 1",
        )?;
        let mut rows = statement.query([stream_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let id: String = row.get(0)?;
        let payload_text: String = row.get(1)?;
        let payload: serde_json::Value = serde_json::from_str(&payload_text)
            .map_err(|error| LedgerError::Database(error.to_string()))?;
        let timestamp_us = payload
            .get("ts_us")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| LedgerError::Database("observation is missing ts_us".to_string()))?;
        Ok(Some(LatestObservation {
            id,
            timestamp_us,
            features: payload
                .get("features")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        }))
    }

    /// Return explicit derivation sources for a persisted node. This is intentionally separate
    /// from `Event.previous_hash`, which represents append order and is not provenance.
    pub fn provenance_sources(&self, derived_id: &str) -> Result<Vec<String>, LedgerError> {
        let mut statement = self.conn.prepare(
            "SELECT source_id FROM provenance_edges WHERE derived_id = ?1 ORDER BY rowid ASC",
        )?;
        let sources = statement
            .query_map([derived_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(sources)
    }

    /// Erase the selected events and every record that explicitly derives from them. The
    /// operation is one SQLite transaction: surviving records retain their IDs and payloads,
    /// while their append-order sequence/hash links are rebuilt from genesis so reopening the
    /// ledger cannot expose a broken chain. This is intentionally a lower-level primitive; the
    /// CLI requires an explicit confirmation before calling it.
    pub fn erase_event_ids(&mut self, event_ids: &[String]) -> Result<usize, LedgerError> {
        let events = self.load_events()?;
        let known_ids: std::collections::HashSet<&str> =
            events.iter().map(|event| event.id.as_str()).collect();
        for id in event_ids {
            if !known_ids.contains(id.as_str()) {
                return Err(LedgerError::UnknownEvent(id.clone()));
            }
        }

        let edges: Vec<(String, String)> = {
            let mut edges = Vec::new();
            let mut statement = self
                .conn
                .prepare("SELECT derived_id, source_id FROM provenance_edges")?;
            for row in statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))? {
                edges.push(row?);
            }
            edges
        };

        let mut erased: std::collections::HashSet<String> = event_ids.iter().cloned().collect();
        loop {
            let mut changed = false;
            for (derived_id, source_id) in &edges {
                if erased.contains(source_id) && erased.insert(derived_id.clone()) {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        if erased.is_empty() {
            return Ok(0);
        }

        // Keep the exact stored payload bytes while rebuilding hashes. Parsing and reserializing
        // JSON here would make a valid noncanonical historical payload change during erase.
        let stored_events = {
            let mut statement = self
                .conn
                .prepare("SELECT id, kind, payload FROM events ORDER BY sequence ASC")?;
            let stored = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            stored
        };
        let survivors: Vec<_> = stored_events
            .into_iter()
            .filter(|(id, _, _)| !erased.contains(id))
            .collect();

        let transaction = self.conn.transaction()?;
        transaction.execute("DELETE FROM provenance_edges", [])?;
        for (derived_id, source_id) in edges {
            if !erased.contains(&derived_id) && !erased.contains(&source_id) {
                transaction.execute(
                    "INSERT INTO provenance_edges (derived_id, source_id) VALUES (?1, ?2)",
                    rusqlite::params![derived_id, source_id],
                )?;
            }
        }
        for id in &erased {
            transaction.execute("DELETE FROM events WHERE id = ?1", [id])?;
        }
        // Move all remaining rows out of the nonnegative sequence namespace before assigning
        // their compact post-erase sequence numbers.
        transaction.execute("UPDATE events SET sequence = -sequence - 1", [])?;
        let mut previous_hash = GENESIS_HASH.to_string();
        for (sequence, (id, _, payload)) in survivors.iter().enumerate() {
            let hash = compute_serialized_hash(&previous_hash, payload);
            transaction.execute(
                "UPDATE events SET sequence = ?1, previous_hash = ?2, hash = ?3 WHERE id = ?4",
                rusqlite::params![sequence as i64, previous_hash, hash, id],
            )?;
            previous_hash = hash;
        }
        transaction.commit()?;
        self.rebuild_runtime_state()?;
        Ok(erased.len())
    }

    /// §87/§88 step 3/§109: verify the persisted hash chain has not been tampered with or torn
    /// mid-write, e.g. on crash-recovery reopen. Hash the exact payload bytes stored in SQLite;
    /// parsing and reserializing JSON can change object-key order without changing its meaning,
    /// which must not make a valid historical chain look corrupted.
    pub fn verify_chain(&self) -> Result<(), LedgerError> {
        let mut previous_hash = GENESIS_HASH.to_string();
        let mut statement = self.conn.prepare(
            "SELECT sequence, payload, previous_hash, hash FROM events ORDER BY sequence ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)? as u64,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (sequence, payload, event_previous_hash, event_hash) = row?;
            if event_previous_hash != previous_hash {
                return Err(LedgerError::ChainBroken(sequence));
            }
            let expected = compute_serialized_hash(&previous_hash, &payload);
            if expected != event_hash {
                return Err(LedgerError::ChainBroken(sequence));
            }
            previous_hash = event_hash;
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
    fn default_db_path_ends_with_the_section_17_relative_path() {
        let path = default_db_path();
        assert!(path.ends_with("Library/Application Support/Liminal/liminal.db"));
    }

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
    fn pending_gap_streams_are_sorted_and_exclude_acknowledged_streams() {
        let mut ledger = Ledger::new(5_000_000);
        ledger.append_observation("camera-1", "camera", 0);
        ledger.append_observation("camera-2", "camera", 20_000_000);
        ledger.append_observation("wifi-1", "wifi", 0);
        ledger.append_observation("wifi-2", "wifi", 20_000_000);
        ledger.record_sensor_gap("wifi-gap", "wifi", 20_000_000);

        assert_eq!(
            ledger.unacknowledged_sensor_gap_streams(),
            vec!["camera".to_string()]
        );
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

    #[test]
    fn sqlite_erase_cascades_provenance_and_rebuilds_chain_on_reopen() {
        let path = temp_db_path("erase-cascade");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "camera", 1).unwrap();
            ledger.append_observation("obs_2", "camera", 2).unwrap();
            ledger
                .append_belief_with_features_and_evidence(
                    "belief_1",
                    "fusion",
                    2,
                    serde_json::json!({"occupancy_probability": 0.8}),
                    &["obs_1".to_string()],
                )
                .unwrap();
            ledger
                .append_derived_record(
                    "episode_1",
                    "episode",
                    serde_json::json!({"duration_us": 1}),
                    &["belief_1".to_string()],
                )
                .unwrap();
            ledger.append_observation("obs_3", "wifi", 3).unwrap();

            assert_eq!(ledger.erase_event_ids(&["obs_1".to_string()]).unwrap(), 3);
            assert!(ledger.verify_chain().is_ok());
            assert_eq!(ledger.events().unwrap().len(), 2);
        }
        {
            let ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            assert!(ledger.verify_chain().is_ok());
            let events = ledger.events().unwrap();
            assert_eq!(
                events
                    .iter()
                    .map(|event| event.id.as_str())
                    .collect::<Vec<_>>(),
                ["obs_2", "obs_3"]
            );
            assert!(ledger.provenance_sources("belief_1").unwrap().is_empty());
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn sqlite_erase_unknown_event_is_atomic() {
        let path = temp_db_path("erase-unknown");
        let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
        ledger.append_observation("obs_1", "camera", 1).unwrap();
        assert_eq!(
            ledger.erase_event_ids(&["missing".to_string()]),
            Err(LedgerError::UnknownEvent("missing".to_string()))
        );
        assert_eq!(ledger.events().unwrap().len(), 1);
        assert!(ledger.verify_chain().is_ok());
        drop(ledger);
        std::fs::remove_file(path).unwrap();
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
    fn migration_creates_events_and_provenance_tables_on_empty_db() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1")
            .unwrap();
        assert!(stmt.exists(["events"]).unwrap());
        assert!(stmt.exists(["provenance_edges"]).unwrap());
        assert!(stmt.exists(["schema_migrations"]).unwrap());

        let version: i64 = conn
            .query_row("SELECT version FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn sqlite_provenance_edges_survive_close_and_reopen() {
        let path = temp_db_path("provenance");
        {
            let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
            ledger.append_observation("obs_1", "camera", 0).unwrap();
            ledger
                .append_belief_with_features_and_evidence(
                    "belief_1",
                    "fusion",
                    1_000_000,
                    serde_json::json!({"occupancy_probability": 0.9}),
                    &["obs_1".to_string()],
                )
                .unwrap();
            assert_eq!(
                ledger.provenance_sources("belief_1").unwrap(),
                vec!["obs_1".to_string()]
            );
        }
        let ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
        assert_eq!(
            ledger.provenance_sources("belief_1").unwrap(),
            vec!["obs_1".to_string()]
        );
        std::fs::remove_file(path).unwrap();
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

        assert!(matches!(
            SqliteLedger::open(&path, 5_000_000),
            Err(LedgerError::ChainBroken(2))
        ));

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

        assert!(matches!(
            SqliteLedger::open(&path, 5_000_000),
            Err(LedgerError::ChainBroken(1))
        ));

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

    #[test]
    fn sqlite_ledger_persists_structural_derived_records_and_sources() {
        let path = temp_db_path("derived-record");
        {
            let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
            ledger.append_observation("obs_1", "camera", 0).unwrap();
            ledger
                .append_derived_record(
                    "episode_1",
                    "episode",
                    serde_json::json!({"start_ts_us": 0, "end_ts_us": 1}),
                    &["obs_1".to_string()],
                )
                .unwrap();
        }

        let ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
        let events = ledger.events().unwrap();
        assert_eq!(events[1].kind, "episode");
        assert_eq!(ledger.provenance_sources("episode_1").unwrap(), ["obs_1"]);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn sqlite_ledger_surfaces_a_temporary_database_lock() {
        let path = temp_db_path("locked");
        let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
        let blocker = Connection::open(&path).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE").unwrap();

        let result = ledger.append_observation("blocked", "camera", 1);

        assert!(matches!(result, Err(LedgerError::Database(_))));
        blocker.execute_batch("ROLLBACK").unwrap();
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn sqlite_ledger_reads_the_latest_feature_observation_without_loading_events() {
        let path = temp_db_path("latest-observation");
        let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
        ledger
            .append_observation_with_features(
                "camera-1",
                "camera",
                1,
                serde_json::json!({"body_count": "zero"}),
            )
            .unwrap();
        ledger
            .append_observation_with_features(
                "camera-2",
                "camera",
                2,
                serde_json::json!({"body_count": "one"}),
            )
            .unwrap();

        let latest = ledger.latest_observation("camera").unwrap().unwrap();
        assert_eq!(latest.id, "camera-2");
        assert_eq!(latest.timestamp_us, 2);
        assert_eq!(latest.features["body_count"], "one");
        assert_eq!(ledger.last_monotonic_sequence("camera"), None);

        drop(ledger);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn sqlite_ledger_reopens_payloads_with_noncanonical_json_key_order() {
        let path = temp_db_path("json-order");
        {
            let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
        }

        let raw_payload = r#"{"z":1,"a":2,"stream_id":"wifi","ts_us":100}"#;
        {
            let conn = Connection::open(&path).unwrap();
            let previous_hash: String = conn
                .query_row("SELECT hash FROM events WHERE sequence = 0", [], |row| {
                    row.get(0)
                })
                .unwrap();
            let mut hasher = blake3::Hasher::new();
            hasher.update(previous_hash.as_bytes());
            hasher.update(raw_payload.as_bytes());
            conn.execute(
                "INSERT INTO events (id, sequence, kind, payload, previous_hash, hash)
                 VALUES ('obs_2', 1, 'observation', ?1, ?2, ?3)",
                rusqlite::params![
                    raw_payload,
                    previous_hash,
                    hasher.finalize().to_hex().to_string()
                ],
            )
            .unwrap();
        }

        assert!(SqliteLedger::open(&path, 5_000_000).is_ok());
        std::fs::remove_file(&path).unwrap();
    }
}
