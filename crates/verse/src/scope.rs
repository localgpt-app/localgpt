//! Scoped teardown — every registration carries its own undo.
//!
//! Ported from the effect/fiber discipline in `deepseek-harness` (Cordis); see
//! LocalGPT's `docs/architecture/plugin/` for the review this came from.
//! The rule is the whole idea:
//!
//! > A registration that cannot be undone is a registration that requires a
//! > process restart.
//!
//! A [`Scope`] accumulates disposers as a unit is mounted, and unwinds them in
//! **reverse registration order** when it is dropped from the app. Register the
//! connection before the things that use it and the teardown order is correct
//! for free.
//!
//! # What "unapply" means under Bevy
//!
//! Bevy has no plugin or system removal — `App::add_plugins` is add-only and
//! systems cannot be pulled out of a schedule. So a scope covers the state that
//! *can* be released:
//!
//! | Registration | Undo |
//! |---|---|
//! | Systems | gate with a run condition — never removed |
//! | Resources | [`Scope::insert_resource`] |
//! | Entities | [`Scope::own_entities`] (despawn by marker) |
//! | Threads, models, channels | [`Scope::defer`] |
//!
//! # Divergence from Cordis: panics are not contained
//!
//! Cordis contains exceptions per-disposer because JavaScript does not
//! distinguish "this failed" from "this code is wrong". Rust does. A disposer
//! returns `Result` and a returned `Err` is logged without starving the
//! disposers after it; a **panic** propagates, because continuing to mutate a
//! `World` that a disposer panicked halfway through is worse than failing loudly.
//!
//! # Divergence from the future LocalGPT `crates/core` port: disposers are sync
//!
//! Bevy's schedule has no async, so disposal is `FnOnce(&mut World)` run from an
//! exclusive system. LocalGPT's `crates/core` version will need async disposers
//! awaited to quiescence. The discipline is identical; the signature is
//! host-specific.
//!
//! # Status
//!
//! [`mount_analysis`](crate::analysis::mount_analysis) is the only unit mounted
//! so far. The resource and entity helpers exist for the plugin split and the
//! mood packs (spec stages 0 and 5) and are covered by this module's tests, so
//! dead-code warnings on the not-yet-called API are expected and allowed — the
//! same stance `crate::agent` takes on its scaffolded surface.

#![allow(dead_code)]

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// Teardown for one registration, run against the world at unmount.
type Disposer = Box<dyn FnOnce(&mut World) -> Result<(), String> + Send + Sync>;

struct Entry {
    label: &'static str,
    disposer: Disposer,
}

/// Accumulates teardown for one mounted unit.
///
/// Disposers run in reverse registration order. Disposal is single-shot: a
/// second [`dispose`](Scope::dispose) is a no-op.
pub struct Scope {
    label: &'static str,
    entries: Vec<Entry>,
    disposed: bool,
}

impl Scope {
    /// Create an empty scope. `label` names the mounted unit in diagnostics.
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            entries: Vec::new(),
            disposed: false,
        }
    }

    /// The unit this scope tears down.
    pub fn label(&self) -> &'static str {
        self.label
    }

    /// Register fallible teardown work, labeled for diagnostics.
    ///
    /// An `Err` at disposal is logged and the remaining disposers still run.
    pub fn defer(
        &mut self,
        label: &'static str,
        undo: impl FnOnce(&mut World) -> Result<(), String> + Send + Sync + 'static,
    ) {
        debug_assert!(
            !self.disposed,
            "registered `{label}` on the already-disposed scope `{}`",
            self.label
        );
        self.entries.push(Entry {
            label,
            disposer: Box::new(undo),
        });
    }

    /// Register infallible teardown work.
    pub fn defer_ok(
        &mut self,
        label: &'static str,
        undo: impl FnOnce(&mut World) + Send + Sync + 'static,
    ) {
        self.defer(label, move |world| {
            undo(world);
            Ok(())
        });
    }

    /// Insert a resource now and remove it when this scope disposes.
    pub fn insert_resource<R: Resource>(&mut self, world: &mut World, value: R) {
        world.insert_resource(value);
        self.defer_ok(std::any::type_name::<R>(), |world| {
            world.remove_resource::<R>();
        });
    }

    /// Despawn every entity carrying marker `M` when this scope disposes.
    ///
    /// Entities are found at disposal time, so anything spawned with the marker
    /// after mounting is still cleaned up.
    pub fn own_entities<M: Component>(&mut self) {
        self.defer_ok(std::any::type_name::<M>(), |world| {
            let mut query = world.query_filtered::<Entity, With<M>>();
            let owned: Vec<Entity> = query.iter(world).collect();
            for entity in owned {
                if let Ok(entity_mut) = world.get_entity_mut(entity) {
                    entity_mut.despawn();
                }
            }
        });
    }

    /// Live registration labels, in registration order (disposal is the reverse).
    pub fn effects(&self) -> Vec<&'static str> {
        self.entries.iter().map(|entry| entry.label).collect()
    }

    /// Number of live registrations.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether this scope holds no registrations.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether [`dispose`](Scope::dispose) has already run.
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    /// Run every disposer in reverse registration order.
    ///
    /// Idempotent. A disposer returning `Err` is logged; the rest still run.
    pub fn dispose(&mut self, world: &mut World) {
        if self.disposed {
            return;
        }
        self.disposed = true;
        for entry in std::mem::take(&mut self.entries).into_iter().rev() {
            if let Err(err) = (entry.disposer)(world) {
                warn!(
                    "scope `{}`: disposing `{}` failed: {err}",
                    self.label, entry.label
                );
            }
        }
    }
}

/// The app's mounted units, keyed by name.
///
/// Held as a resource so a unit can be unmounted from an exclusive system.
/// Disposal needs `&mut World`, so it goes through [`unmount`], which uses
/// `World::resource_scope` to hold the registry and the world at once.
#[derive(Resource, Default)]
pub struct Scopes {
    mounted: HashMap<&'static str, Scope>,
}

impl Scopes {
    /// Whether `name` is currently mounted.
    pub fn is_mounted(&self, name: &str) -> bool {
        self.mounted.contains_key(name)
    }

    /// The names currently mounted, for diagnostics. Order is unspecified.
    pub fn mounted(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.mounted.keys().copied()
    }

    /// Registrations live under `name`, or `None` if it is not mounted.
    pub fn effects(&self, name: &str) -> Option<Vec<&'static str>> {
        self.mounted.get(name).map(Scope::effects)
    }

    /// Insert a built scope. Returns the previous scope under `name`, if any —
    /// the caller must dispose it rather than dropping it, or its registrations
    /// leak.
    #[must_use = "a replaced scope still holds live registrations; dispose it"]
    pub fn insert(&mut self, scope: Scope) -> Option<Scope> {
        self.mounted.insert(scope.label(), scope)
    }

    /// Remove a scope without disposing it. Prefer [`unmount`].
    #[must_use = "a removed scope still holds live registrations; dispose it"]
    pub fn take(&mut self, name: &str) -> Option<Scope> {
        self.mounted.remove(name)
    }
}

/// Mount a unit: build its scope, then register it.
///
/// The builder receives the scope and the world, so registrations can touch the
/// world as they are made. Replacing a mounted name disposes the old scope
/// first, so remounting is safe.
pub fn mount(world: &mut World, name: &'static str, build: impl FnOnce(&mut Scope, &mut World)) {
    unmount(world, name);
    let mut scope = Scope::new(name);
    build(&mut scope, world);
    world.init_resource::<Scopes>();
    world.resource_scope(|_world, mut scopes: Mut<Scopes>| {
        debug_assert!(
            scopes.insert(scope).is_none(),
            "unmount() should have cleared `{name}`"
        );
    });
}

/// Log every mounted unit and its live registrations.
///
/// The reason effects carry labels: without a way to ask a running app what is
/// registered and who owns it, a registration that fails to unwind is invisible
/// until it causes a second bug somewhere else.
pub fn describe(world: &World) {
    let Some(scopes) = world.get_resource::<Scopes>() else {
        info!("scopes: none mounted");
        return;
    };
    let mut names: Vec<&'static str> = scopes.mounted().collect();
    names.sort_unstable();
    info!("scopes: {} mounted", names.len());
    for name in names {
        let effects = scopes.effects(name).unwrap_or_default();
        info!("  {name} ({} effects)", effects.len());
        // Disposal is the reverse of registration, so read bottom-up to see
        // teardown order.
        for effect in effects {
            info!("    {effect}");
        }
    }
}

/// Unmount a unit, running its disposers to completion. No-op if not mounted.
pub fn unmount(world: &mut World, name: &str) {
    if !world.contains_resource::<Scopes>() {
        return;
    }
    world.resource_scope(|world, mut scopes: Mut<Scopes>| {
        if let Some(mut scope) = scopes.take(name) {
            scope.dispose(world);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Resource)]
    struct Marker(u32);

    #[derive(Component)]
    struct Owned;

    #[test]
    fn disposers_run_in_reverse_registration_order() {
        let mut world = World::new();
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut scope = Scope::new("test");

        for n in 0..3 {
            let order = Arc::clone(&order);
            scope.defer_ok("step", move |_| order.lock().unwrap().push(n));
        }
        scope.dispose(&mut world);

        assert_eq!(*order.lock().unwrap(), vec![2, 1, 0]);
    }

    #[test]
    fn dispose_is_single_shot() {
        let mut world = World::new();
        let runs = Arc::new(Mutex::new(0));
        let mut scope = Scope::new("test");

        let counter = Arc::clone(&runs);
        scope.defer_ok("once", move |_| *counter.lock().unwrap() += 1);

        scope.dispose(&mut world);
        scope.dispose(&mut world);

        assert_eq!(*runs.lock().unwrap(), 1);
        assert!(scope.is_disposed());
    }

    #[test]
    fn a_failing_disposer_does_not_starve_the_rest() {
        let mut world = World::new();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let mut scope = Scope::new("test");

        let first = Arc::clone(&ran);
        scope.defer_ok("first", move |_| first.lock().unwrap().push("first"));
        scope.defer("bad", |_| Err("boom".to_string()));
        let last = Arc::clone(&ran);
        scope.defer_ok("last", move |_| last.lock().unwrap().push("last"));

        scope.dispose(&mut world);

        // Reverse order: last, bad (fails), first — and `first` still ran.
        assert_eq!(*ran.lock().unwrap(), vec!["last", "first"]);
    }

    #[test]
    fn insert_resource_is_removed_on_dispose() {
        let mut world = World::new();
        let mut scope = Scope::new("test");

        scope.insert_resource(&mut world, Marker(7));
        assert!(world.contains_resource::<Marker>());

        scope.dispose(&mut world);
        assert!(!world.contains_resource::<Marker>());
    }

    #[test]
    fn owned_entities_are_despawned_on_dispose() {
        let mut world = World::new();
        let mut scope = Scope::new("test");
        scope.own_entities::<Owned>();

        // Spawned AFTER the registration — disposal queries at dispose time.
        world.spawn(Owned);
        world.spawn(Owned);
        let untouched = world.spawn_empty().id();

        scope.dispose(&mut world);

        let mut query = world.query_filtered::<Entity, With<Owned>>();
        assert_eq!(query.iter(&world).count(), 0);
        assert!(world.get_entity(untouched).is_ok());
    }

    fn mount_unit(world: &mut World) {
        mount(world, "unit", |scope, world| {
            scope.insert_resource(world, Marker(1));
            scope.own_entities::<Owned>();
            world.spawn(Owned);
            world.spawn(Owned);
        });
    }

    /// Entities the unit owns. Asserted instead of `World::entities().len()`,
    /// which also counts the entities Bevy allocates for component and resource
    /// type registration — permanent metadata, not world content.
    fn owned_count(world: &mut World) -> usize {
        let mut query = world.query_filtered::<Entity, With<Owned>>();
        query.iter(world).count()
    }

    #[test]
    fn mount_round_trip_leaves_the_world_as_it_was() {
        let mut world = World::new();

        mount_unit(&mut world);
        assert_eq!(owned_count(&mut world), 2);
        assert!(world.contains_resource::<Marker>());
        assert!(world.resource::<Scopes>().is_mounted("unit"));

        unmount(&mut world, "unit");

        assert_eq!(owned_count(&mut world), 0);
        assert!(!world.contains_resource::<Marker>());
        assert!(!world.resource::<Scopes>().is_mounted("unit"));
    }

    #[test]
    fn repeated_mount_unmount_cycles_do_not_accumulate() {
        let mut world = World::new();

        for _ in 0..5 {
            mount_unit(&mut world);
            assert_eq!(owned_count(&mut world), 2, "a cycle left entities behind");
            unmount(&mut world, "unit");
            assert_eq!(owned_count(&mut world), 0);
        }
    }

    #[test]
    fn remounting_disposes_the_previous_scope() {
        let mut world = World::new();

        mount(&mut world, "unit", |scope, world| {
            scope.insert_resource(world, Marker(1));
        });
        mount(&mut world, "unit", |scope, world| {
            scope.insert_resource(world, Marker(2));
        });

        // The second mount's resource is live, not the first's.
        assert_eq!(world.resource::<Marker>().0, 2);

        unmount(&mut world, "unit");
        assert!(!world.contains_resource::<Marker>());
    }

    #[test]
    fn effects_are_labeled_for_diagnostics() {
        let mut scope = Scope::new("test");
        scope.defer_ok("first", |_| {});
        scope.defer_ok("second", |_| {});

        assert_eq!(scope.effects(), vec!["first", "second"]);
        assert_eq!(scope.len(), 2);
    }

    #[test]
    fn unmount_is_a_noop_when_nothing_is_mounted() {
        let mut world = World::new();
        unmount(&mut world, "never-mounted");
        world.init_resource::<Scopes>();
        unmount(&mut world, "never-mounted");
    }
}
