//! Semantic color roles (`frame::theme::Role`) to egui colors: the dark
//! palette, unmultiplied, and the 关系淡化 dim.

use egui::Color32;
use frame::theme::{self, Rgb, Role};

/// The palette color of a role.
pub fn color(role: Role) -> Color32 {
    let c = theme::rgba(role);
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// An opaque palette constant.
pub fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// Dim all premultiplied channels together; changing alpha alone would
/// make an opaque color brighter when blended.
pub fn fade(color: Color32) -> Color32 {
    color.gamma_multiply_u8(theme::ALPHA_DIMMED)
}

/// A role, dimmed when its element is outside the dependency chain.
pub fn ink(role: Role, dimmed: bool) -> Color32 {
    let c = color(role);
    if dimmed { fade(c) } else { c }
}
