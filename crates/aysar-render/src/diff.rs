//! Side-by-side pictures: original | build | difference, one per width, so
//! a person (or a vision model) can see at a glance what the numbers mean.

use image::{DynamicImage, GenericImage, Rgba, RgbaImage};
use std::path::Path;

const GUTTER: u32 = 12;

/// Compose `a.png` and `b.png` into `out.png` as three panels. Returns the
/// share of pixels that differ noticeably (0.0–1.0), a cheap second opinion
/// next to the hash.
pub fn side_by_side(a: &Path, b: &Path, out: &Path) -> anyhow::Result<f32> {
    let a = image::open(a)?.to_rgba8();
    let b = image::open(b)?.to_rgba8();
    let w = a.width().max(b.width());
    let h = a.height().max(b.height());

    let mut canvas = RgbaImage::from_pixel(w * 3 + GUTTER * 2, h, Rgba([40, 40, 40, 255]));
    canvas.copy_from(&DynamicImage::ImageRgba8(a.clone()), 0, 0)?;
    canvas.copy_from(&DynamicImage::ImageRgba8(b.clone()), w + GUTTER, 0)?;

    let mut differing = 0u64;
    let total = (w as u64) * (h as u64);
    let x0 = (w + GUTTER) * 2;
    for y in 0..h {
        for x in 0..w {
            let pa = if x < a.width() && y < a.height() { a.get_pixel(x, y).0 } else { [255, 255, 255, 255] };
            let pb = if x < b.width() && y < b.height() { b.get_pixel(x, y).0 } else { [255, 255, 255, 255] };
            let d = pa
                .iter()
                .zip(pb.iter())
                .take(3)
                .map(|(p, q)| (*p as i32 - *q as i32).unsigned_abs())
                .max()
                .unwrap_or(0);
            let px = if d > 32 {
                differing += 1;
                Rgba([230, 60, 60, 255])
            } else {
                // Faint copy of the original so the shape stays readable.
                let g = ((pa[0] as u32 + pa[1] as u32 + pa[2] as u32) / 3) as u8;
                let g = 200 + (g as u32 * 55 / 255) as u8;
                Rgba([g, g, g, 255])
            };
            canvas.put_pixel(x0 + x, y, px);
        }
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    canvas.save(out)?;
    Ok(if total == 0 { 0.0 } else { differing as f32 / total as f32 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_and_measures() {
        let dir = std::env::temp_dir().join(format!("aysar-diff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = RgbaImage::from_pixel(20, 10, Rgba([255, 255, 255, 255]));
        let mut b = a.clone();
        for x in 0..10 {
            for y in 0..10 {
                b.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        a.save(dir.join("a.png")).unwrap();
        b.save(dir.join("b.png")).unwrap();
        let ratio = side_by_side(&dir.join("a.png"), &dir.join("b.png"), &dir.join("d.png")).unwrap();
        assert!((ratio - 0.5).abs() < 0.01, "{ratio}");
        let d = image::open(dir.join("d.png")).unwrap();
        assert_eq!(d.width(), 20 * 3 + GUTTER * 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
