//! The app icon: the embedded PNG, decoded and downscaled for the
//! window/taskbar.

/// The icon source, a single square PNG checked into the repo.
const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon.png");

/// The icon size handed to the window manager. Taskbars and title
/// bars render icons small; 128×128 is plenty.
const ICON_SIZE: u32 = 128;

/// The app icon for the window (`with_window_icon`). The embedded PNG
/// is pixel art with large "pixels", so it is downscaled with
/// nearest-neighbor filtering — anything smoother would smear the
/// blocky pixels. Any failure means no icon, never fatal.
pub fn window_icon() -> Option<winit::window::Icon> {
    let image = image::load_from_memory(ICON_PNG)
        .ok()?
        .resize_exact(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Nearest)
        .to_rgba8();
    let (width, height) = (image.width(), image.height());
    winit::window::Icon::from_rgba(image.into_raw(), width, height).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded icon decodes into a valid window icon — pure
    /// data, no window needed.
    #[test]
    fn the_embedded_png_makes_a_window_icon() {
        let image = image::load_from_memory(ICON_PNG).expect("the embedded PNG decodes");
        assert!(image.width() == image.height(), "the icon is square");
        assert!(image.width() >= ICON_SIZE, "the icon is at least {ICON_SIZE}px");

        let icon = window_icon().expect("a valid window icon");
        // The public API only Debug-prints it; round-trip through
        // that to pin something observable.
        let debug = format!("{icon:?}");
        assert!(debug.contains("Icon"), "an icon debug-prints as an icon");
    }
}
