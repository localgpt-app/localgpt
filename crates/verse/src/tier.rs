//! On-demand model tiers.
//!
//! The `ml` and `llm` features decide whether a tier's **code** exists — you
//! cannot link `ort` without `ml`, and that stays a compile-time fact. This type
//! decides whether an **instance** exists, at runtime.
//!
//! That split matters here because of how the tiers are actually used: CLAP,
//! demucs, and the recipe LLM only run on a track that has no sidecar yet. Once
//! a library has been analyzed they are never consulted again — so eagerly
//! loading them when the worker starts pays hundreds of MB (and, for the GGUF
//! recipe model, several GB) for capability the steady state never uses.
//! [`Tier`] defers the load to the first track that needs it, which in that
//! steady state means never.
//!
//! A tier that fails to load stays [`Unavailable`](Tier::Unavailable) rather
//! than retrying: `try_load` does real file and runtime work, and a missing
//! model file does not appear part-way through a run. Restart to pick one up.
//!
//! # Status
//!
//! Every caller is behind `ml` or `llm`, so the default build constructs no
//! tiers and warns that the type is unused. The module stays unconditionally
//! compiled anyway: it is generic over the model, so its tests — the ones that
//! prove a needed model loads once, an unneeded one never loads, and a missing
//! one is not retried — run in the default build with a dummy type, with no ML
//! dependency and no model download.

#![allow(dead_code)]

use std::fmt;

/// A model loaded on first use and droppable to reclaim its memory.
pub enum Tier<T> {
    /// Never attempted. The next [`get_or_load`](Tier::get_or_load) tries.
    Cold,
    /// Loaded and resident.
    Loaded(T),
    /// Loading was attempted and the model was absent. Sticky for this worker.
    Unavailable,
}

// Not `#[derive(Default)]`: deriving on a generic enum bounds every type
// parameter, giving `impl<T: Default> Default for Tier<T>`. The models a tier
// holds have no `Default` — that is the point, they are loaded — so the manual
// impl is what makes `Tier::<ClapModel>::default()` exist at all.
#[allow(clippy::derivable_impls)]
impl<T> Default for Tier<T> {
    fn default() -> Self {
        Self::Cold
    }
}

impl<T> Tier<T> {
    /// The resident model, loading it if this is the first call.
    ///
    /// Check whether the work is needed *before* calling this — the point of the
    /// type is that a track which already has its sidecar field never triggers a
    /// load.
    pub fn get_or_load(&mut self, load: impl FnOnce() -> Option<T>) -> Option<&mut T> {
        if matches!(self, Self::Cold) {
            *self = match load() {
                Some(model) => Self::Loaded(model),
                None => Self::Unavailable,
            };
        }
        match self {
            Self::Loaded(model) => Some(model),
            _ => None,
        }
    }

    /// Drop the resident model, returning whether anything was released.
    ///
    /// The tier returns to `Cold`, so the next use reloads: this costs latency,
    /// never capability. `Unavailable` is left alone — there is nothing to free
    /// and the model file is still missing.
    ///
    /// Not yet triggered from the app. Choosing when to release wants a policy
    /// that does not thrash (reloading CLAP once per track would be worse than
    /// holding it), and lazy loading already removes the cost from the common
    /// case, so the trigger is deliberately left for the tier-control work.
    pub fn unload(&mut self) -> bool {
        if matches!(self, Self::Loaded(_)) {
            *self = Self::Cold;
            true
        } else {
            false
        }
    }

    /// Whether a model is currently resident.
    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded(_))
    }
}

impl<T> fmt::Debug for Tier<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cold => "cold",
            Self::Loaded(_) => "loaded",
            Self::Unavailable => "unavailable",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_cold_tier_loads_on_first_use() {
        let mut tier = Tier::Cold;
        assert!(!tier.is_loaded());

        assert_eq!(tier.get_or_load(|| Some(7)).copied(), Some(7));
        assert!(tier.is_loaded());
    }

    #[test]
    fn loading_happens_once_across_repeated_use() {
        let loads = Cell::new(0);
        let mut tier = Tier::Cold;

        for _ in 0..5 {
            tier.get_or_load(|| {
                loads.set(loads.get() + 1);
                Some(7)
            });
        }

        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn a_tier_that_is_never_used_is_never_loaded() {
        let loads = Cell::new(0);
        let mut tier: Tier<u32> = Tier::Cold;

        // The caller decides the work is unnecessary, so it never asks.
        let needed = false;
        if needed {
            tier.get_or_load(|| {
                loads.set(loads.get() + 1);
                Some(7)
            });
        }

        assert_eq!(loads.get(), 0);
        assert!(!tier.is_loaded());
    }

    #[test]
    fn a_missing_model_is_not_retried() {
        let attempts = Cell::new(0);
        let mut tier: Tier<u32> = Tier::Cold;

        for _ in 0..5 {
            let got = tier.get_or_load(|| {
                attempts.set(attempts.get() + 1);
                None
            });
            assert!(got.is_none());
        }

        assert_eq!(attempts.get(), 1, "a missing model file was retried");
        assert!(matches!(tier, Tier::Unavailable));
    }

    #[test]
    fn unload_releases_a_loaded_model_and_the_next_use_reloads() {
        let loads = Cell::new(0);
        let mut tier = Tier::Cold;
        // Captures `loads` by shared reference, so the closure is `Copy` and
        // both calls can take it by value.
        let load = || {
            loads.set(loads.get() + 1);
            Some(7)
        };

        tier.get_or_load(load);
        assert!(tier.unload());
        assert!(!tier.is_loaded());

        tier.get_or_load(load);
        assert!(tier.is_loaded());
        assert_eq!(loads.get(), 2);
    }

    #[test]
    fn unload_reports_nothing_released_when_not_loaded() {
        let mut cold: Tier<u32> = Tier::Cold;
        assert!(!cold.unload());

        let mut unavailable: Tier<u32> = Tier::Unavailable;
        assert!(!unavailable.unload());
        // Still unavailable: there is nothing to free and the file is missing.
        assert!(matches!(unavailable, Tier::Unavailable));
    }

    #[test]
    fn the_resident_model_is_mutable_in_place() {
        let mut tier = Tier::Cold;
        *tier.get_or_load(|| Some(1)).unwrap() += 41;
        assert_eq!(tier.get_or_load(|| Some(0)).copied(), Some(42));
    }
}
