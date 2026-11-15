# History status and overlapping discovery

Extend the reviewed prefix from 31 to 34 upstream commits through
`662daabad243727bc66b9960f6ab3810228b66fa`. Preserve previous decisions unchanged.
The observed `f941a5fb92` snapshot has 62 more commits after this prefix. This
record is source review, not a merge or application qualification.

## `fdad70ba5d`: adapt the ALab fixture, adopt history visibility

Saved history and unavailable conversations gain an explicit
`read_only_history` flag. Status-line items and context use are hidden for
those views even after token updates. A live attachment clears the flag,
including when its execution permission profile is read-only. This flag
describes attachment state, not file permissions. New thread notifications
also clear an inherited flag; cold and warm history-only reads set it.

The footer's renamed context visibility flag still supports pending token
information. The Vim mode suffix emits a separator only after existing
content. Reviewed the production changes, the new live-history-live test,
updated unavailable-conversation snapshots, and changed fixture initializers.

Integration requires one ALab adaptation: the explicit `ThreadSessionState`
literal in `codex-rs/alab-tui/glue/service_tier_tests.rs` needs
`read_only_history: false`. Its read-only permission profile still represents
a live session. Do not add the field before upstream integration: the current
struct does not declare it. Search found no ALab callers of the renamed
context setter or footer field. Require the native service-tier tests, history
visibility test, token-update cases and unavailable/reconnect snapshots on
the integrated candidate.

## `e1340f2ea8`: adopt discovery coalescing behind existing Apps policy

Matching live discovery contexts serialize startup listings through an async
mutex. A waiter reuses a successful publication that happened since it
subscribed; a later startup with no overlapping publication fetches again.
A failed or cancelled owner releases the mutex without manufacturing a
snapshot, allowing a waiter to use its own connection. Each client retains its
own MCP connection; tool discovery sharing does not transfer connection
ownership.

The outer timeout includes waiting for the mutex and the listing, and its
biased selection rejects an expired request when completion is also ready.
The initialization phase retains its separate existing timeout. Scope
construction includes transport/auth context and sorted negotiated discovery
inputs. A server opting out drops its live scope and receives its own listing.
Account-wide latest discovery does not replace the result returned for another
live scope: publication updates the scoped catalog before applying the shared
generation check, and startup reads that scoped catalog.

Read the independent-client, timeout, cancellation and different-scope tests.
Require them, sequential startup/refresh checks, server opt-out checks and ALab
account-isolation checks before integration. No ALab direct live-scope caller
was found. Preserve `apps = false`; this optimization does not authorize
enabling Apps or changing authentication boundaries.

## `662daabad2`: retain removed npm distribution

The upstream wrapper fix correctly keeps forwarding termination signals
instead of treating `ChildProcess.killed` as evidence of exit. Its new test
forces spawn fallback, sends a second SIGTERM after acknowledgment, and checks
numeric exit status. Both changed paths are under `codex-cli`, already removed
by `alab/removed-paths` because atpkg distributes this build. Keep that whole
path absent; do not reintroduce npm packaging to carry this fix. No npm runtime
test is required for absent distribution code.

## Validation

Validate exact 34-commit coverage, unchanged prior 31 decision objects, one
active record per covered commit, and rejection of the next unreviewed target.
Native runtime checks and certified integration remain separate and pending.
