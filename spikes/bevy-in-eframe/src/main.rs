//! Test 2 of docs/rfcs/authoring/all-rust.md: Bevy without a window, drawing
//! the song world on eframe's wgpu device into a texture that two egui
//! viewports show, then with the windows hidden. A crate of its own: nothing
//! here reaches the apps.
//!
//! Environment:
//!   SPIKE_WORLD     a manifest.json (the song world the earlier measurement wrote)
//!   SPIKE_ASSETS    the folder its mesh paths are relative to (default: assets/ beside it)
//!   SPIKE_SIZE      the texture, WxH in pixels (default 2880x1620)
//!   SPIKE_HZ        Bevy's tick rate (default 120; 0 = as fast as it can)
//!   SPIKE_CASCADES  the directional light's shadow cascades (default 4, Bevy's)
//!   SPIKE_HIDE      seconds: hide both windows then (default 12)
//!   SPIKE_SHOW      seconds: show them again (default 20)
//!   SPIKE_EXIT      seconds: write report.json and quit (default 26)
//!   SPIKE_SHOT      seconds: save shot.png from the main window (default 6)
//!   SPIKE_TWO       0 for one window (default: two)
//!   SPIKE_OUT       the folder for report.json and shot.png (default .)

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use bevy::{
    asset::AssetPlugin,
    camera::{ManualTextureViewHandle, RenderTarget},
    core_pipeline::tonemapping::Tonemapping,
    gltf::GltfAssetLabel,
    light::CascadeShadowConfigBuilder,
    prelude::*,
    render::{
        RenderPlugin,
        renderer::{
            RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue,
            WgpuWrapper,
        },
        settings::{RenderCreation, RenderResources},
        texture::{ManualTextureView, ManualTextureViews},
        view::Msaa,
    },
    world_serialization::WorldAssetRoot,
};
use eframe::egui;
use localgpt_world_bevy as wb;
use localgpt_world_types as wt;

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[derive(Clone)]
struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// One device for the panels and the world, made the way Bevy makes its own:
/// every feature and limit the adapter has.
fn make_gpu() -> Gpu {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    desc.backends = wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("an adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("spike: one device"),
        required_features: adapter.features(),
        required_limits: adapter.limits(),
        // What Bevy's own initialisation does (bevy_render/src/renderer/mod.rs).
        experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
        ..Default::default()
    }))
    .expect("a device");
    Gpu {
        instance,
        adapter,
        device,
        queue,
    }
}

/// Shared between the Bevy thread and the window.
#[derive(Default)]
struct Control {
    ticks: AtomicU64,
    stop: AtomicBool,
    hidden: AtomicBool,
    per_second: Mutex<Vec<(f64, u64)>>,
    scene_ready: AtomicBool,
}

fn unix_s() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[derive(Resource)]
struct SpikeWorld {
    manifest: wt::WorldManifest,
    cascades: usize,
}

#[derive(Component)]
struct Orbit {
    center: Vec3,
    radius: f32,
    height: f32,
    angle: f32,
}

fn spawn_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    world: Res<SpikeWorld>,
) {
    let m = &world.manifest;
    let camera_transform = wb::camera_transform(m);
    let look_at = m
        .camera
        .as_ref()
        .map(|c| Vec3::from(c.look_at))
        .unwrap_or(Vec3::ZERO);
    let flat = Vec3::new(
        camera_transform.translation.x - look_at.x,
        0.0,
        camera_transform.translation.z - look_at.z,
    );
    let orbit = Orbit {
        center: look_at,
        radius: flat.length().max(1.0),
        height: camera_transform.translation.y,
        angle: flat.z.atan2(flat.x),
    };
    let mut cam = commands.spawn((
        Camera3d::default(),
        Camera::default(),
        RenderTarget::TextureView(ManualTextureViewHandle(0)),
        Tonemapping::TonyMcMapface,
        Msaa::Sample4,
        camera_transform,
        Projection::Perspective(wb::perspective(m.camera.as_ref())),
        orbit,
    ));
    if let Some(fog) = wb::distance_fog(m.environment.as_ref()) {
        cam.insert(fog);
    }
    let (mut shapes, mut models, mut lights) = (0, 0, 0);
    for def in &m.entities {
        let mut transform = wb::transform(&def.transform);
        if let Some(light) = &def.light {
            transform = wb::light_transform(light, transform);
        }
        let mut e = commands.spawn((
            Name::new(def.name.0.clone()),
            transform,
            wb::visibility(&def.transform),
        ));
        if let Some(shape) = &def.shape {
            let material = def
                .material
                .as_ref()
                .map(wb::standard_material)
                .unwrap_or_default();
            e.insert((
                Mesh3d(meshes.add(wb::shape_mesh(shape))),
                MeshMaterial3d(materials.add(material)),
            ));
            shapes += 1;
        }
        if let Some(mesh) = &def.mesh_asset {
            let handle = asset_server.load(GltfAssetLabel::Scene(0).from_asset(mesh.path.clone()));
            e.insert(WorldAssetRoot(handle));
            models += 1;
        }
        if let Some(light) = &def.light {
            wb::insert_light(&mut e, light);
            if world.cascades != 4 {
                e.insert(
                    CascadeShadowConfigBuilder {
                        num_cascades: world.cascades,
                        maximum_distance: 200.0,
                        ..default()
                    }
                    .build(),
                );
            }
            lights += 1;
        }
    }
    info!(
        "spike: {} entities: {shapes} shapes, {models} models, {lights} lights",
        m.entities.len()
    );
}

fn orbit(time: Res<Time>, mut q: Query<(&mut Transform, &mut Orbit)>) {
    for (mut t, mut o) in &mut q {
        o.angle += time.delta_secs() * 0.15;
        let p = o.center + Vec3::new(o.angle.cos() * o.radius, 0.0, o.angle.sin() * o.radius);
        t.translation = Vec3::new(p.x, o.height, p.z);
        t.look_at(o.center, Vec3::Y);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_bevy(
    ctl: Arc<Control>,
    gpu: Gpu,
    view: wgpu::TextureView,
    size: (u32, u32),
    world: PathBuf,
    assets: PathBuf,
    cascades: usize,
    hz: f64,
) {
    let manifest: wt::WorldManifest =
        serde_json::from_str(&std::fs::read_to_string(&world).expect("the manifest"))
            .expect("a world manifest");
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .set(RenderPlugin {
                render_creation: RenderCreation::Manual(RenderResources(
                    RenderDevice::from(gpu.device.clone()),
                    RenderQueue(Arc::new(WgpuWrapper::new(gpu.queue.clone()))),
                    RenderAdapterInfo(WgpuWrapper::new(gpu.adapter.get_info())),
                    RenderAdapter(Arc::new(WgpuWrapper::new(gpu.adapter.clone()))),
                    RenderInstance(Arc::new(WgpuWrapper::new(gpu.instance.clone()))),
                )),
                synchronous_pipeline_compilation: false,
                ..default()
            })
            .set(AssetPlugin {
                file_path: assets.to_string_lossy().into_owned(),
                ..default()
            })
            // bevy_gltf brings the window feature in: no window entity, no exit on close.
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            }),
    );
    app.insert_resource(wb::clear_color(manifest.environment.as_ref()));
    app.insert_resource(wb::ambient_light(manifest.environment.as_ref()));
    app.world_mut().resource_mut::<ManualTextureViews>().insert(
        ManualTextureViewHandle(0),
        ManualTextureView::with_default_format(view.into(), UVec2::new(size.0, size.1)),
    );
    app.insert_resource(SpikeWorld { manifest, cascades });
    app.add_systems(Startup, spawn_world);
    app.add_systems(Update, orbit);
    app.finish();
    app.cleanup();
    ctl.scene_ready.store(true, Ordering::Relaxed);

    let mut next = Instant::now();
    let mut sec_start = Instant::now();
    let mut sec_ticks = 0u64;
    while !ctl.stop.load(Ordering::Relaxed) {
        app.update();
        ctl.ticks.fetch_add(1, Ordering::Relaxed);
        sec_ticks += 1;
        if sec_start.elapsed() >= Duration::from_secs(1) {
            ctl.per_second.lock().unwrap().push((unix_s(), sec_ticks));
            sec_ticks = 0;
            sec_start = Instant::now();
        }
        if hz > 0.0 {
            next += Duration::from_secs_f64(1.0 / hz);
            match next.checked_duration_since(Instant::now()) {
                Some(d) => std::thread::sleep(d),
                None => next = Instant::now(),
            }
        }
    }
}

/// The world's texture, read back through the shared queue: what both
/// windows sample, as a PNG.
fn dump_texture(gpu: &Gpu, texture: &wgpu::Texture, size: (u32, u32), path: &Path) {
    let bytes_per_row = size.0 * 4;
    let padded = bytes_per_row.div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shot"),
        size: (padded * size.1) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(size.1),
            },
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    match rx.recv() {
        Ok(Ok(())) => {}
        other => {
            eprintln!("spike: reading the texture back failed: {other:?}");
            return;
        }
    }
    let data = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((bytes_per_row * size.1) as usize);
    for row in 0..size.1 {
        let start = (row * padded) as usize;
        pixels.extend_from_slice(&data[start..start + bytes_per_row as usize]);
    }
    drop(data);
    buffer.unmap();
    match image::RgbaImage::from_raw(size.0, size.1, pixels) {
        Some(img) => match img.save(path) {
            Ok(()) => eprintln!("spike: wrote {}", path.display()),
            Err(e) => eprintln!("spike: {}: {e}", path.display()),
        },
        None => eprintln!("spike: the texture's bytes had the wrong length"),
    }
}

struct Phases {
    hide: f64,
    show: f64,
    exit: f64,
    shot: f64,
}

struct Spike {
    ctl: Arc<Control>,
    gpu: Gpu,
    texture: wgpu::Texture,
    tex: egui::TextureId,
    size: [u32; 2],
    t0: Instant,
    t0_unix: f64,
    phases: Phases,
    two: bool,
    main_frames: Vec<f64>,
    second_frames: Arc<Mutex<Vec<f64>>>,
    second_hidden: Arc<AtomicBool>,
    hidden: bool,
    shot_done: bool,
    done: bool,
    out: PathBuf,
    label: String,
    hz: f64,
    cascades: usize,
    last_frame: Option<Instant>,
}

impl Spike {
    fn report(&self) {
        let ticks = self.ctl.per_second.lock().unwrap().clone();
        let second = self.second_frames.lock().unwrap().clone();
        let phase = |t: f64| -> &'static str {
            if t < self.phases.hide {
                "visible"
            } else if t < self.phases.show {
                "hidden"
            } else {
                "visible_again"
            }
        };
        let mut out = String::new();
        out.push_str(&format!(
            "{{\n  \"label\": {:?},\n  \"size\": [{}, {}],\n  \"tick_hz\": {},\n  \"cascades\": {},\n  \"two_windows\": {},\n  \"phases\": {{\"hide\": {}, \"show\": {}, \"exit\": {}}},\n  \"t0_unix\": {:.3},\n",
            self.label, self.size[0], self.size[1], self.hz, self.cascades, self.two,
            self.phases.hide, self.phases.show, self.phases.exit, self.t0_unix
        ));
        for name in ["visible", "hidden", "visible_again"] {
            let (lo, hi) = match name {
                "visible" => (2.0, self.phases.hide),
                "hidden" => (self.phases.hide + 1.0, self.phases.show),
                _ => (self.phases.show + 1.0, self.phases.exit),
            };
            let span = (hi - lo).max(0.001);
            let main = self
                .main_frames
                .iter()
                .filter(|t| **t >= lo && **t < hi)
                .count() as f64
                / span;
            let sec = second.iter().filter(|t| **t >= lo && **t < hi).count() as f64 / span;
            let bevy: Vec<u64> = ticks
                .iter()
                .filter(|(t, _)| {
                    let rel = t - self.t0_unix;
                    rel >= lo && rel < hi
                })
                .map(|(_, n)| *n)
                .collect();
            let bevy_mean = if bevy.is_empty() {
                0.0
            } else {
                bevy.iter().sum::<u64>() as f64 / bevy.len() as f64
            };
            let bevy_min = bevy.iter().min().copied().unwrap_or(0);
            out.push_str(&format!(
                "  \"{name}\": {{\"main_window_fps\": {main:.1}, \"second_window_fps\": {sec:.1}, \"bevy_ticks_per_s_mean\": {bevy_mean:.1}, \"bevy_ticks_per_s_min\": {bevy_min}}},\n"
            ));
        }
        out.push_str(&format!(
            "  \"bevy_ticks_total\": {},\n  \"ticks_per_second\": [{}]\n}}\n",
            self.ctl.ticks.load(Ordering::Relaxed),
            ticks
                .iter()
                .map(|(t, n)| format!("[{:.1}, {n}]", t - self.t0_unix))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let _ = phase(0.0);
        let path = self.out.join("report.json");
        match std::fs::write(&path, &out) {
            Ok(()) => eprintln!("spike: wrote {}\n{out}", path.display()),
            Err(e) => eprintln!("spike: {}: {e}", path.display()),
        }
    }
}

impl eframe::App for Spike {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        // Paced at the display's rate even where nothing presents (a locked
        // Mac has no vsync): a frame sooner than 1/120 s after the last waits.
        let frame = Duration::from_secs_f64(1.0 / 120.0);
        if let Some(last) = self.last_frame {
            let since = last.elapsed();
            if since < frame {
                std::thread::sleep(frame - since);
            }
        }
        self.last_frame = Some(Instant::now());
        let t = self.t0.elapsed().as_secs_f64();
        self.main_frames.push(t);
        let hidden_now = t >= self.phases.hide && t < self.phases.show;
        if hidden_now != self.hidden {
            self.hidden = hidden_now;
            self.ctl.hidden.store(hidden_now, Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(!hidden_now));
            eprintln!(
                "spike: t={t:.1} main window {}",
                if hidden_now { "hidden" } else { "shown" }
            );
        }
        if t >= self.phases.shot && !self.shot_done {
            self.shot_done = true;
            dump_texture(
                &self.gpu,
                &self.texture,
                (self.size[0], self.size[1]),
                &self.out.join("shot.png"),
            );
        }
        if t >= self.phases.exit && !self.done {
            self.done = true;
            self.report();
            self.ctl.stop.store(true, Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let ticks = self.ctl.ticks.load(Ordering::Relaxed);
        egui::Panel::top("bar").show(ui, |ui| {
            ui.label(format!(
                "t={t:5.1}s · Bevy ticks {ticks} · main frames {} · second frames {} · {}",
                self.main_frames.len(),
                self.second_frames.lock().unwrap().len(),
                if hidden_now { "HIDDEN" } else { "visible" }
            ));
        });
        let tex = self.tex;
        let size = egui::vec2(self.size[0] as f32, self.size[1] as f32);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let avail = ui.available_size();
                let scale = (avail.x / size.x).min(avail.y / size.y);
                ui.image((tex, size * scale));
            });
        if self.two {
            let frames = self.second_frames.clone();
            let ctl = self.ctl.clone();
            let hidden_state = self.second_hidden.clone();
            let t0 = self.t0;
            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of("second"),
                egui::ViewportBuilder::default()
                    .with_title("spike: the same world, a second window")
                    .with_inner_size([1000.0, 600.0])
                    .with_position([60.0, 80.0]),
                move |ui, _class| {
                    frames.lock().unwrap().push(t0.elapsed().as_secs_f64());
                    let want_hidden = ctl.hidden.load(Ordering::Relaxed);
                    if want_hidden != hidden_state.load(Ordering::Relaxed) {
                        hidden_state.store(want_hidden, Ordering::Relaxed);
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Visible(!want_hidden));
                    }
                    let avail = ui.available_size();
                    let scale = (avail.x / size.x).min(avail.y / size.y);
                    ui.image((tex, size * scale));
                    ui.ctx()
                        .request_repaint_after(Duration::from_secs_f64(1.0 / 120.0));
                },
            );
        }
        // At most the display's rate, so a run on a locked Mac (no vsync) still
        // means something; with vsync it is the same.
        ctx.request_repaint_after(Duration::from_secs_f64(1.0 / 120.0));
    }
}

fn main() -> eframe::Result {
    let world = std::env::var("SPIKE_WORLD")
        .map(PathBuf::from)
        .expect("SPIKE_WORLD=<manifest.json>");
    let assets = std::env::var("SPIKE_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| world.parent().unwrap_or(Path::new(".")).join("assets"));
    let size: Vec<u32> = std::env::var("SPIKE_SIZE")
        .unwrap_or_else(|_| "2880x1620".into())
        .split('x')
        .filter_map(|s| s.parse().ok())
        .collect();
    let size = (
        size.first().copied().unwrap_or(2880),
        size.get(1).copied().unwrap_or(1620),
    );
    let hz = env_f64("SPIKE_HZ", 120.0);
    let cascades = env_f64("SPIKE_CASCADES", 4.0) as usize;
    let phases = Phases {
        hide: env_f64("SPIKE_HIDE", 12.0),
        show: env_f64("SPIKE_SHOW", 20.0),
        exit: env_f64("SPIKE_EXIT", 26.0),
        shot: env_f64("SPIKE_SHOT", 6.0),
    };
    let two = std::env::var("SPIKE_TWO").map(|v| v != "0").unwrap_or(true);
    let out = std::env::var("SPIKE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| ".".into());
    let _ = std::fs::create_dir_all(&out);
    let label = std::env::var("SPIKE_LABEL").unwrap_or_default();

    let gpu = make_gpu();
    eprintln!("spike: adapter {:?}", gpu.adapter.get_info().name);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("the world"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let ctl = Arc::new(Control::default());
    {
        let (ctl, gpu, view) = (ctl.clone(), gpu.clone(), view.clone());
        std::thread::Builder::new()
            .name("bevy".into())
            .spawn(move || run_bevy(ctl, gpu, view, size, world, assets, cascades, hz))
            .expect("the Bevy thread");
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 760.0])
            .with_title("spike: Bevy in eframe"),
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
    let t0 = Instant::now();
    let t0_unix = unix_s();
    let gpu_for_app = gpu.clone();
    eframe::run_native(
        "spike-bevy-in-eframe",
        options,
        Box::new(move |cc| {
            let rs = cc.wgpu_render_state.as_ref().expect("wgpu");
            let tex = rs.renderer.write().register_native_texture(
                &rs.device,
                &view,
                wgpu::FilterMode::Linear,
            );
            Ok(Box::new(Spike {
                ctl,
                gpu: gpu_for_app,
                texture,
                tex,
                size: [size.0, size.1],
                t0,
                t0_unix,
                phases,
                two,
                main_frames: Vec::new(),
                second_frames: Arc::new(Mutex::new(Vec::new())),
                second_hidden: Arc::new(AtomicBool::new(false)),
                hidden: false,
                shot_done: false,
                done: false,
                out,
                label,
                hz,
                cascades,
                last_frame: None,
            }))
        }),
    )
}
