//! A placeholder 3D world for the HUD to overlay.
//!
//! This is intentionally abstract — the real pipeline assembles worlds from a
//! catalog of glTF assets driven by music (see `idea.md`). For the UI-first
//! milestone we just need a living, mood-tinted backdrop: a ground plane, a
//! field of slowly drifting shapes, fog, and two camera feels (Explore / Drift)
//! so the chrome always has "a live world to justify itself against".

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::pbr::DistanceFog;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::playback::{Beat, Playback};
use crate::theme::Theme;
use crate::{CameraMode, OverlayStack, Paused, QueueOpen, WorldClock};

/// Marker for the single world camera.
#[derive(Component)]
pub struct WorldCamera {
    /// Yaw/pitch used in Explore mode.
    pub yaw: f32,
    pub pitch: f32,
    /// Explore fly speed in m/s; the scroll wheel trims it while locked.
    pub fly_speed: f32,
    /// Auto-orbit angle used in Drift mode.
    pub orbit: f32,
    /// Eased orbit radius/height for Drift choreography (metres).
    pub eased_radius: f32,
    pub eased_height: f32,
}

/// A drifting decorative shape.
#[derive(Component)]
pub struct Drifter {
    pub seed: f32,
    pub base: Vec3,
}

/// An ambient particle (M7 recipe `particles`). Drifts upward or downward per
/// `kind`, recycled when it leaves the volume. Spawned/refreshed on recipe
/// change by [`spawn_particles`]; animated by [`animate_world`].
#[derive(Component)]
pub struct Particle {
    pub kind: crate::recipe::ParticleKind,
    pub seed: f32,
    /// Home position (kept for a future "settle back" reset; recycle currently
    /// clamps Y in place).
    #[allow(dead_code)]
    pub base: Vec3,
    /// Per-particle drift speed multiplier.
    pub drift: f32,
}

/// Half-extent of the particle volume (x/z spread, max height).
const PARTICLE_SPREAD: f32 = 30.0;
const PARTICLE_MAX_Y: f32 = 18.0;

/// The live particle field's shared material handle + tint, published by
/// [`spawn_particles`] so [`animate_world`] can breathe the field's emissive
/// with the live band envelopes without a per-particle cost. `None` when no
/// field is spawned (rate 0 / no recipe).
#[derive(Resource, Default)]
pub struct ParticleField {
    pub material: Option<Handle<StandardMaterial>>,
    pub tint: Color,
}

/// The ground as an instrument: a subdivided plane displaced by a radial wave
/// field whose amplitude rides the bass (the Demucs curve when present, else
/// the live band). Marked so [`ground_waves`] owns its vertices.
#[derive(Component)]
pub struct GroundWaves;

/// Displace the ground each frame with a bass-driven wave field — a radial
/// ripple from the world's centre plus a slower diagonal swell, both faded by
/// distance so the rim stays still. Comfort › gentler world motion halves the
/// amplitude; pause freezes it with everything else (world-clock driven).
pub fn ground_waves(
    time: Res<Time>,
    clock: Res<WorldClock>,
    beat: Res<Beat>,
    stems: Res<crate::playback::StemLevels>,
    comfort: Res<crate::Comfort>,
    mut meshes: ResMut<Assets<Mesh>>,
    ground: Query<&Mesh3d, With<GroundWaves>>,
) {
    let Ok(handle) = ground.single() else {
        return;
    };
    // Copy the base positions out first — the attribute borrow must end
    // before `insert_attribute` mutates the mesh. ~4k verts, cheap.
    let verts: Vec<[f32; 3]> = {
        let Some(mesh) = meshes.get(&handle.0) else {
            return;
        };
        let Some(values) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            return;
        };
        values.as_float3().unwrap_or(&[]).to_vec()
    };

    let t = time.elapsed_secs() * clock.speed;
    let gentle = if comfort.gentler_motion { 0.4 } else { 1.0 };
    let bass = stems.0[1].max(beat.bass);
    let amp = (0.10 + bass * 0.85) * gentle;
    let moved: Vec<[f32; 3]> = verts
        .iter()
        .map(|v| {
            let (x, z) = (v[0], v[2]);
            let r = (x * x + z * z).sqrt();
            let env = 1.0 / (1.0 + r * 0.05);
            let ripple = (r * 0.55 - t * 2.2).sin() * 0.7;
            let swell = (x * 0.12 + t * 0.8).sin() * (z * 0.10 - t * 0.6).sin() * 0.45;
            [x, -0.5 + amp * (ripple + swell) * env, z]
        })
        .collect();
    if let Some(mut mesh) = meshes.get_mut(&handle.0) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, moved);
    }
}

/// Handles to the shared world materials so palette swaps are cheap.
#[derive(Resource)]
pub struct WorldMaterials {
    pub ground: Handle<StandardMaterial>,
    pub drifter: Handle<StandardMaterial>,
}

/// Eased 0..1 visibility for the pause "held ring" (spec 1e): eases in over
/// ~220ms while the transport is frozen, out over ~320ms on resume.
#[derive(Resource, Default)]
pub struct HeldRing(pub f32);

/// The quiet ring that appears while the transport is frozen (spec 1e).
#[derive(Component)]
pub struct HeldRingMesh;

pub fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    theme: Res<Theme>,
) {
    let mood = theme.current();

    // Camera — HDR + bloom give the "HDR bloom seed" glow the spec asks for.
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        DistanceFog {
            color: mood.fog,
            falloff: FogFalloff::Linear {
                start: 18.0,
                end: 95.0,
            },
            ..default()
        },
        Transform::from_xyz(0.0, 3.0, 14.0).looking_at(Vec3::new(0.0, 1.5, 0.0), Vec3::Y),
        // Ambient light is a per-camera component in Bevy 0.19.
        AmbientLight {
            color: mood.ambient,
            brightness: 260.0,
            ..default()
        },
        WorldCamera {
            yaw: 0.0,
            pitch: -0.08,
            fly_speed: 7.0,
            orbit: 0.0,
            eased_radius: 16.0,
            eased_height: 3.0,
        },
    ));

    // Key light.
    commands.spawn((
        DirectionalLight {
            illuminance: 6000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(6.0, 12.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Shared materials.
    let ground_mat = materials.add(StandardMaterial {
        base_color: mood.ground,
        perceptual_roughness: 0.95,
        ..default()
    });
    let drifter_mat = materials.add(StandardMaterial {
        base_color: mood.sky_bottom,
        emissive: scaled_linear(mood.accent, 0.6),
        perceptual_roughness: 0.35,
        metallic: 0.1,
        ..default()
    });
    commands.insert_resource(WorldMaterials {
        ground: ground_mat.clone(),
        drifter: drifter_mat.clone(),
    });

    // Ground — subdivided so the wave field has vertices to displace.
    commands.spawn((
        GroundWaves,
        Mesh3d(
            meshes.add(
                Plane3d::default()
                    .mesh()
                    .size(240.0, 240.0)
                    .subdivisions(64),
            ),
        ),
        MeshMaterial3d(ground_mat),
        Transform::from_xyz(0.0, -0.5, 0.0),
    ));

    // The pause "held ring" (spec 1e): an unlit emissive torus that eases in
    // while the transport is frozen. `sync_held_ring` keeps it ahead of the
    // camera and drives its alpha from the `HeldRing` resource.
    commands.spawn((
        HeldRingMesh,
        Mesh3d(meshes.add(Torus::new(1.1, 1.125).mesh().minor_resolution(64))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: crate::theme::TEXT.with_alpha(0.0),
            emissive: scaled_linear(crate::theme::TEXT, 0.7),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::from_xyz(0.0, 2.0, 5.0),
    ));

    // A field of drifting shapes, placed on a deterministic golden-angle
    // spiral so no RNG dependency is needed.
    let cube = meshes.add(Cuboid::new(0.7, 0.7, 0.7));
    let sphere = meshes.add(Sphere::new(0.5).mesh().ico(3).unwrap());
    let tet = meshes.add(Tetrahedron::default().mesh());
    let count = 90;
    for i in 0..count {
        let fi = i as f32;
        let ang = fi * 2.399_963; // golden angle
        let radius = (fi + 4.0).sqrt() * 3.2;
        let x = ang.cos() * radius;
        let z = ang.sin() * radius - 8.0;
        let y = 1.0 + ((fi * 1.7).sin() * 0.5 + 0.5) * 5.0;
        let base = Vec3::new(x, y, z);
        let mesh = match i % 3 {
            0 => cube.clone(),
            1 => sphere.clone(),
            _ => tet.clone(),
        };
        let scale = 0.6 + (fi * 0.37).fract() * 1.3;
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(drifter_mat.clone()),
            Transform::from_translation(base).with_scale(Vec3::splat(scale)),
            Drifter {
                seed: fi * 0.613,
                base,
            },
        ));
    }
}

/// One paintable snapshot of the world's colours.
#[derive(Clone, Copy)]
struct Palette {
    sky_top: Color,
    ambient: Color,
    fog: Color,
    ground: Color,
    drift_base: Color,
    accent: Color,
}

impl Palette {
    fn of(mood: &crate::theme::WorldMood) -> Self {
        Self {
            sky_top: mood.sky_top,
            ambient: mood.ambient,
            fog: mood.fog,
            ground: mood.ground,
            drift_base: mood.sky_bottom,
            accent: mood.accent,
        }
    }

    fn mix(&self, to: &Self, f: f32) -> Self {
        use bevy::color::Mix as _;
        Self {
            sky_top: self.sky_top.mix(&to.sky_top, f),
            ambient: self.ambient.mix(&to.ambient, f),
            fog: self.fog.mix(&to.fog, f),
            ground: self.ground.mix(&to.ground, f),
            drift_base: self.drift_base.mix(&to.drift_base, f),
            accent: self.accent.mix(&to.accent, f),
        }
    }
}

/// The current section's feel, as authored by the recipe's choreography or
/// (without one) by the section's position. [`sync_section_moment`] writes
/// these as *targets* and eases toward them every frame, so structural verbs
/// land as movements rather than snaps.
#[derive(Resource)]
pub struct SectionFeel {
    /// Additive energy shift (-1..1) for the current section: negative calms
    /// the beat glow, positive intensifies it.
    pub energy_shift: f32,
    /// Motion multiplier for the current section (Calm 0.5 / Drift 1.0 /
    /// Active 1.6), applied on top of the recipe's `motion_speed`.
    pub motion: f32,
    /// Structural verb — beacon brightness multiplier: the chorus lights the
    /// landmarks up, the bridge dims them.
    pub beacons: f32,
    /// Structural verb — particle-field presence multiplier: the chorus
    /// swells the field, the bridge strips it to near-nothing.
    pub particles: f32,
    /// Structural verb — scatter-prop scale multiplier: the outro sinks the
    /// ground cover back into the ground it rose from.
    pub scatter: f32,
}

impl Default for SectionFeel {
    fn default() -> Self {
        Self {
            energy_shift: 0.0,
            motion: 1.0,
            beacons: 1.0,
            particles: 1.0,
            scatter: 1.0,
        }
    }
}

impl SectionFeel {
    /// The role's structural verbs — what the world *does* in a section, over
    /// and above the recipe's numeric modulation. Without the `llm` tier
    /// these still fire, from the section's position in the song.
    fn verbs(role: Option<crate::recipe::SectionRole>) -> Self {
        use crate::recipe::SectionRole as R;
        let mut f = Self::default();
        match role {
            Some(R::Intro) => {
                f.particles = 0.6;
                f.beacons = 0.7;
            }
            Some(R::Chorus) | Some(R::Drop) => {
                f.particles = 1.35;
                f.beacons = 1.4;
                f.scatter = 1.15;
            }
            Some(R::Bridge) => {
                f.particles = 0.15;
                f.beacons = 0.4;
            }
            Some(R::Outro) => {
                f.particles = 0.3;
                f.beacons = 0.6;
                f.scatter = 0.0;
            }
            _ => {}
        }
        f
    }

    fn ease_toward(&mut self, target: &Self, k: f32) {
        self.energy_shift += (target.energy_shift - self.energy_shift) * k;
        self.motion += (target.motion - self.motion) * k;
        self.beacons += (target.beacons - self.beacons) * k;
        self.particles += (target.particles - self.particles) * k;
        self.scatter += (target.scatter - self.scatter) * k;
    }
}

/// Positional role for a segment when no choreography moment targets it —
/// the shape of most songs, so the rule path gets verbs too.
fn positional_role(segment: usize, segments: usize) -> Option<crate::recipe::SectionRole> {
    use crate::recipe::SectionRole as R;
    if segments <= 2 {
        return (segment == 0).then_some(R::Intro);
    }
    if segment == 0 {
        Some(R::Intro)
    } else if segment == segments - 1 {
        Some(R::Outro)
    } else if segment == segments / 2 {
        Some(R::Chorus)
    } else if segment == (segments * 3) / 4 {
        Some(R::Bridge)
    } else {
        Some(R::Verse)
    }
}

/// Apply the recipe's section choreography (M7) and the structural verbs:
/// when the transport crosses into a new measured section, compute that
/// section's target feel — the moment's numeric modulation plus the role's
/// verbs — and ease toward it. A moment flagged `palette_wash` re-runs the
/// 0.8s colour breathe the crossfade already uses. Sections are fractions of
/// the track ([`Playback::sections`]); the moment→section mapping was
/// resolved against the full analysis by `sync_analysis`
/// ([`crate::recipe::ActiveRecipe::moments`]).
pub fn sync_section_moment(
    time: Res<Time>,
    playback: Res<Playback>,
    active_recipe: Res<crate::recipe::ActiveRecipe>,
    mut feel: ResMut<SectionFeel>,
    mut wash: ResMut<PaletteWash>,
    mut last: Local<Option<usize>>,
    mut last_track: Local<Option<Option<String>>>,
) {
    if playback.sections.is_empty() {
        return;
    }
    // A new track re-enters segment 0 — reset the cache so its intro moment
    // (if any) applies instead of being swallowed by the previous track's.
    let track = playback.track().id.clone();
    if *last_track != Some(track.clone()) {
        *last_track = Some(track);
        *last = None;
    }
    let frac = playback.fraction();
    // Boundaries include 0.0, so the segment index is the count of boundaries
    // at-or-before the playhead, minus that leading 0.0.
    let segment = playback
        .sections
        .iter()
        .filter(|&&s| s <= frac)
        .count()
        .saturating_sub(1);
    let segments = playback.sections.len();
    let entered = *last != Some(segment);
    if entered {
        *last = Some(segment);
    }

    // The target feel: the recipe's moment if one targets this segment,
    // else positional verbs so sections still *do* something without the
    // LLM tier.
    let mut target = match active_recipe
        .moments
        .iter()
        .find(|(idx, _)| *idx == segment)
    {
        Some((_, m)) => {
            if entered && m.palette_wash {
                // Re-run the wash to the current palette — a colour breathe.
                wash.from = wash.current;
                wash.t = 0.0;
                wash.active = true;
            }
            let mut f = SectionFeel::verbs(Some(m.at_role));
            f.energy_shift = m.energy_shift;
            f.motion = match m.motion {
                crate::recipe::Motion::Calm => 0.5,
                crate::recipe::Motion::Drift => 1.0,
                crate::recipe::Motion::Active => 1.6,
            };
            f
        }
        None => SectionFeel::verbs(positional_role(segment, segments)),
    };
    // The outro sinks scatter regardless of who authored the feel — the
    // world should always let go at the end.
    if segment == segments.saturating_sub(1) && segments > 2 {
        target.scatter = target.scatter.min(0.0);
    }
    feel.ease_toward(&target, (time.delta_secs() * 2.5).min(1.0));
}

/// Palette-wash duration (spec 1g: "palette wash 0.8s").
const WASH_SECS: f32 = 0.8;

/// Eased world-recolour state — the first beat of the materialize sequence.
#[derive(Resource)]
pub struct PaletteWash {
    current: Palette,
    from: Palette,
    t: f32,
    active: bool,
    last_mood: Option<usize>,
}

impl Default for PaletteWash {
    fn default() -> Self {
        let p = Palette::of(&crate::theme::moods()[0]);
        Self {
            current: p,
            from: p,
            t: 0.0,
            active: false,
            last_mood: Some(0),
        }
    }
}

/// Ease the world's colours toward the current mood instead of snapping
/// (spec 1g's 0.8s palette wash), then paint the (settled or washing) palette
/// every frame with the recipe's atmosphere riding on top — fog density, the
/// ambient tint, and the primary biome's tint (each a ≤40% mix, per
/// `recipe.rs`'s contract). Applying every frame (rather than only while a
/// wash runs) means a recipe landing mid-track modulates the world without
/// retriggering a wash, and un-applies just as cleanly. Runs before
/// `animate_world`, which rescales the drifter emissive per-beat while keeping
/// the washed hue.
#[allow(clippy::too_many_arguments)]
pub fn palette_wash(
    time: Res<Time>,
    theme: Res<Theme>,
    active_recipe: Res<crate::recipe::ActiveRecipe>,
    timbre: Res<crate::analysis::Timbre>,
    blend: Res<crate::analysis::MoodBlend>,
    env_override: Res<crate::agent_types::EnvOverride>,
    mut wash: ResMut<PaletteWash>,
    world_mats: Option<Res<WorldMaterials>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // The agent's own ambient entity (M7, `llm`) owns its colour; the wash
    // owns every other ambient. The marker is ungated so this query compiles
    // in every feature config.
    mut ambient_q: Query<&mut AmbientLight, Without<crate::agent_types::AgentAmbient>>,
    mut fog_q: Query<&mut DistanceFog>,
    mut clear: ResMut<ClearColor>,
) {
    let mood_idx = theme.mood % crate::theme::moods().len();
    if wash.last_mood != Some(mood_idx) {
        wash.last_mood = Some(mood_idx);
        wash.from = wash.current;
        wash.t = 0.0;
        wash.active = true;
    }
    if wash.active {
        wash.t += time.delta_secs();
        let f = (wash.t / WASH_SECS).clamp(0.0, 1.0);
        let s = f * f * (3.0 - 2.0 * f); // smoothstep
        let to = Palette::of(theme.current());
        wash.current = wash.from.mix(&to, s);
        if f >= 1.0 {
            wash.active = false;
        }
    }

    let recipe = active_recipe.get();
    let fog_density = recipe
        .map(|r| r.atmosphere.fog_density)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let ambient_tint = recipe.map(|r| r.atmosphere.ambient_tint);
    let biome_tint = recipe
        .and_then(|r| r.biomes.first())
        .map(|b| (b.tint, b.density.clamp(0.0, 1.0)));

    let mut p = wash.current;
    {
        use bevy::color::Mix as _;
        let tint = |c: Color, t: [f32; 3], f: f32| c.mix(&Color::srgb(t[0], t[1], t[2]), f * 0.4);
        if let Some((t, d)) = biome_tint {
            p.ground = tint(p.ground, t, d);
            p.drift_base = tint(p.drift_base, t, d);
            p.fog = tint(p.fog, t, d);
        }
        if let Some(t) = ambient_tint {
            p.ambient = tint(p.ambient, t, 1.0);
        }
        // Continuous timbre: bright tracks lift fog/ground toward the sky,
        // dark timbres deepen them toward the ground — same mood, different
        // grain. Subtle by construction (≤30% at the extremes).
        let b = timbre.0;
        if b >= 0.5 {
            let f = (b - 0.5) * 0.6;
            p.fog = p.fog.mix(&p.sky_top, f * 0.5);
            p.ground = p.ground.mix(&p.drift_base, f * 0.35);
        } else {
            let f = (0.5 - b) * 0.6;
            p.fog = p.fog.mix(&p.ground, f * 0.5);
            p.drift_base = p.drift_base.mix(&p.fog, f * 0.35);
        }
        // Continuous mood: blend toward the neighbour the mapper almost
        // chose, by how close the call was.
        if blend.amount > 0.001 {
            let moods = crate::theme::moods();
            let toward = moods
                .get(blend.toward.min(moods.len().saturating_sub(1)))
                .copied()
                .unwrap_or(moods[0]);
            p = p.mix(&Palette::of(&toward), blend.amount);
        }
    }

    // The agent's environment override wins when its track is current; the
    // mood's sky is the fallback (and the rule path's only value).
    clear.0 = env_override.background.unwrap_or(p.sky_top);
    for mut ambient in &mut ambient_q {
        ambient.color = p.ambient;
    }
    for mut fog in &mut fog_q {
        fog.color = p.fog;
        // fog_density 0 = today's band (18..95); 1 = a near wall (5..30).
        fog.falloff = FogFalloff::Linear {
            start: 18.0 - 13.0 * fog_density,
            end: 95.0 - 65.0 * fog_density,
        };
    }
    let Some(world_mats) = world_mats else { return };
    if let Some(mut m) = materials.get_mut(&world_mats.ground) {
        m.base_color = p.ground;
    }
    if let Some(mut m) = materials.get_mut(&world_mats.drifter) {
        m.base_color = p.drift_base;
        m.emissive = scaled_linear(p.accent, 0.6);
    }
}

/// Spawn/refresh the ambient particle field from the recipe (M7). Runs each
/// frame but only rebuilds when the recipe's particle signature changes
/// (kind and rounded rate), so it's cheap. No recipe or rate 0 means no
/// particles (and any existing ones are despawned). Seeded deterministically
/// from the recipe seed.
#[allow(clippy::too_many_arguments)]
pub fn spawn_particles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut field: ResMut<ParticleField>,
    active_recipe: Res<crate::recipe::ActiveRecipe>,
    theme: Res<Theme>,
    existing: Query<Entity, With<Particle>>,
    mut last: Local<Option<(crate::recipe::ParticleKind, u32)>>,
) {
    let signature = active_recipe.get().and_then(|r| {
        (r.particles.rate > 0.0).then_some((r.particles.kind, (r.particles.rate * 10.0) as u32))
    });

    // Rebuild only on a signature change (kind, or rate to 1 decimal).
    if *last == signature {
        return;
    }
    *last = signature;

    // Always clear first (covers rate→0 and kind swaps) — including the
    // published material handle, so nothing animates a dead field.
    field.material = None;
    for e in &existing {
        commands.entity(e).despawn();
    }

    let Some(recipe) = active_recipe.get() else {
        return;
    };
    if recipe.particles.rate <= 0.0 {
        return;
    }

    // Particle count scales with rate (capped for perf). rate 0..1 → 0..200.
    let count = (recipe.particles.rate * 200.0).round() as usize;
    let mood = theme.current();
    let tint = match recipe.particles.kind {
        crate::recipe::ParticleKind::Ember => mood.accent,
        crate::recipe::ParticleKind::Spark => mood.accent,
        crate::recipe::ParticleKind::Snow => mood.sky_top,
        crate::recipe::ParticleKind::Spore => mood.ambient,
        crate::recipe::ParticleKind::Dust => mood.fog,
    };
    let mesh = meshes.add(Sphere::new(0.06).mesh().ico(2).unwrap());
    // One shared material for the whole field (cheap); per-kind tint only.
    // The handle is published so `animate_world` can breathe its emissive
    // with the live high band — one material write per frame, not 200.
    let material = materials.add(StandardMaterial {
        base_color: tint.with_alpha(0.7),
        emissive: scaled_linear(tint, 0.5),
        unlit: true,
        ..default()
    });
    *field = ParticleField {
        material: Some(material.clone()),
        tint,
    };
    let mut rng = recipe.seed;
    for _ in 0..count {
        let x = (crate::world_assets::splitmix(&mut rng) as f32 / u32::MAX as f32 - 0.5)
            * 2.0
            * PARTICLE_SPREAD;
        let y = (crate::world_assets::splitmix(&mut rng) as f32 / u32::MAX as f32) * PARTICLE_MAX_Y;
        let z = (crate::world_assets::splitmix(&mut rng) as f32 / u32::MAX as f32 - 0.5)
            * 2.0
            * PARTICLE_SPREAD;
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(x, y, z),
            Particle {
                kind: recipe.particles.kind,
                seed: crate::world_assets::splitmix(&mut rng) as f32 / u32::MAX as f32,
                base: Vec3::new(x, y, z),
                drift: recipe.particles.drift,
            },
        ));
    }
}

/// Drift the shapes; pulse their glow with the beat; obey the world clock so a
/// Drift the shapes; pulse their glow with the beat; obey the world clock so a
/// paused world slows to a near-freeze (time-dilation). The recipe's
/// `motion_speed` and the current section's feel ([`SectionFeel`]) both scale
/// the motion; the section's `energy_shift` rides on the live energy envelope.
///
/// Stem/band reactivity: the world listens per frequency range, not just as
/// one loudness — bass (Demucs curve when present, else the live tap) swells
/// the drifters' sway, the "other" stem quickens their spin, drums accent the
/// beat glow, vocals widen the particles' wander, and the live high band
/// breathes the particle field's emissive.
#[allow(clippy::too_many_arguments)]
pub fn animate_world(
    time: Res<Time>,
    clock: Res<WorldClock>,
    beat: Res<Beat>,
    stems: Res<crate::playback::StemLevels>,
    comfort: Res<crate::Comfort>,
    feel: Res<SectionFeel>,
    active_recipe: Res<crate::recipe::ActiveRecipe>,
    particle_field: Res<ParticleField>,
    world_mats: Option<Res<WorldMaterials>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut drifters: Query<(&Drifter, &mut Transform), Without<Particle>>,
    mut particles: Query<(&Particle, &mut Transform), Without<Drifter>>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs() * clock.speed;
    // Comfort › Gentler world motion damps the sway (spec 1n).
    let gentle = if comfort.gentler_motion { 0.4 } else { 1.0 };
    // M7: a recipe may scale drift speed within [0.25, 2.5] (already clamped),
    // and the current section's choreography multiplies on top (Calm/Active).
    // Absent recipe → 1.0 (today's behaviour).
    let motion = active_recipe.get().map(|r| r.motion_speed).unwrap_or(1.0) * feel.motion;
    // Stem levels take precedence when the Demucs pass ran; the live tap's
    // band envelopes stand in otherwise (max, so the louder truth wins).
    let bass = stems.0[1].max(beat.bass);
    let drums = stems.0[0];
    let vocals = stems.0[2];
    let other = stems.0[3];
    // Comfort › Reduce flashing: reactivity stays, flashing doesn't — the
    // bands scale *motion* (slow) rather than strobing brightness.
    let react = if comfort.reduce_flashing { 0.5 } else { 1.0 };

    for (d, mut tf) in &mut drifters {
        let p = t * clock.speed;
        // Bass swells the sway (±60% around the base amplitude).
        let sway = 0.6 * (0.7 + bass * 0.6 * react);
        tf.translation.y =
            d.base.y + (p * 0.4 * motion + d.seed * std::f32::consts::TAU).sin() * sway * gentle;
        tf.translation.x =
            d.base.x + (p * 0.23 * motion + d.seed * std::f32::consts::PI).cos() * 0.4 * gentle;
        tf.rotate_y(dt * (0.2 + d.seed.fract() * 0.4) * gentle * motion * (0.7 + other * 0.6));
    }

    // M7: drift the recipe's particle layer. Direction is kind-dependent
    // (embers/sparks rise, snow falls, dust/spores hover), speed scaled by the
    // recipe's drift, the global motion multiplier, and the drums (lift) and
    // vocals (wander) levels. Particles recycle to the bottom (rising kinds)
    // or top (falling) when they leave the volume.
    use crate::recipe::ParticleKind;
    for (p, mut tf) in &mut particles {
        let vertical = match p.kind {
            ParticleKind::Ember | ParticleKind::Spark => 1.0, // rise
            ParticleKind::Snow => -0.6,                       // fall
            ParticleKind::Dust | ParticleKind::Spore => 0.15, // hover
        };
        let speed = vertical * p.drift * motion * gentle * 1.5 * (0.8 + drums * 0.4 * react);
        tf.translation.y += dt * speed;
        // Lateral wander for life — widened by the vocal level.
        let wander = 0.3 + vocals * 0.5 * react;
        tf.translation.x += (t * 0.5 + p.seed * std::f32::consts::TAU).sin() * dt * wander * gentle;
        // Recycle when out of bounds.
        if tf.translation.y > PARTICLE_MAX_Y {
            tf.translation.y = 0.0;
        } else if tf.translation.y < 0.0 {
            tf.translation.y = PARTICLE_MAX_Y;
        }
    }

    // The particle field's emissive breathes with the live high band
    // ("sparkle") — one shared-material write, no per-particle cost. The
    // section's presence verb fades the whole field (bridge strips, chorus
    // swells) without respawning it.
    if let Some(handle) = &particle_field.material
        && let Some(mut material) = materials.get_mut(handle)
    {
        let presence = feel.particles.clamp(0.0, 1.5);
        let sparkle = if comfort.reduce_flashing {
            0.45 * presence
        } else {
            ((0.35 + beat.highs * 0.8 + beat.energy * 0.2).min(1.2)) * presence
        };
        material.emissive = scaled_linear(particle_field.tint, sparkle);
        material.base_color = particle_field.tint.with_alpha((0.7 * presence).min(0.9));
    }

    // Beat-reactive emissive on the shared drifter material. (Split out of a
    // let-chain: chained `let` bindings are read-only in Rust 2024.)
    let Some(world_mats) = world_mats else { return };
    if let Some(mut material) = materials.get_mut(&world_mats.drifter) {
        // Comfort › Reduce flashing holds the glow steady (no beat pulse). The
        // section's energy shift rides on the live envelope (clamped ≥0), and
        // the drums stem accents the pulse.
        let glow = if comfort.reduce_flashing {
            0.7
        } else {
            0.55 + beat.pulse
                * 0.9
                * (beat.energy + feel.energy_shift).max(0.0)
                * (1.0 + drums * 0.35)
        };
        // M7: scale the glow by the recipe's bloom ceiling (1.0 = today).
        let glow = glow
            * active_recipe
                .get()
                .map(|r| r.atmosphere.bloom_ceiling)
                .unwrap_or(1.0);
        material.emissive = scaled_linear_from(material.emissive, glow);
    }
}

/// Camera feel: Drift auto-orbits with section-aware choreography; Explore is
/// a pointer-locked first-person fly cam (WASD + Space/Shift, scroll for speed).
///
/// The Drift rig reads the section feel: calm sections pull up and out for the
/// overview, active ones drop in low and close, and the quiet ends (intro,
/// outro, bridge — wherever the beacons dim) add a wider, higher pull-away.
/// Everything eases (never snaps), and the look-target rises with the live
/// energy so the chorus literally lifts the gaze.
#[allow(clippy::too_many_arguments)]
pub fn camera_control(
    time: Res<Time>,
    clock: Res<WorldClock>,
    beat: Res<Beat>,
    feel: Res<SectionFeel>,
    mode: Res<CameraMode>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    cursor_q: Query<&CursorOptions, With<PrimaryWindow>>,
    mut cam_q: Query<(&mut Transform, &mut WorldCamera)>,
) {
    let Ok((mut tf, mut cam)) = cam_q.single_mut() else {
        return;
    };
    let dt = time.delta_secs();

    match *mode {
        CameraMode::Drift => {
            // Section choreography: motion 0.5 (calm) → radius ~19.4, height
            // ~4.9; motion 1.0 (drift) → 16.25 / 3.0; motion 1.6 (active) →
            // 12.5 / 0.75. Quiet sections (dim beacons) pull further away.
            let quiet = (1.0 - feel.beacons.clamp(0.0, 1.0)).max(0.0);
            let target_r = 22.5 - feel.motion * 6.25 + quiet * 4.0;
            let target_h = 6.75 - feel.motion * 3.75 + quiet * 2.0;
            let k = (dt * 1.2).min(1.0);
            cam.eased_radius += (target_r - cam.eased_radius) * k;
            cam.eased_height += (target_h - cam.eased_height) * k;

            cam.orbit += dt * 0.06 * feel.motion.max(0.3) * clock.speed;
            let target = Vec3::new(0.0, 1.5 + beat.energy * 1.5, -6.0);
            tf.translation = target
                + Vec3::new(
                    cam.orbit.cos() * cam.eased_radius,
                    cam.eased_height,
                    cam.orbit.sin() * cam.eased_radius,
                );
            tf.look_at(target, Vec3::Y);
        }
        CameraMode::Explore => {
            // Pointer-locked look: while grabbed, the OS streams raw deltas
            // with no screen-edge limit, so yaw wraps freely through 360° and
            // pitch swings from straight up to straight down. Unlocked means
            // an overlay owns the cursor — don't steer the world from
            // UI-bound mouse movement.
            let locked = cursor_q
                .single()
                .map(|c| c.grab_mode == CursorGrabMode::Locked)
                .unwrap_or(false);
            if locked {
                let d = motion.delta;
                cam.yaw -= d.x * 0.0022;
                cam.pitch = (cam.pitch - d.y * 0.0022).clamp(-1.55, 1.55);
                // Scroll trims fly speed; exponential steps feel even, and
                // pixel-unit (trackpad) deltas are normalised to "notches".
                let notches = match scroll.unit {
                    MouseScrollUnit::Line => scroll.delta.y,
                    MouseScrollUnit::Pixel => scroll.delta.y / 50.0,
                };
                cam.fly_speed = (cam.fly_speed * 1.15f32.powf(notches)).clamp(1.5, 40.0);
            }
            let rot = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
            tf.rotation = rot;

            // Fly: WASD follows the view direction (W goes where you look,
            // even pitched up/down); Space / Shift are pure vertical.
            let mut mv = Vec3::ZERO;
            let fwd = rot * Vec3::NEG_Z;
            let right = rot * Vec3::X;
            if keys.pressed(KeyCode::KeyW) {
                mv += fwd;
            }
            if keys.pressed(KeyCode::KeyS) {
                mv -= fwd;
            }
            if keys.pressed(KeyCode::KeyD) {
                mv += right;
            }
            if keys.pressed(KeyCode::KeyA) {
                mv -= right;
            }
            if keys.pressed(KeyCode::Space) {
                mv += Vec3::Y;
            }
            if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
                mv -= Vec3::Y;
            }
            tf.translation += mv.normalize_or_zero() * dt * cam.fly_speed;
            tf.translation.y = tf.translation.y.clamp(0.6, 60.0);
        }
    }
}

/// Reconcile who owns the pointer: Explore flies first-person with the cursor
/// grabbed and hidden (raw deltas, no screen edge); anything that needs
/// clicking — pause, the queue, a modal overlay, Drift's HUD tabs — releases
/// it. Runs even while paused (unlike [`camera_control`]) so Esc always hands
/// the cursor back to the pause menu, and re-locks on resume.
pub fn update_cursor_grab(
    mode: Res<CameraMode>,
    paused: Res<Paused>,
    queue_open: Res<QueueOpen>,
    stack: Res<OverlayStack>,
    mut cursor_q: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(mut cursor) = cursor_q.single_mut() else {
        return;
    };
    let want_locked =
        *mode == CameraMode::Explore && !paused.0 && !queue_open.0 && stack.is_empty();
    let locked = cursor.grab_mode == CursorGrabMode::Locked;
    if locked == want_locked {
        return;
    }
    cursor.grab_mode = if want_locked {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    cursor.visible = !want_locked;
}

/// Drive the pause "held ring" (spec 1e): ease its visibility in (~220ms) while
/// the transport is frozen and out (~320ms) on resume, and keep it a fixed
/// distance ahead of the camera so it's always centred on screen.
#[allow(clippy::type_complexity)]
pub fn sync_held_ring(
    time: Res<Time>,
    playback: Res<Playback>,
    mut ring_res: ResMut<HeldRing>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut qs: ParamSet<(
        Query<&Transform, With<WorldCamera>>,
        Query<(&mut Transform, &MeshMaterial3d<StandardMaterial>), With<HeldRingMesh>>,
    )>,
) {
    let frozen = !playback.playing;
    let target = frozen as u8 as f32;
    // ~220ms in / ~320ms out.
    let rate = if frozen { 4.5 } else { 3.1 };
    ring_res.0 += (target - ring_res.0) * (time.delta_secs() * rate).min(1.0);
    ring_res.0 = ring_res.0.clamp(0.0, 1.0);

    // Read the camera first (scoped so the query borrow drops before p1),
    // then move the ring ahead of it.
    let cam_tf = {
        let cam_q = qs.p0();
        match cam_q.single() {
            Ok(tf) => *tf,
            Err(_) => return,
        }
    };
    let mut ring_q = qs.p1();
    let Ok((mut ring_tf, mat)) = ring_q.single_mut() else {
        return;
    };
    let cam_fwd = cam_tf.forward();
    ring_tf.translation = cam_tf.translation + cam_fwd * 7.0;
    // A Bevy `Torus` lies in its local XZ plane (hole axis +Y). Point the
    // entity's −Z at the camera, then roll the torus 90° about X to stand it
    // upright so it presents as a circle (not a flat line).
    let stand_up = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    ring_tf.rotation = cam_tf.rotation * stand_up;

    let Some(mut material) = materials.get_mut(&mat.0) else {
        return;
    };
    material.base_color = crate::theme::TEXT.with_alpha(ring_res.0 * 0.45);
}

// ---------------------------------------------------------------------------
// World title — the recipe's name on the horizon
// ---------------------------------------------------------------------------

/// The world's title text on the horizon. One entity; its colour is the only
/// per-frame write.
#[derive(Component)]
pub struct WorldTitle;

/// Show the world's name as 3D text on the horizon while the intro plays —
/// the LLM recipe's authored name (mood name as the fallback) — and ease it
/// out as the first verse arrives. Without measured sections a positional
/// window (first fifth of the track) stands in.
pub fn sync_world_title(
    mut commands: Commands,
    playback: Res<Playback>,
    active_recipe: Res<crate::recipe::ActiveRecipe>,
    theme: Res<Theme>,
    fonts: Res<crate::theme::Fonts>,
    mut existing: Query<(Entity, &mut Text2d, &mut TextColor), With<WorldTitle>>,
) {
    // The title, capped on a char boundary (the horizon is wide, not endless).
    let name = match active_recipe
        .get()
        .map(|r| r.world_name.trim())
        .filter(|n| !n.is_empty())
    {
        Some(n) if n.chars().count() <= 30 => n.to_string(),
        Some(n) => {
            let capped: String = n.chars().take(29).collect();
            format!("{capped}…")
        }
        None => theme.current().world_name.to_string(),
    };

    // Fade profile: in over the first ~2%, hold, then out — anchored to the
    // measured intro section when there is one.
    let frac = playback.fraction();
    let alpha = if playback.sections.len() >= 2 {
        let intro_end = playback.sections[1];
        fade_window(frac, 0.02, (intro_end - 0.02).max(0.05), intro_end + 0.04)
    } else {
        fade_window(frac, 0.02, 0.10, 0.18)
    };

    if let Ok((_, mut text, mut color)) = existing.single_mut() {
        if text.0 != name {
            *text = Text2d::new(name);
        }
        color.0 = crate::theme::TEXT.with_alpha(alpha * 0.55);
        return;
    }
    commands.spawn((
        WorldTitle,
        Text2d::new(name),
        TextFont {
            font: FontSource::Handle(fonts.display.clone()),
            font_size: FontSize::Px(34.0),
            ..default()
        },
        TextColor(crate::theme::TEXT.with_alpha(0.0)),
        Transform::from_xyz(0.0, 8.5, -34.0),
    ));
}

/// 0→1 over `[0, in_by]`, held to `hold_until`, back to 0 by `out_by`.
fn fade_window(f: f32, in_by: f32, hold_until: f32, out_by: f32) -> f32 {
    let fin = (f / in_by.max(1e-3)).clamp(0.0, 1.0);
    let fout = 1.0 - ((f - hold_until) / (out_by - hold_until).max(1e-3)).clamp(0.0, 1.0);
    fin.min(fout).max(0.0)
}

// --- colour helpers -------------------------------------------------------

/// `color * intensity` as a linear-RGB emissive value.
fn scaled_linear(color: Color, intensity: f32) -> LinearRgba {
    let l = color.to_linear();
    LinearRgba::rgb(l.red * intensity, l.green * intensity, l.blue * intensity)
}

/// Rescale an existing emissive to a new intensity while keeping its hue.
fn scaled_linear_from(current: LinearRgba, intensity: f32) -> LinearRgba {
    // Normalise by the max channel so repeated scaling doesn't drift to black.
    let max = current.red.max(current.green).max(current.blue).max(1e-4);
    LinearRgba::rgb(
        current.red / max * intensity,
        current.green / max * intensity,
        current.blue / max * intensity,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_roles_shape_a_song() {
        use crate::recipe::SectionRole as R;
        assert_eq!(positional_role(0, 5), Some(R::Intro));
        assert_eq!(positional_role(4, 5), Some(R::Outro));
        assert_eq!(
            positional_role(2, 5),
            Some(R::Chorus),
            "middle is the chorus"
        );
        // 3/4 point of 4 segments (n=4 boundaries → 4 segments): segment 3 is
        // the last, so the bridge lands at 3 only when it isn't the outro.
        assert_eq!(positional_role(1, 5), Some(R::Verse));
    }

    #[test]
    fn section_feel_eases_toward_verbs() {
        let mut f = SectionFeel::default();
        let target = SectionFeel::verbs(Some(crate::recipe::SectionRole::Outro));
        assert!(target.scatter < 0.01, "outro sinks the scatter");
        for _ in 0..120 {
            f.ease_toward(&target, 0.1);
        }
        assert!((f.scatter - target.scatter).abs() < 1e-3);
        assert!(f.beacons < 0.75, "outro dims beacons");
    }

    /// The exact easing step used by `sync_held_ring`.
    fn step(v: f32, frozen: bool, dt: f32) -> f32 {
        let target = frozen as u8 as f32;
        let rate = if frozen { 4.5 } else { 3.1 };
        (v + (target - v) * (dt * rate).min(1.0)).clamp(0.0, 1.0)
    }

    #[test]
    fn held_ring_eases_up_while_frozen() {
        let mut v = 0.0;
        for _ in 0..60 {
            v = step(v, true, 1.0 / 60.0);
        }
        assert!(v > 0.99, "1s of freeze brings the ring to ~full (got {v})");
    }

    #[test]
    fn held_ring_fades_faster_in_than_out() {
        // 220ms in vs 320ms out: after 0.3s the fade-in is well past half
        // (e^(−1.35) ≈ 0.26 → 0.74 up), while the fade-out is only about
        // half-done (e^(−1.24) ≈ 0.29 → 0.29 down from full).
        let mut up = 0.0;
        for _ in 0..18 {
            up = step(up, true, 1.0 / 60.0); // 0.3s
        }
        let mut down = 1.0;
        for _ in 0..24 {
            down = step(down, false, 1.0 / 60.0); // 0.4s
        }
        assert!(up > 0.5, "fade-in mostly done by 0.3s (got {up})");
        assert!(down < 0.5, "fade-out still partial at 0.4s (got {down})");
        // …and at equal elapsed time the fade-in is strictly faster.
        let a = step(0.0, true, 0.2); // 0.2s in
        let b = step(1.0, false, 0.2); // 0.2s out (remaining)
        assert!(a > b, "in ({a}) should outpace out (remaining {b})");
    }

    #[test]
    fn held_ring_stays_clamped() {
        let mut v = 1.0;
        for _ in 0..600 {
            v = step(v, true, 1.0 / 60.0);
        }
        assert!((0.0..=1.0).contains(&v));
        let mut v = 0.0;
        for _ in 0..600 {
            v = step(v, false, 1.0 / 60.0);
        }
        assert!((0.0..=1.0).contains(&v));
        assert_eq!(v, 0.0, "fully resumed settles to 0");
    }
}
