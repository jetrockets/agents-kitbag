//! The window's look: two palettes that follow the system, Inter from
//! fastframe, the desktop's own text rendering, and Lucide icons.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Theme, Visuals,
};
use fastframe_fonts::{FontSetup, Weight};

/// The colours one theme uses.
pub struct Palette {
    pub bg: Color32,
    pub sidebar_bg: Color32,
    pub card_bg: Color32,
    pub card_border: Color32,
    pub border: Color32,
    pub input_bg: Color32,
    pub hover_bg: Color32,
    pub selected_bg: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_tertiary: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub good_bg: Color32,
    pub warn_bg: Color32,
    pub bad_bg: Color32,
    pub selection: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(24, 24, 23),
    sidebar_bg: Color32::from_rgb(18, 18, 17),
    card_bg: Color32::from_rgb(32, 32, 30),
    card_border: Color32::from_rgb(52, 52, 49),
    border: Color32::from_rgb(44, 44, 41),
    input_bg: Color32::from_rgb(20, 20, 19),
    hover_bg: Color32::from_rgb(36, 36, 34),
    selected_bg: Color32::from_rgb(46, 46, 43),
    text: Color32::from_rgb(242, 241, 237),
    text_secondary: Color32::from_rgb(196, 194, 187),
    text_tertiary: Color32::from_rgb(160, 158, 151),
    accent: Color32::from_rgb(180, 83, 51),
    accent_hover: Color32::from_rgb(201, 100, 66),
    on_accent: Color32::WHITE,
    good: Color32::from_rgb(110, 196, 132),
    warn: Color32::from_rgb(230, 180, 90),
    bad: Color32::from_rgb(240, 120, 110),
    good_bg: Color32::from_rgb(28, 44, 33),
    warn_bg: Color32::from_rgb(50, 41, 24),
    bad_bg: Color32::from_rgb(56, 30, 28),
    selection: Color32::from_rgb(92, 54, 40),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(250, 249, 245),
    sidebar_bg: Color32::from_rgb(240, 238, 230),
    card_bg: Color32::WHITE,
    card_border: Color32::from_rgb(222, 219, 208),
    border: Color32::from_rgb(226, 223, 213),
    input_bg: Color32::WHITE,
    hover_bg: Color32::from_rgb(232, 229, 220),
    selected_bg: Color32::from_rgb(223, 220, 209),
    text: Color32::from_rgb(28, 27, 25),
    text_secondary: Color32::from_rgb(70, 68, 62),
    text_tertiary: Color32::from_rgb(92, 89, 81),
    accent: Color32::from_rgb(180, 83, 51),
    accent_hover: Color32::from_rgb(160, 70, 41),
    on_accent: Color32::WHITE,
    good: Color32::from_rgb(28, 122, 60),
    warn: Color32::from_rgb(150, 98, 8),
    bad: Color32::from_rgb(190, 50, 42),
    good_bg: Color32::from_rgb(228, 243, 232),
    warn_bg: Color32::from_rgb(252, 241, 214),
    bad_bg: Color32::from_rgb(252, 230, 227),
    selection: Color32::from_rgb(246, 214, 201),
};

pub fn palette(ctx: &egui::Context) -> &'static Palette {
    if ctx.theme() == Theme::Dark {
        &DARK
    } else {
        &LIGHT
    }
}

pub const RADIUS: u8 = 6;

pub fn regular(size: f32) -> FontId {
    Weight::Regular.font_id(size)
}

pub fn medium(size: f32) -> FontId {
    Weight::Medium.font_id(size)
}

pub fn semibold(size: f32) -> FontId {
    Weight::SemiBold.font_id(size)
}

pub const XS: f32 = 12.0;
pub const SM: f32 = 13.0;
pub const BASE: f32 = 14.0;
pub const LG: f32 = 20.0;

fastframe_icons::icons! {
    /// Every icon the window draws. A name that is not here does not compile.
    pub enum Icon {
        prefix: "agents-kitbag-icon-",
        directory: "../assets/icons/",
        Back => lucide "arrow-left",
        Check => lucide "circle-check",
        Alert => lucide "circle-alert",
        Failed => lucide "circle-x",
        Clock => lucide "clock",
        External => lucide "external-link",
        Eye => lucide "eye",
        EyeOff => lucide "eye-off",
        Info => lucide "info",
        Lock => lucide "lock",
        Pencil => lucide "pencil",
        Plus => lucide "plus",
        Refresh => lucide "refresh-cw",
        Settings => lucide "settings",
        Trash => lucide "trash-2",
    }
}

/// Fonts, colours and icons. `system_fonts` is off for `--demo-shot` and the
/// tests, so a picture does not depend on what the machine has installed.
pub fn install(ctx: &egui::Context, system_fonts: bool) {
    egui_extras::install_image_loaders(ctx);
    fastframe_icons::install::<Icon>(ctx);

    let rendering = fastframe_text::detect();
    let mut fonts = FontSetup::default()
        .system_fallbacks(system_fonts)
        .definitions();
    rendering.apply_to(&mut fonts);
    ctx.set_fonts(fonts);

    ctx.set_visuals_of(Theme::Dark, visuals(Visuals::dark(), &DARK));
    ctx.set_visuals_of(Theme::Light, visuals(Visuals::light(), &LIGHT));
    ctx.all_styles_mut(|style| {
        rendering.apply_to_visuals(&mut style.visuals);
        style.text_styles = [
            (TextStyle::Small, FontId::new(XS, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(SM, FontFamily::Proportional)),
            (TextStyle::Button, medium(SM)),
            (TextStyle::Monospace, FontId::new(XS, FontFamily::Monospace)),
            (TextStyle::Heading, semibold(LG)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.icon_width = 16.0;
        style.spacing.icon_spacing = 8.0;
    });
}

fn visuals(mut v: Visuals, p: &Palette) -> Visuals {
    let radius = CornerRadius::same(RADIUS);
    v.panel_fill = p.bg;
    v.window_fill = p.card_bg;
    v.extreme_bg_color = p.input_bg;
    v.faint_bg_color = p.card_bg;
    v.code_bg_color = p.hover_bg;
    v.hyperlink_color = p.accent_hover;
    v.weak_text_color = Some(p.text_tertiary);
    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(1.5, p.text);
    v.window_stroke = Stroke::new(1.0, p.card_border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = radius;
    v.text_edit_bg_color = Some(p.input_bg);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text_secondary);
    w.noninteractive.corner_radius = radius;
    for (state, fill) in [
        (&mut w.inactive, p.card_bg),
        (&mut w.hovered, p.hover_bg),
        (&mut w.active, p.selected_bg),
        (&mut w.open, p.selected_bg),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, p.card_border);
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2 contrast between two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        fn luminance(c: Color32) -> f32 {
            let channel = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.040_45 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn text_reads_against_every_background_it_sits_on() {
        for (theme, p) in [("dark", &DARK), ("light", &LIGHT)] {
            for bg in [
                p.bg,
                p.sidebar_bg,
                p.card_bg,
                p.hover_bg,
                p.selected_bg,
                p.input_bg,
            ] {
                for (name, text) in [
                    ("text", p.text),
                    ("text_secondary", p.text_secondary),
                    ("text_tertiary", p.text_tertiary),
                ] {
                    let ratio = contrast(text, bg);
                    assert!(ratio >= 4.5, "{name} {ratio:.2} on {bg:?}, {theme}");
                }
            }
            for (name, text, bg) in [
                ("good", p.good, p.good_bg),
                ("warn", p.warn, p.warn_bg),
                ("bad", p.bad, p.bad_bg),
                ("good on card", p.good, p.card_bg),
                ("bad on card", p.bad, p.card_bg),
                ("warn on card", p.warn, p.card_bg),
                ("on_accent", p.on_accent, p.accent),
            ] {
                let ratio = contrast(text, bg);
                assert!(ratio >= 4.5, "{name} {ratio:.2}, {theme}");
            }
        }
    }
}
