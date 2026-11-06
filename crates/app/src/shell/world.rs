//! The world as a panel: Bevy renders into a texture the shell made on the
//! shared device, the panel shows it, and the panel's input goes back to
//! Bevy as the input a window would have given it (`docs/rfcs/authoring/
//! all-rust.md`, "The world: Bevy without a window").
//!
//! Two halves. [`Panel`] lives in eframe: it draws the texture, makes a new
//! one when the panel's size settles on another, and turns egui's events
//! into [`Input`]. [`Bridge`] is a Bevy plugin: the cameras render into the
//! texture (Gen's [`OffscreenRenderTarget`], pointed at it), and [`Input`]
//! becomes Bevy's keyboard and mouse messages and picking's pointer input,
//! on the texture, so the fly camera, a click that selects and a drag that
//! moves see a hand as they would in a window.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use bevy::camera::{ManualTextureViewHandle, NormalizedRenderTarget};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardFocusLost, KeyboardInput, NativeKey};
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::picking::pointer::{
    Location, PointerAction, PointerButton, PointerId, PointerInput, PointerLocation,
};
use bevy::prelude::*;
use bevy::render::texture::{ManualTextureView, ManualTextureViews};
use bevy::window::{PrimaryWindow, WindowResolution};
use eframe::egui;
use eframe::wgpu;
use localgpt_gen::gen3d::offscreen::OffscreenRenderTarget;

/// The world's texture, as Bevy's cameras know it.
pub const VIEW: ManualTextureViewHandle = ManualTextureViewHandle(0x5e11);
/// What Bevy renders into: the format `ManualTextureView::with_default_format`
/// says, which egui samples as colour.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// The texture before the panel has a size.
pub const INITIAL: [u32; 2] = [1600, 1000];
/// How long a new panel size must hold before the texture follows it.
const SETTLE: Duration = Duration::from_millis(120);

/// A texture the world renders into and the panel shows.
pub struct Target {
    // Kept alive while egui or Bevy may still draw with its view.
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub size: [u32; 2],
}

impl Target {
    pub fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let size = [size[0].max(1), size[1].max(1)];
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("the world"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            _texture: texture,
            view,
            size,
        }
    }
}

/// What the panel tells the world: a window's input, on the texture. Points
/// are the texture's pixels, the coordinates a camera rendering into it
/// takes.
#[derive(Debug, Clone)]
pub enum Input {
    Key {
        code: KeyCode,
        logical: Key,
        pressed: bool,
        repeat: bool,
    },
    /// The world lost the keyboard: every key it held is let go.
    FocusLost,
    Button {
        button: MouseButton,
        pressed: bool,
        at: Vec2,
    },
    /// The pointer moved over the world.
    Moved { at: Vec2, delta: Vec2 },
    /// The mouse moved, as the device says, wherever the pointer is: what the
    /// fly camera turns by.
    Motion(Vec2),
    Wheel {
        x: f32,
        y: f32,
        unit: MouseScrollUnit,
        at: Vec2,
    },
    /// The pointer left the world.
    Left,
    /// Points to pixels: the world's own interface is laid out in points.
    Scale(f32),
}

/// A new texture for the world, and its size.
pub struct Resize {
    pub view: wgpu::TextureView,
    pub size: [u32; 2],
}

/// The panel's end and the world's: [`Panel`] keeps the senders, [`Bridge`]
/// the receivers.
pub fn bridge(view: wgpu::TextureView, size: [u32; 2]) -> (Bridge, PanelLink) {
    let (input, input_rx) = channel();
    let (resize, resize_rx) = channel();
    (
        Bridge {
            view,
            size,
            input: Mutex::new(Some(input_rx)),
            resize: Mutex::new(Some(resize_rx)),
        },
        PanelLink { input, resize },
    )
}

pub struct PanelLink {
    input: Sender<Input>,
    resize: Sender<Resize>,
}

// ---------------------------------------------------------------------------
// Bevy's half
// ---------------------------------------------------------------------------

pub struct Bridge {
    view: wgpu::TextureView,
    size: [u32; 2],
    input: Mutex<Option<Receiver<Input>>>,
    resize: Mutex<Option<Receiver<Resize>>>,
}

#[derive(Resource)]
struct Inbox {
    input: Mutex<Receiver<Input>>,
    resize: Mutex<Receiver<Resize>>,
}

/// The pointer's actions this frame, for picking's input set: read with the
/// rest of the input before Bevy's input systems, written in picking's.
#[derive(Resource, Default)]
struct PointerQueue(Vec<(Vec2, PointerAction)>, bool);

/// A stand-in for the window: the entity Bevy's input messages name, a
/// `Window` marked `PrimaryWindow` that nothing gives a surface. Picking asks
/// for a primary window before it casts a ray from any camera, even one
/// rendering into a texture (bevy_picking 0.20, `Location::is_in_viewport`),
/// and focus requests (`AcquireFocus` on a press, `FocusedInput` for each
/// key) bubble until they reach an entity with a `Window`: on a primary
/// window without one they bounce on it forever, since propagation has no
/// cycle guard. The renderer draws only windows with a native handle, which
/// this never gets (WinitPlugin is off), so it is seen by input and focus
/// and not by the renderer. Its size and scale follow the panel, for what
/// asks the window how big it is. Gen, seeing a primary window, leaves its
/// cameras on it; `aim_cameras` points them at the texture instead.
#[derive(Resource)]
struct Surface(Entity);

/// Every camera aimed at the window renders into the world's texture, and
/// the first is the one the world's own interface (`bevy_ui`) draws through.
fn aim_cameras(
    mut commands: Commands,
    mut cameras: Query<(Entity, &mut bevy::camera::RenderTarget), With<Camera>>,
    ui_camera: Query<(), With<bevy::ui::IsDefaultUiCamera>>,
) {
    let mut has_ui_camera = !ui_camera.is_empty();
    for (camera, mut target) in &mut cameras {
        if matches!(*target, bevy::camera::RenderTarget::Window(_)) {
            *target = bevy::camera::RenderTarget::TextureView(VIEW);
            if !has_ui_camera {
                commands.entity(camera).insert(bevy::ui::IsDefaultUiCamera);
                has_ui_camera = true;
            }
        }
    }
}

impl Plugin for Bridge {
    fn build(&self, app: &mut App) {
        let (Some(input), Some(resize)) = (
            self.input.lock().ok().and_then(|mut i| i.take()),
            self.resize.lock().ok().and_then(|mut r| r.take()),
        ) else {
            return;
        };
        app.world_mut()
            .resource_mut::<ManualTextureViews>()
            .insert(VIEW, manual(&self.view, self.size));
        // Gen points every camera without a window at its offscreen target;
        // this one is the texture, so there is no image to make.
        app.insert_resource(OffscreenRenderTarget {
            width: self.size[0],
            height: self.size[1],
            image_handle: None,
            texture_view: Some(VIEW),
        })
        .insert_resource(Inbox {
            input: Mutex::new(input),
            resize: Mutex::new(resize),
        })
        .init_resource::<PointerQueue>()
        .add_systems(PreStartup, {
            let size = self.size;
            move |mut commands: Commands| {
                let entity = commands
                    .spawn((
                        Name::new("the shell's world panel"),
                        Window {
                            title: "LocalGPT".into(),
                            resolution: WindowResolution::new(size[0], size[1]),
                            ..default()
                        },
                        PrimaryWindow,
                    ))
                    .id();
                commands.insert_resource(Surface(entity));
            }
        })
        .add_systems(First, follow_resize)
        // Before the camera system resolves targets, whenever a camera was
        // spawned: one left on the window would draw nowhere, since the
        // window has no surface.
        .add_systems(
            PostStartup,
            aim_cameras.before(bevy::camera::CameraUpdateSystems),
        )
        .add_systems(
            PostUpdate,
            aim_cameras.before(bevy::camera::CameraUpdateSystems),
        )
        .add_systems(PreUpdate, read_input.before(bevy::input::InputSystems))
        .add_systems(
            PreUpdate,
            write_pointer.in_set(bevy::picking::PickingSystems::Input),
        );
    }
}

fn manual(view: &wgpu::TextureView, size: [u32; 2]) -> ManualTextureView {
    ManualTextureView::with_default_format(view.clone().into(), UVec2::new(size[0], size[1]))
}

fn follow_resize(
    inbox: Res<Inbox>,
    mut views: ResMut<ManualTextureViews>,
    mut offscreen: ResMut<OffscreenRenderTarget>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Ok(resize) = inbox.resize.lock() else {
        return;
    };
    // Only the newest size matters.
    if let Some(last) = resize.try_iter().last() {
        views.insert(VIEW, manual(&last.view, last.size));
        offscreen.width = last.size[0];
        offscreen.height = last.size[1];
        if let Ok(mut window) = window.single_mut() {
            window
                .resolution
                .set_physical_resolution(last.size[0], last.size[1]);
        }
    }
}

#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn read_input(
    inbox: Res<Inbox>,
    surface: Option<Res<Surface>>,
    mut keys: MessageWriter<KeyboardInput>,
    mut focus_lost: MessageWriter<KeyboardFocusLost>,
    mut buttons: MessageWriter<MouseButtonInput>,
    mut motion: MessageWriter<MouseMotion>,
    mut wheels: MessageWriter<MouseWheel>,
    mut pointer: ResMut<PointerQueue>,
    mut scale: ResMut<UiScale>,
    mut windows: Query<&mut Window>,
) {
    let window = surface.map_or(Entity::PLACEHOLDER, |s| s.0);
    let Ok(input) = inbox.input.lock() else {
        return;
    };
    for event in input.try_iter() {
        match event {
            Input::Key {
                code,
                logical,
                pressed,
                repeat,
            } => {
                keys.write(KeyboardInput {
                    key_code: code,
                    logical_key: logical,
                    state: state(pressed),
                    text: None,
                    repeat,
                    window,
                });
            }
            Input::FocusLost => {
                focus_lost.write(KeyboardFocusLost);
            }
            Input::Button {
                button,
                pressed,
                at,
            } => {
                buttons.write(MouseButtonInput {
                    button,
                    state: state(pressed),
                    window,
                });
                if let Some(button) = pointer_button(button) {
                    let action = if pressed {
                        PointerAction::Press(button)
                    } else {
                        PointerAction::Release(button)
                    };
                    pointer.0.push((at, action));
                }
            }
            Input::Moved { at, delta } => {
                pointer.0.push((at, PointerAction::Move { delta }));
                // The window says where the pointer is, as winit's would:
                // the texture's pixels are the window's.
                if let Ok(mut surface) = windows.get_mut(window) {
                    surface.set_physical_cursor_position(Some(at.as_dvec2()));
                }
            }
            Input::Motion(delta) => {
                motion.write(MouseMotion { delta });
            }
            Input::Wheel { x, y, unit, at } => {
                wheels.write(MouseWheel {
                    unit,
                    x,
                    y,
                    window,
                    phase: TouchPhase::Moved,
                });
                pointer.0.push((
                    at,
                    PointerAction::Scroll {
                        unit,
                        x,
                        y,
                        phase: TouchPhase::Moved,
                    },
                ));
            }
            Input::Left => {
                pointer.1 = true;
                if let Ok(mut surface) = windows.get_mut(window) {
                    surface.set_physical_cursor_position(None);
                }
            }
            Input::Scale(points) => {
                if (scale.0 - points).abs() > f32::EPSILON {
                    scale.0 = points;
                }
                if let Ok(mut surface) = windows.get_mut(window)
                    && surface.resolution.scale_factor_override() != Some(points)
                {
                    surface.resolution.set_scale_factor_override(Some(points));
                }
            }
        }
    }
}

/// The pointer's moves and presses, on the world's texture: what picking
/// hit-tests against the cameras that render into it.
fn write_pointer(
    mut queue: ResMut<PointerQueue>,
    mut inputs: MessageWriter<PointerInput>,
    mut locations: Query<(&PointerId, &mut PointerLocation)>,
) {
    for (at, action) in queue.0.drain(..) {
        inputs.write(PointerInput::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::TextureView(VIEW),
                position: at,
            },
            action,
        ));
    }
    if std::mem::take(&mut queue.1) {
        for (id, mut location) in &mut locations {
            if *id == PointerId::Mouse {
                location.location = None;
            }
        }
    }
}

fn state(pressed: bool) -> ButtonState {
    if pressed {
        ButtonState::Pressed
    } else {
        ButtonState::Released
    }
}

fn pointer_button(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Right => Some(PointerButton::Secondary),
        MouseButton::Middle => Some(PointerButton::Middle),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// eframe's half
// ---------------------------------------------------------------------------

/// The world panel: the texture on screen, and a hand on the world.
pub struct Panel {
    link: PanelLink,
    target: Target,
    /// The texture before the last resize, alive until Bevy has drawn a
    /// frame into the new one.
    previous: Option<(Target, Instant)>,
    tex: egui::TextureId,
    /// The size the panel asked for, and since when.
    wanted: Option<([u32; 2], Instant)>,
    /// The modifiers Bevy last heard about, as keys.
    modifiers: egui::Modifiers,
    /// A press began in the world: the pointer is the world's until it lets go.
    held: bool,
    focused: bool,
    points: f32,
}

impl Panel {
    pub fn new(link: PanelLink, target: Target, tex: egui::TextureId) -> Self {
        Self {
            link,
            target,
            previous: None,
            tex,
            wanted: None,
            modifiers: egui::Modifiers::NONE,
            held: false,
            focused: false,
            points: 0.0,
        }
    }

    fn send(&self, input: Input) {
        if matches!(
            input,
            Input::Key { .. } | Input::FocusLost | Input::Button { .. }
        ) {
            tracing::debug!("world input: {input:?}");
        }
        let _ = self.link.input.send(input);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let rect = ui.available_rect_before_wrap();
        let id = ui.scope_id().with("world");
        let response = ui.interact(rect, id, egui::Sense::click_and_drag());
        ui.advance_cursor_after_rect(rect);
        let ppp = ui.ctx().pixels_per_point();
        if (ppp - self.points).abs() > f32::EPSILON {
            self.points = ppp;
            self.send(Input::Scale(ppp));
        }
        self.follow_size(rect, ppp, frame);
        ui.painter().image(
            self.tex,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        // The keyboard is the world's once it is clicked into, until a press
        // lands anywhere else: keys go where the last click went, as the
        // page's do, so a click in the timeline gives them to it.
        if response.clicked() || response.drag_started() || response.secondary_clicked() {
            response.request_focus();
        } else if response.has_focus()
            && ui.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
            })
        {
            response.surrender_focus();
        }
        let focused = response.has_focus();
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: false,
                    },
                )
            });
        }
        if self.focused && !focused {
            self.release_modifiers();
            self.send(Input::FocusLost);
        }
        if focused != self.focused {
            tracing::debug!("world panel focused: {focused}");
        }
        self.focused = focused;
        self.pointer(ui, rect, ppp, focused);
    }

    /// The texture follows the panel's size in pixels, once it holds still.
    fn follow_size(&mut self, rect: egui::Rect, ppp: f32, frame: &eframe::Frame) {
        let want = [
            (rect.width() * ppp).round().max(1.0) as u32,
            (rect.height() * ppp).round().max(1.0) as u32,
        ];
        if let Some((_, since)) = &self.previous
            && since.elapsed() > Duration::from_millis(500)
        {
            self.previous = None;
        }
        if want == self.target.size {
            self.wanted = None;
            return;
        }
        let since = match self.wanted {
            Some((size, since)) if size == want => since,
            _ => {
                self.wanted = Some((want, Instant::now()));
                return;
            }
        };
        if since.elapsed() < SETTLE {
            return;
        }
        let Some(render) = frame.wgpu_render_state() else {
            return;
        };
        let target = Target::new(&render.device, want);
        render
            .renderer
            .write()
            .update_egui_texture_from_wgpu_texture(
                &render.device,
                &target.view,
                wgpu::FilterMode::Linear,
                self.tex,
            );
        let _ = self.link.resize.send(Resize {
            view: target.view.clone(),
            size: target.size,
        });
        let old = std::mem::replace(&mut self.target, target);
        self.previous = Some((old, Instant::now()));
        self.wanted = None;
    }

    fn pointer(&mut self, ui: &egui::Ui, rect: egui::Rect, ppp: f32, focused: bool) {
        let to_pixels =
            |pos: egui::Pos2| Vec2::new((pos.x - rect.min.x) * ppp, (pos.y - rect.min.y) * ppp);
        let events = ui.input(|i| i.events.clone());
        let mut inside = false;
        for event in events {
            match event {
                egui::Event::PointerMoved(pos) => {
                    let here = rect.contains(pos);
                    if here || self.held {
                        inside = true;
                        let delta = ui.input(|i| i.pointer.delta()) * ppp;
                        self.send(Input::Moved {
                            at: to_pixels(pos),
                            delta: Vec2::new(delta.x, delta.y),
                        });
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } => {
                    let Some(button) = mouse_button(button) else {
                        continue;
                    };
                    if pressed && rect.contains(pos) {
                        self.held = true;
                    } else if !pressed && !self.held {
                        continue;
                    } else if !pressed && !ui.input(|i| i.pointer.any_down()) {
                        self.held = false;
                    }
                    if !pressed || rect.contains(pos) {
                        self.send(Input::Button {
                            button,
                            pressed,
                            at: to_pixels(pos),
                        });
                    }
                }
                egui::Event::MouseMoved(delta) => {
                    if self.held
                        || ui
                            .input(|i| i.pointer.hover_pos())
                            .is_some_and(|p| rect.contains(p))
                    {
                        self.send(Input::Motion(Vec2::new(delta.x, delta.y)));
                    }
                }
                egui::Event::MouseWheel { unit, delta, .. } => {
                    let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
                        continue;
                    };
                    if !rect.contains(pos) {
                        continue;
                    }
                    let (unit, x, y) = match unit {
                        egui::MouseWheelUnit::Point => {
                            (MouseScrollUnit::Pixel, delta.x * ppp, delta.y * ppp)
                        }
                        egui::MouseWheelUnit::Line => (MouseScrollUnit::Line, delta.x, delta.y),
                        egui::MouseWheelUnit::Page => {
                            (MouseScrollUnit::Line, delta.x * 10.0, delta.y * 10.0)
                        }
                    };
                    self.send(Input::Wheel {
                        x,
                        y,
                        unit,
                        at: to_pixels(pos),
                    });
                }
                egui::Event::PointerGone => self.send(Input::Left),
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers,
                } if focused => {
                    self.modifiers_now(modifiers);
                    if let Some(code) = key_code(physical_key.unwrap_or(key)) {
                        self.send(Input::Key {
                            code,
                            logical: logical(key),
                            pressed,
                            repeat,
                        });
                    }
                }
                egui::Event::ModifiersChanged(modifiers) if focused => {
                    self.modifiers_now(modifiers)
                }
                _ => {}
            }
        }
        if !inside
            && !self.held
            && ui
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|p| !rect.contains(p))
        {
            // The pointer is over a panel: picking hears it left.
            if ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                self.send(Input::Left);
            }
        }
    }

    /// egui reports the modifiers as a state; Bevy's code asks for them as
    /// keys held (`KeyCode::ShiftLeft`, `SuperLeft`, …).
    fn modifiers_now(&mut self, now: egui::Modifiers) {
        let pairs = [
            (
                self.modifiers.shift,
                now.shift,
                KeyCode::ShiftLeft,
                Key::Shift,
            ),
            (
                self.modifiers.ctrl,
                now.ctrl,
                KeyCode::ControlLeft,
                Key::Control,
            ),
            (self.modifiers.alt, now.alt, KeyCode::AltLeft, Key::Alt),
            (
                self.modifiers.mac_cmd,
                now.mac_cmd,
                KeyCode::SuperLeft,
                Key::Super,
            ),
        ];
        for (was, is, code, logical) in pairs {
            if was != is {
                self.send(Input::Key {
                    code,
                    logical,
                    pressed: is,
                    repeat: false,
                });
            }
        }
        self.modifiers = now;
    }

    fn release_modifiers(&mut self) {
        self.modifiers_now(egui::Modifiers::NONE);
    }
}

fn mouse_button(button: egui::PointerButton) -> Option<MouseButton> {
    match button {
        egui::PointerButton::Primary => Some(MouseButton::Left),
        egui::PointerButton::Secondary => Some(MouseButton::Right),
        egui::PointerButton::Middle => Some(MouseButton::Middle),
        egui::PointerButton::Extra1 => Some(MouseButton::Back),
        egui::PointerButton::Extra2 => Some(MouseButton::Forward),
    }
}

fn logical(key: egui::Key) -> Key {
    let name = key.name();
    match key {
        egui::Key::Enter => Key::Enter,
        egui::Key::Escape => Key::Escape,
        egui::Key::Tab => Key::Tab,
        egui::Key::Backspace => Key::Backspace,
        egui::Key::Delete => Key::Delete,
        egui::Key::Space => Key::Space,
        egui::Key::ArrowUp => Key::ArrowUp,
        egui::Key::ArrowDown => Key::ArrowDown,
        egui::Key::ArrowLeft => Key::ArrowLeft,
        egui::Key::ArrowRight => Key::ArrowRight,
        egui::Key::Home => Key::Home,
        egui::Key::End => Key::End,
        egui::Key::PageUp => Key::PageUp,
        egui::Key::PageDown => Key::PageDown,
        _ if name.chars().count() == 1 => Key::Character(name.to_lowercase().into()),
        _ => Key::Unidentified(NativeKey::Unidentified),
    }
}

/// egui's key, as the physical key Bevy names (`KeyW` is where W is on a
/// US layout, which is what Gen's fly camera means).
fn key_code(key: egui::Key) -> Option<KeyCode> {
    use egui::Key as E;
    Some(match key {
        E::ArrowDown => KeyCode::ArrowDown,
        E::ArrowLeft => KeyCode::ArrowLeft,
        E::ArrowRight => KeyCode::ArrowRight,
        E::ArrowUp => KeyCode::ArrowUp,
        E::Escape => KeyCode::Escape,
        E::Tab => KeyCode::Tab,
        E::Backspace => KeyCode::Backspace,
        E::Enter => KeyCode::Enter,
        E::Space => KeyCode::Space,
        E::Insert => KeyCode::Insert,
        E::Delete => KeyCode::Delete,
        E::Home => KeyCode::Home,
        E::End => KeyCode::End,
        E::PageUp => KeyCode::PageUp,
        E::PageDown => KeyCode::PageDown,
        E::Comma => KeyCode::Comma,
        E::Backslash => KeyCode::Backslash,
        E::Slash => KeyCode::Slash,
        E::OpenBracket => KeyCode::BracketLeft,
        E::CloseBracket => KeyCode::BracketRight,
        E::Backtick => KeyCode::Backquote,
        E::Minus => KeyCode::Minus,
        E::Period => KeyCode::Period,
        E::Equals => KeyCode::Equal,
        E::Semicolon => KeyCode::Semicolon,
        E::Quote => KeyCode::Quote,
        E::Num0 => KeyCode::Digit0,
        E::Num1 => KeyCode::Digit1,
        E::Num2 => KeyCode::Digit2,
        E::Num3 => KeyCode::Digit3,
        E::Num4 => KeyCode::Digit4,
        E::Num5 => KeyCode::Digit5,
        E::Num6 => KeyCode::Digit6,
        E::Num7 => KeyCode::Digit7,
        E::Num8 => KeyCode::Digit8,
        E::Num9 => KeyCode::Digit9,
        E::A => KeyCode::KeyA,
        E::B => KeyCode::KeyB,
        E::C => KeyCode::KeyC,
        E::D => KeyCode::KeyD,
        E::E => KeyCode::KeyE,
        E::F => KeyCode::KeyF,
        E::G => KeyCode::KeyG,
        E::H => KeyCode::KeyH,
        E::I => KeyCode::KeyI,
        E::J => KeyCode::KeyJ,
        E::K => KeyCode::KeyK,
        E::L => KeyCode::KeyL,
        E::M => KeyCode::KeyM,
        E::N => KeyCode::KeyN,
        E::O => KeyCode::KeyO,
        E::P => KeyCode::KeyP,
        E::Q => KeyCode::KeyQ,
        E::R => KeyCode::KeyR,
        E::S => KeyCode::KeyS,
        E::T => KeyCode::KeyT,
        E::U => KeyCode::KeyU,
        E::V => KeyCode::KeyV,
        E::W => KeyCode::KeyW,
        E::X => KeyCode::KeyX,
        E::Y => KeyCode::KeyY,
        E::Z => KeyCode::KeyZ,
        E::F1 => KeyCode::F1,
        E::F2 => KeyCode::F2,
        E::F3 => KeyCode::F3,
        E::F4 => KeyCode::F4,
        E::F5 => KeyCode::F5,
        E::F6 => KeyCode::F6,
        E::F7 => KeyCode::F7,
        E::F8 => KeyCode::F8,
        E::F9 => KeyCode::F9,
        E::F10 => KeyCode::F10,
        E::F11 => KeyCode::F11,
        E::F12 => KeyCode::F12,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fly_cameras_keys_arrive_as_bevy_names_them() {
        for (egui_key, code) in [
            (egui::Key::W, KeyCode::KeyW),
            (egui::Key::A, KeyCode::KeyA),
            (egui::Key::S, KeyCode::KeyS),
            (egui::Key::D, KeyCode::KeyD),
            (egui::Key::Q, KeyCode::KeyQ),
            (egui::Key::Space, KeyCode::Space),
            (egui::Key::Escape, KeyCode::Escape),
            (egui::Key::Delete, KeyCode::Delete),
        ] {
            assert_eq!(key_code(egui_key), Some(code), "{egui_key:?}");
        }
        assert_eq!(logical(egui::Key::W), Key::Character("w".into()));
    }

    #[test]
    fn a_press_of_each_mouse_button_is_a_pointer_press() {
        assert_eq!(
            pointer_button(MouseButton::Left),
            Some(PointerButton::Primary)
        );
        assert_eq!(
            pointer_button(MouseButton::Right),
            Some(PointerButton::Secondary)
        );
        assert_eq!(
            mouse_button(egui::PointerButton::Primary),
            Some(MouseButton::Left)
        );
    }
}
