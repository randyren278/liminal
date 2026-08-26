import AVFoundation

/// Passive/active acoustic organ discovery -- master plan §22 (`audio_input`/`audio_output`),
/// §26 (Passive Acoustic Organ), §29 (Active Acoustic Organ). Querying node formats never
/// requires microphone authorization; only actually tapping the input node for samples does.
public func probeAudioInput() -> AudioProfile {
    let authStatus = AVCaptureDevice.authorizationStatus(for: .audio)
    let state: SensorState = switch authStatus {
    case .notDetermined: .probing
    case .authorized: .available
    case .denied: .denied
    case .restricted: .unsupported
    @unknown default: .unknown
    }

    let engine = AVAudioEngine()
    let format = engine.inputNode.inputFormat(forBus: 0)
    return AudioProfile(
        state: state,
        sampleRate: Int(format.sampleRate),
        channels: Int(format.channelCount),
    )
}

/// Speaker output never requires TCC authorization on macOS -- only §29's active probe (playing
/// audio deliberately) needs the opt-in/safety gating described there, not a permission grant.
public func probeAudioOutput() -> AudioProfile {
    let engine = AVAudioEngine()
    let format = engine.outputNode.outputFormat(forBus: 0)
    let state: SensorState = format.sampleRate > 0 ? .available : .unsupported
    return AudioProfile(
        state: state,
        sampleRate: Int(format.sampleRate),
        channels: Int(format.channelCount),
    )
}
