//! The image viewer overlay: one picture, large, in a modal frame.

use ratatui::layout::Rect;
use ratatui::Frame;

use super::modal;
use crate::app::App;

/// Draw the viewer over `area`, nearly filling it, and record its rect so that a click
/// outside can close it.
pub(super) fn draw_image_viewer(frame: &mut Frame, area: Rect, app: &mut App) {
    let popup =
        modal::centered_rect(area.width.saturating_sub(4), area.height.saturating_sub(2), area);
    let name = app
        .image_viewer
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("image")
        .to_string();
    let zoom = app.image_viewer.zoom();
    let title = format!("{name}   {zoom}%");
    let content = modal::render_modal_frame(
        frame,
        popup,
        &title,
        None,
        &[("zoom", "+/-"), ("pan", "hjkl"), ("whole", "0")],
        app.bg_color,
        false,
    );
    app.image_viewer.area = Some(popup);
    app.images.draw_viewer(frame, content, &app.image_viewer);
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Cell;
    use ratatui::style::Color;
    use ratatui::Terminal;

    use crate::app::App;
    use crate::test_util::TempTestDir;

    #[test]
    fn the_viewer_paints_the_picture_and_its_frame() {
        let dir = TempTestDir::new("mdt-test-ui-viewer");
        dir.create_file("t.md", "# T");
        let path = dir.path().join("red.png");
        image::RgbImage::from_pixel(120, 80, image::Rgb([255, 0, 0])).save(&path).unwrap();

        let mut app = App::new(dir.path(), Color::Reset).unwrap();
        app.images =
            crate::images::ImageState::with_picker(ratatui_image::picker::Picker::halfblocks());
        app.open_image_viewer(path);

        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();

        let rows: Vec<String> =
            (0..20).map(|y| (0..60).map(|x| buf[(x, y)].symbol().to_string()).collect()).collect();
        assert!(
            rows.iter().any(|r| r.contains("red.png") && r.contains("100%")),
            "the title names the file and the zoom: {rows:?}"
        );
        assert!(rows.iter().any(|r| r.contains("zoom") && r.contains("+/-")), "shortcuts bar");

        let painted = (0..20)
            .filter(|&y| {
                (0..60).any(|x| {
                    let cell = &buf[(x, y)];
                    cell.symbol() != " " || cell.style() != Cell::EMPTY.style()
                })
            })
            .count();
        assert!(painted > 5, "the picture fills much of the box: {painted} rows painted");
        assert!(app.image_viewer.area.is_some(), "its rect is recorded for click-outside");
    }
}
