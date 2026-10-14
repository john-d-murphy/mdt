//! The note overlay: a box to say something about what is being read.
//!
//! The comment wraps and takes as many lines as it needs — Enter starts another — so the
//! box is a place to write a few sentences, not a one-line prompt. `Ctrl+e` hands the draft
//! to `$EDITOR` for anything longer.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use super::{modal, theme};
use crate::app::App;
use crate::palette;

/// Rows of the selection to show as context beneath the comment.
const QUOTE_ROWS: usize = 4;

/// Draw the note box over `area`, with whatever is selected shown beneath as context.
pub(super) fn draw_note_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let popup = modal::centered_rect(
        area.width.saturating_sub(8).min(76),
        area.height.saturating_sub(4).min(18),
        area,
    );
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
        None,
        &[("make the note", "ctrl+d"), ("editor", "ctrl+e")],
        app.bg_color,
        false,
    );

    let quoted = app.document.selected_text(&app.selection);
    let quote_lines = quote(quoted.as_deref());
    let rows = u16::try_from(quote_lines.len()).unwrap_or(u16::MAX);
    let [writing, beneath] =
        Layout::vertical([Constraint::Min(2), Constraint::Length(rows)]).areas(content);

    frame.render_widget(Paragraph::new(comment(app)).wrap(Wrap { trim: false }), writing);
    frame.render_widget(Paragraph::new(quote_lines), beneath);
}

/// The comment as it is being typed, with a block cursor at the end — or the placeholder,
/// when nothing has been written yet.
fn comment(app: &App) -> Vec<Line<'static>> {
    let cursor =
        if app.cursor.visible { Span::styled(" ", theme::MODAL_CURSOR) } else { Span::raw(" ") };
    if app.note_input.is_empty() {
        let prompt = if app.document.selected_text(&app.selection).is_some() {
            "what you want to say, or nothing — the passage below is the note"
        } else {
            "what you want to say about it"
        };
        return vec![Line::from(vec![
            cursor,
            Span::styled(prompt, theme::MODAL_SEARCH_PLACEHOLDER),
        ])];
    }
    let mut lines: Vec<Line<'static>> = app
        .note_input
        .split('\n')
        .map(|line| Line::from(Span::styled(line.to_string(), theme::MODAL_SEARCH_TEXT)))
        .collect();
    if let Some(last) = lines.last_mut() {
        last.spans.push(cursor);
    }
    lines
}

/// The selection, dimmed, beneath the comment — at most [`QUOTE_ROWS`] lines of it.
fn quote(selected: Option<&str>) -> Vec<Line<'static>> {
    let style = Style::new().fg(palette::FG_MUTED).add_modifier(Modifier::ITALIC);
    let Some(text) = selected else {
        return vec![
            Line::default(),
            Line::from(Span::styled(
                "Nothing selected — drag over the text to quote some of it.",
                style,
            )),
        ];
    };
    let total = text.lines().count();
    let mut lines = vec![Line::from(Span::styled(
        if total > QUOTE_ROWS {
            format!("quoting {total} lines, beginning:")
        } else {
            "quoting:".to_string()
        },
        style,
    ))];
    lines.extend(
        text.lines()
            .take(QUOTE_ROWS)
            .map(|line| Line::from(Span::styled(format!("> {line}"), style))),
    );
    lines
}
