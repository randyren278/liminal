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
}
