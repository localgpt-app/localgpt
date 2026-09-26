//! Panorama → blockout: read a 360° equirectangular image and plan a layout
//! from what it shows (AI3.4, `gen_panorama_to_world`).
//!
//! The image is split into azimuth sectors. In each column the skyline is
//! found by walking down from the zenith and tracking the sky colour until
//! the pixels stop matching it; the skyline's elevation above the horizon
//! row says how much stands in that direction. Contiguous runs of raised
//! sectors become regions with a hero slot at that bearing, sized from the
//! angle they rise to. The overall skyline picks the terrain type, the
//! ground colour the biome, and the sky brightness the time of day.
//!
//! A single panorama carries no depth, so every region sits on one ring
//! around the viewpoint ([`RING_FRACTION`] of the world half-size) and
//! heights are what the observed angles give at that distance.

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::Serialize;

use super::blockout::*;

/// Azimuth sectors the panorama is split into.
pub const SECTORS: usize = 16;
/// Distance of placed regions from the viewpoint, as a fraction of the
/// world's half-size (the panorama gives bearings and angles, not depth).
pub const RING_FRACTION: f32 = 0.6;
/// A sector whose skyline rises less than this is open horizon.
const RAISED_DEGREES: f32 = 3.0;
/// Colour distance (0..√3 in RGB 0..1) at which a pixel stops being sky.
const SKY_DISTANCE: f32 = 0.16;
/// Consecutive non-sky rows needed to call a skyline (ignores noise).
const SKYLINE_RUN: u32 = 3;
/// Width images are downsampled to before analysis.
const ANALYSIS_WIDTH: u32 = 512;

/// What the analysis read from the panorama.
#[derive(Debug, Clone, Serialize)]
pub struct PanoramaAnalysis {
    pub width: u32,
    pub height: u32,
    /// Mean colour of the top band (sRGB 0..1).
    pub sky_color: [f32; 3],
    /// Mean colour of the bottom band (sRGB 0..1).
    pub ground_color: [f32; 3],
    pub sectors: Vec<Sector>,
}

/// One azimuth slice of the panorama.
#[derive(Debug, Clone, Serialize)]
pub struct Sector {
    /// Bearing of the sector's centre in degrees: 0 is the image centre
    /// (the viewpoint's forward, -Z), positive turns right (+X).
    pub azimuth_deg: f32,
    /// How high the skyline rises above the horizon, in degrees (75th
    /// percentile of the sector's columns).
    pub skyline_deg: f32,
    /// Mean colour between the skyline and the horizon (sRGB 0..1).
    pub color: [f32; 3],
}

/// Decode a panorama from a file path, a `data:` URI or bare base64.
pub fn load_panorama(image: &str) -> Result<image::RgbImage> {
    let path = std::path::Path::new(image);
    let bytes = if path.exists() {
        std::fs::read(path).with_context(|| format!("reading {image}"))?
    } else {
        let data = image
            .strip_prefix("data:")
            .and_then(|rest| rest.split_once(',').map(|(_, b64)| b64))
            .unwrap_or(image);
        match base64::engine::general_purpose::STANDARD.decode(data.trim()) {
            Ok(bytes) => bytes,
            Err(_) => bail!("Panorama image not found and not base64 data: {image}"),
        }
    };
    let decoded = image::load_from_memory(&bytes).context("decoding panorama image")?;
    let (w, h) = (decoded.width(), decoded.height());
    if w < h || h < 16 {
        bail!("Panorama is {w}×{h}; an equirectangular panorama is about twice as wide as tall");
    }
    Ok(decoded.to_rgb8())
}

/// Read sky, ground and the skyline per sector from an equirectangular image.
pub fn analyze(img: &image::RgbImage) -> PanoramaAnalysis {
    let img = if img.width() > ANALYSIS_WIDTH {
        let h = (img.height() as u64 * ANALYSIS_WIDTH as u64 / img.width() as u64).max(16) as u32;
        image::imageops::resize(
            img,
            ANALYSIS_WIDTH,
            h,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img.clone()
    };
    let (w, h) = img.dimensions();
    let horizon = h / 2;
    let px = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        [
            p[0] as f32 / 255.0,
            p[1] as f32 / 255.0,
            p[2] as f32 / 255.0,
        ]
    };
    let band_mean = |y0: u32, y1: u32| {
        let mut sum = [0.0f32; 3];
        let mut n = 0.0;
        for y in y0..y1.max(y0 + 1).min(h) {
            for x in 0..w {
                let c = px(x, y);
                (0..3).for_each(|i| sum[i] += c[i]);
                n += 1.0;
            }
        }
        sum.map(|s| s / n)
    };
    let sky_color = band_mean(0, h / 12);
    let ground_color = band_mean(h - h / 8, h);

    // Skyline per column: follow the sky colour down from the zenith.
    let skyline_row: Vec<u32> = (0..w)
        .map(|x| {
            let mut sky = px(x, 0);
            let mut run = 0;
            for y in 0..horizon {
                let c = px(x, y);
                if distance(c, sky) > SKY_DISTANCE {
                    run += 1;
                    if run >= SKYLINE_RUN {
                        return y + 1 - SKYLINE_RUN;
                    }
                } else {
                    run = 0;
                    // Track the gradient towards the horizon.
                    (0..3).for_each(|i| sky[i] = sky[i] * 0.8 + c[i] * 0.2);
                }
            }
            horizon
        })
        .collect();

    let sectors = (0..SECTORS)
        .map(|s| {
            let x0 = s as u32 * w / SECTORS as u32;
            let x1 = ((s as u32 + 1) * w / SECTORS as u32).max(x0 + 1);
            let mut elevations: Vec<f32> = (x0..x1)
                .map(|x| (horizon - skyline_row[x as usize]) as f32 / horizon as f32 * 90.0)
                .collect();
            elevations.sort_by(f32::total_cmp);
            let skyline_deg = elevations[(elevations.len() * 3 / 4).min(elevations.len() - 1)];

            let mut sum = [0.0f32; 3];
            let mut n = 0.0;
            for x in x0..x1 {
                for y in skyline_row[x as usize]..horizon {
                    let c = px(x, y);
                    (0..3).for_each(|i| sum[i] += c[i]);
                    n += 1.0;
                }
            }
            let color = if n > 0.0 {
                sum.map(|v| v / n)
            } else {
                sky_color
            };
            let u = (s as f32 + 0.5) / SECTORS as f32;
            Sector {
                azimuth_deg: (u - 0.5) * 360.0,
                skyline_deg,
                color,
            }
        })
        .collect();

    PanoramaAnalysis {
        width: w,
        height: h,
        sky_color,
        ground_color,
        sectors,
    }
}

impl PanoramaAnalysis {
    /// Plan a blockout from the analysis. `prompt` still chooses the layout
    /// style and density (and the biome, when it names one); terrain,
    /// regions and time of day come from the image. With `generate_beyond`,
    /// open-horizon directions get sparse walkable regions too, so the world
    /// continues where the panorama shows nothing.
    pub fn to_blockout(
        &self,
        prompt: Option<&str>,
        size: [f32; 2],
        generate_beyond: bool,
    ) -> BlockoutSpec {
        let prompt = prompt.unwrap_or("");
        let mut spec = BlockoutSpec::from_prompt(prompt, size, None);
        let ring = RING_FRACTION * size[0].min(size[1]) / 2.0;
        let sector_arc = std::f32::consts::TAU / SECTORS as f32;

        // Terrain from the skyline as a whole.
        let raised: Vec<&Sector> = self
            .sectors
            .iter()
            .filter(|s| s.skyline_deg >= RAISED_DEGREES)
            .collect();
        let mean_skyline =
            self.sectors.iter().map(|s| s.skyline_deg).sum::<f32>() / self.sectors.len() as f32;
        let coverage = raised.len() as f32 / self.sectors.len() as f32;
        let (terrain_type, verticality) = if raised.is_empty() {
            (TerrainType::Flat, 0.05)
        } else if coverage >= 0.5 && mean_skyline >= 8.0 {
            (
                TerrainType::Mountains,
                (mean_skyline / 20.0).clamp(0.5, 1.0),
            )
        } else {
            (TerrainType::Hills, (mean_skyline / 20.0).clamp(0.1, 0.6))
        };
        spec.terrain.terrain_type = terrain_type;
        spec.terrain.verticality = verticality;

        if !mentions_biome(prompt) {
            spec.palette.primary_biome = biome_from_ground(self.ground_color);
        }
        spec.palette.time_of_day = time_of_day_from_sky(self.sky_color);

        // Regions: the viewpoint, then one per run of raised sectors.
        let mut regions = vec![RegionDef {
            id: "viewpoint".into(),
            bounds: RegionBounds {
                center: [0.0, 0.0],
                size: [ring * 0.6, ring * 0.6],
            },
            region_type: spec.layout.style,
            density: 0.2,
            walkable: true,
            hero_slots: vec![],
            medium_density: 0.2,
            decorative_density: 0.3,
        }];
        for run in sector_runs(&self.sectors, |s| s.skyline_deg >= RAISED_DEGREES) {
            regions.push(self.raised_region(&run, ring, sector_arc, spec.layout.style));
        }
        if generate_beyond {
            for run in sector_runs(&self.sectors, |s| s.skyline_deg < RAISED_DEGREES) {
                regions.push(self.open_region(&run, ring, sector_arc, spec.layout.style));
            }
        }
        spec.paths = regions
            .iter()
            .skip(1)
            .filter(|r| r.walkable)
            .map(|r| PathConnection {
                from: "viewpoint".into(),
                to: r.id.clone(),
                width: 3.0,
                style: PathStyle::Dirt,
            })
            .collect();
        spec.regions = regions;
        spec
    }

    fn raised_region(
        &self,
        run: &[usize],
        ring: f32,
        sector_arc: f32,
        style: LayoutStyle,
    ) -> RegionDef {
        let (azimuth, arc) = run_bearing(run, sector_arc);
        let peak = run
            .iter()
            .map(|&i| &self.sectors[i])
            .max_by(|a, b| a.skyline_deg.total_cmp(&b.skyline_deg))
            .expect("runs are non-empty");
        let color = mean_color(run.iter().map(|&i| self.sectors[i].color));
        let width = (2.0 * ring * (arc / 2.0).min(1.2).sin()).max(6.0);
        let height = (ring * peak.skyline_deg.to_radians().tan()).clamp(2.0, 60.0);
        let depth = (width * 0.5).clamp(6.0, ring * 0.8);
        let [x, z] = bearing_point(azimuth, ring);
        // Low and broad reads as distant land; tall or narrow as a landmark.
        let landform = peak.skyline_deg < 10.0 && run.len() >= 3;
        let (role, what) = if landform {
            ("backdrop", "landform (hills, ridge or treeline)")
        } else {
            ("landmark", "structure or tall landmark")
        };
        RegionDef {
            id: format!("bearing_{:+04.0}", azimuth.to_degrees()),
            bounds: RegionBounds {
                center: [x, z],
                size: [width, depth],
            },
            region_type: style,
            density: 0.5,
            walkable: !landform,
            hero_slots: vec![HeroSlot {
                position: [x, 0.0, z],
                size: [width.min(30.0), height, depth.min(30.0)],
                role: role.into(),
                hint: format!(
                    "{what} seen at bearing {:.0}°, rising {:.0}° above the horizon, colour {}",
                    azimuth.to_degrees(),
                    peak.skyline_deg,
                    hex(color)
                ),
            }],
            medium_density: 0.5,
            decorative_density: 0.4,
        }
    }

    fn open_region(
        &self,
        run: &[usize],
        ring: f32,
        sector_arc: f32,
        style: LayoutStyle,
    ) -> RegionDef {
        let (azimuth, arc) = run_bearing(run, sector_arc);
        let width = (2.0 * ring * (arc / 2.0).min(1.2).sin()).max(6.0);
        let [x, z] = bearing_point(azimuth, ring);
        RegionDef {
            id: format!("open_{:+04.0}", azimuth.to_degrees()),
            bounds: RegionBounds {
                center: [x, z],
                size: [width, ring * 0.6],
            },
            region_type: style,
            density: 0.15,
            walkable: true,
            hero_slots: vec![],
            medium_density: 0.15,
            decorative_density: 0.3,
        }
    }
}

/// Maximal circular runs of sector indices matching `pred`.
fn sector_runs(sectors: &[Sector], pred: impl Fn(&Sector) -> bool) -> Vec<Vec<usize>> {
    let n = sectors.len();
    let hits: Vec<bool> = sectors.iter().map(&pred).collect();
    if hits.iter().all(|&h| h) {
        return vec![(0..n).collect()];
    }
    // Start just after a miss so no run is split by the wrap-around.
    let start = (0..n).find(|&i| !hits[i]).map(|i| i + 1).unwrap_or(0);
    let mut runs = Vec::new();
    let mut current = Vec::new();
    for k in 0..n {
        let i = (start + k) % n;
        if hits[i] {
            current.push(i);
        } else if !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

/// Centre bearing (radians, wrapped to ±π) and angular width of a run.
fn run_bearing(run: &[usize], sector_arc: f32) -> (f32, f32) {
    let n = std::f32::consts::TAU / sector_arc;
    // Sector i's centre is at (i + 0.5)/n of the image, bearing -π at the
    // left edge. Unwrap along the run so a run crossing the seam averages
    // correctly.
    let first = run[0] as f32;
    let mid = first + (run.len() as f32 - 1.0) / 2.0;
    let bearing = ((mid + 0.5) / n - 0.5) * std::f32::consts::TAU;
    let wrapped =
        (bearing + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    (wrapped, run.len() as f32 * sector_arc)
}

/// XZ point at `distance` along a bearing (0 = -Z, positive towards +X).
fn bearing_point(bearing: f32, distance: f32) -> [f32; 2] {
    [bearing.sin() * distance, -bearing.cos() * distance]
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn mean_color(colors: impl Iterator<Item = [f32; 3]>) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    let mut n = 0.0f32;
    for c in colors {
        (0..3).for_each(|i| sum[i] += c[i]);
        n += 1.0;
    }
    sum.map(|v| v / n.max(1.0))
}

pub fn hex(c: [f32; 3]) -> String {
    let b = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2])
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn biome_from_ground(c: [f32; 3]) -> Biome {
    let [r, g, b] = c;
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
    if luma(c) > 0.75 && saturation < 0.15 {
        Biome::Arctic
    } else if luma(c) < 0.12 {
        Biome::Volcanic
    } else if r > g && g > b && r - b > 0.15 && luma(c) > 0.35 {
        Biome::Desert
    } else if g >= r && g >= b {
        if luma(c) < 0.25 {
            Biome::Swamp
        } else {
            Biome::TemperateForest
        }
    } else {
        Biome::Savanna
    }
}

fn time_of_day_from_sky(c: [f32; 3]) -> f32 {
    let l = luma(c);
    if l < 0.1 {
        0.0
    } else if l < 0.3 && c[0] > c[2] {
        0.75 // warm and dim: sunset
    } else if l < 0.3 {
        0.25
    } else {
        0.5
    }
}

fn mentions_biome(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    [
        "desert",
        "sand",
        "arid",
        "snow",
        "arctic",
        "frozen",
        "tundra",
        "tropical",
        "jungle",
        "swamp",
        "marsh",
        "bog",
        "volcano",
        "lava",
        "savanna",
        "grassland",
        "forest",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKY: [u8; 3] = [120, 170, 230];
    const GROUND: [u8; 3] = [90, 140, 60];
    const TOWER: [u8; 3] = [80, 70, 60];

    /// A 512×256 panorama: blue sky, green ground below the horizon, and a
    /// dark tower rising 45° spanning image columns `x0..x1`.
    fn panorama(tower: Option<(u32, u32)>) -> image::RgbImage {
        image::RgbImage::from_fn(512, 256, |x, y| {
            let in_tower = tower.is_some_and(|(x0, x1)| x >= x0 && x < x1 && y >= 64);
            image::Rgb(if in_tower {
                TOWER
            } else if y >= 128 {
                GROUND
            } else {
                SKY
            })
        })
    }

    #[test]
    fn an_empty_horizon_is_flat_with_only_the_viewpoint() {
        let a = analyze(&panorama(None));
        assert!(a.sectors.iter().all(|s| s.skyline_deg < 1.0));
        let spec = a.to_blockout(None, [80.0, 80.0], false);
        assert_eq!(spec.terrain.terrain_type, TerrainType::Flat);
        assert_eq!(spec.regions.len(), 1);
        assert_eq!(spec.palette.primary_biome, Biome::TemperateForest);
        assert_eq!(spec.palette.time_of_day, 0.5);
    }

    #[test]
    fn a_tower_ahead_becomes_a_landmark_ahead() {
        // Image centre (x = 256) is straight ahead (-Z).
        let a = analyze(&panorama(Some((240, 272))));
        let ahead = &a.sectors[SECTORS / 2 - 1..=SECTORS / 2];
        assert!(
            ahead.iter().any(|s| (s.skyline_deg - 45.0).abs() < 2.0),
            "{ahead:?}"
        );

        let spec = a.to_blockout(None, [80.0, 80.0], false);
        assert_eq!(spec.regions.len(), 2, "{:?}", spec.regions);
        let tower = &spec.regions[1];
        let [x, z] = tower.bounds.center;
        assert!(x.abs() < 3.0 && z < -20.0, "tower at {x}, {z}");
        let slot = &tower.hero_slots[0];
        assert_eq!(slot.role, "landmark");
        assert!(
            slot.size[1] > 15.0,
            "a 45° rise at 24 m is tall: {:?}",
            slot.size
        );
        assert!(slot.hint.contains("#50463c"), "{}", slot.hint);
    }

    #[test]
    fn a_tower_behind_wraps_across_the_seam() {
        // Columns at both edges are directly behind (+Z).
        let mut img = panorama(Some((0, 16)));
        for x in 496..512 {
            for y in 64..256 {
                img.put_pixel(x, y, image::Rgb(TOWER));
            }
        }
        let spec = analyze(&img).to_blockout(None, [80.0, 80.0], false);
        assert_eq!(spec.regions.len(), 2, "one region, not split by the seam");
        let [x, z] = spec.regions[1].bounds.center;
        assert!(x.abs() < 3.0 && z > 20.0, "behind at {x}, {z}");
    }

    #[test]
    fn generate_beyond_fills_open_directions() {
        let a = analyze(&panorama(Some((240, 272))));
        let without = a.to_blockout(None, [80.0, 80.0], false);
        let with = a.to_blockout(None, [80.0, 80.0], true);
        assert!(with.regions.len() > without.regions.len());
        assert!(with.regions.iter().any(|r| r.id.starts_with("open_")));
    }

    #[test]
    fn colours_set_biome_and_time_unless_the_prompt_names_a_biome() {
        let sand = image::RgbImage::from_fn(64, 32, |_, y| {
            image::Rgb(if y >= 16 {
                [210, 180, 120]
            } else {
                [20, 20, 40]
            })
        });
        let a = analyze(&sand);
        let spec = a.to_blockout(None, [80.0, 80.0], false);
        assert_eq!(spec.palette.primary_biome, Biome::Desert);
        assert_eq!(spec.palette.time_of_day, 0.0, "a dark sky is night");
        let spec = a.to_blockout(Some("a snowy valley"), [80.0, 80.0], false);
        assert_eq!(spec.palette.primary_biome, Biome::Arctic);
    }

    #[test]
    fn panoramas_load_from_base64_and_reject_garbage() {
        let mut png = Vec::new();
        panorama(None)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        assert_eq!(load_panorama(&b64).unwrap().dimensions(), (512, 256));
        let uri = format!("data:image/png;base64,{b64}");
        assert_eq!(load_panorama(&uri).unwrap().dimensions(), (512, 256));
        assert!(load_panorama("/no/such/panorama.png").is_err());
    }
}
