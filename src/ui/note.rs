//! The note overlay: a box to say something about what is being read.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::modal;
use crate::app::App;
use crate::palette;

/// Draw the note box over `area`, with whatever is selected shown beneath as context.
pub(super) fn draw_note_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let popup = modal::centered_rect(area.width.saturating_sub(10).min(76), 11, area);
    let about = app
        .source_ref()
        .or_else(|| {
            app.document
                .current_file
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "this page".to_string());

    let content = modal::render_modal_frame(
        frame,
        popup,
        &format!("Note on {about}"),
        Some((&app.note_input, "what you want to say about it")),
        &[("make the note", "enter")],
        app.bg_color,
        app.cursor.visible,
    );

    let quoted = app.document.selected_text(&app.selection);
    let style = Style::new().fg(palette::FG_MUTED).add_modifier(Modifier::ITALIC);
    let lines: Vec<Line> = match &quoted {
        Some(text) => std::iter::once(Line::from(Span::styled("selected:", style)))
            .chain(text.lines().map(|l| Line::from(Span::styled(l.to_string(), style))))
            .collect(),
        None => vec![Line::from(Span::styled(
            "Nothing selected — drag over the text to quote some of it.",
            style,
        ))],
    };
    frame.render_widget(Paragraph::new(lines), content);
}
