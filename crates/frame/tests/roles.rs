//! Semantic color roles (#16): every color the viewer drew before the frame
//! existed is a role, and the dark palette resolves each role to exactly
//! the color (and alpha) the deleted egui window painted for that use. The
//! literals are the pre-frame theme constants and paint alphas.

use frame::theme::{self, Rgba, Role};

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Rgba {
    Rgba { r, g, b, a }
}

#[test]
fn the_dark_palette_keeps_every_existing_color_use() {
    let cases = [
        (Role::Canvas, rgba(22, 28, 32, 255)),
        (Role::Panel, rgba(26, 36, 40, 255)),
        (Role::Sidebar, rgba(18, 22, 24, 255)),
        (Role::Line, rgba(42, 61, 54, 255)),
        (Role::Ink, rgba(236, 236, 228, 255)),
        (Role::Muted, rgba(157, 184, 168, 255)),
        (Role::Accent, rgba(232, 196, 72, 255)),
        (Role::Marker, rgba(255, 255, 255, 255)),
        (Role::Selected, rgba(255, 255, 255, 255)),
        (Role::Good, rgba(95, 181, 138, 255)),
        (Role::Danger, rgba(255, 64, 48, 255)),
        // gamma_multiply(0.92) of the canvas: 235/255.
        (Role::ChromePlate, rgba(22, 28, 32, 235)),
        (Role::Popup, rgba(18, 22, 24, 240)),
        (Role::RowWash, rgba(232, 196, 72, 32)),
        (Role::RowBar, rgba(232, 196, 72, 255)),
        (Role::QualityFill(None), rgba(36, 52, 48, 255)),
        (Role::QualityFill(Some(5.0)), rgba(61, 58, 24, 255)),
        (Role::QualityStroke(Some(1.0)), rgba(222, 136, 77, 255)),
        (Role::QualityStroke(Some(10.0)), rgba(95, 181, 138, 255)),
        (Role::PackageFill(Some(10.0)), rgba(30, 74, 56, 80)),
        (Role::PackageStroke(None), rgba(90, 110, 100, 170)),
        (Role::Coverage(None), rgba(157, 184, 168, 255)),
        (Role::Coverage(Some(0.9)), rgba(95, 181, 138, 255)),
        (Role::Coverage(Some(0.6)), rgba(232, 196, 72, 255)),
        (Role::Coverage(Some(0.2)), rgba(224, 122, 74, 255)),
        (Role::NoData, rgba(196, 42, 36, 255)),
        (Role::Relation, rgba(157, 184, 168, 255)),
        (Role::RelationHover, rgba(255, 255, 255, 255)),
        (Role::DependencyChain, rgba(62, 152, 236, 255)),
        (Role::Violation, rgba(196, 42, 36, 255)),
        (Role::ViolationHot, rgba(255, 64, 48, 255)),
        (Role::Indicator, rgba(157, 184, 168, 255)),
        (Role::PortFill, rgba(42, 56, 52, 255)),
        (Role::PortFillMarked, rgba(74, 96, 78, 255)),
        (Role::PortOutline, rgba(157, 184, 168, 255)),
        (Role::PortLink, rgba(157, 184, 168, 255)),
        (Role::ButtonHover, rgba(54, 76, 66, 255)),
        (Role::ButtonActive, rgba(66, 90, 76, 255)),
        (Role::SourceBg, rgba(26, 36, 40, 255)),
        (Role::SourceInk, rgba(236, 236, 228, 255)),
        (Role::SourceGutterBg, rgba(18, 22, 24, 255)),
        (Role::SourceGutter, rgba(157, 184, 168, 255)),
        (Role::SourceSelection, rgba(38, 54, 70, 255)),
        (Role::SourceComment, rgba(120, 140, 130, 255)),
        (Role::SourceString, rgba(230, 160, 110, 255)),
        (Role::SourceKeyword, rgba(122, 162, 247, 255)),
    ];
    for (role, expected) in cases {
        assert_eq!(expected, theme::rgba(role), "{role:?}");
    }
    let named: Vec<&str> = cases.iter().map(|(r, _)| r.name()).collect();
    for role in Role::ALL {
        assert!(named.contains(&role.name()), "{} untested", role.name());
    }
}

#[test]
fn role_names_are_unique_kebab_case() {
    let mut names: Vec<&str> = Role::ALL.iter().map(|r| r.name()).collect();
    for name in &names {
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            "{name}"
        );
    }
    let count = names.len();
    names.sort();
    names.dedup();
    assert_eq!(count, names.len());
}
