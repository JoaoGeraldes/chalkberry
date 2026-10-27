//! Chalk grain texture.
//!
//! Pure Rust: this module must never depend on Windows APIs. It produces a
//! small, seamlessly tiling texture that the renderer paints strokes with,
//! so lines look like chalk dragged over a rough board instead of flat ink.
//!
//! The grain is fixed to the screen (like the bumps of a real board), so
//! strokes drawn over the same spot share the same pores.

/// Width and height of the square grain texture, in pixels. It tiles, so it
/// only needs to be big enough that the repetition is not noticeable.
pub const GRAIN_TEXTURE_SIZE_PX: u32 = 256;

/// Size of the large, soft blotches (uneven chalk pressure). Must divide
/// [`GRAIN_TEXTURE_SIZE_PX`] so the texture tiles seamlessly.
const BLOTCH_CELL_PX: u32 = 16;

/// Size of the small pores (board roughness). Must divide
/// [`GRAIN_TEXTURE_SIZE_PX`] so the texture tiles seamlessly.
const PORE_CELL_PX: u32 = 4;

/// How much each noise layer contributes. They add up to 1.0.
const BLOTCH_WEIGHT: f32 = 0.2;
const PORE_WEIGHT: f32 = 0.35;
const SPECKLE_WEIGHT: f32 = 0.45;

/// Noise below this level becomes a hole in the chalk.
const HOLE_LEVEL: f32 = 0.25;

/// How gradually holes fade into solid chalk. Smaller means harsher grain.
const HOLE_SOFTNESS: f32 = 0.3;

/// Even the deepest hole keeps a little chalk dust.
const MIN_ALPHA: f32 = 0.12;

const BLOTCH_SEED: u32 = 0x9E37_79B9;
const PORE_SEED: u32 = 0x85EB_CA6B;
const SPECKLE_SEED: u32 = 0xC2B2_AE35;

/// Returns the grain coverage of every texture pixel (row by row), from 0
/// (no chalk) to 255 (solid chalk). Always the same result, so the grain
/// does not change between runs.
pub fn grain_alpha_texture() -> Vec<u8> {
    let size = GRAIN_TEXTURE_SIZE_PX;
    let mut alpha = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            alpha.push(to_byte(grain_coverage(x, y)));
        }
    }
    alpha
}

/// Colours a grain texture with `color_rgb` (each 0.0..=1.0) and returns
/// premultiplied BGRA pixels, the format Direct2D bitmaps use.
pub fn tinted_texture_bgra(grain_alpha: &[u8], color_rgb: [f32; 3]) -> Vec<u8> {
    let [red, green, blue] = color_rgb.map(|channel| channel.clamp(0.0, 1.0));
    let mut pixels = Vec::with_capacity(grain_alpha.len() * 4);
    for &alpha_byte in grain_alpha {
        let alpha = f32::from(alpha_byte) / 255.0;
        // Premultiplied: each colour channel is already scaled by alpha.
        pixels.extend_from_slice(&[
            to_byte(blue * alpha),
            to_byte(green * alpha),
            to_byte(red * alpha),
            alpha_byte,
        ]);
    }
    pixels
}

/// Chalk coverage at one texture pixel, 0.0..=1.0.
fn grain_coverage(x: u32, y: u32) -> f32 {
    let noise = BLOTCH_WEIGHT * tiling_value_noise(x, y, BLOTCH_CELL_PX, BLOTCH_SEED)
        + PORE_WEIGHT * tiling_value_noise(x, y, PORE_CELL_PX, PORE_SEED)
        + SPECKLE_WEIGHT * random_unit(x, y, SPECKLE_SEED);
    let solidness = ((noise - HOLE_LEVEL) / HOLE_SOFTNESS).clamp(0.0, 1.0);
    MIN_ALPHA + (1.0 - MIN_ALPHA) * solidness
}

/// Smooth random noise made of a grid of random values blended together.
/// The grid wraps around at the texture edge, so the result tiles.
fn tiling_value_noise(x: u32, y: u32, cell_px: u32, seed: u32) -> f32 {
    let cells_per_side = GRAIN_TEXTURE_SIZE_PX / cell_px;
    let (cell_x, cell_y) = (x / cell_px, y / cell_px);
    let next_x = (cell_x + 1) % cells_per_side;
    let next_y = (cell_y + 1) % cells_per_side;
    let blend_x = smoothstep((x % cell_px) as f32 / cell_px as f32);
    let blend_y = smoothstep((y % cell_px) as f32 / cell_px as f32);

    let top = lerp(
        random_unit(cell_x, cell_y, seed),
        random_unit(next_x, cell_y, seed),
        blend_x,
    );
    let bottom = lerp(
        random_unit(cell_x, next_y, seed),
        random_unit(next_x, next_y, seed),
        blend_x,
    );
    lerp(top, bottom, blend_y)
}

/// A repeatable pseudo-random number in 0.0..1.0 for a grid position.
fn random_unit(x: u32, y: u32, seed: u32) -> f32 {
    let mixed = mix_bits(x.wrapping_mul(0x27D4_EB2D) ^ mix_bits(y ^ seed));
    // The top 24 bits fit exactly in an f32 mantissa.
    (mixed >> 8) as f32 / (1u32 << 24) as f32
}

/// Scrambles bits so neighbouring inputs give unrelated outputs
/// (the "lowbias32" integer hash).
fn mix_bits(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^ (value >> 16)
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn to_byte(unit_value: f32) -> u8 {
    (unit_value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel_index(x: u32, y: u32) -> usize {
        (y * GRAIN_TEXTURE_SIZE_PX + x) as usize
    }

    #[test]
    fn texture_has_one_value_per_pixel() {
        let size = GRAIN_TEXTURE_SIZE_PX as usize;
        assert_eq!(grain_alpha_texture().len(), size * size);
    }

    #[test]
    fn texture_is_the_same_every_run() {
        assert_eq!(grain_alpha_texture(), grain_alpha_texture());
    }

    #[test]
    fn grain_is_mostly_solid_with_some_holes() {
        let alpha = grain_alpha_texture();
        let average =
            alpha.iter().map(|&value| f32::from(value)).sum::<f32>() / alpha.len() as f32 / 255.0;
        let hole_count = alpha.iter().filter(|&&value| value < 128).count();
        assert!((0.6..0.9).contains(&average), "average coverage {average}");
        assert!(hole_count > alpha.len() / 20, "too few holes: {hole_count}");
    }

    #[test]
    fn holes_never_go_fully_transparent() {
        let lowest = grain_alpha_texture().into_iter().min();
        assert_eq!(lowest, Some(to_byte(MIN_ALPHA)));
    }

    #[test]
    fn smooth_layers_wrap_around_texture_edges() {
        // Moving one cell past the last one must land back on the first,
        // otherwise the tiled texture shows a visible seam.
        let size = GRAIN_TEXTURE_SIZE_PX;
        for cell_px in [BLOTCH_CELL_PX, PORE_CELL_PX] {
            assert_eq!(size % cell_px, 0);
            let last_cell_start = size - cell_px;
            let almost_wrapped =
                tiling_value_noise(last_cell_start + cell_px - 1, 0, cell_px, BLOTCH_SEED);
            let first = tiling_value_noise(0, 0, cell_px, BLOTCH_SEED);
            assert!((almost_wrapped - first).abs() < 0.2);
        }
    }

    #[test]
    fn tinting_premultiplies_colour_by_grain() {
        let half_alpha = 128;
        let pixels = tinted_texture_bgra(&[half_alpha, 255], [1.0, 0.5, 0.0]);
        // Half-covered red-orange pixel: colour channels scaled by ~0.5.
        assert_eq!(&pixels[0..4], &[0, 64, 128, 128]);
        // Fully covered pixel keeps the full colour.
        assert_eq!(&pixels[4..8], &[0, 128, 255, 255]);
    }

    #[test]
    fn neighbouring_pixels_differ() {
        let alpha = grain_alpha_texture();
        let differing = (0..GRAIN_TEXTURE_SIZE_PX - 1)
            .filter(|&x| alpha[pixel_index(x, 10)] != alpha[pixel_index(x + 1, 10)])
            .count();
        assert!(differing > GRAIN_TEXTURE_SIZE_PX as usize / 2);
    }
}
