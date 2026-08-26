import AVFoundation
import Foundation
import LiminalCore

// `liminal-capture` -- the Vision + passive-acoustic organ capture daemon (ROADMAP items 2 and
// 5, master plan §120 Vision Organ, §26 Passive Acoustic Organ). Headless: no window, per the
// 2026-08-26 TUI-primary architecture pivot. Requests camera and microphone authorization
// explicitly, one at a time (§90: explain before prompting), extracts 2D body pose per camera
// frame and acoustic features per ~1s audio window, and emits each as a length-delimited
// `liminal-ipc` envelope to the Unix socket at `/tmp/liminal-$UID/core.sock` (§15). Falls back to
// stdout when nothing is listening there, so each organ is independently verifiable.
//
// Zero raw frames or raw continuous audio are ever written to disk -- only derived feature JSON
// leaves this process (§120, §27's "no MFCC persistence" boundary is enforced by AudioFeatures.swift
// simply never computing one).

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

var sequence: UInt64 = 0
let sequenceLock = NSLock()

/// Shared by both organs: builds and sends one envelope, falling back to stdout on any send
/// failure (including "never connected in the first place").
func sendFeatures(streamId: String, payload: Data) {
    sequenceLock.lock()
    sequence += 1
    let currentSequence = sequence
    sequenceLock.unlock()

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
    var granted = false
    AVCaptureDevice.requestAccess(for: mediaType) { result in
        granted = result
        semaphore.signal()
    }
    semaphore.wait()
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
    let visionCoordinator = VisionCaptureCoordinator { observation in
        guard let payload = try? encodePoseObservation(observation) else { return }
        sendFeatures(streamId: "camera", payload: payload)
    }
    do {
        try visionCoordinator.configure()
        visionCoordinator.start()
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
    let audioCoordinator = AudioCaptureCoordinator { features in
        guard let payload = try? JSONEncoder().encode(features) else { return }
        sendFeatures(streamId: "microphone", payload: payload)
    }
    do {
        try audioCoordinator.start()
        print("liminal-capture: passive acoustic organ running.")
    } catch {
        print("liminal-capture: failed to start audio engine: \(error)")
    }
} else {
    print("liminal-capture: microphone access denied. Passive acoustic organ will not run.")
}

print("liminal-capture: running. Press Ctrl+C to stop.")
RunLoop.main.run()
