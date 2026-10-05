//! The window: a sidebar of integrations and, beside it, the selected one's
//! servers, the form that sets one up, or the settings.
//!
//! Views draw a [`Snapshot`] and return [`Command`]s. They never wait.

use eframe::egui::{self, Margin, Stroke, Ui, UiBuilder, vec2};
use toolkit_core::claude;
use toolkit_core::integrations::{self, Integration, Values};
use toolkit_core::platform::Os;
use toolkit_core::storage::StoreChoice;

use crate::snapshot::Snapshot;
use crate::theme::palette;
use crate::updates::{Checked, Offer};
use crate::worker::Command;

mod detail;
mod form;
pub mod kit;
mod settings;
mod sidebar;
#[cfg(test)]
mod tests;

use kit::{Banner, Kind};

const SIDEBAR: f32 = 232.0;
/// Text is easier to read in a column than across a wide window.
const COLUMN: f32 = 660.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Integration,
    Settings,
}

/// Something that is only done after a second click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    Delete(String),
    Purge,
}

/// The setup form of one integration.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub integration: &'static str,
    /// The config key being edited; `None` for a new server.
    pub editing: Option<String>,
    /// The new instance's name, for a multi-instance integration.
    pub instance: String,
    pub values: Values,
    /// `None` until the person picks one: the best available is used.
    pub store: Option<StoreChoice>,
    pub show_token: bool,
    /// Whether this form has asked the worker which stores work here.
    pub asked_stores: bool,
    /// The id of the Save this form waits on.
    pub waiting: Option<u64>,
    pub error: Option<String>,
}

impl Form {
    fn new(integration: &Integration) -> Self {
        Self {
            integration: integration.key,
            editing: None,
            instance: String::new(),
            values: Values::new(),
            store: None,
            show_token: false,
            asked_stores: false,
            waiting: None,
            error: None,
        }
    }

    fn edit(integration: &Integration, key: &str, prefill: &Values) -> Self {
        Self {
            editing: Some(key.to_owned()),
            instance: integration.instance_name(key).to_owned(),
            values: prefill.clone(),
            ..Self::new(integration)
        }
    }
}

pub struct State {
    /// The selected integration's key.
    pub selected: &'static str,
    pub screen: Screen,
    pub form: Option<Form>,
    pub confirm: Option<Confirm>,
    next_save: u64,
    /// A newer release, set by the window each frame.
    pub update: Option<Offer>,
    update_requested: bool,
    /// How the last look for a newer release ended, set by the window.
    pub update_check: Checked,
    /// Whether "Check for updates" can be clicked now.
    pub can_check_updates: bool,
    update_check_requested: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            selected: integrations::ALL[0].key,
            screen: Screen::Integration,
            form: None,
            confirm: None,
            next_save: 1,
            update: None,
            update_requested: false,
            update_check: Checked::Never,
            can_check_updates: false,
            update_check_requested: false,
        }
    }
}

impl State {
    /// Whether "Update" was clicked since the last call.
    pub fn take_update_request(&mut self) -> bool {
        std::mem::take(&mut self.update_requested)
    }

    /// Whether "Check for updates" was clicked since the last call.
    pub fn take_update_check_request(&mut self) -> bool {
        std::mem::take(&mut self.update_check_requested)
    }

    /// Opens the selected integration's setup form.
    pub fn open_form(&mut self) {
        if let Some(integration) = integrations::by_key(self.selected) {
            self.form = Some(Form::new(integration));
        }
    }

    fn select(&mut self, key: &'static str) {
        self.selected = key;
        self.screen = Screen::Integration;
        self.form = None;
        self.confirm = None;
    }

    /// Takes in how a Save this form sent has ended.
    fn absorb(&mut self, snapshot: &Snapshot) {
        let Some(form) = &mut self.form else { return };
        let (Some(waiting), Some(outcome)) = (form.waiting, &snapshot.save) else {
            return;
        };
        if outcome.id != waiting {
            return;
        }
        match &outcome.result {
            Ok(_) => self.form = None,
            Err(error) => {
                form.error = Some(error.clone());
                form.waiting = None;
            }
        }
    }
}

pub fn show(ui: &mut Ui, snapshot: &Snapshot, state: &mut State) -> Vec<Command> {
    let mut out = Vec::new();
    state.absorb(snapshot);

    let p = palette(ui.ctx());
    let full = ui.max_rect();
    let (side, main) = full.split_left_right_at_x(full.left() + SIDEBAR.min(full.width() * 0.4));
    ui.painter().rect_filled(side, 0, p.sidebar_bg);
    ui.painter().rect_filled(main, 0, p.bg);
    ui.painter()
        .vline(side.right(), side.y_range(), Stroke::new(1.0, p.border));

    ui.scope_builder(
        UiBuilder::new().max_rect(side.shrink2(vec2(10.0, 14.0))),
        |ui| {
            sidebar::show(ui, snapshot, state, &mut out);
        },
    );

    let footer_height = footer_height(snapshot);
    let (content, footer) = main.split_top_bottom_at_y(main.bottom() - footer_height);
    ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
        ui.set_clip_rect(content);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(28, 24))
                    .show(ui, |ui| {
                        ui.set_max_width(COLUMN.min(ui.available_width()));
                        ui.spacing_mut().item_spacing.y = 10.0;
                        banners(ui, snapshot, &mut out);
                        match (state.screen, state.form.is_some()) {
                            (Screen::Settings, _) => settings::show(ui, snapshot, state, &mut out),
                            (Screen::Integration, true) => {
                                form::show(ui, snapshot, state, &mut out)
                            }
                            (Screen::Integration, false) => {
                                detail::show(ui, snapshot, state, &mut out)
                            }
                        }
                    });
            });
    });
    if footer_height > 0.0 {
        ui.painter().rect_filled(footer, 0, p.sidebar_bg);
        ui.painter()
            .hline(footer.x_range(), footer.top(), Stroke::new(1.0, p.border));
        ui.scope_builder(
            UiBuilder::new().max_rect(footer.shrink2(vec2(28.0, 10.0))),
            |ui| self::footer(ui, snapshot, &mut out),
        );
    }
    out
}

fn footer_height(snapshot: &Snapshot) -> f32 {
    match (snapshot.needs_restart, snapshot.os, &snapshot.busy) {
        (true, Os::Windows | Os::Linux, _) => 112.0,
        (true, Os::Mac, _) | (false, _, Some(_)) => 54.0,
        (false, _, None) => 0.0,
    }
}

/// What the worker is doing, or the reminder that Claude Desktop only reads
/// its config when it starts.
fn footer(ui: &mut Ui, snapshot: &Snapshot, out: &mut Vec<Command>) {
    let p = palette(ui.ctx());
    if let Some(busy) = &snapshot.busy {
        ui.horizontal_centered(|ui| {
            ui.spinner();
            kit::text(ui, busy, kit::regular(kit::SM), p.text_secondary);
        });
        return;
    }
    match snapshot.os {
        Os::Mac => {
            ui.horizontal_centered(|ui| {
                kit::text(
                    ui,
                    "Restart Claude Desktop to load the changes.",
                    kit::regular(kit::SM),
                    p.text_secondary,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::button(ui, "Restart Claude", Kind::Primary, true).clicked() {
                        out.push(Command::RestartClaude);
                    }
                });
            });
        }
        Os::Windows | Os::Linux => {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    kit::text(ui, "To load the changes:", kit::medium(kit::SM), p.text);
                    for (n, step) in claude::MANUAL_STEPS.iter().enumerate() {
                        kit::text(
                            ui,
                            &format!("{}. {step}", n + 1),
                            kit::regular(kit::XS),
                            p.text_secondary,
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::button(ui, "Done", Kind::Secondary, true).clicked() {
                        out.push(Command::RestartDone);
                    }
                });
            });
        }
    }
}

/// What is wrong or just happened, above every screen.
fn banners(ui: &mut Ui, snapshot: &Snapshot, out: &mut Vec<Command>) {
    if let Some(error) = &snapshot.config_error {
        let message = format!(
            "{error}. Nothing is written until it is fixed: {}",
            snapshot.config_path
        );
        if kit::banner(ui, Banner::Bad, &message, Some("Show file")) {
            out.push(Command::RevealConfig);
        }
    }
    if !snapshot.migration.is_empty() {
        let count = snapshot.migration.len();
        let message = format!(
            "{count} server{} ({}) still start{} through the old Node runner. Move {} onto the built-in one.",
            if count == 1 { "" } else { "s" },
            snapshot.migration.join(", "),
            if count == 1 { "s" } else { "" },
            if count == 1 { "it" } else { "them" },
        );
        if kit::banner(ui, Banner::Warn, &message, Some("Migrate")) {
            out.push(Command::Migrate);
        }
    }
    if let Some(notice) = &snapshot.notice
        && kit::banner(ui, notice.tone.into(), &notice.text, Some("Dismiss"))
    {
        out.push(Command::DismissNotice);
    }
}
