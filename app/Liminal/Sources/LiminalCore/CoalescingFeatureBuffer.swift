import Foundation

public struct PendingFeature: Equatable {
    public let streamId: String
    public let payload: Data
    public let capturedAtUtcUs: Int64
    public let capturedAtMonoUs: Int64

    public init(
        streamId: String,
        payload: Data,
        capturedAtUtcUs: Int64,
        capturedAtMonoUs: Int64,
    ) {
        self.streamId = streamId
        self.payload = payload
        self.capturedAtUtcUs = capturedAtUtcUs
        self.capturedAtMonoUs = capturedAtMonoUs
    }
}

/// A bounded, fair handoff from concurrent sensor callbacks to one blocking writer.
/// At most one latest value is pending per stream. Taking a batch snapshots every
/// pending stream so a continuously updating stream cannot starve another modality.
public final class CoalescingFeatureBuffer {
    private let lock = NSLock()
    private var pending: [String: PendingFeature] = [:]
    private var draining = false

    public init() {}

    /// Returns true only when the caller must schedule the single drain worker.
    public func submit(_ feature: PendingFeature) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        pending[feature.streamId] = feature
        guard !draining else { return false }
        draining = true
        return true
    }

    /// Returns one stable batch containing every stream that was pending at the
    /// snapshot boundary, or nil after atomically marking the drain idle.
    public func takeBatch() -> [PendingFeature]? {
        lock.lock()
        defer { lock.unlock() }
        guard !pending.isEmpty else {
            draining = false
            return nil
        }
        let batch = pending.values.sorted { $0.streamId < $1.streamId }
        pending.removeAll(keepingCapacity: true)
        return batch
    }
}
