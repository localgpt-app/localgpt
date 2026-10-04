//! The branch index: identity, parents, children, tips and paths.
//!
//! The format lets a log leave `id` and `parent` off every entry, in which
//! case file order *is* the chain. Folding therefore has to synthesize
//! identity before it can walk anything, and [`openworldformat::session`]
//! does exactly that on every call. An editor asks the same questions
//! continuously — what are the tips, what is this entry's parent, which
//! entries lie between here and there — so this computes it once.
//!
//! The synthesis rule is the format's and is reproduced exactly: an entry
//! without an `id` is `line-<n>` for its index, and an entry without a
//! `parent` builds on the previous entry. Diverging from it would make a tip
//! this crate reports unfoldable by the format, which is the one bug class
//! that would make the whole crate untrustworthy.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use openworldformat::oplog::OpLogEntry;

/// An entry's identity. Opaque: a reader never computes one, and the
/// synthesized form (`line-<n>`) is as valid as a content hash.
pub type EntryId = String;

/// What went wrong reading a log as a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryError {
    /// Two entries claim the same id.
    DuplicateId(EntryId),
    /// An entry names a parent that no earlier entry defines. Forward
    /// references are refused rather than resolved: a log is append-only, so
    /// a parent is always already written.
    UnknownParent { entry: EntryId, parent: EntryId },
    /// A tip, fork point or ref names an entry that is not in this log.
    NoSuchEntry(EntryId),
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateId(id) => write!(f, "duplicate entry id '{id}'"),
            Self::UnknownParent { entry, parent } => write!(
                f,
                "entry '{entry}' names parent '{parent}', which isn't in the log yet"
            ),
            Self::NoSuchEntry(id) => write!(f, "no entry '{id}' in this log"),
        }
    }
}

impl std::error::Error for HistoryError {}

/// One entry with its resolved identity.
#[derive(Debug, Clone, Copy)]
pub struct EntryRef<'a> {
    /// Index in the log's file order.
    pub index: usize,
    /// The entry's id, synthesized when the log omitted it.
    pub id: &'a str,
    /// The entry this one builds on; `None` for the one rooted at the base.
    pub parent: Option<&'a str>,
    /// The entry itself.
    pub entry: &'a OpLogEntry,
}

/// A log read as a tree.
///
/// Holds no document: a `History` is about shape, and [`crate::FoldCache`]
/// is about state. Separating them is what lets the index be rebuilt cheaply
/// and the expensive folds be kept.
#[derive(Debug, Clone, Default)]
pub struct History {
    entries: Vec<OpLogEntry>,
    /// Resolved id per index, in file order.
    ids: Vec<EntryId>,
    /// Resolved parent per index.
    parents: Vec<Option<EntryId>>,
    index_of: HashMap<EntryId, usize>,
    children: BTreeMap<EntryId, Vec<EntryId>>,
}

impl History {
    /// Read a log as a tree, resolving identity by the format's rule.
    pub fn open(entries: Vec<OpLogEntry>) -> Result<Self, HistoryError> {
        let mut ids = Vec::with_capacity(entries.len());
        let mut parents = Vec::with_capacity(entries.len());
        let mut index_of = HashMap::with_capacity(entries.len());
        let mut children: BTreeMap<EntryId, Vec<EntryId>> = BTreeMap::new();
        let mut previous: Option<EntryId> = None;

        for (n, entry) in entries.iter().enumerate() {
            let id = entry.id.clone().unwrap_or_else(|| format!("line-{n}"));
            if index_of.contains_key(&id) {
                return Err(HistoryError::DuplicateId(id));
            }
            let parent = entry.parent.clone().or_else(|| previous.clone());
            if let Some(p) = &parent
                && !index_of.contains_key(p)
            {
                return Err(HistoryError::UnknownParent {
                    entry: id,
                    parent: p.clone(),
                });
            }
            if let Some(p) = &parent {
                children.entry(p.clone()).or_default().push(id.clone());
            }
            index_of.insert(id.clone(), n);
            ids.push(id.clone());
            parents.push(parent);
            previous = Some(id);
        }

        Ok(Self {
            entries,
            ids,
            parents,
            index_of,
            children,
        })
    }

    /// How many entries the log holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when there is no history yet — the base is the only state.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The log in file order, for persisting or folding.
    pub fn entries(&self) -> &[OpLogEntry] {
        &self.entries
    }

    /// Every entry with its resolved identity, in file order.
    pub fn iter(&self) -> impl Iterator<Item = EntryRef<'_>> {
        self.entries
            .iter()
            .enumerate()
            .map(move |(i, entry)| EntryRef {
                index: i,
                id: &self.ids[i],
                parent: self.parents[i].as_deref(),
                entry,
            })
    }

    /// One entry by id.
    pub fn get(&self, id: &str) -> Option<EntryRef<'_>> {
        let &i = self.index_of.get(id)?;
        Some(EntryRef {
            index: i,
            id: &self.ids[i],
            parent: self.parents[i].as_deref(),
            entry: &self.entries[i],
        })
    }

    /// The id of the entry at a file position.
    pub fn id_at(&self, index: usize) -> Option<&str> {
        self.ids.get(index).map(String::as_str)
    }

    /// The last entry in file order — what a linear log means by "head",
    /// and what the format folds when no tip is named.
    pub fn last_id(&self) -> Option<&str> {
        self.ids.last().map(String::as_str)
    }

    /// An entry's parent.
    pub fn parent_of(&self, id: &str) -> Option<&str> {
        let &i = self.index_of.get(id)?;
        self.parents[i].as_deref()
    }

    /// The entries that build directly on this one, in file order. More than
    /// one means this entry is a fork point.
    pub fn children_of(&self, id: &str) -> &[EntryId] {
        self.children.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Every entry that nobody builds on: the ends of the tree, one per
    /// branch. Empty only when the log is.
    pub fn tips(&self) -> Vec<&str> {
        self.ids
            .iter()
            .filter(|id| self.children_of(id).is_empty())
            .map(String::as_str)
            .collect()
    }

    /// True when more than one entry builds on this one.
    pub fn is_fork_point(&self, id: &str) -> bool {
        self.children_of(id).len() > 1
    }

    /// The path from the base to `tip`, base-first — the entries a fold of
    /// that tip applies, in order.
    pub fn path_to(&self, tip: &str) -> Result<Vec<&str>, HistoryError> {
        if !self.index_of.contains_key(tip) {
            return Err(HistoryError::NoSuchEntry(tip.to_string()));
        }
        let mut path = Vec::new();
        let mut cursor = Some(tip);
        while let Some(id) = cursor {
            let &i = self
                .index_of
                .get(id)
                .ok_or_else(|| HistoryError::NoSuchEntry(id.to_string()))?;
            path.push(self.ids[i].as_str());
            cursor = self.parents[i].as_deref();
        }
        path.reverse();
        Ok(path)
    }

    /// True when `ancestor` lies on the path from the base to `tip`.
    pub fn is_ancestor(&self, ancestor: &str, tip: &str) -> bool {
        let mut cursor = Some(tip);
        while let Some(id) = cursor {
            if id == ancestor {
                return true;
            }
            cursor = self.parent_of(id);
        }
        false
    }

    /// The deepest entry on both paths, or `None` when the two tips share
    /// only the base. This is where a branch rail draws a convergence, and
    /// where a seek between two tips stops walking back.
    pub fn common_ancestor(&self, a: &str, b: &str) -> Option<&str> {
        let on_a: BTreeSet<&str> = {
            let mut set = BTreeSet::new();
            let mut cursor = Some(a);
            while let Some(id) = cursor {
                set.insert(id);
                cursor = self.parent_of(id);
            }
            set
        };
        let mut cursor = Some(b);
        while let Some(id) = cursor {
            if on_a.contains(id) {
                // Return the borrow that lives as long as self.
                return self.index_of.get(id).map(|&i| self.ids[i].as_str());
            }
            cursor = self.parent_of(id);
        }
        None
    }

    /// Append an entry, resolving its identity the same way [`Self::open`]
    /// does. Returns the id it took.
    ///
    /// The caller is expected to have set `parent` already — an editor
    /// always knows which tip it is building on, and leaving it off would
    /// silently chain onto whatever happened to be written last, which is
    /// wrong the moment two branches are alive.
    pub fn push(&mut self, entry: OpLogEntry) -> Result<EntryId, HistoryError> {
        let n = self.entries.len();
        let id = entry.id.clone().unwrap_or_else(|| format!("line-{n}"));
        if self.index_of.contains_key(&id) {
            return Err(HistoryError::DuplicateId(id));
        }
        let parent = entry.parent.clone().or_else(|| self.ids.last().cloned());
        if let Some(p) = &parent
            && !self.index_of.contains_key(p)
        {
            return Err(HistoryError::UnknownParent {
                entry: id,
                parent: p.clone(),
            });
        }
        if let Some(p) = &parent {
            children_push(&mut self.children, p.clone(), id.clone());
        }
        self.index_of.insert(id.clone(), n);
        self.ids.push(id.clone());
        self.parents.push(parent);
        self.entries.push(entry);
        Ok(id)
    }
}

fn children_push(map: &mut BTreeMap<EntryId, Vec<EntryId>>, parent: EntryId, child: EntryId) {
    map.entry(parent).or_default().push(child);
}

#[cfg(test)]
mod tests {
    use super::*;
    use openworldformat::session::SessionOp;

    fn entry(id: Option<&str>, parent: Option<&str>, revision: u64) -> OpLogEntry {
        OpLogEntry {
            revision,
            author: Default::default(),
            ops: Vec::<SessionOp>::new(),
            timestamp_ms: 0,
            id: id.map(str::to_string),
            parent: parent.map(str::to_string),
            message: None,
        }
    }

    #[test]
    fn a_log_without_ids_is_a_chain_in_file_order() {
        let h = History::open(vec![entry(None, None, 1), entry(None, None, 2)]).unwrap();
        assert_eq!(h.id_at(0), Some("line-0"));
        assert_eq!(h.id_at(1), Some("line-1"));
        assert_eq!(h.parent_of("line-1"), Some("line-0"));
        assert_eq!(h.tips(), vec!["line-1"]);
        assert_eq!(h.path_to("line-1").unwrap(), vec!["line-0", "line-1"]);
    }

    #[test]
    fn a_second_child_is_a_branch_and_both_ends_are_tips() {
        let h = History::open(vec![
            entry(Some("e1"), None, 1),
            entry(Some("e2"), Some("e1"), 2),
            entry(Some("e3"), Some("e1"), 3),
        ])
        .unwrap();
        assert!(h.is_fork_point("e1"));
        assert_eq!(h.children_of("e1"), ["e2".to_string(), "e3".to_string()]);
        let mut tips = h.tips();
        tips.sort_unstable();
        assert_eq!(tips, vec!["e2", "e3"]);
        assert_eq!(h.path_to("e3").unwrap(), vec!["e1", "e3"]);
        assert_eq!(h.common_ancestor("e2", "e3"), Some("e1"));
        assert!(h.is_ancestor("e1", "e3"));
        assert!(!h.is_ancestor("e2", "e3"));
    }

    #[test]
    fn mixed_logs_resolve_and_forward_references_are_refused() {
        let h = History::open(vec![entry(None, None, 1), entry(Some("e2"), None, 2)]).unwrap();
        assert_eq!(h.parent_of("e2"), Some("line-0"));

        let err = History::open(vec![entry(Some("e1"), Some("later"), 1)]).unwrap_err();
        assert_eq!(
            err,
            HistoryError::UnknownParent {
                entry: "e1".into(),
                parent: "later".into()
            }
        );

        let err = History::open(vec![entry(Some("e1"), None, 1), entry(Some("e1"), None, 2)])
            .unwrap_err();
        assert_eq!(err, HistoryError::DuplicateId("e1".into()));
    }

    #[test]
    fn push_builds_on_the_parent_it_is_given_not_on_the_last_line() {
        let mut h = History::open(vec![
            entry(Some("e1"), None, 1),
            entry(Some("e2"), Some("e1"), 2),
        ])
        .unwrap();
        // Fork: build on e1 although e2 was written last.
        let id = h.push(entry(Some("e3"), Some("e1"), 3)).unwrap();
        assert_eq!(id, "e3");
        assert!(h.is_fork_point("e1"));
        assert_eq!(h.path_to("e3").unwrap(), vec!["e1", "e3"]);
    }

    #[test]
    fn two_tips_sharing_only_the_base_have_no_common_ancestor() {
        let h =
            History::open(vec![entry(Some("e1"), None, 1), entry(Some("e2"), None, 2)]).unwrap();
        // e2 has no explicit parent, so it chains onto e1 — the degenerate
        // case. A genuinely rootless second entry is not expressible in a
        // single log, which is itself worth asserting.
        assert_eq!(h.parent_of("e2"), Some("e1"));
        assert_eq!(h.common_ancestor("e1", "e2"), Some("e1"));
    }
}
