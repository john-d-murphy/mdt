//! Help overlay drawing.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::{modal, theme};

pub(crate) const HELP_KEYS: &[(&str, &str)] = &[
    ("j/k", "Navigate / Scroll"),
    ("Enter", "Open file / Enter directory"),
    ("Tab", "Switch focus"),
    ("Ctrl+e", "Toggle file tree"),
    ("i/e", "Edit mode"),
    ("/", "Search"),
    ("ff", "Find file"),
    ("n/N", "Next/Previous match"),
    ("gg/G", "Top/Bottom (also Home/End, </>)"),
    ("Ctrl+d/u", "Half page down/up"),
    ("Spc/PgDn", "Page down"),
    ("b/PgUp", "Page up"),
    ("[/]", "Previous/Next heading"),
    (":w", "Save"),
    (":q", "Quit"),
    ("o", "Open links"),
    ("a", "New file"),
    ("A", "New directory"),
    ("d", "Delete"),
    ("r", "Rename"),
    ("m", "Move"),
    ("Ctrl+p", "Toggle live preview"),
    ("Ctrl+s", "Swap preview split"),
    ("?", "This help"),
];

pub(super) fn draw_help_overlay(frame: &mut Frame, area: Rect, bg_color: Color) {
    // Rows of modal chrome (borders, title bar, shortcuts bar) around the key list.
    const FRAME_ROWS: u16 = 6;
    let height = u16::try_from(HELP_KEYS.len()).unwrap_or(u16::MAX).saturating_add(FRAME_ROWS);
    let popup_area = modal::centered_rect(50, height, area);
    let content_area = modal::render_modal_frame(
        frame,
        popup_area,
        "Help",
        None,
        &[("close", "esc")],
        bg_color,
        false,
    );

    let help_lines: Vec<Line> = HELP_KEYS
        .iter()
        .map(|&(key, desc)| {
            Line::from(vec![
                Span::styled(format!("{key:>12}"), theme::HELP_KEY_STYLE),
                Span::styled("  ", Style::default()),
                Span::styled(desc, theme::HELP_DESC_STYLE),
            ])
        })
        .collect();

    let help_content = Paragraph::new(help_lines);
    frame.render_widget(help_content, content_area);
}
