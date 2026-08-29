use crate::ledger_view::RecentObservation;
use image::{Rgb, RgbImage};

const CAMERA: [f64; 3] = [0.04, 0.70, 0.48];
const MICROPHONE: [f64; 3] = [0.70, 0.10, 0.58];
const WIFI: [f64; 3] = [0.04, 0.46, 0.90];
const BLUETOOTH: [f64; 3] = [0.95, 0.50, 0.06];
const OTHER: [f64; 3] = [0.28, 0.34, 0.42];
const NOW: [f64; 3] = [0.32, 0.82, 0.78];
const LANES: usize = 5;
const DENSITY_BINS: usize = 96;

fn lane_for_stream(stream: &str) -> usize {
    match stream {
        "camera" => 0,
        "microphone" => 1,
        "wifi" => 2,
        "bluetooth" => 3,
        _ => 4,
    }
}

fn color_for_lane(lane: usize) -> [f64; 3] {
    match lane {
        0 => CAMERA,
        1 => MICROPHONE,
        2 => WIFI,
        3 => BLUETOOTH,
        _ => OTHER,
    }
}

fn add_light(target: &mut [f64; 3], color: [f64; 3], amount: f64) {
    for (channel, light) in target.iter_mut().zip(color) {
        *channel += light * amount.max(0.0);
    }
}

fn encode(value: f64) -> u8 {
    let value = value.clamp(0.0_f64, 1.0_f64);
    let encoded = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

fn finish_pixel(mut linear: [f64; 3]) -> Rgb<u8> {
    for channel in &mut linear {
        *channel = 1.0 - (-*channel * 2.7).exp();
    }
    let luma = linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
    for channel in &mut linear {
        *channel = luma + (*channel - luma) * 1.16;
    }
    Rgb([encode(linear[0]), encode(linear[1]), encode(linear[2])])
}

fn normalized_time(timestamp_us: i64, min_timestamp: i64, span: f64) -> f64 {
    ((timestamp_us - min_timestamp) as f64 / span).clamp(0.0, 1.0)
}

/// Render timestamped observations as a luminous multi-track memory field. Events are never joined
/// into a continuous path: dark space remains dark, so a gap in the ledger is visually a gap rather
/// than an invented trajectory. Density becomes atmosphere behind discrete observation marks.
pub fn memory_frame(width: u32, height: u32, observations: &[RecentObservation]) -> RgbImage {
    let width = width.max(1);
    let height = height.max(1);
    let mut linear = vec![[0.0; 3]; width as usize * height as usize];

    // Time-grid substrate and a restrained paper-like grain.
    for y in 0..height {
        for x in 0..width {
            let fx = (x as f64 + 0.5) / width as f64;
            let fy = (y as f64 + 0.5) / height as f64;
            let grain = 0.5 + 0.5 * (fx * 101.3 + fy * 77.7 + ((fx - fy) * 29.0).sin()).sin();
            let grid_phase = (fx * 8.0).fract();
            let grid_distance = grid_phase.min(1.0 - grid_phase);
            let grid = (-(grid_distance / 0.018).powi(2)).exp();
            let pixel = &mut linear[y as usize * width as usize + x as usize];
            pixel[0] = 0.0015 + grain * 0.0011 + grid * 0.0006;
            pixel[1] = 0.0032 + grain * 0.0011 + grid * 0.0010;
            pixel[2] = 0.0095 + grain * 0.0011 + grid * 0.0013;
        }
    }

    let Some(first) = observations.first() else {
        let mut image = RgbImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let index = y as usize * width as usize + x as usize;
                *image.get_pixel_mut(x, y) = finish_pixel(linear[index]);
            }
        }
        return image;
    };

    let min_timestamp = observations
        .iter()
        .map(|observation| observation.timestamp_us)
        .min()
        .unwrap_or(first.timestamp_us);
    let max_timestamp = observations
        .iter()
        .map(|observation| observation.timestamp_us)
        .max()
        .unwrap_or(first.timestamp_us);
    let span = (max_timestamp - min_timestamp).max(1) as f64;

    // Lane baselines provide orientation without turning the visual into a spreadsheet.
    for lane in 0..LANES {
        let lane_y = (((lane as f64 + 0.5) / LANES as f64) * height as f64) as i32;
        for offset in -1..=1 {
            let y = lane_y + offset;
            if !(0..height as i32).contains(&y) {
                continue;
            }
            for x in 0..width {
                add_light(
                    &mut linear[y as usize * width as usize + x as usize],
                    color_for_lane(lane),
                    0.006,
                );
            }
        }
    }

    let mut density = [[0u32; DENSITY_BINS]; LANES];
    for observation in observations {
        let lane = lane_for_stream(&observation.stream);
        let normalized = normalized_time(observation.timestamp_us, min_timestamp, span);
        let bin = (normalized * (DENSITY_BINS - 1) as f64) as usize;
        density[lane][bin] = density[lane][bin].saturating_add(1);
    }
    let max_density = density
        .iter()
        .flat_map(|lane| lane.iter())
        .copied()
        .max()
        .unwrap_or(1)
        .max(1) as f64;

    // Density is rendered as faint vertical haze behind the actual events. It adds temporal rhythm
    // but never connects missing observations across a gap.
    for (lane, bins) in density.iter().enumerate() {
        let lane_center = (lane as f64 + 0.5) / LANES as f64;
        let color = color_for_lane(lane);
        for (bin, count) in bins.iter().copied().enumerate() {
            if count == 0 {
                continue;
            }
            let center_x =
                (bin as f64 / (DENSITY_BINS - 1) as f64) * width.saturating_sub(1) as f64;
            let amplitude = (count as f64 / max_density).powf(0.55);
            let x_start = (center_x as i32 - 3).max(0);
            let x_end = (center_x as i32 + 3).min(width as i32 - 1);
            for y in 0..height {
                let fy = (y as f64 + 0.5) / height as f64;
                let vertical = (-((fy - lane_center) / 0.055).powi(2)).exp();
                for x in x_start..=x_end {
                    let horizontal = (-((x as f64 - center_x) / 2.4).powi(2)).exp();
                    add_light(
                        &mut linear[y as usize * width as usize + x as usize],
                        color,
                        vertical * horizontal * amplitude * 0.028,
                    );
                }
            }
        }
    }

    // Discrete observations are bright, compact marks. Their slight deterministic vertical jitter
    // prevents dense bursts from collapsing into one perfect synthetic-looking row.
    for (index, observation) in observations.iter().enumerate() {
        let lane = lane_for_stream(&observation.stream);
        let color = color_for_lane(lane);
        let normalized = normalized_time(observation.timestamp_us, min_timestamp, span);
        let center_x = normalized * width.saturating_sub(1) as f64;
        let lane_center = (lane as f64 + 0.5) / LANES as f64;
        let jitter =
            0.025 * (index as f64 * 12.9898 + observation.timestamp_us as f64 * 0.000_001).sin();
        let center_y = (lane_center + jitter) * height as f64;
        let x_start = (center_x as i32 - 10).max(0);
        let x_end = (center_x as i32 + 10).min(width as i32 - 1);
        let y_start = (center_y as i32 - 10).max(0);
        let y_end = (center_y as i32 + 10).min(height as i32 - 1);

        for y in y_start..=y_end {
            for x in x_start..=x_end {
                let dx = (x as f64 - center_x) / 5.0;
                let dy = (y as f64 - center_y) / 5.0;
                let distance = (dx * dx + dy * dy).sqrt();
                let glow = (-(distance / 1.7).powi(2)).exp();
                let core = (-(distance / 0.42).powi(2)).exp();
                add_light(
                    &mut linear[y as usize * width as usize + x as usize],
                    color,
                    glow * 0.055 + core * 0.31,
                );
            }
        }
    }

    // The most recent observation is a subtle vertical shimmer, making direction of time obvious.
    let latest_x = width.saturating_sub(1) as i32;
    for x in (latest_x - 2).max(0)..=(latest_x + 2).min(width as i32 - 1) {
        let strength = (-(x - latest_x).abs() as f64 / 1.3).exp() * 0.08;
        for y in 0..height {
            add_light(
                &mut linear[y as usize * width as usize + x as usize],
                NOW,
                strength,
            );
        }
    }

    let mut image = RgbImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let index = y as usize * width as usize + x as usize;
            *image.get_pixel_mut(x, y) = finish_pixel(linear[index]);
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(stream: &str, timestamp_us: i64) -> RecentObservation {
        RecentObservation {
            stream: stream.into(),
            timestamp_us,
            kind: "observation".into(),
        }
    }

    #[test]
    fn timeline_changes_when_observations_change() {
        let empty = memory_frame(32, 16, &[]);
        let live = memory_frame(32, 16, &[observation("camera", 10)]);
        assert_ne!(empty.into_raw(), live.into_raw());
    }

    #[test]
    fn normalized_time_clamps_beyond_the_visible_window() {
        assert_eq!(normalized_time(-50, 0, 100.0), 0.0);
        assert_eq!(normalized_time(150, 0, 100.0), 1.0);
    }

    #[test]
    fn timeline_bounds_out_of_order_timestamps_to_the_image() {
        let observations = (0..4096)
            .map(|index| {
                observation(
                    "camera",
                    if index % 2 == 0 {
                        1_000_000 - index
                    } else {
                        1_000_000 + index
                    },
                )
            })
            .collect::<Vec<_>>();
        let image = memory_frame(120, 60, &observations);
        assert_eq!((image.width(), image.height()), (120, 60));
    }

    #[test]
    fn separate_streams_use_visibly_distinct_lanes() {
        let frame = memory_frame(
            96,
            64,
            &[
                observation("camera", 10),
                observation("microphone", 20),
                observation("wifi", 30),
                observation("bluetooth", 40),
            ],
        );
        let non_dark_rows = (0..frame.height())
            .filter(|y| {
                (0..frame.width()).any(|x| {
                    let pixel = frame.get_pixel(x, *y);
                    pixel[0] > 80 || pixel[1] > 80 || pixel[2] > 80
                })
            })
            .count();
        assert!(
            non_dark_rows > 8,
            "memory events collapsed into a flat single-row chart"
        );
    }

    #[test]
    fn a_temporal_gap_stays_visibly_darker_than_event_bursts() {
        let observations = [
            observation("wifi", 0),
            observation("wifi", 1),
            observation("wifi", 2),
            observation("wifi", 98),
            observation("wifi", 99),
            observation("wifi", 100),
        ];
        let frame = memory_frame(120, 60, &observations);
        let lane_y = (2.5 / LANES as f64 * frame.height() as f64) as u32;
        let gap = frame.get_pixel(frame.width() / 2, lane_y);
        let burst = frame.get_pixel(1, lane_y);
        let gap_luma = u32::from(gap[0]) + u32::from(gap[1]) + u32::from(gap[2]);
        let burst_luma = u32::from(burst[0]) + u32::from(burst[1]) + u32::from(burst[2]);
        assert!(
            gap_luma < burst_luma,
            "memory renderer interpolated across an evidence gap"
        );
    }
}
