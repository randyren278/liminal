import AVFoundation
import Foundation
import Vision

/// Converts a Vision framework pose observation into the pure `PoseObservation` type --
/// separated from `VisionCaptureCoordinator` so the mapping logic is unit-testable without a
/// camera. `VNHumanBodyPoseObservation` cannot be constructed in a test (Apple provides no public
/// initializer), so this takes the already-extracted joint list rather than the Vision type
/// itself -- `VisionCaptureCoordinator` is responsible for that extraction, which IS
/// camera-dependent and untestable.
public func poseObservation(fromRawJoints rawJoints: [(name: String, x: Double, y: Double, confidence: Double)])
    -> PoseObservation
{
    let joints = rawJoints.map { Joint(name: $0.name, x: $0.x, y: $0.y, confidence: $0.confidence) }
    let filtered = filterJoints(joints)
    let bodyCount: BodyCount = rawJoints.isEmpty ? .zero : .one
    return PoseObservation(bodyCount: bodyCount, joints: filtered)
}

/// §120 Vision Organ: capture, extract 2D pose, discard the frame. This class owns the
/// `AVCaptureSession` and never writes a frame to disk -- the only thing that leaves
/// `captureOutput` is a `PoseObservation`, already stripped of pixel data.
public final class VisionCaptureCoordinator: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    public typealias ObservationHandler = (PoseObservation) -> Void

    private let session = AVCaptureSession()
    private let queue = DispatchQueue(label: "liminal.vision-capture")
    private let onObservation: ObservationHandler

    public init(onObservation: @escaping ObservationHandler) {
        self.onObservation = onObservation
        super.init()
    }

    /// Configures the session against the default video device. Throws if no camera is
    /// available; does NOT request authorization -- callers must do that first (§90: explain
    /// before prompting) via `AVCaptureDevice.requestAccess(for:)`.
    public func configure() throws {
        guard let device = AVCaptureDevice.default(for: .video) else {
            throw VisionCaptureError.noCameraAvailable
        }
        let input = try AVCaptureDeviceInput(device: device)
        guard session.canAddInput(input) else { throw VisionCaptureError.cannotAddInput }
        session.addInput(input)

        let output = AVCaptureVideoDataOutput()
        output.setSampleBufferDelegate(self, queue: queue)
        guard session.canAddOutput(output) else { throw VisionCaptureError.cannotAddOutput }
        session.addOutput(output)
    }

    public func start() {
        session.startRunning()
    }

    public func stop() {
        session.stopRunning()
    }

    public func captureOutput(
        _: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer,
        from _: AVCaptureConnection,
    ) {
        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }

        let request = VNDetectHumanBodyPoseRequest()
        let handler = VNImageRequestHandler(cvPixelBuffer: pixelBuffer, options: [:])
        // `pixelBuffer` and every intermediate Vision buffer are released when this scope exits;
        // nothing here is written to disk.
        try? handler.perform([request])

        guard let observation = request.results?.first,
              let points = try? observation.recognizedPoints(.all)
        else {
            onObservation(PoseObservation(bodyCount: .zero, joints: []))
            return
        }

        let rawJoints = points.map { name, point in
            (
                name: name.rawValue.rawValue, x: Double(point.location.x), y: Double(point.location.y),
                confidence: Double(point.confidence),
            )
        }
        onObservation(poseObservation(fromRawJoints: rawJoints))
    }
}

public enum VisionCaptureError: Error {
    case noCameraAvailable
    case cannotAddInput
    case cannotAddOutput
}
