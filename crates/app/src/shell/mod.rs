//! The shell: the app's interface in Rust (`docs/rfcs/authoring/all-rust.md`,
//! decided 2026-11-05).
//!
//! eframe owns the window and egui draws the panels. Bevy runs on a thread
//! of its own with no window, and draws the world into a texture on the same
//! wgpu device, which the world panel shows (`world.rs`). The panels are one
//! more client of the page's protocol (`webui::link`): they are sent what the
//! page is sent and ask what the page asks, so the page and the shell stay
//! one interface while the shell catches up, and a paired phone keeps the
//! page. `LOCALGPT_APP_SHELL=egui` opens it.
//!
//! The whole Bevy app is built on its own thread, not built here and sent
//! there: a non-send resource (the terminals, the page) belongs to the thread
//! that inserted it.

mod drive;
mod history;
mod state;
mod terminal;
mod terminals;
mod theme;
mod timeline;
pub mod world;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use bevy::app::PluginsState;
use bevy::prelude::*;
use eframe::egui;
use eframe::egui_wgpu;
use eframe::wgpu;

use crate::webui::FromPage;
use crate::webui::link::{self, ShellEnd};
use state::State;

pub const LOCALGPT_APP_SHELL: &str = "LOCALGPT_APP_SHELL";
/// Bevy's ticks a second while the shell is the interface (default 60).
pub const LOCALGPT_APP_SHELL_HZ: &str = "LOCALGPT_APP_SHELL_HZ";
/// Seconds after which the shell saves a picture of its window to
/// `LOCALGPT_APP_SHELL_SHOT_PATH` and quits: a check without a hand.
pub const LOCALGPT_APP_SHELL_SHOT: &str = "LOCALGPT_APP_SHELL_SHOT";
pub const LOCALGPT_APP_SHELL_SHOT_PATH: &str = "LOCALGPT_APP_SHELL_SHOT_PATH";

/// Whether the shell, not the page, is to be the interface.
pub fn requested() -> bool {
    std::env::var(LOCALGPT_APP_SHELL).is_ok_and(|v| v == "egui")
}

/// One device for the panels and the world, made the way Bevy makes its
/// own: every feature and limit the adapter has.
#[derive(Clone)]
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub fn new() -> anyhow::Result<Self> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter =
            bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            }))
            .map_err(|e| anyhow!("no GPU adapter: {e}"))?;
        let (device, queue) =
            bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("LocalGPT: the panels and the world"),
                required_features: adapter.features(),
                required_limits: adapter.limits(),
                // What Bevy's own initialisation asks for.
                experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
                ..Default::default()
            }))
            .map_err(|e| anyhow!("no GPU device: {e}"))?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }
}

/// Bevy's plugins when the shell owns the window: no winit and no window,
/// rendering on the shell's device.
pub fn base_plugins(gpu: &Gpu) -> bevy::app::PluginGroupBuilder {
    use bevy::render::RenderPlugin;
    use bevy::render::renderer::{
        RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue,
    };
    use bevy::render::settings::RenderCreation;
    DefaultPlugins
        .build()
        .disable::<bevy::winit::WinitPlugin>()
        .disable::<bevy::log::LogPlugin>()
        .set(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            close_when_requested: false,
            ..default()
        })
        .set(RenderPlugin {
            render_creation: RenderCreation::manual(
                RenderDevice::new(gpu.device.clone()),
                RenderQueue::new(gpu.queue.clone()),
                RenderAdapterInfo::new(gpu.adapter.get_info()),
                RenderAdapter::new(gpu.adapter.clone()),
                RenderInstance::new(gpu.instance.clone()),
            ),
            ..default()
        })
        .set(bevy::asset::AssetPlugin {
            file_path: "/".to_string(),
            ..default()
        })
}

/// Opens the shell's window on this thread and runs the app on a thread of
/// its own, built there by `assemble`; returns when the window closes.
pub fn run(
    gpu: Gpu,
    assemble: impl FnOnce(&mut App) -> anyhow::Result<()> + Send + 'static,
) -> anyhow::Result<()> {
    let ctx: Arc<OnceLock<egui::Context>> = Arc::default();
    let wake = {
        let ctx = ctx.clone();
        move || {
            if let Some(ctx) = ctx.get() {
                ctx.request_repaint();
            }
        }
    };
    let (shell_link, end) = link::pair(wake);
    let target = world::Target::new(&gpu.device, world::INITIAL);
    let (bridge, panel_link) = world::bridge(target.view.clone(), target.size);
    let stop = Arc::new(AtomicBool::new(false));
    let hz = std::env::var(LOCALGPT_APP_SHELL_HZ)
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|hz| *hz > 0.0)
        .unwrap_or(60.0);

    let bevy = {
        let gpu = gpu.clone();
        let stop = stop.clone();
        std::thread::Builder::new()
            .name("bevy".into())
            .spawn(move || -> anyhow::Result<()> {
                let mut app = App::new();
                app.add_plugins(base_plugins(&gpu));
                app.insert_resource(shell_link);
                app.add_plugins(bridge);
                assemble(&mut app)?;
                while app.plugins_state() == PluginsState::Adding {
                    bevy::tasks::tick_global_task_pools_on_main_thread();
                }
                app.finish();
                app.cleanup();
                let frame = Duration::from_secs_f64(1.0 / hz);
                let mut next = Instant::now();
                while !stop.load(Ordering::Relaxed) {
                    app.update();
                    if app.should_exit().is_some() {
                        break;
                    }
                    next += frame;
                    match next.checked_duration_since(Instant::now()) {
                        Some(wait) => std::thread::sleep(wait),
                        None => next = Instant::now(),
                    }
                }
                Ok(())
            })
            .map_err(|e| anyhow!("the world's thread: {e}"))?
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LocalGPT")
            .with_inner_size([1440.0, 860.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::Existing(egui_wgpu::WgpuSetupExisting {
                instance: gpu.instance.clone(),
                adapter: gpu.adapter.clone(),
                device: gpu.device.clone(),
                queue: gpu.queue.clone(),
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = eframe::run_native(
        "LocalGPT",
        options,
        Box::new(move |cc| {
            let _ = ctx.set(cc.egui_ctx.clone());
            let render = cc
                .wgpu_render_state
                .as_ref()
                .ok_or("the shell needs wgpu")?;
            let tex = render.renderer.write().register_native_texture(
                &render.device,
                &target.view,
                wgpu::FilterMode::Linear,
            );
            Ok(Box::new(Shell::new(
                &cc.egui_ctx,
                end,
                world::Panel::new(panel_link, target, tex),
            )))
        }),
    );
    stop.store(true, Ordering::Relaxed);
    match bevy.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::error!("the world: {e:#}"),
        Err(_) => tracing::error!("the world's thread panicked"),
    }
    result.map_err(|e| anyhow!("the shell: {e}"))
}

/// The window: what the app said, and the panels that show it.
struct Shell {
    end: ShellEnd,
    world: world::Panel,
    terminals: terminals::Terminals,
    timeline: timeline::Timeline,
    state: State,
    started: Instant,
    shot: Option<(f64, std::path::PathBuf)>,
    shot_asked: bool,
    /// A scripted hand (`LOCALGPT_APP_SHELL_DRIVE`), for checks.
    drive: Option<drive::Drive>,
    /// Where the world panel was last drawn, for the scripted hand's clicks.
    world_rect: egui::Rect,
}

impl Shell {
    fn new(ctx: &egui::Context, end: ShellEnd, world: world::Panel) -> Self {
        // Until it is ready, the app sends a page nothing.
        end.ask(FromPage::Ready);
        theme::apply(ctx);
        // `LOCALGPT_APP_UI_OPEN=terminal` starts with a tab, as it does the
        // page.
        let open_terminal = std::env::var(crate::webui::LOCALGPT_APP_UI_OPEN)
            .is_ok_and(|open| open.split(',').any(|o| o == "terminal"));
        let terminals = terminals::Terminals::new(ctx, open_terminal);
        let shot = std::env::var(LOCALGPT_APP_SHELL_SHOT)
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|at| {
                let path = std::env::var(LOCALGPT_APP_SHELL_SHOT_PATH)
                    .unwrap_or_else(|_| "localgpt-shell.png".into());
                (at, std::path::PathBuf::from(path))
            });
        Self {
            end,
            world,
            terminals,
            timeline: timeline::Timeline::new(),
            state: State::default(),
            started: Instant::now(),
            shot,
            shot_asked: false,
            drive: drive::Drive::from_env(),
            world_rect: egui::Rect::NOTHING,
        }
    }

    fn check_shot(&mut self, ctx: &egui::Context) {
        let Some((at, path)) = &self.shot else {
            return;
        };
        if !self.shot_asked && self.started.elapsed().as_secs_f64() >= *at {
            self.shot_asked = true;
            let (path, closer) = (path.clone(), ctx.clone());
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |image| {
                    save_png(&path, &image);
                    closer.send_viewport_cmd(egui::ViewportCommand::Close);
                }),
            ));
        }
    }
}

impl eframe::App for Shell {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(drive) = &mut self.drive {
            drive.inject(ctx, raw_input, self.world_rect);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        while let Ok(event) = self.end.events.try_recv() {
            self.state.apply(event);
        }
        self.terminals.take(&mut self.state);
        let ctx = ui.ctx().clone();
        self.check_shot(&ctx);

        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("LocalGPT");
                if let Some(title) = &self.state.title {
                    ui.separator();
                    ui.label(title);
                }
            });
        });
        // The timeline runs across the window, under everything else.
        if self.state.timeline.is_some() {
            egui::Panel::bottom("timeline")
                .resizable(true)
                .default_size(300.0)
                .min_size(120.0)
                .show(ui, |ui| self.timeline.ui(ui, &self.state, &self.end));
        }
        if self.state.history.is_some() {
            egui::Panel::left("history")
                .resizable(true)
                .default_size(300.0)
                .show(ui, |ui| history::ui(ui, &self.state, &self.end));
        }
        // The terminals: wide enough for an agent's TUI, never the window.
        let width = ui.available_width();
        egui::Panel::right("terminals")
            .resizable(true)
            .default_size((width * 0.4).clamp(360.0, 640.0))
            .min_size(280.0)
            .max_size((width * 0.65).max(320.0))
            .show(ui, |ui| self.terminals.ui(ui, &self.end));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                self.world_rect = ui.available_rect_before_wrap();
                self.world.ui(ui, frame);
            });
        // The world moves on its own: show its newest frame.
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn save_png(path: &std::path::Path, image: &egui::ColorImage) {
    let [w, h] = image.size;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    let result = image::RgbaImage::from_raw(w as u32, h as u32, bytes)
        .ok_or_else(|| anyhow!("{w}x{h}: the wrong number of pixels"))
        .and_then(|img| Ok(img.save(path)?));
    match result {
        Ok(()) => eprintln!("localgpt-app: the shell saved {}", path.display()),
        Err(e) => eprintln!("localgpt-app: the shell's picture: {e:#}"),
    }
}
