import CoreBluetooth
import Foundation

/// §38/§39/§40: live Bluetooth proximity scanning. Runs as a central scanner with duplicates
/// allowed (so RSSI can be averaged per peripheral within a window), pseudonymizes every
/// peripheral identifier immediately on discovery -- the raw `CBPeripheral.identifier` UUID is
/// never held past the discovery callback that produces the pseudonym. Untestable without a real
/// Bluetooth radio and nearby advertisers; `pseudonymizeBlePeripheral` and the
/// `BluetoothScanWindow` shape (`BluetoothFeatures.swift`) carry the actually-testable logic.
public final class BluetoothScanCoordinator: NSObject, CBCentralManagerDelegate {
    public typealias FeatureHandler = (BluetoothScanWindow) -> Void

    private var manager: CBCentralManager?
    private let onFeatures: FeatureHandler
    private let pseudonymKey: Data
    private let onDiscovery: (() -> Void)?
    private let queue = DispatchQueue(label: "liminal.bluetooth-scan")
    private var rssiByPseudonym: [String: [Int]] = [:]
    private var windowTimer: DispatchSourceTimer?

    public init(
        pseudonymKey: Data,
        onFeatures: @escaping FeatureHandler,
        onDiscovery: (() -> Void)? = nil,
    ) {
        self.pseudonymKey = pseudonymKey
        self.onFeatures = onFeatures
        self.onDiscovery = onDiscovery
        super.init()
    }

    /// Starts a privacy-safe radio diagnostic. Peripheral identifiers are only presented to
    /// CoreBluetooth's callback and are never retained, hashed, or emitted as features.
    public init(onDiscovery: @escaping () -> Void) {
        pseudonymKey = Data()
        self.onDiscovery = onDiscovery
        onFeatures = { _ in }
        super.init()
    }

    /// Does not request Bluetooth authorization itself -- callers check
    /// `CBCentralManager.authorization` and explain first (§90), same convention as the other
    /// organs.
    public func start(windowDurationSeconds: Double = 5.0) {
        manager = CBCentralManager(delegate: self, queue: queue)

        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now() + windowDurationSeconds, repeating: windowDurationSeconds)
        timer.setEventHandler { [weak self] in self?.flushWindow() }
        timer.resume()
        windowTimer = timer
    }

    public func stop() {
        manager?.stopScan()
        windowTimer?.cancel()
        windowTimer = nil
    }

    private func flushWindow() {
        let clusters = rssiByPseudonym.map { pseudonym, readings in
            BluetoothClusterObservation(
                pseudonym: pseudonym, rssi: readings.reduce(0, +) / max(readings.count, 1),
            )
        }
        rssiByPseudonym.removeAll()
        onFeatures(BluetoothScanWindow(clusters: clusters))
    }

    public func centralManagerDidUpdateState(_ central: CBCentralManager) {
        guard central.state == .poweredOn else { return }
        central.scanForPeripherals(withServices: nil, options: [CBCentralManagerScanOptionAllowDuplicatesKey: true])
    }

    public func centralManager(
        _: CBCentralManager, didDiscover peripheral: CBPeripheral,
        advertisementData _: [String: Any], rssi RSSI: NSNumber,
    ) {
        onDiscovery?()
        guard !pseudonymKey.isEmpty else { return }
        let pseudonym = pseudonymizeBlePeripheral(key: pseudonymKey, identifier: peripheral.identifier.uuidString)
        rssiByPseudonym[pseudonym, default: []].append(RSSI.intValue)
    }
}
