import Foundation

// Bluetooth proximity atmosphere organ -- master plan §38 (Bluetooth Organ), §39 (Bluetooth
// Privacy: never a human-readable name, only an HMAC pseudonym), §40 (Bluetooth Feature
// Vector).

/// §39: a single discovered peripheral's pseudonymized proximity reading. Never carries a
/// device name -- there is no field for one.
public struct BluetoothClusterObservation: Codable, Equatable {
    /// `ble:<hex>` -- HMAC-SHA256(local key, `CBPeripheral.identifier`), never the raw UUID.
    public let pseudonym: String
    public let rssi: Int

    public init(pseudonym: String, rssi: Int) {
        self.pseudonym = pseudonym
        self.rssi = rssi
    }
}

/// §40: one scan window's aggregate Bluetooth features.
public struct BluetoothScanWindow: Codable, Equatable {
    public let clusters: [BluetoothClusterObservation]
    public let clusterCount: Int

    public init(clusters: [BluetoothClusterObservation]) {
        self.clusters = clusters
        clusterCount = clusters.count
    }

    enum CodingKeys: String, CodingKey {
        case clusters
        case clusterCount = "cluster_count"
    }
}

/// Pseudonymizes one peripheral identifier. A thin, testable wrapper over `hmacSha256Hex` so
/// call sites read as Bluetooth-domain code, not generic crypto.
public func pseudonymizeBlePeripheral(key: Data, identifier: String) -> String {
    hmacSha256Hex(key: key, message: identifier, prefix: "ble")
}

/// Convert the bounded acceptance state into a diagnostic that distinguishes an unavailable
/// startup path from a completed scan that found no advertisers. A scan window with zero clusters
/// is not a Bluetooth observation and must not be counted as one.
public func bluetoothAcceptanceStatus(
    startupStatus: String,
    discoveredSampleCount: Int,
    discoveredPeripheralCount: Int = 0,
) -> String {
    if discoveredSampleCount > 0 {
        return "observed"
    }
    if discoveredPeripheralCount > 0, startupStatus != "running" {
        return "advertisers_detected_keychain_unavailable"
    }
    if startupStatus == "running" {
        return "no_advertisers_observed"
    }
    return startupStatus
}
