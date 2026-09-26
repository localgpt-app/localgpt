//! Conversion layer between `commands.rs` types and `localgpt-world-types`.
//!
//! These `From`/`Into` implementations bridge the existing gen command protocol
//! (agent ↔ Bevy) with the unified world data model.  This enables:
//! - Saving scenes as RON `WorldManifest` instead of TOML+glTF
//! - Loading scenes by spawning from `WorldEntity` definitions
//! - Preserving parametric shape info through save/load cycles

use std::collections::HashMap;

use localgpt_world_types as wt;

use super::commands::*;

// ---------------------------------------------------------------------------
// Shape conversions
// ---------------------------------------------------------------------------

/// Convert PrimitiveShape + HashMap dimensions → world-types Shape
pub fn shape_from_primitive(shape: PrimitiveShape, dims: &HashMap<String, f32>) -> wt::Shape {
    match shape {
        PrimitiveShape::Cuboid => wt::Shape::Cuboid {
            x: dims.get("x").copied().unwrap_or(1.0),
            y: dims.get("y").copied().unwrap_or(1.0),
            z: dims.get("z").copied().unwrap_or(1.0),
        },
        PrimitiveShape::Sphere => wt::Shape::Sphere {
            radius: dims.get("radius").copied().unwrap_or(0.5),
        },
        PrimitiveShape::Cylinder => wt::Shape::Cylinder {
            radius: dims.get("radius").copied().unwrap_or(0.5),
            height: dims.get("height").copied().unwrap_or(1.0),
        },
        PrimitiveShape::Cone => wt::Shape::Cone {
            radius: dims.get("radius").copied().unwrap_or(0.5),
            height: dims.get("height").copied().unwrap_or(1.0),
        },
        PrimitiveShape::Capsule => wt::Shape::Capsule {
            radius: dims.get("radius").copied().unwrap_or(0.5),
            half_length: dims.get("half_length").copied().unwrap_or(0.5),
        },
        PrimitiveShape::Torus => wt::Shape::Torus {
            major_radius: dims.get("major_radius").copied().unwrap_or(1.0),
            minor_radius: dims.get("minor_radius").copied().unwrap_or(0.25),
        },
        PrimitiveShape::Plane => wt::Shape::Plane {
            x: dims.get("x").copied().unwrap_or(1.0),
            z: dims.get("z").copied().unwrap_or(1.0),
        },
        PrimitiveShape::Pyramid => wt::Shape::Pyramid {
            base_x: dims.get("base_x").copied().unwrap_or(1.0),
            base_z: dims.get("base_z").copied().unwrap_or(1.0),
            height: dims.get("height").copied().unwrap_or(1.0),
        },
        PrimitiveShape::Tetrahedron => wt::Shape::Tetrahedron {
            radius: dims.get("radius").copied().unwrap_or(1.0),
        },
        PrimitiveShape::Icosahedron => wt::Shape::Icosahedron {
            radius: dims.get("radius").copied().unwrap_or(1.0),
        },
        PrimitiveShape::Wedge => wt::Shape::Wedge {
            x: dims.get("x").copied().unwrap_or(1.0),
            y: dims.get("y").copied().unwrap_or(1.0),
            z: dims.get("z").copied().unwrap_or(1.0),
        },
    }
}

/// Convert world-types Shape → PrimitiveShape + dimensions HashMap
#[cfg(test)]
pub fn shape_to_primitive(shape: &wt::Shape) -> (PrimitiveShape, HashMap<String, f32>) {
    match shape {
        wt::Shape::Cuboid { x, y, z } => {
            let mut dims = HashMap::new();
            dims.insert("x".into(), *x);
            dims.insert("y".into(), *y);
            dims.insert("z".into(), *z);
            (PrimitiveShape::Cuboid, dims)
        }
        wt::Shape::Sphere { radius } => {
            let mut dims = HashMap::new();
            dims.insert("radius".into(), *radius);
            (PrimitiveShape::Sphere, dims)
        }
        wt::Shape::Cylinder { radius, height } => {
            let mut dims = HashMap::new();
            dims.insert("radius".into(), *radius);
            dims.insert("height".into(), *height);
            (PrimitiveShape::Cylinder, dims)
        }
        wt::Shape::Cone { radius, height } => {
            let mut dims = HashMap::new();
            dims.insert("radius".into(), *radius);
            dims.insert("height".into(), *height);
            (PrimitiveShape::Cone, dims)
        }
        wt::Shape::Capsule {
            radius,
            half_length,
        } => {
            let mut dims = HashMap::new();
            dims.insert("radius".into(), *radius);
            dims.insert("half_length".into(), *half_length);
            (PrimitiveShape::Capsule, dims)
        }
        wt::Shape::Torus {
            major_radius,
            minor_radius,
        } => {
            let mut dims = HashMap::new();
            dims.insert("major_radius".into(), *major_radius);
            dims.insert("minor_radius".into(), *minor_radius);
            (PrimitiveShape::Torus, dims)
        }
        wt::Shape::Plane { x, z } => {
            let mut dims = HashMap::new();
            dims.insert("x".into(), *x);
            dims.insert("z".into(), *z);
            (PrimitiveShape::Plane, dims)
        }
        wt::Shape::Pyramid {
            base_x,
            base_z,
            height,
        } => {
            let mut dims = HashMap::new();
            dims.insert("base_x".into(), *base_x);
            dims.insert("base_z".into(), *base_z);
            dims.insert("height".into(), *height);
            (PrimitiveShape::Pyramid, dims)
        }
        wt::Shape::Tetrahedron { radius } => {
            let mut dims = HashMap::new();
            dims.insert("radius".into(), *radius);
            (PrimitiveShape::Tetrahedron, dims)
        }
        wt::Shape::Icosahedron { radius } => {
            let mut dims = HashMap::new();
            dims.insert("radius".into(), *radius);
            (PrimitiveShape::Icosahedron, dims)
        }
        wt::Shape::Wedge { x, y, z } => {
            let mut dims = HashMap::new();
            dims.insert("x".into(), *x);
            dims.insert("y".into(), *y);
            dims.insert("z".into(), *z);
            (PrimitiveShape::Wedge, dims)
        }
    }
}

// ---------------------------------------------------------------------------
// Material conversion
// ---------------------------------------------------------------------------

impl From<&SpawnPrimitiveCmd> for wt::MaterialDef {
    fn from(cmd: &SpawnPrimitiveCmd) -> Self {
        let alpha_mode = cmd
            .alpha_mode
            .as_deref()
            .and_then(|s| match s.to_lowercase().as_str() {
                "opaque" => Some(wt::AlphaModeDef::Opaque),
                "blend" => Some(wt::AlphaModeDef::Blend),
                "add" => Some(wt::AlphaModeDef::Add),
                "multiply" => Some(wt::AlphaModeDef::Multiply),
                s if s.starts_with("mask") => {
                    let cutoff = s
                        .strip_prefix("mask:")
                        .or_else(|| s.strip_prefix("mask(").and_then(|s| s.strip_suffix(')')))
                        .and_then(|v| v.parse::<f32>().ok())
                        .unwrap_or(0.5);
                    Some(wt::AlphaModeDef::Mask(cutoff))
                }
                _ => None,
            });
        wt::MaterialDef {
            color: cmd.color,
            metallic: cmd.metallic,
            roughness: cmd.roughness,
            emissive: cmd.emissive,
            alpha_mode,
            unlit: cmd.unlit,
            double_sided: None,
            reflectance: None,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Light conversion
// ---------------------------------------------------------------------------

impl From<LightType> for wt::LightType {
    fn from(lt: LightType) -> Self {
        match lt {
            LightType::Directional => wt::LightType::Directional,
            LightType::Point => wt::LightType::Point,
            LightType::Spot => wt::LightType::Spot,
        }
    }
}

impl From<wt::LightType> for LightType {
    fn from(lt: wt::LightType) -> Self {
        match lt {
            wt::LightType::Directional => LightType::Directional,
            wt::LightType::Point => LightType::Point,
            wt::LightType::Spot => LightType::Spot,
        }
    }
}

impl From<&SetLightCmd> for wt::LightDef {
    fn from(cmd: &SetLightCmd) -> Self {
        wt::LightDef {
            light_type: cmd.light_type.into(),
            color: cmd.color,
            intensity: cmd.intensity,
            direction: cmd.direction,
            shadows: cmd.shadows,
            range: cmd.range,
            outer_angle: cmd.outer_angle,
            inner_angle: cmd.inner_angle,
        }
    }
}

impl From<&wt::LightDef> for SetLightCmd {
    fn from(def: &wt::LightDef) -> Self {
        SetLightCmd {
            name: String::new(), // caller must set
            light_type: def.light_type.into(),
            color: def.color,
            intensity: def.intensity,
            position: None, // stored in transform, not light
            direction: def.direction,
            shadows: def.shadows,
            range: def.range,
            outer_angle: def.outer_angle,
            inner_angle: def.inner_angle,
        }
    }
}

// ---------------------------------------------------------------------------
// Behavior conversion (String ↔ EntityRef)
// ---------------------------------------------------------------------------

impl From<&BehaviorDef> for wt::BehaviorDef {
    fn from(def: &BehaviorDef) -> Self {
        match def {
            BehaviorDef::Orbit {
                center,
                center_point,
                radius,
                speed,
                axis,
                phase,
                tilt,
            } => wt::BehaviorDef::Orbit {
                center: center.as_ref().map(|s| wt::EntityRef::Name(s.clone())),
                center_point: *center_point,
                radius: *radius,
                speed: *speed,
                axis: *axis,
                phase: *phase,
                tilt: *tilt,
            },
            BehaviorDef::Spin { axis, speed } => wt::BehaviorDef::Spin {
                axis: *axis,
                speed: *speed,
            },
            BehaviorDef::Bob {
                axis,
                amplitude,
                frequency,
                phase,
            } => wt::BehaviorDef::Bob {
                axis: *axis,
                amplitude: *amplitude,
                frequency: *frequency,
                phase: *phase,
            },
            BehaviorDef::LookAt { target } => wt::BehaviorDef::LookAt {
                target: wt::EntityRef::Name(target.clone()),
            },
            BehaviorDef::Pulse {
                min_scale,
                max_scale,
                frequency,
            } => wt::BehaviorDef::Pulse {
                min_scale: *min_scale,
                max_scale: *max_scale,
                frequency: *frequency,
            },
            BehaviorDef::PathFollow {
                waypoints,
                speed,
                mode,
                orient_to_path,
            } => wt::BehaviorDef::PathFollow {
                waypoints: waypoints.clone(),
                speed: *speed,
                mode: (*mode).into(),
                orient_to_path: *orient_to_path,
            },
            BehaviorDef::Bounce {
                height,
                gravity,
                damping,
                surface_y,
            } => wt::BehaviorDef::Bounce {
                height: *height,
                gravity: *gravity,
                damping: *damping,
                surface_y: *surface_y,
            },
        }
    }
}

impl From<&wt::BehaviorDef> for BehaviorDef {
    fn from(def: &wt::BehaviorDef) -> Self {
        match def {
            wt::BehaviorDef::Orbit {
                center,
                center_point,
                radius,
                speed,
                axis,
                phase,
                tilt,
            } => BehaviorDef::Orbit {
                center: center.as_ref().map(entity_ref_to_string),
                center_point: *center_point,
                radius: *radius,
                speed: *speed,
                axis: *axis,
                phase: *phase,
                tilt: *tilt,
            },
            wt::BehaviorDef::Spin { axis, speed } => BehaviorDef::Spin {
                axis: *axis,
                speed: *speed,
            },
            wt::BehaviorDef::Bob {
                axis,
                amplitude,
                frequency,
                phase,
            } => BehaviorDef::Bob {
                axis: *axis,
                amplitude: *amplitude,
                frequency: *frequency,
                phase: *phase,
            },
            wt::BehaviorDef::LookAt { target } => BehaviorDef::LookAt {
                target: entity_ref_to_string(target),
            },
            wt::BehaviorDef::Pulse {
                min_scale,
                max_scale,
                frequency,
            } => BehaviorDef::Pulse {
                min_scale: *min_scale,
                max_scale: *max_scale,
                frequency: *frequency,
            },
            wt::BehaviorDef::PathFollow {
                waypoints,
                speed,
                mode,
                orient_to_path,
            } => BehaviorDef::PathFollow {
                waypoints: waypoints.clone(),
                speed: *speed,
                mode: (*mode).into(),
                orient_to_path: *orient_to_path,
            },
            wt::BehaviorDef::Bounce {
                height,
                gravity,
                damping,
                surface_y,
            } => BehaviorDef::Bounce {
                height: *height,
                gravity: *gravity,
                damping: *damping,
                surface_y: *surface_y,
            },
        }
    }
}

// PathMode conversions
impl From<PathMode> for wt::PathMode {
    fn from(m: PathMode) -> Self {
        match m {
            PathMode::Loop => wt::PathMode::Loop,
            PathMode::PingPong => wt::PathMode::PingPong,
            PathMode::Once => wt::PathMode::Once,
        }
    }
}

impl From<wt::PathMode> for PathMode {
    fn from(m: wt::PathMode) -> Self {
        match m {
            wt::PathMode::Loop => PathMode::Loop,
            wt::PathMode::PingPong => PathMode::PingPong,
            wt::PathMode::Once => PathMode::Once,
        }
    }
}

// ---------------------------------------------------------------------------
// Audio conversions
// ---------------------------------------------------------------------------

impl From<&AmbientSound> for wt::AudioSource {
    fn from(sound: &AmbientSound) -> Self {
        match sound {
            AmbientSound::Wind { speed, gustiness } => wt::AudioSource::Wind {
                speed: *speed,
                gustiness: *gustiness,
            },
            AmbientSound::Rain { intensity } => wt::AudioSource::Rain {
                intensity: *intensity,
            },
            AmbientSound::Forest { bird_density, wind } => wt::AudioSource::Forest {
                bird_density: *bird_density,
                wind: *wind,
            },
            AmbientSound::Ocean { wave_size } => wt::AudioSource::Ocean {
                wave_size: *wave_size,
            },
            AmbientSound::Cave {
                drip_rate,
                resonance,
            } => wt::AudioSource::Cave {
                drip_rate: *drip_rate,
                resonance: *resonance,
            },
            AmbientSound::Stream { flow_rate } => wt::AudioSource::Stream {
                flow_rate: *flow_rate,
            },
            AmbientSound::Silence => wt::AudioSource::Silence,
        }
    }
}

impl From<&EmitterSound> for wt::AudioSource {
    fn from(sound: &EmitterSound) -> Self {
        match sound {
            EmitterSound::Water { turbulence } => wt::AudioSource::Water {
                turbulence: *turbulence,
            },
            EmitterSound::Fire { intensity, crackle } => wt::AudioSource::Fire {
                intensity: *intensity,
                crackle: *crackle,
            },
            EmitterSound::Hum { frequency, warmth } => wt::AudioSource::Hum {
                frequency: *frequency,
                warmth: *warmth,
            },
            EmitterSound::Wind { pitch } => wt::AudioSource::WindEmitter { pitch: *pitch },
            EmitterSound::Custom {
                waveform,
                filter_cutoff,
                filter_type,
            } => wt::AudioSource::Custom {
                waveform: (*waveform).into(),
                filter_cutoff: *filter_cutoff,
                filter_type: (*filter_type).into(),
            },
        }
    }
}

/// Convert a world-types AudioSource back to the legacy AmbientSound, if applicable.
/// Returns None if the source is an emitter-only type.
pub fn audio_source_to_ambient(source: &wt::AudioSource) -> Option<AmbientSound> {
    match source {
        wt::AudioSource::Wind { speed, gustiness } => Some(AmbientSound::Wind {
            speed: *speed,
            gustiness: *gustiness,
        }),
        wt::AudioSource::Rain { intensity } => Some(AmbientSound::Rain {
            intensity: *intensity,
        }),
        wt::AudioSource::Forest { bird_density, wind } => Some(AmbientSound::Forest {
            bird_density: *bird_density,
            wind: *wind,
        }),
        wt::AudioSource::Ocean { wave_size } => Some(AmbientSound::Ocean {
            wave_size: *wave_size,
        }),
        wt::AudioSource::Cave {
            drip_rate,
            resonance,
        } => Some(AmbientSound::Cave {
            drip_rate: *drip_rate,
            resonance: *resonance,
        }),
        wt::AudioSource::Stream { flow_rate } => Some(AmbientSound::Stream {
            flow_rate: *flow_rate,
        }),
        wt::AudioSource::Silence => Some(AmbientSound::Silence),
        _ => None,
    }
}

/// Convert a world-types AudioSource back to the legacy EmitterSound, if applicable.
/// Returns None if the source is an ambient-only type.
pub fn audio_source_to_emitter(source: &wt::AudioSource) -> Option<EmitterSound> {
    match source {
        wt::AudioSource::Water { turbulence } => Some(EmitterSound::Water {
            turbulence: *turbulence,
        }),
        wt::AudioSource::Fire { intensity, crackle } => Some(EmitterSound::Fire {
            intensity: *intensity,
            crackle: *crackle,
        }),
        wt::AudioSource::Hum { frequency, warmth } => Some(EmitterSound::Hum {
            frequency: *frequency,
            warmth: *warmth,
        }),
        wt::AudioSource::WindEmitter { pitch } => Some(EmitterSound::Wind { pitch: *pitch }),
        wt::AudioSource::Custom {
            waveform,
            filter_cutoff,
            filter_type,
        } => Some(EmitterSound::Custom {
            waveform: (*waveform).into(),
            filter_cutoff: *filter_cutoff,
            filter_type: (*filter_type).into(),
        }),
        _ => None,
    }
}

/// Convert gen AmbientSound to world-types AudioSource.
pub fn ambient_sound_to_source(sound: &AmbientSound) -> wt::AudioSource {
    sound.into()
}

/// Convert gen EmitterSound to world-types AudioSource.
pub fn emitter_sound_to_source(sound: &EmitterSound) -> wt::AudioSource {
    sound.into()
}

// WaveformType conversions
impl From<WaveformType> for wt::WaveformType {
    fn from(w: WaveformType) -> Self {
        match w {
            WaveformType::Sine => wt::WaveformType::Sine,
            WaveformType::Saw => wt::WaveformType::Saw,
            WaveformType::Square => wt::WaveformType::Square,
            WaveformType::WhiteNoise => wt::WaveformType::WhiteNoise,
            WaveformType::PinkNoise => wt::WaveformType::PinkNoise,
            WaveformType::BrownNoise => wt::WaveformType::BrownNoise,
        }
    }
}

impl From<wt::WaveformType> for WaveformType {
    fn from(w: wt::WaveformType) -> Self {
        match w {
            wt::WaveformType::Sine => WaveformType::Sine,
            wt::WaveformType::Saw => WaveformType::Saw,
            wt::WaveformType::Square => WaveformType::Square,
            wt::WaveformType::WhiteNoise => WaveformType::WhiteNoise,
            wt::WaveformType::PinkNoise => WaveformType::PinkNoise,
            wt::WaveformType::BrownNoise => WaveformType::BrownNoise,
        }
    }
}

// FilterType conversions
impl From<FilterType> for wt::FilterType {
    fn from(f: FilterType) -> Self {
        match f {
            FilterType::Lowpass => wt::FilterType::Lowpass,
            FilterType::Highpass => wt::FilterType::Highpass,
            FilterType::Bandpass => wt::FilterType::Bandpass,
        }
    }
}

impl From<wt::FilterType> for FilterType {
    fn from(f: wt::FilterType) -> Self {
        match f {
            wt::FilterType::Lowpass => FilterType::Lowpass,
            wt::FilterType::Highpass => FilterType::Highpass,
            wt::FilterType::Bandpass => FilterType::Bandpass,
        }
    }
}

// ---------------------------------------------------------------------------
// Avatar conversion
// ---------------------------------------------------------------------------

impl From<&AvatarDef> for wt::AvatarDef {
    fn from(a: &AvatarDef) -> Self {
        wt::AvatarDef {
            spawn_position: a.spawn_position,
            spawn_look_at: a.spawn_look_at,
            pov: a.pov.into(),
            movement_speed: a.movement_speed,
            height: a.height,
            model_entity: a
                .model_entity
                .as_ref()
                .map(|s| wt::EntityRef::Name(s.clone())),
        }
    }
}

impl From<&wt::AvatarDef> for AvatarDef {
    fn from(a: &wt::AvatarDef) -> Self {
        AvatarDef {
            spawn_position: a.spawn_position,
            spawn_look_at: a.spawn_look_at,
            pov: a.pov.into(),
            movement_speed: a.movement_speed,
            height: a.height,
            model_entity: a.model_entity.as_ref().map(entity_ref_to_string),
        }
    }
}

// PointOfView conversions
impl From<PointOfView> for wt::PointOfView {
    fn from(p: PointOfView) -> Self {
        match p {
            PointOfView::FirstPerson => wt::PointOfView::FirstPerson,
            PointOfView::ThirdPerson => wt::PointOfView::ThirdPerson,
        }
    }
}

impl From<wt::PointOfView> for PointOfView {
    fn from(p: wt::PointOfView) -> Self {
        match p {
            wt::PointOfView::FirstPerson => PointOfView::FirstPerson,
            wt::PointOfView::ThirdPerson => PointOfView::ThirdPerson,
        }
    }
}

// ---------------------------------------------------------------------------
// Tour conversion
// ---------------------------------------------------------------------------

impl From<&TourDef> for wt::TourDef {
    fn from(t: &TourDef) -> Self {
        wt::TourDef {
            name: t.name.clone(),
            description: t.description.clone(),
            waypoints: t.waypoints.iter().map(|w| w.into()).collect(),
            speed: t.speed,
            mode: t.mode.into(),
            autostart: t.autostart,
            loop_tour: t.loop_tour,
            pov: t.pov.map(|p| p.into()),
        }
    }
}

impl From<&wt::TourDef> for TourDef {
    fn from(t: &wt::TourDef) -> Self {
        TourDef {
            name: t.name.clone(),
            description: t.description.clone(),
            waypoints: t.waypoints.iter().map(|w| w.into()).collect(),
            speed: t.speed,
            mode: t.mode.into(),
            autostart: t.autostart,
            loop_tour: t.loop_tour,
            pov: t.pov.map(|p| p.into()),
        }
    }
}

impl From<&TourWaypoint> for wt::TourWaypoint {
    fn from(w: &TourWaypoint) -> Self {
        wt::TourWaypoint {
            position: w.position,
            look_at: w.look_at,
            description: w.description.clone(),
            pause_duration: w.pause_duration,
        }
    }
}

impl From<&wt::TourWaypoint> for TourWaypoint {
    fn from(w: &wt::TourWaypoint) -> Self {
        TourWaypoint {
            position: w.position,
            look_at: w.look_at,
            description: w.description.clone(),
            pause_duration: w.pause_duration,
        }
    }
}

// TourMode conversions
impl From<TourMode> for wt::TourMode {
    fn from(m: TourMode) -> Self {
        match m {
            TourMode::Walk => wt::TourMode::Walk,
            TourMode::Fly => wt::TourMode::Fly,
            TourMode::Teleport => wt::TourMode::Teleport,
        }
    }
}

impl From<wt::TourMode> for TourMode {
    fn from(m: wt::TourMode) -> Self {
        match m {
            wt::TourMode::Walk => TourMode::Walk,
            wt::TourMode::Fly => TourMode::Fly,
            wt::TourMode::Teleport => TourMode::Teleport,
        }
    }
}

// ---------------------------------------------------------------------------
// Environment / Camera conversions
// ---------------------------------------------------------------------------

impl From<&EnvironmentCmd> for wt::EnvironmentDef {
    fn from(e: &EnvironmentCmd) -> Self {
        wt::EnvironmentDef {
            background_color: e.background_color,
            ambient_intensity: e.ambient_light,
            ambient_color: e.ambient_color,
            fog_density: None,
            fog_color: None,
        }
    }
}

impl From<&wt::EnvironmentDef> for EnvironmentCmd {
    fn from(e: &wt::EnvironmentDef) -> Self {
        EnvironmentCmd {
            background_color: e.background_color,
            ambient_light: e.ambient_intensity,
            ambient_color: e.ambient_color,
        }
    }
}

impl From<&CameraCmd> for wt::CameraDef {
    fn from(c: &CameraCmd) -> Self {
        wt::CameraDef {
            position: c.position,
            look_at: c.look_at,
            fov_degrees: c.fov_degrees,
        }
    }
}

impl From<&wt::CameraDef> for CameraCmd {
    fn from(c: &wt::CameraDef) -> Self {
        CameraCmd {
            position: c.position,
            look_at: c.look_at,
            fov_degrees: c.fov_degrees,
        }
    }
}

// ---------------------------------------------------------------------------
// ReverbParams conversion
// ---------------------------------------------------------------------------

impl From<&ReverbParams> for wt::audio::ReverbParams {
    fn from(r: &ReverbParams) -> Self {
        wt::audio::ReverbParams {
            room_size: r.room_size,
            damping: r.damping,
            wet: r.wet,
        }
    }
}

impl From<&wt::audio::ReverbParams> for ReverbParams {
    fn from(r: &wt::audio::ReverbParams) -> Self {
        ReverbParams {
            room_size: r.room_size,
            damping: r.damping,
            wet: r.wet,
        }
    }
}

// ---------------------------------------------------------------------------
// WorldTransform ↔ [f32; 3] arrays (for SpawnPrimitiveCmd)
// ---------------------------------------------------------------------------

impl From<&SpawnPrimitiveCmd> for wt::WorldTransform {
    fn from(cmd: &SpawnPrimitiveCmd) -> Self {
        wt::WorldTransform {
            position: cmd.position,
            rotation_degrees: cmd.rotation_degrees,
            scale: cmd.scale,
            visible: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn entity_ref_to_string(r: &wt::EntityRef) -> String {
    match r {
        wt::EntityRef::Id(id) => format!("__id_{}", id.0),
        wt::EntityRef::Name(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_roundtrip() {
        let shapes = vec![
            (PrimitiveShape::Cuboid, {
                let mut m = HashMap::new();
                m.insert("x".into(), 4.0_f32);
                m.insert("y".into(), 3.0);
                m.insert("z".into(), 5.0);
                m
            }),
            (PrimitiveShape::Sphere, {
                let mut m = HashMap::new();
                m.insert("radius".into(), 2.0_f32);
                m
            }),
            (PrimitiveShape::Torus, {
                let mut m = HashMap::new();
                m.insert("major_radius".into(), 3.0_f32);
                m.insert("minor_radius".into(), 0.5);
                m
            }),
        ];

        for (prim, dims) in &shapes {
            let wt_shape = shape_from_primitive(*prim, dims);
            let (back_prim, back_dims) = shape_to_primitive(&wt_shape);
            assert_eq!(*prim as u32, back_prim as u32);
            for (k, v) in dims {
                assert_eq!(back_dims.get(k), Some(v), "key {k} mismatch");
            }
        }
    }

    #[test]
    fn behavior_spin_roundtrip() {
        let cmd_def = BehaviorDef::Spin {
            axis: [0.0, 1.0, 0.0],
            speed: 90.0,
        };
        let wt_def: wt::BehaviorDef = (&cmd_def).into();
        let back: BehaviorDef = (&wt_def).into();
        assert!(matches!(back, BehaviorDef::Spin { speed, .. } if speed == 90.0));
    }

    #[test]
    fn behavior_look_at_string_roundtrip() {
        let cmd_def = BehaviorDef::LookAt {
            target: "player".to_string(),
        };
        let wt_def: wt::BehaviorDef = (&cmd_def).into();
        let back: BehaviorDef = (&wt_def).into();
        assert!(matches!(back, BehaviorDef::LookAt { target } if target == "player"));
    }

    #[test]
    fn ambient_sound_roundtrip() {
        let sounds = vec![
            AmbientSound::Wind {
                speed: 0.7,
                gustiness: 0.3,
            },
            AmbientSound::Rain { intensity: 0.8 },
            AmbientSound::Forest {
                bird_density: 0.5,
                wind: 0.3,
            },
            AmbientSound::Silence,
        ];

        for sound in &sounds {
            let source: wt::AudioSource = sound.into();
            let back = audio_source_to_ambient(&source).expect("should round-trip");
            let source2: wt::AudioSource = (&back).into();
            assert_eq!(
                serde_json::to_string(&source).unwrap(),
                serde_json::to_string(&source2).unwrap()
            );
        }
    }

    #[test]
    fn emitter_sound_roundtrip() {
        let sounds = vec![
            EmitterSound::Fire {
                intensity: 0.8,
                crackle: 0.5,
            },
            EmitterSound::Hum {
                frequency: 440.0,
                warmth: 0.7,
            },
            EmitterSound::Custom {
                waveform: WaveformType::PinkNoise,
                filter_cutoff: 800.0,
                filter_type: FilterType::Lowpass,
            },
        ];

        for sound in &sounds {
            let source: wt::AudioSource = sound.into();
            let back = audio_source_to_emitter(&source).expect("should round-trip");
            let source2: wt::AudioSource = (&back).into();
            assert_eq!(
                serde_json::to_string(&source).unwrap(),
                serde_json::to_string(&source2).unwrap()
            );
        }
    }

    #[test]
    fn avatar_roundtrip() {
        let cmd_avatar = AvatarDef {
            spawn_position: [1.0, 1.8, 5.0],
            spawn_look_at: [0.0, 1.5, 0.0],
            pov: PointOfView::FirstPerson,
            movement_speed: 3.0,
            height: 1.8,
            model_entity: Some("player".to_string()),
        };
        let wt_avatar: wt::AvatarDef = (&cmd_avatar).into();
        let back: AvatarDef = (&wt_avatar).into();
        assert_eq!(back.spawn_position, cmd_avatar.spawn_position);
        assert_eq!(back.model_entity, cmd_avatar.model_entity);
    }

    #[test]
    fn tour_roundtrip() {
        let cmd_tour = TourDef {
            name: "test".to_string(),
            description: Some("A test tour".to_string()),
            waypoints: vec![TourWaypoint {
                position: [0.0, 1.0, 5.0],
                look_at: [0.0, 0.0, 0.0],
                description: None,
                pause_duration: 2.0,
            }],
            speed: 3.0,
            mode: TourMode::Fly,
            autostart: true,
            loop_tour: false,
            pov: Some(PointOfView::FirstPerson),
        };
        let wt_tour: wt::TourDef = (&cmd_tour).into();
        let back: TourDef = (&wt_tour).into();
        assert_eq!(back.name, "test");
        assert_eq!(back.mode, TourMode::Fly);
        assert!(back.autostart);
    }

    #[test]
    fn environment_roundtrip() {
        let env = EnvironmentCmd {
            background_color: Some([0.1, 0.1, 0.2, 1.0]),
            ambient_light: Some(0.3),
            ambient_color: Some([0.5, 0.5, 0.6, 1.0]),
        };
        let wt_env: wt::EnvironmentDef = (&env).into();
        let back: EnvironmentCmd = (&wt_env).into();
        assert_eq!(back.background_color, env.background_color);
        assert_eq!(back.ambient_light, env.ambient_light);
    }

    #[test]
    fn camera_roundtrip() {
        let cam = CameraCmd {
            position: [5.0, 5.0, 5.0],
            look_at: [0.0, 0.0, 0.0],
            fov_degrees: 60.0,
        };
        let wt_cam: wt::CameraDef = (&cam).into();
        let back: CameraCmd = (&wt_cam).into();
        assert_eq!(back.position, cam.position);
        assert_eq!(back.fov_degrees, 60.0);
    }

    #[test]
    fn spawn_primitive_alpha_mode_blend() {
        let cmd = SpawnPrimitiveCmd {
            name: "test".to_string(),
            shape: PrimitiveShape::Cuboid,
            dimensions: Default::default(),
            position: [0.0, 0.0, 0.0],
            rotation_degrees: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0, 0.0, 0.0, 0.0],
            alpha_mode: Some("blend".to_string()),
            unlit: Some(true),
            parent: None,
        };
        let mat: wt::MaterialDef = (&cmd).into();
        assert_eq!(mat.alpha_mode, Some(wt::AlphaModeDef::Blend));
        assert_eq!(mat.unlit, Some(true));
    }

    #[test]
    fn spawn_primitive_alpha_mode_mask() {
        let cmd = SpawnPrimitiveCmd {
            name: "test".to_string(),
            shape: PrimitiveShape::Cuboid,
            dimensions: Default::default(),
            position: [0.0, 0.0, 0.0],
            rotation_degrees: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0, 0.0, 0.0, 0.0],
            alpha_mode: Some("mask:0.75".to_string()),
            unlit: None,
            parent: None,
        };
        let mat: wt::MaterialDef = (&cmd).into();
        assert_eq!(mat.alpha_mode, Some(wt::AlphaModeDef::Mask(0.75)));
    }
}
