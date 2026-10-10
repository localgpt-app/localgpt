//! The shot list: the cameras carrying a `shot`, in order, with the
//! derived lens data (frame, fields of view, camera height, distance to
//! subject) — and the `shotlist.csv` the shot-list tools import.

use serde::Serialize;

use localgpt_world_types as wt;

use crate::cinema::{self, CameraComponent};

/// One row of the shot list: a camera setup with its derived numbers.
#[derive(Debug, Clone, Serialize)]
pub struct ShotRow {
    /// The shot's scene (the `shot.scene` text).
    pub scene: String,
    /// The shot's name — the entity's name ("1A").
    pub shot: String,
    /// The `shot.order` sort key.
    pub order: u32,
    /// The shot size ("WS", "MCU", …).
    pub size: String,
    /// The lens, mm.
    pub focal_length_mm: f64,
    /// The sensor `[w, h]`, mm.
    pub sensor_mm: [f64; 2],
    /// The frame aspect (w/h) after the crop.
    pub frame_aspect: f64,
    /// Derived horizontal field of view, degrees.
    pub hfov_degrees: f64,
    /// Derived vertical field of view, degrees.
    pub vfov_degrees: f64,
    /// The camera's world position.
    pub position: [f64; 3],
    /// The aim point (what the camera looks at).
    pub aim: [f64; 3],
    /// Camera height, m.
    pub height_m: f64,
    /// Distance to subject (camera to aim), m.
    pub distance_m: f64,
    /// Seconds on the clock.
    pub in_s: f64,
    /// Seconds on the clock.
    pub out_s: f64,
    /// Free text.
    pub description: String,
}

/// A manifest's shot list: the cameras with a `shot`, ordered by
/// `shot.order` (ties by entity id — the extension's sort).
pub fn shot_list(manifest: &wt::WorldManifest) -> Vec<ShotRow> {
    let mut rows: Vec<ShotRow> = manifest
        .entities
        .iter()
        .filter_map(|entity| {
            let ext = entity.extra.get(cinema::EXTENSION_NAME)?;
            let shot = ext.get("shot")?;
            if !shot.is_object() {
                return None;
            }
            let component = CameraComponent::of(entity)?;
            let frame = cinema::frame_of(&component);
            let position = entity.transform.position.map(|p| p as f64);
            let aim = component.aim.unwrap_or(position);
            let distance = ((aim[0] - position[0]).powi(2)
                + (aim[1] - position[1]).powi(2)
                + (aim[2] - position[2]).powi(2))
            .sqrt();
            let text = |k: &str| {
                shot.get(k)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let seconds = |k: &str| shot.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
            Some(ShotRow {
                scene: text("scene"),
                shot: entity.name.as_str().to_string(),
                order: shot
                    .get("order")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(u64::MAX) as u32,
                size: text("size"),
                focal_length_mm: component.focal_length_mm,
                sensor_mm: component.sensor_mm,
                frame_aspect: frame.aspect,
                hfov_degrees: frame.hfov_degrees,
                vfov_degrees: frame.vfov_degrees,
                position,
                aim,
                height_m: position[1],
                distance_m: distance,
                in_s: seconds("in_s"),
                out_s: seconds("out_s"),
                description: text("description"),
            })
        })
        .collect();
    rows.sort_by_key(|r| r.order);
    rows
}

/// The lens as shot lists write it: `24`, `35`.
pub fn focal(mm: f64) -> String {
    trim1(mm)
}

/// One decimal place, trailing zeros trimmed (`4.50` → `4.5`).
fn trim1(v: f64) -> String {
    let s = format!("{v:.1}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Two decimals, trailing zeros trimmed (for heights and distances).
fn trim2(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A CSV field: quoted when it carries a comma, quote or newline.
fn field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// The shot list as `shotlist.csv`: the columns shot-list tools import.
pub fn csv(rows: &[ShotRow]) -> String {
    let mut out = String::from(
        "scene,shot,size,focal_length_mm,sensor_mm,aspect,height_m,distance_m,in_s,out_s,description\n",
    );
    for r in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{}\n",
            field(&r.scene),
            field(&r.shot),
            field(&r.size),
            trim1(r.focal_length_mm),
            field(&format!("{:.2}x{:.2}", r.sensor_mm[0], r.sensor_mm[1])),
            trim2(r.frame_aspect),
            trim2(r.height_m),
            trim2(r.distance_m),
            trim1(r.in_s),
            trim1(r.out_s),
            field(&r.description),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fountain, stage};

    fn first_scene() -> wt::WorldManifest {
        let script = fountain::parse(
            "INT. KITCHEN - DAY\n\nMaya cooks. Kai watches.\n\nMAYA\nSit down.\n\nKAI\nI'm fine standing here.\n",
        );
        stage::stage(&stage::scenes(&script)[0])
    }

    #[test]
    fn the_shot_list_is_in_order_with_derived_data() {
        let rows = shot_list(&first_scene());
        let names: Vec<&str> = rows.iter().map(|r| r.shot.as_str()).collect();
        assert_eq!(names, ["1A", "1B", "1C"]);
        let master = &rows[0];
        assert_eq!(master.size, "WS");
        assert_eq!(master.focal_length_mm, 24.0);
        assert!((master.frame_aspect - 2.39).abs() < 1e-9);
        assert!((master.height_m - 1.6).abs() < 1e-4);
        assert!(master.distance_m > 2.0);
        assert_eq!(master.in_s, 0.0);
        assert!(master.out_s >= rows[1].out_s, "the master covers the scene");
        // The singles: 50 mm, at eye height, aimed at a head.
        for single in &rows[1..] {
            assert_eq!(single.focal_length_mm, 50.0);
            assert!((single.height_m - 1.6).abs() < 1e-4);
            assert!((single.aim[1] - 1.6).abs() < 1e-4);
        }
        // The master's vfov matches the normative math: H = 18.66 ·
        // min(1, A/2.39) with A = 24.89/18.66.
        let a = 24.89_f64 / 18.66;
        let h = 18.66 * (a / 2.39).min(1.0);
        let vfov = 2.0 * (h / 48.0).atan() * 180.0 / std::f64::consts::PI;
        assert!((master.vfov_degrees - vfov).abs() < 1e-9);
    }

    #[test]
    fn the_csv_has_the_shot_list_columns() {
        let text = csv(&shot_list(&first_scene()));
        let mut lines = text.lines();
        assert_eq!(
            lines.next(),
            Some("scene,shot,size,focal_length_mm,sensor_mm,aspect,height_m,distance_m,in_s,out_s,description")
        );
        let master = lines.next().unwrap();
        assert!(master.starts_with("1,1A,WS,24,24.89x18.66,2.39,1.6,"), "{master}");
        assert!(master.ends_with(",0,3.4,Master wide — INT. KITCHEN - DAY"), "{master}");
        assert_eq!(lines.count(), 2);
    }

    #[test]
    fn csv_fields_quote_commas() {
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("plain"), "plain");
    }
}
