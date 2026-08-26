//! IPC wire envelope: the Protocol Buffers message every `liminald` IPC frame is wrapped in.
//!
//! Master plan reference: LIMINAL_MASTER_PLAN.md §15 (IPC — envelope fields, `prost` as the
//! Rust transport library), §119 (IPC/Event Spine — "reject schema mismatch"). Reconnect,
//! dedup-by-message-ID, sequence-gap detection, and bounded-queue backpressure are §119
//! requirements for the future `liminald` daemon, not this crate.

include!(concat!(env!("OUT_DIR"), "/liminal.rs"));

/// The `schema_version` this crate's `Envelope` decoder expects. Bump when `proto/liminal.proto`
/// makes a breaking change to the envelope shape.
pub const EXPECTED_SCHEMA_VERSION: u32 = 1;

/// Returned by [`validate_schema_version`] when a decoded envelope's `schema_version` does not
/// match [`EXPECTED_SCHEMA_VERSION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("schema mismatch: expected version {expected}, got {actual}")]
pub struct SchemaMismatch {
    pub expected: u32,
    pub actual: u32,
}

/// §119 "reject schema mismatch": rejects a decoded [`Envelope`] whose `schema_version` does not
/// match [`EXPECTED_SCHEMA_VERSION`], rather than silently accepting it.
pub fn validate_schema_version(envelope: &Envelope) -> Result<(), SchemaMismatch> {
    if envelope.schema_version != EXPECTED_SCHEMA_VERSION {
        return Err(SchemaMismatch {
            expected: EXPECTED_SCHEMA_VERSION,
            actual: envelope.schema_version,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    fn sample_envelope() -> Envelope {
        Envelope {
            schema_version: EXPECTED_SCHEMA_VERSION,
            message_id: "01J8QKXYZ".to_string(),
            sensor_stream_id: "camera-0".to_string(),
            monotonic_sequence: 42,
            captured_at_utc_us: 1_700_000_000_000_000,
            captured_at_mono_ns: 123_456_789,
            payload: vec![1, 2, 3, 4],
        }
    }

    #[test]
    fn round_trips_through_encode_decode() {
        let original = sample_envelope();

        let mut buf = Vec::new();
        original.encode(&mut buf).expect("encode");
        let decoded = Envelope::decode(buf.as_slice()).expect("decode");

        assert_eq!(original, decoded);
    }

    #[test]
    fn accepts_matching_schema_version() {
        let envelope = sample_envelope();
        assert!(validate_schema_version(&envelope).is_ok());
    }

    #[test]
    fn rejects_mismatched_schema_version() {
        let mut envelope = sample_envelope();
        envelope.schema_version = EXPECTED_SCHEMA_VERSION + 1;

        let err = validate_schema_version(&envelope).unwrap_err();

        assert_eq!(
            err,
            SchemaMismatch {
                expected: EXPECTED_SCHEMA_VERSION,
                actual: EXPECTED_SCHEMA_VERSION + 1,
            }
        );
    }
}
