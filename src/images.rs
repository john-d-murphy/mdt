//! Inline images in the preview, drawn with the terminal's own graphics — kitty's protocol,
//! sixels or iTerm2's — and as unicode half-blocks anywhere else, via `ratatui-image`.
//!
//! The markdown renderer turns `![alt](src)` into a [`RenderedBlock::Image`] carrying the
//! file's pixel size; re-wrapping gives it that many blank rows; after the text is drawn,
//! [`ImageState::draw`] paints each wholly visible image over its rows. Kitty places images
//! through unicode placeholders in those cells, so an image is exactly where its rows are and
//! is gone the moment they are not drawn — nothing lingers when the page scrolls.

use std::collections::HashMap;
use std::path::PathBuf;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::StatefulImage;

use crate::markdown::{ImageLayout, RenderedBlock};
use crate::palette;

/// The most rows one image may take, however tall it is.
pub(crate) const MAX_IMAGE_ROWS: u16 = 24;

/// Terminal graphics: the picker that knows the protocol and cell size, the on/off switch,
/// and decoded images keyed by path.
pub(crate) struct ImageState {
    picker: Option<Picker>,
    enabled: bool,
    cache: HashMap<PathBuf, Option<StatefulProtocol>>,
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

    /// Paint every image block whose rows are all on screen. `starts` is the first rendered
    /// line of each block, `total_lines` the line count, `inner` the pane's text area whose
    /// first row is rendered line `scroll_offset`. An image only partly scrolled in is left
    /// blank until it is wholly visible.
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
        let viewport = usize::from(inner.height);

        for (i, block) in blocks.iter().enumerate() {
            let RenderedBlock::Image { path: Some(path), dims: Some(_), .. } = block else {
                continue;
            };
            let Some(&start) = starts.get(i) else { continue };
            let end = starts.get(i + 1).copied().unwrap_or(total_lines);
            if end <= start || start < scroll_offset || end > scroll_offset + viewport {
                continue;
            }
            let rows = u16::try_from(end - start).unwrap_or(u16::MAX);
            let y = inner.y.saturating_add(u16::try_from(start - scroll_offset).unwrap_or(0));
            let area = Rect::new(inner.x, y, inner.width, rows);

            let entry = self.cache.entry(path.clone()).or_insert_with(|| {
                image::open(path).ok().map(|img| picker.new_resize_protocol(img))
            });
            if let Some(protocol) = entry {
                frame.render_stateful_widget(StatefulImage::default(), area, protocol);
            } else {
                let note = format!("[image: could not decode {}]", path.display());
                let style = Style::new().fg(palette::FG_MUTED).add_modifier(Modifier::ITALIC);
                frame.render_widget(
                    Paragraph::new(Span::styled(note, style)),
                    Rect { height: 1, ..area },
                );
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
