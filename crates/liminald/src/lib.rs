//! `liminald` -- the Rust daemon that owns canonical state (ROADMAP item 3).
//!
//! Master plan reference: §14 (`liminald` owns canonical state, IPC, DB, fusion; must not
//! directly access camera/microphone), §15 (Unix domain socket under `/tmp/liminal-$UID/`,
//! 0700 dir / 0600 socket, length-delimited protobuf frames).
//!
//! Accept envelopes from sensor organs, persist observations, and append a transparent derived
//! belief record when the source streams have no unacknowledged gap. The daemon never sees raw
//! media; fusion consumes only the sanitized feature JSON already in the ledger.

use std::io::{self, ErrorKind, Read};
use std::path::PathBuf;

use liminal_ipc::{validate_schema_version, Envelope};
use liminal_ledger::{LatestObservation, SqliteLedger};

/// §15: the canonical socket directory for a given uid, e.g. `/tmp/liminal-501`.
pub fn socket_dir_for_uid(uid: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/liminal-{uid}"))
}

/// §15: the canonical socket path within that directory.
pub fn socket_path_for_uid(uid: u32) -> PathBuf {
    socket_dir_for_uid(uid).join("core.sock")
}

#[derive(Debug, thiserror::Error)]
pub enum PrepareSocketError {
    #[error("failed to create socket directory: {0}")]
    CreateDir(#[source] io::Error),
    #[error("failed to set socket directory permissions: {0}")]
    SetDirPermissions(#[source] io::Error),
    #[error("failed to remove stale socket file: {0}")]
    RemoveStaleSocket(#[source] io::Error),
    #[error("an active liminald instance already owns the socket")]
    ActiveSocket,
}

/// §15: "0700 directory". Creates the socket directory if missing and ensures its permissions,
/// and removes a stale socket file left over from a previous run (binding to an existing path
/// fails with `AddrInUse` otherwise). Does not create the socket itself -- `UnixListener::bind`
/// does that, and its own file gets 0600 applied by the caller after binding (socket-file modes
/// can't be set before the file exists).
pub fn prepare_socket_path(path: &std::path::Path) -> Result<(), PrepareSocketError> {
    use std::os::unix::fs::PermissionsExt;

    let dir = path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/tmp"));
    std::fs::create_dir_all(dir).map_err(PrepareSocketError::CreateDir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(PrepareSocketError::SetDirPermissions)?;

    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(PrepareSocketError::ActiveSocket);
        }
        std::fs::remove_file(path).map_err(PrepareSocketError::RemoveStaleSocket)?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum FrameReadError {
    #[error("io error reading frame: {0}")]
    Io(#[from] io::Error),
    #[error("failed to decode envelope: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("frame is too large: {length} bytes (maximum {maximum})")]
    FrameTooLarge { length: usize, maximum: usize },
}

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_DERIVED_COLLECTION_ITEMS: usize = 128;
const MAX_DERIVED_STRING_BYTES: usize = 128;
const MAX_MESSAGE_ID_BYTES: usize = 128;

/// Reads one §15 length-delimited frame (4-byte big-endian length + serialized `Envelope`) from
/// `reader`. Returns `Ok(None)` on a clean EOF at a frame boundary (the client disconnected
/// normally); any other error is a real problem.
pub fn read_length_delimited_envelope<R: Read>(
    reader: &mut R,
) -> Result<Option<Envelope>, FrameReadError> {
    use prost::Message;

    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(FrameReadError::FrameTooLarge {
            length: len,
            maximum: MAX_FRAME_BYTES,
        });
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    let envelope = Envelope::decode(buf.as_slice())?;
    Ok(Some(envelope))
}

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("schema mismatch: {0}")]
    SchemaMismatch(#[from] liminal_ipc::SchemaMismatch),
    #[error("payload is not valid JSON: {0}")]
    InvalidPayload(#[source] serde_json::Error),
    #[error("unsupported sensor stream: {0}")]
    UnsupportedSensorStream(String),
    #[error("sensor payload for '{stream}' must be a JSON object")]
    InvalidFeatureShape { stream: String },
    #[error("sensor payload field '{field}' is not allowed for stream '{stream}'")]
    ForbiddenFeatureField { stream: String, field: String },
    #[error("message ID must be 1..={MAX_MESSAGE_ID_BYTES} bytes")]
    InvalidMessageId,
    #[error(transparent)]
    Ledger(#[from] liminal_ledger::LedgerError),
}

fn validate_feature_payload(stream: &str, features: &serde_json::Value) -> Result<(), IngestError> {
    let allowed: &[&str] = match stream {
        "camera" => &["body_count", "joints"],
        "microphone" => &[
            "rms",
            "peak",
            "zero_crossing_rate",
            "spectral_centroid_hz",
            "spectral_rolloff_hz",
            "spectral_flatness",
            "voice_activity_probability",
        ],
        "wifi" => &[
            "rssi_mean",
            "noise_mean",
            "visible_network_count",
            "strongest_1",
            "strongest_2",
            "strongest_3",
        ],
        "bluetooth" => &["clusters", "cluster_count"],
        other => return Err(IngestError::UnsupportedSensorStream(other.to_string())),
    };
    let object = features
        .as_object()
        .ok_or_else(|| IngestError::InvalidFeatureShape {
            stream: stream.to_string(),
        })?;
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(IngestError::ForbiddenFeatureField {
                stream: stream.to_string(),
                field: key.clone(),
            });
        }
    }
    let invalid_shape = || IngestError::InvalidFeatureShape {
        stream: stream.to_string(),
    };
    match stream {
        "camera" => {
            if !matches!(
                object.get("body_count").and_then(serde_json::Value::as_str),
                Some("zero" | "one" | "two_or_more" | "unknown")
            ) {
                return Err(invalid_shape());
            }
            if let Some(joints_value) = object.get("joints") {
                let joints = joints_value.as_array().ok_or_else(&invalid_shape)?;
                if joints.len() > MAX_DERIVED_COLLECTION_ITEMS {
                    return Err(invalid_shape());
                }
                for joint in joints {
                    let joint_object = joint.as_object().ok_or_else(&invalid_shape)?;
                    for key in joint_object.keys() {
                        if !matches!(key.as_str(), "name" | "x" | "y" | "confidence") {
                            return Err(IngestError::ForbiddenFeatureField {
                                stream: stream.to_string(),
                                field: format!("joints[].{key}"),
                            });
                        }
                    }
                    if joint_object
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .is_none_or(|name| name.len() > MAX_DERIVED_STRING_BYTES)
                    {
                        return Err(invalid_shape());
                    }
                    for key in ["x", "y", "confidence"] {
                        if !joint_object
                            .get(key)
                            .is_some_and(serde_json::Value::is_number)
                        {
                            return Err(invalid_shape());
                        }
                    }
                }
            }
        }
        "microphone" | "wifi" => {
            for &key in allowed {
                if object.get(key).is_some_and(|value| !value.is_number()) {
                    return Err(invalid_shape());
                }
            }
        }
        "bluetooth" => {
            if object
                .get("cluster_count")
                .is_some_and(|value| !value.is_number())
            {
                return Err(invalid_shape());
            }
            if let Some(clusters_value) = object.get("clusters") {
                let clusters = clusters_value.as_array().ok_or_else(&invalid_shape)?;
                if clusters.len() > MAX_DERIVED_COLLECTION_ITEMS {
                    return Err(invalid_shape());
                }
                for cluster in clusters {
                    let cluster_object = cluster.as_object().ok_or_else(&invalid_shape)?;
                    for key in cluster_object.keys() {
                        if !matches!(key.as_str(), "pseudonym" | "rssi") {
                            return Err(IngestError::ForbiddenFeatureField {
                                stream: stream.to_string(),
                                field: format!("clusters[].{key}"),
                            });
                        }
                    }
                    if cluster_object
                        .get("pseudonym")
                        .and_then(serde_json::Value::as_str)
                        .is_none_or(|pseudonym| {
                            let digest = pseudonym.strip_prefix("ble:");
                            digest.is_none_or(|digest| {
                                digest.len() != 64
                                    || !digest.bytes().all(|byte| {
                                        byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                                    })
                            }) || pseudonym.len() > MAX_DERIVED_STRING_BYTES
                        })
                        || !cluster_object
                            .get("rssi")
                            .is_some_and(serde_json::Value::is_number)
                    {
                        return Err(invalid_shape());
                    }
                }
            }
        }
        _ => unreachable!("unsupported streams returned above"),
    }
    Ok(())
}

#[cfg(test)]
fn latest_features<'a>(
    events: &'a [liminal_ledger::Event],
    stream: &str,
) -> Option<&'a serde_json::Value> {
    events
        .iter()
        .rev()
        .find(|event| {
            event.kind == "observation"
                && event.payload.get("stream_id").and_then(|v| v.as_str()) == Some(stream)
        })
        .and_then(|event| event.payload.get("features"))
}

#[cfg(test)]
fn latest_timestamp(events: &[liminal_ledger::Event], stream: &str) -> Option<i64> {
    events
        .iter()
        .rev()
        .find(|event| {
            event.kind == "observation"
                && event.payload.get("stream_id").and_then(|v| v.as_str()) == Some(stream)
        })
        .and_then(|event| event.payload.get("ts_us").and_then(|value| value.as_i64()))
}

const SENSOR_HEALTH_HORIZON_US: i64 = 10_000_000;

#[cfg(test)]
fn sensor_health(events: &[liminal_ledger::Event], stream: &str, now_us: i64) -> Option<f64> {
    let timestamp = latest_timestamp(events, stream)?;
    let age_us = now_us.saturating_sub(timestamp).max(0);
    Some((1.0 - age_us as f64 / SENSOR_HEALTH_HORIZON_US as f64).clamp(0.0, 1.0))
}

#[cfg(test)]
fn derive_belief_features(
    events: &[liminal_ledger::Event],
    source_observation_id: &str,
    now_us: i64,
) -> Option<serde_json::Value> {
    let camera = latest_features(events, "camera").and_then(|features| {
        match features.get("body_count")?.as_str()? {
            "zero" => Some(0.0),
            "one" | "two_or_more" => Some(1.0),
            _ => None,
        }
    });
    let audio = latest_features(events, "microphone")
        .and_then(|features| features.get("voice_activity_probability")?.as_f64())
        .map(|value| value.clamp(0.0, 1.0));
    let bluetooth = latest_features(events, "bluetooth")
        .and_then(|features| features.get("cluster_count")?.as_f64())
        .map(|value| (value / 4.0).clamp(0.0, 1.0));

    let health = [
        sensor_health(events, "camera", now_us),
        sensor_health(events, "microphone", now_us),
        sensor_health(events, "bluetooth", now_us),
    ];
    let signals = [camera, audio, bluetooth];
    let observed: Vec<f64> = signals.into_iter().flatten().collect();
    if observed.is_empty() {
        return None;
    }
    let mut weighted_sum = 0.0;
    let mut total_weight = 0.0;
    if let Some(value) = camera {
        weighted_sum += value * 0.65 * health[0].unwrap_or(0.0);
        total_weight += 0.65 * health[0].unwrap_or(0.0);
    }
    if let Some(value) = audio {
        weighted_sum += value * 0.20 * health[1].unwrap_or(0.0);
        total_weight += 0.20 * health[1].unwrap_or(0.0);
    }
    if let Some(value) = bluetooth {
        weighted_sum += value * 0.15 * health[2].unwrap_or(0.0);
        total_weight += 0.15 * health[2].unwrap_or(0.0);
    }
    if total_weight == 0.0 {
        return None;
    }
    let min = observed.iter().copied().fold(1.0, f64::min);
    let max = observed.iter().copied().fold(0.0, f64::max);
    let disagreement = max - min;
    let observed_health: Vec<f64> = health.into_iter().flatten().collect();
    let average_health = observed_health.iter().sum::<f64>() / observed_health.len() as f64;
    let state = if disagreement > 0.45 || average_health < 0.25 {
        "contested"
    } else {
        "stable"
    };
    Some(serde_json::json!({
        "occupancy_probability": weighted_sum / total_weight,
        "confidence": (total_weight * average_health * (1.0 - disagreement * 0.5)).clamp(0.0, 1.0),
        "disagreement": disagreement,
        "observed_modalities": observed.len(),
        "sensor_health": average_health,
        "state": state,
        "source_observation_id": source_observation_id,
        "model": "transparent-v1"
    }))
}

fn derive_belief_features_from_latest(
    camera: Option<&LatestObservation>,
    microphone: Option<&LatestObservation>,
    bluetooth: Option<&LatestObservation>,
    source_observation_id: &str,
    now_us: i64,
) -> Option<(serde_json::Value, Vec<String>)> {
    let camera_value =
        camera.and_then(
            |observation| match observation.features.get("body_count")?.as_str()? {
                "zero" => Some(0.0),
                "one" | "two_or_more" => Some(1.0),
                _ => None,
            },
        );
    let audio_value = microphone
        .and_then(|observation| {
            observation
                .features
                .get("voice_activity_probability")?
                .as_f64()
        })
        .map(|value| value.clamp(0.0, 1.0));
    let bluetooth_value = bluetooth
        .and_then(|observation| observation.features.get("cluster_count")?.as_f64())
        .map(|value| (value / 4.0).clamp(0.0, 1.0));

    let observations = [camera, microphone, bluetooth];
    let signals = [camera_value, audio_value, bluetooth_value];
    let observed: Vec<f64> = signals.into_iter().flatten().collect();
    if observed.is_empty() {
        return None;
    }
    let health: Vec<f64> = observations
        .iter()
        .filter_map(|observation| {
            observation.map(|value| sensor_health_timestamp(value.timestamp_us, now_us))
        })
        .collect();
    let average_health = health.iter().sum::<f64>() / health.len() as f64;
    let mut weighted_sum = 0.0;
    let mut total_weight = 0.0;
    for (signal, observation, base_weight) in [
        (camera_value, camera, 0.65),
        (audio_value, microphone, 0.20),
        (bluetooth_value, bluetooth, 0.15),
    ] {
        if let (Some(value), Some(observation)) = (signal, observation) {
            let weight = base_weight * sensor_health_timestamp(observation.timestamp_us, now_us);
            weighted_sum += value * weight;
            total_weight += weight;
        }
    }
    if total_weight == 0.0 {
        return None;
    }
    let min = observed.iter().copied().fold(1.0, f64::min);
    let max = observed.iter().copied().fold(0.0, f64::max);
    let disagreement = max - min;
    let state = if disagreement > 0.45 || average_health < 0.25 {
        "contested"
    } else {
        "stable"
    };
    let evidence_ids = observations
        .into_iter()
        .filter_map(|observation| observation.map(|value| value.id.clone()))
        .collect();
    Some((
        serde_json::json!({
            "occupancy_probability": weighted_sum / total_weight,
            "confidence": (total_weight * average_health * (1.0 - disagreement * 0.5)).clamp(0.0, 1.0),
            "disagreement": disagreement,
            "observed_modalities": observed.len(),
            "sensor_health": average_health,
            "state": state,
            "source_observation_id": source_observation_id,
            "model": "transparent-v1"
        }),
        evidence_ids,
    ))
}

fn sensor_health_timestamp(timestamp_us: i64, now_us: i64) -> f64 {
    let age_us = now_us.saturating_sub(timestamp_us).max(0);
    (1.0 - age_us as f64 / SENSOR_HEALTH_HORIZON_US as f64).clamp(0.0, 1.0)
}

/// Validate and persist one envelope. The payload is decoded as JSON (every organ built so far
/// emits JSON feature payloads) and stored via `append_observation_with_features`, which keeps
/// the same sensor-gap tracking as every other ingest path in `liminal-ledger`.
pub fn ingest_envelope(ledger: &mut SqliteLedger, envelope: &Envelope) -> Result<(), IngestError> {
    validate_schema_version(envelope)?;
    if envelope.message_id.is_empty() || envelope.message_id.len() > MAX_MESSAGE_ID_BYTES {
        return Err(IngestError::InvalidMessageId);
    }
    // Sensor clients may reconnect and replay their last acknowledged frame. Message IDs are the
    // idempotency key: an already persisted ID is a successful no-op, including its derived
    // belief, rather than a second observation or a database uniqueness failure.
    if ledger.contains_event_id(&envelope.message_id)? {
        return Ok(());
    }
    let mut features: serde_json::Value =
        serde_json::from_slice(&envelope.payload).map_err(IngestError::InvalidPayload)?;
    validate_feature_payload(&envelope.sensor_stream_id, &features)?;
    if let Some(last_sequence) = ledger.last_monotonic_sequence(&envelope.sensor_stream_id) {
        if envelope.monotonic_sequence > last_sequence.saturating_add(1) {
            ledger.record_sensor_gap(
                format!(
                    "gap:{}:{}-{}",
                    envelope.sensor_stream_id, last_sequence, envelope.monotonic_sequence
                ),
                &envelope.sensor_stream_id,
                envelope.captured_at_utc_us,
            )?;
        }
    }
    features
        .as_object_mut()
        .expect("validated feature object")
        .insert(
            "_monotonic_sequence".to_string(),
            serde_json::Value::from(envelope.monotonic_sequence),
        );
    ledger.append_observation_with_features(
        envelope.message_id.clone(),
        &envelope.sensor_stream_id,
        envelope.captured_at_utc_us,
        features,
    )?;
    let source_streams = ["camera", "microphone", "bluetooth"];
    if source_streams
        .iter()
        .all(|stream| !ledger.has_unacknowledged_sensor_gap(stream))
    {
        let camera = ledger.latest_observation("camera")?;
        let microphone = ledger.latest_observation("microphone")?;
        let bluetooth = ledger.latest_observation("bluetooth")?;
        if let Some((belief_features, evidence_ids)) = derive_belief_features_from_latest(
            camera.as_ref(),
            microphone.as_ref(),
            bluetooth.as_ref(),
            &envelope.message_id,
            envelope.captured_at_utc_us,
        ) {
            ledger.append_belief_with_features_and_evidence(
                format!("belief:{}", envelope.message_id),
                "fusion",
                envelope.captured_at_utc_us,
                belief_features,
                &evidence_ids,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn socket_path_for_uid_matches_section_15s_shape() {
        assert_eq!(
            socket_path_for_uid(501).to_str().unwrap(),
            "/tmp/liminal-501/core.sock"
        );
    }

    #[test]
    fn prepare_socket_path_creates_a_0700_directory() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("liminald-test-{}", std::process::id()));
        let path = dir.join("core.sock");
        let _ = std::fs::remove_dir_all(&dir);

        prepare_socket_path(&path).unwrap();

        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn prepare_socket_path_removes_a_stale_socket_file() {
        let dir = std::env::temp_dir().join(format!("liminald-test-stale-{}", std::process::id()));
        let path = dir.join("core.sock");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"stale").unwrap();

        prepare_socket_path(&path).unwrap();

        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn prepare_socket_path_refuses_to_unlink_an_active_listener() {
        use std::os::unix::net::UnixListener;

        let dir = std::env::temp_dir().join(format!("liminald-test-active-{}", std::process::id()));
        let path = dir.join("core.sock");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _listener = UnixListener::bind(&path).unwrap();

        assert!(matches!(
            prepare_socket_path(&path),
            Err(PrepareSocketError::ActiveSocket)
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn sample_envelope(schema_version: u32, payload: &[u8]) -> Envelope {
        Envelope {
            schema_version,
            message_id: "msg-1".to_string(),
            sensor_stream_id: "camera".to_string(),
            monotonic_sequence: 1,
            captured_at_utc_us: 1_000,
            captured_at_mono_ns: 2_000,
            payload: payload.to_vec(),
        }
    }

    fn sample_envelope_with_sequence(
        message_id: &str,
        stream_id: &str,
        sequence: u64,
        timestamp_us: i64,
    ) -> Envelope {
        Envelope {
            schema_version: 1,
            message_id: message_id.to_string(),
            sensor_stream_id: stream_id.to_string(),
            monotonic_sequence: sequence,
            captured_at_utc_us: timestamp_us,
            captured_at_mono_ns: timestamp_us,
            payload: br#"{"body_count":"one","joints":[]}"#.to_vec(),
        }
    }

    #[test]
    fn read_length_delimited_envelope_decodes_a_real_frame() {
        use prost::Message;
        let envelope = sample_envelope(1, b"{}");
        let body = envelope.encode_to_vec();
        let mut frame = Vec::new();
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&body);

        let mut cursor = Cursor::new(frame);
        let decoded = read_length_delimited_envelope(&mut cursor)
            .unwrap()
            .unwrap();
        assert_eq!(decoded.message_id, "msg-1");
    }

    #[test]
    fn read_length_delimited_envelope_rejects_an_oversized_frame_before_allocating() {
        let length = (MAX_FRAME_BYTES + 1) as u32;
        let mut frame = Vec::new();
        frame.extend_from_slice(&length.to_be_bytes());
        let err = read_length_delimited_envelope(&mut Cursor::new(frame)).unwrap_err();
        match err {
            FrameReadError::FrameTooLarge {
                length: actual,
                maximum,
            } => {
                assert_eq!(actual, MAX_FRAME_BYTES + 1);
                assert_eq!(maximum, MAX_FRAME_BYTES);
            }
            other => panic!("expected oversized-frame error, got {other:?}"),
        }
    }

    #[test]
    fn read_length_delimited_envelope_returns_none_on_clean_eof() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        assert!(read_length_delimited_envelope(&mut cursor)
            .unwrap()
            .is_none());
    }

    #[test]
    fn read_length_delimited_envelope_errors_on_a_truncated_payload() {
        let mut frame = Vec::new();
        frame.extend_from_slice(&100u32.to_be_bytes()); // promises 100 bytes
        frame.extend_from_slice(b"short"); // far fewer
        let mut cursor = Cursor::new(frame);
        assert!(matches!(
            read_length_delimited_envelope(&mut cursor),
            Err(FrameReadError::Io(_))
        ));
    }

    #[test]
    fn ingest_envelope_rejects_a_schema_mismatch_without_touching_the_ledger() {
        let path = std::env::temp_dir().join(format!(
            "liminald-ingest-test-mismatch-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();

        let envelope = sample_envelope(999, b"{}");
        let err = ingest_envelope(&mut ledger, &envelope).unwrap_err();
        assert!(matches!(err, IngestError::SchemaMismatch(_)));
        assert!(ledger.events().unwrap().is_empty());

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn ingest_envelope_rejects_non_json_payload() {
        let path = std::env::temp_dir().join(format!(
            "liminald-ingest-test-badjson-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();

        let envelope = sample_envelope(1, b"not json");
        let err = ingest_envelope(&mut ledger, &envelope).unwrap_err();
        assert!(matches!(err, IngestError::InvalidPayload(_)));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn ingest_envelope_rejects_empty_or_oversized_message_ids() {
        let path = std::env::temp_dir().join(format!(
            "liminald-ingest-test-message-id-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();

        let mut empty_id = sample_envelope(1, br#"{"body_count":"one","joints":[]}"#);
        empty_id.message_id.clear();
        assert!(matches!(
            ingest_envelope(&mut ledger, &empty_id),
            Err(IngestError::InvalidMessageId)
        ));

        let mut oversized_id = sample_envelope(1, br#"{"body_count":"one","joints":[]}"#);
        oversized_id.message_id = "x".repeat(MAX_MESSAGE_ID_BYTES + 1);
        assert!(matches!(
            ingest_envelope(&mut ledger, &oversized_id),
            Err(IngestError::InvalidMessageId)
        ));
        assert!(ledger.events().unwrap().is_empty());

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn ingest_envelope_rejects_raw_or_unknown_sensor_fields_before_persistence() {
        let path = std::env::temp_dir().join(format!(
            "liminald-ingest-test-privacy-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();

        let envelope = sample_envelope(1, br#"{"body_count":"one","raw_frame":"pixels"}"#);
        let err = ingest_envelope(&mut ledger, &envelope).unwrap_err();
        assert!(matches!(
            err,
            IngestError::ForbiddenFeatureField { field, .. } if field == "raw_frame"
        ));
        assert!(ledger.events().unwrap().is_empty());

        let envelope = sample_envelope(
            1,
            br#"{"body_count":"one","joints":[{"name":"nose","x":0.5,"y":0.5,"raw_frame":"pixels"}]}"#,
        );
        let err = ingest_envelope(&mut ledger, &envelope).unwrap_err();
        assert!(matches!(
            err,
            IngestError::ForbiddenFeatureField { field, .. } if field == "joints[].raw_frame"
        ));
        assert!(ledger.events().unwrap().is_empty());

        let mut bluetooth = sample_envelope_with_sequence("bt-1", "bluetooth", 1, 1_000);
        bluetooth.payload =
            br#"{"clusters":[{"pseudonym":"AA:BB:CC:DD:EE:FF","rssi":-50}],"cluster_count":1}"#
                .to_vec();
        let err = ingest_envelope(&mut ledger, &bluetooth).unwrap_err();
        assert!(
            matches!(err, IngestError::InvalidFeatureShape { stream } if stream == "bluetooth")
        );
        assert!(ledger.events().unwrap().is_empty());

        bluetooth.payload = format!(
            r#"{{"clusters":[{{"pseudonym":"ble:{}","rssi":-50}}],"cluster_count":1}}"#,
            "0".repeat(64)
        )
        .into_bytes();
        ingest_envelope(&mut ledger, &bluetooth).unwrap();
        assert_eq!(
            ledger
                .events()
                .unwrap()
                .iter()
                .filter(|event| event.kind == "observation")
                .count(),
            1
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn ingest_envelope_persists_a_valid_envelope_with_its_features() {
        let path =
            std::env::temp_dir().join(format!("liminald-ingest-test-ok-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();

        let envelope = sample_envelope(1, br#"{"body_count":"one","joints":[]}"#);
        ingest_envelope(&mut ledger, &envelope).unwrap();
        ingest_envelope(&mut ledger, &envelope).unwrap();

        let events = ledger.events().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, "msg-1");
        assert_eq!(events[0].payload["stream_id"], "camera");
        assert_eq!(events[0].payload["features"]["body_count"], "one");
        assert_eq!(events[1].kind, "belief");
        assert_eq!(events[1].payload["stream_id"], "fusion");
        assert_eq!(
            events[1].payload["derived_from"],
            serde_json::json!(["msg-1"])
        );
        assert_eq!(
            events[1].payload["features"]["source_observation_id"],
            "msg-1"
        );
        assert!(ledger.verify_chain().is_ok());

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn ingest_envelope_records_a_sensor_gap_when_sequence_jumps() {
        let path = std::env::temp_dir().join(format!(
            "liminald-ingest-test-sequence-gap-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();

        ingest_envelope(
            &mut ledger,
            &sample_envelope_with_sequence("msg-1", "camera", 1, 1_000),
        )
        .unwrap();
        ingest_envelope(
            &mut ledger,
            &sample_envelope_with_sequence("msg-3", "camera", 3, 3_000),
        )
        .unwrap();
        ingest_envelope(
            &mut ledger,
            &sample_envelope_with_sequence("msg-2", "camera", 2, 2_000),
        )
        .unwrap();
        ingest_envelope(
            &mut ledger,
            &sample_envelope_with_sequence("msg-4", "camera", 4, 4_000),
        )
        .unwrap();

        let events = ledger.events().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    event.kind == "sensor_gap" && event.payload["stream_id"] == "camera"
                })
                .count(),
            1
        );
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn ingest_envelope_persists_all_latest_sensor_sources_as_belief_evidence() {
        let path = std::env::temp_dir().join(format!(
            "liminald-ingest-test-provenance-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();

        let mut camera = sample_envelope_with_sequence("camera-1", "camera", 1, 1_000);
        camera.payload = br#"{"body_count":"one","joints":[]}"#.to_vec();
        ingest_envelope(&mut ledger, &camera).unwrap();
        let mut microphone = sample_envelope_with_sequence("mic-1", "microphone", 1, 1_000);
        microphone.payload = br#"{"voice_activity_probability":0.4}"#.to_vec();
        ingest_envelope(&mut ledger, &microphone).unwrap();

        let events = ledger.events().unwrap();
        let belief = events
            .iter()
            .rev()
            .find(|event| event.kind == "belief")
            .unwrap();
        assert_eq!(
            belief.payload["derived_from"],
            serde_json::json!(["camera-1", "mic-1"])
        );
        assert_eq!(
            ledger.provenance_sources(&belief.id).unwrap(),
            ["camera-1", "mic-1"]
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn fusion_marks_contradictory_modalities_contested() {
        let path = std::env::temp_dir().join(format!(
            "liminald-fusion-health-test-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();
        ledger
            .append_observation_with_features(
                "camera-1",
                "camera",
                1_000,
                serde_json::json!({"body_count":"one"}),
            )
            .unwrap();
        ledger
            .append_observation_with_features(
                "mic-1",
                "microphone",
                1_000,
                serde_json::json!({"voice_activity_probability":0.0}),
            )
            .unwrap();
        let features =
            derive_belief_features(&ledger.events().unwrap(), "camera-1", 1_000).unwrap();
        assert_eq!(features["state"], "contested");
        assert!((features["occupancy_probability"].as_f64().unwrap() - (0.65 / 0.85)).abs() < 1e-9);
        assert!(features["disagreement"].as_f64().unwrap() > 0.9);
        assert_eq!(features["sensor_health"], 1.0);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn fusion_drops_stale_modalities_after_health_horizon() {
        let path = std::env::temp_dir().join(format!(
            "liminald-fusion-stale-test-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();
        ledger
            .append_observation_with_features(
                "camera-1",
                "camera",
                1_000,
                serde_json::json!({"body_count":"one"}),
            )
            .unwrap();
        assert!(derive_belief_features(
            &ledger.events().unwrap(),
            "camera-1",
            1_000 + SENSOR_HEALTH_HORIZON_US + 1,
        )
        .is_none());

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn fusion_weights_modalities_by_their_relative_freshness() {
        let path = std::env::temp_dir().join(format!(
            "liminald-fusion-relative-health-test-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();
        ledger
            .append_observation_with_features(
                "camera-1",
                "camera",
                0,
                serde_json::json!({"body_count":"one"}),
            )
            .unwrap();
        ledger
            .append_observation_with_features(
                "mic-1",
                "microphone",
                9_000_000,
                serde_json::json!({"voice_activity_probability":0.0}),
            )
            .unwrap();
        let features =
            derive_belief_features(&ledger.events().unwrap(), "mic-1", 10_000_000).unwrap();
        assert_eq!(features["occupancy_probability"], 0.0);
        assert!(features["sensor_health"].as_f64().unwrap() > 0.0);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn validate_feature_payload_rejects_malformed_sensor_shapes() {
        use serde_json::json;

        assert!(matches!(
            validate_feature_payload("unknown", &json!({})),
            Err(IngestError::UnsupportedSensorStream(stream)) if stream == "unknown"
        ));
        assert!(matches!(
            validate_feature_payload("camera", &json!(null)),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "camera"
        ));
        assert!(matches!(
            validate_feature_payload("camera", &json!({"body_count":"many"})),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "camera"
        ));
        assert!(matches!(
            validate_feature_payload("camera", &json!({"body_count":"one", "joints":{}})),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "camera"
        ));

        let too_many_joints = (0..=MAX_DERIVED_COLLECTION_ITEMS)
            .map(|_| json!({"name":"nose", "x":0.5, "y":0.5, "confidence":1.0}))
            .collect::<Vec<_>>();
        assert!(matches!(
            validate_feature_payload(
                "camera",
                &json!({"body_count":"one", "joints":too_many_joints})
            ),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "camera"
        ));
        assert!(matches!(
            validate_feature_payload(
                "camera",
                &json!({"body_count":"one", "joints":[null]})
            ),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "camera"
        ));
        assert!(matches!(
            validate_feature_payload(
                "camera",
                &json!({"body_count":"one", "joints":[{"x":0.5, "y":0.5, "confidence":1.0}]})
            ),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "camera"
        ));
        assert!(matches!(
            validate_feature_payload(
                "camera",
                &json!({"body_count":"one", "joints":[{"name":"nose", "y":0.5, "confidence":1.0}]})
            ),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "camera"
        ));

        assert!(validate_feature_payload(
            "wifi",
            &json!({
                "rssi_mean":-50.0,
                "noise_mean":-90.0,
                "visible_network_count":3,
                "strongest_1":-40,
                "strongest_2":-50,
                "strongest_3":-60
            })
        )
        .is_ok());
        assert!(matches!(
            validate_feature_payload("microphone", &json!({"rms":"loud"})),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "microphone"
        ));
        assert!(matches!(
            validate_feature_payload("wifi", &json!({"rssi_mean":"strong"})),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "wifi"
        ));

        assert!(matches!(
            validate_feature_payload("bluetooth", &json!({"cluster_count":"many"})),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "bluetooth"
        ));
        assert!(matches!(
            validate_feature_payload("bluetooth", &json!({"clusters":{}})),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "bluetooth"
        ));
        let too_many_clusters = (0..=MAX_DERIVED_COLLECTION_ITEMS)
            .map(|_| json!({"pseudonym":"ble:0000000000000000000000000000000000000000000000000000000000000000", "rssi":-50}))
            .collect::<Vec<_>>();
        assert!(matches!(
            validate_feature_payload(
                "bluetooth",
                &json!({"clusters":too_many_clusters})
            ),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "bluetooth"
        ));
        assert!(matches!(
            validate_feature_payload("bluetooth", &json!({"clusters":[null]})),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "bluetooth"
        ));
        assert!(matches!(
            validate_feature_payload(
                "bluetooth",
                &json!({"clusters":[{"pseudonym":"ble:0000000000000000000000000000000000000000000000000000000000000000", "rssi":-50, "extra":true}]})
            ),
            Err(IngestError::ForbiddenFeatureField { field, .. }) if field == "clusters[].extra"
        ));
        assert!(matches!(
            validate_feature_payload(
                "bluetooth",
                &json!({"clusters":[{"pseudonym":"ble:zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz", "rssi":-50}]})
            ),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "bluetooth"
        ));
        assert!(matches!(
            validate_feature_payload(
                "bluetooth",
                &json!({"clusters":[{"pseudonym":"ble:0000000000000000000000000000000000000000000000000000000000000000"}]})
            ),
            Err(IngestError::InvalidFeatureShape { stream }) if stream == "bluetooth"
        ));
        assert!(validate_feature_payload(
            "bluetooth",
            &json!({"clusters":[{"pseudonym":"ble:0000000000000000000000000000000000000000000000000000000000000000", "rssi":-50}]})
        )
        .is_ok());
    }

    #[test]
    fn derive_belief_features_handles_empty_and_stable_all_modality_inputs() {
        use serde_json::json;

        assert!(derive_belief_features(&[], "none", 0).is_none());

        let path = std::env::temp_dir().join(format!(
            "liminald-fusion-all-modalities-test-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();
        ledger
            .append_observation_with_features(
                "camera-1",
                "camera",
                1_000,
                json!({"body_count":"one"}),
            )
            .unwrap();
        ledger
            .append_observation_with_features(
                "mic-1",
                "microphone",
                1_000,
                json!({"voice_activity_probability":0.8}),
            )
            .unwrap();
        ledger
            .append_observation_with_features(
                "bt-1",
                "bluetooth",
                1_000,
                json!({"cluster_count":3.2}),
            )
            .unwrap();

        let features = derive_belief_features(&ledger.events().unwrap(), "camera-1", 1_000)
            .expect("fresh all-modality evidence should produce a belief");
        assert_eq!(features["observed_modalities"], 3);
        assert_eq!(features["state"], "stable");
        assert_eq!(features["sensor_health"], 1.0);
        assert!(features["confidence"].as_f64().unwrap() > 0.0);

        std::fs::remove_file(&path).unwrap();
    }
}
