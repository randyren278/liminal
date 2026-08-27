import Foundation

/// Allocates monotonic IPC sequence numbers independently for each sensor stream. Envelopes are
/// interleaved on the shared socket, but §15's sequence is a per-stream ordering field; using one
/// global counter would make every other stream look like a dropped-message gap to `liminald`.
public final class StreamSequenceAllocator {
    private let lock = NSLock()
    private let storageURL: URL
    private var nextByStream: [String: UInt64] = [:]

    public init(storageURL: URL? = nil) throws {
        if let storageURL {
            self.storageURL = storageURL
        } else {
            let applicationSupport = FileManager.default.urls(
                for: .applicationSupportDirectory, in: .userDomainMask,
            )[0]
            self.storageURL = applicationSupport
                .appendingPathComponent("Liminal", isDirectory: true)
                .appendingPathComponent("sequence-state.json")
        }
        if let data = FileManager.default.contents(atPath: self.storageURL.path) {
            nextByStream = try JSONDecoder().decode([String: UInt64].self, from: data)
        }
    }

    public func next(for streamId: String) throws -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        let next = nextByStream[streamId, default: 0] + 1
        nextByStream[streamId] = next
        let data = try JSONEncoder().encode(nextByStream)
        let directory = storageURL.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try data.write(to: storageURL, options: .atomic)
        return next
    }
}
