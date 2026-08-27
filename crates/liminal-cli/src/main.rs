//! `liminal` CLI: read-only data-layer inspection tools over a persisted `SqliteLedger`.
//!
//! Master plan reference: §82 (CLI Contract), §133 (`liminal privacy audit` behavior).
//!
//! ## `events history <id>` is NOT §62 provenance
//!
//! `Event.previous_hash` links each event to whatever was appended immediately before it, in
//! GLOBAL APPEND ORDER across every stream and kind — it exists for hash-chain integrity
//! (§87), not derivation. `events history <id>` walks that chain back to genesis
//! (`previous_hash == "0"`) and is useful for append-order/integrity inspection, but it is NOT
//! the §62 Provenance Graph: it does not tell you what evidence a claim was derived from, only
//! what was written before it. An unrelated event from a different stream or sensor, if
//! appended between two related ones, would appear in this history exactly as if it were
//! evidence — because from the chain's point of view, it's indistinguishable from one.
//!
//! Real per-claim provenance (§62) is exposed by `events provenance`, which reads explicit
//! `derived_from` edges recorded at write time. It remains separate from append-order history.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use liminal_ledger::{Event, LedgerError, SqliteLedger, DEFAULT_MAX_SILENT_GAP_US};
use liminal_memory::{
    evaluate_calibration, replay_memory, segment_occupancy, CalibrationSample, SegmentedEvent,
};
use liminal_policy::privacy_audit::scan_json_for_forbidden_keys;
use liminal_policy::{eligible_for_deletion, RecordKind, RetentionPolicy};
use serde::{Deserialize, Serialize};

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
    /// Score daemon beliefs against explicit human/reference labels.
    Calibration {
        #[command(subcommand)]
        command: CalibrationCommand,
    },
    /// Explicit operator recovery actions over canonical state.
    Recovery {
        #[command(subcommand)]
        command: RecoveryCommand,
    },
    /// Materialize deterministic structural memory records from daemon beliefs.
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Run a deterministic, local-only field-note agent over structured evidence.
    Agents {
        #[command(subcommand)]
        command: AgentsCommand,
    },
    /// Export a privacy-audited, provenance-aware local JSON bundle.
    Export {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Include only records at or after this payload timestamp, in microseconds.
        #[arg(long)]
        since_us: Option<i64>,
        /// Include only records at or before this payload timestamp, in microseconds.
        #[arg(long)]
        until_us: Option<i64>,
    },
    /// Report age-eligible records without deleting anything.
    Retention {
        #[command(subcommand)]
        command: RetentionCommand,
    },
}

#[derive(Subcommand)]
enum RetentionCommand {
    /// Preview the default §85 retention policy at a supplied or current timestamp.
    Preview {
        #[arg(long)]
        db: PathBuf,
        /// Evaluation timestamp in Unix microseconds; defaults to the current clock.
        #[arg(long)]
        now_us: Option<i64>,
    },
    /// Emit the exact age-eligible record IDs for operator review; never deletes records.
    Plan {
        #[arg(long)]
        db: PathBuf,
        /// Evaluation timestamp in Unix microseconds; defaults to the current clock.
        #[arg(long)]
        now_us: Option<i64>,
        /// Optional path for the JSON plan. Without it, the plan is printed to stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Apply the canonical retention plan after explicit operator confirmation.
    Apply {
        #[arg(long)]
        db: PathBuf,
        /// Evaluation timestamp in Unix microseconds; defaults to the current clock.
        #[arg(long)]
        now_us: Option<i64>,
        /// Required acknowledgement for this irreversible local operation.
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
enum MemoryCommand {
    /// Persist Episode and Pattern records with explicit source edges.
    Replay {
        #[arg(long)]
        db: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum AgentName {
    Archivist,
    Ethnographer,
    Skeptic,
    Cartographer,
    Poet,
}

#[derive(Subcommand)]
enum AgentsCommand {
    /// Run one Tier-0 agent and persist its auditable output.
    Run {
        #[arg(value_enum)]
        agent: AgentName,
        #[arg(long)]
        db: PathBuf,
    },
}

#[derive(Subcommand)]
enum RecoveryCommand {
    /// Acknowledge existing sensor gaps so new beliefs may resume.
    AcknowledgeGaps {
        #[arg(long)]
        db: PathBuf,
        /// Limit acknowledgment to these streams; omit to acknowledge every pending stream.
        #[arg(long = "stream")]
        streams: Vec<String>,
    },
}

#[derive(Subcommand)]
enum CalibrationCommand {
    /// Match timestamped JSONL labels to persisted fusion beliefs and emit metrics.
    Score {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        labels: PathBuf,
        /// Maximum absolute timestamp distance between a label and a belief, in microseconds.
        #[arg(long, default_value_t = 500_000)]
        max_offset_us: i64,
        /// Optional path for the JSON report. Without it, the report is printed to stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum PrivacyCommand {
    /// Scan stored payloads and canonical debug captures for forbidden data (§133).
    Audit {
        #[arg(long)]
        db: PathBuf,
    },
    /// Erase records in an inclusive timestamp range and cascade to their dependents.
    Erase {
        #[arg(long)]
        db: PathBuf,
        /// Include records at or after this payload timestamp, in microseconds.
        #[arg(long)]
        since_us: Option<i64>,
        /// Include records at or before this payload timestamp, in microseconds.
        #[arg(long)]
        until_us: Option<i64>,
        /// Required acknowledgement for this irreversible local operation.
        #[arg(long)]
        confirm: bool,
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
    /// Walk an event's append-order hash-chain history back to genesis. NOT a provenance/
    /// derivation graph -- see the module doc comment.
    History {
        id: String,
        #[arg(long)]
        db: PathBuf,
    },
    /// Show explicit derivation sources, not append-order predecessors.
    Provenance {
        id: String,
        #[arg(long)]
        db: PathBuf,
    },
    /// Walk explicit derivation edges from an event toward its source records.
    ProvenanceTree {
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

/// One explicit label from a human annotator or approved reference instrument. This file is
/// intentionally outside the live sensor path: a sensor feature must never become its own label.
#[derive(Debug, Deserialize)]
struct CalibrationLabel {
    ts_us: i64,
    occupied: bool,
}

#[derive(Debug, Serialize)]
struct CalibrationScoreReport {
    labels_total: usize,
    matched_labels: usize,
    unmatched_labels: usize,
    max_offset_us: i64,
    accuracy: f64,
    brier_score: f64,
    positive_precision: f64,
    positive_recall: f64,
}

#[derive(Debug, Serialize)]
struct ExportRecord {
    id: String,
    sequence: u64,
    kind: String,
    payload: serde_json::Value,
    previous_hash: String,
    hash: String,
    provenance_sources: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExportBundle {
    schema: &'static str,
    records: Vec<ExportRecord>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct RetentionPreview {
    observations_eligible: usize,
    belief_frames_eligible: usize,
    events_eligible: usize,
    protected_derived_records: usize,
}

#[derive(Debug, Serialize)]
struct RetentionCandidate {
    id: String,
    sequence: u64,
    kind: String,
    recorded_at_us: i64,
    provenance_sources: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RetentionPlan {
    schema: &'static str,
    evaluated_at_us: i64,
    candidates: Vec<RetentionCandidate>,
    protected_derived_records: usize,
}

/// §133: scan every event payload in the ledger at `db` for forbidden keys and inspect the
/// sibling `debug-captures` directory for raw media files. Returns the empty vec if the ledger
/// and canonical capture directory are clean. This is read-only and never removes captures.
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
    let debug_captures = db
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("debug-captures");
    for path in raw_capture_files(&debug_captures).map_err(|error| {
        LedgerError::Database(format!(
            "scan debug captures {}: {error}",
            debug_captures.display()
        ))
    })? {
        hits.push(AuditHit {
            event_id: path.display().to_string(),
            key_path: "filesystem raw capture".to_string(),
        });
    }
    Ok(hits)
}

fn raw_capture_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    const RAW_EXTENSIONS: &[&str] = &[
        "aif", "aiff", "caf", "jpeg", "jpg", "m4a", "mov", "mp3", "mp4", "png", "wav",
    ];
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            files.extend(raw_capture_files(&path)?);
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| {
                    RAW_EXTENSIONS
                        .iter()
                        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
                })
                .unwrap_or(false)
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn list_events(db: &Path) -> Result<Vec<Event>, LedgerError> {
    let ledger = SqliteLedger::open(db, i64::MAX)?;
    ledger.events()
}

fn show_event(db: &Path, id: &str) -> Result<Option<Event>, LedgerError> {
    let ledger = SqliteLedger::open(db, i64::MAX)?;
    Ok(ledger.events()?.into_iter().find(|e| e.id == id))
}

/// Walk the append-order hash chain from the event `id` back to genesis. Returns `None` if `id`
/// is unknown. The returned chain starts at `id` and ends at the genesis event
/// (`previous_hash == "0"`). This is NOT a provenance/derivation query -- see the module doc
/// comment for why.
fn event_history(db: &Path, id: &str) -> Result<Option<Vec<Event>>, LedgerError> {
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

fn belief_samples(db: &Path) -> Result<Vec<(i64, f64)>, LedgerError> {
    let events = SqliteLedger::open(db, i64::MAX)?.events()?;
    Ok(events
        .into_iter()
        .filter(|event| event.kind == "belief")
        .filter_map(|event| {
            let ts_us = event.payload.get("ts_us")?.as_i64()?;
            let probability = event
                .payload
                .get("features")?
                .get("occupancy_probability")?
                .as_f64()?;
            Some((ts_us, probability))
        })
        .collect())
}

fn score_calibration(
    db: &Path,
    labels_path: &Path,
    max_offset_us: i64,
) -> Result<CalibrationScoreReport, String> {
    if max_offset_us < 0 {
        return Err("--max-offset-us must be non-negative".to_string());
    }
    let text = fs::read_to_string(labels_path)
        .map_err(|error| format!("read labels {}: {error}", labels_path.display()))?;
    let mut labels = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        labels.push(
            serde_json::from_str::<CalibrationLabel>(line)
                .map_err(|error| format!("parse labels line {}: {error}", line_number + 1))?,
        );
    }

    let beliefs = belief_samples(db).map_err(|error| format!("read beliefs: {error}"))?;
    let mut used_beliefs = vec![false; beliefs.len()];
    let mut samples = Vec::new();
    for label in &labels {
        let nearest = beliefs
            .iter()
            .enumerate()
            .filter(|(index, (ts_us, _))| {
                !used_beliefs[*index] && (label.ts_us - *ts_us).abs() <= max_offset_us
            })
            .min_by_key(|(_, (ts_us, _))| (label.ts_us - *ts_us).abs());
        if let Some((index, (_, probability))) = nearest {
            used_beliefs[index] = true;
            samples.push(CalibrationSample {
                predicted_probability: *probability,
                observed_occupied: label.occupied,
            });
        }
    }
    let metrics = evaluate_calibration(&samples);
    Ok(CalibrationScoreReport {
        labels_total: labels.len(),
        matched_labels: samples.len(),
        unmatched_labels: labels.len() - samples.len(),
        max_offset_us,
        accuracy: metrics.accuracy,
        brier_score: metrics.brier_score,
        positive_precision: metrics.positive_precision,
        positive_recall: metrics.positive_recall,
    })
}

fn run_calibration_score(
    db: &Path,
    labels: &Path,
    max_offset_us: i64,
    output: Option<&Path>,
) -> ExitCode {
    match score_calibration(db, labels, max_offset_us) {
        Ok(report) => {
            let rendered = match serde_json::to_string_pretty(&report) {
                Ok(json) => json,
                Err(error) => {
                    eprintln!("error: serialize calibration report: {error}");
                    return ExitCode::FAILURE;
                }
            };
            if let Some(path) = output {
                match fs::File::create(path)
                    .and_then(|mut file| file.write_all(rendered.as_bytes()))
                {
                    Ok(()) => println!("calibration report: {}", path.display()),
                    Err(error) => {
                        eprintln!(
                            "error: write calibration report {}: {error}",
                            path.display()
                        );
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                println!("{rendered}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn acknowledge_gaps(db: &Path, requested_streams: &[String]) -> Result<Vec<String>, String> {
    let mut ledger =
        SqliteLedger::open(db, DEFAULT_MAX_SILENT_GAP_US).map_err(|error| error.to_string())?;
    let pending = ledger.unacknowledged_sensor_gap_streams();
    let selected: Vec<String> = if requested_streams.is_empty() {
        pending
    } else {
        requested_streams
            .iter()
            .filter(|stream| pending.iter().any(|candidate| candidate == *stream))
            .cloned()
            .collect()
    };
    let ts_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("clock before Unix epoch: {error}"))?
        .as_micros() as i64;
    for (index, stream) in selected.iter().enumerate() {
        ledger
            .record_sensor_gap(
                format!("operator-gap-ack-{stream}-{ts_us}-{index}"),
                stream,
                ts_us,
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(selected)
}

fn run_acknowledge_gaps(db: &Path, streams: &[String]) -> ExitCode {
    match acknowledge_gaps(db, streams) {
        Ok(acknowledged) => {
            if acknowledged.is_empty() {
                println!("sensor-gap recovery: no matching pending gaps");
            } else {
                println!(
                    "sensor-gap recovery: acknowledged {}",
                    acknowledged.join(", ")
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn source_ids_for_segment(segment: &SegmentedEvent, beliefs: &[(String, i64)]) -> Vec<String> {
    beliefs
        .iter()
        .filter(|(_, timestamp_us)| {
            *timestamp_us >= segment.start_ts_us && *timestamp_us <= segment.end_ts_us
        })
        .map(|(id, _)| id.clone())
        .collect()
}

fn should_materialize(existing_ids: &HashSet<String>, id: &str) -> bool {
    !existing_ids.contains(id)
}

fn materialize_memory(db: &Path) -> Result<(usize, usize, usize, usize), String> {
    let mut ledger =
        SqliteLedger::open(db, DEFAULT_MAX_SILENT_GAP_US).map_err(|error| error.to_string())?;
    let events = ledger.events().map_err(|error| error.to_string())?;
    let mut beliefs: Vec<(String, i64, f64)> = events
        .iter()
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
                event.id.clone(),
                event.payload.get("ts_us")?.as_i64()?,
                event
                    .payload
                    .get("features")?
                    .get("occupancy_probability")?
                    .as_f64()?,
            ))
        })
        .collect();
    beliefs.sort_unstable_by_key(|(_, timestamp_us, _)| *timestamp_us);
    let samples: Vec<(i64, f64)> = beliefs
        .iter()
        .map(|(_, timestamp_us, probability)| (*timestamp_us, *probability))
        .collect();
    let replay = replay_memory(&segment_occupancy(&samples, &Default::default()));
    let belief_ids_and_timestamps: Vec<(String, i64)> = beliefs
        .iter()
        .map(|(id, timestamp_us, _)| (id.clone(), *timestamp_us))
        .collect();
    let existing: HashSet<String> = events.into_iter().map(|event| event.id).collect();
    let mut written_episodes = 0;
    let mut episode_sources = Vec::with_capacity(replay.episodes.len());
    for (index, episode) in replay.episodes.iter().enumerate() {
        let mut sources = Vec::new();
        for event_index in &episode.event_indices {
            sources.extend(source_ids_for_segment(
                &replay.events[*event_index],
                &belief_ids_and_timestamps,
            ));
        }
        sources.sort();
        sources.dedup();
        let id = format!(
            "memory-episode-{index}-{}-{}",
            episode.start_ts_us, episode.end_ts_us
        );
        if should_materialize(&existing, &id) {
            ledger
                .append_derived_record(
                    &id,
                    "episode",
                    serde_json::json!({
                        "schema": "liminal.memory.episode.v1",
                        "record": episode,
                    }),
                    &sources,
                )
                .map_err(|error| error.to_string())?;
            written_episodes += 1;
        }
        episode_sources.push(sources);
    }
    let mut written_patterns = 0;
    for (index, pattern) in replay.patterns.iter().enumerate() {
        let mut sources = pattern
            .episode_indices
            .iter()
            .flat_map(|episode_index| episode_sources[*episode_index].iter().cloned())
            .collect::<Vec<_>>();
        sources.sort();
        sources.dedup();
        let id = format!("memory-pattern-{index}-{}", pattern.signature);
        if should_materialize(&existing, &id) {
            ledger
                .append_derived_record(
                    &id,
                    "pattern",
                    serde_json::json!({
                        "schema": "liminal.memory.pattern.v1",
                        "record": pattern,
                    }),
                    &sources,
                )
                .map_err(|error| error.to_string())?;
            written_patterns += 1;
        }
    }
    Ok((
        replay.episodes.len(),
        written_episodes,
        replay.patterns.len(),
        written_patterns,
    ))
}

fn run_memory_replay(db: &Path) -> ExitCode {
    match materialize_memory(db) {
        Ok((episodes, written_episodes, patterns, written_patterns)) => {
            println!(
                "memory replay: {episodes} Episode(s), {patterns} Pattern bucket(s); wrote {written_episodes} Episode(s), {written_patterns} Pattern bucket(s)"
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn agent_slug(agent: AgentName) -> &'static str {
    match agent {
        AgentName::Archivist => "archivist",
        AgentName::Ethnographer => "ethnographer",
        AgentName::Skeptic => "skeptic",
        AgentName::Cartographer => "cartographer",
        AgentName::Poet => "poet",
    }
}

fn agent_output(agent: AgentName, evidence_count: usize) -> (&'static str, &'static str, String) {
    match agent {
        AgentName::Archivist => (
            "INFERRED",
            "DRAFT",
            format!(
                "Structured replay supplied {evidence_count} evidence record(s). This run reports what the ledger supports and makes no identity, motive, or location claim."
            ),
        ),
        AgentName::Ethnographer => (
            "INTERPRETED",
            "PENDING_INTERPRETATION",
            format!(
                "A possible place pattern may be worth reviewing from {evidence_count} structured evidence record(s). Alternative explanations remain open; this is not a routine or behavioral conclusion."
            ),
        ),
        AgentName::Skeptic => (
            "INTERPRETED",
            "INSUFFICIENT_EVIDENCE",
            format!(
                "No interpretation is accepted from {evidence_count} structured evidence record(s) without a calibrated history and counter-hypothesis review."
            ),
        ),
        AgentName::Cartographer => (
            "INFERRED",
            "DRAFT",
            format!(
                "The available {evidence_count} structured evidence record(s) do not establish room geometry; any field description remains camera-relative and provisional."
            ),
        ),
        AgentName::Poet => (
            "IMAGINED",
            "ARTIFACT",
            "The room leaves marks before it tells a story.".to_string(),
        ),
    }
}

fn run_agent(db: &Path, agent: AgentName, timestamp_us: i64) -> Result<(String, usize), String> {
    let mut ledger =
        SqliteLedger::open(db, DEFAULT_MAX_SILENT_GAP_US).map_err(|error| error.to_string())?;
    let events = ledger.events().map_err(|error| error.to_string())?;
    let input_ids: Vec<String> = events
        .iter()
        .filter(|event| {
            (event.kind == "belief"
                && event
                    .payload
                    .get("stream_id")
                    .and_then(|value| value.as_str())
                    == Some("fusion"))
                || event.kind == "episode"
                || event.kind == "pattern"
        })
        .map(|event| event.id.clone())
        .collect();
    if input_ids.is_empty() {
        return Err(
            "agent run requires deterministic fusion or memory evidence; no run was persisted"
                .to_string(),
        );
    }
    let (layer, status, text) = agent_output(agent, input_ids.len());
    let id = format!("agent-run-{}-{timestamp_us}", agent_slug(agent));
    ledger
        .append_derived_record(
            &id,
            "agent_run",
            serde_json::json!({
                "schema": "liminal.agent_run.v1",
                "agent_name": agent_slug(agent),
                "model_provider": "tier-0-deterministic",
                "network_mode": "local-only",
                "input_event_ids": input_ids,
                "evidence_ids": input_ids,
                "output": {
                    "layer": layer,
                    "status": status,
                    "text": text,
                },
                "timestamp_us": timestamp_us,
            }),
            &input_ids,
        )
        .map_err(|error| error.to_string())?;
    Ok((id, input_ids.len()))
}

fn run_agent_command(db: &Path, agent: AgentName) -> ExitCode {
    let timestamp_us = match std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros() as i64)
    {
        Ok(timestamp_us) => timestamp_us,
        Err(error) => {
            eprintln!("error: clock before Unix epoch: {error}");
            return ExitCode::FAILURE;
        }
    };
    match run_agent(db, agent, timestamp_us) {
        Ok((id, evidence_count)) => {
            println!(
                "agent run: {} persisted as {id} from {evidence_count} structured evidence record(s)",
                agent_slug(agent)
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Privacy {
            command: PrivacyCommand::Audit { db },
        } => run_privacy_audit(&db),
        Command::Privacy {
            command:
                PrivacyCommand::Erase {
                    db,
                    since_us,
                    until_us,
                    confirm,
                },
        } => run_privacy_erase(&db, since_us, until_us, confirm),
        Command::Events {
            command: EventsCommand::List { db },
        } => run_events_list(&db),
        Command::Events {
            command: EventsCommand::Show { id, db },
        } => run_events_show(&db, &id),
        Command::Events {
            command: EventsCommand::History { id, db },
        } => run_events_history(&db, &id),
        Command::Events {
            command: EventsCommand::Provenance { id, db },
        } => run_events_provenance(&db, &id),
        Command::Events {
            command: EventsCommand::ProvenanceTree { id, db },
        } => run_events_provenance_tree(&db, &id),
        Command::Calibration {
            command:
                CalibrationCommand::Score {
                    db,
                    labels,
                    max_offset_us,
                    output,
                },
        } => run_calibration_score(&db, &labels, max_offset_us, output.as_deref()),
        Command::Recovery {
            command: RecoveryCommand::AcknowledgeGaps { db, streams },
        } => run_acknowledge_gaps(&db, &streams),
        Command::Memory {
            command: MemoryCommand::Replay { db },
        } => run_memory_replay(&db),
        Command::Agents {
            command: AgentsCommand::Run { agent, db },
        } => run_agent_command(&db, agent),
        Command::Export {
            db,
            output,
            since_us,
            until_us,
        } => run_export(&db, &output, since_us, until_us),
        Command::Retention {
            command: RetentionCommand::Preview { db, now_us },
        } => run_retention_preview(&db, now_us),
        Command::Retention {
            command: RetentionCommand::Plan { db, now_us, output },
        } => run_retention_plan(&db, now_us, output.as_deref()),
        Command::Retention {
            command:
                RetentionCommand::Apply {
                    db,
                    now_us,
                    confirm,
                },
        } => run_retention_apply(&db, now_us, confirm),
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

fn privacy_erase(
    db: &Path,
    since_us: Option<i64>,
    until_us: Option<i64>,
    confirm: bool,
) -> Result<usize, String> {
    if !confirm {
        return Err(
            "privacy erase is irreversible; re-run with --confirm after reviewing the range"
                .to_string(),
        );
    }
    if let (Some(since_us), Some(until_us)) = (since_us, until_us) {
        if since_us > until_us {
            return Err("privacy erase requires since_us <= until_us".to_string());
        }
    }
    let mut ledger = SqliteLedger::open(db, i64::MAX).map_err(|error| error.to_string())?;
    let ids = ledger
        .events()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|event| {
            let Some(timestamp_us) = event_timestamp_us(event) else {
                return false;
            };
            since_us.is_none_or(|since| timestamp_us >= since)
                && until_us.is_none_or(|until| timestamp_us <= until)
        })
        .map(|event| event.id)
        .collect::<Vec<_>>();
    ledger
        .erase_event_ids(&ids)
        .map_err(|error| error.to_string())
}

fn run_privacy_erase(
    db: &Path,
    since_us: Option<i64>,
    until_us: Option<i64>,
    confirm: bool,
) -> ExitCode {
    match privacy_erase(db, since_us, until_us, confirm) {
        Ok(count) => {
            println!("privacy erase: removed {count} record(s) and dependent records");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
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

fn run_events_history(db: &Path, id: &str) -> ExitCode {
    match event_history(db, id) {
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

fn run_events_provenance(db: &Path, id: &str) -> ExitCode {
    match SqliteLedger::open(db, i64::MAX).and_then(|ledger| ledger.provenance_sources(id)) {
        Ok(sources) => {
            for source in sources {
                println!("{id}\t<-\t{source}");
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn provenance_tree(db: &Path, id: &str) -> Result<Option<Vec<(usize, Event)>>, LedgerError> {
    let ledger = SqliteLedger::open(db, i64::MAX)?;
    let events = ledger.events()?;
    let by_id: HashMap<String, Event> = events
        .into_iter()
        .map(|event| (event.id.clone(), event))
        .collect();
    if !by_id.contains_key(id) {
        return Ok(None);
    }
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut pending = vec![(0usize, id.to_string())];
    while let Some((depth, current_id)) = pending.pop() {
        if !visited.insert(current_id.clone()) {
            continue;
        }
        let Some(event) = by_id.get(&current_id).cloned() else {
            continue;
        };
        result.push((depth, event));
        let mut sources = ledger.provenance_sources(&current_id)?;
        sources.reverse();
        for source in sources {
            pending.push((depth + 1, source));
        }
    }
    Ok(Some(result))
}

fn run_events_provenance_tree(db: &Path, id: &str) -> ExitCode {
    match provenance_tree(db, id) {
        Ok(Some(records)) => {
            for (depth, event) in records {
                println!("{}{}\t{}", "  ".repeat(depth), event.id, event.kind);
            }
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("event not found: {id}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn event_timestamp_us(event: &Event) -> Option<i64> {
    event
        .payload
        .get("ts_us")
        .and_then(|value| value.as_i64())
        .or_else(|| {
            event
                .payload
                .get("timestamp_us")
                .and_then(|value| value.as_i64())
        })
}

fn export_bundle(
    db: &Path,
    since_us: Option<i64>,
    until_us: Option<i64>,
) -> Result<ExportBundle, String> {
    if let (Some(since), Some(until)) = (since_us, until_us) {
        if since > until {
            return Err("--since-us must not be greater than --until-us".to_string());
        }
    }
    let ledger = SqliteLedger::open(db, i64::MAX).map_err(|error| error.to_string())?;
    let mut records = Vec::new();
    for event in ledger.events().map_err(|error| error.to_string())? {
        if let Some(timestamp_us) = event_timestamp_us(&event) {
            if since_us.is_some_and(|since| timestamp_us < since)
                || until_us.is_some_and(|until| timestamp_us > until)
            {
                continue;
            }
        } else if since_us.is_some() || until_us.is_some() {
            continue;
        }
        if !scan_json_for_forbidden_keys(&event.payload).is_empty() {
            return Err(format!(
                "refusing export: forbidden key in event {}",
                event.id
            ));
        }
        let provenance_sources = ledger
            .provenance_sources(&event.id)
            .map_err(|error| error.to_string())?;
        records.push(ExportRecord {
            id: event.id,
            sequence: event.sequence,
            kind: event.kind,
            payload: event.payload,
            previous_hash: event.previous_hash,
            hash: event.hash,
            provenance_sources,
        });
    }
    Ok(ExportBundle {
        schema: "liminal.export.v1",
        records,
    })
}

fn run_export(db: &Path, output: &Path, since_us: Option<i64>, until_us: Option<i64>) -> ExitCode {
    let bundle = match export_bundle(db, since_us, until_us) {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let rendered = match serde_json::to_string_pretty(&bundle) {
        Ok(rendered) => rendered,
        Err(error) => {
            eprintln!("error: serialize export: {error}");
            return ExitCode::FAILURE;
        }
    };
    match fs::File::create(output).and_then(|mut file| file.write_all(rendered.as_bytes())) {
        Ok(()) => {
            println!(
                "export: wrote {} record(s) to {}",
                bundle.records.len(),
                output.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: write export {}: {error}", output.display());
            ExitCode::FAILURE
        }
    }
}

fn retention_candidate(
    policy: &RetentionPolicy,
    kind: RecordKind,
    recorded_at_us: i64,
    now_us: i64,
) -> bool {
    eligible_for_deletion(policy, kind, recorded_at_us, now_us)
}

fn retention_preview(db: &Path, now_us: i64) -> Result<RetentionPreview, String> {
    let ledger = SqliteLedger::open(db, i64::MAX).map_err(|error| error.to_string())?;
    let policy = RetentionPolicy::DEFAULT;
    let mut preview = RetentionPreview::default();
    for event in ledger.events().map_err(|error| error.to_string())? {
        let Some(recorded_at_us) = event_timestamp_us(&event) else {
            if matches!(event.kind.as_str(), "episode" | "pattern" | "agent_run") {
                preview.protected_derived_records += 1;
            }
            continue;
        };
        let candidate = match event.kind.as_str() {
            "observation" => Some(RecordKind::Observation),
            "belief" => Some(RecordKind::BeliefFrame),
            "event" => Some(RecordKind::Event),
            "episode" | "pattern" | "agent_run" => {
                preview.protected_derived_records += 1;
                None
            }
            _ => None,
        };
        if let Some(kind) = candidate {
            if retention_candidate(&policy, kind, recorded_at_us, now_us) {
                match kind {
                    RecordKind::Observation => preview.observations_eligible += 1,
                    RecordKind::BeliefFrame => preview.belief_frames_eligible += 1,
                    RecordKind::Event => preview.events_eligible += 1,
                }
            }
        }
    }
    Ok(preview)
}

fn retention_plan(db: &Path, now_us: i64) -> Result<RetentionPlan, String> {
    let ledger = SqliteLedger::open(db, i64::MAX).map_err(|error| error.to_string())?;
    let policy = RetentionPolicy::DEFAULT;
    let mut candidates = Vec::new();
    let mut protected_derived_records = 0;
    for event in ledger.events().map_err(|error| error.to_string())? {
        let Some(recorded_at_us) = event_timestamp_us(&event) else {
            if matches!(event.kind.as_str(), "episode" | "pattern" | "agent_run") {
                protected_derived_records += 1;
            }
            continue;
        };
        let Some(record_kind) = (match event.kind.as_str() {
            "observation" => Some(RecordKind::Observation),
            "belief" => Some(RecordKind::BeliefFrame),
            "event" => Some(RecordKind::Event),
            "episode" | "pattern" | "agent_run" => {
                protected_derived_records += 1;
                None
            }
            _ => None,
        }) else {
            continue;
        };
        if retention_candidate(&policy, record_kind, recorded_at_us, now_us) {
            candidates.push(RetentionCandidate {
                provenance_sources: ledger
                    .provenance_sources(&event.id)
                    .map_err(|error| error.to_string())?,
                id: event.id,
                sequence: event.sequence,
                kind: event.kind,
                recorded_at_us,
            });
        }
    }
    Ok(RetentionPlan {
        schema: "liminal.retention-plan.v1",
        evaluated_at_us: now_us,
        candidates,
        protected_derived_records,
    })
}

fn run_retention_plan(db: &Path, now_us: Option<i64>, output: Option<&Path>) -> ExitCode {
    let now_us = match now_us {
        Some(now_us) => now_us,
        None => match std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_micros() as i64)
        {
            Ok(now_us) => now_us,
            Err(error) => {
                eprintln!("error: clock before Unix epoch: {error}");
                return ExitCode::FAILURE;
            }
        },
    };
    let plan = match retention_plan(db, now_us) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let rendered = match serde_json::to_string_pretty(&plan) {
        Ok(rendered) => rendered,
        Err(error) => {
            eprintln!("error: serialize retention plan: {error}");
            return ExitCode::FAILURE;
        }
    };
    match output {
        Some(output) => match fs::File::create(output)
            .and_then(|mut file| file.write_all(rendered.as_bytes()))
        {
            Ok(()) => {
                println!(
                    "retention plan: wrote {} candidate(s) to {}; no records deleted",
                    plan.candidates.len(),
                    output.display()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: write retention plan {}: {error}", output.display());
                ExitCode::FAILURE
            }
        },
        None => {
            println!("{rendered}");
            ExitCode::SUCCESS
        }
    }
}

fn run_retention_preview(db: &Path, now_us: Option<i64>) -> ExitCode {
    let now_us = match now_us {
        Some(now_us) => now_us,
        None => match std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_micros() as i64)
        {
            Ok(now_us) => now_us,
            Err(error) => {
                eprintln!("error: clock before Unix epoch: {error}");
                return ExitCode::FAILURE;
            }
        },
    };
    match retention_preview(db, now_us) {
        Ok(preview) => {
            println!(
                "retention preview: observations={}, belief_frames={}, events={}, protected_derived={}; no records deleted",
                preview.observations_eligible,
                preview.belief_frames_eligible,
                preview.events_eligible,
                preview.protected_derived_records
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn retention_apply(db: &Path, now_us: Option<i64>, confirm: bool) -> Result<usize, String> {
    if !confirm {
        return Err(
            "retention apply is irreversible; re-run with --confirm after reviewing the plan"
                .to_string(),
        );
    }
    let now_us = match now_us {
        Some(now_us) => now_us,
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_micros() as i64)
            .map_err(|error| format!("clock before Unix epoch: {error}"))?,
    };
    let plan = retention_plan(db, now_us)?;
    let ids = plan
        .candidates
        .into_iter()
        .map(|candidate| candidate.id)
        .collect::<Vec<_>>();
    let mut ledger = SqliteLedger::open(db, i64::MAX).map_err(|error| error.to_string())?;
    ledger
        .erase_event_ids(&ids)
        .map_err(|error| error.to_string())
}

fn run_retention_apply(db: &Path, now_us: Option<i64>, confirm: bool) -> ExitCode {
    match retention_apply(db, now_us, confirm) {
        Ok(count) => {
            println!("retention apply: removed {count} eligible record(s) and dependent records");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
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

    fn valid_hash(previous_hash: &str, payload: &serde_json::Value) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(previous_hash.as_bytes());
        hasher.update(payload.to_string().as_bytes());
        hasher.finalize().to_hex().to_string()
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
        let payload = serde_json::json!({ "stream_id": "wifi", "ssid": "HomeNetwork" });
        insert_raw_event(
            &path,
            "obs_1",
            0,
            "observation",
            &payload,
            "0",
            &valid_hash("0", &payload),
        );

        let hits = audit_privacy(&path).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].event_id, "obs_1");
        assert_eq!(hits[0].key_path, "$.ssid");

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn privacy_audit_detects_raw_media_in_the_canonical_debug_capture_directory() {
        let root =
            std::env::temp_dir().join(format!("liminal-cli-audit-captures-{}", std::process::id()));
        let captures = root.join("debug-captures").join("session");
        std::fs::create_dir_all(&captures).unwrap();
        let db = root.join("liminal.db");
        SqliteLedger::open(&db, i64::MAX).unwrap();
        std::fs::write(captures.join("raw.wav"), b"fixture").unwrap();
        std::fs::write(captures.join("derived.json"), b"fixture").unwrap();

        let hits = audit_privacy(&db).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].key_path, "filesystem raw capture");
        assert!(hits[0].event_id.ends_with("raw.wav"));

        std::fs::remove_dir_all(&root).unwrap();
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
        let payload = serde_json::json!({ "bssid": "AA:BB:CC:DD:EE:FF" });
        insert_raw_event(
            &path,
            "obs_1",
            0,
            "observation",
            &payload,
            "0",
            &valid_hash("0", &payload),
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
    fn history_walks_the_hash_chain_back_to_genesis() {
        let path = temp_db_path("history");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
            ledger
                .append_observation("obs_2", "wifi", 1_000_000)
                .unwrap();
            ledger.append_belief("belief_1", "wifi", 1_000_100).unwrap();
        }

        let chain = event_history(&path, "belief_1").unwrap().unwrap();
        let ids: Vec<&str> = chain.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["belief_1", "obs_2", "obs_1"]);
        assert_eq!(chain.last().unwrap().previous_hash, "0");

        std::fs::remove_file(&path).unwrap();
    }

    /// This is the failure mode the module doc comment warns about: an unrelated event from a
    /// different stream, appended between two related ones, appears in the append-order history
    /// exactly as if it were evidence for the later claim. `events history` cannot and does not
    /// claim otherwise -- this test documents that limitation rather than hiding it.
    #[test]
    fn history_includes_unrelated_interleaved_events_because_it_is_append_order_not_provenance() {
        let path = temp_db_path("history-interleaved");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "wifi", 0).unwrap();
            ledger
                .append_observation("cam_1", "camera", 500_000)
                .unwrap();
            ledger
                .append_observation("obs_2", "wifi", 1_000_000)
                .unwrap();
            ledger.append_belief("belief_1", "wifi", 1_000_100).unwrap();
        }

        let chain = event_history(&path, "belief_1").unwrap().unwrap();
        let ids: Vec<&str> = chain.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["belief_1", "obs_2", "cam_1", "obs_1"]);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn history_returns_none_for_unknown_id() {
        let path = temp_db_path("history-missing");
        {
            SqliteLedger::open(&path, i64::MAX).unwrap();
        }

        assert!(event_history(&path, "nope").unwrap().is_none());

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn provenance_reads_explicit_sources_without_append_order_noise() {
        let path = temp_db_path("provenance");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "camera", 0).unwrap();
            ledger.append_observation("noise", "wifi", 1).unwrap();
            ledger
                .append_belief_with_features_and_evidence(
                    "belief_1",
                    "fusion",
                    2,
                    serde_json::json!({"occupancy_probability": 0.8}),
                    &["obs_1".to_string()],
                )
                .unwrap();
        }

        assert_eq!(
            SqliteLedger::open(&path, i64::MAX)
                .unwrap()
                .provenance_sources("belief_1")
                .unwrap(),
            vec!["obs_1".to_string()]
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn provenance_tree_follows_explicit_edges_not_append_order() {
        let path = temp_db_path("provenance-tree");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "camera", 0).unwrap();
            ledger.append_observation("noise", "wifi", 1).unwrap();
            ledger
                .append_derived_record(
                    "episode_1",
                    "episode",
                    serde_json::json!({"start_ts_us": 0, "end_ts_us": 1}),
                    &["obs_1".to_string()],
                )
                .unwrap();
            ledger
                .append_derived_record(
                    "pattern_1",
                    "pattern",
                    serde_json::json!({"signature": "Occupied:0min"}),
                    &["episode_1".to_string()],
                )
                .unwrap();
        }

        let tree = provenance_tree(&path, "pattern_1").unwrap().unwrap();
        let labels: Vec<(usize, String)> = tree
            .into_iter()
            .map(|(depth, event)| (depth, event.id))
            .collect();
        assert_eq!(
            labels,
            vec![
                (0, "pattern_1".to_string()),
                (1, "episode_1".to_string()),
                (2, "obs_1".to_string())
            ]
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn export_bundle_is_range_filtered_and_carries_provenance() {
        let path = temp_db_path("export");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "camera", 0).unwrap();
            ledger.append_observation("obs_2", "camera", 10).unwrap();
            ledger
                .append_derived_record(
                    "episode_1",
                    "episode",
                    serde_json::json!({"timestamp_us": 10}),
                    &["obs_2".to_string()],
                )
                .unwrap();
        }

        let bundle = export_bundle(&path, Some(10), Some(10)).unwrap();
        assert_eq!(bundle.schema, "liminal.export.v1");
        assert_eq!(bundle.records.len(), 2);
        assert_eq!(bundle.records[1].provenance_sources, ["obs_2"]);
        assert_eq!(
            export_bundle(&path, Some(11), None).unwrap().records.len(),
            0
        );

        std::fs::remove_file(&path).unwrap();

        let leaky_path = temp_db_path("export-leaky");
        SqliteLedger::open(&leaky_path, i64::MAX).unwrap();
        let payload = serde_json::json!({"stream_id": "wifi", "ssid": "private"});
        insert_raw_event(
            &leaky_path,
            "leaky",
            0,
            "observation",
            &payload,
            "0",
            &valid_hash("0", &payload),
        );
        assert!(export_bundle(&leaky_path, None, None)
            .unwrap_err()
            .contains("forbidden key"));
        std::fs::remove_file(&leaky_path).unwrap();
    }

    #[test]
    fn privacy_erase_requires_confirmation_and_cascades_the_selected_range() {
        let path = temp_db_path("privacy-erase");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "camera", 10).unwrap();
            ledger.append_observation("obs_2", "camera", 20).unwrap();
            ledger
                .append_derived_record(
                    "belief_1",
                    "belief",
                    serde_json::json!({"ts_us": 10}),
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
        }

        assert!(privacy_erase(&path, Some(10), Some(10), false)
            .unwrap_err()
            .contains("--confirm"));
        assert_eq!(
            SqliteLedger::open(&path, i64::MAX)
                .unwrap()
                .events()
                .unwrap()
                .len(),
            4
        );

        assert_eq!(privacy_erase(&path, Some(10), Some(10), true).unwrap(), 3);
        let ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
        assert!(ledger.verify_chain().is_ok());
        assert_eq!(ledger.events().unwrap().len(), 1);
        assert_eq!(ledger.events().unwrap()[0].id, "obs_2");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn retention_apply_reuses_policy_and_requires_confirmation() {
        let path = temp_db_path("retention-apply");
        let policy = RetentionPolicy::DEFAULT;
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("old", "camera", 0).unwrap();
            ledger
                .append_observation("recent", "camera", policy.observations_us - 1)
                .unwrap();
        }

        assert!(retention_apply(&path, Some(policy.observations_us), false)
            .unwrap_err()
            .contains("--confirm"));
        assert_eq!(
            SqliteLedger::open(&path, i64::MAX)
                .unwrap()
                .events()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            retention_apply(&path, Some(policy.observations_us), true).unwrap(),
            1
        );
        let ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
        assert!(ledger.verify_chain().is_ok());
        assert_eq!(ledger.events().unwrap()[0].id, "recent");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn retention_preview_reports_candidates_without_touching_derived_records() {
        let path = temp_db_path("retention-preview");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "camera", 0).unwrap();
            ledger.append_belief("belief_1", "fusion", 0).unwrap();
            ledger
                .append_derived_record(
                    "event_1",
                    "event",
                    serde_json::json!({"ts_us": 0}),
                    &["belief_1".to_string()],
                )
                .unwrap();
            ledger
                .append_derived_record(
                    "episode_1",
                    "episode",
                    serde_json::json!({"start_ts_us": 0, "end_ts_us": 1}),
                    &["belief_1".to_string()],
                )
                .unwrap();
        }

        let preview = retention_preview(&path, RetentionPolicy::DEFAULT.events_us).unwrap();
        assert_eq!(
            preview,
            RetentionPreview {
                observations_eligible: 1,
                belief_frames_eligible: 1,
                events_eligible: 1,
                protected_derived_records: 1,
            }
        );
        assert_eq!(
            SqliteLedger::open(&path, i64::MAX)
                .unwrap()
                .events()
                .unwrap()
                .len(),
            4
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn retention_plan_lists_exact_candidates_and_provenance_without_deleting() {
        let path = temp_db_path("retention-plan");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger.append_observation("obs_1", "camera", 0).unwrap();
            ledger
                .append_belief_with_features_and_evidence(
                    "belief_1",
                    "fusion",
                    0,
                    serde_json::json!({"occupancy_probability": 0.5}),
                    &["obs_1".to_string()],
                )
                .unwrap();
            ledger
                .append_derived_record(
                    "episode_1",
                    "episode",
                    serde_json::json!({"start_ts_us": 0, "end_ts_us": 1}),
                    &["belief_1".to_string()],
                )
                .unwrap();
        }

        let plan = retention_plan(&path, RetentionPolicy::DEFAULT.events_us).unwrap();
        assert_eq!(plan.schema, "liminal.retention-plan.v1");
        assert_eq!(
            plan.candidates
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            vec!["obs_1", "belief_1"]
        );
        assert_eq!(plan.candidates[1].provenance_sources, vec!["obs_1"]);
        assert_eq!(plan.protected_derived_records, 1);
        assert_eq!(
            SqliteLedger::open(&path, i64::MAX)
                .unwrap()
                .events()
                .unwrap()
                .len(),
            3
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn memory_replay_materializes_idempotent_records_with_sources() {
        let path = temp_db_path("memory-replay");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            for (index, timestamp_us) in [0, 3_000_000, 6_000_000].into_iter().enumerate() {
                ledger
                    .append_belief_with_features_and_evidence(
                        format!("belief-{index}"),
                        "fusion",
                        timestamp_us,
                        serde_json::json!({"occupancy_probability": 0.9}),
                        &[format!("observation-{index}")],
                    )
                    .unwrap();
            }
        }

        assert_eq!(materialize_memory(&path).unwrap(), (1, 1, 1, 1));
        assert_eq!(materialize_memory(&path).unwrap(), (1, 0, 1, 0));
        let ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
        let episode = ledger
            .events()
            .unwrap()
            .into_iter()
            .find(|event| event.kind == "episode")
            .unwrap();
        assert_eq!(
            ledger.provenance_sources(&episode.id).unwrap(),
            vec![
                "belief-0".to_string(),
                "belief-1".to_string(),
                "belief-2".to_string()
            ]
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn agent_run_requires_structured_evidence_and_persists_auditable_output() {
        let empty_path = temp_db_path("agent-empty");
        assert!(run_agent(&empty_path, AgentName::Archivist, 1).is_err());
        std::fs::remove_file(&empty_path).unwrap();

        let path = temp_db_path("agent-run");
        {
            let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
            ledger
                .append_belief_with_features_and_evidence(
                    "belief-1",
                    "fusion",
                    0,
                    serde_json::json!({"occupancy_probability": 0.9}),
                    &["observation-1".to_string()],
                )
                .unwrap();
        }
        let (id, evidence_count) = run_agent(&path, AgentName::Skeptic, 2).unwrap();
        let ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
        let run = ledger
            .events()
            .unwrap()
            .into_iter()
            .find(|event| event.id == id)
            .unwrap();
        assert_eq!(evidence_count, 1);
        assert_eq!(run.kind, "agent_run");
        assert_eq!(
            ledger.provenance_sources(&id).unwrap(),
            vec!["belief-1".to_string()]
        );
        assert_eq!(run.payload["output"]["status"], "INSUFFICIENT_EVIDENCE");
        assert_eq!(run.payload["output"]["layer"], "INTERPRETED");

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn gap_recovery_acknowledges_only_pending_streams() {
        let path = temp_db_path("gap-recovery");
        {
            let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
            ledger.append_observation("camera-1", "camera", 0).unwrap();
            ledger
                .append_observation("camera-2", "camera", 40_000_000)
                .unwrap();
            ledger.append_observation("wifi-1", "wifi", 0).unwrap();
            ledger
                .append_observation("wifi-2", "wifi", 40_000_000)
                .unwrap();
        }

        let acknowledged = acknowledge_gaps(&path, &[String::from("camera")]).unwrap();
        assert_eq!(acknowledged, vec![String::from("camera")]);
        let ledger = SqliteLedger::open(&path, DEFAULT_MAX_SILENT_GAP_US).unwrap();
        assert!(!ledger.has_unacknowledged_sensor_gap("camera"));
        assert!(ledger.has_unacknowledged_sensor_gap("wifi"));
        assert!(ledger
            .events()
            .unwrap()
            .iter()
            .any(|event| event.id.starts_with("operator-gap-ack-camera-")));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn calibration_score_matches_explicit_labels_to_daemon_beliefs() {
        let db = temp_db_path("calibration-db");
        let labels = temp_db_path("calibration-labels");
        {
            let mut ledger = SqliteLedger::open(&db, i64::MAX).unwrap();
            ledger
                .append_belief_with_features(
                    "belief-1",
                    "fusion",
                    1_000_000,
                    serde_json::json!({ "occupancy_probability": 0.9 }),
                )
                .unwrap();
            ledger
                .append_belief_with_features(
                    "belief-2",
                    "fusion",
                    2_000_000,
                    serde_json::json!({ "occupancy_probability": 0.1 }),
                )
                .unwrap();
        }
        fs::write(
            &labels,
            "{\"ts_us\":1000100,\"occupied\":true}\n{\"ts_us\":2000500,\"occupied\":false}\n",
        )
        .unwrap();

        let report = score_calibration(&db, &labels, 500).unwrap();
        assert_eq!(report.labels_total, 2);
        assert_eq!(report.matched_labels, 2);
        assert_eq!(report.unmatched_labels, 0);
        assert_eq!(report.accuracy, 1.0);

        fs::remove_file(db).unwrap();
        fs::remove_file(labels).unwrap();
    }

    #[test]
    fn calibration_score_rejects_negative_matching_window() {
        let db = temp_db_path("calibration-negative");
        let labels = temp_db_path("calibration-negative-labels");
        fs::write(&labels, "").unwrap();
        let error = score_calibration(&db, &labels, -1).unwrap_err();
        assert!(error.contains("non-negative"));
        fs::remove_file(labels).unwrap();
    }
}
