import Darwin
@testable import LiminalCore
import XCTest

final class StreamSequenceAllocatorTests: XCTestCase {
    func testSequencesAdvanceIndependentlyForInterleavedStreams() throws {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("liminal-sequences-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: url) }
        let allocator = try StreamSequenceAllocator(storageURL: url)

        XCTAssertEqual(try allocator.next(for: "camera"), 1)
        XCTAssertEqual(try allocator.next(for: "microphone"), 1)
        XCTAssertEqual(try allocator.next(for: "camera"), 2)
        XCTAssertEqual(try allocator.next(for: "microphone"), 2)
    }

    func testSequencesResumeFromDurableStateAfterRestart() throws {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("liminal-sequences-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: url) }

        let first = try StreamSequenceAllocator(storageURL: url)
        XCTAssertEqual(try first.next(for: "camera"), 1)
        XCTAssertEqual(try first.next(for: "camera"), 2)

        let restarted = try StreamSequenceAllocator(storageURL: url)
        XCTAssertEqual(try restarted.next(for: "camera"), 3)
    }
}

/// Exercises `UnixSocketClient` against a REAL Unix domain socket listener (raw POSIX
/// bind/listen/accept in-process) -- this is genuinely testable in CI without any hardware or
/// permission, unlike the camera/audio/radio probes.
final class UnixSocketClientTests: XCTestCase {
    private func tempSocketPath() -> String {
        let unique = UUID().uuidString
        return FileManager.default.temporaryDirectory
            .appendingPathComponent("liminal-test-\(unique).sock").path
    }

    private func startListener(at path: String) -> Int32 {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(path.utf8)
        withUnsafeMutableBytes(of: &addr.sun_path) { rawPtr in
            let buffer = rawPtr.bindMemory(to: CChar.self)
            for (i, byte) in pathBytes.enumerated() {
                buffer[i] = CChar(bitPattern: byte)
            }
            buffer[pathBytes.count] = 0
        }
        let bindResult = withUnsafePointer(to: &addr) { ptr -> Int32 in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockaddrPtr in
                Darwin.bind(fd, sockaddrPtr, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        precondition(bindResult == 0, "test listener bind failed: \(String(cString: strerror(errno)))")
        precondition(listen(fd, 1) == 0, "test listener listen failed")
        return fd
    }

    func testConnectsAndWritesBytesThatTheListenerActuallyReceives() throws {
        let path = tempSocketPath()
        defer { unlink(path) }
        let listenerFd = startListener(at: path)
        defer { close(listenerFd) }

        let received = expectation(description: "listener received the frame")
        var receivedData = Data()
        let listenerQueue = DispatchQueue(label: "test-listener")
        listenerQueue.async {
            var addr = sockaddr()
            var len = socklen_t(MemoryLayout<sockaddr>.size)
            let clientFd = accept(listenerFd, &addr, &len)
            guard clientFd >= 0 else { return }
            var buffer = [UInt8](repeating: 0, count: 1024)
            let n = read(clientFd, &buffer, buffer.count)
            if n > 0 {
                receivedData = Data(buffer[0 ..< n])
            }
            close(clientFd)
            received.fulfill()
        }

        // Give the listener a moment to reach accept() -- SOCK_STREAM listen() backlog means the
        // connect() below would succeed even before accept() runs, but we still want accept()
        // scheduled first for a deterministic test.
        Thread.sleep(forTimeInterval: 0.05)

        let client = try UnixSocketClient(path: path)
        let envelope = makeEnvelope(
            messageId: "test", sensorStreamId: "camera", monotonicSequence: 1, capturedAtUtcUs: 0,
            capturedAtMonoUs: 0, payload: Data([1, 2, 3]),
        )
        let frame = try lengthDelimitedFrame(envelope)
        try client.write(frame)

        wait(for: [received], timeout: 2.0)

        let (decoded, consumed) = try readLengthDelimitedEnvelope(from: receivedData)
        XCTAssertEqual(consumed, frame.count)
        XCTAssertEqual(decoded.messageID, "test")
        XCTAssertEqual(decoded.payload, Data([1, 2, 3]))
    }

    func testConnectingToANonexistentSocketThrowsRatherThanCrashing() {
        let path = tempSocketPath() // never created
        XCTAssertThrowsError(try UnixSocketClient(path: path)) { error in
            guard case UnixSocketError.connectFailed = error else {
                XCTFail("expected connectFailed, got \(error)")
                return
            }
        }
    }

    func testAPathLongerThanSunPathThrowsPathTooLong() {
        let longPath = "/tmp/" + String(repeating: "a", count: 200) + ".sock"
        XCTAssertThrowsError(try UnixSocketClient(path: longPath)) { error in
            XCTAssertEqual(error as? UnixSocketError, .pathTooLong)
        }
    }
}
