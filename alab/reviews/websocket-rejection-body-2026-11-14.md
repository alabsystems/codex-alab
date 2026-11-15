<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Retain rejected WebSocket upgrade bodies across transport reads

The locked Tungstenite client returns an HTTP rejection as soon as its response
headers parse. It stores only the bytes already read after those headers and
drops the stream. If the entity arrives in a later transport read, the API sees
an empty or partial body. The strict opaque-400 matcher then cannot recognize
the recoverable envelope; specific refusal details can disappear too.

## Evidence and causal limit

At `966b4458a9ed6fe5576ebb4ddbef46f00920f1dd`, a real public Responses
WebSocket client passes the complete-response baseline for 400 and 403. Both
split-response regressions fail with `Some("")` rather than the complete original
body. The server gates headers and entity separately; the current-thread client
is polled on a fresh header-read wake before the entity can be released. This
is an explicit read-poll barrier, not an assertion about TCP acknowledgments.

The retained RED run has one passing test and two failed body-equality assertions.
It uses source-selected Trust 15698 in the explicit unverified test lane. The
fixture preserves an early client result and tolerates its closed connection,
so a server write failure cannot replace the actual empty-body observation.

This establishes a current client reception defect. The original incident has
no retained wire trace and cannot be attributed to response fragmentation. It
does not establish corrupt request history or repair a backend rejection cause.

## Decision and reason

Wrap the plaintext stream after target TLS with ALab-owned bounded rejection
capture, then delegate WebSocket validation and negotiation to the original
library. Preserve successful 101 headers and buffered frame bytes exactly;
never recreate an established WebSocket or copy its protocol parser. Reuse the
current proxy resolution and TCP helpers, including default routing, explicit
tunnels, Windows proxy recovery, TCP_NODELAY and Happy Eyeballs.

Collect only complete HTTP entities under explicit byte, framing and read
bounds, with a five-second absolute deadline for the newly added body work.
Retain the existing header attack limits and leave header timing to the caller;
the provider's configurable connection timeout must not acquire a shorter
transport cap. Honor Content-Length, chunked framing and EOF framing.
An ambiguous, malformed, truncated, oversized or timed-out entity is unavailable;
never expose a partial JSON prefix as a complete backend envelope. Retain the
original status and headers. Typed policy denial, source revocation and outer
connection timeout retain authority over capture.

Capture provider advice when rejection headers arrive and carry it through a
typed response extension. The API mapper uses that captured value, including
captured absence, rather than restarting relative advice after body collection.
Keep explicit Rustls configurations unchanged. The default TLS branch uses a
native-roots-only helper matching the pinned transport; a custom-Codex-CA helper
would change that mode's trust contract.

Record a local INFO event for rejected HTTP upgrades with capture outcome,
status, recognized advice and sanitized upgrade IDs. This event has no prepared
model request, so omit request attempt, payload bytes, model, input and tool
fields. Explicit handshake probes can emit the same event; it does not imply a
model request. Do not log body text, URL, credentials or arbitrary headers. Earlier
admission/authentication/TLS failures, successful upgrades and outer cancellation
are outside this narrow event.

The protocol correction applies to all callers; the existing ALab activation
switch still controls opaque-error recovery classification and local metadata
events. No dependency identity or lockfile change is needed. Upstream changes
are thin import, module, type, getter and advice-mapping hooks; behavior remains
in ALab-owned files.

## Validation and delivery boundary

At checked source `6c143c21f86946aa35d276dae1d031eac639014e`, all 445
native library tests pass with source-selected Trust 15698 in the explicit
unverified test lane:

| Library | Passed |
| --- | ---: |
| API contracts | 10 |
| API | 258 |
| HTTP client | 137 |
| WebSocket client | 40 |

The formerly failing split 400/403 controls now retain complete bodies, original
status/headers/URL, safe diagnostic fields and header-time provider advice.
The delayed-body control advances the clock three seconds before body release;
its deadline stays inside the original header receipt window. Real WSS controls
verify capture above target TLS, unchanged explicit ALPN configuration and the
native-default trust mode using `SSL_CERT_FILE` while ignoring an invalid
`CODEX_CA_CERTIFICATE`.

Framing controls cover complete, chunked and EOF entities; partial opaque
prefixes, invalid/ambiguous framing, unsupported coding, byte/metadata limits,
absolute deadlines and typed policy I/O denial. Successful 101 controls retain
original verification and frame bytes. Review caught an unintended 15-second
header timer before landing; it was removed. New paused-time controls preserve
a valid upgrade after 20 seconds under a 60-second caller budget and give a
late rejection its full five-second body budget. Header attack limits remain.

Scoped all-target Tippy for the same four crates passes with no ALab, hook,
failed-target, missing-script or upstream findings after `alab/lint.jq`
classification. The final run compiled tests in 2m26s and lint in 1m58s; runtime
was 0.30s, 1.84s, 2.10s and 0.13s for the four suites. These are observations,
not a controlled performance comparison. The earlier fixture type/style issues
were corrected and their logs retained separately.

Independent source reviews found no blocking issue in framing, policy
precedence, successful upgrade transparency, the native trust helper or the
manual-poll/clock controls. Committed feature ownership validates. Every
upstream edit is a marked hook; dependency identities, `Cargo.lock` and
`trust.lock` are unchanged. Exact-source hashes, RED/compile/earlier pass logs,
final checks and delta observations are retained under `.git/alab-validation/`.

Incoming main `7042fc731153c19358c15ef5a4d786bc61ad9ccc` adds independent
Autopilot/aterm credential handling. An independent dependency and embedded-input
audit excludes all 29 incoming paths from these four suites' 22 local packages.
`CONTRACT.md` is not a compilation input here. After merging, the complete
selected package paths, embedded defaults/fixtures, workspace configuration and
compiler pin remain identical to checked source. No repeat run is warranted by
that integration; its exact comparison is retained with the validation record.

The application certification prerequisite remains separately deferred at
selected Targo's procedural-macro graph admission. No certified package or
installed application update is claimed.
