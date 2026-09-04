//! Drawing an image into the preview pane, as coloured half-blocks.
//!
//! **Halfblocks, deliberately, not the kitty graphics protocol.** Kitty renders
//! better, but it bypasses the cell buffer — so it cannot be asserted against a
//! `TestBackend` the way every other pane here is, it has to be probed for at
//! runtime, and it does not survive a multiplexer. Half-blocks are ordinary
//! coloured cells: they work in every terminal, under zellij and tmux, and a
//! test can read them straight out of the buffer.
//!
//! Each cell carries two pixels — a foreground block over a background — so the
//! effective resolution is the pane's width by twice its height.

use ratatui::layout::Rect;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::Resize;
use std::path::Path;

/// A decoded image, ready for the pane to draw.
pub type Drawable = Protocol;

/// Decode `path` and build something the pane can draw in `area`.
///
/// Returns `None` for anything that will not decode, which is not an error
/// worth reporting: the caller falls back to describing the file, and a
/// mislabelled `.png` is the ordinary case rather than a fault.
pub fn protocol_for(path: &Path, area: Rect) -> Option<Protocol> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let image = image::ImageReader::open(path).ok()?.decode().ok()?;
    // `halfblocks()`, never `from_query_stdio()`: querying writes an escape
    // sequence and reads the terminal's reply, which cannot be done from inside
    // a drawing pass — and whose answer this deliberately does not want, since
    // the whole point of half-blocks is that they work without one.
    Picker::halfblocks()
        .new_protocol(
            image,
            area,
            // Fit, not crop: a preview exists to show you what the whole file
            // is, and cropping to the pane would hide the half you cared about.
            Resize::Fit(None),
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny PNG written with `image` itself, so the test does not depend on
    /// a fixture file living in the repository.
    fn png(tag: &str, w: u32, h: u32) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cozy-img-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("swatch.png");
        let buf = image::RgbImage::from_fn(w, h, |x, _| {
            if x < w / 2 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        buf.save(&path).unwrap();
        path
    }

    #[test]
    fn an_image_becomes_something_drawable() {
        let path = png("basic", 8, 8);
        let p = protocol_for(&path, Rect::new(0, 0, 20, 10));
        assert!(p.is_some(), "a plain PNG should decode");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_file_that_is_not_an_image_is_declined_rather_than_panicking() {
        // A mislabelled `.png` is ordinary, not a fault.
        let dir = std::env::temp_dir().join(format!("cozy-img-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lying.png");
        std::fs::write(&path, "this is not a png").unwrap();
        assert!(protocol_for(&path, Rect::new(0, 0, 20, 10)).is_none());
        assert!(protocol_for(&dir.join("absent.png"), Rect::new(0, 0, 20, 10)).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_empty_area_is_declined_rather_than_dividing_by_zero() {
        let path = png("zero", 4, 4);
        assert!(protocol_for(&path, Rect::new(0, 0, 0, 10)).is_none());
        assert!(protocol_for(&path, Rect::new(0, 0, 10, 0)).is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_wide_image_is_fitted_rather_than_cropped() {
        // A preview exists to show what the whole file is; cropping would hide
        // the half you cared about.
        let path = png("wide", 64, 8);
        let p = protocol_for(&path, Rect::new(0, 0, 10, 10)).unwrap();
        let area = p.area();
        assert!(area.width <= 10 && area.height <= 10, "{area:?}");
        assert!(area.width > 0 && area.height > 0, "{area:?}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
