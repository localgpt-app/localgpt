//! Spatial interest management (§2 "Spatial Interest Management (AoI)").
//!
//! The flat Phase-1 broadcast (`NetworkTarget::All`) is narrowed per client:
//! the world is partitioned into the shared 64-unit [`ChunkCoord`] grid (the
//! same constant the SpacetimeDB module uses), each client reports where its
//! camera is, and the host only replicates entities whose chunk lies within
//! the client's view radius. Everything outside that radius is represented by
//! a coarse per-chunk [`ChunkSummary`] impostor (HLOD) instead.
//!
//! This module is pure (no Bevy systems) so the policy is unit-testable; the
//! host systems in [`super::host`] apply its decisions through lightyear's
//! per-link visibility API.

use std::collections::{HashMap, HashSet};

use localgpt_world_types::ChunkCoord;
use serde::{Deserialize, Serialize};

/// Default subscription radius in chunks (Chebyshev), i.e. a 5×5 chunk
/// window = 320×320 world units around the client camera.
pub const DEFAULT_VIEW_RADIUS: u8 = 2;

/// Largest radius a client may request (bounds host-side work per client).
pub const MAX_VIEW_RADIUS: u8 = 8;

/// A client's current area of interest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewWindow {
    pub center: ChunkCoord,
    pub radius: u8,
}

impl Default for ViewWindow {
    /// Clients that haven't reported a view yet watch the origin — where the
    /// agent builds unless told otherwise.
    fn default() -> Self {
        Self {
            center: ChunkCoord { x: 0, y: 0 },
            radius: DEFAULT_VIEW_RADIUS,
        }
    }
}

impl ViewWindow {
    /// View window centred on a world-space position, radius clamped to
    /// [`MAX_VIEW_RADIUS`].
    pub fn at(position: [f32; 3], radius: u8) -> Self {
        Self {
            center: ChunkCoord::from_world_pos(position[0], position[2]),
            radius: radius.min(MAX_VIEW_RADIUS),
        }
    }

    /// Whether `chunk` is inside this window (Chebyshev distance — square
    /// windows match square chunks and camera-centred streaming).
    pub fn contains(&self, chunk: ChunkCoord) -> bool {
        let r = self.radius as i32;
        (chunk.x - self.center.x).abs() <= r && (chunk.y - self.center.y).abs() <= r
    }

    /// Every chunk inside the window.
    pub fn chunks(&self) -> HashSet<ChunkCoord> {
        let r = self.radius as i32;
        let mut out = HashSet::with_capacity(((2 * r + 1) * (2 * r + 1)) as usize);
        for dx in -r..=r {
            for dy in -r..=r {
                out.insert(ChunkCoord {
                    x: self.center.x + dx,
                    y: self.center.y + dy,
                });
            }
        }
        out
    }
}

/// Where an entity sits for interest purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relevance {
    /// Relevant everywhere (directional lights, session metadata, chunk
    /// summaries): always replicated.
    Global,
    /// Relevant only to clients whose window covers this chunk.
    Chunk(ChunkCoord),
}

impl Relevance {
    /// Relevance for an entity at a world-space position.
    pub fn at(position: [f32; 3]) -> Self {
        Self::Chunk(ChunkCoord::from_world_pos(position[0], position[2]))
    }

    pub fn visible_in(&self, view: &ViewWindow) -> bool {
        match self {
            Self::Global => true,
            Self::Chunk(chunk) => view.contains(*chunk),
        }
    }
}

/// Per-link cache of what each client currently sees, so the host only
/// issues visibility changes on transitions.
///
/// Replication starts visible by default, so an entity missing from a link's
/// cache is treated as visible.
#[derive(Debug)]
pub struct VisibilityCache<L: Copy + Eq + std::hash::Hash, E: Copy + Eq + std::hash::Hash> {
    visible: HashMap<L, HashMap<E, bool>>,
}

/// A visibility transition to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityChange<L, E> {
    Gain { link: L, entity: E },
    Lose { link: L, entity: E },
}

impl<L: Copy + Eq + std::hash::Hash, E: Copy + Eq + std::hash::Hash> Default
    for VisibilityCache<L, E>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<L: Copy + Eq + std::hash::Hash, E: Copy + Eq + std::hash::Hash> VisibilityCache<L, E> {
    pub fn new() -> Self {
        Self {
            visible: HashMap::new(),
        }
    }

    /// Record the desired visibility of `entity` for `link`, returning a
    /// change only when it differs from the cached state.
    pub fn update(&mut self, link: L, entity: E, desired: bool) -> Option<VisibilityChange<L, E>> {
        let per_link = self.visible.entry(link).or_default();
        let current = per_link.get(&entity).copied().unwrap_or(true);
        per_link.insert(entity, desired);
        match (current, desired) {
            (true, false) => Some(VisibilityChange::Lose { link, entity }),
            (false, true) => Some(VisibilityChange::Gain { link, entity }),
            _ => None,
        }
    }

    /// Drop state for links and entities that no longer exist.
    pub fn retain(&mut self, links: &HashSet<L>, entities: &HashSet<E>) {
        self.visible.retain(|link, _| links.contains(link));
        for per_link in self.visible.values_mut() {
            per_link.retain(|entity, _| entities.contains(entity));
        }
    }

    /// How many entities a link currently sees out of those tracked for it.
    pub fn visible_count(&self, link: L) -> usize {
        self.visible
            .get(&link)
            .map(|m| m.values().filter(|v| **v).count())
            .unwrap_or(0)
    }
}

/// Coarse summary of one chunk's contents — the HLOD impostor streamed to
/// clients for chunks outside their view window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkSummary {
    pub coord: ChunkCoord,
    /// World-space bounds of everything in the chunk.
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Average base color (linear average of sRGB components — impostors are
    /// read at a distance, precision is irrelevant).
    pub color: [f32; 4],
    /// Number of visual entities folded into this summary.
    pub count: u32,
}

/// Accumulates [`ChunkSummary`]s from individual entity bounds.
#[derive(Debug, Default)]
pub struct ChunkSummaryBuilder {
    chunks: HashMap<ChunkCoord, (ChunkSummary, [f32; 4])>,
}

impl ChunkSummaryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one entity's world-space AABB and color into its chunk (keyed by
    /// the AABB centre).
    pub fn add(&mut self, min: [f32; 3], max: [f32; 3], color: [f32; 4]) {
        let center_x = (min[0] + max[0]) * 0.5;
        let center_z = (min[2] + max[2]) * 0.5;
        let coord = ChunkCoord::from_world_pos(center_x, center_z);
        let (summary, color_sum) = self.chunks.entry(coord).or_insert_with(|| {
            (
                ChunkSummary {
                    coord,
                    min,
                    max,
                    color: [0.0; 4],
                    count: 0,
                },
                [0.0; 4],
            )
        });
        for axis in 0..3 {
            summary.min[axis] = summary.min[axis].min(min[axis]);
            summary.max[axis] = summary.max[axis].max(max[axis]);
        }
        for c in 0..4 {
            color_sum[c] += color[c];
        }
        summary.count += 1;
    }

    /// Finish, averaging colors. Output order is unspecified.
    pub fn build(self) -> HashMap<ChunkCoord, ChunkSummary> {
        self.chunks
            .into_iter()
            .map(|(coord, (mut summary, color_sum))| {
                let n = summary.count.max(1) as f32;
                summary.color = [
                    color_sum[0] / n,
                    color_sum[1] / n,
                    color_sum[2] / n,
                    // Impostors are always opaque-ish so they read as mass.
                    (color_sum[3] / n).max(0.6),
                ];
                (coord, summary)
            })
            .collect()
    }
}

/// Whether two summaries differ enough to be worth re-replicating.
pub fn summaries_differ(a: &ChunkSummary, b: &ChunkSummary) -> bool {
    const EPS: f32 = 0.05;
    let ne3 = |x: [f32; 3], y: [f32; 3]| (0..3).any(|i| (x[i] - y[i]).abs() > EPS);
    let ne4 = |x: [f32; 4], y: [f32; 4]| (0..4).any(|i| (x[i] - y[i]).abs() > EPS);
    a.count != b.count || ne3(a.min, b.min) || ne3(a.max, b.max) || ne4(a.color, b.color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_window_contains_chebyshev() {
        let view = ViewWindow::at([10.0, 0.0, 10.0], 1);
        assert_eq!(view.center, ChunkCoord { x: 0, y: 0 });
        assert!(view.contains(ChunkCoord { x: 1, y: 1 }));
        assert!(view.contains(ChunkCoord { x: -1, y: 0 }));
        assert!(!view.contains(ChunkCoord { x: 2, y: 0 }));
        assert_eq!(view.chunks().len(), 9);
    }

    #[test]
    fn view_radius_is_clamped() {
        let view = ViewWindow::at([0.0; 3], 200);
        assert_eq!(view.radius, MAX_VIEW_RADIUS);
    }

    #[test]
    fn relevance_global_always_visible() {
        let far = ViewWindow::at([10_000.0, 0.0, 10_000.0], 0);
        assert!(Relevance::Global.visible_in(&far));
        assert!(!Relevance::at([0.0; 3]).visible_in(&far));
        assert!(Relevance::at([10_010.0, 5.0, 10_010.0]).visible_in(&far));
    }

    #[test]
    fn cache_emits_only_transitions() {
        let mut cache: VisibilityCache<u32, u32> = VisibilityCache::new();
        // Default is visible → desired visible is a no-op.
        assert_eq!(cache.update(1, 10, true), None);
        assert_eq!(
            cache.update(1, 10, false),
            Some(VisibilityChange::Lose {
                link: 1,
                entity: 10
            })
        );
        assert_eq!(cache.update(1, 10, false), None);
        assert_eq!(
            cache.update(1, 10, true),
            Some(VisibilityChange::Gain {
                link: 1,
                entity: 10
            })
        );
        // Unknown entity starting hidden emits a Lose immediately.
        assert_eq!(
            cache.update(2, 10, false),
            Some(VisibilityChange::Lose {
                link: 2,
                entity: 10
            })
        );
        assert_eq!(cache.visible_count(1), 1);
        assert_eq!(cache.visible_count(2), 0);
    }

    #[test]
    fn cache_retain_prunes() {
        let mut cache: VisibilityCache<u32, u32> = VisibilityCache::new();
        cache.update(1, 10, false);
        cache.update(1, 11, false);
        cache.update(2, 10, false);
        cache.retain(&HashSet::from([1]), &HashSet::from([11]));
        // Pruned entity 10 is "new" again → default visible → Lose re-emitted.
        assert!(cache.update(1, 10, false).is_some());
        assert!(cache.update(2, 10, false).is_some());
        assert_eq!(cache.update(1, 11, false), None);
    }

    #[test]
    fn summary_builder_merges_bounds_and_colors() {
        let mut b = ChunkSummaryBuilder::new();
        b.add([0.0, 0.0, 0.0], [2.0, 2.0, 2.0], [1.0, 0.0, 0.0, 1.0]);
        b.add([10.0, 0.0, 10.0], [12.0, 6.0, 12.0], [0.0, 0.0, 1.0, 1.0]);
        b.add([100.0, 0.0, 0.0], [101.0, 1.0, 1.0], [0.0, 1.0, 0.0, 1.0]);
        let out = b.build();
        assert_eq!(out.len(), 2);
        let origin = &out[&ChunkCoord { x: 0, y: 0 }];
        assert_eq!(origin.count, 2);
        assert_eq!(origin.min, [0.0, 0.0, 0.0]);
        assert_eq!(origin.max, [12.0, 6.0, 12.0]);
        assert_eq!(origin.color, [0.5, 0.0, 0.5, 1.0]);
        assert_eq!(out[&ChunkCoord { x: 1, y: 0 }].count, 1);
    }

    #[test]
    fn summaries_differ_threshold() {
        let a = ChunkSummary {
            coord: ChunkCoord { x: 0, y: 0 },
            min: [0.0; 3],
            max: [1.0; 3],
            color: [0.5; 4],
            count: 3,
        };
        let mut b = a.clone();
        b.max[1] += 0.01;
        assert!(!summaries_differ(&a, &b));
        b.count = 4;
        assert!(summaries_differ(&a, &b));
    }
}
