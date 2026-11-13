<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep checking earned usage resets

A quota error used to make one bounded earned-reset attempt. When no credit was
available then, the original sampler ended and recovery waited for the provider's
named reset. A credit granted during that wait was only displayed by the TUI;
there was no request left to redeem it.

Ordinary quota recovery now retains the original sampler and checks every 30
seconds, with a 45-second deadline per attempt. The account custody lock is
released between checks. New credits are selected through the existing durable
ledger, earliest expiry first. Unknown POST replies keep the same idempotency key;
a consumed credit awaiting usage propagation cannot cause another redemption for
the same cycle. Cancellation drops the wait and any held account lock. Identity
changes and explicit spend or entitlement denials still stop this recovery path.
Recovery also accepts a credit when the natural reset is less than five minutes
away; that headroom threshold belongs only to preflight. Preflight remains one
bounded check.

The headless exec quota observation also remains live while this recovery loop
polls. Publishing it only in the later quota retry wait hid the root request
once recovery began waiting indefinitely. The recovery path now holds the same
authenticated request observation lease; cancellation or restoration drops it.
The existing authenticated root and child tests also require shutdown to remove
the observation and invalidate the retained wall.

Unit coverage exercises late credit availability, cancellation and account-lock
release. The native `alab/smoke/weekly_reset.py --case late-credit` case withholds
the first credit list, grants a credit on the next poll, and checks that the same
native turn resumes with its completed tool output intact.

Validated on 2026-11-09 with the Trust toolchain in the unverified lane:
`codex-cli` builds, all 12 quota-recovery unit tests pass, and all 15 native weekly
reset smoke cases pass. The late-credit case issues three model requests and one
redemption: the tool request, the denied sampler, and that same sampler resumed
after the newly granted credit. Its two credit-list reads are at least 29 seconds
apart. No-credit cancellation exits without a guardian timeout. No new upstream
hook or dependency is needed.
`targo tippy -p codex-alab-quota-recovery --all-targets` also passes with no
quota-recovery diagnostics; its warnings are in unchanged upstream dependencies.

## Follow-up review

The installed channel's local updater reported on 2026-11-09 that the selected
checkout is not a member of its local artifact channel. Preserve that selection;
pushing source to main does not establish that a running image has adopted it.

A redemption can advance the reported reset timestamp before its usage percentages
settle. Unreleased consumed-credit custody must therefore block a second
redemption independently of that timestamp, until fresh usage proves recovery.
The regression supplies a second eligible credit during this propagation window.
The prior cycle-dependent guard fails the strengthened regression with two
redemption POSTs (expected one). Removing that cycle comparison makes all 12
quota-recovery tests pass, with one POST while the reset time advances through two
still-denied usage reads. This follow-up changes no hook or public API.
The follow-up's all-target Tippy check passes with no quota-recovery diagnostics.

## Cancellation while redemption is in flight

The sleep-cancellation test did not cover dropping a request that owns both the
account lock and an uncertain POST. Add a loopback HTTP regression for that case:
the server receives the first redemption but delays its response; the caller
cancels after that receipt, then another recovery reconciles the pending operation
through `already_redeemed`. Assert that both POST bodies have the identical credit
and idempotency key, no second credit list is requested, and consumed custody is
released only after fresh usage proves recovery. Bound the second recovery to five
seconds so a leaked account lock fails the test. This protects existing behavior;
no production path needs changing if the test passes.
The regression passes: a new turn reuses the exact pending key and credit, the
account lock is released promptly, and one `already_redeemed` response settles
custody after a fresh allowed usage read. All 13 recovery tests pass. This adds
coverage only; it requires no production behavior change.
The all-target Tippy check also passes with no quota-recovery diagnostics.

## Keep watching a stale natural-reset timestamp

A denied usage read can still contain the exhausted window's expired reset time.
Previously that made `eligible` return no cycle and ended the in-flight recovery,
leaving the session dependent on the outer wall retry schedule again. The loopback
regression reproduces that exit before a subsequent fresh cycle and earned credit
can be read.

Treat a valid, same-account ordinary exhaustion with a positive expired timestamp
as authority to wait for another read only. It cannot list or redeem credits from
that stale observation. Identity mismatch, explicit spend or entitlement denial,
unknown permission, malformed timestamps and unrecognized windows retain the stop
behavior. Preflight is unchanged. The regression now keeps the original request,
waits through the stale read, and redeems exactly once after a fresh cycle and
credit arrive. All 15 recovery tests pass, including rejection cases for the
wait-only predicate.
The all-target Tippy check passes with no quota-recovery diagnostics.

## Join native account reassignment's resource drain

Quota recovery uses the backend client directly. Register the whole bounded
attempt with its AuthManager's account resource barrier, as the app-server account
endpoints do, rather than depending on a caller's task scope or transport factory.
Keep the resource through authentication, account-lock waiting, receipt changes
and HTTP, and release it before the thirty-second poll sleep. The lease carries
liveness only: retain the original identity checks and recheck ownership after
auth resolution and before the first usage HTTP call.

The old path fails two isolated-domain regressions: an unscoped attempt performs
HTTP while reassignment is pending, and a source can freeze while a delayed quota
HTTP call remains active. With the lease, refused admission makes the attempt
wait, cancellation releases the resource so retirement can finish, and the source
can also freeze while the longer quota wait is sleeping. These cases use fake auth,
a loopback server and an isolated AuthManager; no live source selection changes.
All 18 recovery tests pass. No upstream hook or dependency changes are needed.
The all-target Tippy check passes with no quota-recovery diagnostics.

## Distinguish uncertain submissions from restored allowance

The integrated CLI run at `7f25a813c7` exposed a regression in the unconditional
consumed-credit guard: `pending-old-cycle` reconciled its original key as
`already_redeemed`, then waited indefinitely instead of using the current cycle's
fresh credit. A loopback unit regression reproduces `Wait` where recovery should
finish. This result supersedes the narrower earlier validation of that guard.

Ledger format 2 records `awaiting_ack` durably before every POST, including a retry
of an imported receipt. An unknown reply keeps that state even if reset timestamps
change. Fresh usable allowance observed while the ACK is uncertain records
`restored_pending`; this is evidence that a later denial is a new exhaustion.
A known `already_redeemed` response can retire the previous retry right after that
evidence, or when reconciling a version-one pending receipt from a different
pre-retry cycle. Retiring that right itself grants no sampler retry. Fresh current
usage still governs redemption of another available credit.

Version-one receipts retain their original credit, generation, counter and key.
They are read into the format-two state machine; no pending key is regenerated.
New states are rejected in version-one records, and older readers refuse format
two. This migration is needed to preserve already-owned receipt custody while
making new submissions distinguishable across crashes and changed timestamps.

The unit regressions cover older-cycle reconciliation and usable allowance
followed by another exhaustion in the same cycle. Cancellation now also advances
the reset time, leaves usage denied after `already_redeemed`, and exposes a second
credit: recovery must keep the first key and leave that second credit alone. All
20 recovery tests pass. Native smoke additions exercise stale timestamps, delayed
propagation with a spare credit, and the lost-ACK propagation case using the real
CLI and its thirty-second polling cadence.

Final integrated validation passes: the CLI builds, all 20 unit tests pass, all 18
reset smoke cases pass, both native quota-wait cases pass, and the all-target Tippy
check has no quota-recovery diagnostics. In particular, `pending-old-cycle`
reconciles its original receipt and uses one fresh credit; `lost-ack-propagation`
replays the same key, waits through lagging usage, and leaves the spare credit
untouched. `lost-ack` verifies the persisted restoration witness. The tested CLI
SHA256 is `3dcd3b8404b813f4f8c2b8e32b764edf4fa17bc2d07d07ee10970eff773f4bac`.
These are unverified-lane builds and isolated loopback functional checks, not Trust
proof claims or live-account redemptions.

Main advanced during validation with the continuous-goal interrupt fix
`ae05dda1df`. Lossless merge `a6d7fbcce8` preserves the quota Rust and smoke
sources exactly as in `b62843067e` and the incoming goal-interrupt Rust sources
exactly as on origin. On the merged tree all 20 quota-recovery tests and all three
goal-interrupt widget regressions pass. The CLI reset/wait smoke results above
exercise unchanged exec paths; the focused widget run qualifies the incoming TUI
interrupt behavior with the new receipt state machine compiled into the same tree.

## Preflight recognizes a committed credit before the ledger ACK

The successful-credit receipt is saved before the main ledger acknowledgement.
A crash in that interval leaves a pending ledger beside a committed credit. The
old preflight checked only the ledger outcome, so even a fresh usable allowance
could leave that credit unsettled. A later exhaustion then held the old retry
right instead of using a fresh credit. The regression reproduces that failure.

Preflight now uses the validated, committed credit receipt when settling a usable
allowance. It retains the restoration witness and original key; it does not replay
the settled receipt. This preserves the existing safe write order and needs no
new state or data migration. All 21 recovery tests pass, including a partial-write
fixture that later exhausts the allowance and redeems exactly one fresh credit.
The native `committed-ack` case additionally checks settlement before the first
model request, then preserves the completed plan through that later exhaustion.
The CLI builds and all five focused native cases pass: `committed-ack`,
`pending-old-cycle`, `unused-pending`, `lost-ack`, and `lost-ack-propagation`.
The all-target/no-deps Tippy check also passes with warnings denied. The tested
CLI SHA256 is `a4825e99f7e94f4fa962c9f21128f8be325ae4ea3040bc9bccc954881a800883`.

Lossless merge `c33fdf0df7` brings in account-source integration `0413b029ff`;
the quota-recovery and native smoke sources are byte-identical to `cae22813ab`.
On that merged tree all 21 quota unit tests pass, the CLI builds, and all five
focused native cases above pass again against the newly built binary. These
checks exercise the new account infrastructure through actual quota recovery.
The merged CLI SHA256 is `b126630ebda2a00a69960b6eccab63ebe0a9badd0aa6966c834dac4de15a4ce8`.
The merged all-target/no-deps quota Tippy check passes with warnings denied.

## Adoption follows the selected development channel

The selected managed Codex is dev-linked to the package rooted at
`${HOME}/codex-cruise-continuation-20261104-targo/staged`, source
`e51f4b9063da636d23211e37e12ee0fa2c471ead`. The default updater output is a
different channel, so its no-op is correct. Invoking the public updater with the
existing selected output root and durable aterm executable admitted the channel
and dispatched the exact `773afd537c` controller. Packaging stopped because no
qualified `atpkg-publish` was available on PATH; the normal aterm debug/release
build locations also had no publisher. The updater preserved the selected package.
Do not replace the executable with the unverified test binary or bypass native
project qualification. Adoption is deferred until the qualified publisher is
available; rerun the guarded updater against that same selected output root.
The failed controller is preserved by the updater's cleanup policy at
`.git/alab-local-controller-4ss_qe52`. No agent restart was attempted because no
updated managed executable was selected. Continue with remaining native regression
checks on the already built merged CLI while package production is unavailable.

All 14 remaining native reset scenarios pass on the same merged CLI, completing
all 19 cases with the earlier five. This includes late-available credits, stale
reset timestamps, delayed propagation, near-natural reset, repeated exhaustion,
both preflight windows, negative identity/provider cases, and no-credit waiting.
Results are in `codex-rs/target/accounts-merged-remaining-smoke/result.json` in
the recovery worktree. Package adoption remains deferred; these isolated native
checks do not claim the installed launch target changed.

## Qualify the automatic request-timeout path

Manual cancellation already proves reconciliation of an interrupted POST, but it
does not exercise the 45-second attempt watchdog in the continuous wait loop.
Add a real-loopback regression whose first successful reset response is delayed
60 seconds. Without caller cancellation, the original wait must time out that
attempt, release account custody, replay exactly the same credit/key, and finish
within the following 55-second bound. The mock keeps usage denied through the
reconciliation ACK, then restores allowance on the next read. Both POST bodies
must be identical, and the consumed receipt must be settled. Share the fixture
with the manual-cancellation regression; preserve its pending-receipt assertions.
No runtime behavior or timing constants change. This checks the existing bounded
request mechanism independently of manual interruption.

All 22 quota-recovery tests pass, including automatic timeout reconciliation;
the test phase completes in 45.60 seconds. All-target/no-deps Tippy passes with
warnings denied after removing an unused fixture variable. Runtime sources are
unchanged, so the previously qualified CLI and its 19 native scenarios retain
the same source behavior; no redundant CLI rebuild is needed for this test-only
change. These remain unverified-lane checks, without a Trust proof claim.

## Cover natural reset without an earned credit

The full native suite covered newly granted credits but lacked a no-redemption
natural reset. Add `natural-reset`: no credits are offered, the first wall usage
read denies allowance, and the second paced read restores it after the reported
reset time. Assert exactly one empty credit listing, no consume POST, two usage
polls separated by at least 29 seconds, and retained plan/input through success.
The first probe exposed a test-only assumption that every successful recovery had
a consume record; use the quota wall as the freshness anchor when no credit was
redeemed. The production binary already completed the turn correctly.
Document the continuous polling, request bound, identity/cancellation behavior,
and native probe in the README so later reviewers need not infer the policy from
this decision journal. No runtime Rust or installed selection changes.

The new natural-reset case and the existing `recovery` and `preflight-weekly`
controls pass on CLI SHA256
`b126630ebda2a00a69960b6eccab63ebe0a9badd0aa6966c834dac4de15a4ce8`.
These three native cases qualify the changed shared assertions without rebuilding
unchanged runtime code. The suite now offers 20 reset scenarios.

## Build the documented native publisher and retry adoption

A wider discovery found the README's supported producer-build route. No registered
aterm worktree had `atpkg-publish` in its target profiles. Create owned detached
aterm worktree `${HOME}/aterm-codex-reset-s113.noindex` at `798fd5fd8f`,
initialize its pinned `vendor/astream`, and build the native producer with
`targo --unverified build --profile test --locked -p atpkg-publish`. It succeeds
in 1m 03s; Cargo places the test-profile binary at `target/debug/atpkg-publish`.
Its `toolchain run --repo ${HOME}/codex-alab -- /usr/bin/true` admission
succeeds with this source's pinned Trust 15698. The previous unavailable operation
can therefore advance without bypassing qualification or changing the toolchain.
Retry the public updater for the already selected staged output root with that
absolute `ALAB_ATPKG_PUBLISH`. It dispatches exact source `e2050ced0f`, authenticates
its lock, and enters normal protected package production. Producer build logs and
update logs remain in the recovery worktree; this unverified producer build makes
no Trust proof claim. Do not claim adoption until production and guarded selection
finish, and distinguish selection from an agent adopting the new image.

The guarded updater completed successfully and selected
`0.2.0-dev.15212+codex.0.167.0.ge2050ced0` for new launches, under the original
selected staged channel. Package smoke verified layout, executable signatures and
system-library dependencies. `aterm pkg which codex` independently resolves the
managed launch target to the new immutable package. This supersedes the earlier
deferred adoption report. The selected source is `e2050ced0f`; later journal-only
commits do not require rebuilding unchanged runtime code.

The exact selected package passes native `late-credit`, `natural-reset`, and
`committed-ack` scenarios through the probe's `--package` admission. Its binary
SHA256 is `e26f3d46729921375f3a1eeefa1886b7086175444f9bdac04f6132e8a858a50c`.
The report is `codex-rs/target/selected-package-reset-smoke/result.json` in the
recovery worktree. These isolated fake-auth checks do not redeem live credits.
Save a restart handoff and restart this agent through aterm, then independently
verify its actual running image. Package selection alone does not prove adoption
by this or another already running session; peer sessions have not been restarted.

## Verify this session actually adopted the selected image

The controlled restart succeeded. Session 113 now supervises PID 28026; its
executable path is the managed `pkg/store/codex/exec/codex` symlink, resolving to
the selected `e2050ced0f` package. Both path hashes are
`e26f3d46729921375f3a1eeefa1886b7086175444f9bdac04f6132e8a858a50c`.
A sample of the running process reports Mach-O UUID
`1B1EB5BE-F0AE-3D67-854C-BA4AA22DFB78`, identical to `dwarfdump --uuid` of
the selected package executable. This verifies actual image adoption rather than
inferring it from the current symlink. The restart resumed the same conversation
under Cruise. No qualification jobs remained running before the restart.

Read-only observation of the original Trust tab shows its old version with an
update indicator, active compiler/check descendants and 4% weekly usage. Do not
interrupt those owned checks or claim that tab adopted the new package. The safe
update gate can hand it over when its work permits. No peer prompt or restart was
sent. The clean temporary native publisher worktree was removed with
`git worktree remove --force` after adoption was verified. No aterm source edits
were made. Shared submodule registration and the updater's separately preserved
worktrees remain intact; a later producer run can rebuild the documented tool.

## Scope consumed-credit retries to each waiting turn

A waiting turn can already have used its one external-restoration retry. If a
second turn then consumes and settles an earned credit, the account-wide
`retry_released` flag is true but the first turn's external marker already exists.
The old release path therefore stops the first turn despite fresh usable usage.
The new shared-credit unit regression fails on this exact interleaving, including
two successive credits on one natural reset cycle.

Separate the account restoration witness from each turn's retry grant. Preserve
the existing consumed-credit receipt and external marker; write a private durable
marker keyed by both turn and consumed credit to bound each independently. A
fresh usable account read is still required at every call site. Save the account
restoration witness before the final scoped grant, so a failed credit-receipt
write can be retried without spending a turn's grant or repeating a POST. The
account lock serializes these operations. Pending/unspent credits still use the
single external retry, and an unpropagated successful reset still cannot select
another credit. Keys, generations and ledger schema remain unchanged.

Existing receipts have no scoped grant marker. Fresh verified usable allowance
can grant one bounded per-credit retry after upgrade; it cannot grant repeated
retries for that pair or authorize a reset POST. Keep the old external marker to
prevent an additional unscoped retry. This is a deliberate retry-bound change,
not a deletion or reinterpretation of redemption custody.

The native `shared-credit` fixture retains two completed plan actions. During the
second quota wall it atomically installs a peer's consumed-and-settled credit
receipt, then reports restored usage. The tested turn must resume with one local
consume POST total. The old selected image fails this control: under a shortened
10-second guardian it stops at four model requests despite the peer receipt and
fresh usable usage; no fixture contract error or extra redemption occurs. All 23
quota tests pass after the fix, including the 45-second request-timeout case.

The CLI builds on the account integration merged at `d05602796d`. All eight
focused native cases pass on the rebuilt CLI: `shared-credit`, `stale-restored`,
`stale-credit-ledger`, `repeated-same-cycle`, `unused-pending`, `natural-reset`,
`committed-ack`, and `lost-ack-propagation`. The shared-credit turn completes with
five model requests, both plan outputs retained, and only one local consume POST.
The stale-response controls still stop repeated retries for an already granted
turn/credit pair. The tested CLI SHA256 is
`b1a86e2770f7e5f164ced965e5bb486f11da680f837491c370eac79b1268b688`.
This follow-up is source-qualified; the currently selected immutable package
still contains `e2050ced0f` and does not yet include this shared-credit fix.

All-target/no-deps quota Tippy passes with warnings denied. Final review changed
the shared-credit fixture to require a successful usage read before the resumed
model request, permitting harmless extra usage reads. That focused case passes
again on the same binary. All source changes are ALab-owned; no dependency,
upstream hook, authentication boundary or purchase behavior changes.

## Select and qualify the shared-credit package

Rebuild the documented native publisher from the same qualified aterm source in
owned worktree `${HOME}/aterm-codex-reset-s113.noindex`; it succeeds in
1m 03s. The public updater admits exact `2c9d378b21` with pinned Trust 15698,
builds the package under its protected production path, and passes executable
signature, system-library, ripgrep/PCRE2 and layout checks. Guarded selection
advances the original staged channel to
`0.2.0-dev.15225+codex.0.167.0.g2c9d378b2`. The updater reports completed exit0,
and `aterm pkg which codex` independently names the new immutable package.

The exact selected package passes native `shared-credit`, `stale-restored`,
`stale-credit-ledger`, `late-credit`, and `natural-reset` checks through `--package`
admission. This supersedes the source-only qualification above: the shared-credit
fix is now selected for new launches. Its binary SHA256 is
`4edb304d3fb45527da962e0739986d4ef371e77eb999b63a4ebdafe0f789ea40`; Mach-O UUID is
`6EDB80BD-461E-3A77-8E98-DBB26EB41C63`. The report is
`codex-rs/target/shared-credit-selected-smoke/result.json` in the recovery worktree.
The same-key and one-retry-per-pair controls remain effective in the optimized
package, and no live credits were redeemed by these isolated checks. Save the
handoff and expected image identity before restarting this session; selection
alone still does not prove a loaded process adopted the new image.

## Verify the running shared-credit image

After the controlled restart, session113 resumes the same conversation under
Cruise with supervisor PID35238. A running-process sample identifies Mach-O UUID
`6EDB80BD-461E-3A77-8E98-DBB26EB41C63`, matching both the saved qualified-package identity and
`xcrun dwarfdump --uuid` of the selected executable. This verifies adoption of
source `2c9d378b21`, including per-turn shared-credit retries. No package or test
jobs were running at restart. The temporary native-publisher worktree was clean
and has now been removed with `git worktree remove --force`; no aterm source edits
or submodule deinitialization occurred. The updater's own preserved worktrees and
immutable packages are retained by their existing cleanup policy. Peer sessions
were neither prompted nor restarted. The earlier source-only limitation is closed
for the selected package and this running session.

## Qualify transient usage and credit-list outages

The continuous wait loop treats usage/list transport failures as retryable, but
the native scenarios did not inject a failure in those reads. Add `usage-outage`
and `credit-list-outage`, each returning one HTTP 503 only after the model quota
wall. Require the same endpoint to be read again at least 29 seconds later and
for redemption to follow a valid response. A failed usage read must not authorize
a credit listing. Keep the existing retained-input, completed-plan, single-turn
and exact-consume-count assertions. This tests continuation through temporary
service uncertainty without weakening identity or allowance checks.
No runtime code changes are needed. Clarify the README's cadence as 30 seconds
between attempts, since a request itself can take up to the 45-second bound.

Both new outage cases and the ordinary `recovery` control pass against selected
package `0.2.0-dev.15225+codex.0.167.0.g2c9d378b2` through `--package` admission.
Its tested SHA256 is `4edb304d3fb45527da962e0739986d4ef371e77eb999b63a4ebdafe0f789ea40`.
The probe now offers 23 native scenarios; these three selected cases qualify this
fixture change. Runtime sources are unchanged, so no rebuild or adoption restart
is needed. Raw process/HTTP results are retained at
`codex-rs/target/transient-reset-endpoints-smoke/result.json` in the recovery
worktree. The checks use isolated fake auth and loopback, without live redemption.

## Complete the selected package's widest native reset check

After the per-turn credit scope change and outage fixtures settled, run all23
native scenarios against the selected immutable package. Earlier focused checks
qualified individual changes; this full run checks their composition with every
pending-receipt, preflight, propagation, identity/provider and no-credit path.
The report's case names exactly match `CASE_NAMES`, with no omitted or repeated
case, and all23 pass. This includes the newly added endpoint outages and shared
credit interleaving as well as both preflight windows and lost acknowledgements.

Package source remains `2c9d378b21`; binary SHA256 is
`4edb304d3fb45527da962e0739986d4ef371e77eb999b63a4ebdafe0f789ea40`.
Probe SHA256 is `ca1fd8f3f7decbd17180998fdc94427c8c6604655f52986dcc67db7ab43ae93f`.
The complete result and individual HTTP/process evidence are under
`codex-rs/target/selected-reset-full-suite` in the stable recovery worktree.
No runtime source, installed selection or running image changed for this check;
no additional rebuild or restart is needed. These are isolated fake-auth,
loopback functional checks without live credit redemption or a Trust proof claim.

## Require an actual retained wait before probe cancellation

Review found that `no-credit` and `stale-credit-ledger` could signal SIGINT as
soon as a credit-list request appeared in the HTTP log. The server appends that
record before delivering its response, so it did not establish that recovery
processed the result or retained the original sampler. A fallback native quota
notification could also terminate this observation too early.

For these two retained-recovery cases require at least two credit-list polls
after the last model request, separated by at least 29 seconds, with no additional
model or consume request. A fallback quota notification does not satisfy that
contract. Other observed cases still await their native notification. Guard the
initial observation before accessing model timestamps; it begins before startup
has produced any model request. Keep the one-minute hang detector and existing
guardian-owned cancellation/retirement unchanged.

The selected package passes `no-credit`, `stale-credit-ledger`, `stale-restored`
and `identity-mismatch` with this stronger observer. As a negative control, the
old `e51f4b906` image makes exactly two model requests and one credit-list request,
then parks. The probe rejects it with `retained recovery did not make a paced
second poll`, despite its native quota notification; no fixture contract error
occurs. Its binary SHA256 is `ab7226a0bce7340af81031fcd2d7d8d931d7483031e5c160d55dbe5729d742ff`.
This independently demonstrates that a single initial check cannot pass the
retained-wait scenarios. Runtime sources and installed selection are unchanged;
no rebuild or restart is needed. Results are retained under
`codex-rs/target/retained-observer-positive-fixed` and `retained-observer-negative`
in the recovery worktree. Earlier full-suite qualification used the weaker
observer; these four focused runs qualify the strengthened cancellation paths.

## Keep checking after a spent retry without suppressing the native deadline

The shared-credit grant bound correctly stops duplicate sampler retries, but its
boolean result conflates an already used grant with missing/corrupt storage.
After one reset and retry, the model can report quota exhaustion while usage still
looks healthy. The old path then returns Stop and parks in the native quota wait,
missing later usage convergence and the next usable earned credit. The selected
2c9d378b21 image reproduces this: the new stale-allowance-credit fixture times out
under a 10-second guardian after 3 model requests and 1 consume, without a fixture
contract error. The fresh denied usage that could authorize credit B is never read.

Add an explicit Spent result for a previously used, empty regular retry marker.
Keep missing credit proofs and corrupt, linked or unwritable stored markers on
the Stop path. Used grants
retain paced usage polling without another sampler request. Fresh confirmed
exhaustion can still select a new earned credit under the existing account lock
and immutable-key ledger. No extra redemption is authorized by healthy usage.
Read existing markers through the bounded no-follow receipt reader and preserve
empty-file creation and directory/file synchronization for new grants.

This polling must not indefinitely suppress the native quota policy's own reset
retry. Pass the original UsageLimitReached reset timestamp through the ALab-owned
retry glue and the Unix/non-Unix recovery signature. A spent grant polls only
before that deadline, sleeping at most the smaller of 30 seconds and time left.
At an absent or elapsed deadline it hands control back to the native policy.
Ordinary no-credit/transport waits retain their existing behavior. Inventing a
deadline for an error with none would replace the native scheduler's policy.

The spent-retry-deadline fixture reports a 3-second reset on its second quota wall
and permits the final model request only at or after that original deadline. It
passes on the installed baseline and will qualify the same handoff after this
change. The stale-allowance-credit fixture instead keeps one healthy-but-used
observation, then after a paced poll reports real exhaustion and uses credit B.
The stale-restored cancellation observer now requires two paced usage reads,
without another sampler or consume, since that turn should remain in recovery.
All 24 quota unit tests pass, including both external/credit marker classification,
corruption/symlink refusal, and the existing 45-second timeout reconciliation.

The updated CLI builds; all-target/no-deps quota Tippy passes with warnings denied.
All 25 declared native scenarios pass with no omitted or repeated case, including
the new spent-grant convergence, preserved native deadline, and strengthened
stale-restored wait. The tested binary SHA256 is
`3178fd92c5f93881d9117820e6ea1a8f3414cc9c2ec9d1303afc820ebc67abbc`; probe SHA256 is
`ec75aab70132f867a0b7cfd9af24155874fa20692dfa24306cd29adce0d7b4b1`. Full results and HTTP/process evidence are in
`codex-rs/target/spent-retry-full-native` in the stable recovery worktree.
This qualifies source behavior on the account integration already on main. All
changes remain ALab-owned, with no upstream patch or dependency change. The
currently selected package still contains 2c9d378b21; the guarded producer must
select this committed fix before another adoption restart.

## Produce and select the spent-grant continuity fix

Build the documented native publisher at the same qualified aterm source in the
owned detached worktree; it succeeds in 53 seconds. The guarded updater admits
exact `b4f3bf3956` with pinned Trust 15698, completes protected package production,
and passes its version, signature, system-library, ripgrep/PCRE2 and layout checks.
Selection atomically advances the original staged channel to
`0.2.0-dev.15231+codex.0.167.0.gb4f3bf395`. The completed updater reports exit0;
`aterm pkg which codex` independently resolves the managed launch target to it.

The exact selected package passes `spent-retry-deadline`, `stale-allowance-credit`,
`stale-restored` and `shared-credit` through native `--package` admission. Its binary
SHA256 is `c3b3ec48fd8272c805de8a39e3710594fef88f302a199e620ea5ea9031e51340`; Mach-O UUID is
`005BC805-CCF3-3695-B3A0-D8C8218F732E`. Evidence is under
`codex-rs/target/spent-retry-selected-smoke` in the stable recovery worktree.
This closes the source-only selection limitation, without live credit redemption
or a Trust proof claim. Save the handoff and expected image identity, then safely
restart this agent and verify actual loaded-image adoption separately.

## Verify actual spent-grant image adoption

The controlled restart resumes the same conversation under Cruise with supervisor
PID28304. A process sample reports UUID `005BC805-CCF3-3695-B3A0-D8C8218F732E`, matching the saved
qualified-package identity and a fresh `dwarfdump --uuid` of the selected binary.
This verifies the running image contains b4f3bf3956, not merely that its launch
symlink changed. No qualification jobs remained running at restart. The clean
owned publisher worktree was then removed with `git worktree remove --force`,
without aterm source changes or shared submodule deinitialization. Updater-owned
preserved worktrees and immutable packages remain under their existing policy.

A read-only recheck of the original Trust tab still shows its old 14755 build
with an update indicator and a working task. Leave that agent's ongoing work
undisturbed; no peer prompt or restart was sent. Selection and this session's
adoption are complete; this record makes no claim that the Trust peer adopted it.

## Adopt the fix in the original Trust conversation

A later live inspection finds the original Trust tab's tool jobs exited, no work
left running, clear input, no human hold, and a proven supervised foreground
process. This supplies the safe restart point previously unavailable. Save a
handoff in the owned worktree's ignored target directory and request the native
same-conversation restart of PID74324. No Trust source files are changed.
The replacement PID7506 displays version15231; its sampled Mach-O UUID matches
`005BC805-CCF3-3695-B3A0-D8C8218F732E`, proving actual adoption of b4f3bf3956.
Historical terminal excerpts have a ledger gap; these conclusions use live status
and process-image evidence instead.

The restarted conversation remains idle despite Cruise. Reapplying Cruise and a
native Off/back transition do not resume its saved goal. The TUI explicitly says
the goal was left as it was under Autopilot and that `/goal resume` continues it.
Under the user's explicit request for continuous Trust work, use that local
command with a drive lease. It reports the existing compiler/proof-chain objective
active and enters Working. No replacement task prompt, increased goal budget, or
live reset redemption is supplied. Cruise remains enabled. This is operational
recovery evidence, not a claim that arbitrary interrupted goals auto-resume after
every upgrade; the old-image restart migration deserves separate investigation.

The next issue-list read is refused by GitHub with issues=read permission403;
defer that operation without requesting access. Read-only sync status also finds
an old committed/unlanded sync checkpoint stopped at gate exit4, with380 upstream
commits pending and origin moved. No sync continuation or checkpoint mutation is
performed as part of this peer recovery.

## Audit the complete selected package and restart boundary

Cruise's next improvement is stronger deployment evidence: the complete native
reset suite against the selected immutable package, rather than only the four
deployment controls. All25 cases pass with zero guardian timeouts. Independently
check their ordered names against the probe's declared CASE_NAMES, require no
duplicates, verify every case passed, and recompute both binary and probe hashes.
The binary remains c3b3ec48fd8272c805de8a39e3710594fef88f302a199e620ea5ea9031e51340;
the probe is ec75aab70132f867a0b7cfd9af24155874fa20692dfa24306cd29adce0d7b4b1.
Evidence is `codex-rs/target/selected-full-reset-audit/result.json` in the owned
worktree. The separate renewal-fixture oracle suite passes all18 tests. This run
uses fake accounts and loopback only, with no compilation or live credit use.

Review the restart question before adding policy: upstream's goal-resume prompt
is shared by Paused, Blocked and UsageLimited goals. ALab's owned plan_answer glue
deliberately answers Leave paused and emits the notice observed after the restart.
That notice alone establishes neither the original stopping reason nor authority
to resume every goal. Preserve this boundary rather than interpreting Cruise as
permission to revive a person's pause. The original user's explicit request
authorized this conversation's local goal recovery, whose subsequent live reply
confirmed it was checking existing workers and the SDK-linker compiler defect.
Do not generalize that operational recovery into an unconditional goal-resume
patch. Any migration improvement needs persisted stop provenance and a native
regression that proves genuine pauses remain stopped.

## Guard receipt and credit-selection liveness

Hostile review finds two useful missing regressions rather than a demonstrated
production defect. Add a retry-marker FIFO with no writer: recovery must refuse
the nonregular receipt without blocking the agent. Add a backend credit list
containing an expired credit, one expiring exactly now, a malformed expiry, a
corrupt local receipt, an already-tried credit, a usable credit and a nonexpiring
credit. Selection must skip the unusable/uncertain entries and choose the first
usable expiry. This protects both continuous progress and non-reuse of uncertain
credits; it changes no recovery policy or installed runtime. All changes stay in
the ALab-owned quota crate. Validation uses the PATH Trust16387 toolchain in the
owned worktree, explicitly unverified, without a proof claim.
All26 quota unit tests pass, including the actual45-second request watchdog.
Quota-crate all-target Tippy with `--no-deps -- -D warnings` also passes.

## Make partial smoke coverage explicit

The smoke runner permits selected cases, but its old aggregate `passed: true`
does not distinguish subset success from full qualification. Add requestedCases
and fullSuite (true only after all declared cases succeed), retain the failing
case alongside already completed cases, and reject duplicate selections before
creating output. Keep passed as the success of the requested run. This improves
reviewable evidence without changing the runtime or rerunning a costly full
suite for a reporting-only edit.

All22 smoke-tool unit tests pass, including four coverage-accounting controls:
subset success, failure after a completed case, full default selection, and early
duplicate rejection. The actual selected package also passes spent-retry-deadline
through the updated runner; its report explicitly says fullSuite=false with the
one requested case and no failedCase. Evidence is under
`codex-rs/target/reset-report-accounting` in the owned worktree. The prior complete
25-case package audit remains evidence from its recorded exact probe hash; this
single-case report is not represented as another full-suite audit.

## Check the surrounding continuity and deployment boundaries

Review of redemption reconciliation confirms that a known reset with delayed
usage propagation retains its immutable operation and waits rather than spending
another credit. Expand verification to existing adjacent suites: ten renewal
receipt/custody tests and fifteen child-exec continuity tests pass. The updater's
54 integration tests initially produce ten failures under Apple's Python3.9.6;
the native-builder fixtures require3.11 or newer. Rerun the exact suite with the
installed Python3.14.8 and all54 pass. These are not unexplained production
failures or excused assertions: changing only the test interpreter resolves them.

Document this integration-test prerequisite and command beside the updater's
controller/build interpreter requirements, so future review does not confuse the
controller's valid3.9 minimum with builder-fixture compatibility. The updater tests
use isolated fake package hosts and deliberately preserve uncertain-completion
fixtures under their cleanup policy; do not remove that evidence as a shortcut.
No selected package, production controller, Trust work, or recovery policy changes.

## Preserve incomplete reports across cancellation

The runner's case cleanup already unwinds on KeyboardInterrupt/SystemExit, but
the aggregate report previously vanished because it caught only Exception. Catch
these two interruption types after case cleanup, write interrupted=true with
passed=false/fullSuite=false and the current failedCase, then rethrow the original
exception. Preserve signal exit semantics rather than treating cancellation as
ordinary failure or success. Abrupt termination before handlers are installed,
SIGKILL and a second interruption during reporting cannot guarantee a report.

All23 smoke-tool tests pass, including interruption after an already completed
case with exact exception identity retained. An actual selected-package loopback
probe is sent SIGTERM after its native child starts. The probe exits143 and its
aggregate report explicitly records incomplete coverage. A subsequent PID check
finds the recorded native child absent. Its guardian result file exists but is
empty after interruption: do not count that as complete guardian evidence or a
successful case. The interrupted requests trace remains available. Evidence and
the corrected verification summary are under
`codex-rs/target/reset-cancel-report-audit` in the owned worktree. No live account,
credit, production runtime or goal policy changes.

## Keep operational guidance aligned with the report contract

A later read-only check finds the original Trust supervisor still PID7506 on
version15231, Cruise enabled, program_state=working, the same compiler/proof-chain
goal, and recent concrete tool activity. No intervention is needed. This verifies
continued activity after the recovery rather than merely a successful launch;
it does not claim another live reset was redeemed.

Document the weekly-reset probe in the tool index and explain immutable package
admission, requested versus full coverage, failed-case attribution, handled
cancellation and incomplete guardian evidence beside its existing recovery guide.
The implementation and retained report from the preceding change supply the
verification; this documentation-only update needs no native rebuild or repeated
smoke run. This reduces the chance of a future reviewer accepting a subset or
interrupted run as full qualification. No production behavior changes.

## Bound backend recovery by every known natural reset deadline

While the sync waits for admitted resources, review finds the reset deadline only
bounded Recovery::Spent. HTTP errors and ordinary Wait results could hold the
original native request forever past a known reset. Add two adversarial unit
tests: a persistent usage503 and a60-second in-flight usage response, each with a
one-second natural deadline,60-second polling interval and3-second hang detector.
Both fail against the old loop exactly at the hang detector (exit101,3.97seconds),
not at fixture setup or identity checks.

In the separate owned worktree, bound both each45-second attempt and each polling
delay by the remaining original deadline. Expiry returns scheduling to the native
quota policy. Keep no-deadline polling and spent-grant behavior unchanged. Use
saturating arithmetic and checked signed conversion so extreme timestamps cannot
overflow an Instant or panic. Existing receipt handling remains the custody owner.
Add a pending-redemption deadline control that verifies released custody and the
same credit/key on later reconciliation, rather than abandoning an uncertain POST.

The two original regressions turn green, then all29 quota tests pass in46.24seconds;
quota all-target Tippy with --no-deps/-D warnings passes. Add two native smoke
cases for persistent usage/credit-list outages through a three-second wall reset,
requiring no credit redemption, a retained completed plan, no early model request,
and recovery before another30-second poll. Keep backend-observed natural recovery
as an independent paced case with a90-second original wall deadline. Its previous
10-second deadline would now correctly hand off to native scheduling before the
second30-second backend poll. All24 smoke-tool tests pass, including the new oracle
that rejects early model retries and keeps the outage active after reset. Native
CLI qualification of these new cases is still pending, not claimed by unit tests.
The in-flight sync remains frozen on its earlier candidate throughout this work.

The deadline-enabled debug CLI builds successfully with Trust's explicit unverified
lane. Its SHA256 is4c9b8fafe37f1a4c74bc1dd51224ffafd12902c3ee96f240c81b2720987f9e4d.
All four targeted native cases pass: persistent usage outage, persistent credit-list
outage, spent retry deadline and independently observed natural restoration. Each
retains the original completed plan and finishes the original turn; the outage
cases spend no credits and retry only after the quota reset, within20seconds of
the original429. Preserve target/deadline-native-controls. The full27-case suite
is running separately; the four-case result explicitly reports fullSuite=false.
The six smoke fixture/report tests pass again after the final timing assertion.

The third sync candidate passes all1838 ALab tests and lint, but its owned exec
quota-observation integration test fails. Concurrent main already supplies the
missing authenticated wait observation plus eight shutdown assertions. Stop only
the two verified controllers of this obsolete gate with SIGTERM; preserve its
failed/incomplete evidence rather than claiming a pass or killing unrelated work.
Preserve refs/alab/superseded-sync/92a34ffd86, normalize its unpublished merge
subject with an identical tree, and merge1ad8b4f8ec into the idle candidate. The
native continuation creates5093be823c and runs the complete gate from scratch,
with single-thread tests and the qualified real native producer. Its early lint
phase passes with zero ALab warnings; build and test qualification remains active.
Remote main continues advancing independently; retain native race recovery and
never force a push or treat this earlier candidate as validation of later changes.

The complete27-case native suite now passes against that same debug executable,
with fullSuite=true, all requested cases present and no guardian timeout. Preserve
target/deadline-native-full alongside the targeted run; the probe SHA256 is
996fd2f8ec266eee1905d2faa4ab9d8701e104c58a5e9e72922645c1605d8239.
The targeted outage retries take3.737 and3.655seconds, while independent backend
restoration takes30.123seconds. Commit this bounded-deadline correction separately
so later review can distinguish runtime behavior from sync maintenance. This is
source/native qualification, not a claim that a newly packaged executable has
been selected or that the concurrent sync gate has completed.

## Bound smoke evidence hashing memory

After the deadline fix lands asfaf104c113 through guarded Agh publication, the
native updater builds that exact source while the independent sync gate continues.
A next-issue read is refused403 with issues=read missing and no side effect;
record it without retrying or requesting routine permission. Local review instead
finds the native smoke runner loads its entire executable just to compute SHA256.
The tested debug binary is1,041,865,176bytes. Hash the executable and probe in1MiB
chunks, preserving SHA256 evidence with memory bounded independently of file size.
The six report/fixture tests pass, and a streaming hash of that actual binary
matches the digest recorded by both successful native probes. No reset behavior,
case assertion or evidence field changes. This smoke-tool-only improvement does
not alter the immutable runtime source currently being packaged.

Measure the actual streaming hash with macOS time-l:0.81seconds real and
28,557,312bytes maximum resident set (24,166,808bytes peak memory footprint),
including Python and imported smoke dependencies, hashing the same1.04GB binary.
The digest still matches. This is a measured streaming-process footprint; no
separate old-process memory baseline was run under the live build load.

The independent5093 sync gate now trails landed current main, so its expensive
qualification would need repeating after integration. It holds15.9GiB admission
while the actual reset package has waited nine minutes. Prioritize shipping the
user's fix: preserve the gate log and scratch, cancel only the verified native
project front and its test-admission front with SIGTERM, and wait for native group
settlement and the original command's exit. All four tracked controller/compiler
PIDs disappear; the gate records143 and its driver exits1. This incomplete gate
is deferred, never a pass. The released resources immediately admit the real
reset package. Resume sync later against combined current main with its admitted
Agh publisher, rather than repeatedly qualifying superseded intermediate trees.

The guarded native updater completes successfully and selects runtime package
0.2.0-dev.15257+codex.0.167.0.gfaf104c11 for new launches in the existing staged
output. Its immutable source isfaf104c11323e30daf127ecda02716d08e25a519,
native-admitted Trust15698/compiler010b4e686f3b1ea7. Binary SHA256:
e2fa26244ea65cfb60e1afac5436966ca2e6bb1ddc5052b06422123b1309f18a;
Mach-O UUID19DFE9BD-1151-3C1C-AAB0-2DFE0F0CFADD. Packaging's contained build,
smoke and guarded selection succeeded; existing-session adoption is still pending.
Start the complete27-case native suite against this exact package, separately from
the successful debug-binary suite. With updater custody released, the independent
smoke-memory improvement can now land without racing that source check.

## Exact selected runtime qualification

All27 native reset cases now pass against the selected15257 release package,
fullSuite=true, immutable sourcefaf104c11323e30daf127ecda02716d08e25a519 and
binary SHA256e2fa26244ea65cfb60e1afac5436966ca2e6bb1ddc5052b06422123b1309f18a.
Probe SHA25659ad0b0ab38041328afa48bc5480fc1b928a8d93042c236006b7e9606d2e7ebb;
preserve target/deadline-package-full. The original command exits0, every requested
case is present and no guardian times out. No live account credits are used.

Prepare safe runtime adoption only after all own qualification processes finish.
The original Trust tab retains its exact saved compiler goal under Cruise, hold0,
input clear and proven supervisor7506. Its native job telemetry currently shows
active Python compiler work57266, so defer peer restart rather than interrupting
that work. Own supervisor28304 has no remaining qualification command; save the
handoff, restart through the native validated route, and verify actual loaded
Mach-O UUID against19DFE9BD-1151-3C1C-AAB0-2DFE0F0CFADD. Selected package state
alone is not proof of adoption. The older sync remains explicitly deferred and
incomplete, to be integrated and requalified separately under continuous work.

Native self restart succeeds after all qualification and publication commands
finish. The resumed session reports proven supervisor84650, Cruise, hold0 and
current integration. A fresh process sample proves its actual loaded image UUID
is19DFE9BD-1151-3C1C-AAB0-2DFE0F0CFADD, exactly matching qualified runtime15257;
preserve target/deadline-adopted-self.sample. This verifies adoption independently
of the selected link. The Trust peer continues real compiler work with fresh
native jobs8482/9274, so keep its safe upgrade deferred. Its saved goal and work
remain intact. Native sync status confirms its old candidate5093 is inactive,
committed and unlanded; resume by merging current main and qualifying that combined
candidate rather than reusing any earlier incomplete verdict.

## Preserve a superseded sync checkpoint

Current main already contains the provider-write intake from upstream 053e5b1ca9.
The idle a95adec6d7 checkpoint is superseded. Its only unique non-merge commit,
311d58b148, changes a timestamp closure already independently corrected on main
and appends historical decision notes. Retain its complete journal addition below
and preserve the full checkpoint under refs/alab/superseded-sync/a95adec6d7 before
retiring it. These are historical observations, not pending work instructions.

<details><summary>Earlier sync decisions retained from 311d58b148</summary>


## Retire a superseded sync and resolve the next upstream export conflict

The next maintenance task finds the shared sync checkpoint stopped at exit4 for
419hours. Its target90894a780d59 and first parent are already ancestors of current
origin/main; no unique non-merge commit remains. Its remerge diff only removes
MODULE.bazel.lock and the obsolete guardian-v3 BUILD.bazel, both absent on main.
The checkpoint is clean and inactive. Preserve8250b2586387aeffb4e7d0977325f47e07ee605d
at refs/alab/superseded-sync/8250b25863, then use supported `sync.sh --abort` to
retire this obsolete obstruction without discarding unique source work.

A fresh stepwise sync reaches upstream053e5b1ca914 (provider configuration writes).
Only the stable and experimental compressed export archives conflict. Decode all
three index stages, reject any overlapping record edits, and merge by exported
record. Preserve ALab's InternalSessionSource.ts and stable RolloutLine.json;
retain every upstream provider-configuration addition. Independently require the
stable archive's TypeScript and JSON maps to equal the merged on-disk fixtures,
then round-trip level19 zstd output. Resolution evidence and saved input stages
are under `codex-rs/target/sync-053e5b1ca9-exports` in the owned recovery worktree.
The sync continuation commits ba0968837a but the gate refuses before admission:
no qualified atpkg-publish executable was supplied. This is not a passing gate.
Rebuild the same native producer from qualified aterm source798fd5fd8f in the
owned s113 worktree, keeping the required astream pin and explicit unverified
lane. A prematurely started build before checkout completion fails on missing
files; wait for the checkout and submodule to finish before retrying. No installed
aterm image is overwritten and no admission guard is bypassed.

The native producer succeeds after1m47s. With its explicit path, the gate admits
pinned Trust15698, builds/runs codex-cli, then executes all ALab suites. It records
1837 passing tests, four ignored, and one failure across25 completed harnesses:
login_back::a_change_that_moves_no_stat_reloads_nothing times out waiting for a
permissions-only native folder event. The same compiled test passes alone in
15.18seconds outside the gate; that diagnostic is not contained gate qualification
and cannot excuse the failed aggregate result. Preserve gate-first.log. No test
is removed, ignored or relaxed. Retry the full supported gate with the documented
RUST_TEST_THREADS=1 control to test concurrency sensitivity while retaining every
assertion. The first gate's nested-lane temp-file handshake also reports sandbox
EPERM; its outer explicit command remains authorized, and no refusal is bypassed.

The full single-thread test phase passes all1838 tests across25 harnesses with
the same four existing ignores, including the previously failing native watcher
case and all26 quota tests. Recompute both archive hashes against resolution.json
to confirm the tested candidate is unchanged. The gate proceeds to lint; this
test-phase success alone is not a complete gate or permission to land. Native
memory telemetry also reports an outside-profile write refusal, left intact.
Read upstream PR58876 through agh: configuration writes preserve authentication
and selection, retain omitted fields and distinguish explicit null overrides.
The reviewed archive merge keeps that upstream API intent and ALab's types.

The serial gate completes all ALab tests but correctly refuses lint: the reset
deadline glue at responses_retry.rs608 uses a redundant timestamp closure. The
quota-crate-only lint check did not cover this core-compiled glue. Replace it with
the direct chrono::DateTime::timestamp reference, preserving behavior and leaving
the lint enabled. This is an ALab-owned one-line correction, independent of the
upstream export resolution. Preserve the failed serial log; its test success is
not a successful complete gate. Commit the correction in the owned branch, merge
that branch into the pending sync, and qualify the combined candidate before
landing either change on remote main.

</details>

The later combined check passed all 1,841 ALab tests across 25 harnesses, with
four existing ignores, and passed lint with no ALab warning. Its full hooked-library
run exposed an owned child-completion race and was stopped to prioritize the fix;
it was not a successful complete gate. Both compressed archives subsequently
passed all four canonical protocol fixture checks, including experimental parity.
The ignored generator was not run and no fixtures were rewritten.

## Wait for native child completion before idle capture

The broader app-server run exposed an owned race: a review result completed while
its source tree was frozen, but capture failed with "idle input sealing found an
active or suspended turn". Native finalization removes RunningTask before it
clears ActiveTurn. Returning None during that interval falsely classified the
child as idle. Keep the idle validation strict and wait for native completion.

An initial regression fixture held session state and failed before reaching that
transition; preserve it as a fixture failure, not proof of the bug. Replace it
with a real turn-stop lifecycle gate. The native task reaches its stop callback
with no RunningTask but an active finishing record. The regression then fails
against the old implementation exactly at premature-completion assertion (one
test, exit 101, 4.84 seconds), not at setup or a missing test filter.

Preparation now retains the original turn-state identity and native done signal.
Register the notification before inspecting active state to avoid a lost wake;
wait outside the active-turn mutex while that original task finishes. Reject a
changed state or context and keep ordinary selected-task suspension unchanged.
This adds no sleep, arbitrary timeout, weakened idle check, or upstream hook.

With this correction, all six child-suspension tests pass, including cancellation
and retry custody, foreign barriers, mismatched history, and the new regression.
All nine real app-server recovery-tree scenarios also pass in 21.92 seconds,
including completed-while-frozen review delivery, ordinary recovery, refusal,
ephemeral history, deadline cancellation, and embedded bootstrap. These use the
native source-selected Trust 15698 in the explicit unverified lane and make no
formal proof claim. Preserve native-child-completion-red-lifecycle.log,
native-child-completion-green-group.log and native-child-completion-review-tree.log
under the owned target directory. Combined-current-main lint and qualification
remain pending; this child-lifecycle correction is not yet deployed.

Current-main integration replays these owned commits cleanly onto b79d2d3c59,
preserving the independently landed provider-write intake and Codex banner work.
After preserving the checkpoint ref and unique journal addition, native --abort
retires the inactive superseded checkpoint and reports sync idle. Main's two
archive files differ at the compressed-byte level, but their complete decoded
export payloads are identical to the four-test-qualified a95 versions; protocol
sources and checked-in TypeScript/JSON fixtures also have no difference. Preserve
those scoped verification results rather than pretending an interrupted full gate
passed. The monochrome performance prototype remains separate for measurement.

## Select test depth before rendering the banner matrix

Full qualification showed the legacy banner tests spending substantial CPU on
an exhaustive matrix. The monochrome test rendered all 57,840 combinations, then
discarded the three color depths by matching a string in the diagnostic label.
Factor the existing streaming generator through a typed depth slice. Its full
matrix entry still uses every depth; the monochrome test selects Depth::Mono
before rendering, retaining all 14,460 monochrome cases and every original
assertion. Avoid a large cached corpus or reducing the tested configurations.

Measured against current-main banner code from b79d2d3c59 with the same pinned
Trust 15698, test profile and single-worker filter: the unchanged monochrome test
passes in 92.75 seconds; typed selection passes in 0.76 seconds. These are test
runtimes on this shared machine, excluding compilation. Broader banner coverage
then passes all 21 tests in 89.48 seconds with four workers, including the other
full-matrix invariants and the newly landed Codex opening card. Keep that independent
banner work and its formatting unchanged. No runtime rendering code changes.

Preserve banner-mono-baseline.log, banner-mono-selected-depth.log, and
banner-selected-depth-all.log under the owned target directory. The current-main
feature inventory test and all three upstream-status tests also pass. The next
check is native qualification of the combined committed layer, using the documented
four-worker default rather than carrying the earlier serial diagnostic into every
upstream library. Any observed failure still requires proper attribution or a fix.

## Reuse the banner's negotiated palette helpers

Combined qualification stopped at three disallowed color constructors in the
newly landed Codex card. Reuse the renderer's existing narrowly allowed helpers,
exposing them only to sibling banner modules. Keep the exact indexed/RGB values
and theme/depth choices; do not weaken the lint policy. This is a separate small
repair to make the combined layer qualify. The newly integrated proxy-home
isolation fixtures pass both tests. Intake fixtures require Python with tomllib;
the system Python 3.9 cannot import them, so use the installed Python 3.14.

## Combined qualification and resource deferral

On ae124be20f the native Trust15698 gate passes lint with zero ALab diagnostics
(40 upstream diagnostics set aside) and builds/runs codex-cli0.0.0. The subsequent
ALab test compilation is stopped by the native disk guardian at its20GiB free
floor, exit75. This is an incomplete gate, never a pass. Preserve the floor and
source; native packageGC finds nothing reclaimable and a bounded native64GiB
free-space wait times out. Defer the broad run until capacity returns.

The affected behavior already has a red-before/green-after regression, all six
child-suspension tests and all nine public recovery-tree tests passing. All21
banner tests pass, with the same14460Mono cases taking0.76sec rather than92.75sec.
The separate palette repair preserves exact colorvalues and passes the combined
lint. These checks support landing the reviewed small changes while retaining the
resource-blocked broad qualification as open work. No runtime package is selected
for this follow-up yet; both sessions retain the independently qualified15257
reset-recovery package.

Broader Python3.14 discovery runs490 tests in745.106sec with seven skips and
three setUpClass errors, all requiring an explicitly built nativealab-unit-runner
artifact. There are no assertion failures. Rerun check_lint_step, unit_results,
and unit_runner_native with that actual artifact when available. Targeted parent
proxy-home fixtures pass2tests; upstream-intake fixtures pass6tests. Preserve
child-completion-python-suite.log and child-completion-combined-gate-palette-retry.log.

## Recover tooling coverage without rebuilding an unchanged runner

Locate the actual native adapter retained from the previous qualification, prove
its crate, workspace manifest and lockfile unchanged, and copy it with a SHA256
receipt into owned evidence storage. All 17 result-attribution tests pass. Split
the interrupted 38-test lint class into disjoint named batches: 10 pass in128.982s,
10 in111.385s, 10 in245.856s, and8 in135.386s. These55 completed tests resolve two
setup prerequisites; keep the native-context compiler fixture separately pending.

Capacity returned to396GiB free. Native admission correctly refuses old cache
receipts whose stored device identity16777234 differs from current16777231.
Keep those records unchanged. An explicitly fresh target is admitted, but its
saved pristine cache has the same stale identity; use a fresh qualification
worktree at immutable460073cbc0 so both caches get new native custody. Compilation
has started there. This preserves the authority boundary and avoids repeatedly
retrying invalid cache identities. See the concise review for current scope.

## Preserve native fixture context through an argument-list prefix

The remaining native context fixture initially fails because using the shell
project launcher as Cargo changes the inherited signal mask before Cargo starts.
Its direct and adapter entries are equal; do not alter the adapter or weaken the
mask assertion. Add a JSON argument-list command option to this ALab test driver,
allowing direct invocation of the native admission front end and selected Cargo
without shell startup. Retain the original single-executable option.

All three native-context tests then pass in4.672sec through the exact admitted
Trust15698 tools. The Python-startup negative control still detects altered
signals; cfg-runner conflict and missing-receipt controls also pass. Preserve both
invocations and their native entry receipts. This closes the final Python setup
deferral, completing the38+17+3 component runs. The combined gate remains separate
on immutable460073cbc0 in the new qualification worktree; no runtime source changed.

## Bound each native matrix case independently

The fresh combined run passes lint and binary startup but finds three ALab
unit failures. The goal-mutation matrix completes eight cases, then its shared
60-second deadline expires during the ninth fixture's database setup. Its phase
record accounts for46.84seconds in database/manager setup and8.47seconds in fixture
drop; the tested mutations themselves complete. The16-case upgrade matrix has
one analogous deadline over all independent native fixtures. Keep60seconds as
the hang detector for each case, with its case name in a failure; do not treat
cumulative setup across9or16 fixtures as a product latency budget. Preserve all
variants, assertions and native lifecycle cleanup.

The third failure waits for a cleared foreign claim to become active. It passes
alone from the exact same native binary under the maintained sandbox in0.33sec.
Keep its60-second bound and all safety assertions, adding the supervisor journal
and fake requests to a failure so a repeat under parallel load is diagnosable.
Do not claim a runtime fix or waive this full-run failure. Recheck the affected
cases and complete qualification after the candidate is built.

## Finish the requested delivery; retain the wider audit as incomplete

Native qualification of9101bfdcdd passes lint/CLI startup and all25ALab harnesses:
1843pass0fail4existingignores. All3oldfailingautopilotcasespass in868/0/2parallel
library run. Hooked upstream test executables compile, but execution queues for
16GiBbehindhigherprioritynative reservations. Respect that scheduling boundary.
Terminate only verifiedowned controller35298 and admissionfront26184, preserving
native group settlement and receipts. The controller alone first leaves descendants
alive; capacity returns during draining and partial upstream tests start. Stop the
verifiedowned front too. Partial results are not a fullgate pass or a pristine
comparison. No other session's work is stopped and no resource policy is bypassed.

The original request is already delivered: qualified15257reset runtime,29quota
library cases and27exact-release scenarios; restoredfirstTrusttab9849stillworks
its original compiler goal. Fresh loaded-image sample matches19DFE9BD-1151-3C1C-
AAB0-2DFE0F0CFADD; native selectedpackage remainsfaf104c11. Preserve that working
runtime. Allfollow-upsource isonmain (includingcaseguards910andmergeD20), with
38+17+3toolingfixtures also green. A separate rollout of subsequent lifecycle and
another owner's banner work belongs to the normal qualified release pipeline;
it is not necessary to redeliver the already installed reset repair. Record the
optional upstream-wide audit's deferral and finish with clean,publishedsource
and no owned checks running. Currentnative autopilot isLand, so start no unrelated
new task. Detailed scope and evidence are in the concise review.

## Land cleanup under the owner's preservation direction

Delete the fully merged fix/continuous-earned-resets branch after detaching its
clean worktree. Both owned Codex worktrees and their generated pristine checkouts
remain under this clone's explicit October23owner hold in.git/alab-preserve-builds,
which preserves builds, packages and worktrees until the owner changes it. The
routine automated Land prompt does not revoke a person's hold. Record this cleanup
deferral; both detached source commits are reachable from remote main and neither
contains uncommitted source. No source work is parked on a branch.

The independently owned Aterm producer worktree has no such marker, clean source,
and its798fd5fd8commit is already merged. Retain its target by same-volume rename
to ${HOME}/codex-reset-delivery-evidence-20261110/atpkg-publish-build, confirm
producer SHA25686f4389824a81d9e688c040e37d830c7e81c3fa4ade4f1f2d0116a259082aaba
before/after and a successful --help, then remove the worktree through Git with
its required submodule-aware --force. No submodule deinit or shared config change.
Git removal completes successfully; its slow unlink phase is sampled and retained.
The artifact/provenance files live outside Git and are verified in place. Do not
claim the moved target as a newly admitted compiler cache. Native checks are all
settled; no other session's files or jobs are removed.

## Complete the stopped late-steer upstream intake

The Land audit identifies an inactive sync of upstream ba0d0e1f74 with three
staged upstream paths and one conflict. Preserve upstream's mutable task result
and keep ALab's existing run_with_exec_launch hook. Native sync commits that merge
and merges concurrent remote main. Upstream now returns RegularTaskOutput so a
late steer retains MCP startup requirements. Adapt the three synthetic ALab tasks
through a test-only constructor in the existing task-launch extension module;
keep their original completion messages and all recovery assertions. No upstream
visibility change, discarded completion or legacy string conversion shim.

The first lock resolver's global shim cannot read the pinned install-tree record
inside its profile. Native installation confirms15698is present beside current,
which stays unchanged. Run the resolver through authenticated native selection
and its existing ALAB_CARGO setting. Remove that override for the subsequent gate,
which correctly refuses compiler-selection overrides. Do not relax its sandbox,
compiler admission, source checks or mandatory sync gate. Continue the maintained
sync on the committed merge and this separate ALab adaptation.

The native gate catches the third synthetic completion in exec_review_restore_tests
returning a String where RegularTaskOutput is required. Adapt it through the same
constructor, preserving its completed-before-attachment payload and assertions.

The first complete gate on dd09d82c1d passes all 1,844 ALab tests and reports no
ALab lint findings, but fails closed before upstream attribution: 929 TUI cases
fail, predominantly snapshot reads naming the earlier qualification worktree.
The reused cross-worktree target has preserved compiled source paths; this run
is not qualification and its failures must not be waived or its attribution
limit raised. Retain its log and native scratch. Choose a new, sync-owned target
below codex-rs/target/land-ba0-20261110 and let native admission register it,
without copying artifacts or editing cache identity records. Preserve the old
targets under the owner's existing retention hold. Merge remote main's concurrent
banner commits before repeating the unchanged mandatory sync gate.

The native controller intentionally excludes global Git configuration, so retain
the already-used author/committer identity explicitly for the concurrent merge.
The resolver profile refuses the helper from the main checkout; use the same
maintained helper in this admitted sync checkout instead. Authenticated15698
selection resolves the merge and commits f7becc71f0. The following gate correctly
refuses the resolver-only ALAB_CARGO setting; clear it before the full fresh-target
gate. No refused operation is treated as a pass or bypassed.

## Close the qualified sync against newer main

The native fresh-target gate on 7d0c0f4330 completes qualification: zero ALab
lint findings, 1,844 ALab tests pass (four existing ignores), and 6,203 upstream
TUI tests pass (five existing ignores) with no snapshot-read permission warnings.
One shared-run notification failure passes alone. All seventeen persistent
upstream failures reproduce alone on pristine ba0d0e1f74; none gates the layer.
The guarded push then refuses because main has advanced while the gate ran.
This is a publication refusal after qualification, not a failed or skipped gate.

Fresh Agh fetch confirms main at 27f8f15e06 contains newer upstream 053e5b1ca9
and an equivalent native completion constructor in exec_task_test_output.rs.
Its three recovery fixtures already use that constructor with unchanged original
messages and default MCP requirements. Retire the older constructor in this
branch: merge its entire six-commit history into current main while retaining
main's source tree exactly and adding only this review record. This also keeps
main's newer upstream merge on the first-parent history and preserves its portable
evidence paths. Verify that the only tree difference from that main is this
Markdown record, that both parents are reachable, and that whitespace checks pass.
There is no new compiler-input change to qualify in this closure merge.

Retain the complete gate log and old/fresh native targets under the existing
owner preservation hold. After the guarded closure push, retire the now-superseded
sync state through its maintained abort operation only after confirming every
local sync commit is reachable from remote main. No source or build evidence is
discarded. Original reset behavior is already delivered; start no new work.

## Yield a creditless weekly wall to account rotation (2026-11-11)

The retained sampler above starved account rotation in the TUI. Recovery order
is an earned reset first and then another account (`alab/design/accounts.md`),
but the second step acts only on an ended turn: the Autopilot's `follow_renewal`
needs the native turn Complete with a usage wall as its point, and aterm's
`codex-rotation prepare` refuses anything but an ended seven-day wall. With a
real exhausted account the usage read names the account, so recovery found the
wall eligible, found no credit, and polled every 30 seconds until the natural
reset. The turn never ended, silently, and rotation was never asked. The
rotation smoke kept passing only because its usage reply named no account, which
made recovery stop at once; it was last qualified on 2026-11-03/04, before the
loop existed.

Decision: when the weekly allowance is spent, nothing is in redemption custody
and no earned credit can be redeemed, recovery asks whether a host would move
the conversation to another account if the turn ended now. If so, the original
request ends on its original usage error and rotation follows; otherwise the
30-second poll continues unchanged. The question is a capability in the turn's
store, `codex_alab_core::quota::WallHandoff`, as `HeadlessExec` and
`InFlightRetryObserver` are: the Autopilot installs it at `on_turn_start`, and
quota recovery reads it without knowing the session record or aterm. Asking for
rotation from inside the wait was rejected: both the Autopilot and the host
require an ended wall, so it would need host and record changes. No upstream
file or hook changes.

- `Recovery::NoCredit` is returned only at the point where `available()` finds
  nothing, which is reached only with no consumed credit awaiting propagation
  and no pending operation. A credit in custody is never abandoned.
- Only the 10080-minute window at 100% with its reset ahead yields. The host
  answers `not-needed` for anything else, and ending a five-hour turn would
  give up in-turn late-credit polling for nothing. (Superseded 2026-11-12 for
  a host that says it leaves five-hour walls; see "Yield a creditless
  five-hour wall the host would leave" below.)
- A credit list that was read and offers nothing shows there is no credit, and
  yields at once. One that could not be read, or whose credit the backend
  refused in the same attempt, shows nothing either way: the request is kept
  for one more poll and yields only when the next attempt is inconclusive too.
  As first written a single failed read counted as no credit, on the ground
  that an outage must not hold a spent week away from another account; review
  (2026-11-12) found that this let rotation precede an earned reset that
  exists, for example on one 503 from the list. Two consecutive inconclusive
  attempts keep the outage bound (one 30-second poll) without that.
- The answer is advisory, bounded to five seconds and to the known reset, and
  asked again on each poll, so a policy turned on or an account added during
  the wait is picked up. It is true only for the acting primary thread of a
  first-party ChatGPT login whose freshly read host policy allows weekly
  rotation (`codex_rotation=1`, `accounts=early|wall`, `relaunch`,
  `resume_limits`), with no upgrade under way, and whose host shows another
  account to move to (below). That policy read is one function shared with
  `follow_renewal`, which checks everything again after the turn ends. No
  credit is spent and no account is selected by the answer. A wrong yes is not
  cheap: the turn has ended, the host answers `waiting`, and nothing lists
  credits again until the session next samples.
- The handoff holds weak references, so a turn's store keeps neither the
  Autopilot nor its thread alive.
- Exec is unchanged: a turn with `HeadlessExec` never consults the handoff.
  Compaction walls inherit the yield through the same glue function.
- `recover_weekly` takes the wall's named reset and its handoff as one
  `codex_alab_quota_recovery::Wall` value; no hook line in an upstream file
  changes, because the glue function's own signature is the same.

Policy is not evidence of somewhere to go (review, 2026-11-12). As first
written the answer came from host policy alone, and this section called the
loss "that one case" of an exhausted pool. That understated it: every host-side
refusal ended the turn, including the configuration on the Mac this was written
on (`codex_rotation=1 accounts=early`, one enrolled Codex account). There the
first empty credit read ended the turn, `codex-rotation prepare` had no target
and answered `waiting`, the Autopilot re-asked every five seconds, and a credit
granted during the wall was never redeemed, where the sampler on `main` would
have redeemed it within 30 seconds.

Decision: the answer also requires the host's own account pool to show another
account. The Autopilot reads aterm's `codex-accounts snapshot` (read-only, no
claim, selection or credential; already used by `/accounts`) on the claim's
connection, and `Snapshot::another_account_can_run` counts the rows a session
could move to by the host's cached record: login enrolled here, not held for
another machine, condition `usable` or `unseen`. The host's own target rule
refuses a `low`, `limited`, `signed-out` or held row, so those are not
counted. aterm names no row as the session's own (`assignment.account_id` is
always null today), so one able row may be the walled account itself and two
are required; where a host does name it, that row is excluded and one other
is enough. A host that does not serve the verb, answers for another process,
or does not answer inside the five-second bound shows no pool, and the request
keeps polling. Re-arming the credit poll from the Autopilot after `prepare`
answers `waiting` or `not-needed` was the alternative; it needs earned-reset
recovery outside a turn (a new entry into redemption custody and a recovered
turn that must not yield again), which is more than a review correction
should add without the native smokes.

What the pool evidence does not cover, each still ending the turn with no
credit poll until the session next samples:

- Refusals the snapshot cannot show: another row's pin, a keyring-held login,
  a mode mismatch, this session's login being no row of the pool
  (`source-unmanaged`, to which the host answers `not-needed`), and a candidate
  the record calls able but whose fresh read refuses it.
- The record being behind: an account that walled on another Mac or outside
  aterm still reads `usable` or `unseen` here.

And one case where it now errs the other way: when the host's record already
shows this session's own row `limited` or `low` and exactly one other row is
able, two able rows are not on record, the answer is no, and the request polls
as on `main` until the record changes. Closing it needs the host to name
the session's row in the snapshot. As read from aterm's source on 2026-11-12,
not measured, its record of a Codex row comes from its own rotation reads (the
usage tap is Claude Code's), so a session's own row is on record this way only
after an earlier rotation request at a wall there.

Two things remain unfixed from the original change: with no rotation host the
interactive wait still shows no notice, and the live backend's usage reply is
assumed, not observed, to name the account and user the token claims carry
(earned-reset redemption cannot work without that either).

Deviations from the reviewed design, each narrowing it: the handoff also
requires the thread to be registered and not in the vanilla experience, the two
gates `evaluate` applies before `follow_renewal`; and the question is bounded by
the time left to the known reset, so it cannot hold the request past it.

Unit coverage (`alab-quota-recovery/src/handoff_tests.rs`): a creditless weekly
wall yields to a ready handoff and, as the negative control, keeps polling
without one; an available credit is redeemed with the handoff never asked; a
five-hour wall keeps polling (superseded 2026-11-12, as above); an unready
handoff leaves the request to redeem a
late credit; a credit list that fails twice in a row yields and one that fails
once does not; a credit behind one failed list read is redeemed with the
handoff never asked; a credit refused in the attempt that tried it does not
yield in that attempt; a consumed credit awaiting propagation is never
abandoned, with the credit list empty after consumption so that custody alone
decides. `alab-core` bounds a silent probe, and
`alab-autopilot/src/renewal_follow_handoff_tests.rs` starts a real turn against
the fake aterm and reads its store's handoff across each policy key, a standby,
an upgrade request, a non-primary root, a dropped thread and no claim. Removing
the policy read from the answer fails that test at `codex_rotation=0`. Since
2026-11-12 the fake aterm serves `codex-accounts snapshot` from a settable pool
and the same test reads the handoff over no pool, an empty pool, one account,
a second account that is `limited`, `low`, `signed-out` or `not-enrolled`, a
snapshot bound to another process, and a named own row with and without
another able account; each refusal follows a ready answer with only the pool
changed. Removing the pool read from the answer fails it at the first of
those. `alab-aterm` pins the counting rule itself
(`accounts_pool_shows_another_account_only_beyond_the_sessions_own`).

Run on 2026-11-11 with the Trust toolchain in the unverified lane:
`targo --unverified test -p codex-alab-quota-recovery` (36 pass),
`-p codex-alab-core quota` (9 pass), `-p codex-alab-autopilot --lib
renewal_follow` (6 pass, the two new tests included) and `-p codex-core --lib
weekly_reset_retries_original_local_and_remote_compaction` (1 pass). `targo
tippy --all-targets` on `codex-alab-core`, `codex-alab-quota-recovery` and
`codex-alab-autopilot` reports nothing in the changed files. The native smokes
were not run: both need a package built from this source, and
`alab/smoke/renewal_follow.py` also needs the `native_host` fixture, of which no
build exists on this machine. The rotation fixture is corrected separately so
that it exercises this order; until it is run against such a package, the
restart handoff itself is unqualified, and so is the unchanged exec poll in
`weekly_reset.py` (`no-credit`, `late-credit`).

## Ask for an account handoff only until the wall's own reset (2026-11-12)

After a turn ends at a usage wall the Autopilot's `follow_renewal` asks the
host to move the conversation (`codex-rotation prepare`) and owns the point
while it does. `not-needed` releases the point to the wall's ordinary resume.
Every other answer (`waiting`, an `ERR`, a malformed or missing reply) was
asked again every five seconds with no end. The design for this change read
aterm's source (it was not read again or measured here) and found that aterm
answers `waiting` without reading the account in several standing cases: no
destination, another row pinned, a keyring-held login, Codex accounts mode off.
A session in one of them stayed ended past the reset
of the very wall it stood at, when the account it is on could run again and a
`not-needed` would have resumed it. That was tolerable for a weekly wall only
because the yield above requires another account in the host's pool; it is not
acceptable for a five-hour wall, which the next section starts yielding.

Decision: a handoff that no receipt binds is asked for only until the wall's
own reset plus the reset grace (`Timing::reset_grace`, 60 s).
`rotation_ask_lapsed` in `alab-autopilot/src/renewal_follow.rs` is the whole
rule, and it is judged immediately before the broker call. When it holds the
host is not asked and the pass takes the path of a `not-needed` answer
unchanged: `quota_rotation_declined` is set for the accepted turn, the goal
witness is retired, the intent is discarded and `follow_renewal` returns false.
`evaluate` then decides the usage wall as it always has: the resume is typed at
reset plus grace plus this session's jitter when `resume_limits` and
`limit_wait` are on, and attention is raised otherwise. The lapse is judged on
`follow_renewal`'s five-second pass, so it can be up to five seconds late.

- A bound receipt is never abandoned. The host may have committed it. An
  unbound one is safe to drop: `commit` is sent only after binding, and by the
  same reading of aterm's source the host charges its pool only at `commit`.
- Host sleep is counted first. The Autopilot's clock is tokio's `Instant`,
  which stops while the host sleeps, and `evaluate` moved the wall's reset back
  by the time slept only after `follow_renewal` had let the point go. A Mac
  that slept four hours of a five-hour wall would have kept asking for four
  more waking hours. The block that counts sleep is now
  `Autopilot::count_host_sleep`, called by `evaluate` where the block stood and
  by `follow_renewal` before the lapse is judged. A second call on the same
  reading moves nothing (`ThreadState::slept_to` counts only growth since its
  mark), and a finished turn seeds that mark from the reading taken when the
  wall was classified, so the first count in `follow_renewal` already covers
  the time since the wall.

What this does not bound, each as before:

- A wall whose 429 names no reset: there is no moment to lapse at.
- An intent carried across a process restart. A usage wall is not restored
  from the rollout, so the new process holds the intent with no reset on
  record. (Corrected after review: an unbound intent now carries its wall
  across a restart. What is left is under "The wall of an asked handoff is
  carried across a restart" below.)
- A weekly wall in any useful sense: the lapse comes at the reset the 429
  named, which for a spent week is normally the weekly reset, when a
  `not-needed` would have resumed the session anyway. (Corrected after review:
  the 429 may name another reset. See "The lapse comes at the 429's reset"
  below.)
- The earlier `return true` paths of `follow_renewal` (goal reconciliation,
  unreadable history, a refused admission readiness, a failed write-through)
  and a goal-witness retirement that keeps failing. They sit before or after
  the lapse and still retry without a time limit.

The fake aterm gains `FakeAterm::reply_always` (and `reply_always_clear`): the
same raw reply to one exact command every time, after any `reply_once` for it.
`reply_once` answers once, and the fake answers an unscripted `codex-rotation`
with `ERR unknown verb`, so a host whose `waiting` stands could not be modelled.

Coverage. `alab-autopilot/src/renewal_follow.rs`:
`a_rotation_ask_lapses_only_past_the_walls_own_reset_with_no_receipt` (the
table: the reset and its grace reached, one tick earlier, a bound receipt, a
wall with no reset, three other walls, no wall) and
`a_reset_the_host_slept_through_lapses_the_ask_at_the_wake` (two hours of sleep
counted at a wall one hour from its reset lapses the ask; 55 minutes does not
until the grace passes). `alab-autopilot/tests/autopilot/rotation_decline.rs`
runs a real core thread on a scratch ChatGPT login against the fake aterm and a
loopback model: `not-needed` gives one ask and one `usage-resume@v1` at the
reset; a standing `waiting` gives the lapse, one resume and no ask once the
reset has passed; the same `waiting` at a wall an hour
from its reset is asked three times with nothing typed (the negative control);
a host that knows no `codex-rotation` verb lapses the same way; and a host that
sleeps two hours while such a wall is being asked about resumes at the wake,
with the wall's reset still an hour ahead by the wall clock. These tests
script the host's time asleep through `Autopilot::read_host_sleep_as`, a seam
compiled only into test and debug builds, as the login watch's counters are.
(How each reset is brought was changed after review; see "Review repairs"
below. As first written, three of them named a reset four or five seconds
ahead of the harness's start.)
`alab-aterm/src/rotation.rs` pins the fake's standing reply.

Mutations, each restored after its run: the receipt guard removed from
`rotation_ask_lapsed`, and `>=` weakened to `>`, each fail both unit tests;
the sleep count removed from `follow_renewal` fails the slept-through test;
the lapse ignored at the broker call fails the standing-`waiting` test.

Run on 2026-11-12 with the Trust toolchain in the unverified lane, from
`codex-rs/`: `targo --unverified test -p
codex-alab-autopilot --lib` (899 pass, 3 ignored), `-p codex-alab-autopilot
--test autopilot` (99 pass, the five new tests included), `-p codex-alab-aterm`
(162 pass, 2 ignored), and `targo tippy --all-targets` on `codex-alab-aterm`
and `codex-alab-autopilot` (nothing in ALab's files). Not run: any native
smoke, `alab/check.sh`.

## Yield a creditless five-hour wall the host would leave (2026-11-12)

The weekly yield above kept every five-hour wall polling, on the ground that
the host answers `not-needed` for anything but a spent week. aterm's rotation
is changing. On its rotation branch, as the design for this change read it
(aterm's source was not read again here), it also moves a session whose
five-hour window is spent on a usable week when that window's reset is more
than its `RESET_SOON_S` (900 s) away. A session at such a wall would otherwise
sit out up to five hours beside an account that can run. This section
supersedes two statements in the weekly section: the bullet "Only the
10080-minute window at 100% with its reset ahead yields", and "a five-hour
wall keeps polling" in its unit coverage.

Three ways to let the host have such a wall were considered.

- Ask aterm from inside the wait, before ending the turn. Rejected, as it was
  for the weekly wall: `codex-rotation prepare` needs the session record's
  owed marker and a rollout that proves an ended usage wall.
- End every creditless five-hour turn and rely on the bound in the previous
  section. Rejected: it ends turns that the host answers `not-needed` for,
  which gives up the same-request retry and the poll for a late credit for
  nothing. By the design's reading, aterm's main branch and the installed
  0.198.0 are such hosts.
- Chosen: a local mirror of the host's rule, switched on by a word the host
  prints, with the ask bounded by the wall's own reset (the previous section).

The word. While `codex_rotation` is on, aterm will print
`codex_rotation_five_hour=<seconds>` in its `harness policy` reply; the value
is the bound it applies (900 today), so codex-alab follows a change to it.
`codex_rotation=1` alone cannot serve: it is a build qualification, printed
by hosts that leave no five-hour wall. A host that prints no such word leaves
the five-hour yield off, so this change is inert until aterm ships the word.
It has not: `aterm ctl @self harness policy` on the installed aterm 0.198.0
(this Mac, 2026-11-12) prints `codex_rotation=1` and no
`codex_rotation_five_hour`. No aterm that prints the word was run against this
change.

The rule is `leaves_wall` in `alab-aterm/src/account_recovery.rs`, asked from
`quota_handoff_ready` with the wall quota recovery found
(`codex_alab_core::quota::SpentWall`: the window and the moment it resets, in
seconds since the epoch, as the usage read names it; corrected after review,
see "Review repairs" below). It requires the rotation policy as before, and
then:

- a spent week: nothing more;
- a spent five-hour window: `limit_wait` on, the word present as a canonical
  count of seconds, and the wall's reset further away than that bound plus 60
  seconds when the host's policy has been read and the question is answered.
  A missing, blank or unreadable word is no.

The 60 seconds are the skew between the answer and the host's own read after
the turn ends; a wall that clears the host's bound by less than the delay
between them is answered `not-needed` after the turn has ended. By the
design's reading of aterm's rule the mirror is otherwise narrower than it
(exact window lengths, `ordinary_usage_allowed` false, the codex bucket only),
so a yes implies the host's rule unless more than the skew passes. 60 s is a
judgment; the delay was not measured.

`limit_wait` is required because of what a declined turn does. With it off,
the Autopilot raises attention at a usage wall at once, where the request it
ended would have waited the wall out in the turn: a five-hour yield that the
host then declines would alert a person up to five hours early and lose the
chance of a late credit. With it on, a declined or lapsed five-hour turn is
always resumed by the Autopilot at the reset.

Two guards sit in quota recovery itself (`offerable` in
`alab-quota-recovery/src/unix.rs`), because what follows an ended turn runs on
the reset the 429 named, not on the usage read's: the Autopilot's usage wall,
its bound on asking the host, and its resume.

- A five-hour wall whose 429 names no reset is not offered. Nothing would bound
  the ask, and the host's standing `waiting` cases would leave the session
  ended.
- A five-hour wall whose 429 names a reset more than 60 seconds from the usage
  read's five-hour reset, in either direction, is not offered. A later one
  would hold the ended session and the bound to that later moment. An earlier
  one would resume the session into the same wall, typing a resume each time
  the 429's reset came round.

A spent week keeps the weekly yield's behaviour whatever the 429 named,
including an unbounded ask when it names no reset.

Which wall is named (`spent_wall`, replacing `weekly_exhausted`): the week is
judged first, as the host judges it, and a spent five-hour window is named only
where no weekly window is in the way: every weekly window the read carries
shows a finite figure under 100%, which a read that carries no weekly window
satisfies too. A weekly window that is spent with no reset ahead, or whose
figure is not a finite number, names no wall: the five-hour reset would not
end it. The other guards are unchanged (identity match, no explicit denial,
`ordinary_usage_allowed` false, the codex bucket, exact window length, a finite
figure of at least 100, a reset ahead).

The pool gate of the weekly yield applies unchanged: the host's
`codex-accounts` snapshot must show another account the session could move to.
The host's word is checked first, so a host that would not leave the wall is
not asked for its pool.

Costs, each accepted:

- `Recovery::NoCredit` is now returned at a creditless five-hour wall too, so
  each 30-second poll there asks the handoff probe, for up to five hours. As
  first landed every such poll made one `harness policy` request, including
  against a host without the word. Since the second review (below, "A host
  that prints no five-hour bound is not asked about one") a host whose last
  policy reply printed no bound is refused from that remembered reply, with no
  request; a host that prints the bound is still read on every poll.
- Where the host does print the word and its pool shows another account, a
  refusal the snapshot cannot show (another row pinned, a keyring-held login, a
  candidate whose fresh read refuses it) ends the turn, and the session then
  neither polls for a late credit nor notices one redeemed by another session
  on the same account, until the wall's own reset plus grace. That is the
  weekly yield's residual cost, now paid at five-hour walls and bounded by the
  previous section.
- Compaction walls inherit the yield through the same glue function.

Exec is unchanged here: a turn with `HeadlessExec` never consults the handoff
and keeps its request, asking aterm through its own protocol. The design for
this change found, reading aterm's source, that aterm's exec control keeps a
`not-needed` for the whole root turn, so a five-hour wall near its reset would
disable handoff at every later wall of that run; that needs a fix in aterm, not
here.

Not determined: which reset the live backend's 429 names when a five-hour
window is spent, and whether it equals the usage read's five-hour `reset_at`
within 60 seconds. If it does not, the agreement guard keeps every five-hour
wall polling and this yield never happens on a real account. The 429 also
carries `limit_window_minutes`, which this change does not use.

Departures from the reviewed design. The 429 agreement is two-sided (the design
refused only a later 429 reset); the reason is the second guard above. A spent
five-hour window is not named beside a week that is spent without a reset
ahead or unreadable (the design judged the week, then the five-hour window,
with no such case). `leaves_wall` is consulted before the pool read. The glue
test in `alab-core/glue/weekly_compaction_tests.rs` also had to change, since
the review commits that followed the design added a `WallHandoff` there. The
design's smoke case (`alab/smoke/renewal_follow.py`) is another change.

Coverage. `alab-quota-recovery/src/handoff_tests.rs`: a creditless five-hour
wall asks the host once about that window, with the moment it resets, when the
429's reset is the window's own or within 50 seconds either side, and ends the
request on a yes; a host that declines leaves it polling; a 429 with no reset,
a later reset (two minutes and 23 hours) and an earlier one (half an hour) are
never offered, with a spent week and no named reset still offered as the
positive control; both windows spent names the week; `spent_wall` and
`offerable` each have a table. `alab-aterm/src/account_recovery.rs`:
`a_five_hour_wall_is_left_only_under_the_hosts_advertised_bound` (no word, the
bound at 960 and 961 seconds, another bound, `limit_wait` unset or off,
unreadable words, each rotation key, a standby).
`alab-autopilot/src/renewal_follow_handoff_tests.rs`:
`a_five_hour_wall_hands_off_only_under_the_hosts_advertised_bound` reads a
started turn's handoff against the fake aterm across the same conditions and
the pool, each refusal beside a spent week the same host would leave.
`alab-core` tells the probe its wall. The glue test counts the host's answer
only when it is asked about a spent week.

Mutations, each restored after its run (sha256 compared): every five-hour wall
offered, the agreement made one-sided, and `offerable` dropped from the wait,
each fail the 429 tests named above; the usable-week rule dropped fails its
table; the week or the five-hour window never named fails the tests that need
it; in `leaves_wall`, `limit_wait` dropped, `>` weakened to `>=`, the skew
dropped, the rotation policy replaced by `effective` alone and the word parsed
loosely each fail its table; `leaves_wall` dropped from `quota_handoff_ready`
fails the Autopilot test; a probe told a week whatever the wall fails
`wall_handoff_probe_is_told_the_wall`.

Run on 2026-11-12 with the Trust toolchain in the unverified lane, from
`codex-rs/`: `targo --unverified test -p codex-alab-quota-recovery` (48 pass),
`-p codex-alab-core quota` (10 pass), `-p codex-alab-aterm` (163 pass, 2
ignored), `-p codex-alab-autopilot --lib` (900 pass, 3 ignored), `-p
codex-alab-autopilot --test autopilot` (99 pass), `-p codex-core --lib alab_`
with the gate's `RUST_MIN_STACK=8388608` (175 pass, 1 ignored), and `targo
tippy --all-targets` on the four `alab-*` crates touched (nothing in ALab's
files). Without that stack size the `codex-core` run aborts on a stack
overflow in a tree-capture test this change does not touch
(`ephemeral_restore_keeps_fresh_restrictions_on_inherited_authorization`); it
was not judged against the baseline commit. Not run: any native smoke,
`alab/check.sh`, anything against an aterm that prints the word, anything
against the live backend.

## Review repairs to the two sections above (2026-11-12)

A review of the two changes above found no blocker and six defects. Each
repair is its own commit; this section records what changed and why, and each
entry ends with what was run for it.

### The wall's reset is a moment, not a time left

`SpentWall` carried `resets_in`, the time to the window's reset measured when
the walled request read its usage, before the credit-list request of the same
attempt. That figure was then judged later, by `offerable` (against the time
left to the 429's reset, read from the clock again) and by `leaves_wall`
(against the host's bound), after the credit list (an attempt may run 45
seconds), the handoff probe's own policy read (up to 5 seconds), and before the
turn's end and the host's asynchronous read. A wall 965 seconds from its reset
at the usage read, with a credit list that answered after 40 seconds, cleared
900 + 60 seconds on paper and ended the turn; the host, reading with under 900
seconds left, would answer `not-needed`, and the session would sit about 15
minutes without its in-turn poll for a late credit. The 60 seconds of skew were
described as the gap to the host's read but were mostly spent before the host
was asked.

Decision: `SpentWall` carries `resets_at`, the moment the usage read names
(seconds since the epoch), and `SpentWall::resets_in(now)` measures the time
left when it is asked for. `leaves_wall` takes the moment it is asked;
`quota_handoff_ready` passes the clock read after the host's policy reply. The
60 seconds now cover only the turn's end and the host's own read. `offerable`
compares two moments (the 429's reset and the window's), reads no clock, and
its minute is exact; its test no longer allows two seconds of slack per side.
The relative figure is gone, not kept beside the new one.

Not changed: 60 seconds remains a judgment, and the delay between the answer
and the host's read is still not measured.

Coverage. `alab-core/src/quota.rs`:
`a_spent_walls_time_to_reset_is_measured_when_asked`.
`alab-aterm/src/account_recovery.rs`:
`a_five_hour_walls_bound_is_judged_when_the_host_is_asked` (a wall 965 seconds
from its reset is left when asked at once and four seconds later, and not five,
40 or more seconds later; a spent week whenever), and the existing table now
names the moment it is asked. `alab-quota-recovery/src/handoff_tests.rs`: the
host is told exactly the usage read's reset for a week, for a five-hour window
(whatever the 429 named within the minute) and for the week beside a spent
five-hour window; `offerable`'s table is exact at 60 and 61 seconds either
side. `alab-autopilot/src/renewal_follow_handoff_tests.rs` builds its walls
from the clock; the exact second of the bound is left to `leaves_wall`'s table,
since the handoff now reads the real clock.

Mutation, restored after its run (sha256 compared): the moment passed by
`quota_handoff_ready` replaced by zero fails
`a_five_hour_wall_hands_off_only_under_the_hosts_advertised_bound` ("ten
minutes from its reset hands off").

Run with the Trust toolchain in the unverified lane, from `codex-rs/`, on this
commit's tree: `targo --unverified test -p codex-alab-quota-recovery` (48
pass), `-p codex-alab-core quota` (11 pass), `-p codex-alab-aterm` (164 pass,
2 ignored), `-p codex-alab-autopilot --lib renewal_follow` (9 pass).

### The five-hour yield through a real account's usage reply

The only five-hour usage reply sent through the real backend client carried no
weekly window, a shape no real account was seen to report; a five-hour window
spent beside a usable week was tested only on hand-built snapshots. A change to
how the reply's secondary window decodes, or to the usable-week rule, could
have broken the five-hour yield for every real account with every test
passing. The latest reset among several spent windows of one length was not
tested either: `.max()` turned into `.min()` passed.

The wire fixture `creditless_five_hours` now answers with a spent five-hour
window beside a week 40% used, so every five-hour wire test goes through that
reply; the reply with no weekly window is one wire case of its own
(`a_five_hour_wall_on_a_reply_with_no_weekly_window_is_named_too`).
`the_latest_reset_among_spent_windows_of_one_length_is_the_walls` covers two
spent windows of one length in either slot, one of them already reset, and one
of them not spent, for both lengths. The wording in `unix.rs` and above is
aligned with the rule: a read with no weekly window names the five-hour wall;
only a weekly window that is spent without a reset ahead, or whose figure is
not finite, names none. No behaviour changed.

Mutations, each restored after its run (sha256 compared): `.max()` to `.min()`
fails the new table; a week counted usable only when the read carries no
weekly window fails three tests, two of them through the wire
(`a_creditless_five_hour_wall_asks_the_host_about_that_window`,
`a_five_hour_wall_the_host_declines_keeps_polling`).

Run on this commit's tree: `targo --unverified test -p
codex-alab-quota-recovery` (50 pass).

### The lapse tests name their resets when the 429 is answered

Three of the five tests in `rotation_decline.rs` named a reset four or five
seconds after a timestamp taken before the harness (a core thread, the fake
aterm, the claim's proof) was started and the turn submitted. On a loaded Mac
the reset could pass before the first `follow_renewal` pass; the ask then
lapses at once, no `codex-rotation prepare` is sent, and the test fails at "the
handoff is asked for". They failed in that case, never falsely passed, and
were not seen to fail.

The 429's reset is now named by the responder as it answers the walled request
(the harness accepts an empty reply sequence, for a test that mounts its own
responder). Four tests name it an hour ahead and bring it with the scripted
host sleep; one stays awake (`a_handoff_the_host_cannot_answer_resumes_at_the_reset`,
a reset eight seconds after the 429), so the unscripted clock is still
exercised. That one still needs its first ask within eight seconds of the 429.

The assertion "nothing asks it after the lapse" in the standing-`waiting` test
was vacuous: by then the resumed turn had succeeded and no wall stood. It is
replaced by an exact count. The first ask is held at the fake's gate, so its
pass has already judged the bound; the host then sleeps through the reset; the
gate opens. The test requires exactly one `prepare` in all: the pass that
finds the reset passed gives the handoff up before it asks, and nothing asks
after it.

Mutation, restored after its run (sha256 compared): a `prepare` sent in the
pass that finds the ask lapsed fails that count (two asks).

Run on this commit's tree: `targo --unverified test -p codex-alab-autopilot
--test autopilot rotation_decline` (5 pass, 10.6 s).

### The receipt guard and the reset grace, where `follow_renewal` passes them

`rotation_ask_lapsed`'s own table pinned both the receipt guard and the grace,
but nothing pinned what `follow_renewal` passes for them: every lapse test in
`rotation_decline.rs` ran an unbound intent with no grace. The reviewer's two
mutations at the call site (`false` for `renewal.receipt.is_some()`,
`Duration::ZERO` for `self.timing.reset_grace`) passed all 99 integration and
900 lib tests. The first is the one that matters: a bound receipt given up at
the reset types a usage resume on the account the session is leaving, after
the host was sent `commit`.

Two tests in `rotation_decline.rs`:

- `a_bound_handoff_is_never_given_up_at_the_reset`. The host answers `ready`
  with a receipt for the asked origin; the receipt is bound and one `commit`
  is sent (the fake knows no `commit` and answers `ERR`, so the handoff stays
  bound and unfinished). The host then answers `waiting` with that receipt to
  every `status`, and sleeps two hours past a reset an hour ahead. After the
  pass that counted the sleep, two more `status` requests are seen; there is
  no lapse line, no `usage-resume@v1`, no second model request, the intent
  keeps its receipt and `prepare` was sent once.
- `a_handoff_is_asked_for_through_the_reset_grace`. Under ten minutes of grace
  the host sleeps to five minutes past the reset: two more asks are seen after
  the pass that counted the sleep, with nothing typed and no lapse. Ten
  minutes more of sleep and the ask lapses and the resume is typed.

Mutations, each restored after its run (sha256 compared), each failing exactly
the one new test: `false` passed for the receipt ("a bound receipt was given
up at the wall's reset"); `Duration::ZERO` passed for the grace ("the ask was
given up inside the reset's grace").

Run on this commit's tree: `targo --unverified test -p codex-alab-autopilot
--test autopilot rotation_decline` (7 pass, 18.8 s).

### The wall of an asked handoff is carried across a restart

The lapse read the wall only from the Autopilot's memory. The intent is
durable (`OwedAct.renewal`), but a usage wall is not rebuilt from the rollout:
the recorded error names no reset. And while the handoff is asked for,
`follow_renewal` owns the point, so `evaluate` never decides the wall and
never leaves its recovery record. The process after a restart therefore held
the intent with no wall. (As first written this named an adopted update's
re-exec beside a crash and aterm's relaunch. A session does not re-exec while
it asks: see "The restart a session at an ask can take" below.) A host whose
`waiting` stood was asked every five seconds without end, and a `not-needed`
that came after the restart released the intent to no point at all: nothing
was typed until a person typed. Latent while no aterm prints
`codex_rotation_five_hour`, and true of a weekly wall too.

Decision: an unbound intent carries its wall in the record, as the recovery
record every other provider wait already uses. No field was added to the
intent and no record schema changed.

- `recovery::capture_asked_wall` builds the usage wall's `Recovery` for the
  asked turn with no decision: the reset as epoch milliseconds, and a
  `not_before` equal to the point's own moment, which holds nothing back.
- `Autopilot::carry_asked_wall` publishes it in `follow_renewal`'s durable
  step, between `publish_renewal` and the write-through, so the intent and its
  wall reach the record together before the host is asked. Host sleep is
  counted first, because the reset is converted from a clock that stops while
  the host sleeps. A wall that should be carried and cannot be published
  retries like a failed write-through.
- A bound intent carries none. When a receipt binds, the asked turn's usage
  wall is taken off the record in the same step that writes the bound intent,
  before `commit`: the host may then restart the session on the other account,
  and that process must not stand at the wall of the account it left. No
  other turn's recovery is touched (see the correction below), and once the
  intent has reserved its recovered turn (`AccountHandoff::turn`) the turn it
  names is that one, whose own wall is not the ask's to remove (second review,
  below). If the host retires the receipt while this process still holds the
  wall, the wall goes back on record.
- The next process restores the wall as it restores any provider wait
  (`recovery::restore`, checked against the rollout's last failed turn). The
  lapse and the released intent's resume then read it from memory as before.

What this leaves, each by reading the code and not run:

- An intent bound before a restart on the same account and retired by the
  host after it. The new process has no wall, so the unbound intent is asked
  for without limit, as before.
- A start that takes the intent but restores no recovery. The resume restores
  a carried recovery at a relaunch aterm is proven to have typed (and at a
  re-exec, which a session at an ask does not take). After a crash-like exit,
  a start aterm did not type keeps the intent and restores no wall. Run since,
  both ways, in `crash_wait_tests.rs` (second review, below).
- `/resume` away from the thread and back drops the carried wall but keeps the
  unbound intent (by reading: `on_thread_stop` and the re-registration in
  `on_thread_start` clear the thread's recovery; `set_owed_act` refuses to
  erase the renewal), so the ask is again unbounded until a turn starts.
- A wall whose 429 names no reset is still asked about without limit.
- At a relaunch aterm typed, a record that carries a recovery is restored from
  it (`Relaunch::Recovery`) before an owed act or a carry-on is considered.
  That is existing order; a session killed while it asked for a handoff now
  takes that branch, where its record carried no recovery before.
- As first landed, the recovery stayed on record after the intent was
  released to a wall that is then left to a person (`resume_limits` or
  `limit_wait` off), until the next turn started or ended. Repaired in the
  second review (below): that decision now takes the asked wall off.
- While the recovery is on record the TUI reads usage every 15 seconds,
  focused or not (`RECOVERY_USAGE_POLL` in `alab-tui/src/motion.rs`), as it
  does at any usage wall being waited out. This entry first said the poll
  "keeps upstream's pace in an unfocused tab", which understated it: upstream's
  resting pace is one read a minute and an unfocused tab's is one in five
  minutes. Each read is an HTTPS request on a new connection, so an ask that
  stands for an hour costs about 240 of them where an unfocused tab at rest
  makes 12.

Coverage. `recovery_tests.rs`:
`a_usage_wall_whose_handoff_is_asked_for_is_carried_with_its_reset` (capture
and restore round trip two hours later, a wall with no reset, and four
negative controls). `rotation_decline.rs`, where a second Autopilot,
supervisor and fake aterm resume the first one's rollout on the same scratch
login, given the record the first held, seeded as a same-pid image is (a
stand-in: the path a session at an ask takes is the crash relaunch covered in
the second review, below):
`a_handoff_carried_across_a_restart_is_given_up_at_the_walls_reset` (the new
process asks again, three times with nothing typed while the reset is an hour
ahead, then lapses and resumes when its host sleeps through the reset) and
`a_handoff_declined_after_a_restart_resumes_at_the_walls_reset` (`not-needed`
after the restart: nothing typed with the reset an hour ahead, the resume at
the reset, no lapse). `a_bound_handoff_is_never_given_up_at_the_reset` now
also requires the wall on record while the intent is unbound and none once it
is bound. The restart tests hold the new process's launch probe at the fake
until the thread manager is bound. This entry said the resume scan waits for
that probe at most twelve seconds and the order rests on that bound. The bound
then in force was shorter: the held `status` read timed out at the aterm
client's five-second `CALL_TIMEOUT`, which ended the probe. The second review
(below) gives the restarted supervisor a longer deadline, so the twelve
seconds are now the bound.

Mutations, each restored after its run (sha256 compared): the publication
removed fails three tests, the declined-after-restart one at "never happened:
the usage resume's yield is left", which is the reviewed defect itself; the
clear at binding removed fails the bound test ("a bound intent carries no
wall").

Run on this commit's tree: `targo --unverified test -p codex-alab-autopilot
--test autopilot` (103 pass); `--test autopilot rotation_decline` again after
the last comment edit (9 pass, 19.3 s); `-p codex-alab-autopilot --lib` (900
pass, 1 fail, 3 ignored). The one failure is
`resume_needs_custody_tests::a_current_resume_surfaces_its_restored_needs`, a
network wall's resume that reaches none of the changed code, at its own
60-second hang detector while other builds ran on this Mac; alone it passed
three times of three (0.6 to 0.7 s each).

### The lapse comes at the 429's reset

The lapse was justified by "from then the account the session is on can run
again". That holds for a five-hour wall, where the yield requires the 429's
reset to agree with the spent window's to within a minute. It does not hold
for a spent week whose 429 named an earlier reset, a five-hour window's: a
spent week is offered to the host whatever the 429 named, the point's wall
carries the 429's reset, and the ask lapses there. The resume is then typed
into a week that is still spent.

Decision: stated, not changed. The cost is one model request per lapse: it
meets the wall again, and that turn's end asks for a handoff again (by
reading; not run). A `not-needed` at the same wall resumes at the same
moment, so the lapse adds no new behaviour, only the same early resume for a
host that answers `waiting`. Carrying the yielded window's own reset to the
Autopilot's point would close it; that means passing it from the quota
recovery's yield through the turn's end to the wall's classification, which
is a larger change than the defect. `rotation_ask_lapsed`'s doc and
`alab/design/accounts.md` now say which reset is meant. How often a spent
week's 429 names another reset was not measured.

Correction to the above, found reading the landed diff. As first committed
(`af72b0435c`), binding cleared whatever recovery the thread had on record,
and did so again on every later pass while the intent stayed bound. A bound
intent stands until its recovered turn is acknowledged; a wall that turn meets
leaves a recovery of its own, which the next pass would have removed and the
decision after it written again, with a record on disk in between that lacked
it. `carry_asked_wall` now removes only a usage wall's recovery for the turn
the intent names. `a_bound_handoff_is_never_given_up_at_the_reset` plants
another turn's recovery once the receipt is bound and requires it still on
record two passes later; against the first version it fails ("another turn's
recovery is not the bound intent's to remove"). Run on that commit's tree:
`--test autopilot rotation_decline` (9 pass, 19.3 s).

### What was run on the repaired branch

On the tree of `f33bb69d3c`, with the Trust toolchain in the unverified lane,
from `codex-rs/`: `targo --unverified test -p codex-alab-quota-recovery` (50
pass), `-p codex-alab-core` (272 pass), `-p codex-alab-aterm` (164 pass, 2
ignored), `-p codex-alab-autopilot --test autopilot` (103 pass), `-p
codex-alab-autopilot --lib` (901 pass, 3 ignored; the hang-detector failure
recorded above did not recur), and `-p codex-core --lib` for the two glue
tests that use `SpentWall`, with `RUST_MIN_STACK=8388608` (2 pass). `targo
tippy --all-targets` on `codex-alab-core`, `codex-alab-quota-recovery` and
`codex-alab-aterm` found nothing in ALab's files. On `codex-alab-autopilot` it
found one error, a redundant clone in the new test in `recovery_tests.rs`;
the commit that removes it was followed by the same tippy run (nothing in
ALab's files) and `--lib recovery::tests` (23 pass). The warnings tippy
prints for upstream's files are not ALab's and were left.

Not run: `alab/check.sh`, any native smoke, any `targo trust` lane, a crash
relaunch of a session that was asking for a handoff, anything against an
aterm that prints `codex_rotation_five_hour`, anything against the live
backend.

## Second review repairs to the asked handoff (2026-11-12)

A second review of the branch found eight items. Each was confirmed against
the code before it was changed; none was rejected. Each repair is its own
commit. The mutations named were each restored after their run (sha256
compared), and each named test fails without what it pins.

### A bound receipt keeps its point while the host cannot be read

`follow_renewal` returned "not mine" whenever the host's policy could not be
read, whatever the intent held. For a bound receipt that hands the point to
`evaluate`, which decides the usage wall and types `usage-resume@v1` on the
account the session is leaving, after `commit` may have been sent. A read that
fails outright closes the connection, and `evaluate` is then passive anyway;
the case that typed was an `ERR` answer to `harness policy` with the claim
still standing.

Decision: `recovery_host` returns one of three answers (`RecoveryHost`: the
host permits, the host was read and refuses, no reply was read), and a bound
receipt whose host was not read keeps the point and looks again at the recheck
pace. An `ERR` answer counts as not read. An unbound intent is still let go
there, since its lapse is judged after the read, and a refusal that was read
releases as before.

Corrected after the repair's verification (2026-11-12):

- The keep has no bound. A host that answers `ERR` to every policy read with
  its claim standing holds the point without limit: no `status` is asked and
  the wall's resume is not typed after the reset, where before the repair it
  was typed at the reset. Run by the verifier with a temporary edit of the
  test: six failed reads over 30 s, no ask, no resume. That is how a bound
  receipt answered `waiting` is already held. The journal now says once that
  the point is kept, and once when the host reads again. A read that times
  out is another case: it closes the connection and the reconnect owns the
  next read.
- The keep is for a usage-limit handoff only. As first repaired it applied to
  a login handoff too, ahead of two releases a readable pass makes for it (a
  superseded turn, and a sign-in in the same file), which it would have held
  for the login recheck's minute or longer. The harm the keep prevents is a
  usage resume on the account being left, which a login handoff does not type.
  By reading; no test was written for the login case.

The defect is older than the lapse and holds on `main` for a weekly wall.

Coverage: `rotation_decline.rs`,
`a_bound_handoff_keeps_its_point_while_the_host_cannot_be_read` (with the
receipt bound and the reset passed in the host's sleep, the fake answers one
policy read with `ERR harness policy unavailable`; nothing is typed, the wall
does not go back on record, the receipt stays, and the next pass that reads
the host asks about the receipt again). Mutation: the keep arm returning false
fails it ("a usage resume was typed on the account the session is leaving").

### A wall the recovered turn meets stays on the record

Binding removed the usage recovery of "the turn the intent names" on every
pass while the intent stayed bound. Once the recovered turn is admitted the
intent names that turn (`AccountHandoff::turn`, written to the record before
admission), and a usage wall it meets on the new account leaves a recovery of
its own, which the next pass removed.

Decision: the bound branch removes the asked wall only while the intent has
reserved no recovered turn.

Coverage: `a_wall_the_recovered_turn_meets_is_not_the_bound_intents_to_remove`.
The test writes the reservation to the record itself; the admission of a
recovered turn is not run in it. Mutation: the `turn.is_none()` condition
removed fails it ("the recovered turn's own wall was taken off the record").

### Where the asked wall leaves and rejoins the record

Nothing pinned that the asked wall leaves the record in the write that binds
the receipt, before `commit` is sent; and the third call of
`carry_asked_wall`, where a retired receipt puts the wall back, had no test.

No code changed. The harness records every record write through the flush
hook, with what it carried.
`the_asked_wall_leaves_the_record_before_commit_is_sent` holds `commit` at the
fake's gate and requires that a write carrying the bound receipt and no wall
was made before it. `a_retired_receipt_puts_the_wall_back_on_record` requires
the wall in the write that unbinds the intent. The third call is kept: without
it an unbound intent stands with no wall on record until the next pass, five
seconds later.

Mutations: the call at binding deleted, moved after the write-through, and
moved after `commit` each fail the first test; the call at the retired receipt
deleted fails the second ("the write that unbound the intent carried no
wall").

### A released handoff's wall leaves the record when a person has the point

After the intent is released (`not-needed`, or the lapse), the point is
decided as any usage wall. A wait replaces the asked wall with its own
recovery. Attention (`resume_limits` or `limit_wait` off) left none, so the
asked wall stayed: the TUI read usage every 15 seconds for a reset nothing
waited for, and a restart stood at the wall again.

Decision: the ask marks the turn whose wall it put on record
(`ThreadRt::asked_wall`, in memory). A decision of attention at that turn's
point, with no intent standing on the thread, takes the wall off and journals
it. Only a marked wall is removed: a wall recorded by a decision made after
the intent's release, whose wait is switched off later, stays as it did. The
mark is of the turn, not of who published the recovery: a decision's own
recovery left by one pass that let the point go while the intent stood is
marked by the next pass and removed with the rest (harmless: nothing waits
for that reset). The record's writer carries the change at its next write; no
write-through is forced.

While the intent stands the wall stays, whatever a pass decides: a pass that
cannot read the host lets an unbound point go without releasing its intent,
and with the wait switched off it decides attention. Test added after the
repair's verification, where disabling that guard failed nothing:
`an_asked_wall_stays_on_record_while_its_intent_stands_and_the_host_is_unread`
(with the guard disabled it fails at "the wall of an intent that still stands
was taken off the record").

Coverage: `a_declined_handoff_left_to_a_person_leaves_no_wall_on_record`,
`a_handoff_declined_after_a_restart_and_left_to_a_person_leaves_no_wall_on_record`,
and the control
`a_wall_its_own_decision_left_stays_on_record_when_the_wait_is_switched_off`.
Mutations: the removal deleted fails the first two; the marker check deleted
fails the control.

### A host that prints no five-hour bound is not asked about one

At a creditless five-hour wall the in-turn question is asked every 30 seconds,
and each asking read the host's policy, a request, including against every
aterm installed today, which prints no `codex_rotation_five_hour`.

Decision: `quota_handoff_ready` refuses a five-hour wall from the last policy
reply the claim's connection remembers, when that reply prints no bound
(`account_recovery::five_hour_bound`, which `leaves_wall` now uses too). With
no reply remembered the host is read; a host that prints the bound is read
every time, since its current word decides; a spent week is read as before.
The remembered reply is replaced when the supervisor reads the policy again,
which it does on the host's `harness-policy` event (by reading). A host that
starts printing the word without that event is not noticed by this path until
something else reads its policy.

Coverage: `renewal_follow_handoff_tests.rs`,
`a_five_hour_wall_is_refused_without_a_request_to_a_host_that_prints_no_word`
(across the polls the fake sees no `harness policy` and no `codex-accounts`
request; a spent week on the same host is still read, and so is a five-hour
wall once the host prints the word). Mutation: the refusal disabled fails it
("poll 0: the refused five-hour wall sent the host a request").

### Tests that did not prove their names

- `a_declined_rotation_resumes_at_the_usage_reset` looked for a typed resume
  once, when the intent was released, before the released wall was decided.
  It now waits for the journal's line for that decision (`wait <n>s for
  usage-resume@v1`), requires `n` within five minutes under the hour, and then
  that nothing was typed.
- `a_handoff_declined_after_a_restart_resumes_at_the_walls_reset` slept 500 ms
  after the release. It waits on the same line of the restarted process.
- Mutation for both: a usage wall that resumes 1.5 seconds after its point,
  whatever its reset. The tests as they stood passed it (two runs and one);
  both now fail ("waits for its own reset, an hour ahead: Some(1)"). A wall
  that resumes at once failed the old tests too, three runs of three: the old
  look was weak, not blind.
- The restart helper's order. The launch probe is held at the fake until the
  thread manager is bound. The order cannot be made strict: the manager exists
  only once the resume returns, and upstream's test builder has no hook before
  it. The held read used to time out at the client's five seconds; the
  restarted supervisor's calls now get 30 seconds, so the scan's own
  twelve-second `LAUNCH_PROBE_WAIT` is the bound. Slower than that fails at
  "the resume read the rollout" and never passes. With a binding delayed six
  seconds a restart test passes under the new deadline and fails under the
  old.

### The restart a session at an ask can take

The restart tests in `rotation_decline.rs` seed the next process's record
directly, the shape of a same-pid re-exec. A session does not re-exec while it
asks: the ask's recheck is a timer that owes no act, `upgrade_facts` counts
that as a pending wait, and a pending wait holds the live upgrade (by reading;
not run). The restart it can take is a crash or a kill and aterm's relaunch.

`crash_wait_tests.rs`,
`a_relaunch_of_a_session_killed_while_asking_for_a_handoff_stands_at_its_wall`,
runs that through the record's real writer and the real resume scan: a record
holding an unbound weekly-quota intent beside its usage wall is written and
read back, the next process's writer sweeps it and seeds both, and the scan
runs over the failed turn's history. Under aterm's lease the thread stands at
the usage wall with its reset an hour from the ask, the intent is on record,
and the record the new process writes carries both. The control is a start a
person typed: the process keeps the seeded intent, restores no wall, and its
own record carries neither. No code changed for it, and no separate `alab-tui`
test was added, since this one goes through `Writer::write_now`,
`record::read` and `Writer::prepare`. Mutation: a relaunch that never restores
a predecessor's recovery fails it ("the relaunch stands at the usage wall with
its reset: None").

The host's side is a launch witness the test builds (aterm's lease named in
the first `status` read). No real aterm relaunched anything.

### The unread credit lists are counted, not readings of one wall

`wait_for_reset` offers a creditless wall to the host after two credit-list
reads in a row that could not be read. The count is of unread lists, whatever
wall each answer named: a five-hour reading followed by a weekly one counts as
two, and the wall offered is the second's. The comment in
`alab-quota-recovery/src/unix.rs` now says so. No behaviour changed.

### What was run after the second review

With the Trust toolchain in the unverified lane, from `codex-rs/`. On the tree
of `d019c0667d`: `targo --unverified test -p codex-alab-quota-recovery` (50
pass), `-p codex-alab-core` (272 pass), `-p codex-alab-aterm` (168 pass, 2
ignored), and `-p codex-core --lib alab_` with `RUST_MIN_STACK=8388608` (175
pass, 1 ignored). `targo tippy --all-targets` on `codex-alab-aterm` and
`codex-alab-quota-recovery` found nothing in ALab's files; on
`codex-alab-autopilot` it found one warning, the size of the new
`RecoveryHost` enum, which `5237f45bc2` boxes. On that commit's tree: the same
tippy run (nothing in ALab's files), `-p codex-alab-autopilot --lib` (907
pass, 4 ignored) and `-p codex-alab-autopilot --test autopilot` (110 pass).
`targo --unverified build -p codex-cli --bin codex` built on that tree (7 min
30 s); the binary was not run.

Not run: `alab/check.sh`, any native smoke, any `targo trust` lane, a relaunch
by a real aterm, a live upgrade attempted while a handoff is asked for,
`codex-alab-tui`'s tests (the crate is untouched), anything against an aterm
that prints `codex_rotation_five_hour`, anything against the live backend.
