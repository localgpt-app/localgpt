# Conformance, vendored

`authoring.json` is a copy of the Open World Format's
`conformance/authoring/cases.json` (the authoring outcome suite), kept here
so CI runs it with no sibling checkout; `tests/authoring_outcomes.rs` runs
every case through the live authority (`LiveWorld::submit`). Refresh it
from the spec repository when the suite changes; it is not edited here.
