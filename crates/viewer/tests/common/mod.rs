use viewer::theme::Rgb;

/// WCAG 2 contrast ratio between two opaque colors.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let lum = |c: Rgb| {
        let ch = |v: u8| {
            let s = v as f64 / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

pub fn composite(background: Rgb, overlay: egui::Color32) -> Rgb {
    let color = viewer::color::rgb(background).blend(overlay);
    Rgb(color.r(), color.g(), color.b())
}
