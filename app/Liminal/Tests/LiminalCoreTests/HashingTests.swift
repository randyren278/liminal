@testable import LiminalCore
import XCTest

final class HashingTests: XCTestCase {
    func testSha256HexIsDeterministicAndPrefixed() {
        let a = sha256Hex("some-hardware-id", prefix: "camera")
        let b = sha256Hex("some-hardware-id", prefix: "camera")
        XCTAssertEqual(a, b)
        XCTAssertTrue(a.hasPrefix("camera:"))
    }

    func testSha256HexIsNotIdentityAndDiffersByInput() {
        let a = sha256Hex("device-a", prefix: "camera")
        let b = sha256Hex("device-b", prefix: "camera")
        XCTAssertNotEqual(a, b)
        XCTAssertFalse(a.contains("device-a"))
    }

    func testSha256HexProducesA64CharacterHexDigest() throws {
        let hash = sha256Hex("anything", prefix: "x")
        let hex = try XCTUnwrap(hash.split(separator: ":").last)
        XCTAssertEqual(hex.count, 64)
        XCTAssertTrue(hex.allSatisfy(\.isHexDigit))
    }

    func testMachineProfileIdIsStableAcrossCallsOnTheSameMachine() {
        XCTAssertEqual(machineProfileId(), machineProfileId())
        XCTAssertTrue(machineProfileId().hasPrefix("machine:"))
    }

    // MARK: - hmacSha256Hex

    func testHmacSha256HexIsNotIdentityAndIsPrefixed() {
        let key = Data("local-key".utf8)
        let out = hmacSha256Hex(key: key, message: "AA:BB:CC:DD:EE:FF", prefix: "ble")
        XCTAssertNotEqual(out, "AA:BB:CC:DD:EE:FF")
        XCTAssertTrue(out.hasPrefix("ble:"))
    }

    func testHmacSha256HexIsDeterministicForTheSameKeyAndMessage() {
        let key = Data("local-key".utf8)
        let a = hmacSha256Hex(key: key, message: "AA:BB:CC:DD:EE:FF", prefix: "ble")
        let b = hmacSha256Hex(key: key, message: "AA:BB:CC:DD:EE:FF", prefix: "ble")
        XCTAssertEqual(a, b)
    }

    func testHmacSha256HexDiffersAcrossKeys() {
        let a = hmacSha256Hex(key: Data("key-one".utf8), message: "AA:BB:CC:DD:EE:FF", prefix: "ble")
        let b = hmacSha256Hex(key: Data("key-two".utf8), message: "AA:BB:CC:DD:EE:FF", prefix: "ble")
        XCTAssertNotEqual(a, b)
    }

    func testHmacSha256HexProducesA64CharacterHexDigest() throws {
        let out = hmacSha256Hex(key: Data("k".utf8), message: "m", prefix: "x")
        let hex = try XCTUnwrap(out.split(separator: ":").last)
        XCTAssertEqual(hex.count, 64)
        XCTAssertTrue(hex.allSatisfy(\.isHexDigit))
    }
}
