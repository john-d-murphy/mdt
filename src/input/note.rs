//! Keys for the note overlay: write a comment, `Ctrl+d` to make the note, `Ctrl+e` to write
//! it in `$EDITOR` instead, `esc` to abandon it.
//!
//! Enter starts another line rather than making the note, since a note is often more than
//! one sentence — so `Ctrl+d` finishes it, the way it ends any other text being read in.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;

impl App {
    pub(crate) fn handle_note_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.cancel_note(),
            KeyCode::Char('d') if ctrl => self.submit_note(),
            KeyCode::Char('e') if ctrl => self.request_note_editor(),
            KeyCode::Enter => self.note_input.push('\n'),
            KeyCode::Backspace => {
                self.note_input.pop();
            }
            KeyCode::Char(c) if !ctrl => self.note_input.push(c),
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
        app.note_command = Some("cat".to_string());
        app.open_file(&dir.path().join("page.md"));
        app.start_note(3);
        (dir, app)
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_note_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(app: &mut App, c: char) {
        app.handle_note_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    fn type_in(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    #[test]
    fn typing_builds_the_comment_and_backspace_takes_it_back() {
        let (_dir, mut app) = app_noting("mdt-test-note-keys-type");
        type_in(&mut app, "hi!");
        assert_eq!(app.note_input, "hi!");
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.note_input, "hi");
    }

    #[test]
    fn enter_starts_another_line_rather_than_making_the_note() {
        let (_dir, mut app) = app_noting("mdt-test-note-keys-enter");
        type_in(&mut app, "first");
        press(&mut app, KeyCode::Enter);
        type_in(&mut app, "second");
        assert_eq!(app.note_input, "first\nsecond");
        assert!(matches!(app.overlay, Overlay::NoteInput), "still writing");

        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.note_input, "first\nseco", "backspace takes characters off the end");
        for _ in 0..5 {
            press(&mut app, KeyCode::Backspace);
        }
        assert_eq!(app.note_input, "first", "including the line break itself");
    }

    #[test]
    fn ctrl_d_makes_the_note_with_every_line_of_it() {
        let (_dir, mut app) = app_noting("mdt-test-note-keys-ctrl-d");
        app.note_command = Some("printf '%s' \"$MDT_NOTE_COMMENT\" | tr '\\n' '/'".to_string());
        type_in(&mut app, "one");
        press(&mut app, KeyCode::Enter);
        type_in(&mut app, "two");
        ctrl(&mut app, 'd');
        assert_eq!(app.status_message, "one/two");
        assert!(matches!(app.overlay, Overlay::None));
    }

    #[test]
    fn a_control_chord_is_not_typed_into_the_comment() {
        let (_dir, mut app) = app_noting("mdt-test-note-keys-ctrl-other");
        type_in(&mut app, "x");
        ctrl(&mut app, 'z');
        assert_eq!(app.note_input, "x", "Ctrl+z is not a character here");
    }

    #[test]
    fn esc_abandons_it() {
        let (_dir, mut app) = app_noting("mdt-test-note-keys-esc");
        type_in(&mut app, "half a thought");
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.overlay, Overlay::None));
        assert!(app.note_input.is_empty());
    }

    #[test]
    fn ctrl_e_hands_the_note_to_the_command_with_the_terminal() {
        let (_dir, mut app) = app_noting("mdt-test-note-keys-ctrl-e");
        type_in(&mut app, "a start");
        ctrl(&mut app, 'e');

        assert!(app.note_in_editor, "the event loop is asked to step out of the way");
        assert!(matches!(app.overlay, Overlay::None), "the box is out of the way too");
        assert_eq!(app.note_input, "a start", "and the first line is kept for the command");
    }

    #[test]
    fn ctrl_e_with_nothing_to_note_says_so_and_stays_in_the_box() {
        let (_dir, mut app) = app_noting("mdt-test-note-keys-ctrl-e-empty");
        ctrl(&mut app, 'e');
        assert!(!app.note_in_editor);
        assert!(matches!(app.overlay, Overlay::NoteInput), "still in the box");
        assert_eq!(app.status_message, "Nothing written and nothing selected, so no note");
    }
}
