//! A synthetic animated frame generator, used only to prove real bitmap/video rendering works
//! over the terminal graphics protocol (Kitty/Sixel via `ratatui-image`) before any real sensor
//! feed exists. This is explicitly labeled as a demo everywhere it's shown -- master plan §146
//! ("Demo Honesty") applies here just as much as to sensor data: a generated plasma pattern must
//! never be presented as if it were a camera feed.

use image::{Rgb, RgbImage};

/// Deterministic given `(width, height, tick)` -- a pure function so the animation logic is
/// unit-testable without a terminal.
pub fn plasma_frame(width: u32, height: u32, tick: u32) -> RgbImage {
    let mut img = RgbImage::new(width, height);
    let t = tick as f32 * 0.15;
    for y in 0..height {
        for x in 0..width {
            let fx = x as f32 / width.max(1) as f32;
            let fy = y as f32 / height.max(1) as f32;
            let v = (fx * 6.0 + t).sin()
                + (fy * 6.0 + t * 1.3).sin()
                + ((fx + fy) * 6.0 + t * 0.7).sin();
            let normalized = (v + 3.0) / 6.0; // v is in [-3, 3]
            let r = (normalized * 255.0) as u8;
            let g = ((1.0 - normalized) * 255.0) as u8;
            let b = ((0.5 + 0.5 * (t + fx * fy).cos()) * 255.0) as u8;
            img.put_pixel(x, y, Rgb([r, g, b]));
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_an_image_of_the_requested_dimensions() {
        let frame = plasma_frame(32, 16, 0);
        assert_eq!(frame.width(), 32);
        assert_eq!(frame.height(), 16);
    }

    #[test]
    fn is_deterministic_for_the_same_tick() {
        let a = plasma_frame(16, 16, 5);
        let b = plasma_frame(16, 16, 5);
        assert_eq!(a.into_raw(), b.into_raw());
    }

    #[test]
    fn differs_across_ticks_so_the_animation_actually_animates() {
        let a = plasma_frame(16, 16, 0);
        let b = plasma_frame(16, 16, 10);
        assert_ne!(a.into_raw(), b.into_raw());
    }

    #[test]
    fn handles_a_1x1_frame_without_dividing_by_zero() {
        let frame = plasma_frame(1, 1, 0);
        assert_eq!(frame.width(), 1);
        assert_eq!(frame.height(), 1);
    }
}
