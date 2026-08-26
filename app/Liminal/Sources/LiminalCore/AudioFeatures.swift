import Accelerate
import Foundation

// Passive acoustic organ feature types and DSP -- master plan §26 (Passive Acoustic Organ),
// §27 (Passive Audio Features: RMS, peak, spectral centroid/rolloff/flatness, ZCR, VAD
// probability -- MFCC explicitly disabled by default, and stays that way: this module has no
// MFCC function at all). Pure functions over `[Float]` sample buffers only; the actual
// `AVAudioEngine` tap wiring lives in `liminal-capture` and cannot be unit-tested without a
// real microphone, but everything here can be -- and is, against synthetic silence/tone/noise
// buffers.

/// §27: one aggregation window's worth of derived audio features.
public struct AudioFeatureWindow: Codable, Equatable {
    public let rms: Double
    public let peak: Double
    public let zeroCrossingRate: Double
    public let spectralCentroidHz: Double
    public let spectralRolloffHz: Double
    public let spectralFlatness: Double
    /// A heuristic, NOT a trained VAD model -- see `estimateVoiceActivityProbability`'s doc
    /// comment for exactly what it is and its known failure modes. Master plan §28: used only to
    /// suppress active probes and inform privacy UI, never for transcription or identification.
    public let voiceActivityProbability: Double

    public init(
        rms: Double, peak: Double, zeroCrossingRate: Double, spectralCentroidHz: Double,
        spectralRolloffHz: Double, spectralFlatness: Double, voiceActivityProbability: Double,
    ) {
        self.rms = rms
        self.peak = peak
        self.zeroCrossingRate = zeroCrossingRate
        self.spectralCentroidHz = spectralCentroidHz
        self.spectralRolloffHz = spectralRolloffHz
        self.spectralFlatness = spectralFlatness
        self.voiceActivityProbability = voiceActivityProbability
    }

    enum CodingKeys: String, CodingKey {
        case rms
        case peak
        case zeroCrossingRate = "zero_crossing_rate"
        case spectralCentroidHz = "spectral_centroid_hz"
        case spectralRolloffHz = "spectral_rolloff_hz"
        case spectralFlatness = "spectral_flatness"
        case voiceActivityProbability = "voice_activity_probability"
    }
}

public func rmsEnergy(_ samples: [Float]) -> Double {
    guard !samples.isEmpty else { return 0 }
    var meanSquare: Float = 0
    vDSP_measqv(samples, 1, &meanSquare, vDSP_Length(samples.count))
    return Double(sqrtf(meanSquare))
}

public func peakLevel(_ samples: [Float]) -> Double {
    guard !samples.isEmpty else { return 0 }
    var peak: Float = 0
    vDSP_maxmgv(samples, 1, &peak, vDSP_Length(samples.count))
    return Double(peak)
}

/// Fraction of adjacent-sample sign changes, in [0, 1].
public func zeroCrossingRate(_ samples: [Float]) -> Double {
    guard samples.count > 1 else { return 0 }
    var crossings = 0
    for i in 1 ..< samples.count {
        if (samples[i - 1] >= 0) != (samples[i] >= 0) {
            crossings += 1
        }
    }
    return Double(crossings) / Double(samples.count - 1)
}

/// Applies a Hann window in place. Without this, a rectangular window's slow-decaying sidelobes
/// leak enough energy into distant high-frequency bins that `spectralCentroid` on a single pure
/// tone is biased upward by more than 1.5kHz (measured directly: a 1000Hz tone's unwindowed
/// centroid came out near 2555Hz) -- windowing is not an optional refinement here, it's required
/// for the frequency-domain features to mean what their names say.
private func applyHannWindow(_ samples: [Float]) -> [Float] {
    var window = [Float](repeating: 0, count: samples.count)
    vDSP_hann_window(&window, vDSP_Length(samples.count), Int32(vDSP_HANN_NORM))
    var windowed = [Float](repeating: 0, count: samples.count)
    vDSP_vmul(samples, 1, window, 1, &windowed, 1, vDSP_Length(samples.count))
    return windowed
}

/// Magnitude spectrum via a real FFT (Accelerate `vDSP`), Hann-windowed before transforming.
/// `samples.count` must be a power of two; callers window/pad to the nearest power of two before
/// calling. Returns `samples.count / 2` magnitude bins (Nyquist-limited, as is standard for a
/// real-input FFT).
public func magnitudeSpectrum(_ samples: [Float]) -> [Float] {
    let n = samples.count
    guard n > 1, n & (n - 1) == 0 else { return [] } // must be a power of two, and non-trivial

    let windowed = applyHannWindow(samples)

    let log2n = vDSP_Length(log2(Double(n)))
    guard let fftSetup = vDSP_create_fftsetup(log2n, FFTRadix(kFFTRadix2)) else { return [] }
    defer { vDSP_destroy_fftsetup(fftSetup) }

    var realp = [Float](repeating: 0, count: n / 2)
    var imagp = [Float](repeating: 0, count: n / 2)
    var magnitudes = [Float](repeating: 0, count: n / 2)

    realp.withUnsafeMutableBufferPointer { realPtr in
        imagp.withUnsafeMutableBufferPointer { imagPtr in
            var splitComplex = DSPSplitComplex(realp: realPtr.baseAddress!, imagp: imagPtr.baseAddress!)
            windowed.withUnsafeBufferPointer { samplesPtr in
                samplesPtr.baseAddress!.withMemoryRebound(to: DSPComplex.self, capacity: n / 2) {
                    vDSP_ctoz($0, 2, &splitComplex, 1, vDSP_Length(n / 2))
                }
            }
            vDSP_fft_zrip(fftSetup, &splitComplex, 1, log2n, FFTDirection(FFT_FORWARD))
            vDSP_zvmags(&splitComplex, 1, &magnitudes, 1, vDSP_Length(n / 2))
        }
    }
    return magnitudes.map { sqrtf($0) }
}

/// Bin `i`'s center frequency in Hz, given the original (pre-FFT) sample count and sample rate.
private func binFrequency(_ bin: Int, fftInputSize: Int, sampleRate: Double) -> Double {
    Double(bin) * sampleRate / Double(fftInputSize)
}

/// §27: energy-weighted mean frequency -- "brightness" of the sound. Returns 0 for silence
/// (all-zero spectrum), never NaN.
public func spectralCentroid(magnitudes: [Float], fftInputSize: Int, sampleRate: Double) -> Double {
    let totalEnergy = magnitudes.reduce(0, +)
    guard totalEnergy > 0 else { return 0 }
    let weightedSum = magnitudes.enumerated().reduce(0.0) { acc, pair in
        acc + Double(pair.element) * binFrequency(pair.offset, fftInputSize: fftInputSize, sampleRate: sampleRate)
    }
    return weightedSum / Double(totalEnergy)
}

/// §27: the frequency below which `rolloffFraction` of the total spectral energy is contained.
public func spectralRolloff(
    magnitudes: [Float], fftInputSize: Int, sampleRate: Double, rolloffFraction: Double = 0.85,
) -> Double {
    let totalEnergy = magnitudes.reduce(0, +)
    guard totalEnergy > 0 else { return 0 }
    let threshold = Double(totalEnergy) * rolloffFraction
    var cumulative = 0.0
    for (i, magnitude) in magnitudes.enumerated() {
        cumulative += Double(magnitude)
        if cumulative >= threshold {
            return binFrequency(i, fftInputSize: fftInputSize, sampleRate: sampleRate)
        }
    }
    return binFrequency(magnitudes.count - 1, fftInputSize: fftInputSize, sampleRate: sampleRate)
}

/// §27: geometric mean / arithmetic mean of the spectrum -- near 1 for noise-like (flat) sounds,
/// near 0 for tonal sounds concentrated in few bins.
public func spectralFlatness(magnitudes: [Float]) -> Double {
    let nonZero = magnitudes.filter { $0 > 0 }
    guard !nonZero.isEmpty else { return 0 }
    let logSum = nonZero.reduce(0.0) { $0 + log(Double($1)) }
    let geometricMean = exp(logSum / Double(nonZero.count))
    let arithmeticMean = nonZero.reduce(0.0) { $0 + Double($1) } / Double(nonZero.count)
    guard arithmeticMean > 0 else { return 0 }
    return geometricMean / arithmeticMean
}

/// A heuristic voice-activity estimate, NOT a trained model: energy must be above a floor,
/// zero-crossing rate must fall in speech's typical band (very low = silence/hum, very high =
/// noise/fricatives dominate), and the spectrum must not be too flat (voiced speech concentrates
/// energy, unlike white noise). This is intentionally conservative and will misclassify plenty of
/// real audio -- master plan §28 only permits using this to suppress active probes and inform
/// privacy UI, never for transcription, identification, or content analysis, so a coarse
/// approximation is the correct level of investment here, not a defect to fix later without a
/// real evaluation dataset justifying more complexity (§47).
public func estimateVoiceActivityProbability(rms: Double, zeroCrossingRate: Double, spectralFlatness: Double)
    -> Double
{
    guard rms > 0.001 else { return 0.0 }
    let zcrScore = zeroCrossingRate > 0.02 && zeroCrossingRate < 0.35 ? 1.0 : 0.0
    let flatnessScore = 1.0 - min(spectralFlatness, 1.0)
    let energyScore = min(rms * 10, 1.0)
    return (zcrScore + flatnessScore + energyScore) / 3.0
}

/// Rounds `count` down to the nearest power of two >= 2, for windowing samples before FFT.
public func largestPowerOfTwo(atMost count: Int) -> Int {
    guard count >= 2 else { return 2 }
    var p = 1
    while p * 2 <= count {
        p *= 2
    }
    return p
}

/// Computes a full `AudioFeatureWindow` from one buffer of samples. Windows/truncates internally
/// to the largest power-of-two prefix for the FFT-based features -- callers pass whatever buffer
/// size their aggregation window naturally produces.
public func computeAudioFeatureWindow(samples: [Float], sampleRate: Double) -> AudioFeatureWindow {
    let rms = rmsEnergy(samples)
    let peak = peakLevel(samples)
    let zcr = zeroCrossingRate(samples)

    let fftSize = largestPowerOfTwo(atMost: samples.count)
    let fftInput = Array(samples.prefix(fftSize))
    let magnitudes = magnitudeSpectrum(fftInput)
    let centroid = spectralCentroid(magnitudes: magnitudes, fftInputSize: fftSize, sampleRate: sampleRate)
    let rolloff = spectralRolloff(magnitudes: magnitudes, fftInputSize: fftSize, sampleRate: sampleRate)
    let flatness = spectralFlatness(magnitudes: magnitudes)
    let vad = estimateVoiceActivityProbability(rms: rms, zeroCrossingRate: zcr, spectralFlatness: flatness)

    return AudioFeatureWindow(
        rms: rms, peak: peak, zeroCrossingRate: zcr, spectralCentroidHz: centroid,
        spectralRolloffHz: rolloff, spectralFlatness: flatness, voiceActivityProbability: vad,
    )
}
