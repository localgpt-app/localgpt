//! The camera a hand moves. One finger turns around the point the world's
//! camera looks at; two fingers pinch to come closer and drag to move that
//! point; a double tap goes back. Left alone for ten seconds it drifts round,
//! so a run with the iPad on the table keeps drawing a moving world.
// A finger on a button is told by its `Interaction` (see `hud`).
#![allow(deprecated)]

use bevy::prelude::*;

use crate::player::WorldCamera;

pub struct OrbitPlugin;

impl Plugin for OrbitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, orbit);
    }
}

/// Radians per point a finger drags.
const TURN: f32 = 0.006;
/// Radians per second while left alone.
const DRIFT: f32 = 0.06;
/// Seconds without a finger before the drift starts.
const IDLE: f32 = 10.0;
const PITCH: (f32, f32) = (-0.3, 1.45);
const DISTANCE: (f32, f32) = (2.0, 150.0);
/// How far a tap may move, and two taps lie apart, in points.
const TAP_SLOP: f32 = 12.0;
const DOUBLE_TAP: (f32, f32) = (0.35, 40.0);

#[derive(Component, Clone, Copy)]
pub struct Orbit {
    target: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    home: (Vec3, f32, f32, f32),
    idle: f32,
    last_tap: Option<(f32, Vec2)>,
}

impl Orbit {
    /// Around `at`, from `from`.
    pub fn looking(from: Vec3, at: Vec3) -> Self {
        let offset = from - at;
        let distance = offset.length().max(0.5);
        let yaw = offset.x.atan2(offset.z);
        let pitch = (offset.y / distance).clamp(-1.0, 1.0).asin();
        Self {
            target: at,
            yaw,
            pitch,
            distance,
            home: (at, yaw, pitch, distance),
            idle: 0.0,
            last_tap: None,
        }
    }

    /// Back to where the world's camera put it.
    pub fn home(&mut self) {
        (self.target, self.yaw, self.pitch, self.distance) = self.home;
    }

    pub fn transform(&self) -> Transform {
        let rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, -self.pitch, 0.0);
        Transform::from_translation(self.target + rotation * Vec3::new(0.0, 0.0, self.distance))
            .looking_at(self.target, Vec3::Y)
    }
}

fn orbit(
    time: Res<Time>,
    touches: Res<Touches>,
    buttons: Query<&Interaction>,
    mut cameras: Query<(&mut Orbit, &mut Transform), With<WorldCamera>>,
) {
    let Ok((mut orbit, mut transform)) = cameras.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    // A finger on a button is the button's.
    let on_button = buttons.iter().any(|i| *i != Interaction::None);
    let fingers: Vec<_> = touches.iter().collect();
    if on_button || fingers.is_empty() {
        orbit.idle += dt;
        if orbit.idle > IDLE {
            orbit.yaw += DRIFT * dt;
        }
    } else {
        orbit.idle = 0.0;
        match fingers.as_slice() {
            [one] => {
                let moved = one.delta();
                orbit.yaw -= moved.x * TURN;
                orbit.pitch = (orbit.pitch + moved.y * TURN).clamp(PITCH.0, PITCH.1);
            }
            [a, b, ..] => {
                let span = a.position().distance(b.position());
                let before = a.previous_position().distance(b.previous_position());
                if span > 1.0 && before > 1.0 {
                    orbit.distance = (orbit.distance * before / span).clamp(DISTANCE.0, DISTANCE.1);
                }
                // The world follows the fingers.
                let moved = (a.delta() + b.delta()) * 0.5;
                let scale = orbit.distance * 0.0016;
                let shift = (-transform.right() * moved.x + transform.up() * moved.y) * scale;
                orbit.target += shift;
            }
            [] => {}
        }
    }
    if !on_button {
        for released in touches.iter_just_released() {
            if released.distance().length() > TAP_SLOP {
                continue;
            }
            let now = time.elapsed_secs();
            match orbit.last_tap {
                Some((at, place))
                    if now - at < DOUBLE_TAP.0
                        && place.distance(released.position()) < DOUBLE_TAP.1 =>
                {
                    orbit.home();
                    orbit.last_tap = None;
                }
                _ => orbit.last_tap = Some((now, released.position())),
            }
        }
    }
    *transform = orbit.transform();
}
