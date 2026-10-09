//! Application state and logic.

mod document;
mod event;
mod file_finder;
mod image_viewer;
mod link_picker;
mod state;
mod tree;
mod types;
mod watcher_handlers;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tui_tree_widget::TreeState;

use crate::file_tree;
use crate::markdown::{
    deduplicate_links, render_markdown_blocks, render_markdown_blocks_with_source_map,
    rewrap_blocks, ImageLayout,
};

pub use types::{AppMode, Focus};
pub(crate) use types::{FileOp, Overlay, SplitOrientation};

pub(crate) use document::{DocumentState, TreeViewState};
pub(crate) use state::{
    CursorState, EditorState, FileFinderState, LinkPickerState, LivePreviewState, SearchState,
};

/// Top-level application state.
pub struct App {
    pub(crate) search: SearchState,
    pub(crate) editor: EditorState,
    pub(crate) tree: TreeViewState,
    pub(crate) document: DocumentState,
    pub(crate) link_picker: LinkPickerState,
    pub(crate) file_finder: FileFinderState,
    pub(crate) cursor: CursorState,
    pub(crate) mode: AppMode,
    pub(crate) focus: Focus,
    pub(crate) should_quit: bool,
    pub(crate) status_message: String,
    pub(crate) pending_key: Option<(char, Instant)>,
    pub(crate) command_buffer: String,
    pub(crate) overlay: Overlay,
    pub(crate) file_op_input: String,
    pub(crate) show_file_tree: bool,
    pub(crate) bg_color: ratatui::style::Color,
    pub(crate) root_path: PathBuf,
    pub(crate) max_file_size: u64,
    /// Maximum render width in columns (`--max-width` / `MDT_MAX_WIDTH`); `None` = no limit.
    pub(crate) max_width: Option<usize>,
    pub(crate) preview_area: Option<ratatui::layout::Rect>,
    pub(crate) file_list_area: Option<ratatui::layout::Rect>,
    pub(crate) live_preview: LivePreviewState,
    pub(crate) stdin_mode: bool,
    /// Terminal graphics for inline images.
    pub(crate) images: crate::images::ImageState,
    /// The image viewer overlay — which picture is open, and how it is being looked at.
    pub(crate) image_viewer: crate::images::ImageViewerState,
}

impl App {
    /// Default maximum file size (5 MB).
    pub const DEFAULT_MAX_FILE_SIZE: u64 = 5_000_000;

    /// Wrap width for markdown rendering given the available pane width.
    ///
    /// Applies the `--max-width` cap (if set) so long lines wrap at a readable
    /// column count even on wide terminals, similar to `less`'s max width.
    pub(crate) fn render_width(&self, available: usize) -> usize {
        self.max_width.map_or(available, |max| available.min(max))
    }

    /// Create a new `App` rooted at `path`.
    ///
    /// If `path` is a file, the tree is rooted at its parent directory and the
    /// file is opened and selected in the tree (when it appears there).
    pub fn new(path: &Path, bg_color: ratatui::style::Color) -> anyhow::Result<Self> {
        let canonical = std::fs::canonicalize(path)?;
        let (root_path, initial_file) = if canonical.is_file() {
            let parent = canonical
                .parent()
                .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", canonical.display()))?
                .to_path_buf();
            (parent, Some(canonical))
        } else {
            (canonical, None)
        };

        let (tree_items, path_map) = file_tree::build_tree_items(&root_path)?;
        let mut tree_state = TreeState::default();
        // Files passed on the command line sit directly under the root, so their
        // tree identifier is just the file name.
        let initial_id = initial_file
            .as_ref()
            .and_then(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|id| path_map.contains_key(id));
        if let Some(id) = initial_id {
            tree_state.select(vec![id]);
        } else if let Some(first_item) = tree_items.first() {
            tree_state.select(vec![first_item.identifier().clone()]);
        }

        // Initialize viewport dimensions from terminal size so the first file
        // open can wrap to the correct width instead of wrapping with None
        // (which forces an immediate re-wrap on the next draw frame).
        let (init_width, init_height) = crossterm::terminal::size().unwrap_or((80, 24));

        let mut app = Self {
            tree: TreeViewState {
                tree_state,
                tree_items,
                path_map,
                filtered_tree_items: None,
                filtered_path_map: None,
            },
            document: DocumentState {
                current_file: None,
                file_content: String::new(),
                rendered_lines: Vec::new(),
                rendered_lines_lower: Vec::new(),
                rendered_blocks: Vec::new(),
                links: Vec::new(),
                heading_line_offsets: Vec::new(),
                block_line_starts: Vec::new(),
                scroll_offset: 0,
                viewport_height: init_height as usize,
                viewport_width: init_width as usize,
            },
            search: SearchState::default(),
            editor: EditorState::default(),
            link_picker: LinkPickerState::default(),
            file_finder: FileFinderState::default(),
            cursor: CursorState { visible: true, last_toggle: Instant::now() },
            mode: AppMode::Normal,
            focus: Focus::FileList,
            should_quit: false,
            status_message: String::new(),
            pending_key: None,
            command_buffer: String::new(),
            overlay: Overlay::None,
            file_op_input: String::new(),
            show_file_tree: false,
            bg_color,
            root_path,
            max_file_size: Self::DEFAULT_MAX_FILE_SIZE,
            max_width: None,
            preview_area: None,
            file_list_area: None,
            live_preview: LivePreviewState::default(),
            stdin_mode: false,
            images: crate::images::ImageState::disabled(),
            image_viewer: crate::images::ImageViewerState::default(),
        };

        if let Some(file) = initial_file {
            app.open_file(&file);
        }

        Ok(app)
    }

    /// Create an `App` for piped stdin content (read-only, no file tree).
    pub fn from_stdin(content: String, bg_color: ratatui::style::Color) -> Self {
        let (init_width, init_height) = crossterm::terminal::size().unwrap_or((80, 24));

        let (blocks, links) = render_markdown_blocks(&content, None);
        let links = deduplicate_links(links);
        let width = if init_width > 0 { Some(init_width as usize) } else { None };
        let (rendered, block_line_starts) = rewrap_blocks(&blocks, width, ImageLayout::DISABLED);

        let mut document = DocumentState {
            current_file: None,
            file_content: content,
            rendered_lines: rendered,
            rendered_lines_lower: Vec::new(),
            rendered_blocks: blocks,
            links,
            heading_line_offsets: Vec::new(),
            block_line_starts,
            scroll_offset: 0,
            viewport_height: init_height as usize,
            viewport_width: init_width as usize,
        };
        document.rebuild_lower_cache();
        document.rebuild_heading_index();

        Self {
            tree: TreeViewState {
                tree_state: TreeState::default(),
                tree_items: Vec::new(),
                path_map: HashMap::default(),
                filtered_tree_items: None,
                filtered_path_map: None,
            },
            document,
            search: SearchState::default(),
            editor: EditorState::default(),
            link_picker: LinkPickerState::default(),
            file_finder: FileFinderState::default(),
            cursor: CursorState { visible: true, last_toggle: Instant::now() },
            mode: AppMode::Normal,
            focus: Focus::Preview,
            should_quit: false,
            status_message: String::new(),
            pending_key: None,
            command_buffer: String::new(),
            overlay: Overlay::None,
            file_op_input: String::new(),
            show_file_tree: false,
            bg_color,
            root_path: PathBuf::new(),
            max_file_size: Self::DEFAULT_MAX_FILE_SIZE,
            max_width: None,
            preview_area: None,
            file_list_area: None,
            live_preview: LivePreviewState::default(),
            stdin_mode: true,
            images: crate::images::ImageState::disabled(),
            image_viewer: crate::images::ImageViewerState::default(),
        }
    }

    /// Toggle the cursor blink state every ~530ms.
    ///
    /// Advances by the fixed interval rather than resetting to `Instant::now()`
    /// to prevent drift from event-loop latency.
    pub fn tick_cursor(&mut self) {
        let interval = Duration::from_millis(530);
        if self.cursor.last_toggle.elapsed() >= interval {
            self.cursor.visible = !self.cursor.visible;
            self.cursor.last_toggle += interval;
        }
    }

    /// Get the display path for the current file (relative to root).
    pub(crate) fn display_file_path(&self) -> String {
        if self.stdin_mode {
            return "<stdin>".to_string();
        }
        self.document
            .current_file
            .as_ref()
            .map(|p| p.strip_prefix(&self.root_path).unwrap_or(p).to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Toggle live preview on/off.
    pub(crate) fn toggle_live_preview(&mut self) {
        self.live_preview.enabled = !self.live_preview.enabled;
        if self.live_preview.enabled {
            self.update_live_preview();
            self.status_message = "Live preview ON".to_string();
        } else {
            self.status_message = "Live preview OFF".to_string();
        }
    }

    /// Swap split orientation between horizontal and vertical.
    pub(crate) fn toggle_split_orientation(&mut self) {
        self.live_preview.orientation = match self.live_preview.orientation {
            SplitOrientation::Horizontal => {
                self.status_message = "Preview split: vertical".to_string();
                SplitOrientation::Vertical
            }
            SplitOrientation::Vertical => {
                self.status_message = "Preview split: horizontal".to_string();
                SplitOrientation::Horizontal
            }
        };
    }

    /// Re-wrap the document and the live preview from their cached blocks at their current
    /// widths — after the image layout changes (the terminal answered, or `:images`).
    pub(crate) fn rewrap_views(&mut self) {
        let layout = self.images.layout();
        if !self.document.rendered_blocks.is_empty() {
            let width = (self.document.viewport_width > 0).then_some(self.document.viewport_width);
            let (lines, starts) = rewrap_blocks(&self.document.rendered_blocks, width, layout);
            self.document.rendered_lines = lines;
            self.document.block_line_starts = starts;
            self.document.rebuild_lower_cache();
            self.document.rebuild_heading_index();
            self.document.clamp_scroll();
        }
        if !self.live_preview.rendered_blocks.is_empty() {
            let width =
                (self.live_preview.viewport_width > 0).then_some(self.live_preview.viewport_width);
            let (lines, starts) = rewrap_blocks(&self.live_preview.rendered_blocks, width, layout);
            self.live_preview.rendered_lines = lines;
            self.live_preview.block_line_starts = starts;
        }
    }

    /// `:images` / `Ctrl+i`: flip images on or off, re-wrap, and say so in the status bar.
    pub(crate) fn toggle_images(&mut self) {
        if !self.images.available() {
            self.status_message = "Images are off for this run (--no-images)".to_string();
            return;
        }
        let on = self.images.toggle();
        self.rewrap_views();
        self.status_message = if on {
            format!("Images ON ({})", self.images.protocol_name())
        } else {
            "Images OFF".to_string()
        };
    }

    /// Re-render live preview from editor buffer content.
    pub(crate) fn update_live_preview(&mut self) {
        let Some(ref textarea) = self.editor.textarea else {
            return;
        };
        let content = textarea.lines().join("\n");
        let base_dir = self.document.current_file.as_deref().and_then(Path::parent);
        let (blocks, _links, source_lines) =
            render_markdown_blocks_with_source_map(&content, base_dir);
        let width = if self.live_preview.viewport_width > 0 {
            Some(self.live_preview.viewport_width)
        } else if self.document.viewport_width > 0 {
            Some(self.document.viewport_width)
        } else {
            None
        };
        let (rendered, block_line_starts) = rewrap_blocks(&blocks, width, self.images.layout());
        self.live_preview.rendered_lines = rendered;
        self.live_preview.rendered_blocks = blocks;
        self.live_preview.block_line_starts = block_line_starts;
        self.live_preview.block_source_lines = source_lines;
        self.live_preview.debounce = None;
    }

    /// Read a file, render its markdown, and store the result.
    pub(crate) fn open_file(&mut self, path: &Path) {
        let limit = self.max_file_size;
        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.len() > limit {
                let mb = limit / 1_000_000;
                self.status_message = format!("File too large (>{mb}MB)");
                return;
            }
        }

        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.status_message = format!("Error: {e}");
                return;
            }
        };

        let Ok(content) = String::from_utf8(bytes) else {
            self.status_message = "Binary file, cannot preview".to_string();
            return;
        };

        let (blocks, links) = render_markdown_blocks(&content, path.parent());
        let links = deduplicate_links(links);
        let width = if self.document.viewport_width > 0 {
            Some(self.document.viewport_width)
        } else {
            None
        };
        self.images.clear_cache();
        let (rendered, block_line_starts) = rewrap_blocks(&blocks, width, self.images.layout());
        self.document.rendered_lines = rendered;
        self.document.rebuild_lower_cache();
        self.document.block_line_starts = block_line_starts;
        self.document.rendered_blocks = blocks;
        self.document.links = links;
        self.document.file_content = content;
        self.document.current_file = Some(path.to_path_buf());
        self.document.scroll_offset = 0;
        self.document.rebuild_heading_index();
        self.status_message.clear();

        if !self.show_file_tree {
            self.focus = Focus::Preview;
        }
    }
}
