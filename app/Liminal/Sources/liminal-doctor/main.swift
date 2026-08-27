import AVFoundation
import CoreBluetooth
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

private final class AcceptanceCounter {
    private let lock = NSLock()
    private var value = 0

    func increment() {
        lock.lock()
        value += 1
        lock.unlock()
    }

    func read() -> Int {
        lock.lock()
        defer { lock.unlock() }
        return value
    }
}

struct LiveAcceptanceMetric: Codable {
    let status: String
    let derivedSampleCount: Int
}

struct LiveAcceptanceReport: Codable {
    let durationSeconds: Double
    let camera: LiveAcceptanceMetric
    let microphone: LiveAcceptanceMetric
    let wifi: LiveAcceptanceMetric
    let bluetooth: LiveAcceptanceMetric
    let bluetoothDiscoveredPeripheralCount: Int
    let speakerOutput: LiveAcceptanceMetric
}

enum PseudonymKeyLoadOutcome {
    case key(Data)
    case failed
    case timedOut
}

func authorization(for mediaType: AVMediaType, explanation: String) -> Bool {
    let current = AVCaptureDevice.authorizationStatus(for: mediaType)
    if current == .authorized {
        return true
    }
    if current == .denied || current == .restricted {
        return false
    }
    print("liminal-doctor: requesting \(mediaType.rawValue) authorization...")
    print(explanation)
    let semaphore = DispatchSemaphore(value: 0)
    var granted = false
    AVCaptureDevice.requestAccess(for: mediaType) { result in
        granted = result
        semaphore.signal()
    }
    // The callback may be delivered on the main queue. Pump the run loop with a hard bound
    // instead of blocking it, or an OS-mediated request can hang acceptance forever.
    let deadline = Date().addingTimeInterval(30.0)
    while semaphore.wait(timeout: .now()) == .timedOut, Date() < deadline {
        RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.05))
    }
    return granted
}

func loadPseudonymKeyWithin(_ timeout: TimeInterval) -> PseudonymKeyLoadOutcome {
    let semaphore = DispatchSemaphore(value: 0)
    var outcome = PseudonymKeyLoadOutcome.failed
    DispatchQueue.global(qos: .utility).async {
        do {
            let key = try loadOrCreatePseudonymKey(allowInteraction: true)
            outcome = .key(key)
        } catch {
            outcome = .failed
        }
        semaphore.signal()
    }
    let deadline = Date().addingTimeInterval(timeout)
    while semaphore.wait(timeout: .now()) == .timedOut, Date() < deadline {
        RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.05))
    }
    return Date() >= deadline ? .timedOut : outcome
}

/// Runs an explicit, bounded hardware acceptance window. Only derived counts and statuses leave
/// this function; the coordinators discard their source media and pseudonymize BLE discoveries
/// before any callback reaches here. This is deliberately opt-in because it can trigger TCC
/// prompts, unlike the default Sensorium probe.
func runLiveAcceptance(durationSeconds: Double, profile: SensoriumProfile) -> LiveAcceptanceReport {
    let duration = max(1.0, min(durationSeconds, 60.0))
    let cameraCount = AcceptanceCounter()
    let microphoneCount = AcceptanceCounter()
    let wifiCount = AcceptanceCounter()
    let bluetoothCount = AcceptanceCounter()
    let bluetoothDiscoveredPeripheralCount = AcceptanceCounter()

    var cameraStatus = "not_started"
    var microphoneStatus = "not_started"
    var wifiStatus = "failed"
    var bluetoothStatus = "not_started"

    var visionCoordinator: VisionCaptureCoordinator?
    if authorization(
        for: .video,
        explanation: "Liminal will process camera frames in memory for body-pose features only. No video is saved or returned by this acceptance report.",
    ) {
        let coordinator = VisionCaptureCoordinator { _ in cameraCount.increment() }
        do {
            try coordinator.configure()
            coordinator.start()
            visionCoordinator = coordinator
            cameraStatus = "running"
        } catch {
            cameraStatus = "failed"
        }
    } else {
        cameraStatus = "denied"
    }

    var audioCoordinator: AudioCaptureCoordinator?
    if authorization(
        for: .audio,
        explanation: "Liminal will reduce microphone input to derived acoustic features in memory. No audio is recorded, transcribed, or returned by this acceptance report.",
    ) {
        let coordinator = AudioCaptureCoordinator { _ in microphoneCount.increment() }
        do {
            try coordinator.start()
            audioCoordinator = coordinator
            microphoneStatus = "running"
        } catch {
            microphoneStatus = "failed"
        }
    } else {
        microphoneStatus = "denied"
    }

    let wifiCoordinator = WifiScanCoordinator { _ in wifiCount.increment() }
    wifiCoordinator.start(scanIntervalSeconds: 1.0)
    wifiStatus = "running"

    var bluetoothCoordinator: BluetoothScanCoordinator?
    switch CBCentralManager.authorization {
    case .allowedAlways, .notDetermined:
        let keyOutcome = loadPseudonymKeyWithin(2.0)
        switch keyOutcome {
        case let .key(key):
            let coordinator = BluetoothScanCoordinator(
                pseudonymKey: key,
                onFeatures: { window in
                    if !window.clusters.isEmpty {
                        bluetoothCount.increment()
                    }
                },
                // Count discovery callbacks separately so a zero-feature result is
                // distinguishable from an empty radio. The callback never receives or retains
                // the identifier.
                onDiscovery: { bluetoothDiscoveredPeripheralCount.increment() },
            )
            coordinator.start(windowDurationSeconds: 1.0)
            bluetoothCoordinator = coordinator
            bluetoothStatus = "running"
        case .timedOut, .failed:
            let diagnosticCoordinator = BluetoothScanCoordinator {
                bluetoothDiscoveredPeripheralCount.increment()
            }
            diagnosticCoordinator.start(windowDurationSeconds: 1.0)
            bluetoothCoordinator = diagnosticCoordinator
            switch keyOutcome {
            case .timedOut:
                bluetoothStatus = "keychain_timeout"
            case .failed:
                bluetoothStatus = "keychain_interaction_required"
            case .key:
                bluetoothStatus = "keychain_interaction_required"
            }
        }
    case .denied, .restricted:
        bluetoothStatus = "denied"
    @unknown default:
        bluetoothStatus = "unknown"
    }

    RunLoop.current.run(until: Date().addingTimeInterval(duration))

    visionCoordinator?.stop()
    audioCoordinator?.stop()
    wifiCoordinator.stop()
    bluetoothCoordinator?.stop()

    func finished(_ status: String, _ count: Int) -> LiveAcceptanceMetric {
        LiveAcceptanceMetric(status: count > 0 ? "observed" : status, derivedSampleCount: count)
    }

    let bluetoothSamples = bluetoothCount.read()
    let bluetoothMetric = LiveAcceptanceMetric(
        status: bluetoothAcceptanceStatus(
            startupStatus: bluetoothStatus,
            discoveredSampleCount: bluetoothSamples,
            discoveredPeripheralCount: bluetoothDiscoveredPeripheralCount.read(),
        ),
        derivedSampleCount: bluetoothSamples,
    )

    return LiveAcceptanceReport(
        durationSeconds: duration,
        camera: finished(cameraStatus, cameraCount.read()),
        microphone: finished(microphoneStatus, microphoneCount.read()),
        wifi: finished(wifiStatus, wifiCount.read()),
        bluetooth: bluetoothMetric,
        bluetoothDiscoveredPeripheralCount: bluetoothDiscoveredPeripheralCount.read(),
        speakerOutput: LiveAcceptanceMetric(
            status: profile.audioOutput.state.rawValue == SensorState.available.rawValue ? "available" : "unavailable",
            derivedSampleCount: 0,
        ),
    )
}

func printLiveAcceptance(_ report: LiveAcceptanceReport, json: Bool) {
    if json {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try! encoder.encode(report)
        print(String(data: data, encoding: .utf8)!)
        return
    }
    print("LIMINAL --- LIVE SENSOR ACCEPTANCE")
    print("")
    for (name, metric) in [
        ("Camera", report.camera),
        ("Microphone", report.microphone),
        ("Wi-Fi", report.wifi),
        ("Bluetooth", report.bluetooth),
        ("Speaker output", report.speakerOutput),
    ] {
        print("\(name): \(metric.status) (derived samples: \(metric.derivedSampleCount))")
    }
    print("window: \(report.durationSeconds)s")
    print("raw media and identifiers: never persisted or returned")
}

let args = CommandLine.arguments
let profile = buildProfile()

if args.contains("--live") {
    let duration = args.dropFirst().first(where: { $0.hasPrefix("--duration=") })
        .flatMap { Double($0.split(separator: "=", maxSplits: 1).last ?? "") } ?? 5.0
    printLiveAcceptance(runLiveAcceptance(durationSeconds: duration, profile: profile), json: args.contains("--json"))
} else if args.contains("--json") {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    let data = try! encoder.encode(profile)
    print(String(data: data, encoding: .utf8)!)
} else {
    printHumanReadable(profile)
}
