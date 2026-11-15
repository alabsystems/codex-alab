<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Retain committed native goal completion through forced sampling teardown

The next experiment selected at `c2809f52f9` reproduces the lost original source.
An actual Responses stream dispatches native `update_goal(Blocked)` and then
an assistant item. A held async item observer prevents the sampling loop from
reaching its normal tool drain. The real PostToolUse hook independently proves
SQL committed before an actual Interrupt forces sampling-task destruction after
Core's 100ms grace period. With production unchanged, tool outcomes are empty
and the original committed model-block binding is absent. The original
controller finishes101; log:
`~/.ubd/codex-goal-forced-teardown-red.log`.

Decision: preserve the existing outer completion future when its waiter is
dropped. A new ALab Core helper transfers its intact pinned state to an owned
runtime task and cancels only that dispatch's child token. Do not send a
replacement notification from Drop: `terminal_outcome_reached` claims the
native notification before sequential observers finish. The original future
already arbitrates normal completion, finishes-on-cancellation, abort+join,
invocation identity and the sole fallback outcome. Replaying it from the start
or trusting the claim flag as delivery would duplicate or lose observers.

Register thread ownership before ID reservation or eager dispatch. Closing the
thread fences admission before producer shutdown; drain after producers stop
and before goal runtimes/persistence writers retire. After destroying and
joining an interrupted sampling task, wait only for its abandoned completions.
Intentional live CodeMode waiters can still outlive their originating turn.
The already-cancelled abort branch explicitly joins its task before that drain;
a cancelled token alone never proves future destruction or callback delivery.

New logic lives in ALab-owned helper/glue files. Small marked native hooks cover
dispatch preparation/retention, interrupted-task drain and thread shutdown.
Only native default-namespace goal tools are retained, based on product identity
rather than mutable Enhanced mode, so an experience toggle cannot orphan an
admitted call. Other tools keep their existing cancellation domain and wait
semantics. Goal completion remains evidence, never continuation authority:
actual receipt/epoch/owner, history, budget, policy and human holds still govern.

Native fixtures include forced teardown plus real original-goal continuation,
normal release, an explicit API pause after commit, and actual thread shutdown.
The forced controls release only the native hook after witnessing the item
callback's Drop; the sampling observer remains held, proving forced teardown.
Shutdown requires exactly one aborted tool finish with original source accepted
before owner retirement, and no additional model turn. Generic helper tests
cover partial polling without replay, normal disarming, never-polled drop,
closed admission, turn versus thread ownership, cancellation of a drain waiter,
Drop outside the runtime thread, and output destruction before barrier release.
Runtime validation and exact receipts follow in a subsequent record.

Before the new hooks, delta at `c2809f52f9` reports parallel risk480, task abort
risk800 and session handlers risk602. No upstream formatter or protocol change
is used. Certification and binary installation remain deferred at the existing
Trust proc-macro TCB / authenticated inventory refusal; no waiver, compiler
substitution or unverified application installation is authorized or claimed.

Before validation, remote main advances to `19dbb7cc11` with the background
terminal wall-handoff change and its records. It changes Autopilot runtime code,
so compose it before running the new native fixtures and full Rust suites.

Initial composed source `d78ce50a60e7fbf63d13aedcdf6c4f89d154da7d` passes
all nine generic ownership tests (0.00s; receipt
`codex-rs/target/alab-goal-full-libs.6OhuCv/collected.json`) and all 61 native
model-block recovery tests (34.81s; receipt
`codex-rs/target/alab-goal-full-libs.ap3fMx/collected.json`). The native cases
include all four new forced-teardown controls and the existing owner, epoch,
person-stop, budget, reused-ID, restart and overlapping-dispatch controls.

Canonical all-target Tippy completes compilation in 7m58s but rejects six new
production `expect()` calls in the helper. Controller and gate exit1; certified
build is not reached. Decision: remove the invalid helper state combinations and
manual count arithmetic, rather than suppress the lint. This source is not
qualified by that failed gate. Both original controllers finish before edits.

Read-only review finds a fixture interleaving despite the initial green run:
the native runtime has two workers, so opening PostToolUse immediately after
the item callback's Drop can let normal completion race cancellation. Tighten
the test by keeping both gates held until the actual Aborted finish is observed,
then release the hook as cleanup insurance. Native hook cancellation kills its
process group and does not require opening that file gate. This is correction
of a test synchronization race, not another observed production loss.

The independent inventory check also rejects two unowned topics: the earlier
`goal-calls` and the new `goal-completion`. Assign both to `native-session`, with
their actual Core implementation paths and invariants/scenarios. This metadata
does not establish behavioral qualification. Reuse the existing private session
glue anchor: it already hosts transfer, ephemeral-history, idle and quota
continuity adapters, and goal completion depends on no serialized exec setting.
No new upstream module declaration is needed merely to improve a private name.

The corrected helper couples future, cancellation closure and lease in one
optional pending state. Completed polls remain inert. Counter ownership uses
one shared Arc registration token: only actual noncloneable leases clone it;
owner/tracker clones share the Counter instead. Admission stays locked through
lease acquisition; Drop releases the token before notification. This removes
manual count overflow/underflow and impossible-state `expect()` paths, without
saturation, a fake completion result or a lint waiver. A tenth test checks
post-completion polling cannot replay cancellation or create a registration.

Completed validation on corrected clean
`1a3c0af67138a52994ba6a61e5fdfb72a793efb9`:

- Ten retention-helper tests pass in 0.00s; receipt `codex-rs/target/alab-goal-full-libs.HuSVn7/collected.json`.
- All 61 native model-block tests pass in 34.51s, including the deterministically held forced-abort, API-pause and shutdown controls; receipt `codex-rs/target/alab-goal-full-libs.OPZuol/collected.json`.
- Existing upstream Core parallel-dispatch tests: eleven pass in 0.09s; hard-abort timing regression: one passes in 0.16s. Receipts: `codex-rs/target/alab-goal-full-libs.1NBZ2y/collected.json` and `codex-rs/target/alab-goal-full-libs.1JD62I/collected.json`.
- Full ALab suites: Autopilot 1,057 pass/four ignored in 215.71s; Core 344 pass in 0.54s; TUI 387 pass/five ignored in 97.15s. Total 1,788 passed, nine ignored, zero failures; receipt `codex-rs/target/alab-goal-full-libs.9SiaQY/collected.json`.
- Canonical all-target Tippy passes in 7m38s with zero ALab findings and 41 upstream diagnostics set aside. Certification then exits2 at the unchanged in-process proc-macro TCB refusal; no authenticated TRUST_JSON, coverage summary or terminal target inventory exists, so no application proof count or qualification is claimed. Report: `codex-rs/target/alab-certified.UQmQO6/report/`.
- Inventory covers 91 hook topics in fourteen features; all 27 inventory tests pass in 9.597s. Independent review approves exact committed lease ownership, notification ordering, pending-state transfer, native barriers, fixture gates and inventory.

Logs: `~/.ubd/codex-goal-forced-teardown-{helper-final,native-final,parallel,abort,full,gate-final,inventory,inventory-tests}.log`.
All original controllers exit before composition. The maintained collector is
run explicitly for the local helper/native/full reports after exit; the shell
controller retains raw logs, artifact plans and per-library runtime receipts
but does not itself materialize `collected.json`. Earlier overlap/identity
records that named an absent collector file now point to their actual retained
state directories instead. Existing collector files and historical outcomes
remain unchanged.

Remote main subsequently advances to `1ed79bc39c785a82e1cac328dc7aceb518adb8f6`
with C2 account-handoff completion reporting. Review finds no ToolFinish,
ThreadStop, goal epoch or source-adoption change; normal on_turn_stop adds its
separate stop verdict before existing acknowledgment/finish. Core, TUI and
Core dependencies are unchanged. Compose it, preserving both native-session
inventory scenario additions, then validate the actual merged Auto library,
its integration suite, TUI, inventory and canonical gate. Existing Core11+1
results retain their exact source attribution above.

The composed run at `2e94fec2ad8ac8afd2b3fe17f9778d7e3288f528` exposes
one existing crash-fixture failure: the exact Retry-After lower-bound assertion
at crash_wait_tests.rs1191. Its Autopilot result is 1,062 passed/one failed/four
ignored in 204.93s; Core still passes344 in 0.56s and TUI passes387/five ignored
in 88.48s. The actual failed report is retained at
`codex-rs/target/alab-goal-full-libs.Tc2BFV/collected.json`, collected with101.

Independent source review finds that the fixture captures with its injected
synthetic boot clock, then restores from a newly manufactured wall-time sample.
Those axes need not agree, especially when scheduling separates the original
wall and monotonic seed reads. Production capture/restore already uses the
correct sample ordering and conservative future-deadline rounding. Decision:
retain one fixture reader across capture/restore, deliberately offset its boot
axis by30s, and preserve the exact act/deadline assertions. No production pacing
rule or test tolerance changes. All ten crash-wait tests then pass in 0.22s;
receipt `codex-rs/target/alab-goal-full-libs.necdpc/collected.json`.

Full Autopilot integration at2e94 completes with158 passed/four failed in
867.95s. All eleven new C2 completion-reporting cases pass. Every failure is
terminal_wall's setup waiting for a background process, before its handoff
assertions. Failure-only diagnostics now expose the actual native events and
provider tool output without changing its timeout, shell flags or permissions.
An exact-one diagnostic on `402b6c1a7eded03d79f02301654c614f6f3f8a9c` confirms
native exec_command routes correctly but returns71 with
`sandbox-exec: sandbox_apply: Operation not permitted`. It exits101 after60.26s.
This disproves the missing-helper/tool-namespace guesses. Defer qualification
of those four terminal cases under this protected lane; do not remove or bypass
the native/outer sandbox to obtain a green test. Full and diagnostic evidence:
`~/.ubd/codex-goal-forced-teardown-integration-composed.log`,
`~/.ubd/codex-goal-forced-teardown-terminal-diagnostic.log`, and
`codex-rs/target/alab-goal-integration.86XBWn/` / `alab-goal-integration.KGOTyU/`.
These failures remain failures, not passing integration evidence.

The person's subsequent instruction is to pull and bidirectionally align.
Fresh Agh main is `779eb1cd325b3ab00cf8448598683bfb4e84bcea`. The maintained
fetch and fast-forward check confirm checkout main already matches it. Compose
its four commits (Python timeout-evidence tests and their review only) with our
committed clock/diagnostic fixes. This creates clean402b6c above, ahead of main
with no missing remote commits. No upstream OpenAI push, force-push, source
compiler override, profile change or application installation is used.

Final verification on clean402b6c:

- Full Autopilot library passes1,063 tests/four ignored in221.91s, including all61 native goal-recovery cases and the exact crash deadline controls. Official collector report: `codex-rs/target/alab-goal-full-libs.okaOWn/collected.json`; log `~/.ubd/codex-goal-forced-teardown-auto-aligned.log`.
- Core344 and TUI387/five ignored retain the successful2e94 results above: no Core/TUI or production dependency source changes after that run. Together the library components pass1,794 tests, with nine ignored. Do not describe the separate integration run as all green.
- Canonical all-target Tippy passes in50.20s with zero ALab findings and41 upstream diagnostics set aside. Certification independently exits2 at the same proc-macro TCB boundary, with no authenticated proof inventory. Report: `codex-rs/target/alab-certified.a2VUQb/report/`; log `~/.ubd/codex-goal-forced-teardown-gate-aligned.log`.
- Inventory on2e94 covers91 topics/fourteen features and all27 tests pass in7.488s. The later remote pull changes no catalog or production Rust. All controllers fully exit before this documentation update.

Decision: publish the tested goal-completion source and the fixture/evidence
corrections as a fast-forward over the exact remote main, then update checkout
main to the verified remote result. Certified packaging and the protected
lane's nested-sandbox terminal qualification remain unavailable operations,
recorded above; they do not justify abandoning source work or asking a person
to approve our own reversible changes.

Next selected improvement is a bounded native partial-delivery experiment.
Register a fixture ToolFinish observer before Autopilot, hold it after the native
registry claims completion, and independently hold a later streamed item.
Force sampling teardown, then release only the finish observer. Require one
unchanged Completed outcome, the same actual invocation identity before/after
its wait, exactly one post-Autopilot callback, original goal/usage preservation,
and one goal-triggered continuation. Include normal release and actual API
pause controls. The retained future is designed to satisfy this already; this
is a missing native adversarial check, not a newly proven production failure.
