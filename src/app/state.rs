//! Small state structs: search, editor, link picker, cursor.

use std::time::Instant;

use ratatui::text::Line;
use ratatui_textarea::TextArea;

use crate::app::types::SplitOrientation;
use crate::markdown::RenderedBlock;

/// Search-related state (both file search and in-document search).
#[derive(Default)]
pub(crate) struct SearchState {
    pub(crate) active: bool,
    pub(crate) query: String,
    pub(crate) matches: Vec<usize>,
    pub(crate) current: usize,
}

/// Editor (TextArea) state.
#[derive(Default)]
pub(crate) struct EditorState {
    pub(crate) textarea: Option<TextArea<'static>>,
    pub(crate) is_dirty: bool,
    pub(crate) external_change_detected: bool,
}

/// Link picker overlay state.
#[derive(Default)]
pub(crate) struct LinkPickerState {
    pub(crate) selected: usize,
    pub(crate) search_query: String,
    pub(crate) cached_indices: Vec<usize>,
    pub(crate) cached_query: String,
    pub(crate) cached_count: usize,
}

/// File finder overlay state.
#[derive(Default)]
pub(crate) struct FileFinderState {
    pub(crate) query: String,
    pub(crate) selected: usize,
    pub(crate) results: Vec<(String, std::path::PathBuf)>,
}

/// Cursor blink state for overlays with text input.
pub(crate) struct CursorState {
    pub(crate) visible: bool,
    pub(crate) last_toggle: Instant,
}

/// Live preview state for split-pane editing.
#[derive(Default)]
pub(crate) struct LivePreviewState {
    pub(crate) enabled: bool,
    pub(crate) orientation: SplitOrientation,
    pub(crate) debounce: Option<Instant>,
    pub(crate) rendered_lines: Vec<Line<'static>>,
    pub(crate) rendered_blocks: Vec<RenderedBlock>,
    pub(crate) scroll_offset: usize,
    pub(crate) viewport_width: usize,
    /// Per-block starting rendered line (from `rewrap_blocks`).
    pub(crate) block_line_starts: Vec<usize>,
    /// Per-block starting source line (0-based, from offset tracking).
    pub(crate) block_source_lines: Vec<usize>,
    /// Editor inner height (lines visible without scrolling). Set during draw.
    pub(crate) editor_inner_height: usize,
    /// Estimated editor viewport scroll top (tracks when editor actually scrolls).
    pub(crate) estimated_scroll_top: usize,
}

/// Text selected by dragging over the preview, in document coordinates: a rendered line
/// index and a display column. Nothing is put on the clipboard — a selection is there to be
/// read, and to be handed to whatever a note is made with.
#[derive(Default)]
pub(crate) struct SelectionState {
    /// Where the drag started.
    anchor: Option<(usize, usize)>,
    /// Where the pointer is now.
    head: Option<(usize, usize)>,
    /// Whether the pointer has moved since the button went down. A click that never moved
    /// opens a link or an image instead of selecting anything.
    dragged: bool,
}

impl SelectionState {
    /// The button went down at `line`, `col`: the start of a selection, or of a click.
    pub(crate) fn press(&mut self, line: usize, col: usize) {
        self.anchor = Some((line, col));
        self.head = None;
        self.dragged = false;
    }

    /// The pointer moved with the button down.
    pub(crate) fn drag(&mut self, line: usize, col: usize) {
        if self.anchor.is_some() {
            self.head = Some((line, col));
            self.dragged = true;
        }
    }

    /// Whether the last press turned into a drag rather than a click.
    pub(crate) fn dragged(&self) -> bool {
        self.dragged
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    /// The selected span as `((line, column), (line, column))`, the first before the second
    /// and the second one past the last selected cell. `None` when nothing is selected.
    pub(crate) fn range(&self) -> Option<((usize, usize), (usize, usize))> {
        let (anchor, head) = (self.anchor?, self.head?);
        let (first, last) = if anchor <= head { (anchor, head) } else { (head, anchor) };
        Some((first, (last.0, last.1 + 1)))
    }
}
