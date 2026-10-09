//! Custom markdown-to-ratatui renderer using pulldown-cmark.
//!
//! Produces styled [`Text`] with all syntax markers stripped — headings, bold, italic,
//! strikethrough, inline code, code blocks, lists, blockquotes, links, horizontal rules,
//! and task lists are all rendered as properly styled text.
//!
//! The rendering pipeline is split into two phases:
//! 1. **`render_markdown_blocks`** — parses markdown + syntax highlights code blocks → cached blocks
//! 2. **`rewrap_blocks`** — re-wraps cached blocks to a given width → `Vec<Line<'static>>`

use std::path::Path;

use pulldown_cmark::{Options, Parser};
use ratatui::text::Span;

pub(crate) mod blocks;
mod link_line;
pub(crate) mod syntax;
mod wrap;
use syntax::no_color;
mod renderer;
mod theme;
use renderer::Renderer;
use theme::*;

pub(crate) use blocks::{rewrap_blocks, ImageLayout, RenderedBlock};
pub(crate) use renderer::{deduplicate_links, humanize_url, LinkInfo};

#[cfg(test)]
mod test_helpers;
#[cfg(test)]
mod tests;

/// Render markdown input into styled ratatui [`Text`].
///
/// - Pre-expands tabs to 4 spaces (ratatui `Paragraph` silently drops tabs).
/// - Respects the `NO_COLOR` environment variable: when set, returns plain unstyled text.
/// - All markdown syntax markers are stripped; styling is applied via ratatui modifiers/colors.
#[cfg(test)]
pub fn render_markdown(
    input: &str,
    available_width: Option<usize>,
) -> ratatui::text::Text<'static> {
    use ratatui::text::Text;
    let cleaned = input.replace('\t', "    ");

    if no_color() {
        return Text::raw(cleaned);
    }

    let (blocks, _links) = render_markdown_blocks(input, None);
    let (lines, _block_starts) = rewrap_blocks(&blocks, available_width, ImageLayout::DISABLED);
    Text::from(lines)
}

/// Render markdown to width-independent intermediate blocks.
///
/// This is the expensive "phase 1" of the split pipeline — parses markdown and
/// syntax-highlights all code blocks. The result can be cached and cheaply re-wrapped
/// to different widths via [`rewrap_blocks`].
/// `base_dir` is where an image's relative `src` resolves from — the document's directory.
pub(crate) fn render_markdown_blocks(
    input: &str,
    base_dir: Option<&Path>,
) -> (Vec<RenderedBlock>, Vec<LinkInfo>) {
    let cleaned = input.replace('\t', "    ");

    if no_color() {
        // Still extract links even when color is disabled.
        let options =
            Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
        let parser = Parser::new_ext(&cleaned, options);
        let mut renderer = Renderer::new().with_base_dir(base_dir);
        renderer.run(parser);
        let (_styled_blocks, links) = renderer.into_blocks();

        let plain_blocks = cleaned
            .lines()
            .map(|l| RenderedBlock::StyledLine {
                spans: vec![Span::raw(l.to_string())],
                blockquote_depth: 0,
                list_marker_width: 0,
                heading_level: None,
            })
            .collect();

        return (plain_blocks, links);
    }

    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(&cleaned, options);

    let mut renderer = Renderer::new().with_base_dir(base_dir);
    renderer.run(parser);
    renderer.into_blocks()
}

/// Like `render_markdown_blocks` but also returns per-block source line numbers.
///
/// The returned `Vec<usize>` has one entry per block: the source line (0-based)
/// where that block starts. Used by live preview for accurate scroll sync.
pub(crate) fn render_markdown_blocks_with_source_map(
    input: &str,
    base_dir: Option<&Path>,
) -> (Vec<RenderedBlock>, Vec<LinkInfo>, Vec<usize>) {
    let cleaned = input.replace('\t', "    ");

    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(&cleaned, options);

    let mut renderer = Renderer::new().with_base_dir(base_dir);
    renderer.run_with_offsets(parser.into_offset_iter());
    let (blocks, links, byte_offsets) = renderer.into_blocks_with_offsets();

    // Convert byte offsets to line numbers.
    let source_lines: Vec<usize> = byte_offsets
        .iter()
        .map(|&offset| {
            let clamped = offset.min(cleaned.len());
            cleaned[..clamped].matches('\n').count()
        })
        .collect();

    (blocks, links, source_lines)
}

/// The text of the link under display column `col` of a rendered line, if there is one: the
/// run of link-styled spans around that column, joined. Links are the only underlined text
/// the renderer produces (bold inside a link recolours it, so colour is not checked), which
/// identifies them after wrapping has re-split spans.
pub(crate) fn link_text_at(line: &ratatui::text::Line<'_>, col: usize) -> Option<String> {
    use ratatui::style::Modifier;
    use unicode_width::UnicodeWidthStr;

    let is_link = |s: &Span<'_>| s.style.add_modifier.contains(Modifier::UNDERLINED);

    let mut x = 0;
    let mut hit = None;
    for (i, span) in line.spans.iter().enumerate() {
        let w = span.content.width();
        if col >= x && col < x + w {
            hit = Some(i);
            break;
        }
        x += w;
    }
    let hit = hit?;
    if !is_link(&line.spans[hit]) {
        return None;
    }
    let start = (0..hit).rev().take_while(|&i| is_link(&line.spans[i])).last().unwrap_or(hit);
    let end =
        (hit..line.spans.len()).take_while(|&i| is_link(&line.spans[i])).last().unwrap_or(hit);
    Some(line.spans[start..=end].iter().map(|s| s.content.as_ref()).collect())
}
