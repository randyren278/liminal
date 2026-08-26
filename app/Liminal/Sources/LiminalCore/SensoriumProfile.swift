import Foundation

/// Mirrors `liminal_schema::sensorium::SensorState` (crates/liminal-schema/src/sensorium.rs) --
/// master plan §3. Keep the raw values in sync with that Rust enum's `#[serde(rename_all =
/// "snake_case")]` output.
public enum SensorState: String, Codable {
    case unknown
    case probing
    case available
    case degraded
    case denied
    case busy
    case unsupported
    case failed
    case disabledByUser = "disabled_by_user"
}

/// A resolution pair that encodes as a two-element JSON array (`[width, height]`), matching
/// Rust's `(u32, u32)` tuple serialization via serde -- master plan §22's
/// `"selected_resolution": [1280, 720]` example.
public struct Resolution: Codable, Equatable {
    public let width: Int
    public let height: Int

    public init(_ width: Int, _ height: Int) {
        self.width = width
        self.height = height
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.unkeyedContainer()
        try container.encode(width)
        try container.encode(height)
    }

    public init(from decoder: Decoder) throws {
        var container = try decoder.unkeyedContainer()
        width = try container.decode(Int.self)
        height = try container.decode(Int.self)
    }
}

/// Mirrors `liminal_schema::sensorium::CameraProfile` -- §22.
public struct CameraProfile: Codable {
    public let state: SensorState
    public let deviceIdHash: String
    public let selectedResolution: Resolution
    public let selectedFps: Int
    public let depthData: Bool

    public init(
        state: SensorState, deviceIdHash: String, selectedResolution: Resolution,
        selectedFps: Int, depthData: Bool,
    ) {
        self.state = state
        self.deviceIdHash = deviceIdHash
        self.selectedResolution = selectedResolution
        self.selectedFps = selectedFps
        self.depthData = depthData
    }

    enum CodingKeys: String, CodingKey {
        case state
        case deviceIdHash = "device_id_hash"
        case selectedResolution = "selected_resolution"
        case selectedFps = "selected_fps"
        case depthData = "depth_data"
    }
}

/// Mirrors `liminal_schema::sensorium::AudioProfile` -- shared shape for audio_input/output, §22.
public struct AudioProfile: Codable {
    public let state: SensorState
    public let sampleRate: Int
    public let channels: Int

    public init(state: SensorState, sampleRate: Int, channels: Int) {
        self.state = state
        self.sampleRate = sampleRate
        self.channels = channels
    }

    enum CodingKeys: String, CodingKey {
        case state
        case sampleRate = "sample_rate"
        case channels
    }
}

/// Mirrors `liminal_schema::sensorium::WifiProfile` -- §22.
public struct WifiProfile: Codable {
    public let state: SensorState
    public let aggregateRssi: Bool
    public let aggregateNoise: Bool
    public let scanning: Bool
    public let stableApIds: Bool
    public let csi: Bool

    public init(
        state: SensorState, aggregateRssi: Bool, aggregateNoise: Bool, scanning: Bool,
        stableApIds: Bool, csi: Bool,
    ) {
        self.state = state
        self.aggregateRssi = aggregateRssi
        self.aggregateNoise = aggregateNoise
        self.scanning = scanning
        self.stableApIds = stableApIds
        self.csi = csi
    }

    enum CodingKeys: String, CodingKey {
        case state
        case aggregateRssi = "aggregate_rssi"
        case aggregateNoise = "aggregate_noise"
        case scanning
        case stableApIds = "stable_ap_ids"
        case csi
    }
}

/// Mirrors `liminal_schema::sensorium::BluetoothProfile` -- §22.
public struct BluetoothProfile: Codable {
    public let state: SensorState
    public let scanRssi: Bool

    public init(state: SensorState, scanRssi: Bool) {
        self.state = state
        self.scanRssi = scanRssi
    }

    enum CodingKeys: String, CodingKey {
        case state
        case scanRssi = "scan_rssi"
    }
}

/// Mirrors `liminal_schema::sensorium::SensoriumProfile` -- §22. This is the Swift-side
/// counterpart to the Rust type; the two are not yet wired together over IPC (that's `liminal-ipc`
/// plus a future `liminald` client, not this probe), but the JSON shape is kept identical so the
/// eventual wiring is a transport concern, not a schema-reconciliation one.
public struct SensoriumProfile: Codable {
    public let schemaVersion: Int
    public let machineProfileId: String
    public let createdAt: String
    public let camera: CameraProfile
    public let audioInput: AudioProfile
    public let audioOutput: AudioProfile
    public let wifi: WifiProfile
    public let bluetooth: BluetoothProfile

    public init(
        schemaVersion: Int, machineProfileId: String, createdAt: String, camera: CameraProfile,
        audioInput: AudioProfile, audioOutput: AudioProfile, wifi: WifiProfile,
        bluetooth: BluetoothProfile,
    ) {
        self.schemaVersion = schemaVersion
        self.machineProfileId = machineProfileId
        self.createdAt = createdAt
        self.camera = camera
        self.audioInput = audioInput
        self.audioOutput = audioOutput
        self.wifi = wifi
        self.bluetooth = bluetooth
    }

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case machineProfileId = "machine_profile_id"
        case createdAt = "created_at"
        case camera
        case audioInput = "audio_input"
        case audioOutput = "audio_output"
        case wifi
        case bluetooth
    }
}
