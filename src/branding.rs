//! The window uses the same original artwork as the executable resource.
pub fn window_icon() -> egui::IconData {
    egui::IconData {
        rgba: crate::icon_pixels::rgba(32),
        width: 32,
        height: 32,
    }
}
