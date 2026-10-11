use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use crate::app::{App, Focus, Overlay};
use crate::ui::preview::PREVIEW_PADDING;

impl App {
    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) {
        // The image viewer takes the wheel to zoom, and closes on a click beside it.
        if matches!(self.overlay, Overlay::ImageViewer) {
            match mouse.kind {
                MouseEventKind::ScrollUp => self.image_viewer.zoom_in(),
                MouseEventKind::ScrollDown => self.image_viewer.zoom_out(),
                MouseEventKind::Down(MouseButton::Left) => {
                    let inside = self
                        .image_viewer
                        .area
                        .is_some_and(|r| r.contains(Position::new(mouse.column, mouse.row)));
                    if !inside {
                        self.close_image_viewer();
                    }
                }
                _ => {}
            }
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollDown => {
                if self.is_in_preview(mouse.column, mouse.row) {
                    for _ in 0..3 {
                        self.document.scroll_down();
                    }
                } else if self.is_in_file_list(mouse.column, mouse.row) {
                    for _ in 0..3 {
                        self.tree.tree_state.key_down();
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if self.is_in_preview(mouse.column, mouse.row) {
                    for _ in 0..3 {
                        self.document.scroll_up();
                    }
                } else if self.is_in_file_list(mouse.column, mouse.row) {
                    for _ in 0..3 {
                        self.tree.tree_state.key_up();
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.is_in_preview(mouse.column, mouse.row) {
                    self.focus = Focus::Preview;
                    self.click_preview(mouse.column, mouse.row);
                } else if self.is_in_file_list(mouse.column, mouse.row) {
                    self.focus = Focus::FileList;
                }
            }
            _ => {}
        }
    }

    /// Act on a left click in the preview pane: an image opens in the viewer, a link opens
    /// with the system handler, anything else just moves the focus (already done).
    fn click_preview(&mut self, col: u16, row: u16) {
        let Some(area) = self.preview_area else { return };
        let x0 = area.x + PREVIEW_PADDING.left;
        let y0 = area.y + PREVIEW_PADDING.top;
        if col < x0 || row < y0 {
            return;
        }
        let line_idx = self.document.scroll_offset + usize::from(row - y0);
        if let Some(path) = self.document.image_at(line_idx).cloned() {
            self.open_image_viewer(path);
            return;
        }
        let url = self.document.link_at(line_idx, usize::from(col - x0)).map(|l| l.url.clone());
        if let Some(url) = url {
            self.open_url(url);
        }
    }

    fn is_in_preview(&self, col: u16, row: u16) -> bool {
        self.preview_area.is_some_and(|r| r.contains(Position::new(col, row)))
    }

    fn is_in_file_list(&self, col: u16, row: u16) -> bool {
        self.file_list_area.is_some_and(|r| r.contains(Position::new(col, row)))
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    use crate::app::{App, Focus};
    use crate::test_util::TempTestDir;

    fn mouse_event(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent { kind, column: col, row, modifiers: crossterm::event::KeyModifiers::NONE }
    }

    fn setup_app_with_areas() -> (TempTestDir, App) {
        let dir = TempTestDir::new("mdt-test-mouse");
        let content = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n\n");
        dir.create_file("test.md", &content);

        let mut app = App::new(dir.path(), Color::Reset).unwrap();
        app.open_file(&dir.path().join("test.md"));
        app.document.viewport_height = 10;
        // Set up areas: file list on left, preview on right
        app.file_list_area = Some(Rect::new(0, 0, 30, 24));
        app.preview_area = Some(Rect::new(30, 0, 50, 24));
        (dir, app)
    }

    /// Open `markdown` as the document, with the preview pane at (30, 0) 50x24; return the
    /// app and the display column where `needle` starts on the first rendered line.
    fn setup_with_link(name: &str, markdown: &str, needle: &str) -> (TempTestDir, App, u16) {
        use unicode_width::UnicodeWidthStr;
        let dir = TempTestDir::new(name);
        dir.create_file("test.md", markdown);
        let mut app = App::new(dir.path(), Color::Reset).unwrap();
        app.open_file(&dir.path().join("test.md"));
        app.document.viewport_height = 10;
        app.preview_area = Some(Rect::new(30, 0, 50, 24));

        let line = &app.document.rendered_lines[0];
        let flat: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let byte = flat.find(needle).unwrap_or_else(|| panic!("{needle:?} not in {flat:?}"));
        let col = flat[..byte].width() as u16;
        (dir, app, col)
    }

    #[test]
    fn click_on_link_text_opens_it() {
        let (_dir, mut app, col) = setup_with_link(
            "mdt-test-mouse-click-link",
            "See [the docs](https://example.com/docs) now.\n",
            "the docs",
        );
        // Content starts at x = 30 + 2 (left padding), y = 0 + 1 (top padding).
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 32 + col + 3, 1));
        assert_eq!(app.status_message, "Opening: https://example.com/docs");
        assert_eq!(app.focus, Focus::Preview);
    }

    #[test]
    fn click_on_plain_text_opens_nothing() {
        let (_dir, mut app, col) = setup_with_link(
            "mdt-test-mouse-click-plain",
            "See [the docs](https://example.com/docs) now.\n",
            "See",
        );
        app.status_message.clear();
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 32 + col, 1));
        assert_eq!(app.status_message, "");
    }

    #[test]
    fn click_on_autolink_resolves_despite_humanised_label() {
        let (_dir, mut app, col) = setup_with_link(
            "mdt-test-mouse-click-autolink",
            "Go to <https://www.example.com/path/> today.\n",
            "https://www",
        );
        assert_eq!(app.document.links[0].display_text, "example.com/path");
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 32 + col + 5, 1));
        assert_eq!(app.status_message, "Opening: https://www.example.com/path/");
    }

    #[test]
    fn click_accounts_for_scroll_offset() {
        let md = (0..5).map(|i| format!("para {i}")).collect::<Vec<_>>().join("\n\n")
            + "\n\n[last](https://example.com/last)\n";
        let (_dir, mut app, _) = setup_with_link("mdt-test-mouse-click-scrolled", &md, "para 0");
        let link_line = app
            .document
            .rendered_lines
            .iter()
            .position(|l| l.spans.iter().any(|s| s.content.contains("last")))
            .unwrap();
        app.document.scroll_offset = link_line; // link now on the first visible row
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 32 + 1, 1));
        assert_eq!(app.status_message, "Opening: https://example.com/last");
    }

    #[test]
    fn click_in_padding_row_opens_nothing() {
        let (_dir, mut app, col) = setup_with_link(
            "mdt-test-mouse-click-padding",
            "[the docs](https://example.com/docs)\n",
            "the docs",
        );
        app.status_message.clear();
        // Row 0 is the top padding row, above the first rendered line.
        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 32 + col, 0));
        assert_eq!(app.status_message, "");
    }

    #[test]
    fn scroll_down_in_preview() {
        let (_dir, mut app) = setup_app_with_areas();
        let initial = app.document.scroll_offset;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, 40, 10));

        assert_eq!(app.document.scroll_offset, initial + 3);
    }

    #[test]
    fn scroll_up_in_preview() {
        let (_dir, mut app) = setup_app_with_areas();
        app.document.scroll_offset = 10;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollUp, 40, 10));

        assert_eq!(app.document.scroll_offset, 7);
    }

    #[test]
    fn left_click_in_preview_sets_focus() {
        let (_dir, mut app) = setup_app_with_areas();
        app.focus = Focus::FileList;

        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 40, 10));

        assert_eq!(app.focus, Focus::Preview);
    }

    #[test]
    fn left_click_in_file_list_sets_focus() {
        let (_dir, mut app) = setup_app_with_areas();
        app.focus = Focus::Preview;

        app.handle_mouse(mouse_event(MouseEventKind::Down(MouseButton::Left), 10, 10));

        assert_eq!(app.focus, Focus::FileList);
    }

    #[test]
    fn scroll_outside_areas_does_nothing() {
        let (_dir, mut app) = setup_app_with_areas();
        app.preview_area = None;
        app.file_list_area = None;
        let initial = app.document.scroll_offset;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, 40, 10));

        assert_eq!(app.document.scroll_offset, initial);
    }

    #[test]
    fn scroll_up_at_zero_stays_zero() {
        let (_dir, mut app) = setup_app_with_areas();
        app.document.scroll_offset = 0;

        app.handle_mouse(mouse_event(MouseEventKind::ScrollUp, 40, 10));

        assert_eq!(app.document.scroll_offset, 0);
    }
}
