@testable import LiminalCore
import XCTest

/// Mirrors the round-trip test in `crates/liminal-schema/src/sensorium.rs` -- the two
/// `SensoriumProfile` types (Swift and Rust) must agree on JSON shape, since that shape is the
/// eventual IPC contract even though nothing wires them together yet.
final class SensoriumProfileTests: XCTestCase {
    func testEncodesSection22ShapeWithSnakeCaseKeysAndArrayResolution() throws {
        let profile = SensoriumProfile(
            schemaVersion: 1,
            machineProfileId: "machine:test",
            createdAt: "2026-01-01T00:00:00Z",
            camera: CameraProfile(
                state: .available,
                deviceIdHash: "camera:test",
                selectedResolution: Resolution(1280, 720),
                selectedFps: 10,
                depthData: false,
            ),
            audioInput: AudioProfile(state: .available, sampleRate: 48000, channels: 1),
            audioOutput: AudioProfile(state: .available, sampleRate: 48000, channels: 2),
            wifi: WifiProfile(
                state: .available, aggregateRssi: true, aggregateNoise: true, scanning: true,
                stableApIds: false, csi: false,
            ),
            bluetooth: BluetoothProfile(state: .available, scanRssi: true),
        )

        let data = try JSONEncoder().encode(profile)
        let json = try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [String: Any])

        XCTAssertEqual(json["schema_version"] as? Int, 1)
        XCTAssertEqual(json["machine_profile_id"] as? String, "machine:test")

        let camera = try XCTUnwrap(json["camera"] as? [String: Any])
        XCTAssertEqual(camera["state"] as? String, "available")
        XCTAssertEqual(camera["device_id_hash"] as? String, "camera:test")
        XCTAssertEqual(camera["selected_resolution"] as? [Int], [1280, 720])
        XCTAssertEqual(camera["selected_fps"] as? Int, 10)
        XCTAssertEqual(camera["depth_data"] as? Bool, false)

        let wifi = try XCTUnwrap(json["wifi"] as? [String: Any])
        XCTAssertEqual(wifi["aggregate_rssi"] as? Bool, true)
        XCTAssertEqual(wifi["stable_ap_ids"] as? Bool, false)

        let bluetooth = try XCTUnwrap(json["bluetooth"] as? [String: Any])
        XCTAssertEqual(bluetooth["scan_rssi"] as? Bool, true)
    }

    func testDecodesTheLiteralSection22ExampleFromTheMasterPlan() throws {
        // Copied verbatim from LIMINAL_MASTER_PLAN.md §22, to independently confirm the Swift
        // type can parse the exact JSON shape the Rust side's own test decodes.
        let json = """
        {
          "schema_version": 1,
          "machine_profile_id": "machine:...",
          "created_at": "...",
          "camera": {
            "state": "available",
            "device_id_hash": "...",
            "selected_resolution": [1280, 720],
            "selected_fps": 10,
            "depth_data": false
          },
          "audio_input": {
            "state": "available",
            "sample_rate": 48000,
            "channels": 1
          },
          "audio_output": {
            "state": "available",
            "sample_rate": 48000,
            "channels": 2
          },
          "wifi": {
            "state": "available",
            "aggregate_rssi": true,
            "aggregate_noise": true,
            "scanning": true,
            "stable_ap_ids": false,
            "csi": false
          },
          "bluetooth": {
            "state": "available",
            "scan_rssi": true
          }
        }
        """
        let profile = try JSONDecoder().decode(SensoriumProfile.self, from: Data(json.utf8))

        XCTAssertEqual(profile.camera.selectedResolution, Resolution(1280, 720))
        XCTAssertEqual(profile.camera.selectedFps, 10)
        XCTAssertEqual(profile.audioInput.sampleRate, 48000)
        XCTAssertEqual(profile.wifi.stableApIds, false)
        XCTAssertEqual(profile.bluetooth.scanRssi, true)
    }

    func testEverySensorStateVariantSerializesToItsSnakeCaseString() throws {
        let cases: [(SensorState, String)] = [
            (.unknown, "unknown"),
            (.probing, "probing"),
            (.available, "available"),
            (.degraded, "degraded"),
            (.denied, "denied"),
            (.busy, "busy"),
            (.unsupported, "unsupported"),
            (.failed, "failed"),
            (.disabledByUser, "disabled_by_user"),
        ]
        for (state, expected) in cases {
            let data = try JSONEncoder().encode(state)
            let decoded = try XCTUnwrap(String(data: data, encoding: .utf8))
            XCTAssertEqual(decoded, "\"\(expected)\"", "state \(state) should encode as \(expected)")
        }
    }
}
