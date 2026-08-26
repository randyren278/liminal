import Foundation
import LiminalCore

// `liminal doctor` / `liminal doctor --json` -- master plan §21 (Sensorium Discovery, the first
// executable milestone) and §117 ("`liminal doctor --json` must report actual hardware/
// permissions, not mocks"). This probe enumerates real device capabilities and current TCC
// authorization state; it never starts a capture session, taps audio, or scans Wi-Fi/BLE, so it
// requests no permission prompts of its own.

func buildProfile() -> SensoriumProfile {
    let formatter = ISO8601DateFormatter()
    return SensoriumProfile(
        schemaVersion: 1,
        machineProfileId: machineProfileId(),
        createdAt: formatter.string(from: Date()),
        camera: probeCamera(),
        audioInput: probeAudioInput(),
        audioOutput: probeAudioOutput(),
        wifi: probeWifi(),
        bluetooth: probeBluetooth(),
    )
}

func printHumanReadable(_ profile: SensoriumProfile) {
    func bar(_ state: SensorState) -> String {
        switch state {
        case .available: "AVAILABLE"
        case .probing: "PROBING (permission not yet requested)"
        case .denied: "DENIED"
        case .degraded: "DEGRADED"
        case .busy: "BUSY"
        case .unsupported: "UNSUPPORTED"
        case .failed: "FAILED"
        case .disabledByUser: "DISABLED_BY_USER"
        case .unknown: "UNKNOWN"
        }
    }

    print("LIMINAL --- DISCOVERING SENSORIUM")
    print("")
    print("Camera")
    print("  \(bar(profile.camera.state))")
    print(
        "  resolution=\(profile.camera.selectedResolution.width)x\(profile.camera.selectedResolution.height) "
            + "fps=\(profile.camera.selectedFps) depth=\(profile.camera.depthData)",
    )
    print("")
    print("Microphone")
    print("  \(bar(profile.audioInput.state))")
    print(
        "  sample_rate=\(profile.audioInput.sampleRate) channels=\(profile.audioInput.channels)",
    )
    print("")
    print("Speaker Output")
    print("  \(bar(profile.audioOutput.state))")
    print(
        "  sample_rate=\(profile.audioOutput.sampleRate) channels=\(profile.audioOutput.channels)",
    )
    print("")
    print("Wi-Fi RSSI/Noise")
    print("  \(bar(profile.wifi.state))")
    print("")
    print("Wi-Fi Stable AP IDs")
    print(
        "  \(profile.wifi.stableApIds ? "AVAILABLE" : "NEEDS OPTIONAL LOCATION PERMISSION (not requested)")",
    )
    print("")
    print("Wi-Fi CSI")
    print("  UNSUPPORTED_BY_DESIGN")
    print("")
    print("Bluetooth")
    print("  \(bar(profile.bluetooth.state))")
    print("")
    print("machine_profile_id: \(profile.machineProfileId)")
}

let args = CommandLine.arguments
let profile = buildProfile()

if args.contains("--json") {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    let data = try! encoder.encode(profile)
    print(String(data: data, encoding: .utf8)!)
} else {
    printHumanReadable(profile)
}
