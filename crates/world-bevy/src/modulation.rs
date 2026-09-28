//! Signal-driven modulation at runtime — the Bevy half of
//! [`wt::ModulationDef`], evaluated the way the web viewer's
//! `applyModulations` does, so a world performs its song the same way in
//! every renderer:
//!
//! - signals are read at the song's playback position while it plays, else
//!   at the world clock looped over the song's length, else at the world
//!   clock ([`Soundtrack::clock`]);
//! - `energy`, `beat` and the stems read the soundtrack's curves; `bass` and
//!   `highs` read live band levels while the song plays and something
//!   analyses it, else their stem, else `energy`; oscillators and constants
//!   need no soundtrack ([`Soundtrack::signal`]);
//! - a signal nothing provides leaves its target as authored;
//! - several modulations on one target combine: factors multiply, offsets
//!   add.
//!
//! An app adds [`ModulationPlugin`], inserts the component [`modulated`]
//! returns when it spawns an entity that has modulations, and keeps
//! [`Soundtrack`] in step with the world it loaded and the song it plays.
//!
//! **Modulation is only ever seen by the renderer.** [`ModulationSet::Restore`]
//! runs in `PreUpdate` and takes the last frame's modulation back out;
//! [`ModulationSet::Apply`] runs in `PostUpdate`, before transforms
//! propagate, and puts this frame's in. So everything in `Update` — tools,
//! behaviours, saving a world, projecting it into a shared session — sees
//! authored values, and a save never records a scale caught mid-beat. A
//! value changed during the frame is simply the new authored value, so
//! modulation never fights an edit.

use bevy::light::{DirectionalLight, PointLight, SpotLight};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use localgpt_world_types as wt;

/// Where soundtrack signals come from.
#[derive(Resource, Debug, Clone, Default)]
pub struct Soundtrack {
    /// The loaded world's soundtrack, if it has one.
    pub def: Option<wt::SoundtrackDef>,
    /// The song's playback position in seconds, while it plays.
    pub position: Option<f32>,
    /// Live band levels while the song plays, if something analyses it.
    pub live: Option<LiveBands>,
}

/// Live band levels, `0..=1`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LiveBands {
    pub bass: f32,
    pub highs: f32,
}

impl Soundtrack {
    /// The time signals are read at, given the world clock in seconds.
    pub fn clock(&self, elapsed: f32) -> f32 {
        if let Some(position) = self.position {
            return position;
        }
        match &self.def {
            Some(def) if def.duration > 0.0 => elapsed.rem_euclid(def.duration),
            _ => elapsed,
        }
    }

    /// A signal's raw value at `t`, or `None` when nothing provides it.
    pub fn signal(&self, signal: &wt::SignalSource, t: f32) -> Option<f32> {
        if let Some(value) = signal.free_running_at(t) {
            return Some(value);
        }
        let def = self.def.as_ref()?;
        let live = self.live.filter(|_| self.position.is_some());
        Some(match signal {
            wt::SignalSource::Energy => def.energy_at(t),
            wt::SignalSource::Beat => def.beat_at(t),
            wt::SignalSource::Bass => match live {
                Some(live) => live.bass,
                None => def.stem_at(wt::StemKind::Bass, t),
            },
            wt::SignalSource::Highs => match live {
                Some(live) => live.highs,
                None => def.stem_at(wt::StemKind::Other, t),
            },
            wt::SignalSource::Stem(stem) => def.stem_at(*stem, t),
            wt::SignalSource::Oscillator { .. } | wt::SignalSource::Constant(_) => return None,
        })
    }
}

/// The combined effect of an entity's modulations for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Factors {
    pub scale: f32,
    pub offset_y: f32,
    pub emissive: f32,
    pub light_intensity: f32,
    pub opacity: f32,
}

impl Default for Factors {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset_y: 0.0,
            emissive: 1.0,
            light_intensity: 1.0,
            opacity: 1.0,
        }
    }
}

/// An entity's modulations, each with its smoothed signal, and what the last
/// frame changed — `(before, after)` per target — so the next can take it
/// back out.
#[derive(Component, Debug, Clone, Default)]
pub struct Modulated {
    mods: Vec<(wt::ModulationDef, f32)>,
    offset_y: Option<(f32, f32)>,
    scale: Option<(Vec3, Vec3)>,
    emissive: Option<(LinearRgba, LinearRgba)>,
    alpha: Option<(f32, f32)>,
    light: Option<(f32, f32)>,
}

impl Modulated {
    /// The modulations, as authored — for saving the entity back out.
    pub fn defs(&self) -> impl Iterator<Item = &wt::ModulationDef> {
        self.mods.iter().map(|(def, _)| def)
    }

    fn drives(&self, target: wt::ModulationTarget) -> bool {
        self.mods.iter().any(|(def, _)| def.target == target)
    }

    /// Advance every signal by `dt` and combine the factors.
    pub fn step(&mut self, soundtrack: &Soundtrack, t: f32, dt: f32) -> Factors {
        let mut factors = Factors::default();
        for (def, smoothed) in &mut self.mods {
            let factor = match soundtrack.signal(&def.signal, t) {
                Some(raw) => {
                    *smoothed = def.smooth(*smoothed, raw, dt);
                    def.factor(*smoothed)
                }
                None => def.inactive_factor(),
            };
            match def.target {
                wt::ModulationTarget::Scale => factors.scale *= factor,
                wt::ModulationTarget::OffsetY => factors.offset_y += factor,
                wt::ModulationTarget::Emissive => factors.emissive *= factor,
                wt::ModulationTarget::LightIntensity => factors.light_intensity *= factor,
                wt::ModulationTarget::Opacity => factors.opacity *= factor,
            }
        }
        factors
    }
}

/// The component for an entity's modulations; `None` when it has no valid
/// ones.
pub fn modulated(entity: &wt::WorldEntity) -> Option<Modulated> {
    let mods: Vec<_> = entity
        .modulations
        .iter()
        .filter(|def| def.is_valid())
        .map(|def| (def.clone(), 0.0))
        .collect();
    (!mods.is_empty()).then(|| Modulated {
        mods,
        ..Default::default()
    })
}

/// The two halves of a frame's modulation; see the module docs.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModulationSet {
    Restore,
    Apply,
}

/// Evaluates every [`Modulated`] entity each frame.
pub struct ModulationPlugin;

impl Plugin for ModulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Soundtrack>()
            .configure_sets(
                PostUpdate,
                ModulationSet::Apply.before(TransformSystems::Propagate),
            )
            .add_systems(PreUpdate, restore.in_set(ModulationSet::Restore))
            .add_systems(PostUpdate, apply.in_set(ModulationSet::Apply));
    }
}

type Lights<'a> = (
    Option<&'a mut PointLight>,
    Option<&'a mut SpotLight>,
    Option<&'a mut DirectionalLight>,
);

/// Put back what the last frame changed, where nobody has changed it since.
fn restore(
    mut entities: Query<(
        &mut Modulated,
        &mut Transform,
        Option<&MeshMaterial3d<StandardMaterial>>,
        Lights,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mut modulated, mut transform, material, (point, spot, directional)) in &mut entities {
        if let Some((before, after)) = modulated.offset_y.take()
            && transform.translation.y == after
        {
            transform.translation.y = before;
        }
        if let Some((before, after)) = modulated.scale.take()
            && transform.scale == after
        {
            transform.scale = before;
        }
        let emissive = modulated.emissive.take();
        let alpha = modulated.alpha.take();
        if let Some(handle) = material
            && (emissive.is_some() || alpha.is_some())
            && let Some(current) = materials.get(&handle.0)
        {
            let restore_emissive = emissive.filter(|(_, after)| current.emissive == *after);
            let restore_alpha = alpha.filter(|(_, after)| current.base_color.alpha() == *after);
            if (restore_emissive.is_some() || restore_alpha.is_some())
                && let Some(mut material) = materials.get_mut(&handle.0)
            {
                if let Some((before, _)) = restore_emissive {
                    material.emissive = before;
                }
                if let Some((before, _)) = restore_alpha {
                    material.base_color.set_alpha(before);
                }
            }
        }
        if let Some((before, after)) = modulated.light.take() {
            if let Some(mut light) = point
                && light.intensity == after
            {
                light.intensity = before;
            } else if let Some(mut light) = spot
                && light.intensity == after
            {
                light.intensity = before;
            } else if let Some(mut light) = directional
                && light.illuminance == after
            {
                light.illuminance = before;
            }
        }
    }
}

/// Evaluate the signals and apply this frame's modulation.
fn apply(
    time: Res<Time>,
    soundtrack: Res<Soundtrack>,
    mut entities: Query<(
        &mut Modulated,
        &mut Transform,
        Option<&MeshMaterial3d<StandardMaterial>>,
        Lights,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    let t = soundtrack.clock(time.elapsed_secs());
    for (mut modulated, mut transform, material, (point, spot, directional)) in &mut entities {
        let factors = modulated.step(&soundtrack, t, dt);

        if modulated.drives(wt::ModulationTarget::OffsetY) {
            let before = transform.translation.y;
            transform.translation.y = before + factors.offset_y;
            modulated.offset_y = Some((before, transform.translation.y));
        }
        if modulated.drives(wt::ModulationTarget::Scale) {
            let before = transform.scale;
            transform.scale = before * factors.scale;
            modulated.scale = Some((before, transform.scale));
        }

        let drives_emissive = modulated.drives(wt::ModulationTarget::Emissive);
        let drives_opacity = modulated.drives(wt::ModulationTarget::Opacity);
        if (drives_emissive || drives_opacity)
            && let Some(handle) = material
            && let Some(mut material) = materials.get_mut(&handle.0)
        {
            if drives_emissive {
                let before = material.emissive;
                material.emissive = before * factors.emissive;
                modulated.emissive = Some((before, material.emissive));
            }
            if drives_opacity {
                let before = material.base_color.alpha();
                material.base_color.set_alpha(before * factors.opacity);
                // Fading needs blending; an opaque material stays opaque
                // whatever its alpha. Set once, as the web viewer does.
                if material.alpha_mode == AlphaMode::Opaque {
                    material.alpha_mode = AlphaMode::Blend;
                }
                modulated.alpha = Some((before, material.base_color.alpha()));
            }
        }

        if modulated.drives(wt::ModulationTarget::LightIntensity) {
            let factor = factors.light_intensity;
            modulated.light = if let Some(mut light) = point {
                let before = light.intensity;
                light.intensity = before * factor;
                Some((before, light.intensity))
            } else if let Some(mut light) = spot {
                let before = light.intensity;
                light.intensity = before * factor;
                Some((before, light.intensity))
            } else if let Some(mut light) = directional {
                let before = light.illuminance;
                light.illuminance = before * factor;
                Some((before, light.illuminance))
            } else {
                None
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn entity(
        target: wt::ModulationTarget,
        signal: wt::SignalSource,
        range: [f32; 2],
    ) -> wt::WorldEntity {
        let mut entity = wt::WorldEntity::new(1, "beacon");
        entity
            .modulations
            .push(wt::ModulationDef::new(target, signal, range));
        entity
    }

    fn song(bpm: f32, energy: Vec<f32>) -> wt::SoundtrackDef {
        wt::SoundtrackDef {
            duration: energy.len() as f32,
            bpm,
            energy,
            ..Default::default()
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(ModulationPlugin)
            .init_resource::<Time>()
            .init_resource::<Assets<StandardMaterial>>();
        app
    }

    fn tick(app: &mut App, seconds: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(seconds));
        app.update();
    }

    #[test]
    fn the_clock_follows_the_song_then_loops_its_length() {
        let mut soundtrack = Soundtrack::default();
        assert_eq!(soundtrack.clock(75.0), 75.0, "no song: the world clock");
        soundtrack.def = Some(song(120.0, vec![0.0; 30]));
        assert_eq!(soundtrack.clock(75.0), 15.0, "looped over 30 s");
        soundtrack.position = Some(4.5);
        assert_eq!(soundtrack.clock(75.0), 4.5, "playing: the song's position");
    }

    #[test]
    fn signals_without_a_source_are_none_and_free_ones_need_none() {
        let silent = Soundtrack::default();
        assert_eq!(silent.signal(&wt::SignalSource::Energy, 1.0), None);
        assert_eq!(
            silent.signal(&wt::SignalSource::Constant(0.25), 1.0),
            Some(0.25)
        );

        let soundtrack = Soundtrack {
            def: Some(song(60.0, vec![0.0, 1.0, 0.5])),
            ..Default::default()
        };
        assert_eq!(soundtrack.signal(&wt::SignalSource::Energy, 1.0), Some(1.0));
        assert_eq!(soundtrack.signal(&wt::SignalSource::Beat, 0.0), Some(1.0));
        // No stems: a stem falls back to the energy curve.
        let drums = wt::SignalSource::Stem(wt::StemKind::Drums);
        assert_eq!(soundtrack.signal(&drums, 2.0), Some(0.5));
        // Live bands only count while the song plays.
        let live = Soundtrack {
            live: Some(LiveBands {
                bass: 0.9,
                highs: 0.1,
            }),
            ..soundtrack.clone()
        };
        assert_eq!(live.signal(&wt::SignalSource::Bass, 1.0), Some(1.0));
        let playing = Soundtrack {
            position: Some(1.0),
            ..live
        };
        assert_eq!(playing.signal(&wt::SignalSource::Bass, 1.0), Some(0.9));
    }

    #[test]
    fn several_modulations_on_one_target_multiply_and_offsets_add() {
        let mut entity = entity(
            wt::ModulationTarget::Scale,
            wt::SignalSource::Constant(1.0),
            [1.0, 2.0],
        );
        for def in [
            wt::ModulationDef::new(
                wt::ModulationTarget::Scale,
                wt::SignalSource::Constant(1.0),
                [1.0, 3.0],
            ),
            wt::ModulationDef::new(
                wt::ModulationTarget::OffsetY,
                wt::SignalSource::Constant(1.0),
                [0.0, 0.5],
            ),
            wt::ModulationDef::new(
                wt::ModulationTarget::OffsetY,
                wt::SignalSource::Energy,
                [0.0, 9.0],
            ),
        ] {
            entity.modulations.push(def);
        }
        let factors = modulated(&entity)
            .unwrap()
            .step(&Soundtrack::default(), 0.0, 0.016);
        assert_eq!(factors.scale, 6.0);
        assert_eq!(
            factors.offset_y, 0.5,
            "energy has no source: it adds nothing"
        );
    }

    #[test]
    fn update_systems_see_authored_values() {
        #[derive(Resource, Default)]
        struct Seen(Vec<f32>);
        let mut app = app();
        app.init_resource::<Seen>().add_systems(
            Update,
            |q: Query<&Transform, With<Modulated>>, mut seen: ResMut<Seen>| {
                seen.0.extend(q.iter().map(|t| t.scale.x));
            },
        );
        let def = entity(
            wt::ModulationTarget::Scale,
            wt::SignalSource::Constant(1.0),
            [1.0, 3.0],
        );
        let modulated = modulated(&def).unwrap();
        assert_eq!(modulated.defs().count(), 1);
        app.world_mut()
            .spawn((Transform::from_scale(Vec3::splat(2.0)), modulated));
        for _ in 0..3 {
            tick(&mut app, 0.016);
        }
        assert_eq!(app.world().resource::<Seen>().0, [2.0, 2.0, 2.0]);
    }

    #[test]
    fn scale_is_modulated_every_frame_without_compounding() {
        let mut app = app();
        let def = entity(
            wt::ModulationTarget::Scale,
            wt::SignalSource::Constant(1.0),
            [1.0, 3.0],
        );
        let id = app
            .world_mut()
            .spawn((
                Transform::from_scale(Vec3::splat(2.0)),
                modulated(&def).unwrap(),
            ))
            .id();
        let scale = |app: &App| app.world().get::<Transform>(id).unwrap().scale.x;

        tick(&mut app, 0.016);
        assert_eq!(scale(&app), 6.0);
        tick(&mut app, 0.016);
        assert_eq!(scale(&app), 6.0, "restored before it is applied again");

        // An edit between frames is the new authored value, not undone.
        app.world_mut().get_mut::<Transform>(id).unwrap().scale = Vec3::ONE;
        tick(&mut app, 0.016);
        assert_eq!(scale(&app), 3.0);
    }

    #[test]
    fn a_beat_drives_light_and_emissive_from_their_authored_values() {
        let mut app = app();
        app.world_mut().resource_mut::<Soundtrack>().def = Some(song(60.0, vec![0.5; 10]));
        let material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                emissive: LinearRgba::rgb(1.0, 0.5, 0.0),
                ..Default::default()
            });
        let mut def = entity(
            wt::ModulationTarget::Emissive,
            wt::SignalSource::Beat,
            [0.0, 2.0],
        );
        def.modulations.push(wt::ModulationDef::new(
            wt::ModulationTarget::LightIntensity,
            wt::SignalSource::Energy,
            [0.0, 4.0],
        ));
        let id = app
            .world_mut()
            .spawn((
                Transform::default(),
                MeshMaterial3d(material.clone()),
                PointLight {
                    intensity: 1000.0,
                    ..Default::default()
                },
                modulated(&def).unwrap(),
            ))
            .id();

        for _ in 0..3 {
            tick(&mut app, 0.25);
        }
        let light = app.world().get::<PointLight>(id).unwrap().intensity;
        assert_eq!(
            light, 2000.0,
            "energy 0.5 over [0, 4] doubles it, every frame"
        );
        let t = app.world().resource::<Time>().elapsed_secs();
        let beat = song(60.0, vec![0.5; 10]).beat_at(t);
        let emissive = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .unwrap()
            .emissive;
        assert!(
            (emissive.red - 2.0 * beat).abs() < 1e-5,
            "{emissive:?} at beat {beat}"
        );
    }
}
