//! HTML/Three.js export — generates a self-contained static website from a WorldManifest.
//!
//! Converts the entire scene (shapes, materials, lights, behaviors, audio, environment,
//! camera) into a single HTML file using Three.js for 3D and the Web Audio API for
//! procedural sound synthesis.
//!
//! The exported HTML is designed to be embeddable in other websites via `<iframe>`.
//! It includes Open Graph meta tags, responsive sizing, and a postMessage API
//! for parent-frame communication.

use localgpt_world_types as wt;
use std::fmt::Write;

/// Extract name string from an EntityRef.
fn entity_ref_name(r: &wt::EntityRef) -> &str {
    match r {
        wt::EntityRef::Name(name) => name.as_str(),
        wt::EntityRef::Id(id) => {
            // IDs don't have names — fall back to empty string
            // (entityMap lookup will simply not find it)
            let _ = id;
            ""
        }
    }
}

/// Linear→sRGB conversion for a single channel.
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Format [r,g,b,a] linear color as a Three.js hex integer literal.
fn color_hex(c: &[f32; 4]) -> String {
    let r = (linear_to_srgb(c[0]).clamp(0.0, 1.0) * 255.0) as u8;
    let g = (linear_to_srgb(c[1]).clamp(0.0, 1.0) * 255.0) as u8;
    let b = (linear_to_srgb(c[2]).clamp(0.0, 1.0) * 255.0) as u8;
    format!("0x{:02x}{:02x}{:02x}", r, g, b)
}

/// Generate a complete self-contained HTML file from a WorldManifest.
pub fn generate_html(manifest: &wt::WorldManifest) -> String {
    let mut js = String::with_capacity(16384);

    // ---- Scene setup ----
    writeln!(js, "const scene = new THREE.Scene();").unwrap();

    // Environment
    if let Some(ref env) = manifest.environment {
        if let Some(ref bg) = env.background_color {
            writeln!(js, "scene.background = new THREE.Color({});", color_hex(bg)).unwrap();
        }
        if let Some(density) = env.fog_density
            && density > 0.0
        {
            let fog_color = env
                .fog_color
                .as_ref()
                .map(color_hex)
                .unwrap_or_else(|| "0xcccccc".into());
            let near = 1.0;
            let far = 100.0 / density.max(0.01);
            writeln!(
                js,
                "scene.fog = new THREE.Fog({}, {:.1}, {:.1});",
                fog_color, near, far
            )
            .unwrap();
        }
    }

    // Ambient light
    let (amb_color, amb_intensity) = match &manifest.environment {
        Some(env) => {
            let c = env
                .ambient_color
                .as_ref()
                .map(color_hex)
                .unwrap_or_else(|| "0xffffff".into());
            let i = env.ambient_intensity.unwrap_or(0.3);
            (c, i)
        }
        None => ("0xffffff".into(), 0.3),
    };
    writeln!(
        js,
        "scene.add(new THREE.AmbientLight({}, {:.2}));",
        amb_color, amb_intensity
    )
    .unwrap();

    // Camera — prefer explicit camera, fall back to avatar spawn, then default
    let cam = manifest.camera.as_ref().cloned().unwrap_or_else(|| {
        if let Some(ref avatar) = manifest.avatar {
            wt::CameraDef {
                position: avatar.spawn_position,
                look_at: avatar.spawn_look_at,
                ..Default::default()
            }
        } else {
            wt::CameraDef::default()
        }
    });
    writeln!(
        js,
        "const camera = new THREE.PerspectiveCamera({:.1}, window.innerWidth / window.innerHeight, 0.1, 1000);",
        cam.fov_degrees
    ).unwrap();
    writeln!(
        js,
        "camera.position.set({:.4}, {:.4}, {:.4});",
        cam.position[0], cam.position[1], cam.position[2]
    )
    .unwrap();

    // Renderer — append to #scene container (works in both standalone and embedded)
    writeln!(js, "const container = document.getElementById('scene');").unwrap();
    writeln!(
        js,
        "const renderer = new THREE.WebGLRenderer({{ antialias: true }});"
    )
    .unwrap();
    writeln!(js, "renderer.setPixelRatio(window.devicePixelRatio);").unwrap();
    writeln!(js, "renderer.shadowMap.enabled = true;").unwrap();
    writeln!(js, "renderer.shadowMap.type = THREE.PCFSoftShadowMap;").unwrap();
    writeln!(js, "renderer.toneMapping = THREE.ACESFilmicToneMapping;").unwrap();
    writeln!(js, "renderer.toneMappingExposure = 1.0;").unwrap();
    writeln!(js, "renderer.physicallyCorrectLights = true;").unwrap();
    writeln!(js, "container.appendChild(renderer.domElement);").unwrap();

    // Container-aware sizing (supports both full-page and embedded containers)
    writeln!(
        js,
        r#"function resizeRenderer() {{
  const w = container.clientWidth || window.innerWidth;
  const h = container.clientHeight || window.innerHeight;
  renderer.setSize(w, h);
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
}}"#
    )
    .unwrap();
    writeln!(js, "resizeRenderer();").unwrap();
    writeln!(js, "new ResizeObserver(resizeRenderer).observe(container);").unwrap();
    writeln!(js, "window.addEventListener('resize', resizeRenderer);").unwrap();

    // OrbitControls
    writeln!(
        js,
        "const controls = new OrbitControls(camera, renderer.domElement);"
    )
    .unwrap();
    writeln!(
        js,
        "controls.target.set({:.4}, {:.4}, {:.4});",
        cam.look_at[0], cam.look_at[1], cam.look_at[2]
    )
    .unwrap();
    writeln!(js, "controls.enableDamping = true;").unwrap();
    writeln!(js, "controls.dampingFactor = 0.05;").unwrap();
    writeln!(js, "controls.update();").unwrap();

    // ---- Build entity ID→name map for behaviors that reference other entities ----
    writeln!(js, "const entityMap = {{}};").unwrap();

    // ---- Entities ----
    // Build a parent→children map for hierarchy
    let mut children_map: std::collections::HashMap<u64, Vec<usize>> =
        std::collections::HashMap::new();
    let mut root_indices: Vec<usize> = Vec::new();
    for (i, entity) in manifest.entities.iter().enumerate() {
        if let Some(parent_id) = entity.parent {
            children_map.entry(parent_id.0).or_default().push(i);
        } else {
            root_indices.push(i);
        }
    }

    // Emit each entity
    for (i, entity) in manifest.entities.iter().enumerate() {
        emit_entity(&mut js, entity, i);
    }

    // Wire up hierarchy
    for (i, entity) in manifest.entities.iter().enumerate() {
        if let Some(parent_id) = entity.parent {
            // Find parent index
            if let Some(pi) = manifest.entities.iter().position(|e| e.id == parent_id) {
                writeln!(js, "e{}.add(e{});", pi, i).unwrap();
            }
        }
    }

    // Add root entities to scene
    for &i in &root_indices {
        writeln!(js, "scene.add(e{});", i).unwrap();
    }

    // Register entity names for behavior lookups
    for (i, entity) in manifest.entities.iter().enumerate() {
        writeln!(js, "entityMap['{}'] = e{};", entity.name.as_str(), i).unwrap();
    }

    // ---- Behaviors ----
    writeln!(js, "const behaviorTime = {{ value: 0 }};").unwrap();
    writeln!(js, "const behaviors = [];").unwrap();
    for (i, entity) in manifest.entities.iter().enumerate() {
        for behavior in &entity.behaviors {
            emit_behavior(&mut js, behavior, i, &manifest.entities);
        }
    }

    // ---- Audio ----
    emit_audio_system(&mut js, manifest);

    // ---- Animation loop ----
    writeln!(js, "const clock = new THREE.Clock();").unwrap();
    writeln!(js, "function animate() {{").unwrap();
    writeln!(js, "  requestAnimationFrame(animate);").unwrap();
    writeln!(js, "  const dt = clock.getDelta();").unwrap();
    writeln!(js, "  behaviorTime.value += dt;").unwrap();
    writeln!(js, "  const t = behaviorTime.value;").unwrap();
    writeln!(js, "  for (const b of behaviors) b(dt, t);").unwrap();
    writeln!(
        js,
        "  if (typeof updateMovement === 'function') updateMovement(dt);"
    )
    .unwrap();
    writeln!(
        js,
        "  if (typeof updateTour === 'function') updateTour(dt);"
    )
    .unwrap();
    writeln!(js, "  controls.update();").unwrap();
    writeln!(js, "  renderer.render(scene, camera);").unwrap();
    writeln!(js, "}}").unwrap();
    writeln!(js, "animate();").unwrap();

    // ---- WASD keyboard navigation ----
    let move_speed = manifest
        .avatar
        .as_ref()
        .map(|a| a.movement_speed)
        .unwrap_or(5.0);
    emit_keyboard_controls(&mut js, move_speed);

    // ---- Guided tours ----
    emit_tours(&mut js, manifest);

    // ---- postMessage API for parent-frame communication ----
    emit_embed_api(&mut js);

    // ---- Wrap in HTML ----
    let title = &manifest.meta.name;
    let description = manifest
        .meta
        .description
        .as_deref()
        .unwrap_or("A 3D scene exported from LocalGPT Gen");

    let entity_count = manifest.entities.len();
    let has_tours = !manifest.tours.is_empty();
    let has_audio = manifest.entities.iter().any(|e| e.audio.is_some());

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{description}">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{description}">
<meta property="og:type" content="website">
<meta name="twitter:card" content="summary">
<meta name="twitter:title" content="{title}">
<meta name="twitter:description" content="{description}">
<meta name="generator" content="LocalGPT Gen">
<script type="application/ld+json">
{{
  "@context": "https://schema.org",
  "@type": "3DModel",
  "name": "{title}",
  "description": "{description}",
  "encodingFormat": "text/html",
  "creator": {{ "@type": "SoftwareApplication", "name": "LocalGPT Gen" }}
}}
</script>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
html, body {{ width: 100%; height: 100%; overflow: hidden; background: #000; }}
#scene {{ width: 100%; height: 100%; }}
#info {{
  position: absolute; top: 10px; left: 10px;
  color: #fff; font: 14px/1.4 system-ui, sans-serif;
  background: rgba(0,0,0,0.5); padding: 8px 12px; border-radius: 6px;
  pointer-events: none; user-select: none;
}}
#audio-btn {{
  position: absolute; bottom: 20px; right: 20px;
  background: rgba(0,0,0,0.6); color: #fff; border: 1px solid rgba(255,255,255,0.3);
  padding: 8px 16px; border-radius: 6px; cursor: pointer;
  font: 14px system-ui, sans-serif;
}}
#audio-btn:hover, #tour-btn:hover {{ background: rgba(0,0,0,0.8); }}
#tour-btn {{
  position: absolute; bottom: 20px; right: 140px;
  background: rgba(0,0,0,0.6); color: #fff; border: 1px solid rgba(255,255,255,0.3);
  padding: 8px 16px; border-radius: 6px; cursor: pointer;
  font: 14px system-ui, sans-serif; display: none;
}}
#tour-desc {{
  position: absolute; bottom: 60px; left: 50%; transform: translateX(-50%);
  background: rgba(0,0,0,0.7); color: #fff; padding: 10px 20px; border-radius: 8px;
  font: 14px/1.5 system-ui, sans-serif; max-width: 500px; text-align: center;
  display: none; pointer-events: none;
}}
</style>
</head>
<body>
<div id="scene"></div>
<div id="info">{title}<br><small>Drag to orbit &middot; Scroll to zoom &middot; WASD to move</small><br><small>{entity_count} entities{tours_label}{audio_label}</small></div>
<button id="audio-btn" style="display:none" onclick="toggleAudio()">Sound On</button>
<button id="tour-btn">Start Tour</button>
<div id="tour-desc"></div>
<script type="importmap">
{{
  "imports": {{
    "three": "https://unpkg.com/three@0.170.0/build/three.module.js",
    "three/addons/": "https://unpkg.com/three@0.170.0/examples/jsm/"
  }}
}}
</script>
<script type="module">
import * as THREE from 'three';
import {{ OrbitControls }} from 'three/addons/controls/OrbitControls.js';
{js}
</script>
</body>
</html>
"#,
        title = html_escape(title),
        description = html_escape(description),
        entity_count = entity_count,
        tours_label = if has_tours { " &middot; Tours" } else { "" },
        audio_label = if has_audio { " &middot; Audio" } else { "" },
        js = js,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Emit JavaScript that creates a Three.js object for one entity.
fn emit_entity(js: &mut String, entity: &wt::WorldEntity, idx: usize) {
    let var = format!("e{}", idx);
    let t = &entity.transform;

    let has_shape = entity.shape.is_some();
    let has_light = entity.light.is_some();

    // Create geometry + material for shapes
    if let Some(ref shape) = entity.shape {
        emit_geometry(js, shape, &var);
        emit_material(js, entity.material.as_ref(), &var);
        writeln!(js, "const {v} = new THREE.Mesh({v}_geo, {v}_mat);", v = var).unwrap();
        // Enable shadows on meshes
        writeln!(js, "{}.castShadow = true;", var).unwrap();
        writeln!(js, "{}.receiveShadow = true;", var).unwrap();
    } else if entity.mesh_asset.is_some() {
        // Imported glTF mesh — can't inline binary data; emit a wireframe placeholder
        writeln!(
            js,
            "const {v}_geo = new THREE.BoxGeometry(1, 1, 1); \
             const {v}_mat = new THREE.MeshBasicMaterial({{ color: 0x888888, wireframe: true }}); \
             const {v} = new THREE.Mesh({v}_geo, {v}_mat); // placeholder for imported mesh: {}",
            v = var,
            entity
                .mesh_asset
                .as_ref()
                .map(|a| a.path.as_str())
                .unwrap_or("unknown")
        )
        .unwrap();
    } else if !has_light {
        // Group / empty entity
        writeln!(js, "const {} = new THREE.Group();", var).unwrap();
    }

    // Light (can coexist with shape via a group)
    if let Some(ref light) = entity.light {
        if has_shape {
            // Already created a mesh; add light as child
            let light_var = format!("{}_light", var);
            emit_light(js, light, t, &light_var);
            writeln!(js, "{}.add({});", var, light_var).unwrap();
        } else {
            // Light-only entity
            emit_light(js, light, t, &var);
        }
    }

    // Transform
    writeln!(
        js,
        "{v}.position.set({x:.4}, {y:.4}, {z:.4});",
        v = var,
        x = t.position[0],
        y = t.position[1],
        z = t.position[2]
    )
    .unwrap();

    // Rotation (Euler XYZ in degrees → radians)
    let rx = t.rotation_degrees[0].to_radians();
    let ry = t.rotation_degrees[1].to_radians();
    let rz = t.rotation_degrees[2].to_radians();
    if rx != 0.0 || ry != 0.0 || rz != 0.0 {
        writeln!(js, "{}.rotation.set({:.4}, {:.4}, {:.4});", var, rx, ry, rz).unwrap();
    }

    // Scale
    if t.scale != [1.0, 1.0, 1.0] {
        writeln!(
            js,
            "{}.scale.set({:.4}, {:.4}, {:.4});",
            var, t.scale[0], t.scale[1], t.scale[2]
        )
        .unwrap();
    }

    // Visibility
    if !t.visible {
        writeln!(js, "{}.visible = false;", var).unwrap();
    }

    // Name
    writeln!(js, "{}.name = '{}';", var, entity.name.as_str()).unwrap();
}

/// Emit Three.js geometry constructor.
fn emit_geometry(js: &mut String, shape: &wt::Shape, var: &str) {
    match shape {
        wt::Shape::Cuboid { x, y, z } => {
            writeln!(
                js,
                "const {}_geo = new THREE.BoxGeometry({:.4}, {:.4}, {:.4});",
                var, x, y, z
            )
            .unwrap();
        }
        wt::Shape::Sphere { radius } => {
            writeln!(
                js,
                "const {}_geo = new THREE.SphereGeometry({:.4}, 32, 24);",
                var, radius
            )
            .unwrap();
        }
        wt::Shape::Cylinder { radius, height } => {
            writeln!(
                js,
                "const {}_geo = new THREE.CylinderGeometry({r:.4}, {r:.4}, {h:.4}, 32);",
                var,
                r = radius,
                h = height
            )
            .unwrap();
        }
        wt::Shape::Cone { radius, height } => {
            writeln!(
                js,
                "const {}_geo = new THREE.ConeGeometry({:.4}, {:.4}, 32);",
                var, radius, height
            )
            .unwrap();
        }
        wt::Shape::Capsule {
            radius,
            half_length,
        } => {
            writeln!(
                js,
                "const {}_geo = new THREE.CapsuleGeometry({:.4}, {:.4}, 16, 32);",
                var,
                radius,
                half_length * 2.0
            )
            .unwrap();
        }
        wt::Shape::Torus {
            major_radius,
            minor_radius,
        } => {
            writeln!(
                js,
                "const {}_geo = new THREE.TorusGeometry({:.4}, {:.4}, 24, 48);",
                var, major_radius, minor_radius
            )
            .unwrap();
        }
        wt::Shape::Plane { x, z } => {
            // Three.js PlaneGeometry faces +Z by default; rotate to lie on XZ plane
            writeln!(
                js,
                "const {v}_geo = new THREE.PlaneGeometry({x:.4}, {z:.4});",
                v = var,
                x = x,
                z = z
            )
            .unwrap();
            writeln!(js, "{}_geo.rotateX(-Math.PI / 2);", var).unwrap();
        }
        wt::Shape::Pyramid {
            base_x,
            base_z,
            height,
        } => {
            // Three.js ConeGeometry with 4 radial segments = square pyramid
            let radius = (*base_x).max(*base_z) / 2.0;
            writeln!(
                js,
                "const {v}_geo = new THREE.ConeGeometry({r:.4}, {h:.4}, 4);",
                v = var,
                r = radius,
                h = height
            )
            .unwrap();
            // Rotate to align base with XZ plane
            writeln!(js, "{}_geo.rotateY(Math.PI / 4);", var).unwrap();
        }
        wt::Shape::Tetrahedron { radius } => {
            writeln!(
                js,
                "const {v}_geo = new THREE.TetrahedronGeometry({r:.4});",
                v = var,
                r = radius
            )
            .unwrap();
        }
        wt::Shape::Icosahedron { radius } => {
            writeln!(
                js,
                "const {v}_geo = new THREE.IcosahedronGeometry({r:.4}, 0);",
                v = var,
                r = radius
            )
            .unwrap();
        }
        wt::Shape::Wedge { x, y, z } => {
            // Wedge is a triangular prism - use BufferGeometry
            writeln!(
                js,
                r#"const {v}_geo = new THREE.BufferGeometry();
const {v}_hx = {x:.4} / 2, {v}_hy = {y:.4} / 2, {v}_hz = {z:.4} / 2;
const {v}_vertices = new Float32Array([
  // Bottom face (2 triangles)
  -{v}_hx, -{v}_hy, -{v}_hz,
  -{v}_hx, -{v}_hy, {v}_hz,
  {v}_hx, -{v}_hy, -{v}_hz,
  {v}_hx, -{v}_hy, -{v}_hz,
  -{v}_hx, -{v}_hy, {v}_hz,
  {v}_hx, -{v}_hy, {v}_hz,
  // Front face (2 triangles)
  -{v}_hx, -{v}_hy, -{v}_hz,
  {v}_hx, -{v}_hy, -{v}_hz,
  {v}_hx, {v}_hy, -{v}_hz,
  -{v}_hx, -{v}_hy, -{v}_hz,
  {v}_hx, {v}_hy, -{v}_hz,
  -{v}_hx, {v}_hy, -{v}_hz,
  // Back slope (2 triangles)
  -{v}_hx, -{v}_hy, {v}_hz,
  -{v}_hx, {v}_hy, -{v}_hz,
  {v}_hx, {v}_hy, -{v}_hz,
  -{v}_hx, -{v}_hy, {v}_hz,
  {v}_hx, {v}_hy, -{v}_hz,
  {v}_hx, -{v}_hy, {v}_hz,
  // Left face (2 triangles)
  -{v}_hx, -{v}_hy, {v}_hz,
  -{v}_hx, -{v}_hy, -{v}_hz,
  -{v}_hx, {v}_hy, -{v}_hz,
  // Right face (2 triangles)
  {v}_hx, -{v}_hy, -{v}_hz,
  {v}_hx, -{v}_hy, {v}_hz,
  {v}_hx, {v}_hy, -{v}_hz,
]);
{v}_geo.setAttribute('position', new THREE.BufferAttribute({v}_vertices, 3));
{v}_geo.computeVertexNormals();"#,
                v = var,
                x = x,
                y = y,
                z = z
            )
            .unwrap();
        }
    }
}

/// Emit Three.js material constructor.
fn emit_material(js: &mut String, mat: Option<&wt::MaterialDef>, var: &str) {
    let mat = mat.cloned().unwrap_or_default();
    let color = color_hex(&mat.color);
    let opacity = mat.color[3];
    let transparent = opacity < 1.0
        || matches!(
            mat.alpha_mode,
            Some(
                wt::AlphaModeDef::Blend
                    | wt::AlphaModeDef::Add
                    | wt::AlphaModeDef::Multiply
                    | wt::AlphaModeDef::Mask(_)
            )
        );
    let unlit = mat.unlit.unwrap_or(false);

    if unlit {
        writeln!(
            js,
            "const {v}_mat = new THREE.MeshBasicMaterial({{ color: {c}, opacity: {o:.2}, transparent: {t} }});",
            v = var,
            c = color,
            o = opacity,
            t = transparent,
        ).unwrap();
    } else {
        let emissive = color_hex(&mat.emissive);
        let emissive_intensity =
            if mat.emissive[0] > 0.0 || mat.emissive[1] > 0.0 || mat.emissive[2] > 0.0 {
                // Alpha channel of emissive used as intensity multiplier
                mat.emissive[3].max(1.0)
            } else {
                0.0
            };
        writeln!(
            js,
            "const {v}_mat = new THREE.MeshStandardMaterial({{ color: {c}, metalness: {m:.2}, roughness: {r:.2}, emissive: {e}, emissiveIntensity: {ei:.2}, opacity: {o:.2}, transparent: {t}, side: {side} }});",
            v = var,
            c = color,
            m = mat.metallic,
            r = mat.roughness,
            e = emissive,
            ei = emissive_intensity,
            o = opacity,
            t = transparent,
            side = if mat.double_sided.unwrap_or(false) { "THREE.DoubleSide" } else { "THREE.FrontSide" },
        ).unwrap();
    }

    // Alpha mode — set blending mode for Add/Multiply
    match mat.alpha_mode {
        Some(wt::AlphaModeDef::Add) => {
            writeln!(js, "{v}_mat.blending = THREE.AdditiveBlending;", v = var).unwrap();
        }
        Some(wt::AlphaModeDef::Multiply) => {
            writeln!(js, "{v}_mat.blending = THREE.MultiplyBlending;", v = var).unwrap();
        }
        Some(wt::AlphaModeDef::Mask(cutoff)) => {
            writeln!(js, "{v}_mat.alphaTest = {c:.2};", v = var, c = cutoff).unwrap();
        }
        _ => {}
    }

    // Reflectance — map Bevy's 0..1 reflectance to Three.js MeshPhysicalMaterial IOR
    // Note: MeshStandardMaterial doesn't directly support reflectance, but
    // we approximate by adjusting metalness/roughness when reflectance differs from default.
    // Bevy reflectance 0.5 = 4% (F0 = 0.04), which is the Three.js default for dielectrics.
    if let Some(refl) = mat.reflectance {
        if !unlit && (refl - 0.5).abs() > 0.01 {
            // F0 = 0.16 * reflectance^2 (Bevy formula)
            // Map to IOR: n = (1 + sqrt(F0)) / (1 - sqrt(F0))
            let f0 = 0.16 * refl * refl;
            let sqrt_f0 = f0.sqrt();
            let ior = (1.0 + sqrt_f0) / (1.0 - sqrt_f0).max(0.001);
            writeln!(
                js,
                "// reflectance={r:.2} → F0={f0:.4} → IOR={ior:.2} (upgrade to MeshPhysicalMaterial for full support)",
                r = refl,
                f0 = f0,
                ior = ior,
            )
            .unwrap();
        }
    }
}

/// Emit Three.js light.
fn emit_light(js: &mut String, light: &wt::LightDef, transform: &wt::WorldTransform, var: &str) {
    let color = color_hex(&light.color);
    match light.light_type {
        wt::LightType::Directional => {
            // Bevy directional light illuminance → Three.js intensity
            // Three.js DirectionalLight intensity 1.0 ≈ Bevy 10000 lux
            let intensity = (light.intensity / 10000.0).max(0.01);
            writeln!(
                js,
                "const {v} = new THREE.DirectionalLight({c}, {i:.3});",
                v = var,
                c = color,
                i = intensity
            )
            .unwrap();
            if light.shadows {
                writeln!(js, "{v}.castShadow = true;", v = var).unwrap();
                writeln!(js, "{v}.shadow.mapSize.width = 2048;", v = var).unwrap();
                writeln!(js, "{v}.shadow.mapSize.height = 2048;", v = var).unwrap();
                writeln!(js, "{v}.shadow.camera.near = 0.5;", v = var).unwrap();
                writeln!(js, "{v}.shadow.camera.far = 100;", v = var).unwrap();
                writeln!(js, "{v}.shadow.camera.left = -20;", v = var).unwrap();
                writeln!(js, "{v}.shadow.camera.right = 20;", v = var).unwrap();
                writeln!(js, "{v}.shadow.camera.top = 20;", v = var).unwrap();
                writeln!(js, "{v}.shadow.camera.bottom = -20;", v = var).unwrap();
            }
            // Position from transform (Three.js directional light shines from position toward target at origin)
            writeln!(
                js,
                "{v}.position.set({x:.4}, {y:.4}, {z:.4});",
                v = var,
                x = transform.position[0],
                y = transform.position[1],
                z = transform.position[2]
            )
            .unwrap();
        }
        wt::LightType::Point => {
            // Bevy point light intensity is in lumens.
            // Three.js with physicallyCorrectLights uses candela (lumens / 4π).
            let candela = light.intensity / (4.0 * std::f32::consts::PI);
            let distance = light.range.unwrap_or(50.0);
            writeln!(
                js,
                "const {v} = new THREE.PointLight({c}, {i:.2}, {d:.1}, 2);",
                v = var,
                c = color,
                i = candela,
                d = distance
            )
            .unwrap();
            if light.shadows {
                writeln!(js, "{v}.castShadow = true;", v = var).unwrap();
            }
        }
        wt::LightType::Spot => {
            // Bevy spot light intensity is in lumens → convert to candela
            let candela = light.intensity / (4.0 * std::f32::consts::PI);
            let distance = light.range.unwrap_or(50.0);
            let angle = light.outer_angle.unwrap_or(0.5);
            let penumbra =
                if let (Some(outer), Some(inner)) = (light.outer_angle, light.inner_angle) {
                    if outer > 0.0 {
                        1.0 - (inner / outer)
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
            writeln!(
                js,
                "const {v} = new THREE.SpotLight({c}, {i:.2}, {d:.1}, {a:.4}, {p:.2}, 2);",
                v = var,
                c = color,
                i = candela,
                d = distance,
                a = angle,
                p = penumbra
            )
            .unwrap();
            if light.shadows {
                writeln!(js, "{v}.castShadow = true;", v = var).unwrap();
            }
        }
    }
}

/// Emit JavaScript behavior callbacks.
fn emit_behavior(
    js: &mut String,
    behavior: &wt::BehaviorDef,
    entity_idx: usize,
    entities: &[wt::WorldEntity],
) {
    let var = format!("e{}", entity_idx);
    match behavior {
        wt::BehaviorDef::Spin { axis, speed } => {
            let speed_rad = speed.to_radians();
            writeln!(
                js,
                "behaviors.push((dt, t) => {{ {v}.rotateOnAxis(new THREE.Vector3({ax:.4},{ay:.4},{az:.4}).normalize(), {s:.6} * dt); }});",
                v = var, ax = axis[0], ay = axis[1], az = axis[2], s = speed_rad
            ).unwrap();
        }
        wt::BehaviorDef::Bob {
            axis,
            amplitude,
            frequency,
            phase,
        } => {
            let phase_rad = phase.to_radians();
            let entity = &entities[entity_idx];
            let bp = &entity.transform.position;
            writeln!(
                js,
                "behaviors.push((dt, t) => {{ const off = Math.sin(t * {f:.4} * Math.PI * 2 + {p:.4}) * {a:.4}; {v}.position.set({bx:.4} + {ax:.4}*off, {by:.4} + {ay:.4}*off, {bz:.4} + {az:.4}*off); }});",
                v = var, f = frequency, p = phase_rad, a = amplitude,
                ax = axis[0], ay = axis[1], az = axis[2],
                bx = bp[0], by = bp[1], bz = bp[2]
            ).unwrap();
        }
        wt::BehaviorDef::Orbit {
            center,
            center_point,
            radius,
            speed,
            axis,
            phase,
            tilt,
        } => {
            // Resolve center position
            let cp = if let Some(entity_ref) = center {
                // Find the center entity's position
                let center_name = entity_ref_name(entity_ref);
                if let Some(ce) = entities.iter().find(|e| e.name.as_str() == center_name) {
                    ce.transform.position
                } else {
                    [0.0, 0.0, 0.0]
                }
            } else {
                center_point.unwrap_or([0.0, 0.0, 0.0])
            };
            let speed_rad = speed.to_radians();
            let phase_rad = phase.to_radians();
            let tilt_rad = tilt.to_radians();
            // Simplified orbit: rotate in a circle on the plane perpendicular to axis
            // For Y-axis orbit (most common), x = cos, z = sin
            writeln!(
                js,
                "behaviors.push((dt, t) => {{ \
                const angle = t * {s:.6} + {p:.4}; \
                const ax = new THREE.Vector3({axx:.4},{axy:.4},{axz:.4}).normalize(); \
                const up = Math.abs(ax.y) < 0.99 ? new THREE.Vector3(0,1,0) : new THREE.Vector3(1,0,0); \
                const right = new THREE.Vector3().crossVectors(up, ax).normalize(); \
                const fwd = new THREE.Vector3().crossVectors(ax, right).normalize(); \
                const tiltR = {tilt:.4}; \
                const r = {r:.4}; \
                const cx = {cx:.4}, cy = {cy:.4}, cz = {cz:.4}; \
                const cosA = Math.cos(angle), sinA = Math.sin(angle); \
                const dx = right.x * cosA + fwd.x * sinA; \
                const dy = right.y * cosA + fwd.y * sinA + Math.sin(tiltR) * cosA; \
                const dz = right.z * cosA + fwd.z * sinA; \
                {v}.position.set(cx + dx*r, cy + dy*r, cz + dz*r); \
                }});",
                v = var, s = speed_rad, p = phase_rad, r = radius,
                axx = axis[0], axy = axis[1], axz = axis[2],
                tilt = tilt_rad,
                cx = cp[0], cy = cp[1], cz = cp[2]
            ).unwrap();
        }
        wt::BehaviorDef::Pulse {
            min_scale,
            max_scale,
            frequency,
        } => {
            let entity = &entities[entity_idx];
            let bs = &entity.transform.scale;
            writeln!(
                js,
                "behaviors.push((dt, t) => {{ \
                const s = {mn:.4} + ({mx:.4} - {mn:.4}) * (Math.sin(t * {f:.4} * Math.PI * 2) * 0.5 + 0.5); \
                {v}.scale.set({bx:.4}*s, {by:.4}*s, {bz:.4}*s); \
                }});",
                v = var, mn = min_scale, mx = max_scale, f = frequency,
                bx = bs[0], by = bs[1], bz = bs[2]
            ).unwrap();
        }
        wt::BehaviorDef::Bounce {
            height,
            gravity,
            damping,
            surface_y,
        } => {
            writeln!(
                js,
                "{{ \
                let vel = 0, posY = {sy:.4} + {h:.4}; \
                behaviors.push((dt, t) => {{ \
                vel -= {g:.4} * dt; posY += vel * dt; \
                if (posY <= {sy:.4}) {{ posY = {sy:.4}; vel = Math.abs(vel) * {d:.4}; if (vel < 0.1) vel = Math.sqrt(2*{g:.4}*{h:.4}) * {d:.4}; }} \
                {v}.position.y = posY; \
                }}); }}",
                v = var, h = height, g = gravity, d = damping, sy = surface_y
            ).unwrap();
        }
        wt::BehaviorDef::PathFollow {
            waypoints,
            speed,
            mode,
            orient_to_path,
        } => {
            if waypoints.len() < 2 {
                return;
            }
            let wp_json: Vec<String> = waypoints
                .iter()
                .map(|w| format!("[{:.4},{:.4},{:.4}]", w[0], w[1], w[2]))
                .collect();
            let mode_str = match mode {
                wt::PathMode::Loop => "loop",
                wt::PathMode::PingPong => "ping_pong",
                wt::PathMode::Once => "once",
            };
            writeln!(
                js,
                "{{ \
                const wp = [{}]; let seg = 0, frac = 0, dir = 1; \
                behaviors.push((dt, t) => {{ \
                const spd = {s:.4}; \
                const a = wp[seg], b = wp[seg + dir < 0 ? seg - 1 : (seg + 1) % wp.length]; \
                if (!b) return; \
                const dx = b[0]-a[0], dy = b[1]-a[1], dz = b[2]-a[2]; \
                const len = Math.sqrt(dx*dx+dy*dy+dz*dz) || 1; \
                frac += (spd * dt) / len; \
                if (frac >= 1) {{ frac = 0; \
                  if ('{m}' === 'ping_pong') {{ dir *= -1; seg += dir; if (seg < 0) {{ seg = 0; dir = 1; }} if (seg >= wp.length-1) {{ seg = wp.length-1; dir = -1; }} }} \
                  else if ('{m}' === 'loop') {{ seg = (seg + 1) % wp.length; }} \
                  else {{ seg = Math.min(seg + 1, wp.length - 2); }} \
                }} \
                const nx = wp[seg]; const ni = (seg+1)%wp.length; const nb = wp[ni]; \
                {v}.position.set(nx[0]+(nb[0]-nx[0])*frac, nx[1]+(nb[1]-nx[1])*frac, nx[2]+(nb[2]-nx[2])*frac); \
                {orient} \
                }}); }}",
                wp_json.join(","),
                v = var,
                s = speed,
                m = mode_str,
                orient = if *orient_to_path {
                    format!("const look = new THREE.Vector3(nb[0]-nx[0],nb[1]-nx[1],nb[2]-nx[2]); if (look.length()>0) {{ const tgt = {v}.position.clone().add(look); {v}.lookAt(tgt); }}", v = var)
                } else {
                    String::new()
                }
            ).unwrap();
        }
        wt::BehaviorDef::LookAt { target } => {
            let target_name = entity_ref_name(target);
            writeln!(
                js,
                "behaviors.push((dt, t) => {{ const tgt = entityMap['{}']; if (tgt) {}.lookAt(tgt.position); }});",
                target_name, var
            ).unwrap();
        }
    }
}

/// Emit the Web Audio API procedural audio system.
fn emit_audio_system(js: &mut String, manifest: &wt::WorldManifest) {
    // Collect all entities that have audio
    let audio_entities: Vec<(usize, &wt::AudioDef)> = manifest
        .entities
        .iter()
        .enumerate()
        .filter_map(|(i, e)| e.audio.as_ref().map(|a| (i, a)))
        .collect();

    if audio_entities.is_empty() {
        return;
    }

    writeln!(js, "// ---- Audio System ----").unwrap();
    writeln!(js, "let audioCtx = null;").unwrap();
    writeln!(js, "let audioStarted = false;").unwrap();
    writeln!(
        js,
        "document.getElementById('audio-btn').style.display = 'block';"
    )
    .unwrap();

    writeln!(js, "function toggleAudio() {{").unwrap();
    writeln!(js, "  const btn = document.getElementById('audio-btn');").unwrap();
    writeln!(js, "  if (!audioStarted) {{").unwrap();
    writeln!(
        js,
        "    audioCtx = new (window.AudioContext || window.webkitAudioContext)();"
    )
    .unwrap();
    writeln!(js, "    startAudio(audioCtx);").unwrap();
    writeln!(js, "    audioStarted = true;").unwrap();
    writeln!(js, "    btn.textContent = 'Sound Off';").unwrap();
    writeln!(js, "  }} else {{").unwrap();
    writeln!(
        js,
        "    audioCtx.close(); audioCtx = null; audioStarted = false;"
    )
    .unwrap();
    writeln!(js, "    btn.textContent = 'Sound On';").unwrap();
    writeln!(js, "  }}").unwrap();
    writeln!(js, "}}").unwrap();

    writeln!(js, "function startAudio(ctx) {{").unwrap();
    writeln!(js, "  const master = ctx.createGain();").unwrap();
    writeln!(js, "  master.gain.value = 0.5;").unwrap();
    writeln!(js, "  master.connect(ctx.destination);").unwrap();

    for (entity_idx, audio) in &audio_entities {
        emit_audio_source(js, audio, *entity_idx);
    }

    writeln!(js, "}}").unwrap();
}

/// Emit Web Audio nodes for a single audio source.
fn emit_audio_source(js: &mut String, audio: &wt::AudioDef, entity_idx: usize) {
    let vol = audio.volume;
    let var = format!("a{}", entity_idx);

    writeln!(
        js,
        "  {{ const g = ctx.createGain(); g.gain.value = {:.2};",
        vol
    )
    .unwrap();

    match &audio.source {
        wt::AudioSource::Wind { speed, gustiness } => {
            // White noise → lowpass filter with LFO modulating cutoff
            writeln!(
                js,
                "    const buf = ctx.createBuffer(1, ctx.sampleRate*2, ctx.sampleRate);"
            )
            .unwrap();
            writeln!(js, "    const d = buf.getChannelData(0); for(let i=0;i<d.length;i++) d[i]=(Math.random()*2-1);").unwrap();
            writeln!(
                js,
                "    const src = ctx.createBufferSource(); src.buffer = buf; src.loop = true;"
            )
            .unwrap();
            writeln!(
                js,
                "    const filt = ctx.createBiquadFilter(); filt.type = 'lowpass'; filt.frequency.value = {:.0};",
                200.0 + speed * 600.0
            ).unwrap();
            writeln!(
                js,
                "    const lfo = ctx.createOscillator(); lfo.frequency.value = {:.2}; lfo.start();",
                gustiness * 2.0
            )
            .unwrap();
            writeln!(
                js,
                "    const lfoG = ctx.createGain(); lfoG.gain.value = {:.0};",
                gustiness * 200.0
            )
            .unwrap();
            writeln!(js, "    lfo.connect(lfoG); lfoG.connect(filt.frequency);").unwrap();
            writeln!(
                js,
                "    src.connect(filt); filt.connect(g); g.connect(master); src.start();"
            )
            .unwrap();
        }
        wt::AudioSource::Rain { intensity } => {
            writeln!(
                js,
                "    const buf = ctx.createBuffer(1, ctx.sampleRate*2, ctx.sampleRate);"
            )
            .unwrap();
            writeln!(js, "    const d = buf.getChannelData(0); for(let i=0;i<d.length;i++) d[i]=(Math.random()*2-1)*0.5;").unwrap();
            writeln!(
                js,
                "    const src = ctx.createBufferSource(); src.buffer = buf; src.loop = true;"
            )
            .unwrap();
            writeln!(
                js,
                "    const filt = ctx.createBiquadFilter(); filt.type = 'bandpass'; filt.frequency.value = {:.0}; filt.Q.value = 0.5;",
                2000.0 + intensity * 3000.0
            ).unwrap();
            writeln!(
                js,
                "    src.connect(filt); filt.connect(g); g.connect(master); src.start();"
            )
            .unwrap();
        }
        wt::AudioSource::Ocean { wave_size } => {
            writeln!(
                js,
                "    const buf = ctx.createBuffer(1, ctx.sampleRate*4, ctx.sampleRate);"
            )
            .unwrap();
            writeln!(js, "    const d = buf.getChannelData(0); for(let i=0;i<d.length;i++) {{ const t=i/ctx.sampleRate; d[i]=(Math.random()*2-1)*Math.sin(t*0.3*Math.PI)*{:.2}; }}", wave_size).unwrap();
            writeln!(
                js,
                "    const src = ctx.createBufferSource(); src.buffer = buf; src.loop = true;"
            )
            .unwrap();
            writeln!(js, "    const filt = ctx.createBiquadFilter(); filt.type = 'lowpass'; filt.frequency.value = 400;").unwrap();
            writeln!(
                js,
                "    src.connect(filt); filt.connect(g); g.connect(master); src.start();"
            )
            .unwrap();
        }
        wt::AudioSource::Fire { intensity, crackle } => {
            // Brown noise with crackle pops
            writeln!(
                js,
                "    const buf = ctx.createBuffer(1, ctx.sampleRate*2, ctx.sampleRate);"
            )
            .unwrap();
            writeln!(js, "    const d = buf.getChannelData(0); let last=0; for(let i=0;i<d.length;i++) {{ last=(last+Math.random()*2-1)*0.5; d[i]=last*{:.2}; if(Math.random()<{:.4}) d[i]+=Math.random()*{:.2}; }}", intensity, crackle * 0.001, crackle).unwrap();
            writeln!(
                js,
                "    const src = ctx.createBufferSource(); src.buffer = buf; src.loop = true;"
            )
            .unwrap();
            writeln!(js, "    const filt = ctx.createBiquadFilter(); filt.type = 'lowpass'; filt.frequency.value = 800;").unwrap();
            writeln!(
                js,
                "    src.connect(filt); filt.connect(g); g.connect(master); src.start();"
            )
            .unwrap();
        }
        wt::AudioSource::Water { turbulence } => {
            writeln!(
                js,
                "    const buf = ctx.createBuffer(1, ctx.sampleRate*2, ctx.sampleRate);"
            )
            .unwrap();
            writeln!(js, "    const d = buf.getChannelData(0); for(let i=0;i<d.length;i++) d[i]=(Math.random()*2-1);").unwrap();
            writeln!(
                js,
                "    const src = ctx.createBufferSource(); src.buffer = buf; src.loop = true;"
            )
            .unwrap();
            writeln!(
                js,
                "    const filt = ctx.createBiquadFilter(); filt.type = 'bandpass'; filt.frequency.value = {:.0}; filt.Q.value = 1.5;",
                300.0 + turbulence * 500.0
            ).unwrap();
            writeln!(
                js,
                "    src.connect(filt); filt.connect(g); g.connect(master); src.start();"
            )
            .unwrap();
        }
        wt::AudioSource::Hum { frequency, warmth } => {
            writeln!(js, "    const osc = ctx.createOscillator(); osc.type = 'sine'; osc.frequency.value = {:.1};", frequency).unwrap();
            if *warmth > 0.0 {
                writeln!(js, "    const filt = ctx.createBiquadFilter(); filt.type = 'lowpass'; filt.frequency.value = {:.0};", frequency * (1.0 + warmth * 4.0)).unwrap();
                writeln!(
                    js,
                    "    osc.connect(filt); filt.connect(g); g.connect(master); osc.start();"
                )
                .unwrap();
            } else {
                writeln!(js, "    osc.connect(g); g.connect(master); osc.start();").unwrap();
            }
        }
        wt::AudioSource::Stream { flow_rate } => {
            writeln!(
                js,
                "    const buf = ctx.createBuffer(1, ctx.sampleRate*2, ctx.sampleRate);"
            )
            .unwrap();
            writeln!(js, "    const d = buf.getChannelData(0); for(let i=0;i<d.length;i++) d[i]=(Math.random()*2-1);").unwrap();
            writeln!(
                js,
                "    const src = ctx.createBufferSource(); src.buffer = buf; src.loop = true;"
            )
            .unwrap();
            writeln!(
                js,
                "    const filt = ctx.createBiquadFilter(); filt.type = 'bandpass'; filt.frequency.value = {:.0}; filt.Q.value = 2.0;",
                500.0 + flow_rate * 1000.0
            ).unwrap();
            writeln!(
                js,
                "    src.connect(filt); filt.connect(g); g.connect(master); src.start();"
            )
            .unwrap();
        }
        wt::AudioSource::Forest { bird_density, wind } => {
            // Wind base
            writeln!(
                js,
                "    const buf = ctx.createBuffer(1, ctx.sampleRate*2, ctx.sampleRate);"
            )
            .unwrap();
            writeln!(js, "    const d = buf.getChannelData(0); for(let i=0;i<d.length;i++) d[i]=(Math.random()*2-1)*{:.2};", wind).unwrap();
            writeln!(
                js,
                "    const src = ctx.createBufferSource(); src.buffer = buf; src.loop = true;"
            )
            .unwrap();
            writeln!(js, "    const filt = ctx.createBiquadFilter(); filt.type = 'lowpass'; filt.frequency.value = 300;").unwrap();
            writeln!(js, "    src.connect(filt); filt.connect(g);").unwrap();
            // Bird chirps (sine pings at random intervals)
            if *bird_density > 0.0 {
                writeln!(js, "    function chirp() {{ const o = ctx.createOscillator(); o.frequency.value = 2000+Math.random()*3000; const cg = ctx.createGain(); cg.gain.value = {:.2}; o.connect(cg); cg.connect(g); o.start(); cg.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime+0.15); o.stop(ctx.currentTime+0.2); setTimeout(chirp, 500+Math.random()*{}); }}", bird_density * 0.15, (3000.0 / bird_density.max(0.1)) as i32).unwrap();
                writeln!(js, "    setTimeout(chirp, Math.random()*2000);").unwrap();
            }
            writeln!(js, "    g.connect(master); src.start();").unwrap();
        }
        wt::AudioSource::Cave {
            drip_rate,
            resonance,
        } => {
            // Reverberant drips
            writeln!(js, "    function drip() {{ const o = ctx.createOscillator(); o.frequency.value = 800+Math.random()*2000; const cg = ctx.createGain(); cg.gain.value = {:.2}; o.connect(cg); cg.connect(g); o.start(); cg.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime+0.3*{:.2}); o.stop(ctx.currentTime+0.4); setTimeout(drip, {}+Math.random()*{}); }}", resonance * 0.3, resonance, (500.0 / drip_rate.max(0.1)) as i32, (2000.0 / drip_rate.max(0.1)) as i32).unwrap();
            writeln!(js, "    setTimeout(drip, Math.random()*1000);").unwrap();
            writeln!(js, "    g.connect(master);").unwrap();
        }
        wt::AudioSource::WindEmitter { pitch } => {
            writeln!(
                js,
                "    const buf = ctx.createBuffer(1, ctx.sampleRate*2, ctx.sampleRate);"
            )
            .unwrap();
            writeln!(js, "    const d = buf.getChannelData(0); for(let i=0;i<d.length;i++) d[i]=(Math.random()*2-1);").unwrap();
            writeln!(
                js,
                "    const src = ctx.createBufferSource(); src.buffer = buf; src.loop = true;"
            )
            .unwrap();
            writeln!(
                js,
                "    const filt = ctx.createBiquadFilter(); filt.type = 'bandpass'; filt.frequency.value = {:.0}; filt.Q.value = 3.0;",
                pitch * 500.0
            ).unwrap();
            writeln!(
                js,
                "    src.connect(filt); filt.connect(g); g.connect(master); src.start();"
            )
            .unwrap();
        }
        wt::AudioSource::Custom {
            waveform,
            filter_cutoff,
            filter_type,
        } => {
            let wave_type = match waveform {
                wt::WaveformType::Sine => "sine",
                wt::WaveformType::Saw => "sawtooth",
                wt::WaveformType::Square => "square",
                _ => "custom", // noise types
            };
            let is_noise = matches!(
                waveform,
                wt::WaveformType::WhiteNoise
                    | wt::WaveformType::PinkNoise
                    | wt::WaveformType::BrownNoise
            );
            let ftype = match filter_type {
                wt::FilterType::Lowpass => "lowpass",
                wt::FilterType::Highpass => "highpass",
                wt::FilterType::Bandpass => "bandpass",
            };
            if is_noise {
                writeln!(
                    js,
                    "    const buf = ctx.createBuffer(1, ctx.sampleRate*2, ctx.sampleRate);"
                )
                .unwrap();
                writeln!(js, "    const d = buf.getChannelData(0); for(let i=0;i<d.length;i++) d[i]=(Math.random()*2-1);").unwrap();
                writeln!(
                    js,
                    "    const src = ctx.createBufferSource(); src.buffer = buf; src.loop = true;"
                )
                .unwrap();
                writeln!(
                    js,
                    "    const filt = ctx.createBiquadFilter(); filt.type = '{}'; filt.frequency.value = {:.0};",
                    ftype, filter_cutoff
                ).unwrap();
                writeln!(
                    js,
                    "    src.connect(filt); filt.connect(g); g.connect(master); src.start();"
                )
                .unwrap();
            } else {
                writeln!(js, "    const osc = ctx.createOscillator(); osc.type = '{}'; osc.frequency.value = 220;", wave_type).unwrap();
                writeln!(
                    js,
                    "    const filt = ctx.createBiquadFilter(); filt.type = '{}'; filt.frequency.value = {:.0};",
                    ftype, filter_cutoff
                ).unwrap();
                writeln!(
                    js,
                    "    osc.connect(filt); filt.connect(g); g.connect(master); osc.start();"
                )
                .unwrap();
            }
        }
        wt::AudioSource::Silence | wt::AudioSource::Abc { .. } | wt::AudioSource::File { .. } => {
            // Not synthesizable in browser
            writeln!(
                js,
                "    // {} not supported in HTML export",
                match &audio.source {
                    wt::AudioSource::Abc { .. } => "ABC notation",
                    wt::AudioSource::File { .. } => "File audio",
                    _ => "Silence",
                }
            )
            .unwrap();
        }
    }

    let _ = var; // suppress unused warning
    writeln!(js, "  }}").unwrap();
}

/// Emit WASD + Space/Shift keyboard navigation.
fn emit_keyboard_controls(js: &mut String, move_speed: f32) {
    writeln!(
        js,
        r#"// ---- WASD Keyboard Navigation ----
const keysPressed = {{}};
const MOVE_SPEED = {speed:.1};
document.addEventListener('keydown', (e) => {{ keysPressed[e.code] = true; }});
document.addEventListener('keyup', (e) => {{ keysPressed[e.code] = false; }});

function updateMovement(dt) {{
  const dir = new THREE.Vector3();
  camera.getWorldDirection(dir);
  const right = new THREE.Vector3().crossVectors(dir, camera.up).normalize();
  const move = new THREE.Vector3();
  if (keysPressed['KeyW']) move.add(dir);
  if (keysPressed['KeyS']) move.sub(dir);
  if (keysPressed['KeyA']) move.sub(right);
  if (keysPressed['KeyD']) move.add(right);
  if (keysPressed['Space']) move.y += 1;
  if (keysPressed['ShiftLeft'] || keysPressed['ShiftRight']) move.y -= 1;
  if (move.length() > 0) {{
    move.normalize().multiplyScalar(MOVE_SPEED * dt);
    camera.position.add(move);
    controls.target.add(move);
  }}
}}"#,
        speed = move_speed
    )
    .unwrap();
}

/// Emit guided tour system if tours are defined.
fn emit_tours(js: &mut String, manifest: &wt::WorldManifest) {
    if manifest.tours.is_empty() {
        return;
    }

    // Build tour data
    writeln!(js, "// ---- Guided Tours ----").unwrap();
    writeln!(js, "const tours = [];").unwrap();

    for tour in &manifest.tours {
        if tour.waypoints.is_empty() {
            continue;
        }

        let waypoints_json: Vec<String> = tour
            .waypoints
            .iter()
            .map(|wp| {
                format!(
                    "{{pos:[{:.4},{:.4},{:.4}],look:[{:.4},{:.4},{:.4}],pause:{:.1},desc:{}}}",
                    wp.position[0],
                    wp.position[1],
                    wp.position[2],
                    wp.look_at[0],
                    wp.look_at[1],
                    wp.look_at[2],
                    wp.pause_duration,
                    wp.description
                        .as_ref()
                        .map(|d| format!("'{}'", d.replace('\'', "\\'")))
                        .unwrap_or_else(|| "null".into())
                )
            })
            .collect();

        let mode_str = match tour.mode {
            wt::TourMode::Walk => "walk",
            wt::TourMode::Fly => "fly",
            wt::TourMode::Teleport => "teleport",
        };

        writeln!(
            js,
            "tours.push({{ name: '{}', speed: {:.1}, loop: {}, mode: '{}', waypoints: [{}] }});",
            tour.name.replace('\'', "\\'"),
            tour.speed,
            tour.loop_tour,
            mode_str,
            waypoints_json.join(",")
        )
        .unwrap();
    }

    writeln!(
        js,
        r#"let activeTour = null;
let tourWpIdx = 0;
let tourFrac = 0;
let tourPaused = 0;
const tourBtn = document.getElementById('tour-btn');
if (tours.length > 0) tourBtn.style.display = 'block';

function startTour(idx) {{
  activeTour = tours[idx || 0];
  tourWpIdx = 0; tourFrac = 0; tourPaused = 0;
  controls.enabled = false;
  tourBtn.textContent = 'Stop Tour';
  const wp0 = activeTour.waypoints[0];
  if (activeTour.mode === 'teleport') {{
    camera.position.set(wp0.pos[0], wp0.pos[1], wp0.pos[2]);
    camera.lookAt(wp0.look[0], wp0.look[1], wp0.look[2]);
  }}
  const desc = document.getElementById('tour-desc');
  if (wp0.desc) {{ desc.textContent = wp0.desc; desc.style.display = 'block'; }}
}}
function stopTour() {{
  activeTour = null;
  controls.enabled = true;
  tourBtn.textContent = 'Start Tour';
  document.getElementById('tour-desc').style.display = 'none';
}}
function toggleTour() {{ if (activeTour) stopTour(); else startTour(0); }}
if (tourBtn) tourBtn.onclick = toggleTour;

function updateTour(dt) {{
  if (!activeTour) return;
  const wp = activeTour.waypoints;
  if (tourPaused > 0) {{ tourPaused -= dt; return; }}
  const a = wp[tourWpIdx], b = wp[(tourWpIdx + 1) % wp.length];
  if (activeTour.mode === 'teleport') {{
    // Instant jump to next waypoint after pause
    tourWpIdx++;
    if (tourWpIdx >= wp.length - 1) {{
      if (activeTour.loop) tourWpIdx = 0; else {{ stopTour(); return; }}
    }}
    const next = wp[tourWpIdx];
    camera.position.set(next.pos[0], next.pos[1], next.pos[2]);
    camera.lookAt(next.look[0], next.look[1], next.look[2]);
    tourPaused = next.pause;
    tourFrac = 0;
    const desc = document.getElementById('tour-desc');
    if (next.desc) {{ desc.textContent = next.desc; desc.style.display = 'block'; }}
    else desc.style.display = 'none';
    return;
  }}
  const dx = b.pos[0]-a.pos[0], dy = b.pos[1]-a.pos[1], dz = b.pos[2]-a.pos[2];
  const dist = Math.sqrt(dx*dx+dy*dy+dz*dz) || 1;
  tourFrac += (activeTour.speed * dt) / dist;
  if (tourFrac >= 1) {{
    tourWpIdx++;
    if (tourWpIdx >= wp.length - 1) {{
      if (activeTour.loop) tourWpIdx = 0; else {{ stopTour(); return; }}
    }}
    tourFrac = 0;
    tourPaused = wp[tourWpIdx].pause;
    const desc = document.getElementById('tour-desc');
    if (wp[tourWpIdx].desc) {{ desc.textContent = wp[tourWpIdx].desc; desc.style.display = 'block'; }}
    else desc.style.display = 'none';
  }}
  const na = wp[tourWpIdx], nb = wp[(tourWpIdx+1) % wp.length];
  const f = tourFrac;
  camera.position.set(na.pos[0]+(nb.pos[0]-na.pos[0])*f, na.pos[1]+(nb.pos[1]-na.pos[1])*f, na.pos[2]+(nb.pos[2]-na.pos[2])*f);
  const lx = na.look[0]+(nb.look[0]-na.look[0])*f, ly = na.look[1]+(nb.look[1]-na.look[1])*f, lz = na.look[2]+(nb.look[2]-na.look[2])*f;
  camera.lookAt(lx, ly, lz);
  // Walk mode: clamp Y to ground level (approximate ground plane at y=avatar height)
  if (activeTour.mode === 'walk') {{
    const groundY = Math.min(na.pos[1], nb.pos[1]);
    camera.position.y = Math.max(camera.position.y, groundY);
  }}
}}"#
    )
    .unwrap();

    // Auto-start if first tour has autostart flag
    if manifest.tours[0].autostart {
        writeln!(js, "startTour(0);").unwrap();
    }
}

/// Emit postMessage API for parent-frame communication when embedded via iframe.
fn emit_embed_api(js: &mut String) {
    writeln!(
        js,
        r#"// ---- Embed API (postMessage) ----
// Allows parent frames to control the scene via postMessage.
// Messages: {{ action: "startTour", index: 0 }}, {{ action: "stopTour" }},
//           {{ action: "toggleAudio" }}, {{ action: "setCameraPosition", position: [x,y,z] }},
//           {{ action: "setCameraTarget", target: [x,y,z] }}, {{ action: "getSceneInfo" }}
window.addEventListener('message', (event) => {{
  const msg = event.data;
  if (!msg || typeof msg !== 'object' || !msg.action) return;
  switch (msg.action) {{
    case 'startTour':
      if (typeof startTour === 'function') startTour(msg.index || 0);
      break;
    case 'stopTour':
      if (typeof stopTour === 'function') stopTour();
      break;
    case 'toggleAudio':
      if (typeof toggleAudio === 'function') toggleAudio();
      break;
    case 'setCameraPosition':
      if (Array.isArray(msg.position) && msg.position.length === 3) {{
        camera.position.set(msg.position[0], msg.position[1], msg.position[2]);
      }}
      break;
    case 'setCameraTarget':
      if (Array.isArray(msg.target) && msg.target.length === 3) {{
        controls.target.set(msg.target[0], msg.target[1], msg.target[2]);
        controls.update();
      }}
      break;
    case 'getSceneInfo':
      const info = {{
        type: 'sceneInfo',
        entityCount: Object.keys(entityMap).length,
        cameraPosition: [camera.position.x, camera.position.y, camera.position.z],
        cameraTarget: [controls.target.x, controls.target.y, controls.target.z],
        tourCount: typeof tours !== 'undefined' ? tours.length : 0,
        audioEnabled: typeof audioStarted !== 'undefined' ? audioStarted : false,
      }};
      event.source.postMessage(info, event.origin !== 'null' ? event.origin : '*');
      break;
  }}
}});
// Notify parent we're ready
if (window.parent !== window) {{
  window.parent.postMessage({{ type: 'localgpt-scene-ready' }}, '*');
}}"#
    )
    .unwrap();
}
