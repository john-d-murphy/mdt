//! Making a note about what is being read: the overlay, and the command that does it.
//!
//! mdt knows nothing about how a note is made. Right-clicking the preview asks for a
//! comment and then runs the command given by `--note-command`, handing it what mdt does
//! know — the comment, the selected text, the file, the page's own id, the line — in the
//! environment. What that command does with them is its business.

use super::types::Overlay;
use super::App;

impl App {
    /// Start a note about rendered line `line` — a right-click in the preview.
    pub(crate) fn start_note(&mut self, line: usize) {
        if self.note_command.is_none() {
            self.status_message = "No note command: start mdt with --note-command".to_string();
            return;
        }
        self.note_line = line;
        self.note_input.clear();
        self.overlay = Overlay::NoteInput;
    }

    pub(crate) fn cancel_note(&mut self) {
        self.overlay = Overlay::None;
        self.note_input.clear();
        self.status_message.clear();
    }

    /// The quine node a page is about, from the metadata `quine read` prints under the
    /// title:
    ///
    /// ```text
    /// # Why the room is the instrument
    ///
    /// `essay` · 2025-09-21 · `room0`
    /// ```
    ///
    /// The last backticked word there is the node's short id. This reads the page rather
    /// than the file, so it works for a page arriving down a pipe, which has no path.
    /// `None` for a page without that line — an ordinary markdown file — which makes a note
    /// with nothing to link it to.
    pub(crate) fn source_ref(&self) -> Option<String> {
        self.document.file_content.lines().take(8).find_map(|line| {
            let line = line.trim();
            if !line.starts_with('`') || !line.contains(" · ") {
                return None;
            }
            let id = line.rsplit('`').nth(1)?;
            let is_id =
                !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
            is_id.then(|| id.to_string())
        })
    }

    /// `Ctrl+d`: run the note command and report what it said.
    pub(crate) fn submit_note(&mut self) {
        let Some(mut note) = self.take_note(false) else { return };
        let output = note.command.output();
        self.status_message = match output {
            Ok(done) if done.status.success() => {
                first_line(&done.stdout).unwrap_or_else(|| "Note made".to_string())
            }
            Ok(done) => match first_line(&done.stderr) {
                Some(said) => format!("Note failed: {said}"),
                None => format!("Note command failed: {}", done.status),
            },
            Err(e) => format!("Could not run the note command: {e}"),
        };
    }

    /// `Ctrl+e`: ask the event loop to run the note command with the terminal handed over,
    /// so the command can open an editor on the note it makes — `quine new -e` does, which
    /// means the file being edited is the real note, front matter and all.
    pub(crate) fn request_note_editor(&mut self) {
        if self.note_input.trim().is_empty() {
            self.status_message = "Write a first line, then ctrl+e opens it".to_string();
            return;
        }
        if self.note_command.is_none() {
            self.status_message = "No note command: start mdt with --note-command".to_string();
            return;
        }
        self.overlay = Overlay::None;
        self.note_in_editor = true;
    }

    /// Run the note command with the terminal already handed over: its own output goes to
    /// the screen, which the event loop is about to take back, so only how it ended is
    /// reported here.
    pub(crate) fn run_note_in_editor(&mut self) {
        let Some(mut note) = self.take_note(true) else { return };
        self.status_message = match note.command.status() {
            Ok(status) if status.success() => "Note made".to_string(),
            Ok(status) => format!("Note command failed: {status}"),
            Err(e) => format!("Could not run the note command: {e}"),
        };
    }

    /// The note command, loaded with everything mdt knows, and the box emptied. `None` —
    /// having said why — when there is nothing to make a note with.
    fn take_note(&mut self, in_editor: bool) -> Option<Note> {
        let comment = std::mem::take(&mut self.note_input);
        self.overlay = Overlay::None;

        let Some(command) = self.note_command.clone() else {
            self.status_message = "No note command: start mdt with --note-command".to_string();
            return None;
        };
        if comment.trim().is_empty() {
            self.status_message = "Nothing written, so no note".to_string();
            return None;
        }

        let (shell, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
        let mut process = std::process::Command::new(shell);
        process
            .arg(flag)
            .arg(&command)
            .env("MDT_NOTE_COMMENT", &comment)
            .env(
                "MDT_NOTE_SELECTION",
                self.document.selected_text(&self.selection).unwrap_or_default(),
            )
            .env(
                "MDT_NOTE_FILE",
                self.document
                    .current_file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            )
            .env("MDT_NOTE_REF", self.source_ref().unwrap_or_default())
            .env("MDT_NOTE_LINE", (self.note_line + 1).to_string());
        if in_editor {
            // The command has the terminal: it may open an editor.
            process.env("MDT_NOTE_EDIT", "1");
        }
        Some(Note { command: process })
    }
}

/// A note command, ready to run.
struct Note {
    command: std::process::Command,
}

/// The first line a command said, if it said anything.
fn first_line(output: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(output);
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use crate::app::{App, Overlay};
    use crate::test_util::TempTestDir;

    /// A quine page as `quine read --markdown` writes one.
    const PAGE: &str = "# Why the room is the instrument\n\n\
        `essay` · 2025-09-21 · `room0`\n\n---\n\nThe room has a resonance near 110 Hz.\n";

    fn app_with(page: &str, command: Option<&str>) -> (TempTestDir, App) {
        let dir = TempTestDir::new("mdt-test-note");
        dir.create_file("page.md", page);
        let mut app = App::new(dir.path(), Color::Reset).unwrap();
        app.note_command = command.map(str::to_string);
        app.open_file(&dir.path().join("page.md"));
        (dir, app)
    }

    #[test]
    fn the_page_id_comes_from_the_metadata_line() {
        let (_dir, app) = app_with(PAGE, None);
        assert_eq!(app.source_ref().as_deref(), Some("room0"));
    }

    #[test]
    fn a_page_without_that_line_has_no_id() {
        let (_dir, app) = app_with("# Just a file\n\nSome words.\n", None);
        assert_eq!(app.source_ref(), None);
    }

    #[test]
    fn a_page_down_a_pipe_still_has_its_id() {
        let app = App::from_stdin(PAGE.to_string(), Color::Reset);
        assert_eq!(app.source_ref().as_deref(), Some("room0"));
        assert!(app.document.current_file.is_none(), "and no file to go with it");
    }

    #[test]
    fn right_clicking_without_a_command_configured_says_so() {
        let (_dir, mut app) = app_with(PAGE, None);
        app.start_note(3);
        assert!(matches!(app.overlay, Overlay::None), "no overlay to type into");
        assert!(app.status_message.contains("--note-command"), "{}", app.status_message);
    }

    #[test]
    fn the_command_is_given_the_comment_the_page_and_the_line() {
        let (_dir, mut app) =
            app_with(PAGE, Some("echo \"$MDT_NOTE_REF/$MDT_NOTE_LINE: $MDT_NOTE_COMMENT\""));
        app.start_note(3);
        assert!(matches!(app.overlay, Overlay::NoteInput));

        app.note_input = "worth measuring again".to_string();
        app.submit_note();
        assert_eq!(app.status_message, "room0/4: worth measuring again");
        assert!(matches!(app.overlay, Overlay::None));
        assert!(app.note_input.is_empty(), "and the box is emptied");
    }

    #[test]
    fn the_command_is_given_the_selected_text() {
        let (_dir, mut app) = app_with(PAGE, Some("echo \"[$MDT_NOTE_SELECTION]\""));
        app.document.viewport_width = 60;
        let line = app
            .document
            .rendered_lines
            .iter()
            .position(|l| l.spans.iter().any(|s| s.content.contains("resonance")))
            .expect("the body is rendered");

        app.selection.press(line, 15);
        app.selection.drag(line, 23);
        app.note_input = "note".to_string();
        app.submit_note();
        assert_eq!(app.status_message, "[resonance]", "{}", app.status_message);
    }

    #[test]
    fn the_editor_run_tells_the_command_it_has_the_terminal() {
        // `test` says nothing, so nothing is written over the screen being handed back.
        let (_dir, mut app) =
            app_with(PAGE, Some("test -n \"$MDT_NOTE_EDIT\" -a -n \"$MDT_NOTE_COMMENT\""));
        app.note_input = "a start".to_string();
        app.run_note_in_editor();
        assert_eq!(app.status_message, "Note made");
    }

    #[test]
    fn the_quick_run_does_not_claim_to_have_the_terminal() {
        let (_dir, mut app) = app_with(PAGE, Some("echo \"edit=[${MDT_NOTE_EDIT:-}]\""));
        app.note_input = "x".to_string();
        app.submit_note();
        assert_eq!(app.status_message, "edit=[]");
    }

    #[test]
    fn a_failing_command_says_what_it_said() {
        let (_dir, mut app) = app_with(PAGE, Some("echo 'no such relationship' >&2; exit 1"));
        app.note_input = "x".to_string();
        app.submit_note();
        assert_eq!(app.status_message, "Note failed: no such relationship");
    }

    #[test]
    fn an_empty_comment_makes_no_note() {
        let (_dir, mut app) = app_with(PAGE, Some("echo made"));
        app.start_note(3);
        app.note_input = "   ".to_string();
        app.submit_note();
        assert_eq!(app.status_message, "Nothing written, so no note");
    }

    #[test]
    fn escape_abandons_it() {
        let (_dir, mut app) = app_with(PAGE, Some("echo made"));
        app.start_note(3);
        app.note_input = "half a thought".to_string();
        app.cancel_note();
        assert!(matches!(app.overlay, Overlay::None));
        assert!(app.note_input.is_empty());
    }
}
