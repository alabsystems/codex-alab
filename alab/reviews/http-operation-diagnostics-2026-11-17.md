<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Per-request HTTP operation diagnostics — 2026-11-17

Complete HTTP rejections already have local body-capture INFO records with status
and safe response IDs. Prepared-request shape and selected endpoint failures
also exist. They do not connect each completed operation index and its recovery
to the immutable request identity, particularly for non-HTTP and pre-wire errors.
The retry macro's TRACE target is disabled by the default SQLite filter; adding
its caller's module name to a reader allowlist would not expose those records.

Decision: add a per-invocation observer around the cloned request telemetry for
Responses HTTP setup. A private optional diagnostic parameter and callback hook
connect the existing Responses caller to an ALab-owned adapter; the shared
session observer is unchanged. Calls without a captured RequestDiagnostic return
the same optional observer without an adapter allocation, including when the
mode changes after opening.

The original callback receives its unchanged index, status, borrowed error and
duration in its ambient subscriber/span first. Only the new local INFO emission
restores the captured request Dispatch and Span. The adapter owns immutable
per-call state, avoiding ambient lookup or shared request slots during concurrent
calls. An operation may terminate during authentication, source admission or
transport; the event reports that result, not completion of all three stages.

The fixed target is
`codex_api::endpoint::alab_request_diagnostics::http_operations`, with message
`Responses HTTP operation completed`. It reports every failure and successes
whose zero-based operation index exceeds zero, preserving quiet initial success.
An operation can fail before a wire send. A successful result means HTTP setup
returned a stream, not successful SSE completion or a final turn verdict. Neither
the index nor advice presence predicts another retry or counts wire sends.

The new local operation logger emits only request identity, fixed typed labels,
supplied duration, typed status/advice presence and bounded named error-header
IDs. It does not format error text, bodies, URLs or arbitrary headers, read retry
clocks or parse retry values. The original caller observer retains its original
error and behavior. Success inherits no prior error IDs or advice. The reader
admits the exact new target and still projects metadata only; it does not parse
or pair operation fields. Payloads, auth/source policy, returned errors and retry
selection remain unchanged.

Public controls are designed to check recovery callbacks and original typed
values before new rows; distinguish captured and ambient contexts across
shared-client calls; and cover quiet first success, captured absence, no inner
observer, auth/source rejection before transport and cancellation without a
fabricated result. The source-policy case is controlled AuthProvider resolution,
not native credential admission. Two GREEN-only producer units cover exact
borrowed arguments, callback order/context and optional Arc identity. Python
fixtures cover literal target admission through API/CLI, exact scopes,
body-free projection, read-only access, privacy and bounds.

The initial candidate `24bbeae58915646f08ebafd41920ae988fdeede3` was
qualified but remained unpublished. Its guarded publication stopped on changed
protected inputs, including `alab/cli_status.py`, before any composition or
CAS/push write. Its source, raw results and rejected attempts are preserved in
snapshot SHA256
`49b59a898156a29c1460aec9bb5302d4e0e3bd73fdabaf6d78f16e74c9a580a4`.
The following results are historical and apply only to their original sources
and helper inputs.

At historical RED source `f5db0499c2e28b087f86a019f0c100b4a02c0bdd`, the
native wrapper exited 101: all 390 baseline methods passed and only the three
new public methods failed, after their late markers, on missing operation rows.
Ignored, measured and filtered counts were zero. The same-source Python runner
exited 1: 39 passed and only the two new target-admission methods failed, without
errors, skips, expected failures or unexpected successes. These results show
missing requested evidence, not a changed recovery policy.

At historical GREEN source `e0185016740c49c8eabe9911fea2f25b8146c0b2`, all 395
methods passed: 110 public and 285 API library methods, with zero failed, ignored,
measured or filtered. The two direct producer units were GREEN-only and included
in the 285. Scoped all-target Tippy exited 0 with normal and test profiles for
both packages, 657 canonical existing artifact paths and no gating findings.
The same-source Python runner exited 0 with all 41 methods passing and all other
result categories zero. The guide and record in candidate `24bbeae5` were the
only changes in this topic after that historical runtime.

Historical independent qualification and native capture SHA256 bindings are:

| Phase | Qualification | Native capture |
| --- | --- | --- |
| Native RED, f5 | `37fc5e0bbc89d657be374dbffb5e85934362510186a3e6014c52df511bc644f8` | `1708f75b2810e897f07ef3e4b0cd3629687ddf04e131db0643df81af46ee0667` |
| Python RED, f5 | `e427a2364aaeecc0123139b9adb95442e6716249f3215f8eda7a08261f43cb88` | — |
| Native GREEN, e018 | `c9117aed928eacbaf46de6ac9e2cf7795ab53c715e65dc78eeb0cd2ca97bfaa2` | `6bc3601382b31d9db8121c1b5b75f0db30ccf6bbab46ae7f919c22924242866f` |
| Python GREEN, e018 | `2b22c4c7b993d687d2ce9af13d2de86d2dc65641a6bbbdf0e0bc0354352064c5` | — |

The guarded stop required a fresh integration rather than transferring that
paired comparison to a new main baseline. Current runtime
`c79575c67610dc571cec981a5fdd1c6f007653b0` merges runtime `e0185016` with
reviewed main `9c005ceecd69ed2443e49e8feccc92c590b4cada`. All 151 incoming
paths are byte-identical to that main, including 12 changed protected inputs;
none overlaps the topic's ten intended paths. The eight topic runtime paths
remain byte-identical to `e0185016`. Original own commits remain retained. The
prior candidate's documents were excluded from this runtime so fresh checks
could precede their integration. Preserving incoming bytes does not qualify
those peers' Rust, TUI or application behavior.

Fresh timeline qualification at `c79575c6` exits 0 with all 41 methods passing;
failures, errors, skips, expected failures and unexpected successes are zero.
Fresh helper verification separately runs ten controlled fixture modules with
107 distinct passing methods, no nonpass methods and ten original exit statuses
of zero. These are actual retained NamedResult inventories, not inferred AST
counts. The helper scope includes local Git repositories and controlled process,
file-descriptor and cleanup effects; it does not invoke live GitHub writes or a
Rust application build. Neither count is aggregated with native library counts.

| Current c795 phase | Actual result | Accepted qualification SHA256 |
| --- | --- | --- |
| Timeline Python | 41 passed, all other categories zero, exit 0 | `2dbd4907412883855f72e55cfc6f10f145b124ca0b4f0d0e52e82b91f69e7357` |
| Controlled helpers | 107 passed across ten suites, no nonpass methods, all exits 0 | `a9d0e80a34c2cb45d656c4ee33ed37b7cb95a17b713539b62bdb3e815df4b820` |
| Native libraries | 395 passed: 110 public and 285 API, all other categories zero | `6b1117d38f14ac6d40272dbb8c5eb95bc2ff5506d23cb9a0a77f14d20ee45e56` |
| Scoped all-target Tippy | Exit 0, both packages in normal/test profiles, 657 canonical artifact paths, all gating findings zero | Same native qualification |

Current accepted native capture SHA256 is
`90957f05a8b4af32c8774c1180cb3292502972698b9bc926a939ae89d31b1ff2`.
The two direct producer units are included in the current API count. No new RED
was executed against main `9c005cee`, and no historical RED/GREEN outcome is
relabeled as a paired comparison against that baseline.

The five runtime controls received pre-use review SHA256
`7b88a90e19de5d379f67f6454c59b68f5c5e60bccd3c9f68c05564e323847ee0`
before current runtime checks. The complete nine closing controls received
separate pre-use review SHA256
`429e5beb1df04781deac25fa24b48c212a0248859c18e644804afdea975add5c`
after the runtime checks had begun and before closing-control use. This later
review is not represented as pre-use review before the earlier runtime runs.

The public and Python tests remain byte-identical to the historical qualified
sources. The three public methods run strict children: parent completion markers
and source flow establish their asserted row reach, without retained successful
per-operation transcripts. Rows are not additional outer methods. The new row
oracles directly assert HTTP, network, timeout and policy labels. The remaining
fixed labels and extreme-duration clamp are source-reviewed rather than all
independently asserted as new rows; the private units use custom subscriber
stacks, not an arbitrary two-Registry interoperability guarantee. The initial
native tracing-fixture failure and observer-rejected attempts remain excluded
and preserved in the historical snapshot.

After accepted fresh native qualification, candidate `24bbeae5` was merged to
retain all original topic commits, its guide was kept byte-identical and this
record was amended for the integration. The guide and amended record are the
only changes in this topic after qualified runtime
`c79575c67610dc571cec981a5fdd1c6f007653b0`; all eight runtime paths remain
byte-identical. The topic changes ten paths against main `9c005cee`. No future
composition, publication or additional execution is credited here.

These local library/tool checks do not establish a request-generation defect or
the cause of the original opaque HTTP400. They qualify no application, provider,
native source registry, tool effects or complete operation ledger. Certified app
delivery, managed-runtime adoption and existing human holds remain unchanged.
