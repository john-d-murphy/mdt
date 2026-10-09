//! Keys for the image viewer overlay.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;

impl App {
    /// Handle a key while the image viewer is open: zoom, pan, or close.
    pub(crate) fn handle_image_viewer_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.close_image_viewer(),
            KeyCode::Char('+' | '=') => self.image_viewer.zoom_in(),
            KeyCode::Char('-' | '_') => self.image_viewer.zoom_out(),
            KeyCode::Char('0') => self.image_viewer.fit(),
            KeyCode::Char('h') | KeyCode::Left => self.image_viewer.pan(-1.0, 0.0),
            KeyCode::Char('l') | KeyCode::Right => self.image_viewer.pan(1.0, 0.0),
            KeyCode::Char('k') | KeyCode::Up => self.image_viewer.pan(0.0, -1.0),
            KeyCode::Char('j') | KeyCode::Down => self.image_viewer.pan(0.0, 1.0),
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

    /// An app with the viewer open on a 200×100 image.
    fn viewing(name: &str) -> (TempTestDir, App) {
        let dir = TempTestDir::new(name);
        dir.create_file("t.md", "# T");
        let path = dir.path().join("cat.png");
        image::RgbImage::new(200, 100).save(&path).unwrap();

        let mut app = App::new(dir.path(), Color::Reset).unwrap();
        app.images =
            crate::images::ImageState::with_picker(ratatui_image::picker::Picker::halfblocks());
        app.open_image_viewer(path);
        (dir, app)
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_image_viewer_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn plus_and_minus_step_the_zoom_and_stop_at_the_ends() {
        let (_dir, mut app) = viewing("mdt-test-viewer-keys-zoom");
        assert_eq!(app.image_viewer.zoom(), 100);
        press(&mut app, KeyCode::Char('+'));
        assert_eq!(app.image_viewer.zoom(), 150);
        press(&mut app, KeyCode::Char('='));
        assert_eq!(app.image_viewer.zoom(), 200);
        press(&mut app, KeyCode::Char('-'));
        assert_eq!(app.image_viewer.zoom(), 150);
        for _ in 0..10 {
            press(&mut app, KeyCode::Char('-'));
        }
        assert_eq!(app.image_viewer.zoom(), 100, "zooming out stops at the whole picture");
        for _ in 0..20 {
            press(&mut app, KeyCode::Char('+'));
        }
        assert_eq!(app.image_viewer.zoom(), 800, "and in at the last step");
    }

    #[test]
    fn zero_returns_to_the_whole_picture() {
        let (_dir, mut app) = viewing("mdt-test-viewer-keys-fit");
        press(&mut app, KeyCode::Char('+'));
        press(&mut app, KeyCode::Char('+'));
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('0'));
        assert_eq!(app.image_viewer.zoom(), 100);
        assert_eq!(app.image_viewer.crop_for_test(200, 100), (0, 0, 200, 100));
    }

    #[test]
    fn panning_moves_the_view_and_keeps_it_inside_the_picture() {
        let (_dir, mut app) = viewing("mdt-test-viewer-keys-pan");
        press(&mut app, KeyCode::Char('+')); // 150%: two thirds on show
        let (x0, _, w, _) = app.image_viewer.crop_for_test(200, 100);
        press(&mut app, KeyCode::Char('l'));
        let (x1, ..) = app.image_viewer.crop_for_test(200, 100);
        assert!(x1 > x0, "panning right moves the view right: {x0} -> {x1}");
        for _ in 0..20 {
            press(&mut app, KeyCode::Char('l'));
        }
        let (x2, ..) = app.image_viewer.crop_for_test(200, 100);
        assert_eq!(x2 + w, 200, "it stops at the right edge");
        for _ in 0..40 {
            press(&mut app, KeyCode::Char('h'));
        }
        assert_eq!(app.image_viewer.crop_for_test(200, 100).0, 0, "and at the left");
    }

    #[test]
    fn panning_at_the_whole_picture_does_nothing() {
        let (_dir, mut app) = viewing("mdt-test-viewer-keys-pan-fit");
        for code in [KeyCode::Char('l'), KeyCode::Char('j'), KeyCode::Up, KeyCode::Left] {
            press(&mut app, code);
        }
        assert_eq!(app.image_viewer.crop_for_test(200, 100), (0, 0, 200, 100));
    }

    #[test]
    fn esc_q_and_enter_all_close_it() {
        for code in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Enter] {
            let (_dir, mut app) = viewing("mdt-test-viewer-keys-close");
            press(&mut app, code);
            assert!(matches!(app.overlay, Overlay::None), "{code:?} should close the viewer");
        }
    }
}
