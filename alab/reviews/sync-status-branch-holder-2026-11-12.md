# Show the reserved sync branch's actual holder

The full qualification reused a cached worktree whose branch was `alab-sync`.
The default sync worktree did not exist. `sync.sh --status` reported "none yet",
while a later sync with new upstream commits would refuse because the reserved
branch was checked out at the other path. The cached worktree has since been
detached without changing its source or build cache; this change makes future
instances of that mismatch visible.

Share one read-only branch-holder query between status and startup. Parse Git's
NUL-delimited records and preserve unusual path bytes, including trailing
newlines, when returning the holder to the caller. Reject duplicate holders or
failed enumeration instead of treating either as absence. Reset the path at
each record boundary so malformed output cannot inherit a previous path.

When the kernel lock is inactive, status now reports a live holder elsewhere or
an unavailable registered checkout before considering the configured directory.
It prints both paths with Bash quoting. These reports do not prune a registration,
switch branches, reset files or suggest destructive recovery. A held kernel lock keeps
its existing priority and normal matching paths retain their existing status.
Physical path comparison keeps a configured symlink alias equivalent to the
actual worktree.

Startup still owns its existing repair/refusal decisions. The new query avoids
line-oriented path corruption there too; status never invokes those mutations.
An initial implementation exposed a Bash 3.2 parser limitation with nested
`case` patterns inside command substitution. Replacing those patterns with
conditionals fixed the observed failure; validation uses the actual macOS Bash.

Validation results are recorded with the implementation. The tests use real
scratch repositories and compare refs, worktree registrations, indexes and
tracked/untracked file contents before and after status. They also cover held
lock priority, unavailable/duplicate/malformed holder observations and startup
refusal safety. Adjacent push, resolver-containment and daily-job tests retain
their existing boundaries. No Rust or upstream-owned source changed.

All 35 tests passed: 16 status/startup-refusal tests and 19 adjacent sync tests.
A private baseline control reproduced both incorrect holder reports; the
read-only state comparisons passed even on that failing control. Shell syntax
and whitespace checks passed. The failed first Bash 3.2 run is retained with
the final passing logs.

Evidence is retained at
`$ATERM_STATE/.ubd/sync-status-holder-kuau7thd.noindex`.
The earlier full native qualification belongs to `2713722435`; it is not
relabelled as qualification of this later shell change. Source V1 activation
remains held.
