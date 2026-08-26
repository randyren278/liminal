@testable import LiminalCore
import XCTest

final class CameraFeaturesTests: XCTestCase {
    func testFilterJointsDropsJointsBelowTheConfidenceFloor() {
        let joints = [
            Joint(name: "nose", x: 0.5, y: 0.5, confidence: 0.9),
            Joint(name: "leftWrist", x: 0.1, y: 0.2, confidence: 0.1),
            Joint(name: "rightWrist", x: 0.8, y: 0.2, confidence: 0.25),
        ]
        let filtered = filterJoints(joints)
        XCTAssertEqual(filtered.map(\.name), ["nose", "rightWrist"])
    }

    func testFilterJointsRespectsACustomFloor() {
        let joints = [Joint(name: "nose", x: 0.5, y: 0.5, confidence: 0.5)]
        XCTAssertEqual(filterJoints(joints, floor: 0.6).count, 0)
        XCTAssertEqual(filterJoints(joints, floor: 0.4).count, 1)
    }

    func testEncodePoseObservationProducesTheExpectedJsonShape() throws {
        let observation = PoseObservation(
            bodyCount: .one,
            joints: [Joint(name: "nose", x: 0.5, y: 0.6, confidence: 0.9)],
        )
        let data = try encodePoseObservation(observation)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])

        XCTAssertEqual(json["body_count"] as? String, "one")
        let joints = try XCTUnwrap(json["joints"] as? [[String: Any]])
        XCTAssertEqual(joints.count, 1)
        XCTAssertEqual(joints[0]["name"] as? String, "nose")
        XCTAssertEqual(joints[0]["confidence"] as? Double, 0.9)
    }

    func testBodyCountRoundTripsThroughJsonForEveryVariant() throws {
        for count in [BodyCount.zero, .one, .twoOrMore, .unknown] {
            let observation = PoseObservation(bodyCount: count, joints: [])
            let data = try encodePoseObservation(observation)
            let decoded = try JSONDecoder().decode(PoseObservation.self, from: data)
            XCTAssertEqual(decoded.bodyCount, count)
        }
    }
}
