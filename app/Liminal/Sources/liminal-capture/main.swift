import AVFoundation
import CoreBluetooth
import Foundation
import LiminalCore

// `liminal-capture` -- the sensor organ capture daemon, now covering all four ROADMAP items 2,
// 5, and 6 (master plan §120 Vision Organ, §26 Passive Acoustic Organ, §34 Wi-Fi Organ, §38
// Bluetooth Organ). Headless: no window, per the 2026-08-26 TUI-primary architecture pivot.
// Requests camera and microphone authorization explicitly, one at a time (§90: explain before
// prompting); Wi-Fi Mode A scanning needs no permission (confirmed by `liminal-doctor`'s earlier
// probing -- aggregate RSSI/noise doesn't require Location); Bluetooth scanning checks
// `CBCentralManager.authorization` and explains before the OS prompts. Emits each organ's
// features as a length-delimited `liminal-ipc` envelope to the Unix socket at
// `/tmp/liminal-$UID/core.sock` (§15), falling back to stdout when nothing is listening.
//
// Zero raw frames or raw continuous audio are ever written to disk; no SSID/BSSID or Bluetooth
// device name is ever computed by this process, let alone sent anywhere (§120, §27's "no MFCC"
// boundary, §37's Mode A structural guarantee, §39's HMAC-only Bluetooth identifiers).

let socketPath = "/tmp/liminal-\(getuid())/core.sock"

var socketClient: UnixSocketClient?
do {
    socketClient = try UnixSocketClient(path: socketPath)
    print("liminal-capture: connected to \(socketPath)")
} catch {
    print(
        "liminal-capture: no listener at \(socketPath) (\(error)) -- printing observations to stdout instead.",
    )
    socketClient = nil
}

let sequenceAllocator: StreamSequenceAllocator
do {
    sequenceAllocator = try StreamSequenceAllocator()
} catch {
    fputs("liminal-capture: cannot load durable stream sequence state: \(error)\n", stderr)
    exit(EXIT_FAILURE)
}

// Keep every live organ strongly referenced for the lifetime of the daemon. The capture and
// audio coordinators own the delegate/tap callbacks, while the Bluetooth coordinator owns the
// CBCentralManager delegate. Creating them only inside the authorization blocks would make the
// process report "running" and then silently stop delivering callbacks when those scopes ended.
var visionCoordinator: VisionCaptureCoordinator?
var audioCoordinator: AudioCaptureCoordinator?
var bluetoothCoordinator: BluetoothScanCoordinator?

/// Shared by both organs: builds and sends one envelope, falling back to stdout on any send
/// failure (including "never connected in the first place").
func sendFeatures(streamId: String, payload: Data) {
    let currentSequence: UInt64
    do {
        currentSequence = try sequenceAllocator.next(for: streamId)
    } catch {
        print("liminal-capture: cannot persist \(streamId) sequence; dropping observation: \(error)")
        return
    }

    let nowUtcUs = Int64(Date().timeIntervalSince1970 * 1_000_000)
    let nowMonoUs = Int64(DispatchTime.now().uptimeNanoseconds / 1000)
    let envelope = makeEnvelope(
        messageId: UUID().uuidString,
        sensorStreamId: streamId,
        monotonicSequence: currentSequence,
        capturedAtUtcUs: nowUtcUs,
        capturedAtMonoUs: nowMonoUs,
        payload: payload,
    )

    if let client = socketClient {
        do {
            try client.write(lengthDelimitedFrame(envelope))
            return
        } catch {
            print("liminal-capture: write failed (\(error)), switching to stdout fallback.")
            socketClient = nil
        }
    }
    let payloadString = String(data: payload, encoding: .utf8) ?? "<invalid utf8>"
    print("[\(currentSequence)] \(streamId): \(payloadString)")
}

func requestAuthorization(for mediaType: AVMediaType, explanation: String) -> Bool {
    print("liminal-capture: requesting \(mediaType.rawValue) authorization...")
    print(explanation)
    let semaphore = DispatchSemaphore(value: 0)
    let resultLock = NSLock()
    var granted = false
    var completed = false
    AVCaptureDevice.requestAccess(for: mediaType) { result in
        resultLock.lock()
        granted = result
        completed = true
        resultLock.unlock()
        semaphore.signal()
    }
    let deadline = Date().addingTimeInterval(30.0)
    while semaphore.wait(timeout: .now()) == .timedOut, Date() < deadline {
        RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.05))
    }
    resultLock.lock()
    defer { resultLock.unlock() }
    guard completed else {
        print("liminal-capture: (mediaType.rawValue) authorization timed out after 30s.")
        return false
    }
    return granted
}

// MARK: - Vision organ (camera + 2D pose)

if requestAuthorization(
    for: .video,
    explanation: "Liminal wants to access the camera to extract body pose (joint positions + "
        + "confidence) for occupancy/motion sensing. Video frames are processed in memory and "
        + "never saved to disk.",
) {
    print("liminal-capture: camera access granted.")
    let coordinator = VisionCaptureCoordinator { observation in
        guard let payload = try? encodePoseObservation(observation) else { return }
        sendFeatures(streamId: "camera", payload: payload)
    }
    visionCoordinator = coordinator
    do {
        try coordinator.configure()
        coordinator.start()
        print("liminal-capture: Vision organ running.")
    } catch {
        print("liminal-capture: failed to configure camera session: \(error)")
    }
} else {
    print("liminal-capture: camera access denied. Vision organ will not run.")
}

// MARK: - Passive acoustic organ (microphone)

if requestAuthorization(
    for: .audio,
    explanation: "Liminal wants to access the microphone to extract acoustic features (energy, "
        + "spectral shape, voice-activity likelihood) for environmental sensing. No audio is "
        + "recorded, transcribed, or saved to disk -- only derived numbers leave this process.",
) {
    print("liminal-capture: microphone access granted.")
    let coordinator = AudioCaptureCoordinator { features in
        guard let payload = try? JSONEncoder().encode(features) else { return }
        sendFeatures(streamId: "microphone", payload: payload)
    }
    audioCoordinator = coordinator
    do {
        try coordinator.start()
        print("liminal-capture: passive acoustic organ running.")
    } catch {
        print("liminal-capture: failed to start audio engine: \(error)")
    }
} else {
    print("liminal-capture: microphone access denied. Passive acoustic organ will not run.")
}

// MARK: - Wi-Fi organ (Mode A, no permission required)

let wifiCoordinator = WifiScanCoordinator { features in
    guard let payload = try? JSONEncoder().encode(features) else { return }
    sendFeatures(streamId: "wifi", payload: payload)
}

wifiCoordinator.start()
print("liminal-capture: Wi-Fi organ running (Mode A, no permission required).")

// MARK: - Bluetooth organ

let bluetoothAuthorization = CBCentralManager.authorization
switch bluetoothAuthorization {
case .allowedAlways:
    print("liminal-capture: Bluetooth already authorized.")
    startBluetoothOrgan()
case .notDetermined:
    print("liminal-capture: requesting bluetooth authorization...")
    print(
        "Liminal wants to use Bluetooth to detect recurring proximity clusters (pseudonymized, "
            + "never a device name) for environmental sensing. No device identity is ever stored.",
    )
    // CBCentralManager's own init triggers the OS authorization prompt on first use; the
    // subsequent `centralManagerDidUpdateState` callback inside `BluetoothScanCoordinator` is
    // what actually starts scanning once (if) the user grants it.
    startBluetoothOrgan()
default:
    print("liminal-capture: Bluetooth access denied or restricted. Bluetooth organ will not run.")
}

func startBluetoothOrgan() {
    let pseudonymKey: Data
    do {
        pseudonymKey = try loadOrCreatePseudonymKey()
    } catch let error as PseudonymKeyStoreError {
        print(
            "liminal-capture: Bluetooth pseudonym key unavailable (\(error)); "
                + "Bluetooth organ will not run.",
        )
        return
    } catch {
        print(
            "liminal-capture: Bluetooth pseudonym key unavailable (unknown_error); "
                + "Bluetooth organ will not run.",
        )
        return
    }
    let coordinator = BluetoothScanCoordinator(pseudonymKey: pseudonymKey) { window in
        guard let payload = try? JSONEncoder().encode(window) else { return }
        sendFeatures(streamId: "bluetooth", payload: payload)
    }
    bluetoothCoordinator = coordinator
    coordinator.start()
    print("liminal-capture: Bluetooth organ running.")
}

print("liminal-capture: running. Press Ctrl+C to stop.")
RunLoop.main.run()
