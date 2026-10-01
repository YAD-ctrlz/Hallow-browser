//! Rasterize the Hallow logo with resvg (pure Rust) so the PNG icon set is
//! always generated from `branding/logo.svg` instead of being committed.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

use crate::util::write_file;

/// Square icon sizes installed for the window icon and the desktop theme.
pub const ICON_SIZES: [u32; 8] = [16, 22, 24, 32, 48, 64, 128, 256];

pub fn load_logo(branding: &Path) -> Result<Vec<u8>> {
    let path = branding.join("logo.svg");
    fs::read(&path).with_context(|| format!("reading {}", path.display()))
}

/// Render `svg` into a `width`×`height` PNG, scaled to fit and centered.
pub fn render_png(svg: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let tree = Tree::from_data(svg, &Options::default()).context("parsing SVG")?;
    let size = tree.size();
    let scale = (width as f32 / size.width()).min(height as f32 / size.height());
    let dx = (width as f32 - size.width() * scale) / 2.0;
    let dy = (height as f32 - size.height() * scale) / 2.0;
    let mut pixmap =
        Pixmap::new(width, height).ok_or_else(|| anyhow!("bad icon size {width}x{height}"))?;
    resvg::render(
        &tree,
        Transform::from_row(scale, 0.0, 0.0, scale, dx, dy),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().context("encoding PNG")
}

/// Write `default<N>.png` for every size plus the about-dialog artwork.
pub fn render_all(branding: &Path, out: &Path) -> Result<()> {
    let logo = load_logo(branding)?;
    for size in ICON_SIZES {
        write_file(
            &out.join(format!("default{size}.png")),
            render_png(&logo, size, size)?,
        )?;
    }
    write_file(&out.join("about-logo.png"), render_png(&logo, 192, 192)?)?;
    write_file(&out.join("about-logo@2x.png"), render_png(&logo, 384, 384)?)?;
    write_file(&out.join("about.png"), render_png(&logo, 300, 236)?)?;
    eprintln!("rendered icons into {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_repository_logo_at_every_size() {
        let branding = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../branding");
        let logo = load_logo(&branding).unwrap();
        for size in ICON_SIZES {
            let png = render_png(&logo, size, size).unwrap();
            assert_eq!(&png[1..4], b"PNG");
            let decoded = Pixmap::decode_png(&png).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (size, size));
            // The logo must actually draw something.
            assert!(decoded.pixels().iter().any(|p| p.alpha() > 0));
        }
    }
}
