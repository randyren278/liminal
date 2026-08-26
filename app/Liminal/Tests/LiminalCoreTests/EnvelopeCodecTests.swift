@testable import LiminalCore
import XCTest

final class EnvelopeCodecTests: XCTestCase {
    func testMakeEnvelopeSetsEveryFieldFromItsArguments() {
        let payload = Data("hello".utf8)
        let envelope = makeEnvelope(
            messageId: "msg-1",
            sensorStreamId: "camera",
            monotonicSequence: 42,
            capturedAtUtcUs: 1_700_000_000_000_000,
            capturedAtMonoUs: 123_456,
            payload: payload,
        )

        XCTAssertEqual(envelope.schemaVersion, currentSchemaVersion)
        XCTAssertEqual(envelope.messageID, "msg-1")
        XCTAssertEqual(envelope.sensorStreamID, "camera")
        XCTAssertEqual(envelope.monotonicSequence, 42)
        XCTAssertEqual(envelope.capturedAtUtcUs, 1_700_000_000_000_000)
        XCTAssertEqual(envelope.capturedAtMonoNs, 123_456)
        XCTAssertEqual(envelope.payload, payload)
    }

    func testLengthDelimitedFrameRoundTripsThroughReadLengthDelimitedEnvelope() throws {
        let envelope = makeEnvelope(
            messageId: "msg-2",
            sensorStreamId: "wifi",
            monotonicSequence: 7,
            capturedAtUtcUs: 1,
            capturedAtMonoUs: 2,
            payload: Data([1, 2, 3, 4]),
        )

        let frame = try lengthDelimitedFrame(envelope)
        let (decoded, consumed) = try readLengthDelimitedEnvelope(from: frame)

        XCTAssertEqual(consumed, frame.count)
        XCTAssertEqual(decoded, envelope)
    }

    func testLengthDelimitedFrameCanBeFollowedByAnotherFrameInTheSameBuffer() throws {
        let first = makeEnvelope(
            messageId: "a", sensorStreamId: "s", monotonicSequence: 1, capturedAtUtcUs: 0,
            capturedAtMonoUs: 0, payload: Data([9]),
        )
        let second = makeEnvelope(
            messageId: "b", sensorStreamId: "s", monotonicSequence: 2, capturedAtUtcUs: 0,
            capturedAtMonoUs: 0, payload: Data([8, 7]),
        )

        var buffer = try lengthDelimitedFrame(first)
        try buffer.append(lengthDelimitedFrame(second))

        let (decodedFirst, consumed1) = try readLengthDelimitedEnvelope(from: buffer)
        let (decodedSecond, _) = try readLengthDelimitedEnvelope(from: buffer.suffix(from: consumed1))

        XCTAssertEqual(decodedFirst.messageID, "a")
        XCTAssertEqual(decodedSecond.messageID, "b")
    }

    func testReadLengthDelimitedEnvelopeRejectsATruncatedLengthPrefix() {
        let tooShort = Data([0, 0, 1])
        XCTAssertThrowsError(try readLengthDelimitedEnvelope(from: tooShort)) { error in
            XCTAssertEqual(error as? EnvelopeFramingError, .truncatedLengthPrefix)
        }
    }

    func testReadLengthDelimitedEnvelopeRejectsATruncatedPayload() {
        var data = Data()
        var length = UInt32(100).bigEndian
        withUnsafeBytes(of: &length) { data.append(contentsOf: $0) }
        data.append(Data([1, 2, 3])) // far fewer than the 100 bytes the prefix promises

        XCTAssertThrowsError(try readLengthDelimitedEnvelope(from: data)) { error in
            XCTAssertEqual(error as? EnvelopeFramingError, .truncatedPayload)
        }
    }
}
