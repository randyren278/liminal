@testable import LiminalCore
import XCTest

final class AudioFeaturesTests: XCTestCase {
    let sampleRate = 48000.0

    func sineWave(frequencyHz: Double, count: Int, amplitude: Float = 0.8) -> [Float] {
        (0 ..< count).map { i in
            amplitude * Float(sin(2.0 * Double.pi * frequencyHz * Double(i) / sampleRate))
        }
    }

    func whiteNoise(count: Int, amplitude: Float = 0.5) -> [Float] {
        var generator = SystemRandomNumberGenerator()
        return (0 ..< count).map { _ in Float.random(in: -amplitude ... amplitude, using: &generator) }
    }

    // MARK: - rmsEnergy / peakLevel

    func testRmsEnergyOfSilenceIsZero() {
        XCTAssertEqual(rmsEnergy([Float](repeating: 0, count: 1024)), 0)
    }

    func testRmsEnergyOfAConstantSignalEqualsItsMagnitude() {
        let samples = [Float](repeating: 0.5, count: 100)
        XCTAssertEqual(rmsEnergy(samples), 0.5, accuracy: 0.0001)
    }

    func testPeakLevelFindsTheLargestMagnitudeSample() {
        let samples: [Float] = [0.1, -0.9, 0.3, 0.2]
        XCTAssertEqual(peakLevel(samples), 0.9, accuracy: 0.0001)
    }

    func testRmsAndPeakHandleEmptyInputWithoutCrashing() {
        XCTAssertEqual(rmsEnergy([]), 0)
        XCTAssertEqual(peakLevel([]), 0)
    }

    // MARK: - zeroCrossingRate

    func testZeroCrossingRateOfSilenceIsZero() {
        XCTAssertEqual(zeroCrossingRate([Float](repeating: 0, count: 100)), 0)
    }

    func testZeroCrossingRateOfAlternatingSignalIsMaximal() {
        let samples: [Float] = (0 ..< 100).map { $0 % 2 == 0 ? 1.0 : -1.0 }
        XCTAssertEqual(zeroCrossingRate(samples), 1.0, accuracy: 0.0001)
    }

    func testZeroCrossingRateOfAHighFrequencyToneExceedsALowFrequencyTone() {
        let low = zeroCrossingRate(sineWave(frequencyHz: 100, count: 4096))
        let high = zeroCrossingRate(sineWave(frequencyHz: 8000, count: 4096))
        XCTAssertGreaterThan(high, low)
    }

    // MARK: - magnitudeSpectrum / spectralCentroid

    func testMagnitudeSpectrumReturnsEmptyForNonPowerOfTwoInput() {
        XCTAssertEqual(magnitudeSpectrum([Float](repeating: 0, count: 100)), [])
    }

    func testMagnitudeSpectrumReturnsHalfTheInputLengthInBins() {
        let spectrum = magnitudeSpectrum(sineWave(frequencyHz: 440, count: 1024))
        XCTAssertEqual(spectrum.count, 512)
    }

    func testSpectralCentroidOfSilenceIsZeroNotNaN() {
        let magnitudes = magnitudeSpectrum([Float](repeating: 0, count: 1024))
        let centroid = spectralCentroid(magnitudes: magnitudes, fftInputSize: 1024, sampleRate: sampleRate)
        XCTAssertEqual(centroid, 0)
        XCTAssertFalse(centroid.isNaN)
    }

    func testSpectralCentroidOfAHighFrequencyToneExceedsALowFrequencyTone() {
        let lowSpectrum = magnitudeSpectrum(sineWave(frequencyHz: 200, count: 2048))
        let highSpectrum = magnitudeSpectrum(sineWave(frequencyHz: 6000, count: 2048))
        let lowCentroid = spectralCentroid(magnitudes: lowSpectrum, fftInputSize: 2048, sampleRate: sampleRate)
        let highCentroid = spectralCentroid(magnitudes: highSpectrum, fftInputSize: 2048, sampleRate: sampleRate)
        XCTAssertGreaterThan(highCentroid, lowCentroid)
    }

    func testSpectralCentroidOfAPureToneIsNearItsFrequency() {
        let toneHz = 1000.0
        let spectrum = magnitudeSpectrum(sineWave(frequencyHz: toneHz, count: 4096))
        let centroid = spectralCentroid(magnitudes: spectrum, fftInputSize: 4096, sampleRate: sampleRate)
        // Bin resolution at 4096/48kHz is ~11.7Hz; allow generous tolerance for windowing leakage.
        XCTAssertEqual(centroid, toneHz, accuracy: 100)
    }

    // MARK: - spectralRolloff

    func testSpectralRolloffOfSilenceIsZero() {
        let magnitudes = magnitudeSpectrum([Float](repeating: 0, count: 1024))
        XCTAssertEqual(
            spectralRolloff(magnitudes: magnitudes, fftInputSize: 1024, sampleRate: sampleRate), 0,
        )
    }

    func testSpectralRolloffOfAPureLowToneIsWellBelowNyquist() {
        let spectrum = magnitudeSpectrum(sineWave(frequencyHz: 300, count: 4096))
        let rolloff = spectralRolloff(magnitudes: spectrum, fftInputSize: 4096, sampleRate: sampleRate)
        XCTAssertLessThan(rolloff, sampleRate / 4)
    }

    // MARK: - spectralFlatness

    func testSpectralFlatnessOfAPureToneIsLow() {
        let spectrum = magnitudeSpectrum(sineWave(frequencyHz: 440, count: 2048))
        XCTAssertLessThan(spectralFlatness(magnitudes: spectrum), 0.3)
    }

    func testSpectralFlatnessOfWhiteNoiseIsHigherThanAPureTone() {
        let toneSpectrum = magnitudeSpectrum(sineWave(frequencyHz: 440, count: 4096))
        let noiseSpectrum = magnitudeSpectrum(whiteNoise(count: 4096))
        XCTAssertGreaterThan(spectralFlatness(magnitudes: noiseSpectrum), spectralFlatness(magnitudes: toneSpectrum))
    }

    func testSpectralFlatnessOfSilenceIsZeroNotNaN() {
        let spectrum = magnitudeSpectrum([Float](repeating: 0, count: 1024))
        let flatness = spectralFlatness(magnitudes: spectrum)
        XCTAssertEqual(flatness, 0)
        XCTAssertFalse(flatness.isNaN)
    }

    // MARK: - estimateVoiceActivityProbability

    func testVoiceActivityProbabilityIsZeroForSilence() {
        XCTAssertEqual(estimateVoiceActivityProbability(rms: 0, zeroCrossingRate: 0, spectralFlatness: 0), 0)
    }

    func testVoiceActivityProbabilityIsHigherForToneLikeSignalThanPureNoise() {
        let toneVad = estimateVoiceActivityProbability(rms: 0.1, zeroCrossingRate: 0.1, spectralFlatness: 0.1)
        let noiseVad = estimateVoiceActivityProbability(rms: 0.1, zeroCrossingRate: 0.1, spectralFlatness: 0.9)
        XCTAssertGreaterThan(toneVad, noiseVad)
    }

    // MARK: - largestPowerOfTwo

    func testLargestPowerOfTwoAtMostFindsTheExactPowerWhenGivenOne() {
        XCTAssertEqual(largestPowerOfTwo(atMost: 1024), 1024)
    }

    func testLargestPowerOfTwoAtMostRoundsDownForNonPowers() {
        XCTAssertEqual(largestPowerOfTwo(atMost: 1000), 512)
        XCTAssertEqual(largestPowerOfTwo(atMost: 3), 2)
    }

    // MARK: - computeAudioFeatureWindow

    func testComputeAudioFeatureWindowProducesSaneValuesForATone() {
        let window = computeAudioFeatureWindow(samples: sineWave(frequencyHz: 440, count: 2048), sampleRate: sampleRate)
        XCTAssertGreaterThan(window.rms, 0)
        XCTAssertGreaterThan(window.peak, 0)
        XCTAssertGreaterThan(window.spectralCentroidHz, 0)
        XCTAssertFalse(window.spectralCentroidHz.isNaN)
        XCTAssertFalse(window.voiceActivityProbability.isNaN)
    }

    func testComputeAudioFeatureWindowEncodesToTheExpectedJsonKeys() throws {
        let window = computeAudioFeatureWindow(samples: sineWave(frequencyHz: 440, count: 512), sampleRate: sampleRate)
        let data = try JSONEncoder().encode(window)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        for key in [
            "rms", "peak", "zero_crossing_rate", "spectral_centroid_hz", "spectral_rolloff_hz",
            "spectral_flatness", "voice_activity_probability",
        ] {
            XCTAssertNotNil(json[key], "missing key \(key)")
        }
    }
}
