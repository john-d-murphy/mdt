//! Keys for the note overlay: type a comment, Enter to make it, Esc to abandon it.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;

impl App {
    pub(crate) fn handle_note_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.cancel_note(),
            KeyCode::Enter => self.submit_note(),
            KeyCode::Backspace => {
                self.note_input.pop();
            }
            KeyCode::Char(c) => self.note_input.push(c),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Color;

    use crate::app::{App, Overlay};
    use crate::test_util::TempTestDir;

    fn app_noting(name: &str) -> (TempTestDir, App) {
        let dir = TempTestDir::new(name);
        dir.create_file("page.md", "# A page\n\n`note` · 2026-01-01 · `only0`\n\n---\n\nWords.\n");
        let mut app = App::new(dir.path(), Color::Reset).unwrap();
        app.note_command = Some("echo \"$MDT_NOTE_COMMENT\"".to_string());
        app.open_file(&dir.path().join("page.md"));
        app.start_note(3);
        (dir, app)
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_note_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn typing_builds_the_comment_and_backspace_takes_it_back() {
        let (_dir, mut app) = app_noting("mdt-test-note-keys-type");
        for c in "hi!".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert_eq!(app.note_input, "hi!");
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.note_input, "hi");
    }

    #[test]
    fn enter_makes_the_note_and_esc_abandons_it() {
        let (_dir, mut app) = app_noting("mdt-test-note-keys-enter");
        for c in "kept".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.status_message, "kept");
        assert!(matches!(app.overlay, Overlay::None));

        app.start_note(3);
        press(&mut app, KeyCode::Char('x'));
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.overlay, Overlay::None));
        assert!(app.note_input.is_empty());
    }
}
