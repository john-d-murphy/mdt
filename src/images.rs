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

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

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

/// A picture read from disk, scaled for the box it will fill, and encoded for the terminal
/// — the last two as and when they are wanted.
struct Cached {
    /// As read, kept at full size for the viewer to zoom into.
    image: image::DynamicImage,
    /// Scaled to a box, ready to encode. Done on the reading thread where it can be.
    scaled: Option<(Size, image::DynamicImage)>,
    /// Scaled and encoded for the terminal, for the box it was last drawn in.
    sliced: Option<(Size, SlicedProtocol)>,
}

/// What the reading thread hands back: the picture, and it scaled to the box it will fill.
struct Read {
    image: image::DynamicImage,
    scaled: Option<(Size, image::DynamicImage)>,
}

impl From<Read> for Cached {
    fn from(read: Read) -> Self {
        Self { image: read.image, scaled: read.scaled, sliced: None }
    }
}

/// What is on screen of a document, and the box it is drawn in.
pub(crate) struct PaneView<'a> {
    /// The document's blocks, in order.
    pub(crate) blocks: &'a [RenderedBlock],
    /// The first rendered line of each block.
    pub(crate) starts: &'a [usize],
    /// How many rendered lines the document has.
    pub(crate) total_lines: usize,
    /// The rendered line at the top of the pane.
    pub(crate) scroll_offset: usize,
    /// The width the document was wrapped to — which is what images were sized against.
    pub(crate) wrap_width: usize,
    /// The pane's text area, whose first row is line `scroll_offset`.
    pub(crate) inner: Rect,
}

/// Scale `image` down to fill a `size` box of `cell_px` cells, keeping its proportions.
///
/// A triangle filter rather than the nearest-neighbour the crate would use: this runs once
/// per picture, off the drawing thread, and it is what the reader actually looks at.
fn scale_to(image: &image::DynamicImage, size: Size, cell_px: (u16, u16)) -> image::DynamicImage {
    image.resize(
        u32::from(size.width) * u32::from(cell_px.0),
        u32::from(size.height) * u32::from(cell_px.1),
        image::imageops::FilterType::Triangle,
    )
}

/// Terminal graphics: the picker that knows the protocol and cell size, the on/off switch,
/// and decoded images keyed by path (`None` where decoding failed).
pub(crate) struct ImageState {
    picker: Option<Picker>,
    enabled: bool,
    cache: HashMap<PathBuf, Option<Cached>>,
    /// The viewer's last encoding, kept so that a redraw at the same zoom costs nothing.
    viewer_encoding: Option<(ViewerKey, Protocol)>,
    /// Pictures decoded on a background thread, waiting to be taken into the cache.
    decoded: (Sender<Decoded>, Receiver<Decoded>),
    /// What a background thread is decoding, so the same file is not queued twice.
    pending: HashSet<PathBuf>,
}

/// A file and what was read from it, or `None` if it could not be read.
type Decoded = (PathBuf, Option<Read>);

impl ImageState {
    /// No graphics at all: `--no-images`, or before the terminal has been asked.
    pub(crate) fn disabled() -> Self {
        Self {
            picker: None,
            enabled: false,
            cache: HashMap::new(),
            viewer_encoding: None,
            decoded: mpsc::channel(),
            pending: HashSet::new(),
        }
    }

    /// Graphics through `picker`, switched on.
    pub(crate) fn with_picker(picker: Picker) -> Self {
        Self { picker: Some(picker), enabled: true, ..Self::disabled() }
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

    /// Whether `path` has been read into the cache, for tests.
    #[cfg(test)]
    pub(crate) fn is_cached_for_test(&self, path: &std::path::Path) -> bool {
        self.cache.contains_key(path)
    }

    /// Take finished reads into the cache, for tests.
    #[cfg(test)]
    pub(crate) fn collect_for_test(&mut self) {
        self.take_decoded();
    }

    /// How many files a background thread is still reading, for tests.
    #[cfg(test)]
    pub(crate) fn pending_for_test(&self) -> usize {
        self.pending.len()
    }

    /// Forget decoded images (on opening another document).
    pub(crate) fn clear_cache(&mut self) {
        self.cache.clear();
        self.viewer_encoding = None;
    }

    /// Read a document's pictures on a background thread, and scale them there too, so that
    /// scrolling onto one costs nothing but the encode. Reading a megapixel JPEG and scaling
    /// it down are together almost all of the work of showing one.
    ///
    /// Cheap to call again — files already read, or already being read, are skipped — so the
    /// drawing code calls it every frame, which is also how it learns the box to scale to.
    pub(crate) fn prewarm(&mut self, blocks: &[RenderedBlock], wrap_width: usize) {
        if self.picker.is_none() {
            return;
        }
        let layout = self.layout();
        let mut jobs: Vec<(PathBuf, Option<Size>)> = blocks
            .iter()
            .filter_map(|block| match block {
                RenderedBlock::Image { path: Some(path), dims: Some(dims), .. } => {
                    Some((path.clone(), layout.box_for(*dims, wrap_width)))
                }
                _ => None,
            })
            .filter(|(path, _)| !self.cache.contains_key(path) && !self.pending.contains(path))
            .collect();
        jobs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        jobs.dedup_by(|a, b| a.0 == b.0);
        if jobs.is_empty() {
            return;
        }

        self.pending.extend(jobs.iter().map(|(path, _)| path.clone()));
        let tx = self.decoded.0.clone();
        let cell_px = layout.cell_px;
        std::thread::spawn(move || {
            for (path, box_size) in jobs {
                let read = image::open(&path).ok().map(|image| {
                    let scaled = box_size.map(|size| (size, scale_to(&image, size, cell_px)));
                    Read { image, scaled }
                });
                if tx.send((path, read)).is_err() {
                    return; // nobody is listening any more
                }
            }
        });
    }

    /// Take whatever has been read into the cache; `true` if anything arrived, which means
    /// the screen owes a redraw. Called from the event loop and before drawing.
    pub(crate) fn take_decoded(&mut self) -> bool {
        let mut arrived = false;
        while let Ok((path, read)) = self.decoded.1.try_recv() {
            self.pending.remove(&path);
            self.cache.entry(path).or_insert_with(|| read.map(Cached::from));
            arrived = true;
        }
        arrived
    }

    /// Draw the viewer's picture at `area`: the region it is looking at, scaled to fill the
    /// box — enlarged past its natural size if need be, which is the point of the viewer.
    pub(crate) fn draw_viewer(&mut self, frame: &mut Frame, area: Rect, viewer: &ImageViewerState) {
        self.take_decoded();
        let Self { picker, cache, viewer_encoding, .. } = self;
        let (Some(picker), Some(path)) = (picker.as_ref(), viewer.path.as_ref()) else { return };
        if area.width == 0 || area.height == 0 {
            return;
        }

        let entry = cache.entry(path.clone()).or_insert_with(|| {
            image::open(path).ok().map(|image| Read { image, scaled: None }.into())
        });
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

    /// Paint every image block with rows on screen, including the visible part of one that
    /// is partly scrolled off — see [`PaneView`] for what is where.
    pub(crate) fn draw(&mut self, frame: &mut Frame, view: &PaneView<'_>) {
        let &PaneView { blocks, starts, total_lines, scroll_offset, wrap_width, inner } = view;
        self.take_decoded();
        if !self.enabled() {
            return;
        }
        // Reading and scaling happen on another thread; this is where that is set going,
        // since the box to scale to is only known once there is a pane to draw into.
        self.prewarm(blocks, wrap_width);

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
            // The rows re-wrapping gave it, and the columns it is drawn at for that width.
            let rows = u16::try_from(end - start).unwrap_or(u16::MAX);
            let (cols, _) = layout.size_for(*dims, Some(wrap_width));
            let size = Size::new(cols.min(inner.width), rows);
            // Where its top row sits relative to the pane: negative when scrolled off the top.
            let offset = i64::try_from(start).unwrap_or(i64::MAX)
                - i64::try_from(scroll_offset).unwrap_or(i64::MAX);
            let y = i16::try_from(offset).unwrap_or(if offset < 0 { i16::MIN } else { i16::MAX });

            // Still being read: leave its rows blank rather than stall the whole frame
            // reading it here. It appears a moment later, when the read lands.
            let Some(entry) = self.cache.get_mut(path) else { continue };
            let Some(cached) = entry else {
                let note = format!("[image: could not read {}]", path.display());
                let style = Style::new().fg(palette::FG_MUTED).add_modifier(Modifier::ITALIC);
                let row = inner.y.saturating_add(u16::try_from(offset.max(0)).unwrap_or(0));
                let area = Rect::new(inner.x, row, inner.width, 1);
                frame.render_widget(Paragraph::new(Span::styled(note, style)), area);
                continue;
            };
            // Encode once per size; the pane changing width is what makes it stale. The
            // scaling was done on the reading thread, unless the pane has changed width
            // since — then it has to happen here.
            if !matches!(cached.sliced.as_ref(), Some((at, _)) if *at == size) {
                let scaled = match cached.scaled.take() {
                    Some((at, scaled)) if at == size => scaled,
                    _ => scale_to(&cached.image, size, layout.cell_px),
                };
                cached.sliced =
                    SlicedProtocol::new(picker, scaled, Some(size)).ok().map(|s| (size, s));
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
            path: Some(path.clone()),
            dims: Some((w, h)),
        }];
        let pane = Rect::new(0, 0, 40, pane_h);
        let mut terminal = Terminal::new(TestBackend::new(40, pane_h)).unwrap();

        // The first draw sets the reading going on another thread; the picture is painted
        // on a later one, once it has landed.
        let mut landed = false;
        for _ in 0..200 {
            let view = PaneView {
                blocks: &blocks,
                starts: &[0],
                total_lines: usize::from(rows),
                scroll_offset,
                wrap_width: 40,
                inner: pane,
            };
            terminal.draw(|f| state.draw(f, &view)).unwrap();
            if state.is_cached_for_test(&path) {
                landed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(landed, "the picture was not read within two seconds");
        let view = PaneView {
            blocks: &blocks,
            starts: &[0],
            total_lines: usize::from(rows),
            scroll_offset,
            wrap_width: 40,
            inner: pane,
        };
        terminal.draw(|f| state.draw(f, &view)).unwrap();
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
    fn prewarm_reads_a_documents_pictures_in_the_background() {
        let dir = crate::test_util::TempTestDir::new("mdt-test-images-prewarm");
        let path = dir.path().join("red.png");
        image::RgbImage::from_pixel(32, 32, image::Rgb([255, 0, 0])).save(&path).unwrap();
        let blocks = vec![RenderedBlock::Image {
            src: "red.png".to_string(),
            alt: String::new(),
            path: Some(path.clone()),
            dims: Some((32, 32)),
        }];

        let mut state = ImageState::with_picker(Picker::halfblocks());
        state.prewarm(&blocks, 40);
        assert_eq!(state.pending_for_test(), 1, "it is queued, not read on this thread");

        // The worker is a thread; give it a moment, taking what it has finished.
        for _ in 0..200 {
            state.collect_for_test();
            if state.is_cached_for_test(&path) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(state.is_cached_for_test(&path), "the picture was not read within two seconds");
        assert_eq!(state.pending_for_test(), 0);

        // Already read: nothing queued a second time.
        state.prewarm(&blocks, 40);
        assert_eq!(state.pending_for_test(), 0);
    }

    #[test]
    fn prewarm_skips_remote_and_missing_pictures_and_needs_a_picker() {
        let blocks = vec![
            RenderedBlock::Image {
                src: "https://x.example/a.png".to_string(),
                alt: String::new(),
                path: None,
                dims: None,
            },
            RenderedBlock::Image {
                src: "gone.png".to_string(),
                alt: String::new(),
                path: Some(PathBuf::from("/nowhere/gone.png")),
                dims: None,
            },
        ];
        let mut state = ImageState::with_picker(Picker::halfblocks());
        state.prewarm(&blocks, 40);
        assert_eq!(state.pending_for_test(), 0, "nothing on disk to read");

        let mut off = ImageState::disabled();
        off.prewarm(&blocks, 40);
        assert_eq!(off.pending_for_test(), 0, "no picker, no reading");
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
