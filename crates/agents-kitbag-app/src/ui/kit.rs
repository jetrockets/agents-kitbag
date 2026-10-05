//! The window's building blocks: text, buttons, fields, cards, banners.

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Id, Margin, Response, RichText, Stroke, Ui, vec2,
};

use crate::snapshot::Tone;
use crate::theme::{self, Icon, Palette, palette};

pub use crate::theme::{BASE, LG, SM, XS, medium, regular, semibold};

/// Wrapping text in one font and colour.
pub fn text(ui: &mut Ui, text: &str, font: egui::FontId, color: Color32) -> Response {
    ui.add(egui::Label::new(RichText::new(text).font(font).color(color)).wrap())
}

/// Text on one line, however narrow the column: a row's label.
pub fn label(ui: &mut Ui, text: &str, font: egui::FontId, color: Color32) -> Response {
    ui.add(egui::Label::new(RichText::new(text).font(font).color(color)).extend())
}

/// Small grey text under or beside something.
pub fn hint(ui: &mut Ui, text_: &str) -> Response {
    let color = palette(ui.ctx()).text_tertiary;
    text(ui, text_, regular(XS), color)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// The one thing this screen is for.
    Primary,
    Secondary,
    /// Deleting.
    Danger,
}

fn colors(p: &Palette, kind: Kind) -> (Color32, Color32, Color32, Color32) {
    match kind {
        Kind::Primary => (p.accent, p.accent_hover, p.accent, p.on_accent),
        Kind::Secondary => (p.card_bg, p.hover_bg, p.card_border, p.text),
        Kind::Danger => (p.card_bg, p.bad_bg, p.card_border, p.bad),
    }
}

fn styled(
    ui: &mut Ui,
    kind: Kind,
    enabled: bool,
    button: impl FnOnce(Color32) -> egui::Button<'static>,
) -> Response {
    let p = palette(ui.ctx());
    let (fill, hover, stroke, color) = colors(p, kind);
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for (state, fill) in [
            (&mut widgets.inactive, fill),
            (&mut widgets.hovered, hover),
            (&mut widgets.active, hover),
        ] {
            state.weak_bg_fill = fill;
            state.bg_fill = fill;
            state.bg_stroke = Stroke::new(1.0, stroke);
            state.fg_stroke = Stroke::new(1.0, color);
        }
        let response = ui.add_enabled(
            enabled,
            button(color)
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .min_size(vec2(0.0, 32.0)),
        );
        if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        }
    })
    .inner
}

pub fn button(ui: &mut Ui, label: &str, kind: Kind, enabled: bool) -> Response {
    let label = label.to_owned();
    styled(ui, kind, enabled, |color| {
        egui::Button::new(RichText::new(label).font(medium(SM)).color(color))
    })
}

pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, kind: Kind, enabled: bool) -> Response {
    let label = label.to_owned();
    styled(ui, kind, enabled, |color| {
        egui::Button::image_and_text(
            icon.image(color, 14.0),
            RichText::new(label).font(medium(SM)).color(color),
        )
    })
}

/// A single-line field filling the width.
pub fn field(ui: &mut Ui, id: Id, value: &mut String, hint: &str, password: bool) -> Response {
    let p = palette(ui.ctx());
    ui.add(
        egui::TextEdit::singleline(value)
            .id(id)
            .font(regular(SM))
            .text_color(p.text)
            .hint_text(RichText::new(hint).color(p.text_tertiary))
            .password(password)
            .margin(Margin::symmetric(10, 8))
            .desired_width(f32::INFINITY),
    )
}

/// A bordered box across the width.
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let p = palette(ui.ctx());
    Frame::new()
        .fill(p.card_bg)
        .stroke(Stroke::new(1.0, p.card_border))
        .corner_radius(10)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        })
        .inner
}

#[derive(Clone, Copy, PartialEq)]
pub enum Banner {
    Good,
    Warn,
    Bad,
}

impl From<Tone> for Banner {
    fn from(tone: Tone) -> Self {
        match tone {
            Tone::Good => Banner::Good,
            Tone::Bad => Banner::Bad,
        }
    }
}

/// A coloured line across the width, with an optional button at its end.
/// Returns whether the button was clicked.
pub fn banner(ui: &mut Ui, kind: Banner, message: &str, action: Option<&str>) -> bool {
    let p = palette(ui.ctx());
    let (bg, fg, icon) = match kind {
        Banner::Good => (p.good_bg, p.good, Icon::Check),
        Banner::Warn => (p.warn_bg, p.warn, Icon::Alert),
        Banner::Bad => (p.bad_bg, p.bad, Icon::Failed),
    };
    let mut clicked = false;
    Frame::new()
        .fill(bg)
        .corner_radius(8)
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(icon.image(fg, 16.0));
                let reserved = action.map_or(0.0, |label| label.len() as f32 * 7.5 + 40.0);
                ui.allocate_ui_with_layout(
                    vec2((ui.available_width() - reserved).max(80.0), 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| text(ui, message, regular(SM), p.text),
                );
                if let Some(label) = action {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        clicked = button(ui, label, Kind::Secondary, true).clicked();
                    });
                }
            });
        });
    clicked
}

/// A heading above a group of settings or fields.
pub fn section(ui: &mut Ui, title: &str) {
    let color = palette(ui.ctx()).text;
    text(ui, title, medium(BASE), color);
}

/// A field's label.
pub fn field_label(ui: &mut Ui, label: &str) {
    let color = palette(ui.ctx()).text_secondary;
    text(ui, label, medium(SM), color);
}
