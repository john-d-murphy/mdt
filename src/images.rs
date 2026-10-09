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
use ratatui_image::sliced::{SignedPosition, SlicedImage, SlicedProtocol};

use crate::markdown::{ImageLayout, RenderedBlock};
use crate::palette;

/// The most rows one image may take, however tall it is.
pub(crate) const MAX_IMAGE_ROWS: u16 = 24;

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
}

impl ImageState {
    /// No graphics at all: `--no-images`, or before the terminal has been asked.
    pub(crate) fn disabled() -> Self {
        Self { picker: None, enabled: false, cache: HashMap::new() }
    }

    /// Graphics through `picker`, switched on.
    pub(crate) fn with_picker(picker: Picker) -> Self {
        Self { picker: Some(picker), enabled: true, cache: HashMap::new() }
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
