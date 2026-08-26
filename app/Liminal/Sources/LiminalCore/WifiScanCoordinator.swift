import CoreWLAN
import Foundation

/// §34/§35: live Wi-Fi Mode A scanning. `CWInterface.scanForNetworks` is a synchronous, blocking
/// call (can take a few seconds), so this runs on its own background queue rather than blocking
/// the caller. Untestable without a real Wi-Fi radio -- `sanitizeWifiModeA` (`WifiFeatures.swift`)
/// carries all the actually-testable logic; this class only owns the live scan call and the
/// mapping from `CWNetwork` into the already-tested `RawWifiScanResult` shape.
public final class WifiScanCoordinator {
    public typealias FeatureHandler = (WifiObservationModeA) -> Void

    private let onFeatures: FeatureHandler
    private let queue = DispatchQueue(label: "liminal.wifi-scan")
    private var timer: DispatchSourceTimer?

    public init(onFeatures: @escaping FeatureHandler) {
        self.onFeatures = onFeatures
    }

    /// §35: "Network scan interval: 30-60 seconds initially." Starts immediately, then repeats.
    public func start(scanIntervalSeconds: Double = 45) {
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now(), repeating: scanIntervalSeconds)
        timer.setEventHandler { [weak self] in self?.scanOnce() }
        timer.resume()
        self.timer = timer
    }

    public func stop() {
        timer?.cancel()
        timer = nil
    }

    private func scanOnce() {
        guard let interface = CWWiFiClient.shared().interface() else { return }
        do {
            let networks = try interface.scanForNetworks(withSSID: nil)
            let raw = networks.map {
                RawWifiScanResult(ssid: nil, bssid: nil, rssi: $0.rssiValue, noise: $0.noiseMeasurement)
            }
            onFeatures(sanitizeWifiModeA(raw))
        } catch {
            // A failed scan (radio busy, interface off mid-scan) is not fatal -- just skip this
            // window and try again on the next timer tick.
        }
    }
}
