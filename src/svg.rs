//! SVG pictures: the size they ask for, and rasterising them.
//!
//! The `image` crate cannot read SVG at all, so these are rendered with `resvg` into an
//! ordinary RGBA picture before anything else sees them. A vector has no pixel size of its
//! own, so one is chosen — [`RASTER_PX`] on the longest side — big enough to stand being
//! zoomed into in the viewer.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use resvg::{tiny_skia, usvg};

/// Pixels on the longest side of a rasterised SVG.
const RASTER_PX: f32 = 2048.0;

/// What an SVG with words in it contains, somewhere.
const TEXT_TAG: &[u8] = b"<text";

/// Whether `path` names an SVG, by its extension.
pub(crate) fn is_svg(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

/// The size an SVG asks to be drawn at, in whole pixels. `None` for anything else, and for
/// an SVG that cannot be parsed.
pub(crate) fn size(path: &Path) -> Option<(u32, u32)> {
    if !is_svg(path) {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    let size = parse(&data, path)?.size();
    Some((size.width().ceil() as u32, size.height().ceil() as u32))
}

/// Rasterise `path` at [`RASTER_PX`] on its longest side, keeping its proportions.
pub(crate) fn render(path: &Path) -> Option<image::DynamicImage> {
    let data = std::fs::read(path).ok()?;
    let tree = parse(&data, path)?;

    let size = tree.size();
    let longest = size.width().max(size.height());
    if !longest.is_finite() || longest <= 0.0 {
        return None;
    }
    let scale = RASTER_PX / longest;
    let width = (size.width() * scale).round().max(1.0) as u32;
    let height = (size.height() * scale).round().max(1.0) as u32;

    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());

    let mut data = pixmap.take();
    demultiply(&mut data);
    Some(image::DynamicImage::ImageRgba8(image::RgbaImage::from_raw(width, height, data)?))
}

/// Parse `data`, resolving any files it refers to beside `path`.
fn parse(data: &[u8], path: &Path) -> Option<usvg::Tree> {
    let mut options = usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        ..usvg::Options::default()
    };
    // Reading the system's fonts takes a moment, so only do it for an SVG with words in it.
    if data.windows(TEXT_TAG.len()).any(|window| window == TEXT_TAG) {
        options.fontdb = fonts();
    }
    usvg::Tree::from_data(data, &options).ok()
}

/// The system's fonts, read once and shared.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

/// Turn premultiplied RGBA, which is what `tiny_skia` draws, into the straight alpha the
/// `image` crate and the terminal protocols expect. Without this, anything partly
/// transparent comes out too dark.
fn demultiply(data: &mut [u8]) {
    for pixel in data.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 || alpha == 255 {
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel = u8::try_from((u32::from(*channel) * 255 / alpha).min(255)).unwrap_or(255);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempTestDir;

    /// The shape quine's cached SVGs have: a percentage width and height, with the real size
    /// in the viewBox.
    const ICON: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg width="100%" height="100%" viewBox="0 0 40 20" xmlns="http://www.w3.org/2000/svg">
  <rect x="0" y="0" width="40" height="20" style="fill:#c03030"/>
</svg>
"#;

    fn write(dir: &TempTestDir, name: &str, body: &str) -> std::path::PathBuf {
        dir.create_file(name, body);
        dir.path().join(name)
    }

    #[test]
    fn is_svg_goes_by_the_extension_whatever_its_case() {
        assert!(is_svg(Path::new("a/b.svg")));
        assert!(is_svg(Path::new("a/b.SVG")));
        assert!(!is_svg(Path::new("a/b.png")));
        assert!(!is_svg(Path::new("a/svg")));
    }

    #[test]
    fn size_comes_from_the_view_box_when_the_width_is_a_percentage() {
        let dir = TempTestDir::new("mdt-test-svg-size");
        assert_eq!(size(&write(&dir, "icon.svg", ICON)), Some((40, 20)));
    }

    #[test]
    fn size_is_none_for_other_files_and_for_nonsense() {
        let dir = TempTestDir::new("mdt-test-svg-size-none");
        assert_eq!(size(&write(&dir, "not.png", ICON)), None, "not by extension");
        assert_eq!(size(&write(&dir, "bad.svg", "<svg")), None, "unparseable");
        assert_eq!(size(&dir.path().join("missing.svg")), None, "not there");
    }

    #[test]
    fn rendering_fills_the_longest_side_and_keeps_the_proportions() {
        let dir = TempTestDir::new("mdt-test-svg-render");
        let image = render(&write(&dir, "icon.svg", ICON)).expect("rasterised");
        assert_eq!(image.width(), RASTER_PX as u32, "the longest side is filled");
        assert_eq!(image.height(), RASTER_PX as u32 / 2, "and the proportions kept");

        // The red rectangle covers the whole picture, opaque.
        let pixel = image.to_rgba8().get_pixel(image.width() / 2, image.height() / 2).0;
        assert_eq!(pixel, [0xc0, 0x30, 0x30, 0xff], "the paint came out as written");
    }

    #[test]
    fn rendering_nonsense_is_none() {
        let dir = TempTestDir::new("mdt-test-svg-render-bad");
        assert!(render(&write(&dir, "bad.svg", "not an svg at all")).is_none());
    }

    #[test]
    fn demultiply_restores_the_colour_under_a_half_transparent_pixel() {
        // Premultiplied half-transparent red: 0x80 of 0xff, scaled by the alpha.
        let mut data = vec![0x80, 0x00, 0x00, 0x80, 0xff, 0x00, 0x00, 0xff];
        demultiply(&mut data);
        assert_eq!(data[0], 0xff, "the red comes back up to full");
        assert_eq!(data[3], 0x80, "and the alpha is left alone");
        assert_eq!(&data[4..], [0xff, 0x00, 0x00, 0xff], "an opaque pixel is untouched");
    }
}
