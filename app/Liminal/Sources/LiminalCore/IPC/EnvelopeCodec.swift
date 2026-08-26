import Foundation
import SwiftProtobuf

// Builds and frames `Liminal_Envelope` messages -- master plan §15 (IPC envelope fields,
// "length-delimited frames"). The generated `Liminal_Envelope` type lives in
// `Generated/liminal.pb.swift`, produced from `proto/liminal.proto` via:
//
//   protoc --swift_out=Sources/LiminalCore/Generated --swift_opt=Visibility=Public \
//     --proto_path=../../proto ../../proto/liminal.proto
//
// Re-run that command after editing `proto/liminal.proto`; never hand-edit the generated file.

/// The schema version this build emits. Must match `liminal_ipc::EXPECTED_SCHEMA_VERSION` on the
/// Rust side (crates/liminal-ipc/src/lib.rs) -- kept as a plain constant here since Swift and Rust
/// don't share a build-time source of truth for this yet.
public let currentSchemaVersion: UInt32 = 1

/// Build one envelope. `capturedAtUtcUs`/`capturedAtMonoUs` are microseconds since the Unix epoch
/// and since an arbitrary monotonic origin respectively (§16 Clock Model) -- callers supply both
/// explicitly (rather than this function reading the clock) so the construction logic stays a
/// pure, testable function.
public func makeEnvelope(
    messageId: String,
    sensorStreamId: String,
    monotonicSequence: UInt64,
    capturedAtUtcUs: Int64,
    capturedAtMonoUs: Int64,
    payload: Data,
) -> Liminal_Envelope {
    var envelope = Liminal_Envelope()
    envelope.schemaVersion = currentSchemaVersion
    envelope.messageID = messageId
    envelope.sensorStreamID = sensorStreamId
    envelope.monotonicSequence = monotonicSequence
    envelope.capturedAtUtcUs = capturedAtUtcUs
    envelope.capturedAtMonoNs = capturedAtMonoUs
    envelope.payload = payload
    return envelope
}

public enum EnvelopeFramingError: Error, Equatable {
    case truncatedLengthPrefix
    case truncatedPayload
}

/// §15 "length-delimited frames": a 4-byte big-endian length prefix followed by the serialized
/// protobuf message. Symmetric with `readLengthDelimitedEnvelope`.
public func lengthDelimitedFrame(_ envelope: Liminal_Envelope) throws -> Data {
    let body = try envelope.serializedData()
    var frame = Data()
    var length = UInt32(body.count).bigEndian
    withUnsafeBytes(of: &length) { frame.append(contentsOf: $0) }
    frame.append(body)
    return frame
}

/// Decode one length-delimited frame from the front of `data`, returning the envelope and the
/// number of bytes consumed. Used by tests to prove `lengthDelimitedFrame` round-trips, and would
/// be used by a Swift-side reader if one existed (today only Rust reads this stream).
public func readLengthDelimitedEnvelope(from data: Data) throws -> (Liminal_Envelope, Int) {
    guard data.count >= 4 else { throw EnvelopeFramingError.truncatedLengthPrefix }
    // Built byte-by-byte rather than `.load(as: UInt32.self)`: `data.prefix(4)`'s storage is not
    // guaranteed 4-byte aligned (it's a subrange view into a larger buffer), and `load(as:)`
    // requires alignment -- this crashed with "load from misaligned raw pointer" under real
    // socket-received data before this fix.
    let lengthBytes = Array(data.prefix(4))
    let length =
        (UInt32(lengthBytes[0]) << 24) | (UInt32(lengthBytes[1]) << 16) | (UInt32(lengthBytes[2]) << 8)
            | UInt32(lengthBytes[3])
    let start = data.startIndex + 4
    guard data.count >= 4 + Int(length) else { throw EnvelopeFramingError.truncatedPayload }
    let body = data.subdata(in: start ..< (start + Int(length)))
    let envelope = try Liminal_Envelope(serializedBytes: body)
    return (envelope, 4 + Int(length))
}
