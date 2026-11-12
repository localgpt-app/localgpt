//! The Audio button in the bar, and the panel under it: the optional audio
//! models an agent can use (`crates/audio-models`), each installed or removed
//! here.
//!
//! The window only downloads and deletes their files, into the models
//! directory every LocalGPT program shares, so it links no model runtime.
//! The models run in the `localgpt` command an agent starts as its MCP
//! server (`localgpt world mcp`), which offers a model's tool once the model
//! is installed. A server already running lists its tools as it found them
//! at its start, so the panel says to restart it.
//!
//! This is the window's own: the panel is not part of the interface
//! protocol, because the models belong to the machine, not to a paired
//! device.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};
use localgpt_audio_models::{self as models, MODELS, Model};

use super::theme;

#[derive(Default)]
pub struct Audio {
    /// The installs running or failed, by model id.
    jobs: Arc<Mutex<HashMap<&'static str, Job>>>,
}

#[derive(Clone)]
enum Job {
    Installing { done: u64, total: u64 },
    Failed(String),
}

impl Audio {
    /// The Audio button, and its panel.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        use egui::containers::menu::{MenuButton, MenuConfig};
        if MODELS.is_empty() {
            return;
        }
        let Some(dir) = models::models_dir() else {
            return;
        };
        let installed = MODELS.iter().filter(|m| models::installed(&dir, m)).count();
        let busy = self
            .jobs
            .lock()
            .is_ok_and(|jobs| jobs.values().any(|j| matches!(j, Job::Installing { .. })));
        let label = match (busy, installed) {
            (true, _) => "Audio · installing".to_string(),
            (false, 0) => "Audio".to_string(),
            (false, n) => format!("Audio · {n}"),
        };
        let button =
            egui::Button::new(RichText::new(label).color(theme::MUTED)).frame_when_inactive(false);
        let (response, _) = MenuButton::from_button(button)
            .config(MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside))
            .ui(ui, |ui| self.panel(ui, &dir));
        response.on_hover_text("Audio models an agent can use, once installed");
    }

    fn panel(&mut self, ui: &mut egui::Ui, dir: &Path) {
        ui.set_max_width(420.0);
        ui.label(RichText::new("Audio models").strong());
        ui.label(
            RichText::new(
                "Optional. Once a model is installed, an agent's `localgpt world mcp` \
                 offers its tool; restart the agent's MCP server to see it. They run on \
                 this computer's CPU, offline.",
            )
            .color(theme::MUTED),
        );
        ui.separator();
        let jobs = self.jobs.lock().map(|j| j.clone()).unwrap_or_default();
        for model in MODELS {
            self.row(ui, dir, model, jobs.get(model.id));
            ui.add_space(6.0);
        }
        ui.separator();
        ui.label(RichText::new(format!("In {}", dir.display())).color(theme::FAINT));
    }

    fn row(&mut self, ui: &mut egui::Ui, dir: &Path, model: &'static Model, job: Option<&Job>) {
        let installed = models::installed(dir, model);
        ui.horizontal(|ui| {
            ui.label(RichText::new(model.name).color(theme::INK));
            ui.label(RichText::new(megabytes(model.bytes())).color(theme::FAINT));
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match job {
                    Some(Job::Installing { done, total }) => {
                        let fraction = *done as f32 / (*total).max(1) as f32;
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .desired_width(120.0)
                                .show_percentage(),
                        );
                    }
                    _ if installed => {
                        if ui
                            .button("Remove")
                            .on_hover_text("Delete its files; its tool goes with it")
                            .clicked()
                        {
                            self.remove(dir, model);
                        }
                    }
                    _ => {
                        if ui
                            .button("Install")
                            .on_hover_text("Download it, checked against its pinned hash")
                            .clicked()
                        {
                            self.install(ui.ctx().clone(), dir.to_path_buf(), model);
                        }
                    }
                },
            );
        });
        ui.label(RichText::new(model.does).color(theme::MUTED));
        ui.label(RichText::new(model.licence).small().color(theme::FAINT));
        if let Some(Job::Failed(reason)) = job {
            ui.label(RichText::new(reason).small().color(theme::DANGER));
        }
    }

    /// Download `model` on a thread of its own, reporting progress back to
    /// the panel.
    fn install(&self, ctx: egui::Context, dir: PathBuf, model: &'static Model) {
        let jobs = self.jobs.clone();
        set(
            &jobs,
            model.id,
            Some(Job::Installing {
                done: 0,
                total: model.bytes(),
            }),
        );
        std::thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(anyhow::Error::from)
                .and_then(|runtime| {
                    runtime.block_on(models::install(&dir, model, |p| {
                        set(
                            &jobs,
                            model.id,
                            Some(Job::Installing {
                                done: p.done,
                                total: p.total,
                            }),
                        );
                        ctx.request_repaint();
                    }))
                });
            match result {
                Ok(()) => set(&jobs, model.id, None),
                Err(e) => {
                    tracing::warn!("installing the {} audio model: {e:#}", model.id);
                    set(&jobs, model.id, Some(Job::Failed(format!("{e:#}"))));
                }
            }
            ctx.request_repaint();
        });
    }

    fn remove(&self, dir: &Path, model: &'static Model) {
        let job = models::remove(dir, model)
            .err()
            .map(|e| Job::Failed(format!("{e:#}")));
        set(&self.jobs, model.id, job);
    }
}

fn set(jobs: &Mutex<HashMap<&'static str, Job>>, id: &'static str, job: Option<Job>) {
    if let Ok(mut jobs) = jobs.lock() {
        match job {
            Some(job) => jobs.insert(id, job),
            None => jobs.remove(id),
        };
    }
}

fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.0;
    if mb < 10.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{mb:.0} MB")
    }
}
