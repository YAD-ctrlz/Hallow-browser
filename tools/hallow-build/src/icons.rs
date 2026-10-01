//! Render the Hallow icon set from the master logo (`branding/logo.png`),
//! so every size is generated rather than committed.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};
use image::imageops::{self, FilterType};
use image::{ImageFormat, Rgba, Rgba32FImage, RgbaImage};

use crate::util::write_file;

/// Sizes Firefox's branding package expects (`default<N>.png`).
pub const BRANDING_SIZES: [u32; 8] = [16, 22, 24, 32, 48, 64, 128, 256];

/// Sizes installed into the desktop icon theme (`hicolor/<N>x<N>/apps`).
pub const THEME_SIZES: [u32; 11] = [16, 22, 24, 32, 48, 64, 96, 128, 192, 256, 512];

pub fn load_logo(branding: &Path) -> Result<RgbaImage> {
    let path = branding.join("logo.png");
    let image = image::open(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(image.into_rgba8())
}

/// Scale `logo` to fit `width`×`height`, centered on a transparent canvas,
/// and encode it as PNG.
///
/// Resampling works on premultiplied alpha: averaging straight RGBA would
/// pull the (black) color of transparent pixels into the antialiased edge
/// and leave a dark fringe around the logo.
pub fn render_png(logo: &RgbaImage, width: u32, height: u32) -> Result<Vec<u8>> {
    let scale = (width as f32 / logo.width() as f32).min(height as f32 / logo.height() as f32);
    let w = ((logo.width() as f32 * scale).round() as u32).clamp(1, width);
    let h = ((logo.height() as f32 * scale).round() as u32).clamp(1, height);

    let mut linear = Rgba32FImage::new(logo.width(), logo.height());
    for (src, dst) in logo.pixels().zip(linear.pixels_mut()) {
        let a = src[3] as f32 / 255.0;
        *dst = Rgba([
            src[0] as f32 / 255.0 * a,
            src[1] as f32 / 255.0 * a,
            src[2] as f32 / 255.0 * a,
            a,
        ]);
    }
    let resized = imageops::resize(&linear, w, h, FilterType::Lanczos3);

    let mut canvas = RgbaImage::new(width, height);
    let (dx, dy) = ((width - w) / 2, (height - h) / 2);
    for (x, y, p) in resized.enumerate_pixels() {
        let a = p[3].clamp(0.0, 1.0);
        let channel = |c: f32| {
            if a > 0.0 {
                ((c / a).clamp(0.0, 1.0) * 255.0).round() as u8
            } else {
                0
            }
        };
        canvas.put_pixel(
            x + dx,
            y + dy,
            Rgba([
                channel(p[0]),
                channel(p[1]),
                channel(p[2]),
                (a * 255.0).round() as u8,
            ]),
        );
    }

    let mut png = Vec::new();
    canvas
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .context("encoding PNG")?;
    Ok(png)
}

/// An SVG that embeds the logo as a PNG, for the places Firefox loads
/// `about-logo.svg` (onboarding, feature callouts).
pub fn logo_svg(logo: &RgbaImage, size: u32) -> Result<String> {
    let png = render_png(logo, size, size)?;
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{size}\" height=\"{size}\" \
         viewBox=\"0 0 {size} {size}\"><image width=\"{size}\" height=\"{size}\" \
         href=\"data:image/png;base64,{}\"/></svg>\n",
        base64(&png)
    ))
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Write the branding and desktop icon sets into `out` (for inspection).
pub fn render_all(branding: &Path, out: &Path) -> Result<()> {
    let logo = load_logo(branding)?;
    fs::create_dir_all(out)?;
    for size in BRANDING_SIZES.iter().chain(&THEME_SIZES) {
        write_file(
            &out.join(format!("default{size}.png")),
            render_png(&logo, *size, *size)?,
        )?;
    }
    write_file(&out.join("about-logo.png"), render_png(&logo, 192, 192)?)?;
    write_file(&out.join("about-logo@2x.png"), render_png(&logo, 384, 384)?)?;
    write_file(&out.join("about.png"), render_png(&logo, 300, 236)?)?;
    write_file(&out.join("about-logo.svg"), logo_svg(&logo, 512)?)?;
    eprintln!("rendered icons into {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository_logo() -> RgbaImage {
        load_logo(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../branding")).unwrap()
    }

    #[test]
    fn renders_the_repository_logo_at_every_size() {
        let logo = repository_logo();
        assert_eq!(logo.width(), logo.height(), "master logo must be square");
        for size in BRANDING_SIZES.iter().chain(&THEME_SIZES) {
            let png = render_png(&logo, *size, *size).unwrap();
            let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
            assert_eq!(decoded.dimensions(), (*size, *size));
            // Mostly drawn, with transparent corners.
            assert!(decoded.pixels().filter(|p| p[3] > 200).count() as u32 > size * size / 3);
            assert_eq!(decoded.get_pixel(0, 0)[3], 0);
        }
    }

    #[test]
    fn non_square_targets_are_centered() {
        let logo = repository_logo();
        let png = render_png(&logo, 300, 236).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
        assert_eq!(decoded.dimensions(), (300, 236));
        // Columns at the far left and right are padding.
        assert!((0..236).all(|y| decoded.get_pixel(0, y)[3] == 0));
        assert!((0..236).all(|y| decoded.get_pixel(299, y)[3] == 0));
    }

    #[test]
    fn premultiplied_resize_keeps_edges_bright() {
        // An opaque white square on transparent black must not turn grey at
        // its antialiased edge when scaled down.
        let mut img = RgbaImage::new(64, 64);
        for y in 16..48 {
            for x in 16..48 {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let png = render_png(&img, 10, 10).unwrap();
        let out = image::load_from_memory(&png).unwrap().into_rgba8();
        for p in out.pixels().filter(|p| p[3] > 16) {
            assert!(p[0] > 240, "dark fringe: {p:?}");
        }
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
