use liminal_ipc::Envelope;
use liminal_ledger::SqliteLedger;
use liminald::ingest_envelope;

fn envelope(
    message_id: &str,
    stream_id: &str,
    monotonic_sequence: u64,
    captured_at_utc_us: i64,
    payload: &[u8],
) -> Envelope {
    Envelope {
        schema_version: 1,
        message_id: message_id.to_string(),
        sensor_stream_id: stream_id.to_string(),
        monotonic_sequence,
        captured_at_utc_us,
        captured_at_mono_ns: captured_at_utc_us,
        payload: payload.to_vec(),
    }
}

#[test]
fn persisted_fusion_marks_contradictory_live_modalities_contested() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "liminald-mutation-invariant-{}-{unique}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();

    let camera = envelope(
        "camera-1",
        "camera",
        1,
        1_000,
        br#"{"body_count":"one","joints":[]}"#,
    );
    ingest_envelope(&mut ledger, &camera).unwrap();

    let microphone = envelope(
        "microphone-1",
        "microphone",
        1,
        1_000,
        br#"{"voice_activity_probability":0.0}"#,
    );
    ingest_envelope(&mut ledger, &microphone).unwrap();

    let events = ledger.events().unwrap();
    let belief = events
        .iter()
        .rev()
        .find(|event| event.kind == "belief" && event.payload["stream_id"] == "fusion")
        .expect("fusion belief should be persisted");

    assert_eq!(belief.payload["features"]["state"], "contested");
    assert!(belief.payload["features"]["disagreement"]
        .as_f64()
        .is_some_and(|value| value > 0.9));

    drop(ledger);
    std::fs::remove_file(path).unwrap();
}
