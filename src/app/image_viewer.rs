//! The image viewer overlay: opening and closing it.

use std::path::PathBuf;

use super::types::Overlay;
use super::App;

impl App {
    /// Show `path` in the viewer, whole and centred — a click on an image in the preview.
    ///
    /// Needs a picker, so under `--no-images` it only says why it cannot.
    pub(crate) fn open_image_viewer(&mut self, path: PathBuf) {
        if !self.images.available() {
            self.status_message = "Images are off for this run (--no-images)".to_string();
            return;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
        self.image_viewer.open(path);
        self.overlay = Overlay::ImageViewer;
        self.status_message = format!("{name} — +/- to zoom, hjkl to pan, esc to close");
    }

    pub(crate) fn close_image_viewer(&mut self) {
        self.image_viewer.close();
        self.overlay = Overlay::None;
        self.status_message.clear();
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use crate::app::{App, Overlay};
    use crate::test_util::TempTestDir;

    fn app_with_images(name: &str) -> (TempTestDir, App) {
        let dir = TempTestDir::new(name);
        dir.create_file("t.md", "# T");
        let mut app = App::new(dir.path(), Color::Reset).unwrap();
        app.images =
            crate::images::ImageState::with_picker(ratatui_image::picker::Picker::halfblocks());
        (dir, app)
    }

    #[test]
    fn opening_sets_the_overlay_and_names_the_file() {
        let (dir, mut app) = app_with_images("mdt-test-viewer-open");
        app.open_image_viewer(dir.path().join("cat.png"));
        assert!(matches!(app.overlay, Overlay::ImageViewer));
        assert_eq!(app.image_viewer.path.as_deref(), Some(dir.path().join("cat.png").as_path()));
        assert!(app.status_message.starts_with("cat.png — "), "{}", app.status_message);
        assert_eq!(app.image_viewer.zoom(), 100, "it opens showing the whole picture");
    }

    #[test]
    fn closing_clears_the_overlay() {
        let (dir, mut app) = app_with_images("mdt-test-viewer-close");
        app.open_image_viewer(dir.path().join("cat.png"));
        app.close_image_viewer();
        assert!(matches!(app.overlay, Overlay::None));
        assert!(app.image_viewer.path.is_none());
        assert!(app.status_message.is_empty());
    }

    #[test]
    fn without_a_picker_it_says_so_and_opens_nothing() {
        let dir = TempTestDir::new("mdt-test-viewer-no-picker");
        dir.create_file("t.md", "# T");
        let mut app = App::new(dir.path(), Color::Reset).unwrap();

        app.open_image_viewer(dir.path().join("cat.png"));
        assert!(matches!(app.overlay, Overlay::None));
        assert!(app.status_message.contains("--no-images"), "{}", app.status_message);
    }
}
