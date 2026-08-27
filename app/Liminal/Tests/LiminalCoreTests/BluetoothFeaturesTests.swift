@testable import LiminalCore
import XCTest

final class BluetoothFeaturesTests: XCTestCase {
    func testPseudonymizeBlePeripheralIsNotIdentityAndIsPrefixed() {
        let key = Data("local-key".utf8)
        let out = pseudonymizeBlePeripheral(key: key, identifier: "AA-BB-CC-DD")
        XCTAssertNotEqual(out, "AA-BB-CC-DD")
        XCTAssertTrue(out.hasPrefix("ble:"))
    }

    func testPseudonymizeBlePeripheralIsStableForTheSameIdentifierAndKey() {
        let key = Data("local-key".utf8)
        XCTAssertEqual(
            pseudonymizeBlePeripheral(key: key, identifier: "same-id"),
            pseudonymizeBlePeripheral(key: key, identifier: "same-id"),
        )
    }

    func testBluetoothScanWindowNeverSerializesADeviceName() throws {
        let window = BluetoothScanWindow(clusters: [
            BluetoothClusterObservation(pseudonym: "ble:abc123", rssi: -55),
        ])
        let data = try JSONEncoder().encode(window)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        for forbiddenKey in ["name", "device_name", "peripheral_name"] {
            XCTAssertNil(json[forbiddenKey])
        }
        let clusters = try XCTUnwrap(json["clusters"] as? [[String: Any]])
        XCTAssertEqual(clusters.first?["pseudonym"] as? String, "ble:abc123")
        XCTAssertEqual(json["cluster_count"] as? Int, 1)
    }

    func testBluetoothAcceptanceDistinguishesNoAdvertisersFromStartupFailure() {
        XCTAssertEqual(
            bluetoothAcceptanceStatus(startupStatus: "running", discoveredSampleCount: 0),
            "no_advertisers_observed",
        )
        XCTAssertEqual(
            bluetoothAcceptanceStatus(startupStatus: "keychain_timeout", discoveredSampleCount: 0),
            "keychain_timeout",
        )
        XCTAssertEqual(
            bluetoothAcceptanceStatus(startupStatus: "running", discoveredSampleCount: 1),
            "observed",
        )
        XCTAssertEqual(
            bluetoothAcceptanceStatus(
                startupStatus: "keychain_timeout",
                discoveredSampleCount: 0,
                discoveredPeripheralCount: 1,
            ),
            "advertisers_detected_keychain_unavailable",
        )
    }
}
