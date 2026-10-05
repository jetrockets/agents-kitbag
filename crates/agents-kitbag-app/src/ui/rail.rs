//! The rail: one icon for each section, and the settings at the bottom.

use eframe::egui::{self, Color32, Id, Sense, Stroke, Ui, vec2};

use super::{Screen, Section, State};
use crate::theme::{Icon, palette};

const BUTTON: f32 = 36.0;
const ICON: f32 = 18.0;

/// A square button that shows where the window is. Returns whether it was
/// clicked.
fn button(ui: &mut Ui, id: Id, icon: Icon, name: &str, selected: bool) -> bool {
    let p = palette(ui.ctx());
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), BUTTON), Sense::hover());
    let rect = egui::Rect::from_center_size(slot.center(), vec2(BUTTON, BUTTON));
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(name);
    let described = name.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &described)
    });
    let fill = if selected {
        p.selected_bg
    } else if response.hovered() {
        p.hover_bg
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8, fill);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            8,
            Stroke::new(1.5, p.accent),
            egui::StrokeKind::Inside,
        );
    }
    let color = if selected { p.text } else { p.text_tertiary };
    ui.put(
        egui::Rect::from_center_size(rect.center(), vec2(ICON, ICON)),
        icon.image(color, ICON),
    );
    response.clicked()
}

pub fn show(ui: &mut Ui, state: &mut State) {
    ui.spacing_mut().item_spacing.y = 6.0;
    for section in Section::ALL {
        let selected = state.screen == Screen::Section(*section);
        if button(
            ui,
            Id::new(("section", section.name())),
            section.icon(),
            section.name(),
            selected,
        ) {
            state.open(Screen::Section(*section));
        }
    }

    // The settings sit at the bottom of the window.
    let gap = (ui.available_height() - BUTTON).max(6.0);
    ui.add_space(gap);
    let selected = state.screen == Screen::Settings;
    if button(
        ui,
        Id::new("settings"),
        Icon::Settings,
        "Settings",
        selected,
    ) {
        state.open(Screen::Settings);
    }
}
