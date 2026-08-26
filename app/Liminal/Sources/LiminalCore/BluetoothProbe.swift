import CoreBluetooth
import Foundation

/// Bluetooth proximity atmosphere organ discovery -- master plan §38 (Bluetooth Organ), §39
/// (Bluetooth Privacy). `CBCentralManager.authorization` reads the current TCC grant without
/// prompting; determining radio power state requires a live `CBCentralManager` delegate
/// callback, so this probe creates one and waits briefly on the run loop rather than assuming a
/// state synchronously.
private final class BluetoothStateObserver: NSObject, CBCentralManagerDelegate {
    var state: CBManagerState = .unknown
    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        state = central.state
    }
}

public func probeBluetooth() -> BluetoothProfile {
    let authorization = CBCentralManager.authorization
    let authState: SensorState = switch authorization {
    case .notDetermined: .probing
    case .allowedAlways: .available
    case .denied: .denied
    case .restricted: .unsupported
    @unknown default: .unknown
    }

    guard authState == .available else {
        return BluetoothProfile(state: authState, scanRssi: false)
    }

    let observer = BluetoothStateObserver()
    let queue = DispatchQueue(label: "liminal-doctor.bluetooth-probe")
    let manager = CBCentralManager(delegate: observer, queue: queue)

    let deadline = Date().addingTimeInterval(2.0)
    while observer.state == .unknown, Date() < deadline {
        RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.05))
    }
    _ = manager // keep alive until the wait loop above finishes

    let state: SensorState = switch observer.state {
    case .poweredOn: .available
    case .poweredOff: .disabledByUser
    case .unauthorized: .denied
    case .unsupported: .unsupported
    default: .probing
    }

    return BluetoothProfile(state: state, scanRssi: state == .available)
}
