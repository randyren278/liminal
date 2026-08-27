use crate::ledger_view::LedgerSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteLayer {
    Fact,
    Limitation,
    Imagined,
}

impl NoteLayer {
    fn label(self) -> &'static str {
        match self {
            Self::Fact => "FACT",
            Self::Limitation => "LIMITATION",
            Self::Imagined => "IMAGINED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldNote {
    pub role: &'static str,
    pub layer: NoteLayer,
    pub text: String,
}

/// Build the read-only notes shown by FIELD NOTES. These are agent-shaped drafts, not persisted
/// claims: the builder has no write path and never turns a sensor observation into an identity,
/// location, or calibrated occupancy assertion.
pub fn build_field_notes(snapshot: &LedgerSnapshot) -> Vec<FieldNote> {
    let streams = snapshot
        .stream_event_counts
        .iter()
        .map(|(stream, count)| format!("{stream}: {count}"))
        .collect::<Vec<_>>()
        .join("  ");
    let belief_evidence = snapshot
        .persisted_belief
        .as_ref()
        .map(|belief| {
            if belief.evidence_ids.is_empty() {
                "none recorded".to_string()
            } else {
                belief.evidence_ids.join(", ")
            }
        })
        .unwrap_or_else(|| "no persisted daemon belief".to_string());
    let agent_outputs = if snapshot.agent_runs.is_empty() {
        "none persisted".to_string()
    } else {
        snapshot
            .agent_runs
            .iter()
            .map(|run| {
                format!(
                    "{} / {} / {} / {} evidence: {}",
                    run.role, run.layer, run.status, run.evidence_count, run.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    vec![
        FieldNote {
            role: "ARCHIVIST",
            layer: NoteLayer::Fact,
            text: format!(
                "Observed {} ledger events across {}. Latest daemon-belief evidence: {}. {} persisted Tier-0 agent run(s) are available as read-only drafts:\n{}",
                snapshot.total_event_count, streams, belief_evidence, snapshot.agent_run_count, agent_outputs
            ),
        },
        FieldNote {
            role: "SKEPTIC",
            layer: NoteLayer::Limitation,
            text: "Occupancy, identity, and location are not calibrated. No factual claim is promoted from these observations.".to_string(),
        },
        FieldNote {
            role: "CARTOGRAPHER",
            layer: NoteLayer::Limitation,
            text: "Position is unavailable. Wi-Fi and Bluetooth are environmental signals here, not a map.".to_string(),
        },
        FieldNote {
            role: "ETHNOGRAPHER",
            layer: NoteLayer::Limitation,
            text: format!(
                "Structural replay currently contains {} Episode(s) and {} Pattern bucket(s); long-range recurrence interpretation is waiting for a calibrated history window ({} populated day bucket(s) currently visible). Pending fusion gaps: {}.",
                snapshot.episode_count,
                snapshot.pattern_count,
                snapshot.historical_buckets.len(),
                if snapshot.pending_gap_streams.is_empty() {
                    "none".to_string()
                } else {
                    snapshot.pending_gap_streams.join(", ")
                }
            ),
        },
        FieldNote {
            role: "POET",
            layer: NoteLayer::Imagined,
            text: "The room leaves marks before it tells a story.".to_string(),
        },
    ]
}

pub fn format_field_notes(notes: &[FieldNote]) -> String {
    let mut output = String::from("FIELD NOTES / READ-ONLY\n\n");
    for (index, note) in notes.iter().enumerate() {
        if index > 0 {
            output.push_str("\n\n");
        }
        output.push_str(note.role);
        output.push_str(" / ");
        output.push_str(note.layer.label());
        output.push('\n');
        output.push_str(&note.text);
    }
    output.push_str("\n\nEvery card is a ledger fact, a limitation, or explicitly imagined.");
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger_view::{
        AgentRunSummary, HistoricalBucket, RecentObservation, TelemetrySnapshot,
    };
    use std::collections::BTreeMap;

    fn snapshot() -> LedgerSnapshot {
        LedgerSnapshot {
            total_event_count: 4,
            sensor_gap_count: 0,
            pending_gap_streams: vec![],
            belief_count: 0,
            agent_run_count: 0,
            agent_runs: vec![],
            stream_event_counts: BTreeMap::from([(String::from("camera"), 4)]),
            latest_observation_timestamps: BTreeMap::from([(String::from("camera"), 1)]),
            latest_event: None,
            latest_record: None,
            recent_records: Vec::new(),
            latest_camera_joints: vec![],
            telemetry: TelemetrySnapshot::default(),
            persisted_belief: None,
            recent_observations: vec![RecentObservation {
                stream: "camera".to_string(),
                timestamp_us: 1,
                kind: "observation".to_string(),
            }],
            historical_buckets: vec![HistoricalBucket {
                day_index: 1,
                observation_count: 4,
            }],
            occupancy_events: vec![],
            episode_count: 0,
            pattern_count: 0,
        }
    }

    #[test]
    fn notes_are_role_labeled_and_keep_imagined_text_separate() {
        let mut snapshot = snapshot();
        snapshot.persisted_belief = Some(crate::ledger_view::PersistedBelief {
            occupancy_probability: 0.5,
            confidence: 0.5,
            disagreement: 0.0,
            observed_modalities: 1,
            sensor_health: 1.0,
            state: crate::belief::BeliefState::Stable,
            evidence_ids: vec!["observation-1".to_string()],
        });
        let notes = build_field_notes(&snapshot);
        assert_eq!(notes.len(), 5);
        assert_eq!(notes[0].layer, NoteLayer::Fact);
        assert_eq!(notes[4].layer, NoteLayer::Imagined);
        let rendered = format_field_notes(&notes);
        assert!(rendered.contains("ARCHIVIST / FACT"));
        assert!(rendered.contains("POET / IMAGINED"));
        assert!(rendered.contains("4 ledger events"));
        assert!(rendered.contains("observation-1"));
    }

    #[test]
    fn notes_render_persisted_agent_output_with_its_layer_and_status() {
        let mut snapshot = snapshot();
        snapshot.agent_run_count = 1;
        snapshot.agent_runs = vec![AgentRunSummary {
            role: "archivist".to_string(),
            layer: "INFERRED".to_string(),
            status: "DRAFT".to_string(),
            evidence_count: 3,
            text: "No identity claim.".to_string(),
        }];

        let rendered = format_field_notes(&build_field_notes(&snapshot));
        assert!(rendered.contains("archivist / INFERRED / DRAFT / 3 evidence"));
        assert!(rendered.contains("No identity claim."));
    }
}
