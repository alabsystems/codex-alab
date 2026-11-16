<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve specific streamed request refusals — 2026-11-15

Decision: classify only the exact, successfully decoded response.failed code
`invalid_request` as the existing terminal InvalidRequest. Keep ALab activation,
typed validation and code precedence. Do not classify from an error type or its
message text, and do not infer that this separate gap caused the live opaque
HTTP400 incident.

Before this change, the parser sent that specific error through Retryable and
Core's Stream channel. A pending body replaced it with an idle timeout; a later
completion could hide it. The provider's specific request diagnosis therefore
lost its meaning and could produce repeated invalid requests.

A single match arm calls an ALab-owned factory after the existing Error decode.
Nonblank messages are preserved byte for byte. Absent, null and blank messages
use `Invalid request.`. Existing InvalidRequest stream delivery and bridge retry
policy apply without changes. Unknown codes, type-only errors, malformed decoded
fields, inactive behavior and quota/overload classifications retain their paths.

Hostile review found a second composition gap: Autopilot reinterpreted an
InvalidRequest whose message happened to equal the opaque JSON as an outage.
Remove that legacy content-based exception rather than rewrite the message or
invent another marker. The active HTTP400 bridge already normalizes a genuine
opaque backend envelope to its distinct prefixed Stream error. That path retains
paced outage recovery. Existing recorded server waits and their clocks are not
changed. The old raw InvalidRequest row is removed from the recorded-server
matrix; its obsolete assumption is replaced by a direct classify/state/decide
regression.

The tests-only API source 35c430c8f07ccb7c9b24ef10f2176460d13fc40e fails two
required native tests for the intended reasons: Pending returns an idle-timeout
Stream, and a message-less EOF returns empty Retryable. The other two tests
covering inactive behavior and malformed/type-only/code-precedence controls pass.
At 6bb96a57eacfab2fce773e0afd459c9f6e7650f3, all 58 public API contracts and
260 API units pass, with zero ignored/filtered tests. All four new exact names
pass once. All-target scoped Tippy has zero owned, hook-spanning, failed-target,
missing-script or set-aside findings, using its separate maintained cache.

Each positive public case verifies terminal outcome before idle timeout or a
contradictory completion, no tail consumption, body drop, channel closure, one
actual endpoint send, message fidelity, no mapped retry/advice and one safe
semantic diagnostic. Payloads and arbitrary headers stay out of those diagnostics.
These are public endpoint and bridge policy tests, not a real Core retry-loop
execution. The native Autopilot regression additionally checks actual decision
selection for the two distinct error meanings under both compaction settings.

The composition regression fails at 183efc1776a8e3b2908bb9035d9040f3f47c6f31:
its explicit InvalidRequest is incorrectly classified as a recoverable Server.
At 94d3f69204f6b6b832f48d3e5cf478806c4030c8, the complete Autopilot library
selects 1,080 methods: 1,074 pass, zero fail and six remain explicitly ignored.
The ignored identities are two intentional native-failure child fixtures,
write_guard_corpus, land_reads_a_holdout and two opt-in real Cargo-copy proof
fixtures. None is counted as an executed pass. All-target scoped Autopilot Tippy
has zero owned, spanning, failed-target or missing-script findings; three
upstream diagnostics are set aside.

Remote main advanced to f32575b43e5aadaa0d56f75942689a421c6eaa0c with docs,
smokes and a native prerequisite-allocation change. Composition source
3cb349f6a46ee6ebb7e9c50fa85d7f0a873b4077 has identical Rust content to the
tested Autopilot source. Both API suites and their scoped Tippy are rerun
through the merged helper: 318 pass, zero ignored and no lint findings. The
complete 37-test recovery family also passes through that helper, zero ignored,
including the new typed-refusal decision, real opaque HTTP400 recovery and
provider timing/restart cases. Its other 1,043 Autopilot methods are filtered;
the earlier full-library result retains its own source identity.

Native admission initially waits for memory and another tab's timed-machine
reservation. The same existing jobs resume without changing claims, resource
limits, toolchain or another tab's processes. All checks finish before publication.
Independent read-only review accepts the source boundaries, API artifacts,
new control oracles and guide; the final native receipts are retained for review.

The debugging guide also gains a current-history step. Its query is restricted
to a verified unbranched current paginated root; forks/reverts require existing
lineage-aware history resolution. It lists recent statuses together, preserves
error_json and uses a bound rollout ID, read-only connection and private,
non-overwriting output. The exact documented shell passes synthetic checks with
all eight committed migrations and 14 seeded turns: ten-row ordinal ordering,
parameter isolation, escaped/Unicode/null error preservation, current versus
historical status, mode0600 output, existing-file/symlink refusal, missing-file
refusal without database creation, rejected writes and unchanged database bytes.
No private live database is used by this guide verification. An explicit private
supplement clarifies the digest difference between the executed shell argument
and its archived script's terminal newline; the original record is preserved.

Private evidence is under shared Git alab-validation/streamed-invalid-request-20261115,
with exact sources, controllers, native attribution, logs, lint classifications,
synthetic guide records and independent reviews. The final archive indexes hashes
and retains cleanup/publication observations. The original server's opaque
rejection cause remains unknown.

Source-selected Trust15698 runs through the existing qualified project frontend,
protected profile and native unit runner in its explicit unverified test lane.
Both API packages are selected in RED and GREEN to retain the same Hyper feature
graph. No new dependency or application build/install is introduced. The earlier
managed0.9 runtime smoke remains evidence for its own packaged source; it is not
relabeled as delivery evidence for this later source change. The unchanged
application certification limitation remains separately recorded.
