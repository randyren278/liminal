use crate::ledger_view::RecentObservation;
use image::{Rgb, RgbImage};

/// Render a compact event-density timeline. X is append time; Y is sensor stream. Long gaps are
/// left dark instead of interpolated into a false path, preserving the ledger's uncertainty.
pub fn memory_frame(width: u32, height: u32, observations: &[RecentObservation]) -> RgbImage {
    let width = width.max(1);
    let height = height.max(1);
    let mut image = RgbImage::from_pixel(width, height, Rgb([7, 12, 19]));
    let Some(first) = observations.first() else {
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
    for observation in observations {
        let normalized = (observation.timestamp_us - min_timestamp) as f64 / span;
        let x = timeline_pixel_x(normalized, width);
        let stream_index = match observation.stream.as_str() {
            "camera" => 0,
            "microphone" => 1,
            "wifi" => 2,
            "bluetooth" => 3,
            _ => 4,
        };
        let y = ((stream_index as u32 * height) / 5).min(height - 1);
        let color = match observation.stream.as_str() {
            "camera" => [80, 220, 180],
            "microphone" => [218, 105, 190],
            "wifi" => [90, 160, 255],
            "bluetooth" => [255, 184, 92],
            _ => [150, 160, 170],
        };
        for dy in 0..3 {
            if y + dy < height {
                image.put_pixel(x, y + dy, Rgb(color));
            }
        }
    }
    image
}

fn timeline_pixel_x(normalized: f64, width: u32) -> u32 {
    (normalized.clamp(0.0, 1.0) * width.saturating_sub(1) as f64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_changes_when_observations_change() {
        let empty = memory_frame(32, 16, &[]);
        let live = memory_frame(
            32,
            16,
            &[RecentObservation {
                stream: "camera".into(),
                timestamp_us: 10,
                kind: "observation".into(),
            }],
        );
        assert_ne!(empty.into_raw(), live.into_raw());
    }

    #[test]
    fn timeline_bounds_out_of_order_timestamps_to_the_image() {
        let observations = (0..4096)
            .map(|index| RecentObservation {
                stream: "camera".into(),
                timestamp_us: if index % 2 == 0 {
                    1_000_000 - index
                } else {
                    1_000_000 + index
                },
                kind: "observation".into(),
            })
            .collect::<Vec<_>>();

        let image = memory_frame(120, 60, &observations);
        assert_eq!((image.width(), image.height()), (120, 60));
    }

    #[test]
    fn timeline_pixel_x_clamps_outside_normalized_range() {
        assert_eq!(timeline_pixel_x(1.5, 120), 119);
        assert_eq!(timeline_pixel_x(-0.5, 120), 0);
    }
}
