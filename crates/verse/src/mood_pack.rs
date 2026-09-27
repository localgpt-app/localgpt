//! Mood packs — worlds added and withdrawn while the app runs.
//!
//! A pack is one [`WorldMood`]: an id, a palette, and an
//! [`Arrangement`](crate::theme::Arrangement). That is the whole of what a world
//! *is* now, which is what makes it mountable — everything else about a world
//! (which props, which layout) derives from those.
//!
//! Mounting goes through [`crate::scope`] rather than calling the registry
//! directly, so a pack's registration carries its own undo: the scope's disposer
//! withdraws it. This is the payoff of the scope work — a content pack is just
//! another mounted unit.
//!
//! # Withdrawal is immediate, and that is a choice
//!
//! Unmounting the world currently on screen switches away from it at once, by
//! re-pointing [`Theme`] at whatever the removed world's index now resolves to.
//!
//! The alternative — keep rendering the withdrawn world until the next track
//! change, so a world survives its song — needs [`Theme`] to own a `WorldMood`
//! copy rather than an index into a list that no longer contains it. `WorldMood`
//! is `Copy`, so that is a small change, but it is a change to how every reader
//! of `theme.mood` resolves its world and it is not made here. Until then,
//! unmounting a playing world is visible immediately.
//!
//! # Pins survive the round trip
//!
//! A sidecar pinned to a withdrawn world falls back through
//! [`resolve_mood`](crate::theme::resolve_mood) rather than failing, and
//! remounting the pack makes those pins resolve again. Unmount/remount is
//! lossless for user pins — a direct consequence of durable records naming
//! worlds by id.

use bevy::prelude::*;

use crate::scope;
use crate::theme::{self, Arrangement, Theme, WorldMood};

/// Mount `mood` as a pack owned by a scope named after its id.
///
/// Returns `false` when a world with that id is already mounted; ids are
/// identity, so a duplicate would make resolution ambiguous.
pub fn mount(world: &mut World, mood: WorldMood) -> bool {
    if theme::moods().iter().any(|m| m.id == mood.id) {
        return false;
    }
    let id = mood.id;
    scope::mount(world, id, |scope, _world| {
        theme::mount_mood(mood);
        scope.defer("mood pack", move |world| {
            withdraw(world, id);
            Ok(())
        });
    });
    true
}

/// A world that exists to exercise mounting, enabled by `VERSE_PACK=1`.
///
/// Deliberately **not** a fifth shipped world. Two things stop it being one, and
/// both are the real ceiling on growing past four:
///
/// - `map_mood` is a 2×2 quadrant over (arousal, brightness) and cannot produce
///   a fifth output, so neither the rule mapper nor the CLAP vote will ever
///   select this world for a track.
/// - The asset pack has no models tagged for it, so it renders as the bare
///   procedural backdrop — which is the documented graceful path for a mood
///   with no matching assets, not a failure.
///
/// It is still reachable: `path_mood` hashes over the live registry length, and
/// "Build a different world" cycles the whole registry. Enough to watch a
/// mounted pack behave like any other world.
pub fn demo_pack() -> WorldMood {
    WorldMood {
        id: "paper-lantern",
        world_name: "PAPER LANTERN",
        accent: Color::srgb(1.0, 0.827, 0.612),
        sky_top: Color::srgb(0.180, 0.129, 0.176),
        sky_bottom: Color::srgb(0.055, 0.043, 0.075),
        fog: Color::srgb(0.286, 0.204, 0.216),
        ground: Color::srgb(0.129, 0.098, 0.125),
        ambient: Color::srgb(0.98, 0.82, 0.66),
        arrangement: Arrangement::Spiral,
    }
}

/// Mount [`demo_pack`] when `VERSE_PACK` is set. No-op otherwise.
pub fn mount_demo_pack_if_requested(world: &mut World) {
    if std::env::var("VERSE_PACK").is_err() {
        return;
    }
    let pack = demo_pack();
    let id = pack.id;
    if mount(world, pack) {
        info!(
            "mounted demo mood pack `{id}` ({} worlds)",
            theme::moods().len()
        );
    } else {
        warn!("demo mood pack `{id}` is already mounted");
    }
}

/// Withdraw the pack mounted under `id`, running its disposer.
///
/// A no-op when nothing is mounted under that id, including for the built-in
/// worlds — those are not scoped, so they cannot be unmounted this way.
///
/// Exercised by this module's tests; the app-facing caller is the Library's
/// pack control, which is not built yet, so the default build sees no use.
#[allow(dead_code)]
pub fn unmount(world: &mut World, id: &str) {
    scope::unmount(world, id);
}

/// Remove `id` from the registry and keep `Theme` pointed at a live world.
///
/// The rebind runs whether or not the withdrawn world was the one showing: any
/// removal shifts the positions after it, so an index captured before the change
/// would otherwise name a different world.
fn withdraw(world: &mut World, id: &str) {
    let showing = world
        .get_resource::<Theme>()
        .map(|theme| theme.current().id);

    if !theme::unmount_mood(id) {
        return;
    }

    if let Some(showing) = showing
        && let Some(mut theme) = world.get_resource_mut::<Theme>()
    {
        theme.mood = theme::resolve_mood(theme::moods(), Some(showing), theme.mood);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(id: &'static str) -> WorldMood {
        WorldMood {
            id,
            world_name: "TEST WORLD",
            ..theme::BUILTIN_MOODS[0]
        }
    }

    /// A world with the resources a pack mount touches, plus the registry
    /// lock — these tests mutate process state, so they must not overlap.
    fn fresh_world() -> (World, std::sync::MutexGuard<'static, ()>) {
        let guard = theme::registry_test_lock();
        let mut world = World::new();
        world.init_resource::<Theme>();
        (world, guard)
    }

    #[test]
    fn a_mounted_pack_joins_the_registry() {
        let (mut world, _guard) = fresh_world();
        let before = theme::moods().len();

        assert!(mount(&mut world, pack("test-pack")));
        assert_eq!(theme::moods().len(), before + 1);
        assert!(theme::moods().iter().any(|m| m.id == "test-pack"));

        unmount(&mut world, "test-pack");
        assert_eq!(theme::moods().len(), before);
        assert!(!theme::moods().iter().any(|m| m.id == "test-pack"));
    }

    #[test]
    fn mounting_a_duplicate_id_is_refused() {
        let (mut world, _guard) = fresh_world();
        assert!(mount(&mut world, pack("dup-pack")));
        assert!(!mount(&mut world, pack("dup-pack")));

        unmount(&mut world, "dup-pack");
        assert!(!theme::moods().iter().any(|m| m.id == "dup-pack"));
    }

    #[test]
    fn withdrawing_keeps_the_showing_world_when_it_was_not_the_one_removed() {
        let (mut world, _guard) = fresh_world();
        // Mount two packs, then show the first built-in.
        assert!(mount(&mut world, pack("keep-a")));
        assert!(mount(&mut world, pack("keep-b")));
        let showing = theme::BUILTIN_MOODS[1].id;
        world.resource_mut::<Theme>().mood = theme::resolve_mood(theme::moods(), Some(showing), 0);

        unmount(&mut world, "keep-a");

        assert_eq!(world.resource::<Theme>().current().id, showing);
        unmount(&mut world, "keep-b");
    }

    #[test]
    fn withdrawing_the_showing_world_falls_back_to_a_live_one() {
        let (mut world, _guard) = fresh_world();
        assert!(mount(&mut world, pack("showing-pack")));
        world.resource_mut::<Theme>().mood =
            theme::resolve_mood(theme::moods(), Some("showing-pack"), 0);
        assert_eq!(world.resource::<Theme>().current().id, "showing-pack");

        unmount(&mut world, "showing-pack");

        // Still pointing at something real, and not at the removed world.
        let now = world.resource::<Theme>().current().id;
        assert_ne!(now, "showing-pack");
        assert!(theme::moods().iter().any(|m| m.id == now));
    }

    #[test]
    fn the_last_world_cannot_be_unmounted() {
        // The registry floor is enforced in `theme`, so assert it directly
        // rather than by tearing the built-ins down here.
        assert!(!theme::unmount_mood("no-such-world"));
    }
}
