//! The MCP servers section's list: the assistant they belong to, and each
//! integration with a mark for the state of its tokens.

use agents_kitbag_core::health::Status;
use agents_kitbag_core::integrations;
use eframe::egui::{self, Align2, Color32, Id, Response, Sense, Stroke, Ui, vec2};

use super::kit::{self, Kind};
use super::{Screen, Section, State};
use crate::snapshot::{HealthView, IntegrationView, Snapshot};
use crate::theme::{Icon, palette};
use crate::worker::Command;

/// What the dot beside an integration says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    NotConfigured,
    /// Configured, not checked yet or being checked.
    Pending,
    Good,
    /// A check could not be made.
    Warn,
    /// A token was refused.
    Bad,
}

pub fn mark(view: &IntegrationView) -> Mark {
    if view.instances.is_empty() {
        return Mark::NotConfigured;
    }
    let status = |wanted: Status| {
        view.instances
            .iter()
            .any(|i| matches!(&i.health, HealthView::Known(h) if h.status == wanted))
    };
    let pending = view
        .instances
        .iter()
        .any(|i| !matches!(i.health, HealthView::Known(_)));
    if status(Status::Expired) {
        Mark::Bad
    } else if status(Status::Error) {
        Mark::Warn
    } else if pending {
        Mark::Pending
    } else {
        Mark::Good
    }
}

impl Mark {
    /// The mark in words, for a screen reader and the tests.
    pub fn describe(self) -> &'static str {
        match self {
            Mark::NotConfigured => "not configured",
            Mark::Pending => "checking",
            Mark::Good => "working",
            Mark::Warn => "could not be checked",
            Mark::Bad => "token refused",
        }
    }
}

const ROW: f32 = 34.0;

fn row(
    ui: &mut Ui,
    id: Id,
    label: &str,
    described: &str,
    selected: bool,
) -> (egui::Rect, Response) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let described = described.to_owned();
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
    ui.painter().rect_filled(rect, 6, fill);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            6,
            Stroke::new(1.5, p.accent),
            egui::StrokeKind::Inside,
        );
    }
    let color = if selected { p.text } else { p.text_secondary };
    ui.painter().text(
        rect.left_center() + vec2(30.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        if selected {
            kit::medium(kit::SM)
        } else {
            kit::regular(kit::SM)
        },
        color,
    );
    (rect, response)
}

pub fn show(ui: &mut Ui, snapshot: &Snapshot, state: &mut State, out: &mut Vec<Command>) {
    let p = palette(ui.ctx());
    ui.spacing_mut().item_spacing.y = 2.0;

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.vertical(|ui| {
            kit::text(ui, "Claude Desktop", kit::semibold(kit::BASE), p.text);
            kit::text(
                ui,
                Section::Servers.name(),
                kit::regular(kit::XS),
                p.text_tertiary,
            );
        });
    });
    ui.add_space(14.0);

    for integration in integrations::ALL {
        let view = snapshot.integration(integration.key);
        let mark = view.map_or(Mark::NotConfigured, mark);
        let count = view.map_or(0, |v| v.instances.len());
        let selected =
            state.screen == Screen::Section(Section::Servers) && state.selected == integration.key;
        let (rect, response) = row(
            ui,
            Id::new(("integration", integration.key)),
            integration.name,
            &format!("{}, {}", integration.name, mark.describe()),
            selected,
        );
        let dot = rect.left_center() + vec2(15.0, 0.0);
        match mark {
            Mark::NotConfigured => {
                ui.painter()
                    .circle_stroke(dot, 4.0, Stroke::new(1.5, p.text_tertiary));
            }
            Mark::Pending => {
                ui.painter().circle_filled(dot, 4.5, p.text_tertiary);
            }
            Mark::Good => {
                ui.painter().circle_filled(dot, 4.5, p.good);
            }
            Mark::Warn => {
                ui.painter().circle_filled(dot, 4.5, p.warn);
            }
            Mark::Bad => {
                ui.painter().circle_filled(dot, 4.5, p.bad);
            }
        }
        if integration.is_multi() && count > 0 {
            ui.painter().text(
                rect.right_center() - vec2(10.0, 0.0),
                Align2::RIGHT_CENTER,
                count.to_string(),
                kit::regular(kit::XS),
                p.text_tertiary,
            );
        }
        if response.clicked() {
            state.select(integration.key);
        }
    }

    // The actions sit at the bottom of the window.
    let actions = ROW + 2.0 + state.update.as_ref().map_or(0.0, |_| 84.0) + 12.0;
    let gap = (ui.available_height() - actions).max(12.0);
    ui.add_space(gap);

    if let Some(offer) = state.update.clone() {
        kit::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let message = if offer.failed {
                format!("Update to {} failed", offer.version)
            } else if offer.downloading {
                format!("Downloading {}...", offer.version)
            } else {
                format!("Version {} is available", offer.version)
            };
            kit::text(ui, &message, kit::regular(kit::XS), p.text_secondary);
            let label = if offer.failed {
                "Try again"
            } else {
                "Restart to update"
            };
            if kit::button(ui, label, Kind::Primary, !offer.downloading).clicked() {
                state.update_requested = true;
            }
        });
        ui.add_space(6.0);
    }

    let any_configured = snapshot
        .integrations
        .iter()
        .any(|i| !i.instances.is_empty());
    let (rect, response) = row(
        ui,
        Id::new("check-all"),
        "Check all tokens",
        "Check all tokens",
        false,
    );
    ui.put(
        egui::Rect::from_center_size(rect.left_center() + vec2(15.0, 0.0), vec2(14.0, 14.0)),
        Icon::Refresh.image(p.text_tertiary, 14.0),
    );
    if response.clicked() && any_configured {
        out.push(Command::CheckAll);
    }
}
