//! The eframe app: it hands the worker's snapshot to the views and their
//! commands back to the worker.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::ui::{self, Screen};
use crate::updates::Updates;
use crate::worker::{Command, Handle};

/// How long `--demo-shot` lets the window settle before the picture.
const SHOT_AFTER: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Normal,
    /// Made-up data in a throwaway folder.
    Demo,
    /// The demo, drawn once into a PNG, then quit.
    Shot {
        path: PathBuf,
        light: bool,
    },
}

/// What `--demo-shot` opens before it takes the picture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opening {
    /// The integration to select.
    pub select: Option<&'static str>,
    pub form: bool,
    pub settings: bool,
}

pub struct Window {
    worker: Handle,
    ui: ui::State,
    updates: Updates,
    mode: Mode,
    was_focused: bool,
    shot_started: Option<Instant>,
    shot_asked: bool,
}

impl Window {
    pub fn new(worker: Handle, updates: Updates, mode: Mode, opening: &Opening) -> Self {
        let mut state = ui::State::default();
        if let Some(key) = opening.select {
            state.selected = key;
        }
        if opening.form {
            state.open_form();
        }
        if opening.settings {
            state.screen = Screen::Settings;
        }
        Self {
            worker,
            ui: state,
            updates,
            mode,
            was_focused: true,
            shot_started: None,
            shot_asked: false,
        }
    }

    /// `--demo-shot`: asks for a screenshot once the window has settled,
    /// saves it and quits.
    fn shot(&mut self, ctx: &egui::Context) {
        let Mode::Shot { path, light } = &self.mode else {
            return;
        };
        ctx.set_theme(if *light {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        });
        // Settled by time, not by a count of frames: a slow machine draws
        // fewer frames in the same time.
        let started = *self.shot_started.get_or_insert_with(Instant::now);
        if !self.shot_asked && started.elapsed() >= SHOT_AFTER {
            self.shot_asked = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        for event in ctx.input(|i| i.raw.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                let saved =
                    image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, bytes)
                        .map(|picture| picture.save(path));
                if !matches!(saved, Some(Ok(()))) {
                    log::warn!("could not save {}", path.display());
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        ctx.request_repaint();
    }
}

impl eframe::App for Window {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.updates.installing() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // Claude Desktop or an editor may have changed the
        // config while the window was in the background.
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(true);
        if focused && !std::mem::replace(&mut self.was_focused, focused) {
            self.worker.send(Command::Refresh);
        }
        self.was_focused = focused;
        self.shot(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let snapshot = self.worker.snapshot();
        self.ui.update = self.updates.offer();
        self.ui.update_check = self.updates.checked();
        self.ui.can_check_updates = self.updates.can_check();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                for command in ui::show(ui, &snapshot, &mut self.ui) {
                    self.worker.send(command);
                }
            });
        if self.ui.take_update_request() {
            self.updates.install();
        }
        if self.ui.take_update_check_request() {
            self.updates.check_now();
        }
    }
}
