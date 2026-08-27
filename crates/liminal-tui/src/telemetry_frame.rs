//! A telemetry-driven spectral artwork. It renders derived feature values only; this is not a
//! camera preview and never needs raw audio, video, Wi-Fi identifiers, or Bluetooth identities.

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

/// Lift a present signal through a perceptual curve. `None` and an explicit zero remain dark,
/// while weak-but-real observations get enough energy to survive terminal scaling.
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
/// the hue and lightness intentional instead of clipping individual channels into brown/gray.
fn oklch_to_linear_srgb(lightness: f64, chroma: f64, hue_degrees: f64) -> [f64; 3] {
    let hue = hue_degrees.to_radians();
    let a = hue.cos();
    let b = hue.sin();

    let convert = |chroma: f64| {
        let l = lightness + 0.3963377774 * chroma * a + 0.2158037573 * chroma * b;
        let m = lightness - 0.1055613458 * chroma * a - 0.0638541728 * chroma * b;
        let s = lightness - 0.0894841775 * chroma * a - 1.2914855480 * chroma * b;
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

/// Accumulate light in linear RGB. Tone mapping happens once at the end, avoiding the muddy
/// result that comes from repeatedly adding already gamma-encoded sRGB channel values.
fn add_light(target: &mut [f64; 3], color: [f64; 3], amount: f64) {
    let amount = amount.max(0.0);
    for (channel, light) in target.iter_mut().zip(color) {
        *channel += light * amount;
    }
}

fn finish_pixel(mut linear: [f64; 3]) -> Rgb<u8> {
    // Filmic exposure preserves dark structure while ensuring luminous ribbons reach the bright
    // end of the terminal's sRGB range. A small chroma lift keeps overlapping colors distinct.
    for channel in &mut linear {
        *channel = 1.0 - (-*channel * 2.25).exp();
    }
    let luma = linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
    for channel in &mut linear {
        *channel = luma + (*channel - luma) * 1.14;
    }
    Rgb([
        linear_to_srgb(linear[0]),
        linear_to_srgb(linear[1]),
        linear_to_srgb(linear[2]),
    ])
}

#[derive(Clone, Copy)]
struct Signal {
    strength: f64,
    tension: f64,
    hue: f64,
}

/// Draw a slowly moving, feature-shaped field as one abstract composition.
///
/// The five modalities become five chromatic orbiting ribbons around a dark aperture rather than
/// five independent horizontal bars. A permanent low-energy substrate keeps the field beautiful
/// when the ledger is quiet; telemetry controls the visible ribbons' phase, shape, and luminosity.
/// Missing values never create a sensor-colored signal by themselves.
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
    let wifi_strength = (wifi * 0.75 + wifi_density * 0.25)
        .max(if telemetry.wifi_noise_mean.is_some() {
            0.08
        } else {
            0.0
        })
        .clamp(0.0, 1.0);
    let ble_clusters = bar(telemetry.bluetooth_cluster_count, 0.0, 12.0);
    let ble_rssi = bar(telemetry.bluetooth_mean_rssi, -100.0, -20.0);
    let ble = (ble_clusters * 0.75 + ble_rssi * 0.25).clamp(0.0, 1.0);
    let vad = telemetry.audio_vad.unwrap_or(0.0).clamp(0.0, 1.0);
    let camera = telemetry.camera_presence.unwrap_or(0.0).clamp(0.0, 1.0);
    let audio_strength = audio.max(if telemetry.audio_centroid_hz.is_some() {
        0.08
    } else {
        0.0
    });
    let signals = [
        Signal {
            strength: visual_strength(camera),
            tension: 0.15,
            hue: 350.0,
        },
        Signal {
            strength: visual_strength(audio_strength),
            tension: centroid,
            hue: 174.0,
        },
        Signal {
            strength: visual_strength(wifi_strength),
            tension: noise + wifi_density * 0.15,
            hue: 218.0,
        },
        Signal {
            strength: visual_strength(ble),
            tension: 1.0 - wifi,
            hue: 62.0,
        },
        Signal {
            strength: visual_strength(vad),
            tension: audio,
            hue: 321.0,
        },
    ];
    let signal_colors =
        signals.map(|signal| oklch_to_linear_srgb(0.65 + signal.strength * 0.16, 0.22, signal.hue));
    let substrate_colors = [
        oklch_to_linear_srgb(0.58, 0.20, 185.0),
        oklch_to_linear_srgb(0.56, 0.20, 247.0),
        oklch_to_linear_srgb(0.59, 0.20, 315.0),
    ];
    let phase = tick as f64 * 0.035;
    let aspect = width as f64 / height as f64;
    let mut image = RgbImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let fx = x as f64 / width as f64;
            let fy = y as f64 / height as f64;
            let dx = (fx - 0.5) * aspect;
            let dy = fy - 0.5;
            let radius = (dx * dx + dy * dy).sqrt();
            let angle = dy.atan2(dx);

            // Deep navy base plus an always-on chromatic substrate. This is an aesthetic layer,
            // not sensor evidence; its job is to keep an idle field intentional and alive.
            let drift = ((dx * 4.8 + phase).sin() + (dy * 5.6 - phase * 0.7).cos()) * 0.5 + 0.5;
            let grain = ((fx * 37.0 + fy * 19.0 + phase * 3.0).sin() * 0.5 + 0.5) * 0.004;
            let mut linear = [
                0.002 + drift * 0.002 + grain,
                0.006 + drift * 0.004 + grain,
                0.016 + drift * 0.008 + grain * 1.4,
            ];
            let filament_a =
                ((angle * 2.1 + radius * 21.0 - phase * 0.8).sin() * 0.5 + 0.5).powf(13.0) * 0.18;
            let filament_b =
                ((angle * 3.0 - radius * 15.0 + phase * 0.6).sin() * 0.5 + 0.5).powf(17.0) * 0.14;
            let halo = (-(radius - 0.31).powi(2) / 0.055).exp() * 0.035;
            add_light(&mut linear, substrate_colors[0], filament_a);
            add_light(&mut linear, substrate_colors[1], filament_b);
            add_light(&mut linear, substrate_colors[2], halo);

            // Five chromatic ribbons share the same center but occupy different orbital phases.
            for (index, (signal, color)) in signals.iter().zip(signal_colors).enumerate() {
                let orbit_phase = phase * (0.8 + index as f64 * 0.07)
                    + index as f64 * std::f64::consts::TAU / 5.0
                    + signal.tension * 1.8;
                let orbit_radius =
                    0.19 + signal.strength * 0.13 + (index as f64 * 0.011).sin() * 0.025;
                let orbit_x = orbit_radius * orbit_phase.cos();
                let orbit_y = orbit_radius * orbit_phase.sin() * 0.82;
                let ribbon_x = dx - orbit_x;
                let ribbon_y = dy - orbit_y;
                let ribbon_distance = (ribbon_x * ribbon_x + ribbon_y * ribbon_y).sqrt();
                let ribbon = (-ribbon_distance * ribbon_distance / 0.013).exp();

                let petal_angle = index as f64 * std::f64::consts::TAU / 5.0 - phase * 0.45;
                let angular_distance = (angle - petal_angle).sin().abs();
                let petal_radius = 0.23 + signal.strength * 0.18;
                let petal = (-(radius - petal_radius).abs() / 0.035).exp()
                    * (1.0 - angular_distance).powf(2.0)
                    * (0.12 + signal.strength * 0.88);

                let contour =
                    (((ribbon_distance * 34.0 + phase * 2.0 + signal.tension).sin() * 0.5 + 0.5)
                        .powf(12.0))
                        * (0.08 + signal.strength * 0.4);
                let amount = ribbon * (0.10 + signal.strength * 0.72)
                    + petal * (0.15 + signal.strength * 0.32)
                    + contour * 1.25;
                add_light(&mut linear, color, amount);
            }

            // The aperture is the quiet center of the piece. A thin teal rim gives the
            // composition a recognizable Liminal threshold even when all sensors are idle.
            let aperture = (-radius * radius / 0.010).exp();
            for channel in &mut linear {
                *channel *= 1.0 - aperture * 0.32;
            }
            let rim = (-(radius - 0.115).abs() / 0.012).exp();
            add_light(&mut linear, substrate_colors[0], rim * 0.17);

            *image.get_pixel_mut(x, y) = finish_pixel(linear);
        }
    }

    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_dimensions_are_stable_for_zero_sizes() {
        let frame = spectral_frame(0, 0, 0, &TelemetrySnapshot::default());
        assert_eq!((frame.width(), frame.height()), (1, 1));
    }

    #[test]
    fn quiet_field_keeps_a_vibrant_artistic_substrate() {
        let frame = spectral_frame(96, 64, 0, &TelemetrySnapshot::default());
        assert!(frame.pixels().any(|pixel| pixel[1] > 70 && pixel[2] > 100));
        assert!(frame
            .pixels()
            .any(|pixel| pixel[0] > 40 && pixel[2] > pixel[0]));
    }

    #[test]
    fn live_feature_values_change_the_rendered_frame() {
        let empty = spectral_frame(32, 16, 0, &TelemetrySnapshot::default());
        let live = spectral_frame(
            32,
            16,
            0,
            &TelemetrySnapshot {
                camera_presence: None,
                audio_rms: Some(0.2),
                audio_centroid_hz: Some(2400.0),
                audio_vad: Some(0.8),
                wifi_rssi_mean: Some(-45.0),
                wifi_noise_mean: Some(-85.0),
                wifi_network_count: Some(5.0),
                bluetooth_cluster_count: Some(3.0),
                bluetooth_mean_rssi: Some(-55.0),
            },
        );
        assert_ne!(empty.into_raw(), live.into_raw());
    }

    #[test]
    fn camera_presence_changes_the_rendered_frame() {
        let absent = spectral_frame(
            32,
            16,
            0,
            &TelemetrySnapshot {
                camera_presence: Some(0.0),
                ..TelemetrySnapshot::default()
            },
        );
        let present = spectral_frame(
            32,
            16,
            0,
            &TelemetrySnapshot {
                camera_presence: Some(1.0),
                ..TelemetrySnapshot::default()
            },
        );
        assert_ne!(absent.into_raw(), present.into_raw());
    }

    #[test]
    fn every_derived_telemetry_value_can_change_the_rendered_frame() {
        let variants = [
            TelemetrySnapshot {
                camera_presence: Some(1.0),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                audio_rms: Some(0.2),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                audio_centroid_hz: Some(2400.0),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                audio_vad: Some(0.8),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                wifi_rssi_mean: Some(-45.0),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                wifi_noise_mean: Some(-85.0),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                wifi_network_count: Some(5.0),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                bluetooth_cluster_count: Some(3.0),
                ..TelemetrySnapshot::default()
            },
            TelemetrySnapshot {
                bluetooth_mean_rssi: Some(-55.0),
                ..TelemetrySnapshot::default()
            },
        ];
        let baseline = spectral_frame(32, 16, 0, &TelemetrySnapshot::default()).into_raw();
        for variant in variants {
            assert_ne!(baseline, spectral_frame(32, 16, 0, &variant).into_raw());
        }
    }
}
