//! One integration: its configured servers, and the way to add one.

use eframe::egui::{self, Ui};
use toolkit_core::health::Status;
use toolkit_core::integrations::{self, Integration};

use super::kit::{self, Banner, Kind};
use super::{Confirm, Form, State};
use crate::snapshot::{HealthView, InstanceView, Snapshot};
use crate::theme::{Icon, palette};
use crate::worker::Command;

/// The words for "add one more" of this integration.
pub fn add_label(integration: &Integration) -> String {
    format!("Add {}", integration.noun())
}

pub fn show(ui: &mut Ui, snapshot: &Snapshot, state: &mut State, out: &mut Vec<Command>) {
    let p = palette(ui.ctx());
    let Some(integration) = integrations::by_key(state.selected) else {
        return;
    };
    let instances: &[InstanceView] = snapshot
        .integration(integration.key)
        .map_or(&[], |view| &view.instances);

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            kit::text(ui, integration.name, kit::semibold(kit::LG), p.text);
            let summary = match (integration.is_multi(), instances.len()) {
                (_, 0) => "Not configured".to_owned(),
                (false, _) => "Configured".to_owned(),
                (true, 1) => format!("1 {}", integration.noun()),
                (true, n) => format!("{n} {}s", integration.noun()),
            };
            kit::text(ui, &summary, kit::regular(kit::SM), p.text_tertiary);
        });
        if integration.is_multi() && !instances.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, Icon::Plus, &add_label(integration), Kind::Primary, true)
                    .clicked()
                {
                    state.form = Some(Form::new(integration));
                    state.confirm = None;
                }
            });
        }
    });
    ui.add_space(4.0);

    if let Some(missing) = snapshot
        .integration(integration.key)
        .and_then(|v| v.missing)
    {
        let message = format!(
            "{} was not found in the usual places. {} servers need {} to start.",
            missing.program, integration.name, missing.needs
        );
        if kit::banner(ui, Banner::Warn, &message, Some("Get it")) {
            out.push(Command::OpenUrl(missing.url.to_owned()));
        }
    }

    if integration.key == "github"
        && snapshot.gh_lacks_project_scope
        && kit::banner(
            ui,
            Banner::Warn,
            "The GitHub CLI's token has no 'project' scope, so the Projects tools stay hidden.",
            Some("Add the scope"),
        )
    {
        out.push(Command::GhAddProjectScope);
    }

    if instances.is_empty() {
        empty(ui, integration, state);
        return;
    }
    for instance in instances {
        card(ui, integration, instance, state, out);
    }
}

/// Nothing configured: what setting it up involves, and the button.
fn empty(ui: &mut Ui, integration: &Integration, state: &mut State) {
    let p = palette(ui.ctx());
    kit::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        kit::text(
            ui,
            &format!("Connect {} to Claude Desktop", integration.name),
            kit::medium(kit::BASE),
            p.text,
        );
        for step in integration.steps {
            kit::text(ui, step, kit::regular(kit::SM), p.text_secondary);
        }
        ui.add_space(4.0);
        let label = if integration.is_multi() {
            add_label(integration)
        } else {
            format!("Set up {}", integration.name)
        };
        if kit::button(ui, &label, Kind::Primary, true).clicked() {
            state.form = Some(Form::new(integration));
        }
    });
}

/// The words and colour for a server's health.
pub fn health_text(health: &HealthView) -> (String, Banner, Icon) {
    match health {
        HealthView::Unknown => ("Not checked yet".to_owned(), Banner::Warn, Icon::Clock),
        HealthView::Checking => ("Checking...".to_owned(), Banner::Warn, Icon::Clock),
        HealthView::Known(health) => {
            let detail = health.detail.clone().unwrap_or_default();
            match health.status {
                Status::Ok => ("Token works".to_owned(), Banner::Good, Icon::Check),
                Status::Expired => (
                    format!("Token expired or revoked ({detail})"),
                    Banner::Bad,
                    Icon::Failed,
                ),
                Status::Error => (
                    format!("Could not check: {detail}"),
                    Banner::Warn,
                    Icon::Alert,
                ),
                Status::Skip => (detail, Banner::Warn, Icon::Info),
            }
        }
    }
}

fn card(
    ui: &mut Ui,
    integration: &Integration,
    instance: &InstanceView,
    state: &mut State,
    out: &mut Vec<Command>,
) {
    let p = palette(ui.ctx());
    kit::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        kit::text(ui, &instance.key, kit::medium(kit::BASE), p.text);

        let (words, tone, icon) = health_text(&instance.health);
        let neutral = matches!(&instance.health, HealthView::Unknown | HealthView::Checking)
            || matches!(&instance.health, HealthView::Known(h) if h.status == Status::Skip);
        let color = match tone {
            _ if neutral => p.text_tertiary,
            Banner::Good => p.good,
            Banner::Warn => p.warn,
            Banner::Bad => p.bad,
        };
        ui.horizontal(|ui| {
            if instance.health == HealthView::Checking {
                ui.add(egui::Spinner::new().size(14.0));
            } else {
                ui.add(icon.image(color, 15.0));
            }
            kit::text(ui, &words, kit::regular(kit::SM), color);
        });

        let rows = instance
            .rows
            .iter()
            .map(|(label, value)| (label.as_str(), value.as_str()));
        let location = instance
            .location
            .as_deref()
            .map(|location| ("Token kept in", location));
        egui::Grid::new(("rows", &instance.key))
            .num_columns(2)
            .spacing([16.0, 6.0])
            .show(ui, |ui| {
                for (label, value) in rows.chain(location) {
                    kit::label(ui, label, kit::regular(kit::SM), p.text_tertiary);
                    kit::text(ui, value, kit::regular(kit::SM), p.text);
                    ui.end_row();
                }
            });

        ui.add_space(2.0);
        let deleting = state.confirm == Some(Confirm::Delete(instance.key.clone()));
        if deleting {
            kit::text(
                ui,
                &format!(
                    "Remove {} from Claude Desktop? A token kept in a credential store stays there.",
                    instance.key
                ),
                kit::regular(kit::SM),
                p.text_secondary,
            );
            ui.horizontal(|ui| {
                if kit::button(ui, &format!("Delete {}", instance.key), Kind::Danger, true)
                    .clicked()
                {
                    out.push(Command::Delete(vec![instance.key.clone()]));
                    state.confirm = None;
                }
                if kit::button(ui, "Keep", Kind::Secondary, true).clicked() {
                    state.confirm = None;
                }
            });
            return;
        }
        ui.horizontal(|ui| {
            let checking = instance.health == HealthView::Checking;
            if kit::icon_button(ui, Icon::Refresh, "Check", Kind::Secondary, !checking).clicked() {
                out.push(Command::Check(instance.key.clone()));
            }
            let edit = if integration.has_token_field() {
                "Replace token"
            } else if integration.fields.is_empty() {
                "Reconnect"
            } else {
                "Edit"
            };
            if kit::icon_button(ui, Icon::Pencil, edit, Kind::Secondary, true).clicked() {
                state.form = Some(Form::edit(integration, &instance.key, &instance.prefill));
            }
            if kit::icon_button(ui, Icon::Trash, "Delete", Kind::Danger, true).clicked() {
                state.confirm = Some(Confirm::Delete(instance.key.clone()));
            }
        });
    });
}
