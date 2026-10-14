//! The image viewer overlay: which picture it is on, and moving between them.

use super::types::Overlay;
use super::App;

impl App {
    /// Show the document's `index`th image, scrolling the page to it so that closing the
    /// viewer leaves the reader where they were looking.
    pub(crate) fn show_image(&mut self, index: usize) {
        let images = self.document.images();
        let Some((path, line)) = images.get(index).cloned() else {
            self.status_message = "No images in this document".to_string();
            return;
        };
        if !self.images.available() {
            self.status_message = "Images are off for this run (--no-images)".to_string();
            return;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
        self.document.scroll_to_line(line);
        self.image_viewer.open(path, index);
        self.overlay = Overlay::ImageViewer;
        self.status_message = format!("{name} ({} of {})", index + 1, images.len());
    }

    /// Open the viewer on the first image on screen, or the next one below it — `I`.
    pub(crate) fn view_nearest_image(&mut self) {
        let images = self.document.images();
        if images.is_empty() {
            self.status_message = "No images in this document".to_string();
            return;
        }
        let from = self.document.scroll_offset;
        let index = images.iter().position(|(_, line)| *line >= from).unwrap_or(images.len() - 1);
        self.show_image(index);
    }

    /// Step `delta` images on from the one on show, wrapping round — `n` and `N`.
    pub(crate) fn step_image(&mut self, delta: isize) {
        let count = self.document.images().len();
        if count == 0 {
            return;
        }
        let count_i = isize::try_from(count).unwrap_or(isize::MAX);
        let here = isize::try_from(self.image_viewer.index).unwrap_or(0);
        let next = (here + delta).rem_euclid(count_i);
        self.show_image(usize::try_from(next).unwrap_or(0));
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

    /// A document of three images, with the picker in place.
    fn app_with_images(name: &str) -> (TempTestDir, App) {
        let dir = TempTestDir::new(name);
        for file in ["one.png", "two.png", "three.png"] {
            image::RgbImage::new(16, 16).save(dir.path().join(file)).unwrap();
        }
        dir.create_file(
            "t.md",
            "# T\n\n![one](one.png)\n\nwords\n\n![two](two.png)\n\nmore\n\n![three](three.png)\n",
        );
        let mut app = App::new(dir.path(), Color::Reset).unwrap();
        app.images =
            crate::images::ImageState::with_picker(ratatui_image::picker::Picker::halfblocks());
        app.open_file(&dir.path().join("t.md"));
        (dir, app)
    }

    fn showing(app: &App) -> String {
        app.image_viewer
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("none")
            .to_string()
    }

    #[test]
    fn the_document_lists_its_images_in_order() {
        let (_dir, app) = app_with_images("mdt-test-viewer-list");
        let names: Vec<String> = app
            .document
            .images()
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["one.png", "two.png", "three.png"]);
        let lines: Vec<usize> = app.document.images().iter().map(|(_, l)| *l).collect();
        assert!(lines[0] < lines[1] && lines[1] < lines[2], "in document order: {lines:?}");
    }

    #[test]
    fn showing_an_image_opens_the_overlay_and_scrolls_the_page_to_it() {
        let (_dir, mut app) = app_with_images("mdt-test-viewer-show");
        app.document.viewport_height = 4;
        app.show_image(2);
        assert!(matches!(app.overlay, Overlay::ImageViewer));
        assert_eq!(showing(&app), "three.png");
        assert_eq!(app.status_message, "three.png (3 of 3)");
        let (_, line) = app.document.images()[2].clone();
        assert!(app.document.scroll_offset + 4 > line, "the page moved to it");
    }

    #[test]
    fn i_opens_the_first_image_on_screen_or_the_next_one_down() {
        let (_dir, mut app) = app_with_images("mdt-test-viewer-nearest");
        app.view_nearest_image();
        assert_eq!(showing(&app), "one.png", "from the top, the first one");

        app.close_image_viewer();
        let (_, second) = app.document.images()[1].clone();
        app.document.scroll_offset = second;
        app.view_nearest_image();
        assert_eq!(showing(&app), "two.png", "scrolled to the second, that one");
    }

    #[test]
    fn stepping_moves_through_them_and_wraps_both_ways() {
        let (_dir, mut app) = app_with_images("mdt-test-viewer-step");
        app.show_image(0);
        app.step_image(1);
        assert_eq!(showing(&app), "two.png");
        app.step_image(1);
        assert_eq!(showing(&app), "three.png");
        app.step_image(1);
        assert_eq!(showing(&app), "one.png", "past the last comes the first");
        app.step_image(-1);
        assert_eq!(showing(&app), "three.png", "and back the other way");
    }

    #[test]
    fn a_document_with_no_images_says_so() {
        let dir = TempTestDir::new("mdt-test-viewer-none");
        dir.create_file("t.md", "# Just words\n");
        let mut app = App::new(dir.path(), Color::Reset).unwrap();
        app.images =
            crate::images::ImageState::with_picker(ratatui_image::picker::Picker::halfblocks());
        app.open_file(&dir.path().join("t.md"));

        app.view_nearest_image();
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(app.status_message, "No images in this document");
    }

    #[test]
    fn without_a_picker_it_says_so_and_opens_nothing() {
        let dir = TempTestDir::new("mdt-test-viewer-no-picker");
        image::RgbImage::new(16, 16).save(dir.path().join("one.png")).unwrap();
        dir.create_file("t.md", "![one](one.png)\n");
        let mut app = App::new(dir.path(), Color::Reset).unwrap();
        app.open_file(&dir.path().join("t.md"));

        app.view_nearest_image();
        assert!(matches!(app.overlay, Overlay::None));
        assert!(app.status_message.contains("--no-images"), "{}", app.status_message);
    }

    #[test]
    fn closing_clears_the_overlay() {
        let (_dir, mut app) = app_with_images("mdt-test-viewer-close");
        app.show_image(0);
        app.close_image_viewer();
        assert!(matches!(app.overlay, Overlay::None));
        assert!(app.image_viewer.path.is_none());
        assert!(app.status_message.is_empty());
    }
}
