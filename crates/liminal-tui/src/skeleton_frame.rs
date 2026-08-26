//! Renders real Vision-derived joint positions as a skeleton image -- see `ledger_view.rs` for
//! why this exists instead of a camera-pixel view (raw frames are never persisted or
//! transmitted).

use image::{Rgb, RgbImage};

/// Vision's normalized coordinates have origin at bottom-left with y increasing upward; images
/// are addressed top-left with row increasing downward. This is the one place that conversion
/// happens, so it's covered by a test rather than embedded silently in the render loop.
fn to_pixel(x: f64, y: f64, width: u32, height: u32) -> (i64, i64) {
    let px = (x * width as f64).round() as i64;
    let py = ((1.0 - y) * height as f64).round() as i64;
    (px, py)
}

/// Draws a small filled square centered at each joint above `confidence_floor`, on a black
/// background. This is deliberately not a photographic reconstruction -- it's a sparse mark
/// (§101: OBSERVED layer = "sharp / thin / high-frequency marks"), matching the epistemic-honesty
/// visual grammar the rest of the project already uses.
pub fn skeleton_frame(
    width: u32,
    height: u32,
    joints: &[(f64, f64, f64)],
    confidence_floor: f64,
) -> RgbImage {
    let mut img = RgbImage::new(width.max(1), height.max(1));
    let dot_color = Rgb([0, 255, 180]);
    let half_size: i64 = 2;

    for &(x, y, confidence) in joints {
        if confidence < confidence_floor {
            continue;
        }
        let (cx, cy) = to_pixel(x, y, width, height);
        for dx in -half_size..=half_size {
            for dy in -half_size..=half_size {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && py >= 0 && (px as u32) < width && (py as u32) < height {
                    img.put_pixel(px as u32, py as u32, dot_color);
                }
            }
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_an_image_of_the_requested_dimensions() {
        let frame = skeleton_frame(64, 32, &[], 0.25);
        assert_eq!(frame.width(), 64);
        assert_eq!(frame.height(), 32);
    }

    #[test]
    fn joints_below_the_confidence_floor_are_not_drawn() {
        let frame = skeleton_frame(20, 20, &[(0.5, 0.5, 0.1)], 0.25);
        // Every pixel should remain black (the default RgbImage fill) since the only joint was
        // filtered out.
        assert!(frame.pixels().all(|p| *p == Rgb([0, 0, 0])));
    }

    #[test]
    fn a_joint_above_the_floor_is_drawn_somewhere_in_the_frame() {
        let frame = skeleton_frame(20, 20, &[(0.5, 0.5, 0.9)], 0.25);
        assert!(frame.pixels().any(|p| *p == Rgb([0, 255, 180])));
    }

    #[test]
    fn to_pixel_flips_the_y_axis_from_visions_bottom_left_origin() {
        // Vision's y=0 is the bottom of the frame; image row 0 is the top. y=0 should map near
        // the bottom (high row index), y=1 near the top (row 0).
        let (_, bottom_row) = to_pixel(0.5, 0.0, 100, 100);
        let (_, top_row) = to_pixel(0.5, 1.0, 100, 100);
        assert!(bottom_row > top_row);
    }

    #[test]
    fn handles_a_1x1_frame_without_panicking() {
        let frame = skeleton_frame(1, 1, &[(0.5, 0.5, 0.9)], 0.25);
        assert_eq!((frame.width(), frame.height()), (1, 1));
    }

    #[test]
    fn handles_a_zero_sized_request_by_falling_back_to_1x1() {
        let frame = skeleton_frame(0, 0, &[], 0.25);
        assert_eq!((frame.width(), frame.height()), (1, 1));
    }
}
