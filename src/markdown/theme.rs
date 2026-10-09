//! Style constants for markdown rendering.

use ratatui::style::{Color, Modifier, Style};

use crate::palette;

pub(super) const H1_STYLE: Style =
    Style::new().add_modifier(Modifier::BOLD).fg(palette::ACCENT_CYAN);
pub(super) const H2_STYLE: Style =
    Style::new().add_modifier(Modifier::BOLD).fg(palette::ACCENT_GREEN);
pub(super) const H3_STYLE: Style =
    Style::new().add_modifier(Modifier::BOLD).fg(palette::ACCENT_YELLOW);
pub(super) const H4_STYLE: Style = Style::new().add_modifier(Modifier::BOLD).fg(palette::FG_MUTED);
pub(super) const BOLD_STYLE: Style =
    Style::new().add_modifier(Modifier::BOLD).fg(palette::ACCENT_RED);
pub(super) const ITALIC_STYLE: Style = Style::new().add_modifier(Modifier::ITALIC);
pub(super) const STRIKETHROUGH_STYLE: Style = Style::new().add_modifier(Modifier::CROSSED_OUT);
pub(super) const INLINE_CODE_STYLE: Style = Style::new().fg(palette::ACCENT_LIGHT_CYAN);
pub(super) const LINK_STYLE: Style =
    Style::new().add_modifier(Modifier::UNDERLINED).fg(palette::ACCENT_BLUE);
pub(super) const BLOCKQUOTE_STYLE: Style = Style::new().fg(palette::ACCENT_CYAN);
pub(super) const CODE_BORDER_STYLE: Style = Style::new().fg(palette::BORDER);
pub(super) const CODE_DEFAULT_STYLE: Style = Style::new();
pub(super) const HR_STYLE: Style = Style::new().fg(palette::BORDER);
pub(super) const BLOCKQUOTE_INDENT_COLS: usize = 2;
pub(super) const TABLE_HEADER_STYLE: Style =
    Style::new().add_modifier(Modifier::BOLD).fg(palette::ACCENT_CYAN);
pub(super) const TABLE_BORDER_STYLE: Style = Style::new().fg(palette::BORDER);

// Link lines (quine's `## Links out` / `## Links in` items). The colours are the roles in
// homunculus `env/palette.tsv`: id 36, edge 1;33, title 1, prose 3, bad 31; the kind is dim
// because mdt has no hub seed to know its kind group's colour.
pub(super) const LINK_LINE_MARKER_STYLE: Style = Style::new().fg(palette::ACCENT_CYAN);
pub(super) const LINK_LINE_EDGE_STYLE: Style =
    Style::new().add_modifier(Modifier::BOLD).fg(palette::ACCENT_YELLOW);
pub(super) const LINK_LINE_NAME_STYLE: Style = Style::new().add_modifier(Modifier::BOLD);
pub(super) const LINK_LINE_ID_STYLE: Style = Style::new().fg(palette::ACCENT_CYAN);
pub(super) const LINK_LINE_KIND_STYLE: Style = Style::new().add_modifier(Modifier::DIM);
pub(super) const LINK_LINE_DANGLING_STYLE: Style = Style::new().fg(Color::Red);
pub(super) const LINK_LINE_PROSE_STYLE: Style = Style::new().add_modifier(Modifier::ITALIC);
/// Columns the lines under a link line (what was said on the edge) hang at.
pub(super) const LINK_LINE_HANG_COLS: usize = 6;
