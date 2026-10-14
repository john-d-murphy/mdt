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

    /// Run the note command with what has been gathered, and say how it went.
    pub(crate) fn submit_note(&mut self) {
        let comment = std::mem::take(&mut self.note_input);
        self.overlay = Overlay::None;

        let Some(command) = self.note_command.clone() else {
            self.status_message = "No note command: start mdt with --note-command".to_string();
            return;
        };
        if comment.trim().is_empty() {
            self.status_message = "Nothing written, so no note".to_string();
            return;
        }

        let selection = self.document.selected_text(&self.selection).unwrap_or_default();
        let file = self
            .document
            .current_file
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let reference = self.source_ref().unwrap_or_default();

        let (shell, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
        let output = std::process::Command::new(shell)
            .arg(flag)
            .arg(&command)
            .env("MDT_NOTE_COMMENT", &comment)
            .env("MDT_NOTE_SELECTION", &selection)
            .env("MDT_NOTE_FILE", &file)
            .env("MDT_NOTE_REF", &reference)
            .env("MDT_NOTE_LINE", (self.note_line + 1).to_string())
            .output();

        self.status_message = match output {
            Ok(done) if done.status.success() => match first_line(&done.stdout) {
                Some(said) => said,
                None => "Note made".to_string(),
            },
            Ok(done) => match first_line(&done.stderr) {
                Some(said) => format!("Note failed: {said}"),
                None => format!("Note command failed: {}", done.status),
            },
            Err(e) => format!("Could not run the note command: {e}"),
        };
    }
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
