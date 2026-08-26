@testable import LiminalCore
import XCTest

final class VisionCaptureCoordinatorTests: XCTestCase {
    func testEmptyRawJointsProducesZeroBodyCount() {
        let observation = poseObservation(fromRawJoints: [])
        XCTAssertEqual(observation.bodyCount, .zero)
        XCTAssertTrue(observation.joints.isEmpty)
    }

    func testNonEmptyRawJointsProducesOneBodyCount() {
        let observation = poseObservation(fromRawJoints: [
            (name: "nose", x: 0.5, y: 0.5, confidence: 0.9),
        ])
        XCTAssertEqual(observation.bodyCount, .one)
    }

    func testLowConfidenceJointsAreFilteredOutOfTheFinalObservation() {
        let observation = poseObservation(fromRawJoints: [
            (name: "nose", x: 0.5, y: 0.5, confidence: 0.9),
            (name: "leftEye", x: 0.4, y: 0.4, confidence: 0.05),
        ])
        XCTAssertEqual(observation.joints.map(\.name), ["nose"])
    }
}
