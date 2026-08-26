import Foundation

// Wi-Fi radio atmosphere organ -- master plan §34 (Wi-Fi Organ), §36 (Mode A: anonymous
// aggregate, the default and only mode implemented here), §37 (Wi-Fi Feature Vector). Mirrors
// `liminal_policy::{RawWifiScanResult, WifiObservationModeA, sanitize_wifi_mode_a}` on the Rust
// side field-for-field and behavior-for-behavior -- the sanitization happens here, on the Swift
// side, before anything crosses the IPC boundary, which is a stronger privacy property than
// sanitizing after the fact: there is no code path in this process that could accidentally send
// a raw SSID/BSSID, because `WifiObservationModeA` has no field to put one in.

/// A raw Wi-Fi scan hit exactly as `CoreWLAN` would surface it. Only ever constructed from a live
/// `CWNetwork` scan result inside `WifiScanCoordinator` -- never persisted or sent anywhere.
public struct RawWifiScanResult {
    public let ssid: String?
    public let bssid: String?
    public let rssi: Int
    public let noise: Int

    public init(ssid: String?, bssid: String?, rssi: Int, noise: Int) {
        self.ssid = ssid
        self.bssid = bssid
        self.rssi = rssi
        self.noise = noise
    }
}

/// §37: Mode A (default) anonymous atmosphere features. No SSID/BSSID field exists on this type
/// by construction -- the same structural guarantee `liminal_policy::WifiObservationModeA` makes.
public struct WifiObservationModeA: Codable, Equatable {
    public let rssiMean: Double
    public let noiseMean: Double
    public let visibleNetworkCount: Int
    public let strongest1: Int
    public let strongest2: Int
    public let strongest3: Int

    public init(
        rssiMean: Double, noiseMean: Double, visibleNetworkCount: Int, strongest1: Int,
        strongest2: Int, strongest3: Int,
    ) {
        self.rssiMean = rssiMean
        self.noiseMean = noiseMean
        self.visibleNetworkCount = visibleNetworkCount
        self.strongest1 = strongest1
        self.strongest2 = strongest2
        self.strongest3 = strongest3
    }

    enum CodingKeys: String, CodingKey {
        case rssiMean = "rssi_mean"
        case noiseMean = "noise_mean"
        case visibleNetworkCount = "visible_network_count"
        case strongest1 = "strongest_1"
        case strongest2 = "strongest_2"
        case strongest3 = "strongest_3"
    }
}

/// Reduce raw scans to Mode A aggregate features. Mirrors the Rust side's
/// `sanitize_wifi_mode_a` exactly: mean RSSI/noise, visible network count, and the three
/// strongest RSSI values -- nothing else is retained from the raw scan.
public func sanitizeWifiModeA(_ raw: [RawWifiScanResult]) -> WifiObservationModeA {
    let count = max(raw.count, 1)
    let rssiMean = Double(raw.reduce(0) { $0 + $1.rssi }) / Double(count)
    let noiseMean = Double(raw.reduce(0) { $0 + $1.noise }) / Double(count)
    let sortedRssi = raw.map(\.rssi).sorted(by: >)
    func strongest(_ i: Int) -> Int {
        i < sortedRssi.count ? sortedRssi[i] : Int.min
    }
    return WifiObservationModeA(
        rssiMean: rssiMean, noiseMean: noiseMean, visibleNetworkCount: raw.count,
        strongest1: strongest(0), strongest2: strongest(1), strongest3: strongest(2),
    )
}
