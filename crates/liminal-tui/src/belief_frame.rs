use crate::belief::{BeliefSnapshot, BeliefState};
use image::{Rgb, RgbImage};

const CYAN: [f64; 3] = [0.05, 0.55, 0.88];
const TEAL: [f64; 3] = [0.04, 0.72, 0.58];
const ICE: [f64; 3] = [0.48, 0.82, 0.94];
const ROSE: [f64; 3] = [0.92, 0.18, 0.48];
const AMBER: [f64; 3] = [0.98, 0.52, 0.12];
const VIOLET: [f64; 3] = [0.34, 0.19, 0.72];

fn add_light(target: &mut [f64; 3], color: [f64; 3], amount: f64) {
    for (channel, light) in target.iter_mut().zip(color) {
        *channel += light * amount.max(0.0);
    }
}

fn encode(value: f64) -> u8 {
    let value = value.clamp(0.0, 1.0);
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

/// Render belief as an uncertainty volume rather than a progress meter. Probability controls the
/// occupied volume, confidence controls edge coherence, disagreement splits the field into
/// competing chromatic lobes, sensor health controls halo stability, and observed modalities are
/// represented as evidence nodes around the perimeter.
pub fn belief_frame(width: u32, height: u32, tick: u32, belief: BeliefSnapshot) -> RgbImage {
    let width = width.max(1);
    let height = height.max(1);
    let probability = belief.occupancy_probability.clamp(0.0, 1.0);
    let confidence = belief.confidence.clamp(0.0, 1.0);
    let disagreement = belief.disagreement.clamp(0.0, 1.0);
    let health = belief.sensor_health.clamp(0.0, 1.0);
    let contested = belief.state == BeliefState::Contested;
    let phase = tick as f64 * 0.04;
    let aspect = width as f64 / height as f64;
    let base_radius = 0.10 + probability * 0.31;
    let edge_width = 0.020 + 0.075 * (1.0 - confidence) + 0.030 * disagreement;
    let center_x = 0.035 * (phase * 0.31).sin() * disagreement;
    let center_y = 0.025 * (phase * 0.23).cos() * (1.0 - health);
    let modality_count = belief.observed_modalities.min(6) as usize;

    let mut image = RgbImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let fx = (x as f64 + 0.5) / width as f64;
            let fy = (y as f64 + 0.5) / height as f64;
            let dx = (fx - 0.5) * aspect - center_x;
            let dy = fy - 0.5 - center_y;
            let radius = (dx * dx + dy * dy).sqrt();
            let angle = dy.atan2(dx);
            let grain = 0.5 + 0.5 * (fx * 91.3 + fy * 73.1 + ((fx - fy) * 27.0).sin()).sin();
            let mut linear = [
                0.0016 + grain * 0.0012,
                0.0035 + grain * 0.0012,
                0.0100 + grain * 0.0012,
            ];

            let ambient_radius = base_radius + 0.14;
            let ambient_width = 0.18 + 0.04 * (1.0 - health);
            let ambient = (-((radius - ambient_radius) / ambient_width).powi(2)).exp();
            add_light(&mut linear, VIOLET, ambient * 0.012);

            let mut lobed_radius = base_radius
                * (1.0
                    + 0.070 * (angle * 3.0 + phase * 0.25).sin()
                    + 0.045 * (angle * 5.0 - phase * 0.17).sin());
            if contested {
                lobed_radius *= 1.0 + 0.12 * disagreement * (angle * 2.0 + phase * 0.42).sin();
            }

            let signed_distance = (lobed_radius - radius) / edge_width.max(0.006);
            let fill = 1.0 / (1.0 + (-signed_distance * 3.4).exp());
            let core = (-(radius / (base_radius * 0.52).max(0.015)).powi(2)).exp() * fill;
            let shell = (-(radius - lobed_radius).abs() / (edge_width * 0.72).max(0.007)).exp();
            let iso = (0.5 + 0.5 * ((radius / lobed_radius.max(0.02)) * 20.0 - phase * 0.18).cos())
                .powf(18.0)
                * fill;

            if contested {
                let left_distance =
                    ((dx + 0.07 * disagreement).powi(2) + (dy - 0.02).powi(2)).sqrt();
                let right_distance =
                    ((dx - 0.08 * disagreement).powi(2) + (dy + 0.025).powi(2)).sqrt();
                let lobe_radius = (base_radius * 0.82).max(0.03);
                let left = (-(left_distance / lobe_radius).powi(2)).exp() * fill;
                let right = (-(right_distance / lobe_radius).powi(2)).exp() * fill;
                add_light(&mut linear, ROSE, left * (0.045 + 0.19 * disagreement));
                add_light(&mut linear, AMBER, right * (0.040 + 0.16 * disagreement));

                let split =
                    (0.5 + 0.5 * (dx * 19.0 + dy * 8.0 + phase * 1.1).sin()).powf(8.0) * fill;
                add_light(
                    &mut linear,
                    ICE,
                    split * (0.012 + 0.065 * (1.0 - confidence)),
                );
                add_light(&mut linear, ROSE, shell * (0.018 + 0.085 * disagreement));
            } else {
                // Stable belief is intentionally translucent: nested iso-probability contours make
                // changes in volume visible and keep the view from collapsing into a flat blob.
                add_light(&mut linear, CYAN, fill * (0.020 + 0.10 * confidence));
                add_light(&mut linear, TEAL, core * (0.030 + 0.13 * confidence));
                add_light(&mut linear, ICE, shell * (0.020 + 0.10 * health));
                add_light(&mut linear, ICE, iso * (0.014 + 0.055 * confidence));
            }

            let fragment = (0.5
                + 0.5 * (angle * 7.0 - phase * 0.41 + (angle * 3.0).sin() * 1.3).sin())
            .powf(6.0);
            add_light(
                &mut linear,
                VIOLET,
                shell * fragment * (0.012 + 0.09 * (1.0 - confidence) + 0.05 * disagreement),
            );

            for index in 0..modality_count {
                let index_f = index as f64;
                let node_angle = index_f * 2.399_963 + phase * 0.13;
                let node_radius =
                    base_radius + 0.055 + 0.012 * (index_f * 2.1 + phase * 0.20).sin();
                let node_x = node_radius * node_angle.cos();
                let node_y = node_radius * 0.90 * node_angle.sin();
                let distance = ((dx - node_x).powi(2) + (dy - node_y).powi(2)).sqrt();
                let glow = (-(distance / 0.025).powi(2)).exp();
                let dot = (-(distance / 0.007).powi(2)).exp();
                let color = if contested && index % 2 == 1 {
                    AMBER
                } else {
                    TEAL
                };
                add_light(
                    &mut linear,
                    color,
                    glow * 0.055 * health + dot * 0.26 * health,
                );
            }

            let aperture = (-(radius / 0.048).powf(4.0)).exp();
            for channel in &mut linear {
                *channel *= 1.0 - aperture * 0.22;
            }

            *image.get_pixel_mut(x, y) = finish_pixel(linear);
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(
        probability: f64,
        confidence: f64,
        disagreement: f64,
        state: BeliefState,
    ) -> BeliefSnapshot {
        BeliefSnapshot {
            occupancy_probability: probability,
            confidence,
            disagreement,
            observed_modalities: 4,
            sensor_health: 0.85,
            state,
        }
    }

    #[test]
    fn belief_probability_changes_the_volume() {
        let empty = belief_frame(32, 16, 0, snapshot(0.0, 1.0, 0.0, BeliefState::Stable));
        let occupied = belief_frame(32, 16, 0, snapshot(1.0, 1.0, 0.0, BeliefState::Stable));
        assert_ne!(empty.into_raw(), occupied.into_raw());
    }

    #[test]
    fn contested_belief_has_a_distinct_visual_language() {
        let stable = belief_frame(64, 40, 9, snapshot(0.6, 0.6, 0.7, BeliefState::Stable));
        let contested = belief_frame(64, 40, 9, snapshot(0.6, 0.6, 0.7, BeliefState::Contested));
        assert_ne!(stable.into_raw(), contested.into_raw());
    }

    #[test]
    fn evidence_modality_count_is_visible() {
        let mut low = snapshot(0.7, 0.8, 0.1, BeliefState::Stable);
        low.observed_modalities = 1;
        let mut high = low;
        high.observed_modalities = 5;
        assert_ne!(
            belief_frame(64, 40, 4, low).into_raw(),
            belief_frame(64, 40, 4, high).into_raw()
        );
    }
}
