# Prune custody fixture Git

The artifact-retention companion ran 11 methods but failed 26 assertions or
subcases before exercising its intended custody decisions: its system-only
child PATH selected Apple's unavailable Git launcher. The production pruner
correctly failed closed because it could not establish the clone cleanup policy.
That safe refusal masked the test scenarios rather than indicating lost custody.

Decision: discover the caller's Git, require it, and include its bin with the
fixture's system utilities for both normal prune runs and the paused-census
child. The private HOME, package root, selector lock and session records remain
fixture-owned. No production cleanup policy, process census or deletion rule
changed. Also always communicate with the owned census child in finally, even
if it already exited, so an early assertion drains/closes its pipes; an unreaped
live child is killed first as before.

The corrected suite passed all 11 methods in 4.568 seconds with ResourceWarning
configured as an error and no resource warning emitted. Coverage includes
canonical/atomic/quarantined relaunch records, malformed and inaccessible
records, a rename during census, absent versus inaccessible session directories,
special/missing/contended store locks, manual selection between censuses, and
an actual second writer excluded while the pruner holds its snapshot/deletion
lock. Dry runs and clone preservation remain effective. Only exclusive scratch
artifact directories are pruned. Diff whitespace checks passed; no compiler or
real package selector ran.

A single supported m157 status retry still reported a missing/conflicted label
binding. The remote rollout run remains deferred; no alternate identity, address
or duplicate proof was used.
