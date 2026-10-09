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
    ("Ctrl+i", "Toggle images (:images)"),
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
    // Rows of modal chrome around the key list — a border, a padding row, the title and its
    // gap above; the gap, the shortcuts bar, a padding row and a border below (modal.rs's
    // zones) — and the columns of chrome either side (a border and two of padding each).
    // Six was the count before, which cut the last two keys off the card.
    const FRAME_ROWS: u16 = 8;
    const FRAME_COLS: usize = 6;
    const KEY_W: usize = 12;
    const GAP: usize = 4;
    let desc_w = HELP_KEYS.iter().map(|(_, d)| d.chars().count()).max().unwrap_or(0);
    let col_w = KEY_W + 2 + desc_w;
    // Two columns where the terminal is wide enough: the card stays short enough to fit a
    // terminal of thirty rows with every key on it. Else one column, as before.
    let two = 2 * col_w + GAP + FRAME_COLS <= usize::from(area.width);
    let half = (HELP_KEYS.len() + 1) / 2;
    let rows = if two { half } else { HELP_KEYS.len() };
    let width = if two { 2 * col_w + GAP + FRAME_COLS } else { col_w + FRAME_COLS };
    let height = u16::try_from(rows).unwrap_or(u16::MAX).saturating_add(FRAME_ROWS);
    let popup_area = modal::centered_rect(u16::try_from(width).unwrap_or(u16::MAX), height, area);
    let content_area = modal::render_modal_frame(
        frame,
        popup_area,
        "Help",
        None,
        &[("close", "esc")],
        bg_color,
        false,
    );

    let entry = |(key, desc): &(&str, &str)| {
        vec![
            Span::styled(format!("{key:>KEY_W$}"), theme::HELP_KEY_STYLE),
            Span::styled("  ", Style::default()),
            Span::styled(format!("{desc:<desc_w$}"), theme::HELP_DESC_STYLE),
        ]
    };
    let help_lines: Vec<Line> = (0..rows)
        .map(|i| {
            let mut spans = entry(&HELP_KEYS[i]);
            if let Some(right) = two.then(|| HELP_KEYS.get(i + rows)).flatten() {
                spans.push(Span::styled(" ".repeat(GAP), Style::default()));
                spans.extend(entry(right));
            }
            Line::from(spans)
        })
        .collect();

    let help_content = Paragraph::new(help_lines);
    frame.render_widget(help_content, content_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn rows_at(width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw_help_overlay(f, f.area(), Color::Reset)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
            .collect()
    }

    #[test]
    fn a_wide_terminal_gets_two_columns_and_every_key_fits_on_a_short_one() {
        let rows = rows_at(110, 24);
        let first = rows.iter().find(|r| r.contains("j/k")).expect("the first key is drawn");
        let (half_key, _) = HELP_KEYS[(HELP_KEYS.len() + 1) / 2];
        assert!(first.contains(half_key), "the right column starts on the first row: {first:?}");
        for (key, desc) in HELP_KEYS {
            assert!(
                rows.iter().any(|r| r.contains(key) && r.contains(desc)),
                "{key} is on the card"
            );
        }
        let half = (HELP_KEYS.len() + 1) / 2;
        let (_, bottom_left) = HELP_KEYS[half - 1];
        let last = rows.iter().rposition(|r| r.contains(bottom_left)).unwrap();
        let first_at = rows.iter().position(|r| r.contains("j/k")).unwrap();
        assert_eq!(last - first_at + 1, half, "half the entries a column");
    }

    #[test]
    fn a_narrow_terminal_keeps_one_column() {
        let rows = rows_at(60, 40);
        let first = rows.iter().position(|r| r.contains("j/k")).unwrap();
        let last = rows.iter().rposition(|r| r.contains("This help")).unwrap();
        assert_eq!(last - first + 1, HELP_KEYS.len());
    }
}
