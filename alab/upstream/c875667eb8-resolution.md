# Six-commit frontier review

Target: `c875667eb837663697d5ffa32d375043434e73b4`, following the
previous `39b42aaacd` frontier. The six-entry intake records primary-source
decisions, not successful runtime qualification.

Required before landing: native dependency re-resolution preserving every upstream
sourced package; full ALab gate; WebSocket framing regression; app-server model
list behavior; TUI Daybreak model-switch coverage; marketplace upgrade coverage.
The SOCKS change additionally needs explicit network-proxy tests. Linux TUN,
synthetic DNS, port admission and relay execution need a Linux qualification lane;
a macOS CLI build alone cannot qualify them.

Existing ALab recovery tests passed on the previous integrated frontier. That
evidence is retained separately and does not transfer to this source revision.
No ALab policy defaults or captured per-turn recovery settings should be changed
merely to make this merge compile.

## Focused native library run

Candidate `00104b2ac5` completed the native Trust 15698 library suites with
`codex-api`: 246 passed / 1 failed, `codex-core-plugins`: 506 passed / 1 failed,
and `codex-network-proxy`: 340 passed / 16 failed. The new bounded WebSocket
frame regression, Git-version/mismatch-cache controls, and both TCP upstream
tunnel tests passed. The suites as a whole failed and are not qualification.

The owned inactive-capacity test expected obsolete buffering for context-window
errors. Its corrected contract preserves immediate context-window delivery and
upstream buffering for specialized cyber/bio/misalignment/prompt failures. The
first attempted correction incorrectly grouped those distinct typed errors and
failed 16 passed / 1 failed. On `41c633a057`, the corrected capacity filter passed
all 17 tests, zero failed (230 filtered out), including no-EOF and contradictory
completion controls. Plugin runtime registration failed for missing actor
credential and needs attribution. Proxy failures tried writing the owner's
`.codex/proxy` under the protected profile; subsequent tests poisoned a mutex.
Use an isolated test Codex home inside admitted scratch, without broadening the
profile or writing the owner's configuration. Re-run and compare pristine where
needed; do not treat these suites as passed.

The reconciled historical-base intake now records 102 source decisions and 307
pending entries across the 409-commit range from main `b79d2d3c59` to this target.
Nineteen decisions added in this continuation include upstream's removal of
background WebSocket recovery, which must not be resurrected during ALab porting.
The independent feature-reference validator landed as main `bf5452c9f4` with six
passing Python tests; this does not transfer product qualification.

## Native proxy isolation and actor source capture

On `ec15a4ecc8`, the focused login actor-credential reproduction passed one test
and the complete network-proxy library passed all 356 tests with a private Codex
home under the admitted target. No profile access was widened. Main
`24e7576f0d` makes this isolation automatic in the shared unit harness; its two
functional shell-driver tests cover success/failure cleanup and preserve other
packages' requested environment. A native run through that updated harness
remains required.

The plugin failure is specifically after policy invalidation, not initial
registration. On `704775a674`, the expanded login test passed: old captured
credentials cannot pair with a new policy account, explicit source reload creates
a usable fresh capture, and the old capture remains revoked even after reload.
The plugin fixture now explicitly reloads its retained real credential manager
after its own policy invalidation/publication. Its original pending-request
revocation, cross-origin header stripping, both auth modes and shared registration
assertions remain. This is fixture adaptation; no production credential boundary
was weakened. The complete plugin/API/proxy run on the adapted source is pending.

Tippy passed for the ALab layer on `5da4d9814b` (40 upstream diagnostics set aside).
The full gate continued into build/tests; no final passing gate result is recorded.
Six additional source decisions cover host admission/accounting, bound final-message
output, durable ancestry/index recovery and critical-path metrics with exporters
still disabled. Their source review is not runtime qualification.

## Passing API/plugin/proxy candidate suites

Candidate `704d214d66c43e11ae6a9b79d90926554b6465e1` passed all three native
Trust 15698 library suites through the updated shared harness: `codex-api` 247,
`codex-core-plugins` 507, and `codex-network-proxy` 356; zero failed, zero ignored,
zero filtered out (1,110 total). The controller used the committed lock, explicit
unverified lane and existing protected profile with four build/test threads.
The harness allocated and removed its private proxy home; the controller supplied
no manual home override. The runtime-auth registration/redirect/revocation fixture
also passed with its explicit fresh source capture. Log:
`codex-alignment-transport-isolated-native.log` in the external evidence directory.

The full gate on the older `5da4d9814b` candidate ended unsuccessfully: its owned
autopilot integration suite passed 93 tests and failed
`old_auth_error_cannot_block_replacement_goal_after_cache_ownership_changes`,
whose request-text ownership assertion needs diagnosis. Other owned suites ran,
but their success cannot erase that failure. A focused run with stronger failure
diagnostics is in progress; this candidate is not qualified or landed.

## Goal representation and root capability retention

The focused goal failure was an obsolete representation assertion, not a lost
objective: upstream `8703d32bd0` now supplies the objective as untrusted
`agent/goal_objective` context. The owned regression checks its exact output,
absence from continuation instructions, and unchanged original ownership checks.
Native focused run `codex-alignment-replacement-goal-context-native.log` passed
one test (eight ownership/mode combinations; 93 unrelated tests filtered).

Upstream `40cf168567` adds immutable host restrictions without changing root
attribution. ALab's tree capture omitted those restrictions. Owned code now
captures the actual flag, requires snapshot version 3 and its restriction field,
and applies captured restrictions before root and active/idle child startup.
It never removes fresh host restrictions. The native capture/codec/startup test
covers all four source/fresh restriction combinations and rejects old/missing
schema data; it is not yet qualified. The first focused run and composed full
gate both stopped at compilation because a direct test snapshot constructor
lacked the new field. That constructor is fixed in `2eea581620`. The rerun must
pass; production compilation alone is insufficient.

The source review ledger now contains 109 decisions and 300 pending entries.
This adds root restrictions, untrusted goal context, bounded exec-server recovery,
injected-mail delivery boundaries, owner hook latency, extension-owned board
construction and modified Ctrl+C routing. Ordered source records and reviewed
feature IDs were validated against the exact 409-commit range. This is partial
review coverage and does not close the intake or qualify the integrated source.

The root restriction rerun passed on `cb51fc02b1`: one native Core library test,
zero failures, 3,183 unrelated tests filtered. It exercises four source/destination
restriction combinations through real capture, codec and runtime startup, plus
version/missing-field rejection. Evidence: `codex-alignment-root-capabilities-v2-native.log`.
This does not substitute for the full gate or root-only tool/authorization scenarios.

The review ledger now records 119 decisions and 290 pending changes. New decisions
retain side panes and hosted inherited session IDs as opt-in; adopt active-turn replay
retention, protected Guardian framing, elastic section limit validation, concurrent
SQLite migrations, atomic retryable metadata, shared installation-ID reads, and
captured MCP owner sandbox context used for Guardian evidence. Source review and
runtime qualification remain separate.

## Model refresh and authorization-context boundary

On `e3b1a7f21a`, the focused native run passed the complete model-manager library
(60 tests), app-server refresh-worker filter (8 tests, 479 filtered) and Core
tree-capture filter (8 tests, 3,176 filtered). No failures or ignored tests in
these runs. This includes retained-originator refresh and live experience changes.
Log: `codex-alignment-model-discovery-native.log`. It does not establish live
account availability or all CLI/TUI new-model capability scenarios.

The source review ledger now records 129 reviewed changes and 280 pending.
Additional decisions cover effective-mode strict third-party deferral, shared
filesystem enforcement, opt-in host cycling, observational WebSocket metrics,
MCP doctor remediation, model-selected search metadata/discovery/provider policy
and per-message completion counts. Canonical generated export consistency and
runtime provider scenarios remain qualification work.

A stronger ephemeral restore test exposed a remaining authorization gap on
`3d0fd9e38a`: a fresh restricted destination still authorized both the inherited
and local instruction because saved model context overwrote its native policy.
The native negative run failed exactly that assertion (one failed, 3,184 filtered);
log `codex-alignment-inherited-authorization-native.log`. The earlier startup-flag
regression did not exercise context installation and cannot cover this boundary.

Owned correction `5c98c0886b` passes the destination's existing native context
into installation. When inherited-authorization policy tightens, retain Core's
canonical authorization reconstruction, review history and review mode, while
preserving captured model items, accounting and fresh process evidence. Retention
cannot be broadened beyond the fresh policy. The correction requires a successful
rerun; it is not yet qualified. No new upstream hook or parallel authorization
filter was introduced.

## Fresh authorization correction and continued intake

On `27520d4b1c`, the corrected native ephemeral restore regression passed
(one test, 3,184 filtered), followed by the exact context accounting/process
evidence control (one test, 3,184 filtered). The complete controller exited zero;
log `codex-alignment-fresh-authorization-native.log`. The former exercises both
fresh restricted and unrestricted destinations with local authorization retained.
The latter proves the matching-policy context remains intact and renews process
evidence. This does not qualify compacted checkpoint replay. Commit `403d6ce5a3`
adds a separate checkpoint variant using Core's real checkpoint producer and the
canonical ephemeral journal path; its native run is pending.

The source ledger now contains 142 reviewed changes and 267 pending. Newly
reviewed changes cover executor attachment membership and stale event cancellation,
Guardian denial preservation on reconsideration timeout, spawn trace redaction,
transient external-auth policy fencing, terminal status and per-turn summary
corrections, implicit Decisions credential filtering, agent opt-in precedence,
Windows MXC deny-parent markers, committed secrets cache publication and configured
gateway credential fixtures. Upstream installer stderr repair is superseded by
the explicit `scripts/install` removal: atpkg supplies ALab distribution. These
are source decisions, with platform/runtime qualification tracked separately.

The composed full gate is still running on older source `2eea581620`. It passed
ALab tests but observed daemon updater and protected Unix socket failures. The
new upstream fixed shared daemon socket root intentionally ignores environment
overrides; preserve the containment boundary. Pristine attribution must finish
before classifying failures. This run lacks the new authorization correction and
cannot qualify that correction even if its remaining comparisons pass.

## Checkpoint provenance and terminal advice

The checkpoint regression on `305b94c599` exposed a second real boundary: ordinary
history passed, but the checkpoint variant retained inherited authorization under
fresh restrictions (one passed, one failed; 3,184 filtered). Log:
`codex-alignment-checkpoint-authorization-native.log`. Native replay cloned an
already-adopted retained checkpoint; using the fresh context alone did not remove
its inherited entries.

Correction `e44a2db735` applies the native inherited-message policy to checkpoint
provenance during retained-context restore. The implementation lives in owned
glue; two upstream files gain only a module adapter and policy call. It removes
inherited user/assistant entries, leaving local instructions, verified answers,
revisions, counters and uncertainty intact. A round-trip checkpoint preservation
test and the actual runtime restore variants require native qualification. Delta
risk reports were inspected before and after the two new adapters.

The older full gate also found two fixtures supplying the removed recovery step
`approvals_reviewer` setting; `c80b893fb6` updates those fixtures without relaxing
strict deserialization. A real inference-hook denial returned before the common
retry handler, leaving an earlier deadline in the turn. `22e8613165` records each
received sampling failure before native early returns, preserving the terminal
verdict and clearing absent advice. The existing real sampling/queued-steering
regression must pass after this correction. No runtime success is claimed yet.

The source review ledger now contains 151 decisions and 258 pending entries. New
reviews cover source fork reservation/revalidation, bounded elastic REPL headers,
code-mode error traces, cloud Apps prewarm configuration, queued side focus
changes, shared writer coordination, backend-specific sandbox preparation, the
obsolete Seatbelt wrapper and ordered interactive request replay. Qualification
and complete intake coverage remain separate and unfinished.

## Composed boundary and TUI follow-up

On `102993e86e`, the native boundary controller passed the typed retained-context
checkpoint preservation test (one passed, 38 filtered), real ordinary/checkpoint
ephemeral restore (two passed, 3,184 filtered), admitted recovery accounting/mail
(one passed, 3,185 filtered), inference-hook steering (one passed, 3,185 filtered)
and Exec checkpoint identity (one passed, 70 filtered). Its final Login policy
check is still pending; do not claim a complete controller result. Evidence:
`codex-alignment-boundary-corrections-native.log`.

The old full gate on `2eea581620` completed its initial batch with failures. ALab
owned suites passed 1,844 tests with four existing ignored tests. The upstream
Core library had 3,178 passed/five failed/one ignored; TUI had 6,320 passed/ten
failed/five ignored. Nine upstream targets failed overall. Known ALab fixture and
behavior failures were preserved in the log. The run was deliberately cancelled
(exit 143) during isolated rechecks before pristine attribution; it is an
incomplete qualification run, not a pass or a set of waived failures.

The former full worktree now composes `102993e86e` with separate topic changes:
- `5049c31253` confines early retry-advice publication to terminal branches, with
  a real observer count assertion to prevent duplicate publication.
- `bda06e06bc` binds an explicit realtime key's transport to the selected runtime
  credential rather than retaining a different login manager's source fence.
  Configured destination restrictions remain; global source pairing is unchanged.
- `4234b00441` retains aterm background-work status in owned glue while upstream
  foreground title status remains native. Its independent comparison reads the
  native status directly and counts the intervening continuation separately.
- `cdea2717a7` observes actual preview renderer placements across overlays instead
  of consuming a second one-shot draw request.
- `57fd927d25` migrates the notice custody fixture to concurrent security RPC
  requests while retaining gated HTTP and all original settlement/race assertions.
- `5981359acc` audits the legacy authorization projection in both ThreadOwned and
  Independent review modes. Its stricter regression may expose additional work;
  retained-context success alone does not close that boundary.

These changes require native qualification. Delta reports were captured before
and after new upstream adapters. The source ledger now records 174 decisions
and 235 pending changes, including startup model/program selection, credential
custody, incremental Code Mode transport/defaults/metrics, scoped board APIs,
protected-model permissions, memory roots and full Cloud transcript hydration.
Complete intake, semantic gate wiring, live model readiness, source landing and
artifact adoption remain unfinished.


## Composed boundary checks and follow-up corrections (2026-11-10)

The first combined native run on `9371b5e8fc` stopped during test compilation:
its owned notice RPC fixture referenced reqwest without a TUI dependency. Use
the existing codex-http-client direct client, restricted to the fixture endpoint
and without redirects, instead. No new upstream manifest dependency is required.

On `ac90cf3af9`, the compiled combined run executed 41 focused tests: 37 passed
and four failed. Both realtime credential variants, all eight security-notice
custody scenarios, all nine turn/session cases, actual pet preview placement,
terminal-steering cases, retained-context accounting and ThreadOwned restore
controls passed. This is focused evidence, not a passing full gate.

Two failures reproduced inherited authorization in Independent review history
(ordinary and checkpoint restoration). Retained facts had already excluded it,
but the legacy authorization projection and raw review transcript still exposed
it. The correction filters stored inherited envelope provenance from the native
review-history checkpoint whenever fresh policy excludes inherited messages.
Native reset preserves budgeting and invalidates prefix generation. The model
window, local evidence and retained-context uncertainty remain separate. The
real four-variant regression additionally checks local legacy evidence and the
raw backup; it does not accept deleting the entire review history.

The old provider-policy fixture expected a captured credential transport to stay
valid through external credential publication. The corrected owned probe retains
that test's application-policy assertions and separately requires old transport
and permit revocation, fresh destination restrictions, and primary replacement
revocation. Its focused native check on `70ee81841e` passes one test (321 filtered)
in 0.17 seconds. No production credential boundary was relaxed.

The final failed UI case showed a failed turn's plan percentage while its
background process still ran. Determine plan ownership from the actual ALab
program state, retaining the percentage as an error only after background work
ends. The existing transition regression is retained.

These corrections compose at `f27663d19f`. Its combined qualification is pending;
use the exact run result before claiming them qualified. Logs are
`codex-alignment-composed-boundaries-http-client-native.log`,
`codex-alignment-provider-policy-native.log`, and the pending
`codex-alignment-composed-policy-native.log` under `${HOME}/.ubd/`.

Source intake now records 179 reviewed decisions and 230 pending commits in the
same complete 409-commit historical range. New reviews cover worker permission
continuity, one-time Guardian response parsing, local-daemon Bedrock eligibility,
daemon failure classification, and dangerous-command consent without sandbox
escalation. Each records runtime qualification as pending where no check ran.


A fresh anonymous Agh fetch completed at 2026-11-10T09:05Z and observed upstream
`6dfaca4dad030f5ecaf4925f210f539353b15594`, eight commits after this batch target.
Keep this 409-commit record immutable; the eight additions have a separate intake
record based on composed candidate `576b5fec15`. Three source decisions are
recorded there, five remain pending, and those eight commits are not yet merged.
The Windows idle-watcher fix is Bazel-only: ALab removes that patch and Cargo
still locks registry notify 8.2.0. It requires native dependency adaptation and
Windows qualification before being counted as a delivered fix.

Candidate `576b5fec15` also merges current main `460073cbc0`, preserving the native
child-completion wait and banner test-depth optimization. The sole conflict was
between two equivalent palette-helper calls; retained imports call the same
constructors with identical color values. This composed candidate needs its own
qualification; the preceding pending run is on `f27663d19f`.


The native combined run on `f27663d19f` completed successfully: 41 passed,
zero failed/ignored across Core 19, Exec 1, History 1, Login 1 and TUI 19.
All 12 requested groups actually executed (eight notice custody, nine turn/session,
four restore variants, eleven terminal-steering, two realtime, and seven single
checks). Core finished in 15.83 seconds; TUI in 3.12 seconds. The log is
`${HOME}/.ubd/codex-alignment-composed-policy-native.log`. ANSI controls
between a test name and its result must be stripped before counting success
records; after normalization all 41 names are accounted for. This is focused
qualification of that source, not the full gate or the newer composed candidate.

The subsequent owned restoration hardening also checks stored provenance when a
snapshot's captured retention flag already says restricted. Such a snapshot can
carry the previous independent-history defect. Its new roundtrip regression
requires local evidence, model items and elastic budgeting to survive, inherited
retained/legacy evidence to disappear, prefix generation to advance, and repeated
restriction to be idempotent. Qualify it together with current main's six native
child-suspension scenarios before treating the composed source as checked.


The fresh eight-commit intake now has four source decisions and four pending.
The added local diagnostic ZIP flow is explicit user action and remains usable
without enabling uploads. The reviewed path preserves separate log consent,
refuses local saves on remote workspaces, distinguishes save/send receipts, and
does not turn failed saves into uploads. ALab keeps feedback.enabled=false;
its actual local-save/no-upload regressions must run after source integration.


## Current-main composition and fresh source merge

The current-main combined run on `cfddd384d9` completes with 48 passing tests,
zero failed/ignored: Core 26, Exec 1, History 1, Login 1 and TUI 19. Core takes
15.22 seconds; TUI takes 3.12 seconds. It includes all six native child-suspension
cases and the older-restricted-snapshot provenance regression. Preserve
`${HOME}/.ubd/codex-alignment-current-main-boundaries-native.log`.

All eight supplemental source decisions now validate against their exact range
and behavior contract IDs. Candidate merge `fc08dea554` contains upstream
`6dfaca4dad` and current main `460073cbc0`. Only the two expected modify/delete
conflicts in removed Bazel paths needed resolution; the new notify patch was also
removed under the existing whole-path policy. No Cargo lockfile change occurred.
The supplemental changes still require native qualification. The Windows native
dependency adaptation remains open; source ancestry does not establish that fix.
The original backlog still has 230 pending source decisions.

Before the next gate, marker counts against origin/main show exactly these
previously intentional losses, rechecked after the eight-commit merge:

- core-test-fixtures 7 to 6: upstream's Guardian fixture removes its explicit
  thread stack override and inherits the runner's configured stack budget.
- exec-tree 14 to 13: remove the obsolete launch-wrapper trait import; the task
  ingress retains owned exec admission before native host admission/execution.
- startup-test-fixture 11 to 10: the obsolete startup case disappears while the
  shared recorder drain helper retains attachment/exit custody and hang bounds.

ALAB_ALLOW_HOOK_LOSS=1 covers these reviewed structural changes. It does not waive
runtime failures or authorize other marker loss. No release package has been
selected or installed for this candidate.


## Full-gate fixture corrections and further intake

The full gate on 7b8fe11c02 stops at Tippy/type-checking, exit 1. It does not
establish a CLI build or test pass. Upstream's new feedback receipt type exposed
an owned FIFO fixture still constructing a string result; use the explicit
Uploaded receipt with the same thread ID. The authorization-restore test also
needs a lexical block around its state guard before asynchronous shutdown,
retaining every authorization assertion and avoiding a lint exemption.

The historical intake now has 185 reviewed and 224 pending decisions. Since the
previous record, source review covers the mixed CI/message-board launch-lock
change, stale-denial authorization revalidation, Bazel-only zstd packaging,
terminal session budgets and Windows resolved-policy ACL preparation, and
optional GIF generation. The animation feature remains native default false;
actual GIF preservation and still-preview replay need native qualification.
Windows ACL enforcement also needs its actual platform scenario.

The terminal session-budget review adds an owned real sampling regression for
HTTP400 and SSE refusal with Retry-After and queued steering, requiring the typed
terminal outcome, one request and replaced stale advice. Its focused native run
on b5649f6892 remains pending. It is included in the next full candidate; no
runtime retry policy was changed to satisfy the fixture.


## Terminal budget qualification and composed candidate

The native terminal-steering module on 323ad6ecdb passes all 12 libtest entries
(exit 0); the session-budget parent additionally requires completion of its two
isolated HTTP and SSE child scenarios. Preserve
`${HOME}/.ubd/codex-alignment-terminal-steering-budget-native.log`.
The initial focused run on b5649f6892 failed because the HTTP fixture incorrectly
expected raw Retry-After metadata to disappear for a terminal budget error.
323ad6ecdb corrects that fixture and explicitly verifies retry_delay(1) is None.
Typed terminal errors, actual request count, queued steering, observer counts
and replacement of stale pacing advice remain asserted; runtime policy is unchanged.

After the focused run ended, merge the full candidate's typed feedback receipt
and lexical guard fixture corrections into this branch. The full gate remains
on frozen e2c424005d, which lacks the corrected budget fixture: its result cannot
qualify this newer composed candidate. That gate has passed Tippy, built and run
the CLI, and passed 1,844 ALab-owned tests (four existing ignores); hooked upstream
qualification remains in progress.

The historical intake now contains 187 reviewed and 222 pending decisions, with
board cleanup lifecycle ownership and typed cloud transcript updates reviewed.
The independent eight-commit supplemental record remains complete. Runtime-auth
079e17a42234 is still pending: its real-session integration fixture has now been
read, including lazy shared registration, per-thread modes, replacement-credential
resume, catalog refresh and third-party destination isolation. This source review
does not establish native integration execution or live model availability.


## Shared runtime authentication intake

079e17a42234 is reviewed: adopt its consolidated request/catalog construction,
keep runtime_agent_instances opt-in at native default false. The existing ALab
source-bound factory hook already covers both native request branches; an added
ALab provider framework would duplicate that seam. Catalog settings preserve host
cache policy and live discovery while credentials, per-thread auth mode and pooled
connection identity retain their owner. Real-session runtime authentication and
injected-cache integration tests require a separate native run; the standard
hooked-library gate does not execute these integration fixtures. Historical
source review is now 188 complete, 221 pending; this remains a partial intake.


## Native runtime/catalog results and cloud/host intake

Focused native integration on 87084e4142 exits 0: three runtime_auth scenarios
and six injected_models_cache scenarios pass, zero failed/ignored, no skips
reported. Preserve ${HOME}/.ubd/codex-alignment-runtime-catalog-native.log.
This includes the actual shared-runtime/per-thread-mode/key-b-resume fixture,
MCP resumed-actor uploads, Daybreak auth and provider/cache ownership changes.
It is deterministic client evidence, not a live account entitlement check.

Cloud item lifecycle and history pagination source reviews are recorded: typed
identity/status/presence coexist with raw unknown payloads, and single-page
history is explicitly not synchronized Attach replay. Native cloud-client tests
remain to run. The board host migration also follows native public interfaces
rather than adding an ALab compatibility wrapper. Historical intake now has
191 reviewed and 218 pending decisions; full intake and qualification remain open.


Cloud-client native qualification on 3998257dfa exits 0 with 52 tests passed,
zero failed/ignored. The focused qualification observation JSON records exact
source/compiler identities and completed log hashes for this, runtime/catalog
and terminal steering. It is manually collected evidence, not a replacement
for semantic coverage enforcement or the full composed gate. The ongoing full
gate on e2c424005d has daemon-update failures and local control-socket permission
failures; pristine comparison remains pending, so no attribution or waiver yet.


## Selected credential ownership and model metadata

Source review now covers faeb379646bd native per-source runtime bindings,
12dc86beb3d4 thread-specific startup/selected metadata, and 22103802324d shared
thread-catalog reviewer policy. Retain native source ownership and public host
interfaces rather than introducing another ALab credential or model registry.
Historical intake has 194 reviewed and 215 pending decisions. Targeted native
provider/runtime, models-cache and Guardian policy qualification remains pending.

The GIF artifact/still-preview/replay integration passes one test on e23516d6be.
The board filter mistakenly used the source filename and selected zero tests;
that supplies no board qualification. Correct the filter to the actual nested
module and reject empty selections in the next focused controller. The full gate
is still running on older e2c424005d; its known budget fixture failure is fixed
on this branch, while the sticky-turn permission failure needs diagnosis and
pristine comparison. No full-gate waiver or completion is implied.


## Corrected selection results and admitted test tooling

The corrected native selection on 6c7808dc8a exits1 overall. Its 24 board tests
include 22 passes and two failures because codex-message-board-mcp was not prepared.
Models-cache authentication passes nine tests, model-provider 142 and runtime bindings 28,
all zero failed/ignored; these pipelines reached the checker only after status 0.
The protected profile refuses reading the external checker, so preserve its
failed status and move future checks to the repository-owned focused_results.py.
The new checker passes eight adversarial tests and rejects empty, incomplete, failed,
ignored, skipped, overlapping and inconsistent output. It checks counts only and
does not replace native artifact provenance or the full check.sh gate. Prepare
the exact native board helper and bind both cargo_bin executable aliases before
rerunning board integration.

Historical source intake now records201 reviewed and 208 pending, including terminal
runtime denials, same-origin gateway discovery, async extension budgets, native
attribution auth with ALab no-attribution retained, and exec-catalog lifecycle
regressions. These reviews do not complete intake or their pending integration
qualification. The full gate remains frozen on e2c424005d and active.


The d4a341bd52cc runtime WebSocket admission review is complete: reconnect
for each runtime request/classification, avoid speculative runtime admission,
retain ordinary reuse and preserve explicit auth/policy/429 errors instead of
hiding them behind HTTP fallback. Historical source review now has202 complete
and207 pending decisions. Paused-runtime and Guardian transport matrix native
scenarios remain to qualify. The full gate's isolated rerun reproduces all 19
failures across 19 executed copies; pristine comparison has started building its
helpers. A reproduced candidate failure is not yet an attributable ALab fault
or an upstream waiver.


The board mailbox routing review f05aa41da6d0 is complete. Native public
submission now holds the residency pin through dispatch; ordinary queued mail
can survive idle recipients without reopening finalized answers. Existing ALab
input/controller mailbox custody already captures receive IDs, late eviction
transfers and ordered restore. Keep those adapters instead of creating a second
board-specific queue. Seven relevant scenarios report passes in the prior
partial native run; the complete board suite still awaits the prepared-helper
rerun. Historical intake now has 203 reviewed and 206 pending decisions.


Guardian denial reconsideration a80514348682 stays opt-in at native default
false. The single follow-up retains the original evidence, deadline and model
settings; invalid or failed follow-ups preserve denial and discard the session.
The feature is not an ALab permission override. Source/config/schema and actual
command/cancellation fixtures are reviewed; native opt-in execution and races
remain to qualify. Historical source intake is 204 reviewed and 205 pending.


Windows deny-read scanning review 61506432d60f is complete: one in-process
walker replaces ripgrep discovery and fallback traversal. The changed link
boundary is explicit (skip child directory links, follow linked scan roots);
Windows ACL and MXC behavior still need platform qualification. Do not infer
permission parity from macOS compilation or protocol glob tests. Historical
intake now has 205 reviewed and 204 pending decisions.
