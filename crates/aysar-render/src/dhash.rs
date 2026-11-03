//! Difference hash: a 64-bit fingerprint of an image's shape. Two screenshots
//! with a small Hamming distance look alike; the distance survives colour
//! shifts and small size changes, which is what we want for "does the build
//! look like the original".

use image::imageops::FilterType;
use image::GenericImageView;

/// Compute the dHash of a PNG, as 16 hex characters.
pub fn dhash_png(png: &[u8]) -> anyhow::Result<String> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)?;
    Ok(dhash_image(&img))
}

/// dHash of any decoded image.
pub fn dhash_image(img: &image::DynamicImage) -> String {
    // Tall pages: hash the top 4:3 region plus the whole page, blended by
    // simply hashing the whole page resized. Keep it simple and stable.
    let (w, h) = img.dimensions();
    let crop_h = h.min(w * 3); // cap very tall pages so the hash is not all whitespace
    let cropped = img.crop_imm(0, 0, w, crop_h);
    let small = cropped.resize_exact(9, 8, FilterType::Triangle).to_luma8();
    let mut bits: u64 = 0;
    for y in 0..8 {
        for x in 0..8 {
            let l = small.get_pixel(x, y)[0];
            let r = small.get_pixel(x + 1, y)[0];
            bits <<= 1;
            if l > r {
                bits |= 1;
            }
        }
    }
    format!("{bits:016x}")
}

/// Hamming distance between two hex hashes (0–64). `None` if malformed.
pub fn distance(a: &str, b: &str) -> Option<u32> {
    let a = u64::from_str_radix(a, 16).ok()?;
    let b = u64::from_str_radix(b, 16).ok()?;
    Some((a ^ b).count_ones())
}

/// Similarity 0.0–1.0 from two hashes.
pub fn similarity(a: &str, b: &str) -> Option<f32> {
    distance(a, b).map(|d| 1.0 - d as f32 / 64.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};

    fn img(f: impl Fn(u32, u32) -> u8) -> DynamicImage {
        let mut im = RgbImage::new(64, 64);
        for (x, y, p) in im.enumerate_pixels_mut() {
            let v = f(x, y);
            *p = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(im)
    }

    #[test]
    fn same_shape_same_hash() {
        // A horizontal gradient and a dimmer copy share a hash; a vertical one does not.
        let a = img(|x, _| 255 - (x * 4) as u8);
        let b = img(|x, _| 200 - (x * 3) as u8);
        let c = img(|_, y| (y * 4) as u8);
        let ha = dhash_image(&a);
        let hb = dhash_image(&b);
        let hc = dhash_image(&c);
        assert_eq!(ha, hb);
        assert!(similarity(&ha, &hc).unwrap() < 0.6, "{ha} vs {hc}");
        assert_eq!(distance("0", "0"), Some(0));
        assert_eq!(distance("ffffffffffffffff", "0"), Some(64));
    }
}
