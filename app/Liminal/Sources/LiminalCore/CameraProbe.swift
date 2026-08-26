import AVFoundation
import CoreMedia

/// Camera organ discovery -- master plan §3 (Camera row), §22 (`camera` profile), §120 (Vision
/// organ). Discovery and format enumeration never requires camera authorization on macOS; only
/// starting an `AVCaptureSession` does. This probe never starts a session.
public func probeCamera() -> CameraProfile {
    let discovery = AVCaptureDevice.DiscoverySession(
        deviceTypes: [.builtInWideAngleCamera, .external, .continuityCamera],
        mediaType: .video,
        position: .unspecified,
    )

    guard let device = discovery.devices.first else {
        return CameraProfile(
            state: .unsupported,
            deviceIdHash: sha256Hex("no-camera-device", prefix: "camera"),
            selectedResolution: Resolution(0, 0),
            selectedFps: 0,
            depthData: false,
        )
    }

    let authStatus = AVCaptureDevice.authorizationStatus(for: .video)
    let state: SensorState = switch authStatus {
    case .notDetermined: .probing
    case .authorized: .available
    case .denied: .denied
    case .restricted: .unsupported
    @unknown default: .unknown
    }

    let dimensions = CMVideoFormatDescriptionGetDimensions(device.activeFormat.formatDescription)
    let maxFps = device.activeFormat.videoSupportedFrameRateRanges.map(\.maxFrameRate).max() ?? 0
    // `AVCaptureDevice.Format.supportedDepthDataFormats` is an iOS-only API. Per master plan §23
    // ("Liminal does not assume the built-in Mac camera provides depth"), default to no depth
    // support -- stock Mac cameras and Continuity Camera do not expose depth data on macOS.
    let hasDepth = false

    return CameraProfile(
        state: state,
        deviceIdHash: sha256Hex(device.uniqueID, prefix: "camera"),
        selectedResolution: Resolution(Int(dimensions.width), Int(dimensions.height)),
        selectedFps: Int(maxFps),
        depthData: hasDepth,
    )
}
