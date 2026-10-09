//! Markdown renderer — converts pulldown-cmark events into width-independent blocks.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Tag, TagEnd};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use super::blocks::RenderedBlock;
use std::path::{Path, PathBuf};

use super::link_line::{parse_link_line, Tail};
use super::syntax::highlight_code;
use super::theme::*;

/// Metadata for a link found in the markdown document.
#[derive(Clone, Debug)]
pub struct LinkInfo {
    pub display_text: String,
    pub url: String,
}

/// Convert a raw URL into a human-readable label by stripping scheme and www prefix.
pub(crate) fn humanize_url(url: &str) -> String {
    let stripped =
        url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).unwrap_or(url);
    let stripped = stripped.strip_prefix("www.").unwrap_or(stripped);
    let stripped = stripped.strip_suffix('/').unwrap_or(stripped);
    stripped.to_string()
}

/// Where an image's `src` points on disk, and its pixel size if that file can be read.
/// URLs and data URIs resolve to nothing: only local files are drawn. A `file://` prefix,
/// and any `?query` or `#fragment`, are dropped.
fn resolve_image(src: &str, base_dir: Option<&Path>) -> (Option<PathBuf>, Option<(u32, u32)>) {
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return (None, None);
    }
    let src = src.strip_prefix("file://").unwrap_or(src);
    let src = src.split(['?', '#']).next().unwrap_or(src);
    let path = Path::new(src);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.unwrap_or_else(|| Path::new(".")).join(path)
    };
    let dims = image::image_dimensions(&path).ok();
    (Some(path), dims)
}

/// Deduplicate links by URL, preferring entries with descriptive display text.
pub fn deduplicate_links(links: Vec<LinkInfo>) -> Vec<LinkInfo> {
    use std::collections::HashMap;
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut result: Vec<LinkInfo> = Vec::new();

    for link in links {
        if let Some(&idx) = seen.get(&link.url) {
            if result[idx].display_text == result[idx].url && link.display_text != link.url {
                result[idx] = link;
            }
        } else {
            seen.insert(link.url.clone(), result.len());
            result.push(link);
        }
    }

    for link in &mut result {
        if link.display_text == link.url {
            link.display_text = humanize_url(&link.url);
        }
    }

    result
}

pub(super) struct Renderer {
    pub(super) blocks: Vec<RenderedBlock>,
    pub(super) current_spans: Vec<Span<'static>>,
    pub(super) style_stack: Vec<Style>,
    /// Stack of list contexts: None = unordered, Some(n) = ordered starting at n.
    pub(super) list_stack: Vec<Option<u64>>,
    /// Whether we're inside a code block.
    pub(super) in_code_block: bool,
    /// Language hint for the current code block.
    pub(super) code_block_lang: Option<String>,
    /// Accumulated code block content.
    pub(super) code_block_buf: String,
    /// Current nesting depth for blockquotes.
    pub(super) blockquote_depth: usize,
    /// Whether the next text event is the first in a list item (needs marker).
    pub(super) pending_list_marker: bool,
    /// Track if we need a blank line separator.
    pub(super) needs_newline: bool,
    /// Current link destination (Some while inside a link).
    pub(super) link_dest: Option<String>,
    /// Accumulated display text inside the current link (for comparison with URL).
    pub(super) link_text: String,
    /// Collected link metadata for the document.
    pub(super) link_infos: Vec<LinkInfo>,
    /// Whether we're inside a heading (to apply heading style to all text).
    /// Width of the current list marker (indent + bullet/number) for hanging indent.
    pub(super) list_marker_width: usize,
    /// How many leading spans of the current line are the list marker (0: none on this line).
    pub(super) marker_spans: usize,
    /// Whether the current list item is a quine link line, so the hard-broken lines under it
    /// are what was said on the edge: hung and italic.
    pub(super) in_link_item: bool,
    /// Source byte offset for each block (populated by `run_with_offsets`).
    pub(super) block_source_offsets: Vec<usize>,
    /// Byte offset of the most recently seen event (for final flush).
    pub(super) last_event_offset: usize,
    pub(super) in_heading: bool,
    pub(super) heading_level: Option<u8>,
    /// Whether we're inside a table.
    pub(super) in_table: bool,
    /// Column alignments for the current table.
    pub(super) table_alignments: Vec<pulldown_cmark::Alignment>,
    /// Buffered table rows. Each row is a vec of cells, each cell is a vec of spans.
    pub(super) table_rows: Vec<Vec<Vec<Span<'static>>>>,
    /// Spans for the current cell being built.
    pub(super) table_cell_spans: Vec<Span<'static>>,
    /// Whether we're in the header row.
    pub(super) in_table_header: bool,
    /// Where an image's relative `src` resolves from (the document's directory).
    pub(super) base_dir: Option<PathBuf>,
    /// The image being read — its `src` and the alt text gathered so far.
    pub(super) image: Option<(String, String)>,
}

impl Renderer {
    pub(super) fn new() -> Self {
        Self {
            blocks: Vec::new(),
            current_spans: Vec::new(),
            style_stack: Vec::new(),
            list_stack: Vec::new(),
            in_code_block: false,
            code_block_lang: None,
            code_block_buf: String::new(),
            blockquote_depth: 0,
            pending_list_marker: false,
            needs_newline: false,
            link_dest: None,
            link_text: String::new(),
            link_infos: Vec::new(),
            in_heading: false,
            heading_level: None,
            in_table: false,
            table_alignments: Vec::new(),
            table_rows: Vec::new(),
            table_cell_spans: Vec::new(),
            in_table_header: false,
            list_marker_width: 0,
            marker_spans: 0,
            in_link_item: false,
            block_source_offsets: Vec::new(),
            last_event_offset: 0,
            base_dir: None,
            image: None,
        }
    }

    pub(super) fn with_base_dir(mut self, base_dir: Option<&Path>) -> Self {
        self.base_dir = base_dir.map(Path::to_path_buf);
        self
    }

    pub(super) fn run<'a>(&mut self, parser: impl Iterator<Item = Event<'a>>) {
        for event in parser {
            self.handle_event(event);
        }
        self.flush_line();
    }

    /// Run with byte offset tracking — records source offset for each block.
    pub(super) fn run_with_offsets<'a>(
        &mut self,
        parser: impl Iterator<Item = (Event<'a>, std::ops::Range<usize>)>,
    ) {
        for (event, range) in parser {
            self.last_event_offset = range.start;
            self.handle_event(event);
            // Tag any newly-pushed blocks with this event's source offset.
            while self.block_source_offsets.len() < self.blocks.len() {
                self.block_source_offsets.push(range.start);
            }
        }
        self.flush_line();
        while self.block_source_offsets.len() < self.blocks.len() {
            self.block_source_offsets.push(self.last_event_offset);
        }
    }

    pub(super) fn into_blocks(self) -> (Vec<RenderedBlock>, Vec<LinkInfo>) {
        (self.blocks, self.link_infos)
    }

    pub(super) fn into_blocks_with_offsets(
        self,
    ) -> (Vec<RenderedBlock>, Vec<LinkInfo>, Vec<usize>) {
        (self.blocks, self.link_infos, self.block_source_offsets)
    }

    // ── Event dispatch ──────────────────────────────────────────────────

    fn handle_event<'a>(&mut self, event: Event<'a>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.on_text(&text),
            Event::Code(code) => self.on_inline_code(&code),
            Event::SoftBreak => self.on_soft_break(),
            Event::HardBreak => self.on_hard_break(),
            Event::Rule => self.on_rule(),
            Event::TaskListMarker(checked) => self.on_task_list_marker(checked),
            Event::Html(html) => self.on_html(&html),
            Event::InlineHtml(html) => self.on_inline_html(&html),
            Event::FootnoteReference(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {}
        }
    }

    // ── Start tags ──────────────────────────────────────────────────────

    fn start_tag<'a>(&mut self, tag: Tag<'a>) {
        match tag {
            Tag::Heading { level, .. } => {
                if self.needs_newline {
                    self.push_blank_line();
                }
                let style = match level {
                    HeadingLevel::H1 => H1_STYLE,
                    HeadingLevel::H2 => H2_STYLE,
                    HeadingLevel::H3 => H3_STYLE,
                    HeadingLevel::H4 | HeadingLevel::H5 | HeadingLevel::H6 => H4_STYLE,
                };
                self.style_stack.push(style);
                self.in_heading = true;
                self.heading_level = Some(match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                });
            }
            Tag::Paragraph => {
                if self.needs_newline && !self.is_in_list_item() {
                    self.push_blank_line();
                }
            }
            Tag::Strong => {
                self.push_merged_style(BOLD_STYLE);
            }
            Tag::Emphasis => {
                self.push_merged_style(ITALIC_STYLE);
            }
            Tag::Strikethrough => {
                self.push_merged_style(STRIKETHROUGH_STYLE);
            }
            Tag::BlockQuote(_) => {
                if self.needs_newline {
                    self.push_blank_line();
                }
                self.blockquote_depth += 1;
                self.style_stack.push(Style::new().fg(Color::Gray));
            }
            Tag::CodeBlock(kind) => {
                if self.needs_newline {
                    self.push_blank_line();
                }
                self.in_code_block = true;
                self.code_block_buf.clear();
                self.code_block_lang = match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let lang = lang.split_whitespace().next().unwrap_or("").to_string();
                        if lang.is_empty() {
                            None
                        } else {
                            Some(lang)
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
            }
            Tag::List(start) => {
                if self.list_stack.is_empty() && self.needs_newline {
                    self.push_blank_line();
                }
                if !self.list_stack.is_empty() {
                    self.flush_line();
                }
                self.list_stack.push(start);
            }
            Tag::Item => {
                self.pending_list_marker = true;
            }
            Tag::Link { dest_url, .. } => {
                self.link_dest = Some(dest_url.to_string());
                self.link_text.clear();
                self.push_merged_style(LINK_STYLE);
            }
            Tag::Table(alignments) => {
                if self.needs_newline {
                    self.push_blank_line();
                }
                self.in_table = true;
                self.table_alignments.clone_from(&alignments);
                self.table_rows.clear();
            }
            Tag::TableHead => {
                self.in_table_header = true;
            }
            Tag::TableRow => {
                // Start a new row — nothing special needed, cells accumulate
            }
            Tag::TableCell => {
                self.table_cell_spans.clear();
            }
            Tag::Image { dest_url, .. } => {
                if !self.in_table {
                    self.flush_line();
                }
                self.image = Some((dest_url.to_string(), String::new()));
            }
            Tag::FootnoteDefinition(_)
            | Tag::HtmlBlock
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript => {}
        }
    }

    // ── End tags ────────────────────────────────────────────────────────

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.in_heading = false;
                self.style_stack.pop();
                self.flush_line();
                self.heading_level = None;
                self.needs_newline = true;
            }
            TagEnd::Paragraph => {
                self.flush_line();
                self.needs_newline = true;
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => {
                self.style_stack.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.style_stack.pop();
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.needs_newline = true;
            }
            TagEnd::CodeBlock => {
                self.render_code_block();
                self.in_code_block = false;
                self.code_block_lang = None;
                self.needs_newline = true;
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.needs_newline = true;
                }
            }
            TagEnd::Item => {
                self.flush_line();
                self.pending_list_marker = false;
                self.list_marker_width = 0;
                self.marker_spans = 0;
                self.in_link_item = false;
            }
            TagEnd::Link => {
                self.style_stack.pop();
                let text = std::mem::take(&mut self.link_text);
                if let Some(url) = self.link_dest.take() {
                    if !url.is_empty() {
                        self.link_infos.push(LinkInfo {
                            display_text: if text.is_empty() { url.clone() } else { text },
                            url,
                        });
                    }
                }
            }
            TagEnd::TableCell => {
                let mut spans = std::mem::take(&mut self.table_cell_spans);
                if self.in_table_header {
                    spans = spans
                        .into_iter()
                        .map(|s| Span::styled(s.content, s.style.patch(TABLE_HEADER_STYLE)))
                        .collect();
                }
                if let Some(last_row) = self.table_rows.last_mut() {
                    last_row.push(spans);
                } else {
                    self.table_rows.push(vec![spans]);
                }
            }
            TagEnd::TableHead => {
                self.in_table_header = false;
                // Push a new empty row for the next body row
                self.table_rows.push(Vec::new());
            }
            TagEnd::TableRow => {
                self.table_rows.push(Vec::new());
            }
            TagEnd::Table => {
                self.render_table();
                self.in_table = false;
                self.table_alignments.clear();
                self.table_rows.clear();
                self.needs_newline = true;
            }
            TagEnd::Image => self.end_image(),
            TagEnd::FootnoteDefinition
            | TagEnd::HtmlBlock
            | TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript => {}
        }
    }

    // ── Inline events ───────────────────────────────────────────────────

    fn on_text(&mut self, text: &str) {
        if self.in_code_block {
            self.code_block_buf.push_str(text);
            return;
        }
        if let Some((_, alt)) = self.image.as_mut() {
            alt.push_str(text);
            if self.link_dest.is_some() {
                self.link_text.push_str(text);
            }
            return;
        }

        // Accumulate link display text regardless of table context.
        if self.link_dest.is_some() {
            self.link_text.push_str(text);
        }

        if self.in_table {
            let style = self.current_style();
            self.table_cell_spans.push(Span::styled(text.to_string(), style));
            return;
        }

        let style = self.current_style();

        // If there's a pending list marker, emit it first.
        if self.pending_list_marker {
            self.emit_list_marker();
            self.pending_list_marker = false;
        }

        // Handle multi-line text (e.g., from HTML blocks).
        for (i, line) in text.lines().enumerate() {
            if i > 0 {
                self.flush_line();
            }
            if !line.is_empty() {
                self.current_spans.push(Span::styled(line.to_string(), style));
            }
        }
    }

    fn on_inline_code(&mut self, code: &str) {
        if let Some((_, alt)) = self.image.as_mut() {
            alt.push_str(code);
            return;
        }
        if self.link_dest.is_some() {
            self.link_text.push_str(code);
        }
        if self.pending_list_marker {
            self.emit_list_marker();
            self.pending_list_marker = false;
        }
        if self.in_table {
            self.table_cell_spans.push(Span::styled(format!(" {} ", code), INLINE_CODE_STYLE));
            return;
        }
        // Render inline code with distinct style, padded with spaces.
        let span = Span::styled(format!(" {} ", code), INLINE_CODE_STYLE);
        self.current_spans.push(span);
    }

    fn on_soft_break(&mut self) {
        if self.in_code_block {
            self.code_block_buf.push('\n');
            return;
        }
        if let Some((_, alt)) = self.image.as_mut() {
            alt.push(' ');
            return;
        }
        if self.in_table {
            self.table_cell_spans.push(Span::styled(" ", self.current_style()));
            return;
        }
        // Treat soft break as a space in inline context.
        let style = self.current_style();
        self.current_spans.push(Span::styled(" ", style));
    }

    fn on_hard_break(&mut self) {
        self.flush_line();
    }

    /// Close `![alt](src)`: in a table the alt text stands in; elsewhere the image becomes
    /// a block of its own, resolved against `base_dir` and measured if it can be read.
    fn end_image(&mut self) {
        let Some((src, alt)) = self.image.take() else { return };
        if self.in_table {
            let style = self.current_style();
            let label = if alt.is_empty() { src } else { alt };
            self.table_cell_spans.push(Span::styled(label, style));
            return;
        }
        self.flush_line();
        let (path, dims) = resolve_image(&src, self.base_dir.as_deref());
        self.blocks.push(RenderedBlock::Image { src, alt, path, dims });
        self.needs_newline = true;
    }

    fn on_rule(&mut self) {
        self.flush_line();
        if !self.blocks.is_empty() {
            self.push_blank_line();
        }
        self.blocks.push(RenderedBlock::HorizontalRule { blockquote_depth: self.blockquote_depth });
        self.needs_newline = true;
    }

    fn on_task_list_marker(&mut self, checked: bool) {
        // Replace the pending bullet/number marker with a checkbox.
        let checkbox = if checked { "☑ " } else { "☐ " };
        let indent = self.list_indent_prefix();
        let style = self.current_style();
        self.current_spans.push(Span::styled(indent, Style::default()));
        self.current_spans.push(Span::styled(checkbox, style));
        // Mark that we've already emitted the marker.
        self.pending_list_marker = false;
    }

    fn on_html(&mut self, html: &str) {
        // Render HTML blocks as plain text.
        for (i, line) in html.lines().enumerate() {
            if i > 0 {
                self.flush_line();
            }
            self.current_spans.push(Span::styled(line.to_string(), Style::default()));
        }
        self.flush_line();
    }

    fn on_inline_html(&mut self, html: &str) {
        // Strip inline HTML tags, render content if any.
        if html.is_empty() {
            return;
        }
        let content = html.to_string();
        self.current_spans.push(Span::styled(content, self.current_style()));
    }

    // ── Code block rendering ────────────────────────────────────────────

    fn render_code_block(&mut self) {
        let code = std::mem::take(&mut self.code_block_buf);
        let lang = self.code_block_lang.clone().unwrap_or_default();

        // Highlight (width-independent) — the expensive part done once.
        let highlighted_lines = highlight_code(&code, &lang);

        self.blocks.push(RenderedBlock::CodeBlock {
            lang,
            highlighted_lines,
            blockquote_depth: self.blockquote_depth,
        });
    }

    // ── Table rendering ─────────────────────────────────────────────────

    fn render_table(&mut self) {
        // Drain rows to avoid cloning; filter out empty trailing rows.
        let rows: Vec<Vec<Vec<Span<'static>>>> =
            self.table_rows.drain(..).filter(|r| !r.is_empty()).collect();

        if rows.is_empty() {
            return;
        }

        let num_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        if num_cols == 0 {
            return;
        }

        self.blocks.push(RenderedBlock::Table {
            rows,
            alignments: std::mem::take(&mut self.table_alignments),
            blockquote_depth: self.blockquote_depth,
        });
    }

    // ── List helpers ────────────────────────────────────────────────────

    fn emit_list_marker(&mut self) {
        let depth = self.list_stack.len();
        let indent = self.list_indent_prefix();

        if let Some(list_type) = self.list_stack.last_mut() {
            match list_type {
                None => {
                    // Unordered list — use bullet character.
                    let bullet = if depth <= 1 {
                        "•"
                    } else if depth == 2 {
                        "◦"
                    } else {
                        "▪"
                    };
                    self.current_spans.push(Span::styled(indent, Style::default()));
                    self.current_spans
                        .push(Span::styled(format!("{bullet} "), Style::new().fg(Color::Gray)));
                }
                Some(ref mut num) => {
                    // Ordered list — use number.
                    self.current_spans.push(Span::styled(indent, Style::default()));
                    self.current_spans
                        .push(Span::styled(format!("{num}. "), Style::new().fg(Color::Gray)));
                    *num += 1;
                }
            }
        }
        // Store the marker width for hanging indent on wrapped continuation lines.
        self.list_marker_width =
            self.current_spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum();
        self.marker_spans = self.current_spans.len();
    }

    /// If the spans after the list marker read as a quine link line, repaint them the way
    /// quine's triage does: marker cyan, `—rel→` bold yellow, the name bold, `[` `]` plain,
    /// the id cyan with no code padding, the kind dim, `[dangling]` red — two spaces between
    /// the parts. `None` for any other item.
    fn restyle_link_line(
        spans: &[Span<'static>],
        marker_spans: usize,
    ) -> Option<Vec<Span<'static>>> {
        // Rebuild the item's text; an inline code span goes back into backticks.
        let text: String = spans[marker_spans..]
            .iter()
            .map(|s| {
                if s.style == INLINE_CODE_STYLE {
                    format!("`{}`", s.content.trim())
                } else {
                    s.content.to_string()
                }
            })
            .collect();
        let link = parse_link_line(&text)?;

        let mut out: Vec<Span<'static>> = spans[..marker_spans]
            .iter()
            .map(|s| Span::styled(s.content.clone(), s.style.patch(LINK_LINE_MARKER_STYLE)))
            .collect();
        // The marker ends in one space; the triage sets two between parts.
        out.push(Span::raw(" "));
        out.push(Span::styled(link.edge, LINK_LINE_EDGE_STYLE));
        out.push(Span::raw("  "));
        out.push(Span::styled(link.name, LINK_LINE_NAME_STYLE));
        match link.tail {
            Tail::Dangling => {
                out.push(Span::raw("  "));
                out.push(Span::styled("[dangling]", LINK_LINE_DANGLING_STYLE));
            }
            Tail::Node { id, kind } => {
                out.push(Span::raw("  ["));
                out.push(Span::styled(id, LINK_LINE_ID_STYLE));
                if let Some(kind) = kind {
                    out.push(Span::raw(" "));
                    out.push(Span::styled(kind, LINK_LINE_KIND_STYLE));
                }
                out.push(Span::raw("]"));
            }
        }
        Some(out)
    }

    fn list_indent_prefix(&self) -> String {
        let depth = self.list_stack.len();
        if depth <= 1 {
            String::new()
        } else {
            "  ".repeat(depth - 1)
        }
    }

    fn is_in_list_item(&self) -> bool {
        !self.list_stack.is_empty()
    }

    // ── Style helpers ───────────────────────────────────────────────────

    fn current_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or_default()
    }

    fn push_merged_style(&mut self, new_style: Style) {
        let current = self.current_style();
        self.style_stack.push(current.patch(new_style));
    }

    // ── Block management ────────────────────────────────────────────────

    fn flush_line(&mut self) {
        if self.current_spans.is_empty() {
            return;
        }
        let mut spans = std::mem::take(&mut self.current_spans);
        let mut list_marker_width = self.list_marker_width;

        let marker_spans = std::mem::take(&mut self.marker_spans);
        if marker_spans > 0 {
            // The first line of a list item: a quine link line gets the triage's paint.
            self.in_link_item = false;
            if let Some(painted) = Self::restyle_link_line(&spans, marker_spans) {
                // Wrapped head lines hang under the edge, after the widened marker.
                list_marker_width = painted[..=marker_spans]
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum();
                self.list_marker_width = list_marker_width;
                spans = painted;
                self.in_link_item = true;
            }
        } else if self.in_link_item {
            // What was said on the edge: hung six columns in from the item, italic.
            let hang = self.list_indent_prefix().len() + LINK_LINE_HANG_COLS;
            let mut indented = vec![Span::raw(" ".repeat(hang))];
            indented.extend(
                spans
                    .into_iter()
                    .map(|s| Span::styled(s.content, s.style.patch(LINK_LINE_PROSE_STYLE))),
            );
            spans = indented;
            list_marker_width = hang;
        }

        self.blocks.push(RenderedBlock::StyledLine {
            spans,
            blockquote_depth: self.blockquote_depth,
            list_marker_width,
            heading_level: self.heading_level,
        });
    }

    fn push_blank_line(&mut self) {
        self.flush_line();
        self.blocks.push(RenderedBlock::BlankLine { blockquote_depth: self.blockquote_depth });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── humanize_url ───────────────────────────────────────────────

    #[test]
    fn humanize_url_strips_https() {
        assert_eq!(humanize_url("https://example.com"), "example.com");
    }

    #[test]
    fn humanize_url_strips_http() {
        assert_eq!(humanize_url("http://example.com"), "example.com");
    }

    #[test]
    fn humanize_url_strips_www() {
        assert_eq!(humanize_url("https://www.example.com"), "example.com");
    }

    #[test]
    fn humanize_url_strips_trailing_slash() {
        assert_eq!(humanize_url("https://example.com/"), "example.com");
    }

    #[test]
    fn humanize_url_preserves_path() {
        assert_eq!(humanize_url("https://example.com/path/page"), "example.com/path/page");
    }

    #[test]
    fn humanize_url_no_scheme() {
        assert_eq!(humanize_url("example.com"), "example.com");
    }

    #[test]
    fn humanize_url_all_strippable() {
        assert_eq!(humanize_url("https://www.example.com/"), "example.com");
    }

    // ── deduplicate_links ──────────────────────────────────────────

    #[test]
    fn deduplicate_links_removes_duplicate_urls() {
        let links = vec![
            LinkInfo {
                display_text: "https://example.com".to_string(),
                url: "https://example.com".to_string(),
            },
            LinkInfo {
                display_text: "https://example.com".to_string(),
                url: "https://example.com".to_string(),
            },
        ];
        let result = deduplicate_links(links);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn deduplicate_links_prefers_descriptive_text() {
        let links = vec![
            LinkInfo {
                display_text: "https://example.com".to_string(),
                url: "https://example.com".to_string(),
            },
            LinkInfo {
                display_text: "Example Site".to_string(),
                url: "https://example.com".to_string(),
            },
        ];
        let result = deduplicate_links(links);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].display_text, "Example Site");
    }

    #[test]
    fn deduplicate_links_humanizes_url_only_display() {
        let links = vec![LinkInfo {
            display_text: "https://www.example.com/".to_string(),
            url: "https://www.example.com/".to_string(),
        }];
        let result = deduplicate_links(links);
        assert_eq!(result[0].display_text, "example.com");
    }

    #[test]
    fn deduplicate_links_keeps_different_urls() {
        let links = vec![
            LinkInfo { display_text: "A".to_string(), url: "https://a.com".to_string() },
            LinkInfo { display_text: "B".to_string(), url: "https://b.com".to_string() },
        ];
        let result = deduplicate_links(links);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn deduplicate_links_empty_input() {
        let result = deduplicate_links(Vec::new());
        assert!(result.is_empty());
    }

    // ── Renderer integration ──────────────────────────────────────

    #[test]
    fn renderer_heading_produces_styled_line() {
        let md = "# Hello";
        let parser = pulldown_cmark::Parser::new(md);
        let mut r = Renderer::new();
        r.run(parser);
        let (blocks, _) = r.into_blocks();

        assert!(!blocks.is_empty());
        match &blocks[0] {
            RenderedBlock::StyledLine { heading_level, .. } => {
                assert_eq!(*heading_level, Some(1));
            }
            _ => panic!("Expected StyledLine for heading"),
        }
    }

    #[test]
    fn renderer_collects_links() {
        let md = "[Rust](https://rust-lang.org)";
        let parser = pulldown_cmark::Parser::new(md);
        let mut r = Renderer::new();
        r.run(parser);
        let (_, links) = r.into_blocks();

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].display_text, "Rust");
        assert_eq!(links[0].url, "https://rust-lang.org");
    }

    #[test]
    fn renderer_code_block() {
        let md = "```rust\nlet x = 1;\n```";
        let parser = pulldown_cmark::Parser::new(md);
        let mut r = Renderer::new();
        r.run(parser);
        let (blocks, _) = r.into_blocks();

        let has_code = blocks
            .iter()
            .any(|b| matches!(b, RenderedBlock::CodeBlock { lang, .. } if lang == "rust"));
        assert!(has_code);
    }

    #[test]
    fn renderer_horizontal_rule() {
        let md = "---";
        let parser = pulldown_cmark::Parser::new(md);
        let mut r = Renderer::new();
        r.run(parser);
        let (blocks, _) = r.into_blocks();

        let has_hr = blocks.iter().any(|b| matches!(b, RenderedBlock::HorizontalRule { .. }));
        assert!(has_hr);
    }

    #[test]
    fn renderer_blockquote_depth() {
        let md = "> quoted text";
        let parser = pulldown_cmark::Parser::new(md);
        let mut r = Renderer::new();
        r.run(parser);
        let (blocks, _) = r.into_blocks();

        let has_quote = blocks.iter().any(|b| match b {
            RenderedBlock::StyledLine { blockquote_depth, .. } => *blockquote_depth > 0,
            _ => false,
        });
        assert!(has_quote);
    }

    #[test]
    fn renderer_unordered_list() {
        let md = "- item one\n- item two";
        let parser = pulldown_cmark::Parser::new(md);
        let mut r = Renderer::new();
        r.run(parser);
        let (blocks, _) = r.into_blocks();

        // Should produce at least 2 styled lines for list items
        let styled_count =
            blocks.iter().filter(|b| matches!(b, RenderedBlock::StyledLine { .. })).count();
        assert!(styled_count >= 2);
    }

    #[test]
    fn renderer_ordered_list() {
        let md = "1. first\n2. second";
        let parser = pulldown_cmark::Parser::new(md);
        let mut r = Renderer::new();
        r.run(parser);
        let (blocks, _) = r.into_blocks();

        let styled_count =
            blocks.iter().filter(|b| matches!(b, RenderedBlock::StyledLine { .. })).count();
        assert!(styled_count >= 2);
    }

    #[test]
    fn renderer_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let opts = pulldown_cmark::Options::ENABLE_TABLES;
        let parser = pulldown_cmark::Parser::new_ext(md, opts);
        let mut r = Renderer::new();
        r.run(parser);
        let (blocks, _) = r.into_blocks();

        let has_table = blocks.iter().any(|b| matches!(b, RenderedBlock::Table { .. }));
        assert!(has_table);
    }

    #[test]
    fn renderer_task_list() {
        let md = "- [x] done\n- [ ] todo";
        let opts = pulldown_cmark::Options::ENABLE_TASKLISTS;
        let parser = pulldown_cmark::Parser::new_ext(md, opts);
        let mut r = Renderer::new();
        r.run(parser);
        let (blocks, _) = r.into_blocks();

        // Task list items produce styled lines with checkbox chars
        let text: String = blocks
            .iter()
            .filter_map(|b| match b {
                RenderedBlock::StyledLine { spans, .. } => {
                    Some(spans.iter().map(|s| s.content.to_string()).collect::<String>())
                }
                _ => None,
            })
            .collect();
        assert!(text.contains('☑') || text.contains('☐'));
    }
}
