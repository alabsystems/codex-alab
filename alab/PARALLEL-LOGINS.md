<!-- Copyright 2026 Andrew Yates -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Parallel logins and quota rotation

## Requirement audit, 2026-11-03

The target is an unbounded account roster, simultaneous account logins, and
automatic continuation on another account when a usage allowance runs out,
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

Amended 2026-11-12. Only a spent week rotated when the paragraph above was
written. aterm's rotation branch, as the design for this amendment read it,
also leaves a five-hour wall whose reset is more than 900 seconds away, and
codex-alab now ends a creditless interactive turn at such a wall for a host
that says so (`alab/continuous-earned-resets.md`, the two sections dated
2026-11-12):

- The host says so with `codex_rotation_five_hour=<seconds>` in its `harness
  policy` reply. That word is not shipped: the installed aterm 0.198.0 prints
  `codex_rotation=1` without it (measured on this Mac, 2026-11-12), so every
  five-hour wall still waits in its turn there, as before.
- `not-needed` still releases the intent to ordinary quota recovery and
  suppresses another attempt for the same accepted turn.
- The ask is now bounded by the wall's own reset: an unbound handoff that the
  host keeps answering `waiting` or an error for is given up at the reset
  plus the reset grace, counted across host sleep, and the session resumes on
  the account it is on. An unbound intent carries its wall's reset in the
  record, so the process after a restart keeps the limit (repaired after
  review, 2026-11-12; the re-exec shape is tested on loopback fixtures, a
  crash relaunch is not). Still asked about without limit: a wall whose 429
  names no reset, and an intent that was bound before a restart on the same
  account and retired after it.
- A `codex exec` run keeps its request at every wall and asks aterm through
  its own protocol. For a wall that follows an earlier `not-needed` in the
  same run it needs a fix in aterm's exec control, which the design found
  keeps that answer for the whole root turn.

Not run: any native qualification of the five-hour handoff. The two cases
planned for it in `alab/smoke/renewal_follow.py` (`five-hour-quota`,
`five-hour-near-reset`) are not in this tree, and they need a `native_host`
fixture built from an aterm that prints the word.

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

## Rotation smokes against a recorded host, 2026-11-12

No rotation case is qualified by this section. It records how the rotation
smokes are run against today's aterm test host, what their fixture now serves,
and what was run on 2026-11-12 with the only package this Mac had: the
installed `2026111101`, built from `49f3c2ba233b`, which predates the rotation
branch (it holds neither `c7641e9bad` nor `d2f4338223`). A TUI quota case needs
a package whose source contains `c7641e9bad`. None could be built:

```sh
alab/package.sh --commit ae9b0eb15e --any-commit --no-promote \
  --out "$HOME/.aterm/.ubd/codex-rotation-2026-11-12/pkg"
# alab: a qualified atpkg-publish is required (ALAB_ATPKG_PUBLISH)   exit 1
```

`aterm pkg which atpkg-publish` reported it absent from the signed index. The
refusal was not worked around. The package holds no Python, so it is not
rebuilt per smoke commit.

### What the fixture serves

Before this work a walled TUI turn against the fixture never ended. The
fixture's usage windows had no `reset_after_seconds`; the client's
`RateLimitWindowSnapshot` requires it, the whole usage read failed, and the
turn polled until the smoke's hang detector. `alab/smoke/provider_fixture.py`
now builds the usage, earned-reset list and redemption replies, and `check()`
refuses one the client would not decode. A reply the fixture cannot build is
recorded in `model.fixture_faults` and fails the run at once instead of at the
detector, in the TUI controller and in both exec smokes
(`alab/tests/test_rotation_smoke_runs.py` runs each controller whole against a
stand-in host and requires it).

The refusal of a model request at a wall is built there too (`refusal`): the
429's body and its `x-codex-*` headers come from one reading of the account's
usage, so the reset the body names is the spent window's own, and `check`
refuses a refusal the client would not read as that wall. The three 429s of
`renewal_follow.py` are built this way; the exec smokes' own 429s (a body with
no plan and no windows) are as they were.

The module's golden replies (`alab/smoke/fixtures/provider/*.json`: three
usage readings, the credit list, the redemption reply and the weekly and
five-hour refusals) are decoded by the real client in the gate, with no
package. The usage replies go through `codex-backend-client`; the refusals go
through `codex_api::map_api_error`, as a refused model request's status,
headers and body do, and must come out as a usage limit with the plan, the
reset, the reached type and both windows the fixture stated:

```sh
cd codex-rs && targo --unverified test -p codex-alab-quota-recovery fixture_tests
# 8 passed; whole crate: 49 passed; 0 failed
```

The fixture also now states each account's windows in rate-limit headers on
every model response, keeps the source login healthy until the provider has
refused it, routes the redemption, token and usage endpoints by path, gives
each fixture login a JWT access token, and records which program sent each
request (`client`: `host` for aterm's usage reader, whose User-Agent is
exactly `codex-cli`, else `codex`).

A fixture login carries no `tokens.login_owner_id`, as before this work. A
real sign-in writes one, and `09cd86ba9d` gave each fixture login its own; it
was withdrawn. With distinct owner ids the client's `same_owner()`
(`codex-rs/login/src/auth/change_state.rs`) answers false for two logins of
one account and user, where with none it answers true, and that is what every
same-account case presents. Those cases last passed with no owner id, and no
run here could judge the change: the one `happy` trial made with it
(`t15-happy-step8`) differs from the two made without it. It is to be applied
again alone, once `happy` has run with a rotation-branch package before and
after it.

### Recording the host

The host binary says nothing of its source. `native_host_record.py` writes
`build.json` beside it: the image's sha256 and size and the verdict of the
host's own no-argument self-run are measured; the aterm commit is attested, with
a mandatory sentence saying how it is known and a field saying what that
attestation is worth. `"attestation": "trial"` says the commit is not proven to
be the binary's source. `"built-from-clean-commit"` says the host was built
from a clean checkout of it, and the record then names who built it and when
(`builder`, `built_at`). A host that fails its self-run gets no record, and a
record is never replaced; a record of an earlier schema, which no longer loads,
is kept beside the new one as `build.json.legacy`.

Only a `built-from-clean-commit` record whose self-run was made qualifies a
run. `renewal_matrix.py` refuses any other before a case runs, because its
`qualification.json` is read by its `passed` field and not by a sentence. A
single case can still be run against a trial record with `renewal_follow.py`;
its `result.json` carries `nativeHost.attestation` and
`nativeHost.qualifies`, as the exec smokes' reports do.

The host file must be named `aterm` or `aterm-*`. aterm's live snapshot accepts
a session only when the process that opened its terminal has such a name
(`owned_by_aterm`), and here that process is the host. Under the name
`native_host` every snapshot is refused as `terminal` and each TUI case ends at
its hang detector; `renewal_follow.py` and `renewal_matrix.py` now refuse such
a host by name.

```sh
cp <aterm target>/native_host <dir>/aterm-native-host
python3.14 -B -I <wt>/alab/smoke/native_host_record.py \
  --host <dir>/aterm-native-host \
  --source 414e1ae381d4bbd4272a161ca20a24eb1a714961 \
  --aterm-checkout <aterm checkout> --source-evidence '<text>' \
  --attestation trial
# a host that qualifies a run:
#  --attestation built-from-clean-commit --builder '<who>' \
#  --built-at 2026-11-12T09:30:00-08:00
```

The record used by the runs in the table below is
`$HOME/.aterm/.ubd/codex-rotation-2026-11-12/native-host-trial-aterm-name/build.json`,
written under schema 2, which had no attestation field; the runs made after
the review use a second byte copy of the same image recorded as a trial under
schema 3, `.../host-trial-s3/build.json`. Both say:
sha256 `08c2e3978e363336df385f1c8784f6c87eff3f6bbc1a94306f9c910023687841`,
178839392 bytes, self-run `PASS real-host shipping-qualified policy and
wrong-token refusal; worker EOF joined`. Its evidence text is: "TRIAL,
provenance unproven: a byte copy of
codex-rotation-2026-11-11/native-host-5h/native_host (mtime 2026-11-12 00:17:48
-0800), renamed aterm-native-host so that its process name begins with aterm-.
That binary was handed over as built from aterm 414e1ae381d, but the commit is
dated 00:18:27, after the binary was written, so it was built from a working
tree that only later became that commit, or from an earlier one. It was not
rebuilt from the commit and reports no source of its own."

A record that qualifies a run needs a host built from a clean checkout of the
landed aterm commit, that commit as `--source`, `--attestation
built-from-clean-commit` with the builder and the build time, and evidence
text saying how that is known; the hash, size and self-run verdict are then
measured from that file, named `aterm-*`.

### Running a case

```sh
python3.14 -B <wt>/alab/smoke/renewal_follow.py --host <host> --package <pkg> \
  --output <out> --case <case>
```

`<out>` must not exist. It receives `result.json`, `requests.json`,
`http-events.json`, `unrouted-posts.json`, `host-stdout.jsonl`,
`host-stderr.txt` and `renewal-control-trace.jsonl`; the guardian's verdict is
the sibling `<out>-guardian-result.json`. `result.json` names the package
source, the host record, and the sha256 of the controller and of both fixture
modules. Do not pass `-I`: the controller imports its siblings. Keep `<out>`
short: the host puts its control socket under it and exits with `path must be
shorter than SUN_LEN` when `<out>/control.sock` exceeds 103 bytes (seen with
a 93-byte output directory). Budgets are in
`alab/smoke/renewal-case-budgets.md`: 270/300 s for `weekly-quota`,
`weekly-quota-exhausted` and `five-hour-wall-soon`, 450/480 s for the chain
and rollback cases.

A case that reaches the hang detector says what it saw: the host's
live-snapshot outcomes and, for a quota case, what followed the first 429
(codex usage reads, codex credit-list reads, prepares traced) and which of
four causes that points to.

### Results of 2026-11-12

All ran against the host record above and package source `49f3c2ba233b`, and
are trial evidence only. Paths are under
`$HOME/.aterm/.ubd/codex-rotation-2026-11-12/`; the commit is the smoke tree
that ran, and the hash is the first 16 hex digits of `controllerSha256`.

| Result | Command | Smoke tree | Outcome |
| --- | --- | --- | --- |
| `trial-01-step1-weekly-quota-oldpkg` | `--case weekly-quota` | `c9e9fb3a32`, `2ec949df9e0b22b7` | failed at the detector: 13 usage reads, no credit read: the old fixture's reply did not decode |
| `t09-wq-step5` | `--case weekly-quota` | `d91b6f91ab`, `5d4404f589b0b2bb` | failed at the detector, as the negative control must: 61 codex usage reads, 9 codex credit-list reads, no prepare; `unrouted-posts.json` empty |
| `t11-wq-step6` | `--case weekly-quota` | `054b78fdb1`, `74810eeec9999fd4` | failed at the detector the same way: 50 usage reads, 9 credit-list reads, no prepare |
| `trial-03-step3-happy-oldpkg` | `--case happy`, host named `native_host` | `fcb210a427` before its amendment, `cafeadd65c7e7985` | failed at the detector: live snapshot `terminal`, `renew` never published |
| `trial-04-step3-happy-oldpkg-aterm-name` | `--case happy` | `fcb210a427` before its amendment, `cafeadd65c7e7985` | failed at the detector: snapshot published, prepare ready, source signalled at 122.0 s and exited `signal:15`, attach refused twice (126.0 s, 182.3 s), receipt left `committing`; then 1088 `resume` actions from 126.9 s to 269.9 s, 877 answered ok and 211 refused |
| `t15-happy-step7` | `--case happy` | `6f6211d0b1`, `ec95469941359264` | failed at the same point as `trial-04`: signalled at 121.9 s, attach refused twice (124.2 s, 182.1 s), receipt left `committing`; then 1099 `resume` actions from 125.0 s to 270.1 s, 891 ok and 208 refused |
| `t15-happy-step8` | `--case happy` | `09cd86ba9d`, `cd980bb6a9340264` | failed at the detector with the receipt left `committing`, but not as the two runs above: signalled at 61.9 s, attach refused three times (64.2 s, 122.2 s, 242.3 s), one `threads-ambiguous` snapshot; then 1572 `resume` actions from 65.0 s to 270.1 s, 1108 ok and 464 refused |
| `t16-5h-soon-step9` | `--case five-hour-wall-soon` | `2d19fd2e13`, `1d38329080bd8b6c` | passed: refused at 0.8 s, one walled usage read, retried and answered 30.33 s later, three requests, no prepare traced |
| `trial-05-exec-accounts-baseline-oldpkg` | `exec_accounts.py` | `ab9f144248` | passed, 3 requests; the exec fixture's old reply |
| `trial-06-exec-accounts-step4-oldpkg` | `exec_accounts.py` | `dc63e73112` | passed, 3 requests; the client now reads its credit list after each usage read |
| `t07b-exec-chain`, `t10-exec-step6-chain` | `exec_accounts.py --chain` | `dc63e73112`, `054b78fdb1` | passed, 4 requests |
| `t08-exec-step5`, `t10-exec-step6`, `t12-exec-step7`, `t14-exec-step8` | `exec_accounts.py` | `d91b6f91ab`, `054b78fdb1`, `6f6211d0b1`, `09cd86ba9d` | passed, 3 requests each; the last with the per-login owner ids since withdrawn |
| `t13-parallel-logins-step7` | `parallel_logins.py --package <pkg> --output <out> --count 40` | `6f6211d0b1` | passed: 40 concurrent requests, credential files unchanged |
| `t17-weekly-reset` | `python3.14 -I -B alab/smoke/weekly_reset.py --package <pkg> --output <out>` | `2d19fd2e13` | passed, 27 of 27 cases; this suite has its own fixture |

The exec reports name the host record but not their controller or fixture
hashes; the commit above is what identifies them. The two `trial-0[34]` runs
used the controller as it was before `b98f89d549` was amended to exempt the
exec fixture from the reply check (`checked_replies`); the TUI path is the
same in both.

Runs made after the review of this work, with the same image and package.
`f1-5h` used the schema-2 record; the others used the trial record
`host-trial-s3/build.json`, and their reports say `"attestation": "trial",
"qualifies": false`.

| Result | Command | Smoke tree | Outcome |
| --- | --- | --- | --- |
| `f1-5h` | `--case five-hour-wall-soon` | `3c3f812923`, `7639ccc0d7e2ea54` | passed: one walled usage read, retried and answered 30.27 s after the refusal, three foreground requests and one title request, no prepare traced, `renew` never published |
| `f2-exec` | `exec_accounts.py` | `b7020b0107` | passed, 3 requests; the host read the source's usage (not allowed), then the target's (allowed) |
| `f3-5h` | `--case five-hour-wall-soon` | `21ddd4d585`, `fc2dabd5c09117d1` | passed, with the 429 built by `provider_fixture.refusal`: its headers stated the five-hour window at 100% and the week at 10%; retried and answered 30.28 s later, three foreground requests, no prepare traced |
| `f4-exec` | `exec_accounts.py` | `21ddd4d585` | passed, 3 requests, the same order of reads as `f2-exec` |
| `f5-weekly` | `--case weekly-quota` | `21ddd4d585`, `fc2dabd5c09117d1` | failed at the detector as `t09` and `t11` did: 51 codex usage reads after the 429, 9 codex credit-list reads, no prepare; no fixture fault; the 429's headers stated the week at 100% |

`renewal_matrix.py --case weekly-quota` against the trial record exits 1
before any case with `the native host record qualifies no run: its
attestation is `trial``.

What the runs show:

- The negative control holds. With a package that cannot hand a walled turn
  over, `weekly-quota` fails at the detector with codex credit-list reads and
  no prepare. Before the fixture was corrected the same run made no credit
  read at all. It still holds with the refusal built by `provider_fixture.py`
  (`f5-weekly`).
- `five-hour-wall-soon` passes with this package (`t16`, `f1-5h`, `f3-5h`).
  Since the review its other account reads usable throughout and answers a
  model request at once, so a turn that waits its wall out had somewhere to go
  and did not; the controller no longer publishes `renew` in this case, and it
  fails the run by name, while it runs and again after its loop, when a
  charged rotation, a handoff receipt, a native action or a model request from
  another login appears. No run exercised that failure: package `2026111101` hands no
  five-hour wall over, so it is covered by
  `alab/tests/test_rotation_smoke_runs.py` against a stand-in host only.
- `happy` does not pass with this package against this host: the source ends
  by signal 15 and the host refuses both the forward and the rollback launch's
  attach. What follows is the outcome rotation must never have. No successor
  attaches, so the session has ended with nobody resuming it, and the host
  then issues `resume` without bound: 7.6 to 7.7 actions a second in all three
  runs (`native-actions.jsonl`), from the first refused attach until the
  detector stopped the run. Each run shows the same cycle: about 57 s of
  `resume` answered ok on the signalled operation, a refused attach, the same
  again, and then `resume` on a second operation id, refused; the run
  signalled a minute earlier went one step further, to a refused attach and
  then accepted `resume` on that second operation. Nothing in the run ends it
  but the detector. Its cause is not determined: the package
  predates the rotation branch and the host's provenance is unproven. It is the
  first thing to check when `happy` runs with a package from the rotation
  branch against a host of proven provenance: a source ended by the host must
  be followed by an attached successor, and `resume` must stop.
- Whether the refused attach is the old package or the unproven host is not
  determined. It was the same with and without the JWT access tokens
  (`trial-04` against `t15-happy-step7`: the signal at 122 s, two refused
  attaches, 211 and 208 refused `resume` actions). It was not the same with
  the per-login owner ids (`t15-happy-step8`): the source was signalled at
  61.9 s, one 60 s poll earlier, attach was refused three times, 464 `resume`
  actions were refused, and the host traced a `threads-ambiguous` live
  snapshot that neither other run has. The extra refused attach and the larger
  counts are what one more 60 s cycle before the detector produces; why the
  signal came a poll earlier, and the ambiguous snapshot, are not explained.
  One run on each side decides nothing, so the owner-id commit was reverted
  rather than kept on reading alone.
- What the host's pool holds at a wall was not shown by any run; what follows
  is the evidence and where it stops.
  - Recorded: in the exec runs the host's first provider request comes after
    the first 429. It reads the source's usage, then one destination's; in the
    chain run its first read of the final account follows the second wall. In
    the TUI quota runs (`t09`, `t11`, `t16`) the host made no provider request
    in 270 s and `host-state/accounts-codex/state.json` held no account entry.
    That the host reads usage only when asked to move a session is inferred
    from this order of events, not observed as a rule.
  - Not exercised: package `2026111101` never sends `codex-accounts snapshot`.
    No control trace of any run holds that verb, so no run tested what the
    snapshot shows or what the client decides from it.
  - By reading only: on the aterm rotation branch (`414e1ae381d`,
    `crates/aterm-gui/src/codex_accounts_control.rs`) `snapshot_in` builds its
    rows from `peek_account_records`, a read of the roster, logins and state
    files that makes no provider request, and reports
    `assignment.account_id` as null. The client's
    `Snapshot::another_account_can_run`
    (`codex-rs/alab-aterm/src/accounts.rs`) then needs two rows that are
    enrolled, not held and `usable` or `unseen`. At a TUI turn's first wall
    the rows `main` and `target` would both be unseen, which is two.
  - Untested, and a likely place for a rotation-branch run to stop: a second
    wall. By then the host has sampled the source and the middle account, so
    in `weekly-quota-chain` only the final row is still able, which is one
    where two are required. A hang there is the pool gate, and this section
    does not rule it out.

  The oracles of `weekly-quota`, `weekly-quota-exhausted` and
  `weekly-quota-chain` were left as written; none was run with a package that
  can end a walled turn.

### Not run

- `weekly-quota`, `weekly-quota-chain`, `weekly-quota-rollback` and
  `weekly-quota-exhausted` with a package whose source contains `c7641e9bad`,
  singly or through `renewal_matrix.py --case weekly-quota --case
  weekly-quota-exhausted --case weekly-quota-chain --case
  weekly-quota-rollback` (worst case 1560 s): no such package exists.
- `five-hour-wall-soon` with such a package. It is not in
  `renewal_matrix.CASES` until that run passes.
- `lost-consume-ack` and every other same-account fault case.
- `exec_shared_pool.py`, `exec_review.py` and every `exec_accounts.py` option
  other than `--chain`.
- `weekly-quota-chain` since its second account's 429 came to be built by
  `provider_fixture.refusal`; `exec_accounts.py --chain` since the review.
- `happy`, or any same-account case, since the per-login owner id was
  withdrawn.
- Any run against a host with proven provenance, and so any run of
  `renewal_matrix.py`, which now refuses a trial record.

A five-hour rotation in the TUI is unqualified, and not only for want of a run.
`alab-quota-recovery` ends a walled turn for the host only when
`weekly_exhausted` holds for the reading, which requires the 10080-minute
window at 100%. A turn refused on its five-hour window therefore waits on its
own account whatever the reset, which is what `five-hour-wall-soon` asserts
for a reset ten minutes off. The far-reset case, handed to another account,
belongs to the commit that makes that crate yield a five-hour wall.

### Unit tests

```sh
python3.14 -B -m unittest discover -s <wt>/alab/tests -p 'test_*.py'
cd <wt>/alab/smoke && python3.14 -B -m unittest renewal_follow_test
```

`renewal_follow_test` runs 28 tests with one known error, present at the
baseline: `test_usage_server_requires_its_explicit_tls_root` fails with
`CERTIFICATE_VERIFY_FAILED` (self-signed certificate) under python3.14, so its
last assertions do not execute. It is not python3.14's strict flag: clearing
`VERIFY_X509_STRICT` changes nothing. The fixture CA and its server
certificate share the subject `CN=127.0.0.1`, and the OpenSSL 3.5.9 in this
python reports the leaf as self-signed, while `/usr/bin/openssl verify` and
the client's rustls accept the same chain. Correcting it changes the
certificates every smoke serves, which needs a smoke run with a package that
can pass, so it was left.

`alab/tests/test_rotation_smoke_runs.py` runs the controllers whole, in
process, in about eight seconds: `renewal_follow.py --case
five-hour-wall-soon`, `exec_accounts.py`, `exec_shared_pool.py` and
`renewal_matrix.py` against
`alab/tests/fixtures/rotation-host/stand_in_host.py` and a stand-in package.
The stand-in is a script that signs in with the run's own fixture login and
sends a session's requests to the controller's loopback provider. It is no
host and says nothing about aterm or codex; what it tests is that a fixture
fault, a missing or misnamed host record and a handoff end a run where and
how the controllers say they do.

The whole `alab/tests` directory ran 668 tests in 821 s at `21ddd4d585` with
one failure, three errors and seven skips, the same four as at `2d19fd2e13`
(647 tests) and none in a file this work touches: three classes need a built
unit runner (`ALAB_TEST_UNIT_RUNNER`), and `test_contained_budget`'s
`test_runtime_grant_does_not_read_its_siblings_or_account_canaries` fails the
same way at `ae9b0eb15e`. The outputs are
`$HOME/.aterm/.ubd/codex-rotation-2026-11-12/unit-all-2d19fd2e13.log` and
`unit-all-21ddd4d585.log`.

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
