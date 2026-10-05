//! The config file, its backups, the runner, updates and logs.

use eframe::egui::Ui;
use toolkit_core::platform::Os;

use super::kit::{self, Kind};
use super::{Confirm, State};
use crate::snapshot::Snapshot;
use crate::theme::palette;
use crate::worker::Command;

fn file_manager(os: Os) -> &'static str {
    match os {
        Os::Mac => "Show in Finder",
        Os::Windows => "Show in Explorer",
        Os::Linux => "Show in Files",
    }
}

pub fn show(ui: &mut Ui, snapshot: &Snapshot, state: &mut State, out: &mut Vec<Command>) {
    let p = palette(ui.ctx());
    kit::text(ui, "Settings", kit::semibold(kit::LG), p.text);

    kit::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        kit::section(ui, "Claude Desktop config");
        kit::text(
            ui,
            &snapshot.config_path,
            kit::regular(kit::XS),
            p.text_secondary,
        );
        if kit::button(ui, file_manager(snapshot.os), Kind::Secondary, true).clicked() {
            out.push(Command::RevealConfig);
        }
    });

    kit::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        kit::section(ui, "Config backups");
        let count = match snapshot.backups {
            0 => "No backups.".to_owned(),
            1 => "1 backup.".to_owned(),
            n => format!("{n} backups."),
        };
        kit::text(
            ui,
            &format!(
                "{count} The config is copied before each change, and a copy holds whatever the config held: a token stored in plain text stays readable there after it is replaced."
            ),
            kit::regular(kit::SM),
            p.text_secondary,
        );
        if state.confirm == Some(Confirm::Purge) {
            kit::text(
                ui,
                "Delete every backup? This cannot be undone.",
                kit::regular(kit::SM),
                p.text,
            );
            ui.horizontal(|ui| {
                if kit::button(ui, "Delete all backups", Kind::Danger, true).clicked() {
                    out.push(Command::PurgeBackups);
                    state.confirm = None;
                }
                if kit::button(ui, "Keep", Kind::Secondary, true).clicked() {
                    state.confirm = None;
                }
            });
        } else if kit::button(ui, "Delete backups...", Kind::Danger, snapshot.backups > 0).clicked()
        {
            state.confirm = Some(Confirm::Purge);
        }
    });

    kit::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        kit::section(ui, "Token runner");
        kit::text(
            ui,
            "A server whose token is in a credential store is started through this program, which fetches the token at launch.",
            kit::regular(kit::SM),
            p.text_secondary,
        );
        kit::text(
            ui,
            &snapshot.runner_path,
            kit::regular(kit::XS),
            p.text_secondary,
        );
        if let Some(warning) = &snapshot.runner_warning {
            kit::banner(ui, kit::Banner::Warn, warning, None);
        }
    });

    kit::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        kit::section(ui, "About");
        kit::text(
            ui,
            &format!("Claude Toolkit {}", env!("CARGO_PKG_VERSION")),
            kit::regular(kit::SM),
            p.text_secondary,
        );
        let update = match &state.update {
            Some(offer) if offer.downloading => format!("Downloading {}...", offer.version),
            Some(offer) if offer.failed => format!("Updating to {} failed.", offer.version),
            Some(offer) => format!("Version {} is available.", offer.version),
            None => "No update is waiting.".to_owned(),
        };
        kit::text(ui, &update, kit::regular(kit::SM), p.text_secondary);
        if let Some(log_dir) = &snapshot.log_dir {
            kit::text(
                ui,
                &format!("Logs: {log_dir}"),
                kit::regular(kit::XS),
                p.text_tertiary,
            );
            if kit::button(ui, "Open the log folder", Kind::Secondary, true).clicked() {
                out.push(Command::RevealLogs);
            }
        }
    });
}
