import Foundation
@testable import LiminalCore
import XCTest

final class CoalescingFeatureBufferTests: XCTestCase {
    func testBatchContainsEveryPendingStreamWithoutStarvation() {
        let buffer = CoalescingFeatureBuffer()
        XCTAssertTrue(buffer.submit(feature("camera", value: 1, timestamp: 10)))
        XCTAssertFalse(buffer.submit(feature("microphone", value: 2, timestamp: 20)))
        XCTAssertFalse(buffer.submit(feature("wifi", value: 3, timestamp: 30)))

        let firstBatch = buffer.takeBatch()
        XCTAssertEqual(firstBatch?.map(\.streamId), ["camera", "microphone", "wifi"])

        // Camera can refill the next batch while the first batch drains, but it
        // cannot displace microphone or Wi-Fi from the already-taken snapshot.
        XCTAssertFalse(buffer.submit(feature("camera", value: 4, timestamp: 40)))
        XCTAssertFalse(buffer.submit(feature("camera", value: 5, timestamp: 50)))
        XCTAssertEqual(firstBatch?.map(\.capturedAtUtcUs), [10, 20, 30])
        XCTAssertEqual(buffer.takeBatch(), [feature("camera", value: 5, timestamp: 50)])
        XCTAssertNil(buffer.takeBatch())
    }

    func testIdleTransitionSchedulesExactlyOneNewDrain() {
        let buffer = CoalescingFeatureBuffer()
        XCTAssertTrue(buffer.submit(feature("camera", value: 1, timestamp: 10)))
        XCTAssertNotNil(buffer.takeBatch())
        XCTAssertNil(buffer.takeBatch())
        XCTAssertTrue(buffer.submit(feature("wifi", value: 2, timestamp: 20)))
    }

    private func feature(_ streamId: String, value: UInt8, timestamp: Int64) -> PendingFeature {
        PendingFeature(
            streamId: streamId,
            payload: Data([value]),
            capturedAtUtcUs: timestamp,
            capturedAtMonoUs: timestamp + 1,
        )
    }
}
