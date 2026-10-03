//! Windows-specific branding: the app, document and private browsing icons
//! (`.ico`), the Start menu tiles, the installer's artwork and the
//! certificates the updater trusts. Everything is rendered from the same
//! master logo as the Linux icons (see `icons.rs`).

use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use image::{Rgba, RgbaImage, imageops};

use crate::icons::{self, DARK_BACKGROUND, WINDOWS_ICON_SIZES, WINDOWS_SMALL_ICON_SIZES};
use crate::util::write_file;

/// Images at or above this size are stored PNG-compressed in `.ico` files
/// (Windows Vista and later read them); smaller ones as 32-bit bitmaps,
/// which every consumer understands.
const ICO_PNG_MIN_SIZE: u32 = 64;

/// Supersampling factor for the shapes drawn here (page, disc).
const SUPERSAMPLE: u32 = 4;

/// Write the Windows branding files into the branding directory `dest`
/// (replacing the unofficial ones it was copied from) and the installer
/// and updater icons the Firefox tree keeps outside it.
pub fn install_branding(logo: &RgbaImage, dest: &Path, source: &Path) -> Result<()> {
    let app: Vec<RgbaImage> = WINDOWS_ICON_SIZES
        .iter()
        .map(|&s| icons::icon_image(logo, s))
        .collect();
    write_file(&dest.join("firefox.ico"), ico(&app)?)?;

    let small = |render: &dyn Fn(u32) -> RgbaImage| -> Result<Vec<u8>> {
        let images: Vec<RgbaImage> = WINDOWS_SMALL_ICON_SIZES
            .iter()
            .map(|&s| render(s))
            .collect();
        ico(&images)
    };
    // Icon of the stub installer and of the full installer (setup.ico):
    // bitmaps only, as NSIS has always required.
    let installer_icon = small(&|s| icons::icon_image(logo, s))?;
    write_file(&dest.join("firefox64.ico"), &installer_icon)?;
    write_file(
        &source.join("toolkit/mozapps/installer/windows/nsis/setup.ico"),
        &installer_icon,
    )?;
    // Shown by the updater's progress window while an update is applied.
    write_file(
        &source.join("toolkit/mozapps/update/updater/updater.ico"),
        &installer_icon,
    )?;

    let mut document: Vec<RgbaImage> = WINDOWS_SMALL_ICON_SIZES
        .iter()
        .map(|&s| document_icon(logo, s))
        .collect();
    document.push(document_icon(logo, 256));
    write_file(&dest.join("document.ico"), ico(&document)?)?;

    let mut private: Vec<RgbaImage> = WINDOWS_SMALL_ICON_SIZES
        .iter()
        .map(|&s| private_icon(logo, s))
        .collect();
    private.push(private_icon(logo, 256));
    write_file(&dest.join("pbmode.ico"), ico(&private)?)?;

    // Start menu tiles (VisualElementsManifest.xml in branding/overlay sets
    // their background color).
    for (name, size, art) in [
        ("VisualElements_150.png", 300, 150),
        ("VisualElements_70.png", 142, 88),
    ] {
        let tile = icons::center(&icons::icon_image(logo, art), size, size);
        write_file(&dest.join(name), icons::encode_png(&tile)?)?;
    }
    for (name, size, art) in [
        ("PrivateBrowsing_150.png", 300, 150),
        ("PrivateBrowsing_70.png", 142, 88),
    ] {
        let tile = icons::center(&private_icon(logo, art), size, size);
        write_file(&dest.join(name), icons::encode_png(&tile)?)?;
    }

    // Installer artwork: the welcome/finish page panel and the header of
    // the other pages (left-aligned for right-to-left languages).
    write_file(
        &dest.join("wizWatermark.bmp"),
        bmp(&installer_watermark(logo)),
    )?;
    write_file(
        &dest.join("wizHeader.bmp"),
        bmp(&installer_header(logo, false)),
    )?;
    write_file(
        &dest.join("wizHeaderRTL.bmp"),
        bmp(&installer_header(logo, true)),
    )?;
    Ok(())
}

/// Install the DER certificates of the keys that sign Hallow's update
/// packages as the updater's trusted certificates. Gecko's updater accepts
/// a MAR signed by either; release builds embed only Hallow's release key.
pub fn install_update_certificates(source: &Path, primary: &Path, secondary: &Path) -> Result<()> {
    let dir = source.join("toolkit/mozapps/update/updater");
    for (cert, name) in [
        (primary, "release_primary.der"),
        (secondary, "release_secondary.der"),
    ] {
        let der = fs::read(cert).with_context(|| format!("reading {}", cert.display()))?;
        // An X.509 certificate is a DER SEQUENCE.
        if der.len() < 256 || der[0] != 0x30 {
            bail!("{} is not a DER certificate", cert.display());
        }
        fs::write(dir.join(name), der)?;
        eprintln!("updater trusts {} as {name}", cert.display());
    }
    Ok(())
}

/// Encode `images` (square, distinct sizes) as a Windows `.ico` file.
pub fn ico(images: &[RgbaImage]) -> Result<Vec<u8>> {
    let mut entries = Vec::new();
    for image in images {
        let size = image.width();
        if size != image.height() || size == 0 || size > 256 {
            bail!("icon images must be square and at most 256 px");
        }
        let data = if size >= ICO_PNG_MIN_SIZE {
            icons::encode_png(image)?
        } else {
            ico_bitmap(image)
        };
        entries.push((size, data));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * entries.len() as u32;
    for (size, data) in &entries {
        let dim = if *size == 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in entries {
        out.extend_from_slice(&data);
    }
    Ok(out)
}

/// An icon image as a 32-bit BGRA DIB with its 1-bit transparency mask.
fn ico_bitmap(image: &RgbaImage) -> Vec<u8> {
    let (w, h) = image.dimensions();
    let mask_stride = w.div_ceil(32) * 4;
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    // The height covers the color image and the mask.
    out.extend_from_slice(&(2 * h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(w * h * 4 + mask_stride * h).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for y in (0..h).rev() {
        for x in 0..w {
            let p = image.get_pixel(x, y);
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    for y in (0..h).rev() {
        let mut row = vec![0u8; mask_stride as usize];
        for x in 0..w {
            if image.get_pixel(x, y)[3] == 0 {
                row[(x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&row);
    }
    out
}

/// Encode `image` as an opaque 24-bit BMP (the format NSIS needs for its
/// bitmaps); it must not have transparent pixels.
pub fn bmp(image: &RgbaImage) -> Vec<u8> {
    let (w, h) = image.dimensions();
    let stride = (w * 3).div_ceil(4) * 4;
    let size = 54 + stride * h;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(stride * h).to_le_bytes());
    // 96 dpi, as Windows' own bitmaps.
    out.extend_from_slice(&3780u32.to_le_bytes());
    out.extend_from_slice(&3780u32.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    for y in (0..h).rev() {
        let start = out.len();
        for x in 0..w {
            let p = image.get_pixel(x, y);
            out.extend_from_slice(&[p[2], p[1], p[0]]);
        }
        out.resize(start + stride as usize, 0);
    }
    out
}

/// Antialiased shape of `size`×`size` pixels: `paint` gives the color at a
/// point of the unit square (or `None` outside the shape).
fn draw(size: u32, paint: impl Fn(f32, f32) -> Option<[u8; 4]>) -> RgbaImage {
    let n = SUPERSAMPLE;
    let mut out = RgbaImage::new(size, size);
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let mut acc = [0.0f32; 4];
        for sy in 0..n {
            for sx in 0..n {
                let u = (x * n + sx) as f32 / (size * n) as f32 + 0.5 / (size * n) as f32;
                let v = (y * n + sy) as f32 / (size * n) as f32 + 0.5 / (size * n) as f32;
                if let Some(c) = paint(u, v) {
                    let a = c[3] as f32 / 255.0;
                    for i in 0..3 {
                        acc[i] += c[i] as f32 * a;
                    }
                    acc[3] += a;
                }
            }
        }
        let samples = (n * n) as f32;
        if acc[3] > 0.0 {
            *pixel = Rgba([
                (acc[0] / acc[3]).round() as u8,
                (acc[1] / acc[3]).round() as u8,
                (acc[2] / acc[3]).round() as u8,
                (acc[3] / samples * 255.0).round() as u8,
            ]);
        }
    }
    out
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

/// The icon of web pages and other documents Hallow opens: a sheet of paper
/// with a folded corner and the Hallow globe on it.
pub fn document_icon(logo: &RgbaImage, size: u32) -> RgbaImage {
    let (left, right, top, bottom, fold) = (0.15, 0.85, 0.03, 0.97, 0.22);
    // About one pixel of outline at any size.
    let border = (1.0 / size as f32).max(0.02);
    let paper = draw(size, |u, v| {
        if u < left || u > right || v < top || v > bottom || u - (right - fold) > v - top {
            return None;
        }
        let in_fold = u > right - fold && v < top + fold;
        let near_edge = u < left + border
            || u > right - border
            || v > bottom - border
            || v < top + border
            || (u - (right - fold)) - (v - top) > -border * 1.42;
        let fold_edge =
            in_fold && (u > right - fold && u < right - fold + border || v > top + fold - border);
        if near_edge || fold_edge {
            Some([0x8a, 0x93, 0xa3, 255])
        } else if in_fold {
            Some([0xd9, 0xde, 0xe6, 255])
        } else {
            Some([0xff, 0xff, 0xff, 255])
        }
    });
    let globe = ((size as f32 * 0.6).round() as u32).max(8);
    let art = icons::icon_image(logo, globe);
    let mut out = paper;
    let x = (size - globe) / 2;
    let y = ((size as f32 * 0.6) - globe as f32 / 2.0).round().max(0.0) as u32;
    let y = y.min(size - globe);
    imageops::overlay(&mut out, &art, x.into(), y.into());
    out
}

/// The private browsing icon: the Hallow globe on a violet disc.
pub fn private_icon(logo: &RgbaImage, size: u32) -> RgbaImage {
    let (light, dark) = ([0x8b, 0x5c, 0xf6], [0x4c, 0x1d, 0x95]);
    let disc = draw(size, |u, v| {
        let (dx, dy) = (u - 0.5, v - 0.5);
        if dx * dx + dy * dy > 0.49 * 0.49 {
            return None;
        }
        let [r, g, b] = mix(light, dark, v);
        Some([r, g, b, 255])
    });
    let globe = ((size as f32 * 0.72).round() as u32).max(8);
    let art = icons::icon_image(logo, globe);
    let mut out = disc;
    let offset = (size - globe) / 2;
    imageops::overlay(&mut out, &art, offset.into(), offset.into());
    out
}

/// Opaque `width`×`height` canvas filled with a vertical gradient.
fn gradient(width: u32, height: u32, top: [u8; 3], bottom: [u8; 3]) -> RgbaImage {
    RgbaImage::from_fn(width, height, |_, y| {
        let [r, g, b] = mix(top, bottom, y as f32 / (height - 1) as f32);
        Rgba([r, g, b, 255])
    })
}

/// Composite `layer` over the opaque `base` at `(x, y)`; `base` stays
/// opaque.
fn paste(base: &mut RgbaImage, layer: &RgbaImage, x: u32, y: u32) {
    for (lx, ly, p) in layer.enumerate_pixels() {
        let (bx, by) = (x + lx, y + ly);
        if bx >= base.width() || by >= base.height() {
            continue;
        }
        let a = p[3] as f32 / 255.0;
        let b = base.get_pixel_mut(bx, by);
        for c in 0..3 {
            b[c] = (p[c] as f32 * a + b[c] as f32 * (1.0 - a)).round() as u8;
        }
        b[3] = 255;
    }
}

/// The installer's welcome and finish page panel (164×314): the logo on
/// Hallow's dark violet.
pub fn installer_watermark(logo: &RgbaImage) -> RgbaImage {
    let mut panel = gradient(164, 314, [0x22, 0x17, 0x4a], DARK_BACKGROUND);
    let art = icons::icon_image(logo, 116);
    paste(&mut panel, &art, (164 - 116) / 2, 72);
    panel
}

/// The header of the installer's other pages (150×57, on white).
pub fn installer_header(logo: &RgbaImage, rtl: bool) -> RgbaImage {
    let mut header = RgbaImage::from_pixel(150, 57, Rgba([255, 255, 255, 255]));
    let art = icons::icon_image(logo, 49);
    let x = if rtl { 6 } else { 150 - 49 - 6 };
    paste(&mut header, &art, x, 4);
    header
}

/// Write every Windows asset into `out` for inspection (`cargo hb icons`).
pub fn render_all(logo: &RgbaImage, out: &Path) -> Result<()> {
    let windows = out.join("windows");
    fs::create_dir_all(&windows)?;
    let fake_source = windows.join("tree");
    for dir in [
        "toolkit/mozapps/installer/windows/nsis",
        "toolkit/mozapps/update/updater",
    ] {
        fs::create_dir_all(fake_source.join(dir))?;
    }
    install_branding(logo, &windows, &fake_source)?;
    for (name, image) in [
        ("document-256.png", document_icon(logo, 256)),
        ("document-32.png", document_icon(logo, 32)),
        ("document-16.png", document_icon(logo, 16)),
        ("private-256.png", private_icon(logo, 256)),
        ("private-32.png", private_icon(logo, 32)),
        ("private-16.png", private_icon(logo, 16)),
        ("wizWatermark.png", installer_watermark(logo)),
        ("wizHeader.png", installer_header(logo, false)),
    ] {
        write_file(&windows.join(name), icons::encode_png(&image)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logo() -> RgbaImage {
        icons::load_logo(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../branding")).unwrap()
    }

    /// Parse an `.ico` into `(size, is_png, data)` entries, checking the
    /// directory is consistent.
    fn parse_ico(data: &[u8]) -> Vec<(u32, bool, Vec<u8>)> {
        let u16_at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
        let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        assert_eq!(u16_at(0), 0);
        assert_eq!(u16_at(2), 1);
        let count = u16_at(4) as usize;
        let mut entries = Vec::new();
        let mut end = 6 + 16 * count;
        for i in 0..count {
            let e = 6 + 16 * i;
            let size = if data[e] == 0 { 256 } else { data[e] as u32 };
            assert_eq!(data[e], data[e + 1], "square");
            assert_eq!(u16_at(e + 6), 32);
            let (len, offset) = (u32_at(e + 8) as usize, u32_at(e + 12) as usize);
            assert_eq!(offset, end, "entries are contiguous");
            end = offset + len;
            let body = data[offset..offset + len].to_vec();
            let png = body.starts_with(b"\x89PNG");
            entries.push((size, png, body));
        }
        assert_eq!(end, data.len());
        entries
    }

    #[test]
    fn app_icon_has_every_windows_size() {
        let logo = logo();
        let images: Vec<RgbaImage> = WINDOWS_ICON_SIZES
            .iter()
            .map(|&s| icons::icon_image(&logo, s))
            .collect();
        let data = ico(&images).unwrap();
        let entries = parse_ico(&data);
        let sizes: Vec<u32> = entries.iter().map(|e| e.0).collect();
        assert_eq!(sizes, WINDOWS_ICON_SIZES);
        for (size, png, body) in entries {
            assert_eq!(png, size >= ICO_PNG_MIN_SIZE, "{size}");
            if png {
                let image = image::load_from_memory(&body).unwrap();
                assert_eq!(image.width(), size);
            } else {
                // BITMAPINFOHEADER with double height, 32 bpp, then pixels
                // and the mask.
                let header = u32::from_le_bytes(body[0..4].try_into().unwrap());
                assert_eq!(header, 40);
                let height = i32::from_le_bytes(body[8..12].try_into().unwrap());
                assert_eq!(height, 2 * size as i32);
                let stride = size.div_ceil(32) * 4;
                assert_eq!(body.len() as u32, 40 + size * size * 4 + stride * size);
            }
        }
    }

    #[test]
    fn bitmap_entries_keep_alpha_and_mask() {
        let mut image = RgbaImage::new(16, 16);
        image.put_pixel(3, 0, Rgba([10, 20, 30, 200]));
        let data = ico_bitmap(&image);
        // Top row is stored last; pixel (3, 0) is BGRA.
        let row = 40 + 15 * 16 * 4;
        assert_eq!(&data[row + 12..row + 16], &[30, 20, 10, 200]);
        // Mask rows: 4 bytes each, top row last; (3, 0) is opaque.
        let mask = 40 + 16 * 16 * 4 + 15 * 4;
        assert_eq!(data[mask], !(0x80u8 >> 3));
        assert_eq!(data[mask + 1], 0xff);
    }

    #[test]
    fn bmp_rows_are_padded_and_bottom_up() {
        let mut image = RgbaImage::from_pixel(3, 2, Rgba([255, 255, 255, 255]));
        image.put_pixel(0, 0, Rgba([1, 2, 3, 255]));
        let data = bmp(&image);
        assert_eq!(&data[0..2], b"BM");
        assert_eq!(data.len(), 54 + 12 * 2);
        // The top row comes second.
        assert_eq!(&data[54 + 12..54 + 15], &[3, 2, 1]);
        assert_eq!(&data[54 + 9..54 + 12], &[0, 0, 0], "padding");
    }

    #[test]
    fn installer_artwork_has_the_sizes_nsis_expects() {
        let logo = logo();
        let watermark = installer_watermark(&logo);
        assert_eq!(watermark.dimensions(), (164, 314));
        assert!(watermark.pixels().all(|p| p[3] == 255));
        let header = installer_header(&logo, false);
        assert_eq!(header.dimensions(), (150, 57));
        assert!(header.pixels().all(|p| p[3] == 255));
        // The logo sits at the right, at the left for right-to-left.
        let drawn = |img: &RgbaImage, x: u32| (0..57).any(|y| img.get_pixel(x, y)[0] < 200);
        assert!(drawn(&header, 120) && !drawn(&header, 20));
        let rtl = installer_header(&logo, true);
        assert!(drawn(&rtl, 30) && !drawn(&rtl, 130));
    }

    #[test]
    fn document_and_private_icons_render_at_every_size() {
        let logo = logo();
        for size in WINDOWS_SMALL_ICON_SIZES.iter().chain(&[256]) {
            for icon in [document_icon(&logo, *size), private_icon(&logo, *size)] {
                assert_eq!(icon.dimensions(), (*size, *size));
                let opaque = icon.pixels().filter(|p| p[3] > 128).count() as u32;
                assert!(opaque > size * size / 3, "{size}: {opaque}");
                // Corners are transparent.
                assert_eq!(icon.get_pixel(0, size - 1)[3], 0);
            }
        }
    }

    #[test]
    fn update_certificates_must_be_der() {
        let tmp = std::env::temp_dir().join(format!("hallow-certs-{}", std::process::id()));
        let dir = tmp.join("toolkit/mozapps/update/updater");
        fs::create_dir_all(&dir).unwrap();
        let pem = tmp.join("cert.pem");
        fs::write(&pem, "-----BEGIN CERTIFICATE-----\n").unwrap();
        assert!(install_update_certificates(&tmp, &pem, &pem).is_err());
        let der = tmp.join("cert.der");
        let mut fake = vec![0x30, 0x82, 0x05, 0x00];
        fake.resize(1300, 7);
        fs::write(&der, &fake).unwrap();
        install_update_certificates(&tmp, &der, &der).unwrap();
        assert_eq!(fs::read(dir.join("release_primary.der")).unwrap(), fake);
        assert_eq!(fs::read(dir.join("release_secondary.der")).unwrap(), fake);
        fs::remove_dir_all(&tmp).unwrap();
    }
}
