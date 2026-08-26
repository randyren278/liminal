import Foundation

// Vision organ feature types -- master plan §24 (Vision Features). Pure data + pure functions
// only; the actual `AVCaptureSession`/`VNDetectHumanBodyPoseRequest` wiring lives in
// `liminal-capture` and cannot be unit-tested without a real camera, but everything here can be.

/// §608-615: body count is one of these four states, never a raw guess.
public enum BodyCount: String, Codable, Equatable {
    case zero
    case one
    case twoOrMore = "two_or_more"
    case unknown
}

/// §618-628: normalized joint position and confidence.
public struct Joint: Codable, Equatable {
    public let name: String
    public let x: Double
    public let y: Double
    public let confidence: Double

    public init(name: String, x: Double, y: Double, confidence: Double) {
        self.name = name
        self.x = x
        self.y = y
        self.confidence = confidence
    }
}

/// §24: one frame's worth of derived Vision features. Never carries pixel data -- constructing
/// one already implies the raw frame has been discarded.
public struct PoseObservation: Codable, Equatable {
    public let bodyCount: BodyCount
    public let joints: [Joint]

    public init(bodyCount: BodyCount, joints: [Joint]) {
        self.bodyCount = bodyCount
        self.joints = joints
    }

    enum CodingKeys: String, CodingKey {
        case bodyCount = "body_count"
        case joints
    }
}

/// §628: "Initial joint confidence floor: 0.25, configurable and experimentally validated."
public let defaultJointConfidenceFloor: Double = 0.25

/// Drop joints below the confidence floor -- low-confidence joints are noise, not evidence.
public func filterJoints(_ joints: [Joint], floor: Double = defaultJointConfidenceFloor) -> [Joint] {
    joints.filter { $0.confidence >= floor }
}

/// Encode a `PoseObservation` as the JSON bytes that go into an IPC envelope's `payload` field.
public func encodePoseObservation(_ observation: PoseObservation) throws -> Data {
    let encoder = JSONEncoder()
    return try encoder.encode(observation)
}
