//! Setting an integration up: its fields, where the token is kept, Save.

use agents_kitbag_core::assistant::Assistant;
use agents_kitbag_core::gh;
use agents_kitbag_core::integrations::{self, FieldKind, Integration, TOKEN, TokenSource};
use agents_kitbag_core::storage::{Available, StoreChoice};
use eframe::egui::{self, Id, Ui};

use super::kit::{self, Banner, Kind};
use super::{Form, State};
use crate::snapshot::Snapshot;
use crate::theme::{Icon, palette};
use crate::worker::Command;

pub fn field_id(id: &str) -> Id {
    Id::new(("field", id))
}

pub const INSTANCE_FIELD: &str = "instance-name";

pub fn show(ui: &mut Ui, snapshot: &Snapshot, state: &mut State, out: &mut Vec<Command>) {
    let p = palette(ui.ctx());
    let Some(integration) = state
        .form
        .as_ref()
        .and_then(|f| integrations::by_key(f.integration))
    else {
        state.form = None;
        return;
    };
    let waiting = state.form.as_ref().is_some_and(|f| f.waiting.is_some());

    if kit::icon_button(ui, Icon::Back, "Back", Kind::Secondary, !waiting).clicked() {
        state.form = None;
        return;
    }
    let Some(form) = state.form.as_mut() else {
        return;
    };
    // Each form asks afresh: the GitHub CLI may have been signed in, or
    // 1Password unlocked, since the last one.
    if !std::mem::replace(&mut form.asked_stores, true) {
        out.push(Command::LoadStores);
    }
    let title = match &form.editing {
        Some(key) => format!("Update {key}"),
        None => format!("Set up {}", integration.name),
    };
    kit::text(ui, &title, kit::semibold(kit::LG), p.text);

    kit::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        for (n, step) in integration.steps.iter().enumerate() {
            let line = if integration.steps.len() > 1 && integration.token_url.is_some() {
                format!("{}. {step}", n + 1)
            } else {
                (*step).to_owned()
            };
            kit::text(ui, &line, kit::regular(kit::SM), p.text_secondary);
        }
        if let Some(url) = integration.token_url {
            ui.add_space(4.0);
            if kit::icon_button(
                ui,
                Icon::External,
                "Open the token page",
                Kind::Secondary,
                true,
            )
            .clicked()
            {
                out.push(Command::OpenUrl(url.to_owned()));
            }
        }
    });

    if integration.is_multi() {
        kit::field_label(ui, &format!("Name of this {}", integration.noun()));
        ui.add_enabled_ui(form.editing.is_none() && !waiting, |ui| {
            kit::field(
                ui,
                field_id(INSTANCE_FIELD),
                &mut form.instance,
                "acme, client-b",
                false,
            );
        });
        if form.editing.is_none() {
            kit::hint(
                ui,
                &format!(
                    "Lowercase letters, digits and hyphens. {} sees the server under this name.",
                    snapshot.assistant.name()
                ),
            );
        }
    }

    for field in integration.fields {
        kit::field_label(ui, field.label);
        let value = form.values.entry(field.id.to_owned()).or_default();
        let secret = field.kind == FieldKind::Secret;
        ui.add_enabled_ui(!waiting, |ui| {
            ui.horizontal(|ui| {
                let toggle = if secret { 44.0 } else { 0.0 };
                ui.allocate_ui(egui::vec2(ui.available_width() - toggle, 34.0), |ui| {
                    kit::field(
                        ui,
                        field_id(field.id),
                        value,
                        field.hint,
                        secret && !form.show_token,
                    );
                });
                if secret {
                    let (icon, label) = if form.show_token {
                        (Icon::EyeOff, "Hide token")
                    } else {
                        (Icon::Eye, "Show token")
                    };
                    let toggle = ui
                        .add(egui::Button::image(icon.image(p.text_tertiary, 15.0)).frame(false))
                        .on_hover_text(label);
                    toggle.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
                    });
                    if toggle.clicked() {
                        form.show_token = !form.show_token;
                    }
                }
            });
        });
    }

    let mut ready = true;
    if matches!(integration.token, TokenSource::GhCli { .. }) {
        ready = github_cli(ui, snapshot, out);
    }
    if integration.token != TokenSource::None && ready {
        match &snapshot.stores {
            None => {
                kit::hint(ui, "Looking for credential stores...");
                ready = false;
            }
            Some(available) => stores(ui, integration, available, form, snapshot, !waiting),
        }
    }

    if ready {
        assistants(ui, form, snapshot, !waiting);
    }

    if let Some(error) = &form.error {
        kit::banner(ui, Banner::Bad, error, None);
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let label = if waiting {
            "Saving..."
        } else if integration.fields.is_empty() {
            "Connect"
        } else {
            "Save"
        };
        if kit::button(
            ui,
            label,
            Kind::Primary,
            ready && !waiting && snapshot.config_error.is_none(),
        )
        .clicked()
        {
            let id = state.next_save;
            state.next_save += 1;
            let form = state.form.as_mut().expect("the form being drawn");
            let store = chosen(form, snapshot.stores.as_ref(), integration);
            form.waiting = Some(id);
            form.error = None;
            out.push(Command::Save {
                id,
                integration: integration.key,
                instance: integration.is_multi().then(|| form.instance.clone()),
                values: form.values.clone(),
                store,
                also: form.also.clone(),
            });
        }
        if kit::button(ui, "Cancel", Kind::Secondary, !waiting).clicked() {
            state.form = None;
        }
    });
}

/// "Set up for": the assistant in view, and any other on this machine that
/// should get the same server in the same Save.
fn assistants(ui: &mut Ui, form: &mut Form, snapshot: &Snapshot, enabled: bool) {
    let others: Vec<Assistant> = snapshot
        .installed
        .iter()
        .copied()
        .filter(|assistant| *assistant != snapshot.assistant)
        .collect();
    if others.is_empty() {
        return;
    }
    ui.add_space(4.0);
    kit::field_label(ui, "Set up for");
    ui.add_enabled_ui(enabled, |ui| {
        // One row: the form is long enough already.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            let mut always = true;
            ui.add_enabled(
                false,
                egui::Checkbox::new(&mut always, snapshot.assistant.name()),
            );
            for other in others {
                let mut on = form.also.contains(&other);
                if ui.checkbox(&mut on, other.name()).changed() {
                    form.also.retain(|a| *a != other);
                    if on {
                        form.also.push(other);
                    }
                }
            }
        });
    });
    if !form.also.is_empty() {
        kit::hint(
            ui,
            "Each assistant gets its own copy of the token, so one can be removed without the others.",
        );
    }
}

fn is_gh_source(integration: &Integration) -> bool {
    matches!(integration.token, TokenSource::GhCli { .. })
}

/// The store this form saves to: the person's pick, else the best available.
fn chosen(form: &Form, available: Option<&Available>, integration: &Integration) -> StoreChoice {
    form.store.clone().unwrap_or_else(|| {
        available.map_or(StoreChoice::Plain, |a| {
            a.default_choice(is_gh_source(integration))
        })
    })
}

/// "Keep the token in": only the stores that work on this machine.
fn stores(
    ui: &mut Ui,
    integration: &Integration,
    available: &Available,
    form: &mut Form,
    snapshot: &Snapshot,
    enabled: bool,
) {
    let p = palette(ui.ctx());
    let mut choice = chosen(form, Some(available), integration);
    let before = choice.clone();
    ui.add_space(4.0);
    kit::field_label(ui, "Keep the token in");
    ui.add_enabled_ui(enabled, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        if is_gh_source(integration) && available.gh {
            ui.radio_value(
                &mut choice,
                StoreChoice::Gh,
                "GitHub CLI (no second copy is stored)",
            );
        }
        // The runner fetches a stored token at launch; without it only
        // 1Password and the config file are left.
        let runner_ok = snapshot
            .runner_warning
            .as_ref()
            .is_none_or(|w| !w.contains("missing"));
        if let Some(system) = available.system.filter(|_| runner_ok) {
            ui.radio_value(&mut choice, StoreChoice::System, system.label());
        }
        if let Some(vaults) = &available.vaults {
            let vault = match &choice {
                StoreChoice::OnePassword { vault } => vault.clone(),
                _ => available.default_vault().unwrap_or("Private").to_owned(),
            };
            let selected = matches!(choice, StoreChoice::OnePassword { .. });
            if ui.radio(selected, "1Password").clicked() {
                choice = StoreChoice::OnePassword {
                    vault: vault.clone(),
                };
            }
            if selected && vaults.len() > 1 {
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    kit::text(ui, "Vault", kit::regular(kit::SM), p.text_tertiary);
                    let mut picked = vault.clone();
                    egui::ComboBox::from_id_salt("vault")
                        .selected_text(picked.clone())
                        .show_ui(ui, |ui| {
                            for name in vaults {
                                ui.selectable_value(&mut picked, name.clone(), name);
                            }
                        });
                    if picked != vault {
                        choice = StoreChoice::OnePassword { vault: picked };
                    }
                });
            }
        }
        ui.radio_value(&mut choice, StoreChoice::Plain, "Config file (plain text)");
    });
    if choice != before {
        form.store = Some(choice.clone());
    }
    if choice == StoreChoice::Plain {
        kit::hint(
            ui,
            "Anyone who can read the config file, or a backup of it, can read the token.",
        );
    }
    if available.op_unreachable {
        kit::hint(
            ui,
            "1Password is installed but its CLI cannot reach your vaults. Unlock it, and turn on Settings, Developer, Integrate with 1Password CLI.",
        );
    }
    if let Some(warning) = &snapshot.runner_warning {
        kit::banner(ui, Banner::Warn, warning, None);
    }
}

/// GitHub's token is the GitHub CLI's. Returns whether it is ready to use.
fn github_cli(ui: &mut Ui, snapshot: &Snapshot, out: &mut Vec<Command>) -> bool {
    let busy = snapshot.busy.is_some();
    match snapshot.gh {
        None => {
            kit::hint(ui, "Looking for the GitHub CLI...");
            false
        }
        Some(gh::State::Ready) => {
            kit::banner(
                ui,
                Banner::Good,
                "The GitHub CLI is installed and signed in.",
                None,
            );
            true
        }
        Some(gh::State::NotInstalled) => {
            if kit::banner(
                ui,
                Banner::Warn,
                "The GitHub CLI (gh) is not installed.",
                Some("Install it"),
            ) && !busy
            {
                out.push(Command::InstallGh);
            }
            false
        }
        Some(gh::State::NotSignedIn) => {
            if kit::banner(
                ui,
                Banner::Warn,
                "The GitHub CLI is not signed in.",
                Some("Sign in"),
            ) {
                out.push(Command::GhLogin);
            }
            if kit::icon_button(ui, Icon::Refresh, "Check again", Kind::Secondary, !busy).clicked()
            {
                out.push(Command::LoadStores);
            }
            false
        }
    }
}

/// The id of the token field, for the tests.
#[allow(dead_code, reason = "used by the tests")]
pub fn token_field() -> Id {
    field_id(TOKEN)
}
