<!-- Copyright 2026 Andrew Yates -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Parallel logins and quota rotation

## Requirement audit, 2026-11-03

The target is an unbounded account roster, simultaneous account logins, and
automatic continuation on another account when weekly allowance runs out,
integrated with aterm's email authentication, shared roster and machine identity.
Passing same-account renewal or quota-wait tests alone does not meet that target.

Current source and installed-host evidence:

| Requirement | Evidence | Status |
| --- | --- | --- |
| Separate credentials with shared conversation storage | `CODEX_AUTH_PATH` selects the auth file; 40 actual exec processes passed a simultaneous-request barrier with distinct account identities and one shared `CODEX_HOME` | Runtime isolation qualified for 40 concurrent logins; credential files unchanged and no shared default login created |
| Refresh serialization per login | `alab-core/src/auth_lock.rs`: a separate `<auth-file>.lock` for every slot | Busy/refused locks now return transient failures instead of sending without a lock; all eight lock tests pass, including retry and 32 simultaneous slots |
| Roster and session capacity | Aterm removes fixed account, preparation, history and session-count truncation; bounded storage writes remain atomic | Count caps removed from roster, sessions, claims and activity; 40 concurrent processes and 256-claim round trips passed; storage byte budgets remain |
| Account selection on launch | Installed `aterm harness accounts --help` describes Codex pool selection with `CODEX_AUTH_PATH` | Host supports selection; not evidence of mid-task rotation |
| Move a running interactive task to a different account after quota exhaustion | Separate typed rotation protocol, two-identity receipts, native quota selection and original-turn continuation; real-process happy path passed | Ordinary aterm app at `e75b1aeb3f79` builds and reports rotation enabled on Apple Silicon macOS; installed deployment and final combined-build rotation qualification remain pending |
| Move a running headless exec task after quota exhaustion | Native contract 3 reserves through aterm before Core suspension; four/eight shared-pool sessions, repeated moves and lost acknowledgements pass. Codex `7a6cae40a6` also retains the original managed checkout, owner and local edits across repeated handoffs | Ordinary and managed-worktree exec continuation qualified locally; ephemeral/review modes and failures after suspension remain incomplete |
| All eligible accounts exhausted | Real-process `weekly-quota-exhausted` checks both accounts at 100%, two native prepare polls, retained original task and no native actions | Passed without a rotation charge or replayed model request |
| Consecutive quota handoffs | Real-process `weekly-quota-chain` exhausts the source and first replacement, then continues on a third account | Passed with the same original turn, one completed plan, two native handoffs and two charges |
| Email authentication | Codex and Claude use aterm's common mailbox reader; 75 mailbox tests and the cross-CLI sender-scope test passed; the updated native CLI reached OpenAI | Live email-code login remains unverified because Cloudflare stopped the authorization flow before OTP retrieval |
| Shared roster and machine identity | Aterm discovers registered roster/mailbox pointers; Codex uses shared claim publication, home-account discovery and upkeep; two-machine claim and native-commit tests passed | Local replicated-table tests passed; live fleet control excludes this installation, so live cross-machine placement remains unqualified |

The native account report selected `main` and excluded `codex-m22` as not enrolled.
Its signed Commons state excludes this installation from control. The registered
secrets inventory and native enrollment flow were consulted without displaying
secret values. The unchanged provider challenge has not been bypassed, and no
live login was changed by the stopped enrollment attempt.

## Current integration and qualification

Aterm source `96d05c4c3b` claims a native quota destination through the shared
machine-claim service before committing the pool handoff. It validates the move
before the claim and again afterward. A peer that acquires the destination after
plan preparation prevents the handoff and its rotation charge. All 104
account-pool tests passed, including that case and native receipt retry accounting.

The shared claim path now includes Codex in publication, home-account discovery,
periodic upkeep and shutdown handling. The public-entry-point test publishes a
Codex claim and checks that another simulated machine refuses that account.
All 61 shared-claim tests and 38 signed-table tests passed. Account count no
longer truncates a claim set at 16 or local activity at 4,096. Canonical signed
rows fit the existing encrypted transport's 60 KiB payload budget; local activity
has a 2 MiB budget. Oversized updates refuse rather than discard live entries.

The native recovery qualification now also covers lost consume acknowledgement,
post-signal expiry and resumption, a successor crashing before acceptance, and
Stop after Signal. The latter passed with the cleanup assertion in the controller:
no replacement launched, one charge remained, the pool binding was removed, and
`launch_attempted` stayed true. Results are retained under:

- `$HOME/.aterm/.ubd/codex-quota-lost-consume-ack/result.json`
- `$HOME/.aterm/.ubd/codex-quota-post-signal-resume/result.json`
- `$HOME/.aterm/.ubd/codex-quota-successor-prefix-crash/result.json`
- `$HOME/.aterm/.ubd/codex-quota-owner-stop-inline/result.json`

The full Codex gate passed before merging main `86459bf183`. The merge completed
without conflicts, and the combined ALab library run passed 1,251 tests. A later
full gate was stopped while adding the exec suspension interface and must run
again on the finished source.

Aterm at `e75b1aeb3f794b0714b2890aa0b89c851d8162cf` passed formatting, native
lint and Linux ARM, Windows ARM, WebAssembly CPU and WebAssembly GPU cells for
the selected agent, GUI, database, types and PTY crates. Its ordinary dev app
built without installation. A private instance reported `codex_rotation=1` and
no generic native constructors. That is a production policy check, not a quota
handoff result; the report is in
`$HOME/.aterm/.ubd/aterm-production-quota-instance/production-policy-result.json`.
Newer aterm main has since been fetched and still needs composition.

These are unverified Trust checks and isolated runtime fixtures, not live
enrollment or installed fleet activation. Deployment and the remaining
interactive/exec integration are still required.

## Implementation history

The following sections record earlier intermediate states. The current audit
above takes precedence over their then-pending work.

## Next implementation boundary

Cross-account rotation needs its own host-authorized transition. Do not relax
same-account renewal's identity checks: those protect a different operation.
The host must select an eligible roster row under current policy and machine
placement, bind both old and new identity to the failed native turn, and derive
the new auth path itself. The successor must consume a durable receipt exactly
once before continuing the original task. A user cancellation or intervening
turn must invalidate the transition, and a crash must not duplicate accepted
tool work. Quota exhaustion of every eligible row must retain the task and wait
for a measured reset without cycling accounts indefinitely.

Qualification must include simultaneous independent processes, two accounts
exhausting in sequence, all accounts exhausted, fresh roster changes, renewed
credentials, crash/restart at receipt boundaries, cancellation, and unchanged
conversation provenance. Test both interactive and exec hosts. Keep synthetic
protocol qualification separate from live provider/email evidence.

## Refresh-lock validation

Commit `a833c8a8f3` removes the unlocked fallback when another process holds or
storage refuses a file login's refresh lock. The native auth glue maps the
failure to `RefreshTokenError::Transient`, so it does not record a permanent
credential failure. Its new integration case checks that a refused lock sends
no token request and preserves both stored and cached credentials.

The actual `alab-core/src/lib.rs` and its modules passed all 151 unit tests in
an isolated Cargo harness seeded with this repository's committed dependency
lock. This includes eight refresh-lock tests. The harness lives at
`$HOME/.aterm/.ubd/codex-parallel-login-core-check`; it only redirects the
manifest and cache and does not copy or replace the source files. The first
exploratory eight-test run used newly resolved dependencies; the 151-test run
then used the repository's versions. These checks do not compile the login
crate's glue or establish that the product binary contains this change.

The original workspace test process remains queued behind another build's
package-cache lock. Native login integration, full product build, landing,
and the cross-account transition remain outstanding.

## Account-count implementation, pending qualification

The companion aterm worktree is
`$HOME/.aterm/worktrees/aterm-codex-accounts-s26`, branch
`codex-accounts-s26`, commit `2c01634247`. It removes the fixed account count
from roster parsing/sync, retained quota and login state, shared pins, native
preparation, and account reporting. File byte budgets still apply; direct
snapshot writes now refuse oversize images before replacing readable state.
The rotation breaker retains all observations in its time window rather than
losing evidence after 128 rotations. Native scheduler models now require new
accounts to remain eligible while preserving cooldowns and identity admission.

New cases cover 257-row Codex/Claude parsing, 96-account state and scheduler
behavior, 192 retained rotation observations, shared adoption and roster growth,
and a sibling survey with 8,192 account facts. These are test definitions, not
passing results: aterm validation is still fetching dependencies. The Codex
workspace run is also still fetching dependencies. The isolated core lint
check did pass with warnings denied. Neither branch has been merged or installed.

## Cross-account protocol work

`alab-aterm/src/rotation.rs` adds a separate `codex-rotation` client. Prepare
names the failed thread/turn and source account/user digests. A receipt also
names a distinct destination account/user; it carries no path or command.
Ready, committed, consumed, retired and waiting responses preserve those
bindings. Invalid or ambiguous responses are rejected without echoing peer
text. All calls require the client's native self-claim.

The standalone client harness passed all 80 tests, including rejection of
same-account, ambiguous, malformed and renewal-protocol receipts and rejection
of unclaimed calls before any request reaches the host. Its lint check passed
with warnings denied. This is client protocol validation only. The host handler,
durable quota intent, destination authentication check, private restart and
original-turn continuation still need integration before rotation can run.

## Native pool planning entry point

The companion aterm branch now exposes `accounts_pool::native_quota_plan` for a
self-supervised Codex session at a stamped weekly wall. It uses the existing
pool selection and delayed commit checks, while ordinary window-driven planning
still refuses self-supervised sessions. Captured source/destination rows are
planning data only. The host still must prove the native wall and identities,
authenticate the destination and authorize the restart. Regression definitions
cover separate window/native behavior and policy or target-generation changes
before commit. These tests are pending the aterm dependency build; no runtime
rotation or host endpoint is claimed yet.

## Receipt and usage identity preparation

On the aterm branch, `f8f351cd8b` adds a purpose to the existing durable account
recovery receipt. Existing renewal records retain their same-account encoding;
a weekly-quota receipt requires an explicit different destination account.
Changing that destination changes the immutable intent. Typed usage-failure
history and the `rotation` marker remain distinct from login failure and the
`renewal` marker. This metadata work does not activate the new host endpoint.

`303d3cf873` adds identity-bound usage reads using the exact credential snapshot
sent in the successful HTTP request, including refresh/reload paths. Its new
loopback case replaces the login file before replying and checks that the old
account still owns the usage result. These aterm changes await compilation and
runtime tests. The original Codex workspace run has now passed all eight
refresh-lock tests; the native login-manager regression is building.

## Host-side candidate preparation

The aterm branch's `0bfd8edf35` adds `codex_rotation::prepare`. It requires a
managed self-supervised weekly wall, confirms fresh exhaustion on the exact
source identity, and uses the existing pool's candidate selection with a fresh
identity-bound usage check per candidate. Unusable or same-account candidates
are excluded without allocating a rotation. The entire probe sequence shares
one deadline, and disabled rotation makes no provider request. The returned
proposal retains the delayed pool commit; it neither switches accounts nor
restarts a process.

The native Codex login test build found two missing imports in the added test
case; those are corrected and that test is building again. Both the aterm
build and native Codex test have progressed beyond dependency fetching. The
control handler, idempotent pool commit linked to the durable receipt, private
restart, and original-turn continuation still need completion and qualification.

## Durable pool admission and first native test results

The native Codex auth-manager regression now passes, including refusal to send
refresh credentials when the account lock cannot be acquired. The isolated
core harness passed 151 tests and its warnings-denied lint; the isolated aterm
client harness passed 80 tests and its warnings-denied lint. These results do
not substitute for the complete workspace gate.

Aterm commit `5485aeffdd` adds a native receipt-to-binding map in guarded login
schema 5. The pool records this binding in the same durable transaction as its
rotation accounting. A retry of that receipt returns the prepared handoff
without charging a second rotation; a different binding for the same receipt
is refused. Existing login schemas remain the encoding when no native receipt
is present. Ordinary metadata edits cannot erase or change retained receipts.

The new receipt retry, schema guarding, identity-bound candidate preparation,
and disabled-policy tests passed in the first native account run. That run had
733 passing tests and one failure in an old hostile-state size assertion; the
fixture now checks the actual snapshot byte budget instead of a size derived
from the removed account count. The corrected run and the aterm verifier are
in progress. Native receipts still need terminal lifecycle cleanup, and the
host must bind these commits to its private restart receipt before exposing a
quota-rotation endpoint. No installed runtime or live account switch is claimed.

## Shared recovery state and continuation protocol

`alab-core::account_handoff` now represents both login renewal and weekly-quota
rotation. A quota receipt retains the original request identity and a distinct
destination; receipt binding cannot replace an existing selection or repair an
invalid admission state. The original native turn, start count and goal remain
part of the same handoff. The TUI writes quota custody under `owed.rotation`,
keeps login custody under `owed.renewal`, and refuses records naming both.
Its cold-restart custody checks now compare destination identity as well.

The autopilot's existing continuation lifecycle uses a purpose-aware protocol
adapter, including receipt status, commit and consumption. Destination identity
is checked before admitting recovered work. This is shared continuation
plumbing: automatic weekly-wall intent creation, quota goal provenance and the
aterm quota handler are still required before this path can activate.

The isolated core harness passed 154 tests. The handoff model's three focused
tests and warnings-denied lint passed, and the TUI/autopilot all-target compile
check passed. Their full library test run is still building. The earlier full
Codex gate was deliberately stopped while integrating these changes and does
not count as a passing gate. Aterm's revised native-commit guards passed all five
quota proposal/retry tests and its formatting, lint, Linux and Windows checks.
The wider aterm host run is still in progress; its Linux cell found a benchmark
import warning already corrected on origin/main, outside the account changes.

## Quota intent and goal provenance

The autopilot now prepares quota recovery at a native usage wall only when the
host advertises `codex_rotation=1`, account routing is `early` or `wall`, and
relaunch/resume policy allows it. The source still needs the live subscription
request scope. A `not-needed` prepare reply releases this intent to ordinary
quota recovery and suppresses another rotation attempt for the same accepted
turn, so a five-hour-only wall cannot become an indefinite weekly rotation wait.
No current host capability is advertised by this work yet.

The existing goal witness transaction now records an owned quota failure as
`limited` alongside the `UsageLimited` goal state. Restoration retains the
original goal ID, objective, usage accounting and budget. The same edit-revoking
triggers apply, and a generic usage-limited goal without that witness cannot be
restored by account recovery. Upstream receives only marked enum/match hooks;
the implementation remains in ALab glue. `alab/delta.sh --topics` could not run
because this checkout has no upstream remote; the two upstream diffs were
inspected directly and contain only these marked hooks.

The goal runtime's four focused tests and goal store's 24 tests pass, as do all
80 native client tests and the latest autopilot all-target compile check. The
prior shared-handoff component's full library run passed 735 autopilot tests
(one ignored) and all 315 TUI tests. A focused run on the newly changed autopilot
is still compiling. The companion aterm branch merged origin/main, including
the benchmark import correction, and its new non-weekly outcome passed all five
quota proposal/retry tests. The native quota host handler, private restart
integration, terminal receipt cleanup and live qualification remain unfinished.

## Host admission and remaining qualification

The focused quota intent run finished with four passing tests. Aterm commit
`7d5ba47be1` adds the selected-account guard used by private restart admission:
it checks both credential identities, the target generation, current policy and
pin state, and the immutable pool receipt binding. Ready preparation has a
separate guard and cannot authorize execution. Six quota proposal tests,
25 agent renewal tests and 15 host renewal tests passed. Its formatting check
passed; its later lint/platform run was stopped before completion while work
continued on the endpoint, and is not a passing verifier.

The missing `upstream` remote is now configured for OpenAI's repository. The
hook audit ran successfully against this commit: the newest contained upstream
base is `3026a8e54e`, and all three account-quota hooks appear under `accounts`.
The audit found 102 newer upstream commits, which have not been merged here.
This restores the audit prerequisite without claiming a full Codex gate pass.

The native quota endpoint is under development in the companion aterm
worktree. Capability advertisement remains off until native restart and
original-turn continuation are qualified. Terminal pool-receipt cleanup also
remains required. The current recovery loop requires Autopilot to be acting;
manual-session behavior must be addressed before the broader parallel-login
and seamless rotation goal is complete. No live quota switch, installed runtime
support, or remote-main landing is claimed by these checks.

## Shared email authentication boundary

Email authentication belongs to aterm's provider-neutral mailbox service.
Codex enrollment and renewal must use aterm's account methods, as Claude Code
does; this project must not grow a separate Proton Mail client or OTP reader.
In current aterm, provider sign-in receives `MailboxConfig` through
`VendorApprover`, and the shared `headless_oauth` mailbox reader delegates
Proton retrieval to the native mail layer. Codex-specific code handles OAuth
and account identity, while that shared layer owns mailbox connections,
message selection, decryption and authentication-code retrieval.

After pulling current main in both task worktrees, the installed aterm's
`accounts list --cli codex` reports `main` usable and `codex-m22` not enrolled.
A native `accounts renew codex-m22 --cli codex --here` attempt reached the
headless-browser prerequisite and stopped because that installed build found
no browser. It did not test live OTP retrieval or change the live login. The
current aterm source has native headless-browser preparation; live validation
must use that updated build and its existing shared mailbox/auth flow.

The updated aterm CLI (0.151.0, built from the pulled source) subsequently ran
that native renewal flow. It reached OpenAI's authorization page and stopped
when the provider's Cloudflare check did not clear in 90 seconds. No live login
changed, and no bypass was attempted. This is not evidence of live mailbox OTP
retrieval. The shared-mailbox suite passed 75 tests, and a new cross-CLI test
passed with OpenAI and Anthropic codes in the same inbox, in both read orders,
using aterm's common reader and each CLI's sender scope.

The private canonical package built successfully and passed its version, layout,
signature and bundled-tool checks without promotion. The first real-process
weekly-quota run stopped during bootstrap because the fixture used HTTP and
Codex requires an HTTPS workspace backend. The fixture now uses its generated
CA for both clients: Codex's existing CA setting and an explicit debug-only
aterm fixture transport restricted to loopback HTTPS. Production platform trust
and OAuth refresh verification remain unchanged. The TLS fixture rejects an
untrusted connection; all 15 controller tests and 51 aterm auth-reader tests
pass. This correction is not yet evidence of a completed native quota switch.

The real-process run then exposed an incorrect exhaustion threshold in aterm:
`PCT_MAX` is the parser's upper bound (1,000), not a full quota (100 percent).
The rotation reader now uses 100 percent for exhaustion and availability.
Its proposal tests now parse the real backend response shape instead of
inventing samples at the parser bound; 44 quota tests pass. The fixture also
uses separate emails and user IDs, because aterm correctly rejects duplicate
account rows for one provider email. The native handoff remains under test.

## Native weekly-quota qualification

The `weekly-quota` real-process case passed on 2026-11-03. It used the private
canonical package from `fcd1de322e`, an aterm native host built from
`029eb3d8ed`, and controller `2e56bf5d25`. The isolated result is at
`$HOME/.aterm/.ubd/codex-weekly-quota-complete/result.json`; its guardian
returned zero without a timeout.

The source process (PID 48998) received the typed weekly-quota refusal. Aterm
selected the distinct destination account and restarted the same conversation
as PID 49395 with its separate auth file. The destination accepted the original
turn ID, preserved the original input and completed plan/tool output, and made
one additional model request. The receipt reached `consumed`, one rotation was
charged, and its terminal pool-binding entry was removed. Both process births
and the canonical executable were checked against the native receipt.

This proves the isolated happy path with synthetic credentials, not live quota
exhaustion or enrollment. The subsequent five-second quota polling change needs
a fresh package measurement, and the compensation path needs separate fault
qualification before the production capability can be enabled.

The package built from `b8a9f83eca` also passed the same happy-path case with
native host `ddafa62ae9`. In that isolated run, Signal occurred 11.694 seconds
after host start and the native resume at 14.554 seconds, compared with 121.258
and 124.035 seconds in the preceding run. Both runs preserved the same original
turn, input and completed tool output and charged one rotation. The newer result
is `$HOME/.aterm/.ubd/codex-weekly-quota-fast/result.json`.

The `weekly-quota-rollback` case passed against host `2b7e41b868`: after Signal,
the controller removed the destination credential before its launcher attached.
The native engine restored the original auth path and conversation, did not
accept or replay the failed turn, and left the target unavailable. Its retained
pool-binding entry exposed the next cleanup requirement. The current host adds
cleanup only when the matching native completion ledger proves that rollback
finished; that strengthened case remains under test.

The strengthened rollback case subsequently passed against host `371042c61c`
with the fast package. It restored the source without replay, preserved the
single rotation charge, and removed the terminal binding. The host's background
service now also retains cleanup custody until the native completion record is
available, so cleanup does not depend on another client poll. Its regression
test verifies that a retired receipt without completion evidence keeps that
service alive. The result is
`$HOME/.aterm/.ubd/codex-quota-background-rollback/result.json`.

## Concurrent credential isolation

`alab/smoke/parallel_logins.py` ran the canonical `b8a9f83eca` package with 40
independent auth files and a shared conversation directory. The loopback HTTPS
server held every response until all 40 account requests were outstanding,
checked each bearer credential against its account header and original task,
then returned a distinct answer to each process. All 40 completed successfully,
their auth files were byte-for-byte unchanged, and no `auth.json` appeared in
the shared `CODEX_HOME`. The result is
`$HOME/.aterm/.ubd/codex-parallel-40-final/result.json`. The final run also
checks 40 distinct conversation IDs and records the tested executable hash.

The same package passed `weekly-quota-exhausted` with both provider usage
responses full. Across repeated native prepare requests, the original task
remained pending on its source account. No native action, committed receipt,
rotation charge or additional model request occurred. The result is
`$HOME/.aterm/.ubd/codex-weekly-quota-exhausted/result.json`.

`weekly-quota-chain` passed against native host `2403ea6607`: the source and
first replacement both received typed quota refusals, and a third login resumed
the same original turn. The two receipts linked the exact process births and
account identities across both handoffs, both were consumed, and both terminal
bindings were removed. Four model requests covered the initial plan, two quota
refusals, and the final answer; the plan ran only once. The result is
`$HOME/.aterm/.ubd/codex-weekly-quota-chain/result.json`.

The shared fault controller now accepts `--quota`. Its lost-Signal-ACK,
pre-Signal panic and late-hold cases passed with distinct accounts, including
single-charge cleanup after continuation. Stopping before Signal also passed
against host `85fa67146f`, with cleanup requiring the native operation's
effect-free terminal state and drained input before clearing its attempted
launch marker. Unknown operations retain that marker. The retained result is
`$HOME/.aterm/.ubd/codex-quota-owner-stop-cleanup/result.json`.

Stopping after Signal prevented any successor from launching and passed the
existing stop oracle. That case still retains a pool-binding entry despite its
observed closed, no-execution cohort; retirement from that stronger closure
proof remains under review before production activation.

The test supports a configurable count; 40 crosses the old 32-account boundary.
This complements the roster/session capacity tests and the per-login refresh
locking tests. It does not claim that arbitrary concurrent loads need no memory
or that the synthetic credentials performed real enrollment.

## Accepted root input qualification, 2026-11-04

The private `71d149396b` package passes the regression that lost accepted child
mail in the earlier package: the message is present once after native quota
rotation, with the original process and turn. Human output, a dropped consume
acknowledgment and an intervening model/configuration change also pass. Eight
concurrent sessions pass two shared-pool rotations with delayed preparation.
The full repository gate passed on `4329f1a5d1`; details and artifacts are in
[the input handoff record](EXEC-INPUT-HANDOFF.md).

The account/mail boundary remains aterm's shared service. These isolated
provider fixtures do not establish live email-code enrollment or fleet
placement. Complete runtime preservation, ephemeral/review rotation and the
other unresolved requirements above remain open. No package was promoted.


## Unread controller mail qualification, 2026-11-04

Native exec handoff now retains unread controller mail for unloaded recipients,
including accepted eviction transfers that finish after queue sealing. Restore
requires original-turn admission and preserves per-recipient order, receive IDs,
and shared message-consumption state. Seven focused tests and the full gate on
`23d8563e0b` passed; see [the controller-mail record](EXEC-CONTROLLER-MAIL.md).
Active child turn recovery remains incomplete. The private combined package and the internal-start barrier passed their
[recorded qualification](EXEC-TREE-STARTS.md#qualification-2026-11-04), including
eight shared-pool sessions and post-consumption cancellation. The active-child
probe still reproduces interruption of the original child turn. No package
was promoted.
