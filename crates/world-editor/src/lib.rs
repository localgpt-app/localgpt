//! The non-linear editing model for the Open World Format — tips, forks,
//! folds and deltas — with no engine and no I/O in it.
//!
//! A `.world` is an append-only log over a base document. The state at any
//! point is a *fold* of the path from the base to that entry, so history is a
//! tree: fork at any entry, develop independently, scrub as a tree. That is
//! the format's model (`openworldformat::session::fold_path`, the spec's
//! branching-histories RFC); this crate is what an editor needs on top of it.
//!
//! Three things the format does not provide, and this crate does:
//!
//! 1. **A branch index.** [`History`] gives entry identity (the spec's
//!    synthesis rule for logs without ids), parents, children, tips and the
//!    path to any tip, computed once instead of per fold.
//! 2. **A fold that is fast enough to scrub.** [`FoldCache`] keeps snapshots
//!    and folds forward from the nearest cached ancestor, so moving along a
//!    branch costs the few entries between where you were and where you are
//!    going — not the whole path from the base, which is what `fold_path`
//!    does on every call.
//! 3. **A delta.** [`Editor::goto`] answers "what changed between the
//!    document on screen and the document at this tip" as ordered ops, which
//!    is the only thing a renderer needs in order to move.
//!
//! ## Not an authority
//!
//! `world-agent`'s `LiveWorld` owns the package and the commit: ingestion
//! through the Authoring profile, all-or-nothing batches, undo as an appended
//! inverse, `manifest.json` and its guard, the git commit, `verify`. This
//! crate deliberately has none of that — it had two of them briefly, which was
//! one too many — and follows instead: the authority commits, hands the entry
//! to [`Editor::committed`], and the view says what changed.
//!
//! ## The rule this crate exists to enforce
//!
//! **Ops → document → fold → delta → renderer. One direction, always.**
//!
//! The scene is never a co-author. A viewport that drags an object submits an
//! op like any agent would; it does not mutate its own state and let a
//! projection notice later. Gen does the opposite today — it projects its
//! scene into ops a few times a second — and that is survivable for a live
//! room but not for a history you can scrub, because a projection of the old
//! tip races every seek. See `docs/rfcs/authoring/world-editor-stack.md`.
//!
//! ## What is deliberately absent
//!
//! No I/O: an [`Editor`] is opened from a base document and a log that a
//! caller read, and the entries it appends are returned for the caller to
//! persist. No engine: the delta is ops, and what a renderer does with them
//! is its business. No CRDT: forks are explicit and a merge is validated
//! against the document, which is the format's decision, not this crate's.

mod delta;
mod editor;
mod fold;
mod history;

#[cfg(test)]
mod test_support;

pub use delta::{Delta, delta_between};
pub use editor::{Editor, EditorError};
pub use fold::FoldCache;
pub use history::{EntryId, EntryRef, History, HistoryError};
