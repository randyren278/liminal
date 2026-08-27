use crate::belief::{BeliefSnapshot, BeliefState};
use image::{Rgb, RgbImage};

pub fn belief_frame(width: u32, height: u32, tick: u32, belief: BeliefSnapshot) -> RgbImage {
    let width = width.max(1);
    let height = height.max(1);
    let mut image = RgbImage::from_pixel(width, height, Rgb([7, 11, 20]));
    let probability = belief.occupancy_probability.clamp(0.0, 1.0);
    let confidence = belief.confidence.clamp(0.0, 1.0);
    let radius = (width.min(height) as f64 * (0.12 + probability * 0.28)) as i64;
    let cx = width as i64 / 2;
    let cy = height as i64 / 2;
    let jitter = ((tick as f64 * 0.08).sin() * belief.disagreement * 3.0) as i64;
    for y in 0..height as i64 {
        for x in 0..width as i64 {
            let distance = ((x - cx - jitter).pow(2) + (y - cy).pow(2)) as f64;
            let edge = (1.0 - distance.sqrt() / radius.max(1) as f64).clamp(0.0, 1.0);
            if edge > 0.0 {
                let alpha = edge * (0.18 + confidence * 0.72);
                let color = if belief.state == BeliefState::Contested {
                    [255.0, 140.0, 92.0]
                } else {
                    [88.0, 205.0, 255.0]
                };
                image.put_pixel(
                    x as u32,
                    y as u32,
                    Rgb([
                        (color[0] * alpha) as u8,
                        (color[1] * alpha) as u8,
                        (color[2] * alpha) as u8,
                    ]),
                );
            }
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn belief_probability_changes_the_volume() {
        let empty = belief_frame(
            32,
            16,
            0,
            BeliefSnapshot {
                occupancy_probability: 0.0,
                confidence: 1.0,
                disagreement: 0.0,
                observed_modalities: 1,
                sensor_health: 1.0,
                state: crate::belief::BeliefState::Stable,
            },
        );
        let occupied = belief_frame(
            32,
            16,
            0,
            BeliefSnapshot {
                occupancy_probability: 1.0,
                confidence: 1.0,
                disagreement: 0.0,
                observed_modalities: 1,
                sensor_health: 1.0,
                state: crate::belief::BeliefState::Stable,
            },
        );
        assert_ne!(empty.into_raw(), occupied.into_raw());
    }
}
