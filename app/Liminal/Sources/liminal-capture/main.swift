import AVFoundation
import Foundation
import LiminalCore

// `liminal-capture` -- the Vision organ capture daemon (ROADMAP item 2, master plan §120).
// Headless: no window, per the 2026-08-26 TUI-primary architecture pivot. Requests camera
// authorization explicitly (§90: explain before prompting), extracts 2D body pose per frame via
// `VisionCaptureCoordinator`, and emits each observation as a length-delimited `liminal-ipc`
// envelope to the Unix socket at `/tmp/liminal-$UID/core.sock` (§15). If nothing is listening
// there yet (`liminald` doesn't exist as of this commit -- ROADMAP item 3), falls back to
// printing each observation to stdout so this organ is independently verifiable before the
// receiving end exists.
//
// Zero raw frames are ever written to disk -- only `PoseObservation` JSON leaves this process.

let socketPath = "/tmp/liminal-\(getuid())/core.sock"

print("liminal-capture: requesting camera authorization...")
print(
    "Liminal wants to access the camera to extract body pose (joint positions + confidence) for "
        + "occupancy/motion sensing. Video frames are processed in memory and never saved to disk.",
)

let semaphore = DispatchSemaphore(value: 0)
var authorized = false
AVCaptureDevice.requestAccess(for: .video) { granted in
    authorized = granted
    semaphore.signal()
}

semaphore.wait()

guard authorized else {
    print("liminal-capture: camera access denied. Vision organ cannot run. Exiting.")
    exit(1)
}

print("liminal-capture: camera access granted.")

var socketClient: UnixSocketClient?
do {
    socketClient = try UnixSocketClient(path: socketPath)
    print("liminal-capture: connected to \(socketPath)")
} catch {
    print("liminal-capture: no listener at \(socketPath) (\(error)) -- printing observations to stdout instead.")
    socketClient = nil
}

var sequence: UInt64 = 0

let coordinator = VisionCaptureCoordinator { observation in
    sequence += 1
    let nowUtcUs = Int64(Date().timeIntervalSince1970 * 1_000_000)
    let nowMonoUs = Int64(DispatchTime.now().uptimeNanoseconds / 1000)

    guard let payload = try? encodePoseObservation(observation) else { return }
    let envelope = makeEnvelope(
        messageId: UUID().uuidString,
        sensorStreamId: "camera",
        monotonicSequence: sequence,
        capturedAtUtcUs: nowUtcUs,
        capturedAtMonoUs: nowMonoUs,
        payload: payload,
    )

    if let client = socketClient {
        do {
            try client.write(lengthDelimitedFrame(envelope))
        } catch {
            print("liminal-capture: write failed (\(error)), switching to stdout fallback.")
            socketClient = nil
        }
    } else {
        let payloadString = String(data: payload, encoding: .utf8) ?? "<invalid utf8>"
        print("[\(sequence)] camera: \(payloadString)")
    }
}

do {
    try coordinator.configure()
} catch {
    print("liminal-capture: failed to configure camera session: \(error)")
    exit(1)
}

coordinator.start()
print("liminal-capture: running. Press Ctrl+C to stop.")
RunLoop.main.run()
