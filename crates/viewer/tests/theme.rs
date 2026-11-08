//! Theme parity with `love/uml/domain/theme.lua`: palette, ramps and the
//! missing-means-grade-1 rule that paints unmeasured classes red.

use crate::common;

use common::{composite, contrast};
use viewer::color;
use viewer::theme::{self, Rgb, Role};

#[test]
fn quality_ramps_match_the_lua_breakpoints() {
    // Missing metrics enter the ramps as grade 1 (red family).
    assert_eq!(Rgb(36, 52, 48), theme::fill(None));
    assert_eq!(Rgb(90, 110, 100), theme::stroke(None));
    // grade 1 -> 1/5 of the way from the red end to the midpoint
    assert_eq!(Rgb(71, 44, 24), theme::fill(Some(1.0)));
    assert_eq!(Rgb(222, 136, 77), theme::stroke(Some(1.0)));
    // grade 5 -> midpoint
    assert_eq!(Rgb(61, 58, 24), theme::fill(Some(5.0)));
    assert_eq!(Rgb(212, 192, 90), theme::stroke(Some(5.0)));
    // grade 10 -> green end
    assert_eq!(Rgb(30, 74, 56), theme::fill(Some(10.0)));
    assert_eq!(Rgb(95, 181, 138), theme::stroke(Some(10.0)));
}

#[test]
fn band_colors_are_the_traffic_light() {
    assert_eq!(Rgb(196, 42, 36), theme::band_color("red"));
    assert_eq!(Rgb(232, 196, 72), theme::band_color("yellow"));
    assert_eq!(Rgb(95, 181, 138), theme::band_color("green"));
    // Unknown bands are red, matching the Lua fallback.
    assert_eq!(Rgb(196, 42, 36), theme::band_color("bogus"));
}

#[test]
fn translucent_colors_composite_at_their_nominal_alpha() {
    let background = theme::SOURCE_BG;
    let original = Rgb(200, 214, 204);
    let dimmed = composite(background, color::fade(color::rgb(original)));
    let washed = composite(background, color::color(Role::RowWash));

    assert!(
        dimmed.0 < original.0 && dimmed.1 < original.1 && dimmed.2 < original.2,
        "dimming must darken, got {dimmed:?}"
    );
    assert!(
        washed.0 < theme::GOLD.0 && washed.1 < theme::GOLD.1 && washed.2 < theme::GOLD.2,
        "a wash must not become solid gold, got {washed:?}"
    );
    assert_eq!(
        composite(background, color::fade(color::rgb(original))),
        composite(
            background,
            egui::Color32::from_rgba_unmultiplied(
                original.0,
                original.1,
                original.2,
                theme::ALPHA_DIMMED
            )
        ),
        "fade and an equally alpha'd opaque color must composite alike"
    );
}

#[test]
fn source_window_is_dark_and_readable() {
    // The source window joins the dark theme instead of a light island.
    assert_eq!(theme::PANEL, theme::SOURCE_BG);
    for ink in [
        theme::SOURCE_INK,
        theme::SOURCE_KEYWORD,
        theme::SOURCE_STRING,
    ] {
        assert!(contrast(ink, theme::SOURCE_BG) >= 4.5, "{ink:?}");
        assert!(
            contrast(ink, theme::SOURCE_SELECTION) >= 4.5,
            "{ink:?} on selection"
        );
    }
    assert!(contrast(theme::SOURCE_COMMENT, theme::SOURCE_BG) >= 3.0);
    assert!(contrast(theme::SOURCE_GUTTER, theme::SOURCE_GUTTER_BG) >= 4.5);
}

#[test]
fn the_dependency_chain_hue_stays_clear_of_quality_and_plain_edges() {
    let distance = |a: Rgb, b: Rgb| {
        let d = |x: u8, y: u8| (x as f64 - y as f64).powi(2);
        (d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)).sqrt()
    };
    let mut others: Vec<Rgb> = (0..=10).map(|g| theme::stroke(Some(g as f64))).collect();
    others.extend(["red", "yellow", "green"].map(theme::band_color));
    others.extend([theme::EDGE, theme::GOLD, theme::WARN, theme::WHITE]);
    for other in others {
        assert!(
            distance(theme::RELATED, other) >= 100.0,
            "chain {:?} too close to {other:?}",
            theme::RELATED
        );
    }
}
