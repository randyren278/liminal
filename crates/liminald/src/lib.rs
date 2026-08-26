//! `liminald` -- the Rust daemon that owns canonical state (ROADMAP item 3).
//!
//! Master plan reference: §14 (`liminald` owns canonical state, IPC, DB, fusion; must not
//! directly access camera/microphone), §15 (Unix domain socket under `/tmp/liminal-$UID/`,
//! 0700 dir / 0600 socket, length-delimited protobuf frames).
//!
//! This is an ingest-only skeleton: accept envelopes from a sensor organ (today, only
//! `liminal-capture`'s Vision organ exists), validate their schema version, and persist them as
//! observations via `liminal_ledger::SqliteLedger`. No fusion, no belief frames yet -- that's
//! later roadmap work, once there's more than one organ's worth of real data to fuse.

use std::io::{self, ErrorKind, Read};
use std::path::PathBuf;

use liminal_ipc::{validate_schema_version, Envelope};
use liminal_ledger::SqliteLedger;

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
}

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
    #[error(transparent)]
    Ledger(#[from] liminal_ledger::LedgerError),
}

/// Validate and persist one envelope. The payload is decoded as JSON (every organ built so far
/// emits JSON feature payloads) and stored via `append_observation_with_features`, which keeps
/// the same sensor-gap tracking as every other ingest path in `liminal-ledger`.
pub fn ingest_envelope(ledger: &mut SqliteLedger, envelope: &Envelope) -> Result<(), IngestError> {
    validate_schema_version(envelope)?;
    let features: serde_json::Value =
        serde_json::from_slice(&envelope.payload).map_err(IngestError::InvalidPayload)?;
    ledger.append_observation_with_features(
        envelope.message_id.clone(),
        &envelope.sensor_stream_id,
        envelope.captured_at_utc_us,
        features,
    )?;
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
    fn ingest_envelope_persists_a_valid_envelope_with_its_features() {
        let path =
            std::env::temp_dir().join(format!("liminald-ingest-test-ok-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut ledger = SqliteLedger::open(&path, 30_000_000).unwrap();

        let envelope = sample_envelope(1, br#"{"body_count":"one","joints":[]}"#);
        ingest_envelope(&mut ledger, &envelope).unwrap();

        let events = ledger.events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "msg-1");
        assert_eq!(events[0].payload["stream_id"], "camera");
        assert_eq!(events[0].payload["features"]["body_count"], "one");
        assert!(ledger.verify_chain().is_ok());

        std::fs::remove_file(&path).unwrap();
    }
}
