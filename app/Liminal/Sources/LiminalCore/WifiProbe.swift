import CoreWLAN

/// Wi-Fi radio atmosphere organ discovery -- master plan §34 (Wi-Fi Organ), §36 (Wi-Fi Privacy
/// Modes). Reading aggregate RSSI/noise/channel/rate never requires Location authorization on
/// macOS; only resolving SSID/BSSID (Mode B, not implemented by this probe) does.
public func probeWifi() -> WifiProfile {
    guard let interface = CWWiFiClient.shared().interface() else {
        return WifiProfile(
            state: .unsupported,
            aggregateRssi: false,
            aggregateNoise: false,
            scanning: false,
            stableApIds: false,
            csi: false,
        )
    }

    let state: SensorState = interface.powerOn() ? .available : .disabledByUser

    return WifiProfile(
        state: state,
        aggregateRssi: true,
        aggregateNoise: true,
        scanning: state == .available,
        // Mode A (anonymous aggregate) is this probe's only mode -- stable AP identity (Mode B)
        // requires explicit user enablement and Location permission, per §36, neither requested
        // here.
        stableApIds: false,
        // §34: CSI is UNSUPPORTED_BY_DESIGN on stock CoreWLAN hardware.
        csi: false,
    )
}
