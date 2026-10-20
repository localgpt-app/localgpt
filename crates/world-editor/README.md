# localgpt-world-editor

The non-linear editing model for the [Open World Format](https://openworldformat.org):
tips, forks, folds and deltas, with no engine and no I/O.

A `.world` is an append-only log over a base, and the state at any entry is
the fold of the path to it, so history is a tree. On that model this crate
gives an editor three things:

1. **A branch index** (`History`): entry identity, parents, children, tips
   and the path to any tip.
2. **A fold fast enough to scrub** (`FoldCache`): snapshots, folding forward
   from the nearest cached ancestor.
3. **A delta** (`Editor::goto`): the ordered ops between the document on
   screen and the one at any tip, which is all a renderer needs to move.

It is not an authority: `localgpt-world-agent`'s `LiveWorld` commits, and
this crate follows. Part of [LocalGPT](https://github.com/localgpt-app/localgpt). License: Apache-2.0.
