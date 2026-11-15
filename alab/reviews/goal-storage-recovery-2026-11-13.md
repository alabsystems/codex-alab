# Goal storage recovery, 2026-11-13

Decision and implementation are described in
[goal-storage-recovery.md](../design/goal-storage-recovery.md). Work used an owned
stable checkout, private Cargo source cache, the committed Trust 15698 selection,
and the existing containment and resource admission. These tests changed no installed application,
credential, compiler selection, dependency pin, or provider source was changed.

## Actual observations

The bounded SQLite regression first reproduced the unprotected SQLx failure, then
passed against the actual production pool initializer and protected callback.
The actual locked `codex-state` test command also passed both pool tests. The
injection is SQLite's page limit, never actual filesystem exhaustion.

| Check | Result |
| --- | --- |
| Production `codex-state` pool regression | 2 passed |
| Current `codex-alab-core` persistence tests | 8 passed |
| Typed read/write failure classification | 2 passed |
| New native Autopilot persistence regressions | 6 passed |
| Native retained-usage/terminal/Plan regressions | 4 passed |
| Original shared-usage quarantine boundary | 1 passed |
| Existing native goal resume | 11 passed |
| Existing native stop admission | 3 passed |
| Existing native upgrade/goal reads | 10 passed |
| Existing renewal follow tests | 10 passed |
| Production goal extension check | passed |

These commands used the explicit unverified test/check lane. They establish
runtime behavior and source compilation; they are not an application
certification or a package publication result. Normal certified gate and Main
landing remain separately required.

The latest frozen-source focused run passed all 19 selected tests: core 8,
shared boundary 1, new native retained-usage 4, and existing native persistence 6.
Earlier pool/classifier and existing native groups remain separate observations;
the full gate must still judge the final merged Main tree.

The new native regressions use actual Core threads and GoalService/SQLite writes
with private mock model and host servers. Failed accounting leaves the objective,
status and usage unchanged; repeated healthy reads and automatic admission checks
send zero model requests. A failed external pause with no usage delta also holds
admission, and a later successfully persisted explicit action releases it. A stop
queued behind same-ID reactivation cannot overwrite its active status. Standalone authentication recovery also respects a retained hold when the
goal row is absent. Healthy
pre-write validation refusals preserve healthy admission and cannot clear a prior
failed stop.

A real idle accounting write blocked by a scratch SQLite writer also outlives
its cancelled observer, commits its native baseline, and admits exactly one
subsequent model request through the existing paced recovery owner. A healthy
unfinished write is distinct from a failed-write hold.

The retained-usage cases forward actual provider token events and actual native
goal tools through an opt-in fixture. A failed 22-token turn followed by manual
input is charged exactly once before native stop or tool completion; completion
also accounts the new turn's 10 tokens, for 32. The same original stopped or
complete goal recovers its 22 tokens and permits an idle checkpoint. A zero-own-
token failed turn retains its positive shared time, while a delayed Plan response
adds no Plan seconds. Raw replacement, current-turn `Unchanged`, and idle-only
failed accounting preserve or quarantine their original usage without attribution
to a replacement. The shared-boundary test preserves 7 descendant tokens despite
11 later tokens and a raw ID restoration.

Owned-write cancellation and queued ambiguity tests also exercise the shared primitive.
They do not inject a committed-then-lost-acknowledgement error through each of the
three native accounting entry points. Those entry points additionally recheck and
publish ambiguity under the native progress semaphore; this is source-inspected
coverage rather than a claim of that additional fault-injection scenario.

## Retained evidence and failed attempts

Evidence root:
`~/.ubd/codex-root-disk-controls-evidence.noindex`.

| Complete log | SHA-256 |
| --- | --- |
| `sqlite-source-harness-5.log` | `35b4acb6d072c60e7f5818ca58b3ad95d9bff6df806bcdb249f4b9110fa6dce5` |
| `state-core-focused-1.log` | `9f701bc8bfec535d5ef20fa2690c61c8d280d61995debb0d13621449ca9bc240` |
| `goal-native-5.log` | `d6a161a12ba9e1878c0823a5ac0ca678023984bee048b6f3f07d50a1d4a6275a` |
| `goal-regression-2.log` | `4a8a3debb00ef48789702a9dbf6cb5190d2fa541196b749005cf0738b5c0ee85` |
| `goal-retained-native-5.log` | `5609069e7f33233752dc6c309ddc4bd77135b3b6dfe8041368693b86a92fdae7` |

The first canonical attempt stopped in its lint phase on five owned diagnostics;
it produced no application certification verdict. Those diagnostics were fixed
without waivers. Its full log is `canonical-gate-1.log`, SHA-256
`bfcb9c3239869331db855786b9959bf7372876838a69663bf57a4e1c53b0270a`.

Retained-usage attempt 3 failed three native assertions because the historical
upgrade fixture omitted token callbacks and goal tools. Its log is
`goal-retained-native-3.log`, SHA-256
`d962a48eacee8ee001694dcfd3eb7ca667139c52f33569caa4e31eb7854848c7`.
The opt-in fixture now forwards the production contributors without fabricating
usage; its static enabled setting and inner event sink do not establish general
configuration or event-delivery fidelity. Attempt 4 passed all four new native
tests but exposed a real cancelled-observer liveness regression: an ordinary
preparation guard spanned the owned write and manufactured a failure hold after
cancellation. Its log is `goal-retained-native-4.log`, SHA-256
`49ae019cdfa7cdd37446636bd181160765933f243487ba2922a15010fdbc570f`.
Validation now finishes before additive dispatch under the native permit; the
owned task alone finalizes the write. The existing cancellation test observes a
registered additive owner rather than generic validation `Writing`, retaining
its 60-second detector and exact one-request assertion. Attempt 5 passed all 19.
Source fingerprints verified that each running attempt's files stayed unchanged.
Printed fixture roots are command-local scratch removed by normal containment;
the full logs, receipts, source manifests and patches above remain preserved.

Earlier retained attempts stopped before compilation on missing exact Git sources,
containment output-path mistakes, or new-test compile mistakes. The first broad
regression attempt used the tool shell's soft file-descriptor limit of 256 and
unbounded default test parallelism; it failed fixture setup with `EMFILE` and
subsequent hang detectors. Repeating the same source with the canonical gate's
soft limit of 65536 and four test threads passed all 43 selected regressions.
No assertion, timeout, production gate, or containment rule was weakened.

The source-only acquisition completed normal locked offline Cargo fetch without
URL mappings and with its original lock SHA unchanged. The private completed cache
and exact dependency provenance are recorded in the source-acquisition handoff;
this is dependency preparation, not compiler or application qualification.
