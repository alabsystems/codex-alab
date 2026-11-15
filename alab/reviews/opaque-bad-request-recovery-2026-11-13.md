<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Opaque bad-request recovery

The reported running session ended a turn with `{"detail":"Bad Request"}` and
protocol category `Other`. The HTTP bridge maps an ordinary 400 to
`InvalidRequest`, whose retry policy is `Never`; Autopilot then treats that
`Other` detail as a fatal request error. This explains the interruption. The
backend supplied no diagnostic identifying why it rejected the request.

An isolated loopback reproduction using installed package `2026111301` produced
one request, exit 1, and the same terminal error despite a nonzero stream retry
budget. No real account or external model endpoint was used.

## Change

Recognize only the single-field opaque envelope, rejecting duplicate keys,
additional fields, and specific provider errors. Route that HTTP 400 through
core's bounded retries and transport fallback. If those attempts end the turn,
use Autopilot's paced server retry, without the repeated-cutoff hold. A saved
`Other` failure resumes through its matching recovery record, preserving the
provider deadline. Raw error text alone does not supply automatic retry authority.
Explicit validation, policy, access, and configuration refusals retain their
existing behavior; current retry policy still governs continuation.

The existing affected tab's goal remained stalled after a manually continued
turn succeeded. `/goal resume` restored its existing goal, and the tab reported
`Goal active` and resumed execution. Its application binary was not replaced.

## Validation and delivery boundary

The envelope unit test, 25 recovery tests, and 28 HTTP-bridge tests passed using
source-selected Trust 15698's explicit unverified test lane. The native core
fixture `native_turn_retries_opaque_http_400_but_not_a_specific_refusal` also
passed: a real loopback HTTP 400 followed by SSE completion took two model
requests and completed the same turn; a specific validation error took one
request and failed the turn.

The full native gate on `1eff1aebac8e46fb892be8d8100c10102e69d661` passed Tippy
with no ALab findings (41 upstream diagnostics set aside). Its certified
application build then failed before compilation: Trust refuses the existing
in-process procedural-macro dependency graph and consequently emits no
identity-bound proof inventory. The retained observation is under
`.git/alab-qualification/1eff1aebac8e46fb892be8d8100c10102e69d661/run-0tpns5xh/`.

This is not a successful full gate or a certified application build. Preserve
the certification requirement and its failure; do not substitute an unverified
application build or change the pinned compiler to claim delivery. A new
package and the fixed-binary reproduction remain blocked on that compiler
capability. The source fix and native-core runtime test do not claim that the
installed application has been upgraded or that the backend's own rejection
cause has been repaired.

## Follow-up: restart pacing regression

The normalized `Stream` diagnostic alone cannot prove whether the response
carried a `Retry-After` deadline. Do not broaden text-only restoration to treat
it as an unadvised server error. The validated recovery record retains the
provider deadline and the already selected retry boundary.

Add a regression using the actual `CodexErr` display text: without the recovery
record it requires attention; with the matching record, a 40-minute provider
deadline wins over the earlier 30-minute local retry. The capture/restore
matrix also exercises an opaque failure with explicit five-minute advice.
These cases guard against both lost recovery classification and early retries.
All 27 `recovery::tests` passed with source-selected Trust 15698 using
`targo --unverified test --locked -p codex-alab-autopilot --lib recovery::tests`.

Removing only the helper's Serde derive would not clear the application's
broader procedural-macro certification blocker. Defer that separate compiler
capability work and preserve the current strict JSON deserializer.

## Follow-up: remove the legacy raw-body shortcut

The same missing-deadline constraint applies to the legacy raw 400 body, not
only the normalized diagnostic. A before-change regression confirmed that the
raw-body shortcut produced `api-retry@v1` with no matching recovery record. It
could therefore run before a provider deadline that was no longer available.

Remove that compatibility exception. Live request retry and matching saved
recovery continue unchanged. Without the record, both raw and normalized
failures require explicit user action instead of inventing timing metadata.
The regression also supplies a matching saved record and requires its
40-minute provider deadline in both Cruise and Land, including relaunches and
current retry-policy revocation.
The regression failed against the old shortcut and all 27 recovery tests
passed after removal, including the live HTTP retry fixture.

## Follow-up: native continuation after exhausted request retries

The immediate HTTP retry fixture and the recovery decision tests left one
integration boundary untested for this error: a real failed core turn reaching
the claimed native Autopilot and then continuing the original task. Extend the
existing provider-deadline integration test with the exact opaque HTTP 400
envelope, with both ordinary request and stream retries disabled.

The new case passed alongside HTTP 429 and 503 (three tests). It observes two
failed turns followed by successful recovery, requires the first provider's
two-second deadline before continuation, and checks that absent advice on the
second failure does not reuse the first deadline. The original input and the
completed tool result remain in context exactly once, and the completed tool
executes once. This adds evidence for the landed behavior without changing
production retry policy or relaxing the deferred certified-build requirement.

## Follow-up: crash and signal pacing during an ordinary retry

The native crash-record fixture previously covered the unbounded overload
wait. Add the opaque HTTP 400 to that fixture with one ordinary stream retry
and a one-hour provider deadline. This checks the shared persistence hook
through a different retry path, rather than assuming the overload result also
proves the bounded stream path.

Both a witnessed crash and SIGTERM teardown preserve the busy record's
selected wait. The actual record writer and reader, native rollout reader,
predecessor reconciliation, and restored decision retain a retry no earlier
than the original provider deadline. Each case sends only the original HTTP
request. All eight crash-pacing fixture tests passed. This is additional
restart evidence; it does not claim a certified application package or alter
the production policy.

## Broader verification and delivery handoff

At `fbaae0f7c8`, the full shared-runtime and aterm-client library suites passed:
267 `codex-alab-core` tests and 179 `codex-alab-aterm` tests, with two existing
ignored client tests. These supplement the complete Autopilot run recorded
in `publication-test-deadline-2026-11-13.md` and the later crash-pacing cases.
No further production change was justified by these checks.

The original aterm session still reports its goal and is actively running its
compiler qualification work. Its unsent input was left untouched. That is
evidence that the original interruption was resumed, not evidence that it is
running the patched application. The source still selects Trust 15698 via
`trust.lock`; the previously retained certification refusal remains the
delivery blocker. Do not claim an installed fix, change that lock by hand, or
replace the required certified build with an unverified application build.
The next delivery step remains a successful native qualification and package
build once the compiler can certify this application's dependencies.

## Follow-up: compaction recovery boundary

Extend the existing compaction exhaustion/deadline matrix with the opaque
HTTP 400 for both legacy and v2 compaction. With ordinary retries disabled,
the failed compaction reaches the lifecycle once, retains `Retry-After`, and
allows the original task to recover only after that deadline. The full ten
compaction integration tests passed, including the specific invalid-input
refusals that must stop even when they carry retry advice. This tests the
compaction request path without broadening the production error classifier.

## Follow-up: an active goal stopped by the opaque error

Extend the native goal-recovery fixture with the exact opaque HTTP 400 and
two-second provider advice. The original scenario covered HTTP 500; both
now use the same fixture with ordinary request retries disabled. The goal
starts active, its turn fails, and `GoalService` resumes that same goal once.
The goal ID and objective are checked before and after recovery. The resumed
turn fails again and leaves a paced retry, preserving the
provider deadline with `cut_off=false`. No replacement prompt is typed.

All six goal-recovery integration tests passed, including the existing
controls for a goal blocked before the failed turn, a paused goal, and a
refused resume. This adds coverage of the original user's active-goal case;
it changes no production goal or retry policy. Focused Tippy passed with no
ALab diagnostics; the three upstream deprecation warnings remain unchanged.

## Follow-up: retain correlation evidence for the backend rejection

Revisit the original rollout rather than infer a server cause from the client
classification. Its final tool call has a corresponding result. Reconstructing
the retained items after the last compaction found no missing or duplicate
tool-call/result IDs. The last reported input token count was below the
reported context limit. These observations do not establish that the actual
wire request was valid, and do not justify changing or repairing history.
The available local logs contain no correlated parent-session diagnostic for
the original failure. A later child failure also records only the opaque body.

The HTTP bridge discards correlation headers when it maps this particular
400 to a recoverable stream error. Record a local informational event with
`x-request-id` (falling back to `x-oai-request-id`) and `cf-ray`, when supplied.
Accept only nonempty ASCII alphanumeric, underscore and hyphen tokens of at
most 128 bytes; discard other values rather than log arbitrary response text
or produce a misleading truncated ID. Do not record request content or other
headers. The event is activation-gated and emitted only for the exact opaque
envelope; retry policy, provider pacing and error text stay unchanged.

This improves future diagnosis, but cannot recover metadata already lost by
the installed binary or identify the backend's original rejection cause.
The certified-build delivery boundary above still applies.

All 29 HTTP-bridge tests passed in the source-selected unverified test lane,
including token length/content boundaries, activation isolation, exact-envelope
classification, and preservation of provider retry advice.
Focused Tippy completed successfully with no diagnostics in the changed code;
its only two warnings concern existing `chunks_exact` uses in `utils/path-uri`.

## Follow-up: verify the emitted diagnostic, not just its parser

The previous boundary test checked accepted identifiers but did not observe
the tracing event. Extend the existing isolated HTTP-bridge regression with a
scoped subscriber. It checks that only the opaque envelope emits an event,
that the primary ID wins when valid, that an invalid primary falls back to
the alternate ID, and that the event contains exactly the message, selected
request ID and Cloudflare ID. Synthetic authorization and unrelated headers
are present on the response but absent from the event. Retry advice remains
unchanged through both cases.

Source review confirms the TUI's SQLite layer admits this INFO event via its
normal default target filter, while the remote log exporter admits only
`codex_otel` targets. This is a local diagnostic, not an added remote export.
The regression observes the real HTTP bridge and emitted fields; it does not
claim to exercise a running TUI or the SQLite writer. All 29 HTTP-bridge tests
passed. No production behavior changes in this follow-up.
Focused Tippy also passed with only the same two unrelated `utils/path-uri`
warnings recorded above.

## Follow-up: complete API library verification

At `a78c95ff3e2230d9098bb5821dea89955081c538`, run the complete API
library suite in the source-selected unverified test lane, rather than only
the bridge filter: all 251 tests passed, with no failures or ignored tests.
This includes the surrounding transport, streamed-error, quota, policy,
file-retry and realtime-websocket cases. It provides broader evidence that
the isolated diagnostic subscriber and narrow 400 mapping do not disturb
the other API paths; it remains test evidence, not a certified application.

A read-only observation of the original aterm tab still shows active work
on the same TLA+ compiler goal and a working descendant process. Its unsent
input remains untouched. No new application was installed. Keep the
previously recorded certification blocker as the delivery prerequisite;
do not replay an unchanged failed gate or claim that source validation
upgraded the running process. No further production change is justified
by this review.

## Follow-up: check for a supported macro certification mode

Inspect the retained minimal derive fixture's refusal against local Trust
source `ab4744ffff21eacfff8ab19eaf1300dbaa8c0e13`, without changing that
checkout or rerunning the failed gate. In
`src/tools/targo/crates/targo-core/src/compiler/mod.rs`,
`rustc_loaded_dependency_closure` collects transitive proc-macro units and
`admit_in_process_proc_macro_tcb` rejects unaudited providers whenever the
lane claims proof authority. Its audited-provider path is bound to specific
Trust provider sources; it is not a general exception for serde or clap.

The compiler documents a separate WASM proc-macro option, but this Targo
admission step classifies proc-macro dependency units before compiler
loading and has no alternate WASM admission branch. The advisory alternative
explicitly forfeits proof authority. Neither supplies a supported certified
build configuration for the retained application dependency graph. Do not
remove the certification policy, relabel dependencies as audited, or treat
an advisory build as a deliverable. The delivery prerequisite remains a
supported compiler capability or an explicitly reviewed source architecture
that eliminates these dependencies from the proof graph. This investigation
does not establish that fixing that boundary alone would pass the full gate.

## Follow-up: refuse a vacuous isolated bridge test

The isolated HTTP-bridge test previously accepted its child process solely
by exit status. Libtest exits successfully for an exact filter matching zero
tests, so moving or renaming the test could silently remove this coverage.
Derive the module prefix from `module_path!()`, capture the child output,
and require a reported test pass as well as a successful exit. Preserve the
child's stdout and stderr in assertion failures for diagnosis.

All 29 bridge tests passed after the change. A separate negative control
invoked the same native test binary with a deliberately nonexistent exact
filter: it exited zero with zero tests, and its output fails the new
pass-count condition. Focused Tippy passed with the same two unrelated
`utils/path-uri` warnings. This closes a test-admission gap; production retry
and diagnostic behavior are unchanged.

## Follow-up: determine whether a local layer change can clear admission

At `46cfddbc9262f952317858fec92d44d2ad71892e`, collect locked Cargo
metadata filtered for `aarch64-apple-darwin`, then walk normal/build edges
from `codex-cli`, excluding dev-only edges. The workspace-resolved graph
contains 78 reachable proc-macro packages; all still have a path when
`codex-alab-*` packages are excluded. This is a diagnostic overapproximation:
workspace feature unification and Cargo package metadata are not the native
compiler's authenticated per-unit build inventory. Do not use the count to
claim 78 active or failed shipping units.

Two short witnesses establish the practical blocker without relying on that
count: `codex-cli -> serde -> serde_derive` and
`codex-cli -> clap -> clap_derive`. The pristine upstream CLI manifest at
`ce3031018ef130e653ed4d9c5bb971652de97161` explicitly enables `derive`
for both dependencies. Removing derives only from ALab's crates therefore
cannot make this shipping target pass the current boundary. Rewriting
upstream's serialization and CLI architecture solely to evade that boundary
would substantially expand the carried layer and is not an appropriate
repair for this request-error bug.

Retain the metadata and name/version witness paths under this source's
`.git/alab-validation` evidence directory. Metadata collection downloaded
missing locked crate sources after offline collection reported cache misses;
it neither built an application nor changed the lockfile. No new production
change or certification claim follows from this inventory.

## Follow-up: websocket handshake admission

Exercise the real websocket error conversion, `map_ws_error`, followed by
the shared HTTP bridge. The fixture supplies a failed handshake response
with the original body, request ID and seven-second provider advice. With
ALab active, only the exact opaque HTTP 400 becomes the normalized stream
error. The same 400 stays terminal when inactive; a specific validation
error and an envelope with an added error code remain terminal in either
mode. HTTP 403 retains its HTTP identity and existing upstream retry policy.
Every case preserves its provider deadline and response body at the bridge.

The active cases run in the existing isolated child, which requires its
completion marker and a passing test count. This covers the actual
handshake-to-HTTP error conversion; it is not a live network connection or
a new policy for wrapped websocket stream frames. No production change is
needed because the handshake already uses the shared HTTP bridge.
All 252 API library tests passed. Focused Tippy passed with only the two
previously recorded `utils/path-uri` warnings.

## Follow-up: native connection retries and HTTP fallback

Extend the real-core request recovery fixture to start with websocket
transport enabled. A loopback HTTP server rejects the actual websocket
upgrade with the opaque 400; the same server supplies the eventual SSE
completion over HTTP. With two stream retries allowed and request retries
disabled, the fixture checks the handshake attempt count, the single HTTP
replay, successful completion, and exactly one copy of the original user
input in the recovered request. The specific invalid-request control must
finish with an error and no HTTP replay. The existing HTTP-only recovery
case uses the same fixture and retains its original request counts.

This exercises the native connection/fallback path in addition to the API
mapper regression. It changes no production retry or transport policy.

The native connection cases passed: the opaque response produced four
upgrade attempts (startup prewarm plus the original turn's attempt and two
retries), followed by one successful HTTP request. The specific refusal
produced two upgrade attempts (prewarm and the turn attempt), no HTTP replay,
and an errored turn. The recovered request contained the original input
exactly once and completed with the expected assistant response.
All 28 recovery-module tests passed in the source-selected unverified test
lane, including the HTTP and websocket native cases.
Focused Tippy completed successfully; its 12 warnings were outside the
changed ALab test file. The source-selected certified-build prerequisite
remains unchanged.

## Follow-up: align the feature inventory with verified recovery policy

The inventory's old blanket statement that unadvised errors never become
generic retries did not describe the existing transient transport policy.
Clarify the invariant around terminal request refusals, and explicitly name
the now-tested HTTP and websocket-handshake recovery, single-copy original
input, and saved-record deadline scenarios. The gate already selects the
Autopilot crate's tests and the hooked API library's tests; no new build
exception or test selection path is needed. This updates the review contract,
not production behavior or the deferred installed-delivery status.


## Object envelope regression

A hostile review found that deriving Serde deserialization for the one-field
struct also accepts the positional JSON array `["Bad Request"]`. That is not
the observed backend object and must retain ordinary error classification.
Require an object opening token before deserializing; the existing strict
parser still validates the entire document, unknown fields and duplicate keys.
This central predicate covers transport and Autopilot recovery together.

The added array case fails against the previous implementation and passes
with the guard. Include whitespace-wrapped arrays, a string scalar and null
as negative controls. All 271 `codex-alab-core` library tests pass using
Trust 16387 in the explicit unverified test lane. Core library Tippy also
passes with warnings denied. No application build or
installed qualification is implied; the previously recorded certification
blocker remains.


## Array rejection through native transports

Extend the object-envelope regression beyond the central parser. The HTTP
bridge and WebSocket handshake matrices now reject the array while keeping
provider deadlines and headers intact. The native HTTP and WebSocket
fixtures each exercise the same array response: the turn ends with an
error without recovery requests. A separate Autopilot classification check
requires a fatal wall for the raw invalid-request array.

All 252 API library tests pass. All 29 tests selected by the exact
`recovery::tests::` module prefix pass, including both native fixtures and
the new fatal-wall case. These used Trust 16387's explicit unverified test
lane; logs are retained as `array-envelope-api.log` and
`array-envelope-recovery.log` under the parent commit's local validation
folder. Inspect the named cases and test counts: the initial broader
`recovery_tests::` filter selected a different five-test module, so its
success was insufficient evidence and was replaced by the exact-module run.
No shipping proof or installed update is claimed.


## Complete Autopilot check after array hardening

Run the unfiltered Autopilot library suite on clean landed source
`ef41000a7d4eaa298c221721214174dd71433b22`: 920 tests passed,
zero failed, four ignored, in 96.68 seconds. This checks the object-envelope
change alongside goal recovery, deadlines, cancellation and policy rather
than relying only on the focused transport cases. Use Trust 16387 with the
explicit unverified test lane; this is not application certification.

The four pre-existing ignored tests require external Cargo-copy proof
fixtures (two cases), generate a corpus fixture (one), or run a separate
holdout measurement (one). They were not silently counted as passes or
activated as ordinary read-only tests. The full log and command manifest
are retained under the source commit's local validation folder at
`autopilot-array-full/`. No further source correction was required.


## Persisted request-correlation regression

Close the remaining diagnostic evidence gap with an isolated regression
that calls the actual API error mapper under the installed dynamic SQLite
filter and writer, flushes the writer, and queries a temporary database.
The retained row has INFO level, the original thread identity, request ID
and Cloudflare correlation ID. Synthetic authorization and unrelated
headers are absent. The error still admits its bounded transport retry.
This tests local retention; it does not claim remote export, recover the
original incident's missing ID, or execute an installed application.

Keep the test in the ALab-owned Autopilot crate, using test-only API and
tracing-subscriber dependencies (the external version pinned exactly).
Only the ALab package entry changes in the lockfile. Reuse the installed
filter source instead of duplicating its rules. The child process isolates
activation and tracing; its parent requires one real test pass, not only a
successful exit. A temporary SQLite home keeps live user data untouched.

The focused regression passes, and the complete library suite passes
921 tests with zero failures and four pre-existing ignores (120.52 seconds),
using Trust 16387's explicit unverified test lane. The known Darwin linker
warning concerns the test binary's large unwind section; it does not assert
application qualification. Shipping remains blocked by the certified-build
prerequisite already recorded above.

Tippy passes for the Autopilot library and tests with no ALab diagnostic.
Its three warnings are existing `fetch_update` deprecations in upstream
`codex-diagnostics`, `codex-core`, and `core_test_support`. Preserve the
focused, full-suite and lint logs under the parent source commit's local
validation directory at `opaque-sqlite/`.


## Saturate pacing counters restored from records

`WallTrack::carried` accepts the complete `u32` attempt count, while the next
retry previously incremented it with unchecked addition. A valid maximum
count therefore panicked in a checked build or wrapped to the initial retry
rung in an unchecked build. The two short-turn streak increments had the
same problem after `seed_pace` restored a maximum streak.

Use saturating addition for these three increments. Counts below the maximum
retain their existing behavior; at the maximum the retry ladder remains on
its final rung. Real work still clears the short-turn streak. This is a
record-boundary case, not evidence that the original Bad Request incident
had exhausted a counter.

The new retry regression fails on the previous code with `attempt to add
with overflow`. With the fix, three consecutive retries remain paced at the
final rung. The short-streak regression exercises both Autopilot and other
turns and confirms that real work resets the saturated count. All 923
Autopilot library tests pass, with four pre-existing ignores, in 94.93
seconds using Trust 16387's explicit unverified test lane. Installation
still requires the previously blocked certified application build.

Tippy's first pass identified a redundant clone in the regression fixture;
remove it and rerun the library/test lint. The final pass has no ALab
findings; its three warnings are the existing upstream `fetch_update`
deprecations. The full-suite result predates only that clone removal; the final source
passes lint. A focused rerun after the cleanup began a duplicate dependency
rebuild, which was deliberately stopped before tests to preserve capacity.
It is not counted as another test pass or a test failure.


## Packaged-binary regression smoke

Add `alab/smoke/request_recovery.py` so the blocked delivery has an executable
acceptance check when a certified candidate becomes available. It runs an
existing binary under the command guardian with a fresh home and a loopback
Responses fixture. Complete an `update_plan` call, then inject the opaque
400. Recovery must make exactly one further request, preserve the entire
input including the single completed tool result, honor Retry-After, and
finish one turn with the expected answer. Specific provider rejection and
array-shaped responses must end without retry. Every case retains requests,
commands, native events and guardian outcomes, even when an earlier case fails.
The executable hash is recorded and checked again after the run.

Run against a fresh output directory, without building or installing:

```sh
python3 alab/smoke/request_recovery.py --package /absolute/candidate-package \
  --output /absolute/fresh-smoke-output
```

`--codex /absolute/executable` is also accepted. A package's manifest supplies
its source identity; this smoke does not assert build qualification. The
shared fixture accepts an explicit stream-retry count while retaining zero
as its default for existing smokes. Eight unit tests pass, covering valid
and invalid result traces, timing, unchanged default configuration and the
three fixture envelopes.

Run the final smoke against installed signed build `2026111301`, source
`d75c9b47196df14d99247827e7522d1bd29f8a6d`, executable SHA-256
`6bd514a71736c8995c7e9aa79e641d8bccc047ae15b5ae8865dd12172bddcfc4`.
It reproduces the defect after the plan completes: two requests, exit 1,
`turn.failed`, and no retry of the opaque response. Both terminal controls
pass. Treat the overall exit 1 as the expected installed baseline, not a
passing recovery smoke. The positive packaged-binary result remains deferred
until a certified build containing the fix exists. The selected trace files
are retained under this change's local `packaged-opaque-baseline` validation
folder; no live credentials or user conversation were used. No child remains
running after the guardian finishes.


## Reject malformed histories in the packaged smoke

Review found that comparing the failed request with its retry could accept
the same damaged history twice. In particular, the smoke checked the completed
tool result but not its matching call or preservation of the initial input.
Require the original request input as an unchanged prefix, exactly one matching
`update_plan` call and result, and the call before its result. Continue to require
the retry's entire input to equal that failed request.

Seven regression subcases previously passed invalid histories: missing or
duplicated call, wrong tool, mismatched call ID, reversed call/result order,
and missing or changed original prompt. Each now fails the smoke. The test
fixture keeps independent request snapshots, so mutating the later request
cannot silently mutate the original expected prompt too. All nine related
unit tests pass. Running the stricter smoke on the same installed signed
build reproduces the opaque failure and still passes both refusal controls;
this is another negative installed baseline, not a positive recovery result.


## Preserve the smoke summary if its executable disappears

The final identity hash previously raised before writing the overall result
if the executable was removed or became unreadable during the run. Retain
all case results and write an explicit failed summary for that condition,
just as for changed executable bytes. This does not accept a different image
or convert an unavailable identity into success.

A filesystem regression removes the synthetic executable after the final
case and previously reproduced the uncaught FileNotFoundError. Both removal
and content replacement now return failure with the complete summary retained.
All ten related unit tests pass. These are local fixture tests; they add no
positive installed-recovery or application-certification claim.


## Positive retry control in the packaged smoke

Add a 503 overload case using the same completed plan, guardian, input
validation and provider deadline checks as the opaque-400 case. This makes
the smoke verify a known working retry path, so an opaque failure is not
mistaken for an unattainable fixture requirement. The four cases retain
independent homes and traces.

Installed build 2026111301 passes that control: three requests, exit zero,
unchanged original input and ordered plan call/result, one completed turn,
and 2.0056 seconds after response headers before the retry. It still fails
the opaque case after two requests and exit one; the specific and array
refusal controls pass. Ten related unit tests pass. The overall smoke remains
an expected negative installed baseline. The successful overload control
neither identifies the original backend rejection cause nor qualifies a
fixed application build.
