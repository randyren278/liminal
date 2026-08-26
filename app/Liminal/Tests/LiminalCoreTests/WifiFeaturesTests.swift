@testable import LiminalCore
import XCTest

final class WifiFeaturesTests: XCTestCase {
    func testSanitizeWifiModeANeverSerializesSsidOrBssid() throws {
        let raw = [
            RawWifiScanResult(ssid: "HomeNetwork", bssid: "AA:BB:CC:DD:EE:FF", rssi: -45, noise: -90),
            RawWifiScanResult(ssid: "Neighbor", bssid: "11:22:33:44:55:66", rssi: -70, noise: -92),
        ]
        let sanitized = sanitizeWifiModeA(raw)
        let data = try JSONEncoder().encode(sanitized)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])

        for forbiddenKey in ["ssid", "bssid"] {
            XCTAssertNil(json[forbiddenKey], "forbidden key '\(forbiddenKey)' leaked into Mode A JSON")
        }
        // Belt-and-suspenders: the raw SSID/BSSID strings themselves must not appear anywhere in
        // the encoded bytes, not just as top-level keys.
        let rawText = String(data: data, encoding: .utf8) ?? ""
        XCTAssertFalse(rawText.contains("HomeNetwork"))
        XCTAssertFalse(rawText.contains("AA:BB:CC:DD:EE:FF"))

        XCTAssertEqual(sanitized.visibleNetworkCount, 2)
    }

    func testSanitizeWifiModeAComputesMeansAndStrongestValues() {
        let raw = [
            RawWifiScanResult(ssid: nil, bssid: nil, rssi: -40, noise: -90),
            RawWifiScanResult(ssid: nil, bssid: nil, rssi: -60, noise: -80),
            RawWifiScanResult(ssid: nil, bssid: nil, rssi: -80, noise: -70),
        ]
        let sanitized = sanitizeWifiModeA(raw)
        XCTAssertEqual(sanitized.rssiMean, -60, accuracy: 0.001)
        XCTAssertEqual(sanitized.noiseMean, -80, accuracy: 0.001)
        XCTAssertEqual(sanitized.strongest1, -40)
        XCTAssertEqual(sanitized.strongest2, -60)
        XCTAssertEqual(sanitized.strongest3, -80)
    }

    func testSanitizeWifiModeAHandlesFewerThanThreeNetworks() {
        let raw = [RawWifiScanResult(ssid: nil, bssid: nil, rssi: -50, noise: -90)]
        let sanitized = sanitizeWifiModeA(raw)
        XCTAssertEqual(sanitized.strongest1, -50)
        XCTAssertEqual(sanitized.strongest2, Int.min)
        XCTAssertEqual(sanitized.strongest3, Int.min)
    }

    func testSanitizeWifiModeAHandlesNoNetworksWithoutDividingByZero() {
        let sanitized = sanitizeWifiModeA([])
        XCTAssertEqual(sanitized.visibleNetworkCount, 0)
        XCTAssertEqual(sanitized.rssiMean, 0)
        XCTAssertEqual(sanitized.noiseMean, 0)
    }
}
