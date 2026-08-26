import AVFoundation
import Foundation

/// §26 Passive Acoustic Organ: taps the microphone input node and aggregates samples into
/// windows for feature extraction. All DSP happens in `AudioFeatures.swift`'s pure functions;
/// this class only owns the `AVAudioEngine` tap, which cannot be unit-tested without a real
/// microphone.
public final class AudioCaptureCoordinator {
    public typealias FeatureHandler = (AudioFeatureWindow) -> Void

    private let engine = AVAudioEngine()
    private let onFeatures: FeatureHandler
    private var buffer: [Float] = []
    /// §27 lists a 1s aggregate window alongside the 20ms low-level frame; this organ reports at
    /// the 1s cadence (the coarser of the two) to keep envelope volume reasonable for a skeleton.
    private let windowSampleCount: Int
    private let sampleRate: Double

    public init(windowDurationSeconds: Double = 1.0, onFeatures: @escaping FeatureHandler) {
        self.onFeatures = onFeatures
        let format = engine.inputNode.inputFormat(forBus: 0)
        sampleRate = format.sampleRate > 0 ? format.sampleRate : 48000
        windowSampleCount = Int(sampleRate * windowDurationSeconds)
    }

    /// Starts tapping the input node. Does not request microphone authorization -- callers must
    /// do that first (§90), same convention as `VisionCaptureCoordinator`.
    public func start() throws {
        let format = engine.inputNode.inputFormat(forBus: 0)
        engine.inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] pcmBuffer, _ in
            self?.handle(pcmBuffer)
        }
        try engine.start()
    }

    public func stop() {
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
    }

    private func handle(_ pcmBuffer: AVAudioPCMBuffer) {
        guard let channelData = pcmBuffer.floatChannelData else { return }
        let frameLength = Int(pcmBuffer.frameLength)
        // §26/§27: mono feature extraction is sufficient for this organ's stated features
        // (energy/spectral/ZCR/VAD are not stereo-dependent); only the first channel is read.
        let samples = Array(UnsafeBufferPointer(start: channelData[0], count: frameLength))
        buffer.append(contentsOf: samples)

        while buffer.count >= windowSampleCount {
            let window = Array(buffer.prefix(windowSampleCount))
            buffer.removeFirst(windowSampleCount)
            let features = computeAudioFeatureWindow(samples: window, sampleRate: sampleRate)
            onFeatures(features)
        }
    }
}
