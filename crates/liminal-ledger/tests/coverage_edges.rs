use liminal_ledger::{Ledger, LedgerError, SqliteLedger};

fn temp_db_path(name: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "liminal-ledger-coverage-{name}-{}-{unique}.db",
        std::process::id()
    ))
}

#[test]
fn in_memory_feature_beliefs_cover_gap_and_no_gap_paths() {
    let mut ledger = Ledger::new(5_000_000);
    assert!(!ledger.has_unacknowledged_sensor_gap("camera"));

    ledger.append_observation("obs-1", "camera", 0);
    ledger
        .append_belief_with_features(
            "belief-ok",
            "camera",
            1,
            serde_json::json!({"occupancy_probability": 0.9}),
        )
        .unwrap();

    ledger.append_observation("obs-2", "camera", 20_000_000);
    assert!(ledger.has_unacknowledged_sensor_gap("camera"));
    assert_eq!(
        ledger
            .append_belief_with_features(
                "belief-blocked",
                "camera",
                20_000_001,
                serde_json::json!({"occupancy_probability": 0.9}),
            )
            .unwrap_err(),
        LedgerError::SensorGapNotAcknowledged("camera".to_string())
    );
}

#[test]
fn sqlite_feature_beliefs_cover_gap_and_no_gap_paths() {
    let path = temp_db_path("sqlite-belief");
    let mut ledger = SqliteLedger::open(&path, 5_000_000).unwrap();
    assert!(!ledger.has_unacknowledged_sensor_gap("camera"));

    ledger.append_observation("obs-1", "camera", 0).unwrap();
    ledger
        .append_belief_with_features(
            "belief-ok",
            "camera",
            1,
            serde_json::json!({"occupancy_probability": 0.7}),
        )
        .unwrap();

    ledger
        .append_observation("obs-2", "camera", 20_000_000)
        .unwrap();
    assert!(ledger.has_unacknowledged_sensor_gap("camera"));
    assert_eq!(
        ledger
            .append_belief_with_features(
                "belief-blocked",
                "camera",
                20_000_001,
                serde_json::json!({"occupancy_probability": 0.7}),
            )
            .unwrap_err(),
        LedgerError::SensorGapNotAcknowledged("camera".to_string())
    );

    drop(ledger);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sqlite_lookup_helpers_cover_empty_present_and_sequence_update_paths() {
    let path = temp_db_path("lookups");
    let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();

    assert!(!ledger.contains_event_id("missing").unwrap());
    assert!(ledger.latest_observation("camera").unwrap().is_none());
    assert_eq!(ledger.last_monotonic_sequence("camera"), None);

    ledger
        .append_observation_with_features(
            "camera-1",
            "camera",
            1,
            serde_json::json!({"_monotonic_sequence": 5, "body_count": "one"}),
        )
        .unwrap();
    ledger
        .append_observation_with_features(
            "camera-2",
            "camera",
            2,
            serde_json::json!({"_monotonic_sequence": 3, "body_count": "one"}),
        )
        .unwrap();
    ledger
        .append_observation_with_features(
            "camera-3",
            "camera",
            3,
            serde_json::json!({"_monotonic_sequence": 8, "body_count": "one"}),
        )
        .unwrap();

    assert!(ledger.contains_event_id("camera-2").unwrap());
    assert_eq!(ledger.last_monotonic_sequence("camera"), Some(8));
    assert_eq!(
        ledger.latest_observation("camera").unwrap().unwrap().id,
        "camera-3"
    );

    drop(ledger);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn latest_observation_handles_missing_features_and_missing_timestamp() {
    let path = temp_db_path("latest-shape");
    let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();

    ledger
        .append_derived_record(
            "observation-no-features",
            "observation",
            serde_json::json!({"stream_id": "camera", "ts_us": 10}),
            &[],
        )
        .unwrap();
    let latest = ledger.latest_observation("camera").unwrap().unwrap();
    assert_eq!(latest.features, serde_json::Value::Null);

    ledger
        .append_derived_record(
            "observation-no-ts",
            "observation",
            serde_json::json!({"stream_id": "camera", "features": {"body_count": "one"}}),
            &[],
        )
        .unwrap();
    assert!(matches!(
        ledger.latest_observation("camera"),
        Err(LedgerError::Database(_))
    ));

    drop(ledger);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn erasing_an_empty_selection_is_a_noop() {
    let path = temp_db_path("empty-erase");
    let mut ledger = SqliteLedger::open(&path, i64::MAX).unwrap();
    ledger.append_observation("obs-1", "camera", 1).unwrap();

    assert_eq!(ledger.erase_event_ids(&[]).unwrap(), 0);
    assert_eq!(ledger.events().unwrap().len(), 1);
    assert!(ledger.verify_chain().is_ok());

    drop(ledger);
    std::fs::remove_file(path).unwrap();
}
