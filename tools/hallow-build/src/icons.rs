//! Render the Hallow icon set from the master logo (`branding/logo.png`),
//! so every size is generated rather than committed.
//!
//! The logo (a globe with a halo floating above it) is taller than it is
//! wide, so fitted into a square as-is the globe only spans ~78% of the
//! canvas and the app looks smaller than Firefox or Chromium next to it.
//! App icons therefore use a tighter composition of the same artwork: the
//! halo rests on the globe and the result fills the canvas with the slim
//! margins Firefox's own icons use. Below [`HALO_MIN_SIZE`] the halo is a
//! one-pixel smear, so those sizes show the globe alone, and up to
//! [`SHARPEN_MAX_SIZE`] the downscaled artwork is sharpened so the
//! continents stay distinct in panels, title bars and the taskbar.
//!
//! The same renderer makes the Linux icon theme and the Windows icons
//! (`.ico` files with every size Windows asks for at each display scale,
//! Start menu tiles and the installer's artwork).

use std::fs;
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};
use image::imageops::{self, FilterType};
use image::{ImageFormat, Rgba, Rgba32FImage, RgbaImage};

use crate::util::write_file;

/// Sizes Firefox's branding package expects (`default<N>.png`). They become
/// the window icons (`_NET_WM_ICON`) of every Hallow window.
pub const BRANDING_SIZES: [u32; 8] = [16, 22, 24, 32, 48, 64, 128, 256];

/// Sizes installed into the desktop icon theme (`hicolor/<N>x<N>/apps`).
pub const THEME_SIZES: [u32; 11] = [16, 22, 24, 32, 48, 64, 96, 128, 192, 256, 512];

/// Smallest icon size that still shows the halo.
pub const HALO_MIN_SIZE: u32 = 32;

/// How far the halo is lowered onto the globe, as a fraction of its height.
const HALO_OVERLAP: f32 = 0.3;

/// Transparent margin around app icons, as a fraction of the canvas: about
/// what Firefox's icons use (0 px at 16, 1 px at 48, 4 px at 128).
const ICON_MARGIN: f32 = 0.03;

/// Alpha below which a logo pixel counts as empty when locating its parts.
const ALPHA_EMPTY: u8 = 16;

/// Largest icon size that is sharpened after downscaling.
pub const SHARPEN_MAX_SIZE: u32 = 48;

/// Strength of that sharpening (unsharp mask amount).
const SHARPEN_AMOUNT: f32 = 0.5;

/// Sizes in Hallow's Windows app icon: every size Windows uses for the
/// taskbar, title bars, Start, Explorer and the desktop at display scales
/// from 100% to 400% (Microsoft's list for Win32 app icons), so Windows
/// never has to scale the artwork.
pub const WINDOWS_ICON_SIZES: [u32; 15] =
    [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256];

/// Sizes of the smaller Windows icons (installer, documents).
pub const WINDOWS_SMALL_ICON_SIZES: [u32; 7] = [16, 20, 24, 32, 40, 48, 64];

/// Background of Hallow's dark surfaces (About dialog, Start tile,
/// installer artwork).
pub const DARK_BACKGROUND: [u8; 3] = [0x12, 0x0c, 0x24];

pub fn load_logo(branding: &Path) -> Result<RgbaImage> {
    let path = branding.join("logo.png");
    let image = image::open(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(image.into_rgba8())
}

/// Bounding box `(x, y, width, height)` of the visible pixels, if any.
fn visible_bounds(image: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in image.enumerate_pixels() {
        if p[3] > ALPHA_EMPTY {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0 <= x1).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

fn row_is_empty(image: &RgbaImage, y: u32) -> bool {
    (0..image.width()).all(|x| image.get_pixel(x, y)[3] <= ALPHA_EMPTY)
}

/// Split the logo at the transparent gap between the halo (above) and the
/// globe (below). `None` if the artwork has no such gap.
fn split_halo(logo: &RgbaImage) -> Option<(RgbaImage, RgbaImage)> {
    let height = logo.height();
    let top = (0..height).find(|&y| !row_is_empty(logo, y))?;
    let gap = (top..height).find(|&y| row_is_empty(logo, y))?;
    let globe = (gap..height).find(|&y| !row_is_empty(logo, y))?;
    let halo = imageops::crop_imm(logo, 0, 0, logo.width(), gap).to_image();
    let globe = imageops::crop_imm(logo, 0, globe, logo.width(), height - globe).to_image();
    Some((halo, globe))
}

/// The artwork used for app icons of `size` pixels, cropped to its visible
/// pixels: the globe with the halo resting on it, or the globe alone for
/// small sizes. Falls back to the plain logo if it cannot be taken apart.
pub fn icon_artwork(logo: &RgbaImage, size: u32) -> RgbaImage {
    let crop = |image: &RgbaImage| match visible_bounds(image) {
        Some((x, y, w, h)) => imageops::crop_imm(image, x, y, w, h).to_image(),
        None => image.clone(),
    };
    let Some((halo, globe)) = split_halo(logo) else {
        return crop(logo);
    };
    let (Some(hb), Some(gb)) = (visible_bounds(&halo), visible_bounds(&globe)) else {
        return crop(logo);
    };
    if size < HALO_MIN_SIZE {
        return crop(&globe);
    }
    // Keep both parts at their horizontal positions in the logo, lower the
    // halo onto the globe, and draw it in front so its near rim overlaps
    // the top of the globe.
    let overlap = (hb.3 as f32 * HALO_OVERLAP).round() as u32;
    let left = hb.0.min(gb.0);
    let right = (hb.0 + hb.2).max(gb.0 + gb.2);
    let mut art = RgbaImage::new(right - left, hb.3 - overlap + gb.3);
    let globe = imageops::crop_imm(&globe, gb.0, gb.1, gb.2, gb.3).to_image();
    let halo = imageops::crop_imm(&halo, hb.0, hb.1, hb.2, hb.3).to_image();
    imageops::overlay(
        &mut art,
        &globe,
        (gb.0 - left).into(),
        (hb.3 - overlap).into(),
    );
    imageops::overlay(&mut art, &halo, (hb.0 - left).into(), 0);
    art
}

/// App icon of `size`×`size` pixels: [`icon_artwork`] centered with a
/// slim margin, sharpened at small sizes.
pub fn icon_image(logo: &RgbaImage, size: u32) -> RgbaImage {
    let margin = (size as f32 * ICON_MARGIN).round() as u32;
    let inner = size - 2 * margin;
    let mut fitted = resize_premultiplied(&icon_artwork(logo, size), inner, inner);
    if size <= SHARPEN_MAX_SIZE {
        fitted = sharpen(&fitted, SHARPEN_AMOUNT);
    }
    center(&fitted, size, size)
}

/// PNG of [`icon_image`].
pub fn render_icon(logo: &RgbaImage, size: u32) -> Result<Vec<u8>> {
    encode_png(&icon_image(logo, size))
}

/// Unsharp mask on color only: each pixel moves away from the average color
/// of its visible neighbors. Alpha (the icon's shape) is unchanged, and
/// transparent neighbors do not darken or lighten the edge.
fn sharpen(image: &RgbaImage, amount: f32) -> RgbaImage {
    const KERNEL: [[f32; 3]; 3] = [[1.0, 2.0, 1.0], [2.0, 4.0, 2.0], [1.0, 2.0, 1.0]];
    let (w, h) = image.dimensions();
    let mut out = image.clone();
    for y in 0..h {
        for x in 0..w {
            let p = image.get_pixel(x, y);
            if p[3] == 0 {
                continue;
            }
            let (mut sum, mut weight) = ([0.0f32; 3], 0.0f32);
            for (dy, row) in KERNEL.iter().enumerate() {
                for (dx, k) in row.iter().enumerate() {
                    let (nx, ny) = (x as i64 + dx as i64 - 1, y as i64 + dy as i64 - 1);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let n = image.get_pixel(nx as u32, ny as u32);
                    let wgt = k * n[3] as f32 / 255.0;
                    for c in 0..3 {
                        sum[c] += wgt * n[c] as f32;
                    }
                    weight += wgt;
                }
            }
            let q = out.get_pixel_mut(x, y);
            for c in 0..3 {
                let average = sum[c] / weight;
                let v = p[c] as f32 + amount * (p[c] as f32 - average);
                q[c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Scalable app icon (`hicolor/scalable/apps`). The logo is raster pixel
/// art, so this wraps the 512 px icon; desktops use it for sizes between
/// the PNG sizes, which stay authoritative for the standard sizes.
pub fn icon_svg(logo: &RgbaImage) -> Result<String> {
    let png = render_icon(logo, 512)?;
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"512\" height=\"512\" \
         viewBox=\"0 0 512 512\"><image width=\"512\" height=\"512\" \
         href=\"data:image/png;base64,{}\"/></svg>\n",
        base64(&png)
    ))
}

/// Scale `logo` to fit `width`×`height`, centered on a transparent canvas,
/// and encode it as PNG.
///
/// Resampling works on premultiplied alpha: averaging straight RGBA would
/// pull the (black) color of transparent pixels into the antialiased edge
/// and leave a dark fringe around the logo.
pub fn render_png(logo: &RgbaImage, width: u32, height: u32) -> Result<Vec<u8>> {
    encode_png(&center(
        &resize_premultiplied(logo, width, height),
        width,
        height,
    ))
}

/// Scale `image` down (or up) to fit within `width`×`height`, keeping its
/// aspect ratio.
fn resize_premultiplied(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let scale = (width as f32 / image.width() as f32).min(height as f32 / image.height() as f32);
    let w = ((image.width() as f32 * scale).round() as u32).clamp(1, width);
    let h = ((image.height() as f32 * scale).round() as u32).clamp(1, height);

    let mut linear = Rgba32FImage::new(image.width(), image.height());
    for (src, dst) in image.pixels().zip(linear.pixels_mut()) {
        let a = src[3] as f32 / 255.0;
        *dst = Rgba([
            src[0] as f32 / 255.0 * a,
            src[1] as f32 / 255.0 * a,
            src[2] as f32 / 255.0 * a,
            a,
        ]);
    }
    let resized = imageops::resize(&linear, w, h, FilterType::Lanczos3);

    let mut out = RgbaImage::new(w, h);
    for (p, dst) in resized.pixels().zip(out.pixels_mut()) {
        let a = p[3].clamp(0.0, 1.0);
        let channel = |c: f32| {
            if a > 0.0 {
                ((c / a).clamp(0.0, 1.0) * 255.0).round() as u8
            } else {
                0
            }
        };
        *dst = Rgba([
            channel(p[0]),
            channel(p[1]),
            channel(p[2]),
            (a * 255.0).round() as u8,
        ]);
    }
    out
}

/// `image` on a transparent `width`×`height` canvas, centered.
pub fn center(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let mut canvas = RgbaImage::new(width, height);
    let (dx, dy) = ((width - image.width()) / 2, (height - image.height()) / 2);
    imageops::replace(&mut canvas, image, dx.into(), dy.into());
    canvas
}

pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    image
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

/// A 4:3 SVG with the logo centered and padded, used in place of Firefox's
/// "Kit" fox illustrations (Settings cards, notifications), which are shown
/// with `object-fit: cover` and so need room around the logo.
pub fn illustration_svg(logo: &RgbaImage) -> Result<String> {
    let png = render_png(logo, 256, 256)?;
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"400\" height=\"300\" \
         viewBox=\"0 0 400 300\"><image x=\"100\" y=\"50\" width=\"200\" height=\"200\" \
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
            render_icon(&logo, *size)?,
        )?;
    }
    write_file(&out.join("hallow.svg"), icon_svg(&logo)?)?;
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
            let png = render_icon(&logo, *size).unwrap();
            let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
            assert_eq!(decoded.dimensions(), (*size, *size));
            // Mostly drawn, with transparent corners.
            assert!(decoded.pixels().filter(|p| p[3] > 200).count() as u32 > size * size / 2);
            assert_eq!(decoded.get_pixel(0, 0)[3], 0);
        }
    }

    #[test]
    fn app_icons_fill_the_canvas_like_firefox() {
        // Firefox's icons span 89-100% of their canvas; the plain logo only
        // reached ~80% in width, which made Hallow look small in docks and
        // menus. The visible artwork must now span at least 90% of the
        // canvas in its larger dimension and 82% in the other (the globe
        // with its halo is slightly taller than wide).
        let logo = repository_logo();
        for size in THEME_SIZES {
            let png = render_icon(&logo, size).unwrap();
            let icon = image::load_from_memory(&png).unwrap().into_rgba8();
            let (_, _, w, h) = visible_bounds(&icon).unwrap();
            let (major, minor) = (w.max(h) as f32, w.min(h) as f32);
            let size_f = size as f32;
            assert!(major >= 0.9 * size_f, "{size}px: spans {w}x{h}");
            assert!(minor >= 0.82 * size_f, "{size}px: spans {w}x{h}");
            // What sets the apparent size is how much of the canvas is
            // covered: Firefox's icons cover 54-71%; the plain logo 54%.
            let opaque = icon.pixels().filter(|p| p[3] > 128).count() as f32;
            let coverage = opaque / (size * size) as f32;
            assert!(coverage >= 0.57, "{size}px: covers {coverage:.2}");
        }
    }

    #[test]
    fn small_icons_drop_the_halo() {
        let logo = repository_logo();
        let globe = icon_artwork(&logo, HALO_MIN_SIZE - 1);
        let full = icon_artwork(&logo, HALO_MIN_SIZE);
        // The globe alone is about round; with the halo it is taller.
        let ratio = |img: &RgbaImage| img.height() as f32 / img.width() as f32;
        assert!((ratio(&globe) - 1.0).abs() < 0.05, "{}", ratio(&globe));
        assert!(ratio(&full) > 1.1, "{}", ratio(&full));
        // ...but shorter than the logo, whose halo floats above the globe.
        let (_, _, lw, lh) = visible_bounds(&logo).unwrap();
        assert!(ratio(&full) < lh as f32 / lw as f32);
    }

    #[test]
    fn logos_without_a_halo_gap_still_render() {
        let mut img = RgbaImage::new(40, 40);
        for y in 5..35 {
            for x in 10..30 {
                img.put_pixel(x, y, Rgba([10, 20, 200, 255]));
            }
        }
        let art = icon_artwork(&img, 64);
        assert_eq!(art.dimensions(), (20, 30));
    }

    #[test]
    fn scalable_icon_is_svg() {
        let svg = icon_svg(&repository_logo()).unwrap();
        assert!(svg.starts_with("<svg ") && svg.contains("viewBox=\"0 0 512 512\""));
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
