//! A telemetry-driven spectral artwork. It renders derived feature values only; this is not a
//! camera preview and never needs raw audio, video, Wi-Fi identifiers, or Bluetooth identities.
//!
//! The visual grammar deliberately keeps modalities distinct:
//! - acoustics become interference contours, wave surfaces, and reverberation tails;
//! - Wi-Fi becomes a slow contour field plus RF-change ripples;
//! - Bluetooth becomes discrete luminous clusters with uncertainty halos and faint trails;
//! - camera-derived presence bends the field as a refractive volume instead of drawing a person.
//!
//! This follows the master-plan intent more closely than the old generic orbiting-ribbon field and
//! makes changes in real derived telemetry legible without turning the console into a dashboard.

use crate::ledger_view::TelemetrySnapshot;
use image::{Rgb, RgbImage};

fn clamp(value: f64, low: f64, high: f64) -> f64 {
    value.clamp(low, high)
}

fn bar(value: Option<f64>, low: f64, high: f64) -> f64 {
    value
        .map(|v| clamp((v - low) / (high - low), 0.0, 1.0))
        .unwrap_or(0.0)
}

/// Lift a present signal through a perceptual curve. `None` and explicit zero remain dark while
/// weak-but-real observations survive terminal scaling.
fn visual_strength(value: f64) -> f64 {
    if value <= 0.0 {
        return 0.0;
    }
    let gamma = value.clamp(0.0, 1.0).powf(0.48);
    let smooth = gamma * gamma * (3.0 - 2.0 * gamma);
    0.16 + smooth * 0.84
}

fn in_gamut(rgb: [f64; 3]) -> bool {
    rgb.iter().all(|channel| (0.0..=1.0).contains(channel))
}

/// Convert OKLCH to linear sRGB and reduce chroma until the color fits the sRGB gamut. This keeps
/// hue and lightness intentional instead of clipping individual channels into brown/gray.
fn oklch_to_linear_srgb(lightness: f64, chroma: f64, hue_degrees: f64) -> [f64; 3] {
    let hue = hue_degrees.to_radians();
    let a = hue.cos();
    let b = hue.sin();

    let convert = |candidate_chroma: f64| {
        let l =
            lightness + 0.3963377774 * candidate_chroma * a + 0.2158037573 * candidate_chroma * b;
        let m =
            lightness - 0.1055613458 * candidate_chroma * a - 0.0638541728 * candidate_chroma * b;
        let s =
            lightness - 0.0894841775 * candidate_chroma * a - 1.2914855480 * candidate_chroma * b;
        let l3 = l * l * l;
        let m3 = m * m * m;
        let s3 = s * s * s;
        [
            4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3,
            -1.2684380046 * l3 + 2.6097574011 * m3 - 0.3413193965 * s3,
            -0.0041960863 * l3 - 0.7034186147 * m3 + 1.7076147010 * s3,
        ]
    };

    let candidate = convert(chroma);
    if in_gamut(candidate) {
        return candidate;
    }

    let mut low = 0.0;
    let mut high = chroma;
    for _ in 0..14 {
        let middle = (low + high) * 0.5;
        if in_gamut(convert(middle)) {
            low = middle;
        } else {
            high = middle;
        }
    }
    convert(low)
}

fn linear_to_srgb(value: f64) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

fn add_light(target: &mut [f64; 3], color: [f64; 3], amount: f64) {
    let amount = amount.max(0.0);
    for (channel, light) in target.iter_mut().zip(color) {
        *channel += light * amount;
    }
}

fn finish_pixel(mut linear: [f64; 3]) -> Rgb<u8> {
    for channel in &mut linear {
        *channel = 1.0 - (-*channel * 2.45).exp();
    }
    let luma = linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
    for channel in &mut linear {
        *channel = luma + (*channel - luma) * 1.17;
    }
    Rgb([
        linear_to_srgb(linear[0]),
        linear_to_srgb(linear[1]),
        linear_to_srgb(linear[2]),
    ])
}

/// Draw a slowly evolving field whose visual structures are explicitly modality-shaped.
pub fn spectral_frame(
    width: u32,
    height: u32,
    tick: u32,
    telemetry: &TelemetrySnapshot,
) -> RgbImage {
    let width = width.max(1);
    let height = height.max(1);

    let audio = bar(telemetry.audio_rms, 0.0, 0.35);
    let centroid = bar(telemetry.audio_centroid_hz, 0.0, 8000.0);
    let wifi = bar(telemetry.wifi_rssi_mean, -100.0, -20.0);
    let noise = bar(telemetry.wifi_noise_mean, -110.0, -50.0);
    let wifi_density = bar(telemetry.wifi_network_count, 0.0, 20.0);
    let ble_clusters = bar(telemetry.bluetooth_cluster_count, 0.0, 12.0);
    let ble_rssi = bar(telemetry.bluetooth_mean_rssi, -100.0, -20.0);
    let vad = telemetry.audio_vad.unwrap_or(0.0).clamp(0.0, 1.0);
    let camera = telemetry.camera_presence.unwrap_or(0.0).clamp(0.0, 1.0);

    let audio_present = telemetry.audio_rms.is_some()
        || telemetry.audio_centroid_hz.is_some()
        || telemetry.audio_vad.is_some();
    let wifi_present = telemetry.wifi_rssi_mean.is_some()
        || telemetry.wifi_noise_mean.is_some()
        || telemetry.wifi_network_count.is_some();
    let ble_present =
        telemetry.bluetooth_cluster_count.is_some() || telemetry.bluetooth_mean_rssi.is_some();

    let audio_strength = if audio_present {
        visual_strength(audio.max(if telemetry.audio_centroid_hz.is_some() {
            0.08
        } else {
            0.0
        }))
    } else {
        0.0
    };
    let wifi_strength = if wifi_present {
        visual_strength((wifi * 0.70 + wifi_density * 0.30).clamp(0.0, 1.0))
    } else {
        0.0
    };
    let ble_strength = if ble_present {
        visual_strength((ble_clusters * 0.72 + ble_rssi * 0.28).clamp(0.0, 1.0))
    } else {
        0.0
    };
    let camera_strength = visual_strength(camera);
    let vad_strength = visual_strength(vad);

    let teal = oklch_to_linear_srgb(0.68, 0.19, 178.0);
    let cyan = oklch_to_linear_srgb(0.69, 0.18, 218.0);
    let violet = oklch_to_linear_srgb(0.65, 0.20, 292.0);
    let amber = oklch_to_linear_srgb(0.73, 0.18, 72.0);
    let rose = oklch_to_linear_srgb(0.68, 0.20, 350.0);
    let blue = oklch_to_linear_srgb(0.60, 0.18, 245.0);

    let phase = tick as f64 * 0.045;
    let aspect = width as f64 / height as f64;
    let field_cx = 0.04 * (phase * 0.17 + wifi * 2.1).sin() - 0.03 * camera;
    let field_cy = 0.035 * (phase * 0.13 + audio * 2.7).cos() + 0.02 * wifi_density;
    let ble_count = telemetry
        .bluetooth_cluster_count
        .unwrap_or(0.0)
        .round()
        .clamp(0.0, 8.0) as usize;

    let mut image = RgbImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let fx = (x as f64 + 0.5) / width as f64;
            let fy = (y as f64 + 0.5) / height as f64;
            let dx = (fx - 0.5) * aspect - field_cx;
            let dy = fy - 0.5 - field_cy;

            // Domain warping makes the field feel spatial rather than like a centered screensaver.
            let warp_x = 0.045 * (dy * 8.4 + phase * 0.31).sin()
                + 0.018 * ((dx + dy) * 17.0 - phase * 0.21).sin();
            let warp_y = 0.035 * (dx * 7.1 - phase * 0.27).cos()
                + 0.017 * ((dx - dy) * 14.0 + phase * 0.18).sin();
            let wx = dx + warp_x;
            let wy = dy + warp_y;
            let radius = (wx * wx + wy * wy).sqrt();
            let angle = wy.atan2(wx);

            let flow =
                0.5 + 0.5 * (wx * 4.1 + 1.4 * (wy * 5.3 - phase * 0.11).cos() + phase * 0.08).sin();
            let grain = 0.5 + 0.5 * (fx * 117.1 + fy * 83.7 + ((fx + fy) * 31.0).sin()).sin();
            let mut linear = [
                0.0015 + flow * 0.0017 + grain * 0.0012,
                0.0035 + flow * 0.0027 + grain * 0.0012,
                0.0110 + flow * 0.0055 + grain * 0.0012,
            ];

            // Always-on atmospheric wisps are intentionally low-energy. They provide a visual
            // substrate without pretending missing telemetry exists.
            let wisp_a = (-(wy - 0.20 * (wx * 2.8 + phase * 0.08).sin()).powi(2) / 0.16f64.powi(2))
                .exp()
                * (0.5 + 0.5 * (wx * 8.5 - phase * 0.12).sin()).powf(8.0);
            let wisp_b = (-(wx + 0.18 * (wy * 3.1 - phase * 0.06).sin()).powi(2) / 0.19f64.powi(2))
                .exp()
                * (0.5 + 0.5 * (wy * 9.2 + phase * 0.10).cos()).powf(10.0);
            add_light(&mut linear, violet, wisp_a * 0.020);
            add_light(&mut linear, teal, wisp_b * 0.012);

            // Wi-Fi: slow contour field + displaced RF-change ripple source.
            if wifi_strength > 0.0 {
                let field = (wx * 11.5 + wy * 3.2 + phase * 0.13 + noise * 2.3).sin()
                    + 0.8 * (wy * 10.2 - wx * 2.4 - phase * 0.09 + wifi_density * 1.1).cos();
                let contour = (0.5 + 0.5 * (field * std::f64::consts::PI * 1.45).cos()).powf(18.0);
                let elliptical_radius = ((wx * 0.88).powi(2) + (wy * 1.05).powi(2)).sqrt();
                let envelope = (-(elliptical_radius / 0.78).powf(4.0)).exp();
                add_light(
                    &mut linear,
                    blue,
                    contour * envelope * (0.025 + 0.15 * wifi_strength),
                );

                let ripple_x = wx - (-0.18 + 0.22 * wifi);
                let ripple_y = wy - (0.12 - 0.20 * noise);
                let ripple_radius = (ripple_x * ripple_x + ripple_y * ripple_y).sqrt();
                let ripple = (0.5
                    + 0.5
                        * (ripple_radius * (26.0 + 11.0 * wifi_density)
                            - phase * (0.18 + 0.45 * wifi_strength)
                            + (angle * 3.0).sin() * 0.7)
                            .cos())
                .powf(14.0);
                add_light(
                    &mut linear,
                    cyan,
                    ripple * envelope * (0.012 + 0.070 * wifi_strength),
                );
            }

            // Audio: asymmetric virtual sources create interference surfaces instead of generic
            // bars. A diagonal ridge behaves as a soft reverberation tail.
            if audio_strength > 0.0 {
                let d1 = ((wx + 0.30).powi(2) + (wy - 0.07).powi(2)).sqrt();
                let d2 = ((wx - 0.16).powi(2) + (wy + 0.17).powi(2)).sqrt();
                let d3 = ((wx - 0.38).powi(2) + (wy - 0.25).powi(2)).sqrt();
                let frequency = 23.0 + 21.0 * centroid;
                let interference = (0.5
                    + 0.5 * ((d1 - d2) * frequency + phase * (1.05 + 0.65 * audio)).sin())
                .powf(6.0);
                let interference_b = (0.5
                    + 0.5 * ((d2 - d3) * frequency * 0.72 - phase * (0.63 + 0.40 * audio)).cos())
                .powf(8.0);
                let cloud = (-(wy + 0.04 + 0.06 * (wx * 5.0 + phase * 0.17).sin()).powi(2)
                    / 0.31f64.powi(2))
                .exp()
                    * (-(wx / 0.78).powf(6.0)).exp();
                add_light(
                    &mut linear,
                    teal,
                    interference * cloud * (0.018 + 0.16 * audio_strength),
                );
                add_light(
                    &mut linear,
                    violet,
                    interference_b * cloud * (0.012 + 0.09 * audio_strength),
                );

                let ridge = wy + 0.21 * wx + 0.23 * (wx * 2.4 + phase * 0.16).sin();
                let tail = (-ridge.abs() / (0.04 + 0.04 * (1.0 - audio))).exp()
                    * (0.5 + 0.5 * (wx * 10.0 - phase * 0.16).sin()).powf(4.0);
                add_light(&mut linear, teal, tail * (0.008 + 0.055 * audio_strength));
            }

            // Vision-derived presence is a refractive void: recognizable as a disturbance, but it
            // never impersonates a raw frame or a body silhouette.
            if camera_strength > 0.0 {
                let px = wx - (-0.08 + 0.12 * (phase * 0.09).sin());
                let py = wy - 0.01;
                let body = (-((px / (0.13 + 0.05 * camera)).powi(2)
                    + (py / (0.34 + 0.07 * camera)).powf(4.0)))
                .exp();
                let shoulder = (-((px + 0.03) / (0.28 + 0.04 * camera)).powf(4.0)
                    - ((py + 0.06) / 0.16).powi(2))
                .exp();
                let presence = body.max(shoulder * 0.52);
                for channel in &mut linear {
                    *channel *= 1.0 - presence * (0.11 + 0.16 * camera);
                }
                let shape =
                    (px / (0.17 + 0.04 * camera)).powi(2) + (py / (0.36 + 0.04 * camera)).powi(2);
                let rim = (-(shape - 1.0).abs() / 0.085).exp();
                add_light(&mut linear, rose, rim * (0.010 + 0.065 * camera_strength));
            }

            // VAD is explicitly heuristic, so it appears only as a transient pulse rather than a
            // semantic label such as "speech".
            if vad_strength > 0.0 {
                let vx = wx + 0.12;
                let vy = wy - 0.09;
                let voice_radius = (vx * vx + vy * vy).sqrt();
                let pulse = (-(voice_radius / 0.23).powi(2)).exp()
                    * (0.25
                        + 0.75
                            * (0.5 + 0.5 * (phase * 3.0 - voice_radius * 28.0 + vx * 7.0).sin())
                                .powf(8.0));
                add_light(&mut linear, rose, pulse * (0.015 + 0.11 * vad_strength));
            }

            // Bluetooth: discrete pseudonymous cluster count becomes orbiting points. Aggregate
            // mean RSSI controls halo size and orbit radius; no device identity is implied.
            for index in 0..ble_count {
                let index_f = index as f64;
                let orbit_angle = phase * (0.18 + 0.027 * index_f)
                    + index_f * 2.399_963
                    + 0.55 * (index_f * 1.77).sin();
                let orbit_radius = 0.18
                    + 0.055 * (index % 3) as f64
                    + 0.10 * (1.0 - ble_rssi)
                    + 0.025 * (index_f * 2.1 + phase * 0.07).sin();
                let node_x = orbit_radius * orbit_angle.cos() - 0.05;
                let node_y = orbit_radius * 0.78 * orbit_angle.sin() + 0.04;
                let distance = ((wx - node_x).powi(2) + (wy - node_y).powi(2)).sqrt();
                let glow = (-(distance / (0.026 + 0.020 * (1.0 - ble_rssi))).powi(2)).exp();
                let core = (-(distance / 0.008).powi(2)).exp();
                add_light(
                    &mut linear,
                    amber,
                    glow * (0.025 + 0.090 * ble_strength) + core * (0.10 + 0.26 * ble_strength),
                );

                let angular_distance = (angle - orbit_angle)
                    .sin()
                    .atan2((angle - orbit_angle).cos())
                    .abs();
                let trail = (-((radius - orbit_radius) / 0.017).powi(2)).exp()
                    * (-angular_distance / 0.48).exp();
                add_light(&mut linear, amber, trail * (0.006 + 0.028 * ble_strength));
            }

            // A small off-center aperture remains the recognisable Liminal threshold. It is kept
            // subordinate to sensor-driven layers so the composition does not collapse into a logo.
            let aperture_x = wx + 0.03;
            let aperture_y = wy - 0.02;
            let aperture_radius =
                ((aperture_x * 1.08).powi(2) + (aperture_y * 0.94).powi(2)).sqrt();
            let aperture = (-(aperture_radius / 0.085).powf(4.0)).exp();
            for channel in &mut linear {
                *channel *= 1.0 - aperture * 0.26;
            }
            let rim = (-(aperture_radius - 0.092).abs() / 0.008).exp();
            add_light(&mut linear, teal, rim * 0.095);

            *image.get_pixel_mut(x, y) = finish_pixel(linear);
        }
    }

    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_telemetry() -> TelemetrySnapshot {
        TelemetrySnapshot {
            camera_presence: Some(0.72),
            audio_rms: Some(0.18),
            audio_centroid_hz: Some(2800.0),
            audio_vad: Some(0.64),
            wifi_rssi_mean: Some(-48.0),
            wifi_noise_mean: Some(-86.0),
            wifi_network_count: Some(7.0),
            bluetooth_cluster_count: Some(4.0),
            bluetooth_mean_rssi: Some(-58.0),
        }
    }

    #[test]
    fn output_dimensions_are_stable_for_zero_sizes() {
        let frame = spectral_frame(0, 0, 0, &TelemetrySnapshot::default());
        assert_eq!((frame.width(), frame.height()), (1, 1));
    }

    #[test]
    fn quiet_field_keeps_a_low_energy_artistic_substrate() {
        let frame = spectral_frame(96, 64, 0, &TelemetrySnapshot::default());
        let brightest = frame
            .pixels()
            .map(|pixel| pixel[0].max(pixel[1]).max(pixel[2]))
            .max()
            .unwrap_or(0);
        let average = frame
            .pixels()
            .map(|pixel| u64::from(pixel[0]) + u64::from(pixel[1]) + u64::from(pixel[2]))
            .sum::<u64>()
            / (frame.width() as u64 * frame.height() as u64 * 3);
        assert!(
            brightest > 30,
            "idle field should still feel intentionally alive"
        );
        assert!(
            average < 55,
            "idle substrate must not masquerade as live evidence"
        );
    }

    #[test]
    fn live_feature_values_change_the_rendered_frame() {
        let empty = spectral_frame(32, 16, 0, &TelemetrySnapshot::default());
        let live = spectral_frame(32, 16, 0, &demo_telemetry());
        assert_ne!(empty.into_raw(), live.into_raw());
    }

    #[test]
    fn every_derived_telemetry_family_changes_the_field() {
        let baseline = spectral_frame(48, 32, 7, &TelemetrySnapshot::default()).into_raw();
        let variants = [
            TelemetrySnapshot {
                camera_presence: Some(1.0),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                audio_rms: Some(0.2),
                audio_centroid_hz: Some(2400.0),
                audio_vad: Some(0.7),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                wifi_rssi_mean: Some(-45.0),
                wifi_noise_mean: Some(-82.0),
                wifi_network_count: Some(8.0),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                bluetooth_cluster_count: Some(5.0),
                bluetooth_mean_rssi: Some(-55.0),
                ..TelemetrySnapshot::default()
            },
        ];

        for telemetry in variants {
            assert_ne!(
                baseline,
                spectral_frame(48, 32, 7, &telemetry).into_raw(),
                "each sensor family needs its own visible visual consequence"
            );
        }
    }

    #[test]
    fn wifi_network_density_has_an_independent_visual_consequence() {
        let sparse = TelemetrySnapshot {
            wifi_network_count: Some(1.0),
            ..TelemetrySnapshot::default()
        };
        let dense = TelemetrySnapshot {
            wifi_network_count: Some(12.0),
            ..TelemetrySnapshot::default()
        };
        assert_ne!(
            spectral_frame(64, 40, 5, &sparse).into_raw(),
            spectral_frame(64, 40, 5, &dense).into_raw()
        );
    }

    #[test]
    fn bluetooth_rssi_has_an_independent_visual_consequence() {
        let weak = TelemetrySnapshot {
            bluetooth_cluster_count: Some(4.0),
            bluetooth_mean_rssi: Some(-92.0),
            ..TelemetrySnapshot::default()
        };
        let strong = TelemetrySnapshot {
            bluetooth_cluster_count: Some(4.0),
            bluetooth_mean_rssi: Some(-35.0),
            ..TelemetrySnapshot::default()
        };
        assert_ne!(
            spectral_frame(64, 40, 5, &weak).into_raw(),
            spectral_frame(64, 40, 5, &strong).into_raw()
        );
    }

    #[test]
    fn live_field_contains_multiple_visual_accents() {
        let frame = spectral_frame(96, 64, 11, &demo_telemetry());
        let cyan_like = frame
            .pixels()
            .filter(|pixel| {
                u16::from(pixel[1]) > 70 && u16::from(pixel[2]) > u16::from(pixel[0]) + 15
            })
            .count();
        let warm_like = frame
            .pixels()
            .filter(|pixel| {
                u16::from(pixel[0]) > 90 && u16::from(pixel[0]) > u16::from(pixel[2]) + 10
            })
            .count();
        assert!(cyan_like > 30, "live field lost its cool contour language");
        assert!(
            warm_like > 2,
            "Bluetooth/presence accents became visually invisible"
        );
    }
}
