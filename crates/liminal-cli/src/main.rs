//! `liminal` CLI: read-only data-layer inspection tools over a persisted `SqliteLedger`.
//!
//! Master plan reference: §82 (CLI Contract), §62 (Provenance Graph — see `explain` below for
//! why this walks the hash chain rather than `liminal_ledger::ProvenanceGraph`), §133
//! (`liminal privacy audit` behavior).
//!
//! `explain <id>` does not use `liminal_ledger::ProvenanceGraph`: that type is a separate,
//! purely in-memory structure with no connection to `SqliteLedger` and no persisted
//! `depends_on` edges to walk. Instead it walks the hash chain that IS persisted — each
//! `Event.previous_hash` links to its predecessor's `hash` — back to the genesis event
//! (`previous_hash == "0"`), which is a legitimate provenance drilldown over real data.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use liminal_ledger::{Event, LedgerError, SqliteLedger};
use liminal_policy::privacy_audit::scan_json_for_forbidden_keys;

#[derive(Parser)]
#[command(name = "liminal")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Privacy-boundary checks over persisted data.
    Privacy {
        #[command(subcommand)]
        command: PrivacyCommand,
    },
    /// Inspect persisted ledger events.
    Events {
        #[command(subcommand)]
        command: EventsCommand,
    },
    /// Walk an event's provenance chain back to genesis.
    Explain {
        id: String,
        #[arg(long)]
        db: PathBuf,
    },
}

#[derive(Subcommand)]
enum PrivacyCommand {
    /// Scan every stored event payload for forbidden keys (§133).
    Audit {
        #[arg(long)]
        db: PathBuf,
    },
}

#[derive(Subcommand)]
enum EventsCommand {
    /// List every event in the ledger.
    List {
        #[arg(long)]
        db: PathBuf,
    },
    /// Show the full record for one event.
    Show {
        id: String,
        #[arg(long)]
        db: PathBuf,
    },
}

/// One forbidden-key hit found while auditing a persisted event.
struct AuditHit {
    event_id: String,
    key_path: String,
}

/// §133: scan every event payload in the ledger at `db` for forbidden keys. Returns the empty
/// vec if the ledger is clean.
fn audit_privacy(db: &Path) -> Result<Vec<AuditHit>, LedgerError> {
    let ledger = SqliteLedger::open(db, i64::MAX)?;
    let mut hits = Vec::new();
    for event in ledger.events()? {
        for key_path in scan_json_for_forbidden_keys(&event.payload) {
            hits.push(AuditHit {
                event_id: event.id.clone(),
                key_path,
            });
        }
    }
    Ok(hits)
}

fn list_events(db: &Path) -> Result<Vec<Event>, LedgerError> {
    let ledger = SqliteLedger::open(db, i64::MAX)?;
    ledger.events()
}

fn show_event(db: &Path, id: &str) -> Result<Option<Event>, LedgerError> {
    let ledger = SqliteLedger::open(db, i64::MAX)?;
    Ok(ledger.events()?.into_iter().find(|e| e.id == id))
}

/// Walk the hash chain from the event `id` back to genesis. Returns `None` if `id` is unknown.
/// The returned chain starts at `id` and ends at the genesis event (`previous_hash == "0"`).
fn explain_chain(db: &Path, id: &str) -> Result<Option<Vec<Event>>, LedgerError> {
    let events = SqliteLedger::open(db, i64::MAX)?.events()?;
    let Some(start) = events.iter().find(|e| e.id == id) else {
        return Ok(None);
    };

    let mut chain = vec![start.clone()];
    let mut previous_hash = start.previous_hash.clone();
    while previous_hash != "0" {
        let Some(predecessor) = events.iter().find(|e| e.hash == previous_hash) else {
            break;
        };
        previous_hash = predecessor.previous_hash.clone();
        chain.push(predecessor.clone());
    }
    Ok(Some(chain))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Privacy {
            command: PrivacyCommand::Audit { db },
        } => run_privacy_audit(&db),
        Command::Events {
            command: EventsCommand::List { db },
        } => run_events_list(&db),
        Command::Events {
            command: EventsCommand::Show { id, db },
        } => run_events_show(&db, &id),
        Command::Explain { id, db } => run_explain(&db, &id),
    }
}

fn run_privacy_audit(db: &Path) -> ExitCode {
    match audit_privacy(db) {
        Ok(hits) if hits.is_empty() => {
            println!("privacy audit: clean, no forbidden keys found");
            ExitCode::SUCCESS
        }
        Ok(hits) => {
            for hit in &hits {
                println!(
                    "forbidden key found: event {} at {}",
                    hit.event_id, hit.key_path
                );
            }
            println!("privacy audit: {} forbidden key(s) found", hits.len());
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn format_event_list_line(event: &Event) -> String {
    let ts_us = event.payload.get("ts_us").and_then(|v| v.as_i64());
    match ts_us {
        Some(ts) => format!("{}\t{}\t{}\t{}", event.id, event.kind, event.sequence, ts),
        None => format!("{}\t{}\t{}", event.id, event.kind, event.sequence),
    }
}

fn run_events_list(db: &Path) -> ExitCode {
    match list_events(db) {
        Ok(events) => {
            for event in &events {
                println!("{}", format_event_list_line(event));
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_events_show(db: &Path, id: &str) -> ExitCode {
    match show_event(db, id) {
        Ok(Some(event)) => {
            println!("id:            {}", event.id);
            println!("sequence:      {}", event.sequence);
            println!("kind:          {}", event.kind);
            println!("payload:       {}", event.payload);
            println!("previous_hash: {}", event.previous_hash);
            println!("hash:          {}", event.hash);
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("event not found: {id}");
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_explain(db: &Path, id: &str) -> ExitCode {
    match explain_chain(db, id) {
        Ok(Some(chain)) => {
            for event in &chain {
                let ts_us = event.payload.get("ts_us").and_then(|v| v.as_i64());
                match ts_us {
                    Some(ts) => println!("{}\t{}\tts_us={}", event.id, event.kind, ts),
                    None => println!("{}\t{}", event.id, event.kind),
                }
            }
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("event not found: {id}");
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn temp_db_path(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("liminal-cli-test-{name}-{unique}.db"))
    }

    /// Inserts a leaked-key row directly via raw SQL: `SqliteLedger`'s own append methods only
    /// ever produce clean observation/gap/belief payloads, so a forbidden-key scenario must be
    /// injected below the ledger API, matching the `events` table schema from Task 3's
    /// migration.
    fn insert_raw_event(
        db: &Path,
        id: &str,
        sequence: i64,
        kind: &str,
        payload: &serde_json::Value,
        previous_hash: &str,
        hash: &str,
    ) {
        let conn = Connection::open(db).unwrap();
        conn.execute(
            "INSERT INTO events (id, sequence, kind, payload, previous_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, sequence, kind, payload.to_string(), previous_hash, hash],
        )
        .unwrap();
    }

    #[test]
    fn privacy_audit_reports_clean_when_no_forbidden_keys_present() {
        let path = temp_db_path("audit-clean");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
        }

        let hits = audit_privacy(&path).unwrap();
        assert!(hits.is_empty());

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn privacy_audit_detects_a_leaked_forbidden_key_in_a_persisted_record() {
        let path = temp_db_path("audit-leaky");
        {
            // Open once so the migration runs and the table exists.
            SqliteLedger::open(&path, i64::MAX).unwrap();
        }
        insert_raw_event(
            &path,
            "obs_1",
            0,
            "observation",
            &serde_json::json!({ "stream_id": "wifi", "ssid": "HomeNetwork" }),
            "0",
            "deadbeef",
        );

        let hits = audit_privacy(&path).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].event_id, "obs_1");
        assert_eq!(hits[0].key_path, "$.ssid");

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn run_privacy_audit_exits_success_on_clean_ledger() {
        let path = temp_db_path("audit-exit-clean");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
        }

        assert_eq!(run_privacy_audit(&path), ExitCode::SUCCESS);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn run_privacy_audit_exits_failure_on_leaked_key() {
        let path = temp_db_path("audit-exit-leaky");
        {
            SqliteLedger::open(&path, i64::MAX).unwrap();
        }
        insert_raw_event(
            &path,
            "obs_1",
            0,
            "observation",
            &serde_json::json!({ "bssid": "AA:BB:CC:DD:EE:FF" }),
            "0",
            "deadbeef",
        );

        assert_eq!(run_privacy_audit(&path), ExitCode::FAILURE);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn events_list_returns_every_event_in_sequence_order() {
        let path = temp_db_path("list");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
            ledger
                .append_observation("obs_2", "wifi", 1_000_000)
                .unwrap();
        }

        let events = list_events(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, "obs_1");
        assert_eq!(events[1].id, "obs_2");

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn events_list_line_includes_the_timestamp() {
        let path = temp_db_path("list-ts");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger
                .append_observation("obs_1", "wifi", 1_000_000)
                .unwrap();
        }

        let events = list_events(&path).unwrap();
        let line = format_event_list_line(&events[0]);
        assert_eq!(line, "obs_1\tobservation\t0\t1000000");

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn events_show_finds_the_matching_event() {
        let path = temp_db_path("show-found");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
        }

        let event = show_event(&path, "obs_1").unwrap().unwrap();
        assert_eq!(event.kind, "observation");

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn events_show_returns_none_for_unknown_id() {
        let path = temp_db_path("show-missing");
        {
            SqliteLedger::open(&path, i64::MAX).unwrap();
        }

        assert!(show_event(&path, "nope").unwrap().is_none());

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn explain_walks_the_hash_chain_back_to_genesis() {
        let path = temp_db_path("explain");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
            ledger
                .append_observation("obs_2", "wifi", 1_000_000)
                .unwrap();
            ledger.append_belief("belief_1", "wifi", 1_000_100).unwrap();
        }

        let chain = explain_chain(&path, "belief_1").unwrap().unwrap();
        let ids: Vec<&str> = chain.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["belief_1", "obs_2", "obs_1"]);
        assert_eq!(chain.last().unwrap().previous_hash, "0");

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn explain_returns_none_for_unknown_id() {
        let path = temp_db_path("explain-missing");
        {
            SqliteLedger::open(&path, i64::MAX).unwrap();
        }

        assert!(explain_chain(&path, "nope").unwrap().is_none());

        std::fs::remove_file(&path).unwrap();
    }
}
