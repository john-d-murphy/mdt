//! Inline images in the preview, drawn with the terminal's own graphics — kitty's protocol,
//! sixels or iTerm2's — and as unicode half-blocks anywhere else, via `ratatui-image`.
//!
//! The markdown renderer turns `![alt](src)` into a [`RenderedBlock::Image`] carrying the
//! file's pixel size; re-wrapping gives it that many blank rows; after the text is drawn,
//! [`ImageState::draw`] paints each image over whichever of its rows are on screen — a
//! picture scrolled half off the top or bottom shows the half that is left, through the
//! crate's sliced rendering. Kitty places images through unicode placeholders in those cells,
//! so an image is exactly where its rows are and is gone the moment they are not drawn —
//! nothing lingers when the page scrolls.

use std::collections::HashMap;
use std::path::PathBuf;

use ratatui::layout::{Rect, Size};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::sliced::{SignedPosition, SlicedImage, SlicedProtocol};
use ratatui_image::{FilterType, Image, Resize};

use crate::markdown::{ImageLayout, RenderedBlock};
use crate::palette;

/// The most rows one image may take inline, however tall it is. A picture bigger than this
/// is still shown whole, just small — click it to open the viewer.
pub(crate) const MAX_IMAGE_ROWS: u16 = 24;

/// The viewer's zoom steps, as a percentage of "the whole picture, fit to the box".
const ZOOM_STEPS: [u32; 7] = [100, 150, 200, 300, 400, 600, 800];

/// The image viewer overlay: which file is on show, how far in, and where.
#[derive(Default)]
pub(crate) struct ImageViewerState {
    /// The file on show; `None` when the viewer is closed.
    pub(crate) path: Option<PathBuf>,
    /// Which of [`ZOOM_STEPS`] is in force.
    step: usize,
    /// Where the view is centred, as a fraction of the image's width and height.
    center: (f64, f64),
    /// The modal's rect as last drawn, so that a click outside it can close the viewer.
    pub(crate) area: Option<Rect>,
}

impl ImageViewerState {
    /// Show `path`, whole and centred.
    pub(crate) fn open(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.fit();
        self.area = None;
    }

    pub(crate) fn close(&mut self) {
        self.path = None;
        self.area = None;
    }

    /// The zoom in force, as a percentage.
    pub(crate) fn zoom(&self) -> u32 {
        ZOOM_STEPS[self.step.min(ZOOM_STEPS.len() - 1)]
    }

    pub(crate) fn zoom_in(&mut self) {
        self.step = (self.step + 1).min(ZOOM_STEPS.len() - 1);
        self.clamp_center();
    }

    pub(crate) fn zoom_out(&mut self) {
        self.step = self.step.saturating_sub(1);
        self.clamp_center();
    }

    /// The whole picture again, centred.
    pub(crate) fn fit(&mut self) {
        self.step = 0;
        self.center = (0.5, 0.5);
    }

    /// Move the view by a fifth of what is on show, `dx`/`dy` in units of one step.
    pub(crate) fn pan(&mut self, dx: f64, dy: f64) {
        let step = 0.2 * self.visible();
        self.center = (self.center.0 + dx * step, self.center.1 + dy * step);
        self.clamp_center();
    }

    /// The fraction of each axis on show at this zoom.
    fn visible(&self) -> f64 {
        100.0 / f64::from(self.zoom())
    }

    /// Keep the view inside the picture: at 100% there is nowhere to pan to.
    fn clamp_center(&mut self) {
        let half = self.visible() / 2.0;
        let (lo, hi) = (half.min(0.5), (1.0 - half).max(0.5));
        self.center = (self.center.0.clamp(lo, hi), self.center.1.clamp(lo, hi));
    }

    /// The region on show, for tests: see [`Self::crop`].
    #[cfg(test)]
    pub(crate) fn crop_for_test(&self, w: u32, h: u32) -> (u32, u32, u32, u32) {
        self.crop(w, h)
    }

    /// The region of a `w`×`h` picture on show: `(x, y, width, height)` in pixels.
    fn crop(&self, w: u32, h: u32) -> (u32, u32, u32, u32) {
        let (w, h) = (w.max(1), h.max(1));
        let visible = self.visible();
        let cw = (f64::from(w) * visible).round().max(1.0) as u32;
        let ch = (f64::from(h) * visible).round().max(1.0) as u32;
        let (cw, ch) = (cw.min(w), ch.min(h));
        let x = (f64::from(w) * self.center.0 - f64::from(cw) / 2.0).round().max(0.0) as u32;
        let y = (f64::from(h) * self.center.1 - f64::from(ch) / 2.0).round().max(0.0) as u32;
        (x.min(w - cw), y.min(h - ch), cw, ch)
    }
}

/// What a cached viewer encoding was made for: the file, the pixels on show, the box.
type ViewerKey = (PathBuf, (u32, u32, u32, u32), (u16, u16));

/// A decoded image and, once drawn, its encoding for the size it was last drawn at.
struct Cached {
    image: image::DynamicImage,
    sliced: Option<(Size, SlicedProtocol)>,
}

/// Terminal graphics: the picker that knows the protocol and cell size, the on/off switch,
/// and decoded images keyed by path (`None` where decoding failed).
pub(crate) struct ImageState {
    picker: Option<Picker>,
    enabled: bool,
    cache: HashMap<PathBuf, Option<Cached>>,
    /// The viewer's last encoding, kept so that a redraw at the same zoom costs nothing.
    viewer_encoding: Option<(ViewerKey, Protocol)>,
}

impl ImageState {
    /// No graphics at all: `--no-images`, or before the terminal has been asked.
    pub(crate) fn disabled() -> Self {
        Self { picker: None, enabled: false, cache: HashMap::new(), viewer_encoding: None }
    }

    /// Graphics through `picker`, switched on.
    pub(crate) fn with_picker(picker: Picker) -> Self {
        Self { picker: Some(picker), enabled: true, cache: HashMap::new(), viewer_encoding: None }
    }

    /// Whether the terminal was asked at all (false under `--no-images`).
    pub(crate) fn available(&self) -> bool {
        self.picker.is_some()
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled && self.picker.is_some()
    }

    /// Flip images on or off; returns the new state. Stays off when unavailable.
    pub(crate) fn toggle(&mut self) -> bool {
        self.enabled = self.available() && !self.enabled;
        self.enabled
    }

    /// The protocol in use, for the status line.
    pub(crate) fn protocol_name(&self) -> &'static str {
        match self.picker.as_ref().map(Picker::protocol_type) {
            Some(ProtocolType::Kitty) => "kitty",
            Some(ProtocolType::Sixel) => "sixel",
            Some(ProtocolType::Iterm2) => "iTerm2",
            Some(ProtocolType::Halfblocks) => "half-blocks",
            None => "off",
        }
    }

    /// How re-wrapping should lay images out right now.
    pub(crate) fn layout(&self) -> ImageLayout {
        match &self.picker {
            Some(picker) if self.enabled => {
                let font = picker.font_size();
                ImageLayout {
                    enabled: true,
                    cell_px: (font.width, font.height),
                    max_rows: MAX_IMAGE_ROWS,
                }
            }
            _ => ImageLayout::DISABLED,
        }
    }

    /// Forget decoded images (on opening another document).
    pub(crate) fn clear_cache(&mut self) {
        self.cache.clear();
        self.viewer_encoding = None;
    }

    /// Draw the viewer's picture at `area`: the region it is looking at, scaled to fill the
    /// box — enlarged past its natural size if need be, which is the point of the viewer.
    pub(crate) fn draw_viewer(&mut self, frame: &mut Frame, area: Rect, viewer: &ImageViewerState) {
        let Self { picker, cache, viewer_encoding, .. } = self;
        let (Some(picker), Some(path)) = (picker.as_ref(), viewer.path.as_ref()) else { return };
        if area.width == 0 || area.height == 0 {
            return;
        }

        let entry = cache
            .entry(path.clone())
            .or_insert_with(|| image::open(path).ok().map(|image| Cached { image, sliced: None }));
        let Some(cached) = entry else {
            let note = format!("[image: could not decode {}]", path.display());
            let style = Style::new().fg(palette::FG_MUTED).add_modifier(Modifier::ITALIC);
            frame.render_widget(Paragraph::new(Span::styled(note, style)), area);
            return;
        };

        let crop = viewer.crop(cached.image.width(), cached.image.height());
        let key = (path.clone(), crop, (area.width, area.height));
        if !matches!(viewer_encoding.as_ref(), Some((at, _)) if *at == key) {
            let region = cached.image.crop_imm(crop.0, crop.1, crop.2, crop.3);
            let size = Size::new(area.width, area.height);
            // `Scale` rather than `Fit`: `Fit` never enlarges, and a small picture shown
            // bigger is what the viewer is for.
            *viewer_encoding = picker
                .new_protocol(region, size, Resize::Scale(Some(FilterType::Triangle)))
                .ok()
                .map(|protocol| (key, protocol));
        }
        if let Some((_, protocol)) = viewer_encoding.as_ref() {
            // Centre it: the proportions are kept, so it rarely fills the box exactly.
            let size = protocol.size();
            let x = area.x + area.width.saturating_sub(size.width) / 2;
            let y = area.y + area.height.saturating_sub(size.height) / 2;
            let rect = Rect::new(x, y, size.width.min(area.width), size.height.min(area.height));
            Image::new(protocol).allow_clipping(true).render(rect, frame.buffer_mut());
        }
    }

    /// Paint every image block with rows on screen, including the visible part of one that is
    /// partly scrolled off. `starts` is the first rendered line of each block, `total_lines`
    /// the line count, `inner` the pane's text area whose first row is rendered line
    /// `scroll_offset`.
    pub(crate) fn draw(
        &mut self,
        frame: &mut Frame,
        blocks: &[RenderedBlock],
        starts: &[usize],
        total_lines: usize,
        scroll_offset: usize,
        inner: Rect,
    ) {
        if !self.enabled() {
            return;
        }
        let Some(picker) = self.picker.as_ref() else { return };
        let layout = self.layout();
        let viewport = usize::from(inner.height);

        for (i, block) in blocks.iter().enumerate() {
            let RenderedBlock::Image { path: Some(path), dims: Some(dims), .. } = block else {
                continue;
            };
            let Some(&start) = starts.get(i) else { continue };
            let end = starts.get(i + 1).copied().unwrap_or(total_lines);
            if end <= start || end <= scroll_offset || start >= scroll_offset + viewport {
                continue;
            }
            // The rows re-wrapping gave it, and the columns it is drawn at for this width.
            let rows = u16::try_from(end - start).unwrap_or(u16::MAX);
            let (cols, _) = layout.size_for(*dims, Some(usize::from(inner.width)));
            let size = Size::new(cols.min(inner.width), rows);
            // Where its top row sits relative to the pane: negative when scrolled off the top.
            let offset = i64::try_from(start).unwrap_or(i64::MAX)
                - i64::try_from(scroll_offset).unwrap_or(i64::MAX);
            let y = i16::try_from(offset).unwrap_or(if offset < 0 { i16::MIN } else { i16::MAX });

            let entry = self.cache.entry(path.clone()).or_insert_with(|| {
                image::open(path).ok().map(|image| Cached { image, sliced: None })
            });
            let Some(cached) = entry else {
                let note = format!("[image: could not decode {}]", path.display());
                let style = Style::new().fg(palette::FG_MUTED).add_modifier(Modifier::ITALIC);
                let row = inner.y.saturating_add(u16::try_from(offset.max(0)).unwrap_or(0));
                let area = Rect::new(inner.x, row, inner.width, 1);
                frame.render_widget(Paragraph::new(Span::styled(note, style)), area);
                continue;
            };
            // Encode once per size; the pane changing width is what makes it stale.
            if !matches!(cached.sliced.as_ref(), Some((at, _)) if *at == size) {
                cached.sliced = SlicedProtocol::new(picker, cached.image.clone(), Some(size))
                    .ok()
                    .map(|sliced| (size, sliced));
            }
            if let Some((_, sliced)) = cached.sliced.as_ref() {
                SlicedImage::new(sliced, SignedPosition { x: 0, y })
                    .render(inner, frame.buffer_mut());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_state_has_no_layout_and_cannot_toggle_on() {
        let mut s = ImageState::disabled();
        assert!(!s.available() && !s.enabled());
        assert_eq!(s.layout(), ImageLayout::DISABLED);
        assert!(!s.toggle());
        assert_eq!(s.protocol_name(), "off");
    }

    /// Draw one `w`×`h` red PNG as the only block, `scroll_offset` rendered lines into it, on
    /// a pane `pane_h` rows tall; return for each pane row whether anything was painted there.
    /// (Half-blocks paints colours into a space, kitty a placeholder symbol, so both count.)
    fn painted_rows(w: u32, h: u32, scroll_offset: usize, pane_h: u16) -> Vec<bool> {
        use ratatui::backend::TestBackend;
        use ratatui::buffer::Cell;
        use ratatui::Terminal;

        let dir = crate::test_util::TempTestDir::new("mdt-test-images-draw");
        let path = dir.path().join("red.png");
        image::RgbImage::from_pixel(w, h, image::Rgb([255, 0, 0])).save(&path).unwrap();

        let mut state = ImageState::with_picker(Picker::halfblocks());
        let rows = state.layout().rows_for((w, h), Some(40));
        let blocks = vec![RenderedBlock::Image {
            src: "red.png".to_string(),
            alt: String::new(),
            path: Some(path),
            dims: Some((w, h)),
        }];
        let pane = Rect::new(0, 0, 40, pane_h);
        let mut terminal = Terminal::new(TestBackend::new(40, pane_h)).unwrap();
        terminal
            .draw(|f| state.draw(f, &blocks, &[0], usize::from(rows), scroll_offset, pane))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..pane_h)
            .map(|y| {
                (0..40).any(|x| {
                    let cell = &buf[(x, y)];
                    // An untouched cell is a space with Reset colours — not `Style::default()`.
                    cell.symbol() != " " || cell.style() != Cell::EMPTY.style()
                })
            })
            .collect()
    }

    /// How many rows from the top of the pane were painted.
    fn drawn(painted: &[bool]) -> usize {
        painted.iter().take_while(|p| **p).count()
    }

    #[test]
    fn a_wholly_visible_image_paints_a_run_of_rows() {
        let painted = painted_rows(16, 80, 0, 20);
        let rows = drawn(&painted);
        assert!(rows >= 2, "a tall image paints several rows: {painted:?}");
        assert!(painted[rows..].iter().all(|p| !*p), "and nothing below it: {painted:?}");
    }

    #[test]
    fn an_image_scrolled_off_the_top_paints_one_row_fewer() {
        let whole = drawn(&painted_rows(16, 80, 0, 20));
        let painted = painted_rows(16, 80, 1, 20);
        assert!(painted[0], "what is left of it starts at the top row: {painted:?}");
        assert_eq!(drawn(&painted), whole - 1, "one row of it has scrolled off: {painted:?}");
    }

    #[test]
    fn an_image_running_off_the_bottom_paints_the_row_it_has() {
        assert_eq!(painted_rows(16, 80, 0, 1), [true], "a one-row pane still shows its top row");
    }

    #[test]
    fn picker_state_toggles_and_reports_its_protocol() {
        let mut s = ImageState::with_picker(Picker::halfblocks());
        assert!(s.enabled());
        assert_eq!(s.protocol_name(), "half-blocks");
        assert!(s.layout().enabled);
        assert_eq!(s.layout().max_rows, MAX_IMAGE_ROWS);
        assert!(!s.toggle());
        assert_eq!(s.layout(), ImageLayout::DISABLED);
        assert!(s.toggle());
    }
}
