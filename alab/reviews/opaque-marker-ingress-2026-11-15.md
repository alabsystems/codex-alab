<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Reserve opaque-HTTP recovery provenance at stream ingress — 2026-11-15

Decision: reserve the internal opaque HTTP 400 prefix at all raw API-to-core
Stream conversion arms for the lifetime of an ALab process. Only a string that
starts with that reserved prefix gains a fixed `provider stream: ` label; retain
the complete original text and already captured advice. Genuine HTTP 400
normalization remains the trusted producer and bypasses this boundary.

An HTTP 200 response.failed event with an unknown code can supply the exact
internal prefix plus an opaque JSON body as its message. The shared parser
returns Retryable and the bridge previously preserved that text as Stream.
Core's clean-transport selector and Autopilot's non-cutoff opaque outage branch
both use the exact marker predicate. Without provenance discipline, provider
text could select that meaning despite no HTTP 400 transport result.

One stable hook at the private conversion entry calls owned code covering five
raw variants: Retryable, Stream, Network, Build and untyped RateLimit. It covers
both the public caller and existing HTTP-terminal-parser recursion. All selection
and encoding stay in owned glue. Typed
errors, ordinary text and advice capture/reattachment remain unchanged. An
already labeled string stays unchanged; the entire prefix namespace is reserved,
even when a suffix does not currently parse as an opaque body. No JSON parsing,
clock sampling or raw-content logging is added by the encoder.

Reserve by is_alab_binary, not live active/enhanced mode. CLI startup activates
identity before provider requests; the flag has no shipping reset. Enhanced mode
can change while a mapped error is queued. An active-only guard would let vanilla
text gain marker meaning after a mode change. Reserved-prefix collisions thus
gain the label even in ALab vanilla mode; ordinary vanilla strings and genuinely
unactivated upstream behavior remain exact. Genuine vanilla HTTP 400 behavior
is unchanged. Source documentation records this product invariant explicitly.

A first-class provenance variant would be more universal but would require
protocol/error representation and consumer integration. The current bounded
graph has one trusted producer and complete raw-string ingress into two exact
consumers. Disjoint encoding at that boundary closes the demonstrated collision
without a new error kind. Kimi/Bedrock stream mappings delegate to the bridge;
other Core Stream constructors use fixed local text. Image generation's separate
raw Stream is tool-local and does not reach these recovery consumers. This is
not a claim to sandbox arbitrary trusted Rust constructors. Future raw ingress
or consumer normalization must preserve the same reservation invariant.

Tests-only source 2426b9baa8228acfc4317fe1b4ee35a6a74c6bed reproduces three
failures: of 350 outer methods, 347 pass, three fail and none are ignored.
The enhanced bridge test first proves the genuine status-bearing HTTP 400
fixture still normalizes with its captured seven-second advice, then the first
raw Retryable imitation is wrongly recognized as internal. The mode-transition
test proves vanilla HTTP 400 remains InvalidRequest, maps five values while
vanilla, enables enhanced mode, then fails at the first held marker. Later
active and queued assertions are GREEN coverage. All 25 unactivated mappings pass.

The public HTTP 200/SSE unknown-error fixture reaches its final display mismatch
after checking original ApiError::Retryable text, captured deadline, no HTTP
status, ordinary protocol identity/retry, matching attempt/thread/turn,
metadata privacy, one custom endpoint send, body drop and channel closure.
This is an in-memory transport fixture, not a real HTTP/TCP or WebSocket run.
It does not execute Core's transport selector or an Autopilot recovery action;
the unchanged consumers use the exact predicate tested at the bridge boundary.

Initial implementation source 414bc79d673dff750ed8f1874373f4e742c82695 passes all
350 runtime methods but fails the scoped lint verdict on one collapsible match.
Its controller exits 1; it is not accepted as a full passing run. Keep original
green log/controller/native paths untouched. Move the nested prefix condition
into the match guard without suppressing the lint or changing behavior. The
corrected run uses distinct green-final paths to preserve historical references.

Final source 6f468e2d55d3b23b859e888ae6b08a905bde6604 passes 79 public contracts
plus 271 API units: 350 passed, zero failed/ignored/measured/filtered. All four
required method identities pass exactly once. Scoped all-target Tippy succeeds
with zero owned, spanning, failed-target, missing-script or set-aside findings.
The test sections remain byte-identical from RED through the final run.

The owned tests cover five raw variants against five message forms in enhanced
and unactivated processes: exact imitation, ordinary text, reserved prefix with
an invalid suffix, already encoded text, and a nonleading prefix. Five errors
held across vanilla-to-enhanced activation remain encoded with original advice.
Genuine enhanced/vanilla HTTP 400 controls retain their different existing
policies. The encoder does not alter the API error kind, status, HTTP body or deadline.
This verifies current boundary semantics and source composition, without claiming
a universal type-level provenance guarantee or modifying restored history.

Both phases and final correction use source-selected Trust 15698, the qualified
native project frontend/protected profile and native unit runner in the explicit
unverified runtime lane. Independent reviews bind sources, actual library/runner
receipts, current artifact snapshots and separate-target lint scope. One marked
upstream hook is added; delta topics were inspected before and after, and the
upstream file is not formatted. Private evidence is retained in the shared Git
alab-validation/opaque-marker-ingress-20261115 directory.

The guide records label, mode and authority limits. The source-selected compiler,
qualified native project frontend and explicit unverified runtime lane remain
unchanged. No actor policy, protocol schema, new application certificate, package
or installation is claimed. The recorded application-certification prerequisite
and original unknown opaque HTTP 400 backend cause remain unchanged.

Publication initially refused before any remote write because main advanced from
257042ffca82669dbb5ae6cdef3b321befc7b04d to
17c687c949e098ec64de426a63ea7413e81c9c15. Preserve that attempt's raw outputs and
controller status. The peer adds two Autopilot integration regression files and
their review; it changes no production, manifest, runner or compiler selection.
Merge both histories without rewriting either. All three peer files remain
byte-identical, and all six own paths match the premerge candidate.

Merged source 7234368374833fab65dbb138d725a659387c7b15 reruns the same two API
libraries: 79 plus 271 methods, all 350 passed with zero failed, ignored, measured
or filtered. The four required regression identities each pass once. Scoped
all-target Tippy again has zero findings in every classification. Independent
artifact review binds the merged source and current native receipts/snapshots;
independent source review confirms both parents and byte-exact composition.
These API selections do not execute the peer's Autopilot integration tests.
The final post-run change records only this decision and verification evidence.

A later fresh fetch observes main at
4f1e18fadbb24cfe745ec183e78e062143d8a63b, including peer terminal/mail and goal
recovery changes, delta tooling and intake records. Preserve all 28 incoming
paths byte-for-byte and merge again; the six own paths do not overlap them.
Merged source 918f65b44dd8378c2ae9a6b918fc56b876083054 passes the same 350 API
methods and all-target scoped lint with zero findings. Separate merged-forward
evidence paths retain every earlier run. This selection verifies API behavior
on that composition, not the peer suites or a certified application build.
The API compiled sources remain unchanged after the final runtime source.

The tooling-only incoming commit at 09a96ca9ff58d5dc275d7f77904b747257f96379
changed only delta-report tooling, its Python tests and its review. That phase
preserved those three paths exactly and kept the complete codex-rs tree identical
to tested source 918f65b44dd8378c2ae9a6b918fc56b876083054. Repeating the API run
then added no coverage; its exact runtime attribution was retained.

Another compare-and-swap attempt refuses before writing as main advances to
6554574fc64134b48c694ed9505f7016883fa6a8. Preserve the six incoming paths,
which add startup-record custody behavior/tests and ownership/review records.
Their Rust changes are confined to alab-tui, outside both selected API packages'
local path-dependency graph and every workspace local patch. All 28 graph
directories plus alab/vendor/allocative remain byte-identical to tested918;
compiler, manifests and runner inputs are unchanged.
Retain the existing API runtime attribution without claiming TUI execution.
