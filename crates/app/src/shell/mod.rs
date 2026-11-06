//! The shell: the app's interface, in Rust (`docs/rfcs/authoring/all-rust.md`,
//! decided 2026-11-05).
//!
//! eframe owns the window and egui draws the panels. Bevy runs on a thread
//! of its own with no window, and draws the world into a texture on the same
//! wgpu device, which the world panel shows (`world.rs`). The panels are a
//! client of the app's interface protocol (`webui::link`), as a paired
//! phone's page is: they are sent what it is sent and ask what it asks, so
//! the phone sees what the window does.
//!
//! The whole Bevy app is built on its own thread, not built here and sent
//! there: a non-send resource (the terminals, the page) belongs to the thread
//! that inserted it.

mod bar;
mod document;
mod drive;
mod history;
mod scene;
mod state;
mod terminal;
mod terminals;
mod theme;
mod tiles;
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

/// Bevy's ticks a second (default 60).
pub const LOCALGPT_APP_SHELL_HZ: &str = "LOCALGPT_APP_SHELL_HZ";
/// A dev hook for a look without a hand: a picture of the window, panels
/// and all, to this path, a while after launch, then quit; the frame comes
/// back black when the Mac is locked.
pub const LOCALGPT_APP_SCREENSHOT: &str = "LOCALGPT_APP_SCREENSHOT";
/// The while, in seconds (default 12: long enough for a world's assets to
/// land).
pub const LOCALGPT_APP_SCREENSHOT_AFTER: &str = "LOCALGPT_APP_SCREENSHOT_AFTER";
/// How long a closed window waits for the world to stop.
const STOP_WAIT: Duration = Duration::from_secs(5);

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
    // The world finishes its frame and drops (the terminals' shells hang
    // up). A world that does not stop must not keep a closed window's
    // process alive: past the wait, the thread is left to the exit.
    let deadline = Instant::now() + STOP_WAIT;
    while !bevy.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if bevy.is_finished() {
        match bevy.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!("the world: {e:#}"),
            Err(_) => tracing::error!("the world's thread panicked"),
        }
    } else {
        tracing::error!(
            "the world did not stop within {} s of the window closing",
            STOP_WAIT.as_secs()
        );
    }
    result.map_err(|e| anyhow!("the shell: {e}"))
}

/// The window: what the app said, and the panels that show it.
struct Shell {
    end: ShellEnd,
    bar: bar::Bar,
    world: world::Panel,
    terminals: terminals::Terminals,
    timeline: timeline::Timeline,
    scene: scene::Scene,
    document: document::Document,
    history: history::History,
    /// Where the panels are: the page's columns, until a hand moves them.
    layout: tiles::Layout,
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
        let open = std::env::var(crate::webui::LOCALGPT_APP_UI_OPEN).unwrap_or_default();
        let opens = |what: &str| open.split(',').any(|o| o == what);
        let terminals = terminals::Terminals::new(ctx, opens("terminal"));
        // `outliner` starts the left column on its Outliner tab. The panels
        // come back as a hand left them (`<state>/app-layout.json`).
        let layout = tiles::Layout::open(
            opens("outliner"),
            localgpt_world_agent::paths::Dirs::resolve()
                .map(|dirs| dirs.state.join("app-layout.json")),
        );
        // `LOCALGPT_APP_SCREENSHOT=<png>`: a picture of the window a while
        // in, then quit.
        let shot = std::env::var(LOCALGPT_APP_SCREENSHOT).ok().map(|path| {
            let at = std::env::var(LOCALGPT_APP_SCREENSHOT_AFTER)
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(12.0);
            (at, std::path::PathBuf::from(path))
        });
        Self {
            end,
            bar: bar::Bar::default(),
            world,
            terminals,
            timeline: timeline::Timeline::new(),
            scene: scene::Scene::new(),
            document: document::Document::new(),
            history: history::History::default(),
            layout,
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
    fn on_exit(&mut self) {
        self.layout.on_exit();
    }

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
            self.bar.ui(ui, &self.state, &self.end, &mut self.layout)
        });
        // The rest are tiles: the left column (the document, a live
        // canvas's history, the world's things), the world, the terminals,
        // and the timeline across their foot, until a hand moves them. Their
        // first sizes come from the window, as the page's columns do, not
        // from what the panels before them left: those appear when the
        // app's first messages arrive, sooner or later.
        self.layout.follow(&self.state, ui.available_size());
        let world_drawn = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                self.layout.ui(
                    ui,
                    tiles::Panels {
                        state: &mut self.state,
                        end: &self.end,
                        frame,
                        world: &mut self.world,
                        world_rect: &mut self.world_rect,
                        terminals: &mut self.terminals,
                        scene: &mut self.scene,
                        document: &mut self.document,
                        history: &mut self.history,
                        timeline: &mut self.timeline,
                    },
                )
            })
            .inner;
        if !world_drawn {
            self.world.away();
        }
        // A modulation's link in the inspector: its lane, shown next frame.
        if let Some(track) = self.scene.take_reveal() {
            self.timeline.reveal(track);
        }
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
