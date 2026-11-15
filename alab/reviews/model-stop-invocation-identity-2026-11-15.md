<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Match native model-stop evidence to its issuing call

After receipt chaining lands at `c313b1753a`, investigate the pending native
marker key separately. It uses host thread, turn and call ID. A nested call's
host-generated UUID normally differs from a direct provider ID, but the host
does not universally reserve that string across the two dispatchers. A native
adversarial fixture will capture the real nested ID and reuse it in the direct
provider response, retaining actual handlers, hooks, SQL and callbacks.

Decision: require that native RED before changing production. Do not present a
controlled collision as an established ordinary provider failure or a new hold
bypass. Keep accepted outcome, mutation epoch, native owner, original source,
person stops, budget, policy and history checks intact.

Both the native extension executor and its normal/aborted completion expose the
same host ToolCallSource. Review is evaluating a source-qualified pending key
against an opaque invocation scope. The former fits existing ALab hooks and
must include the complete nested cell/runtime pair; it must not be described as
a universal non-reusable capability. The latter needs new native lifetime hooks
and must preserve identity into outer cancellation callbacks as well as the
spawned handler. Record the selected contract and its evidence after the RED.

Certified application packaging remains deferred at the existing Trust
proc-macro TCB restriction. No compiler/profile waiver or unverified application
installation is part of this investigation.

The native RED confirms the expected loss in 1.65s: the actual Code Mode host
call and the direct provider call use the same observed ID in one native turn,
both commit and complete successfully, but the original source is absent.
Receipt: `codex-rs/target/alab-goal-full-libs.e9J41A/`; log:
`~/.ubd/codex-goal-invocation-identity-red.log`. Its original
controller exits 101 before production changes begin.

Decision: use an opaque allocation identity for each actual dispatch. Adding
the full source enum would distinguish this demonstrated cross-source clash,
but cancellation releases a direct call's execution gate before its outer
Aborted callback. Already-created queued calls can run in that interval.
FuturesOrdered orders returned outputs, not every callback. A same-source
cancellation collision is source-feasible, not a separately proven native RED;
source qualification is therefore not the universal invocation contract we need.

The ALab-owned identity is allocated once before native dispatch spawn, with
Arc pointer equality and no string-derived constructor. Five marked hooks in
the existing private native dispatcher scope the spawned registry future and
both outer abort notifications with that same token. The goal executor and
normal completion already run inside the registry future, so no GoalTool API
hook changes are needed. This also avoids modifying upstream protocol types.

The pending key requires the opaque token plus the existing thread, turn and
call headers. Unscoped publication and consumption fail closed. A dropped outer
future cannot lend an old marker to a fresh invocation reusing those strings;
bounded retained markers keep the token allocated until consumption or eviction.
This is correlation evidence, not automatic resume authority. Existing outcome,
native receipt, epoch, owner, person-stop, policy, budget and history checks stay.

Before the hooks, delta reports native parallel.rs risk 224 (32 upstream changes
in 90 days) and GoalTool risk 85 (five changes). The selected shape touches the
five lifetime boundaries in one native file and keeps all implementation in
ALab Core. Two read-only reviews approve allocation/equality, poll scope,
aborted completion and missing-scope behavior. Runtime validation follows.

Validation on committed `7562936116e8a04d021d4eba7bb8ca2409a08391`:

- All 26 Core model-goal tests pass in 0.01s, including nine new scope tests,
  fourteen marker/evidence tests and three session-custody checks. Receipt:
  `codex-rs/target/alab-goal-full-libs.yuy5ld/`.
- All 57 native model-block recovery tests pass in 35.14s. The four actual
  colliding-ID cases cover both callback orders, a direct failure before SQL
  that cannot consume the nested marker, and a committed nested post-hook
  interruption. Existing owner, epoch, budget, person-stop and restart controls
  still pass. Receipt: `codex-rs/target/alab-goal-full-libs.KZeZ0Z/`.
- All eleven existing upstream Core parallel-dispatch tests pass in 0.10s,
  including cancellation before admission, completed lifecycle preservation,
  tracing context and work timing. Receipt:
  `codex-rs/target/alab-goal-full-libs.9qVdgU/`.
- Canonical all-target Tippy passes in 7m18s with zero ALab findings and 41
  upstream diagnostics set aside. Certification separately exits 2 at the
  unchanged proc-macro TCB refusal, without authenticated proof inventory.
  No compiler exception or application installation is claimed.
- Post-change delta reports parallel.rs risk 480 (+11/-4 including earlier
  hooks); GoalTool remains unchanged at risk 85. Both production reviewers
  approve the actual committed diff, including both abort scopes. All original
  controllers exit before further composition.

Logs under `~/.ubd/`:
`codex-goal-invocation-identity-{red,core,green,parallel,gate}.log`.

Main advances to `35f846c32c216fa9260faeb0661873f54a006b29` before the planned
full run. Its net change is exactly two Python fixture/test files and their
review record. Independent review finds no Rust, runtime, production checker,
toolchain, profile or gate-policy change and no path overlap. Decision: compose
it and run the full ALab Rust suites on that merged source. Retain canonical
evidence at its actual source above; no canonical repetition is justified for
this test/documentation-only composition. Main's separate Python workflow and
focused evidence remain attributed to their original recorded sources.

Next investigation selected: native forced interruption after a committed goal
stop while an async assistant-item observer prevents the sampling loop from
reaching its tool drain. Ordinary provider-stream errors already converge on
`drain_in_flight`, so they do not establish this gap. Forced task teardown after
the 100ms abort grace can instead drop an outer completion before its callback
is polled. This is source-feasible, not yet a native RED. A bounded fixture
should hold a real item-completed observer plus the goal's real PostToolUse,
interrupt, then check original-source recovery; include normal-release and
explicit API-pause controls. Do not manufacture completion authority or change
drop behavior before that experiment. The opaque token already prevents an
orphaned marker from being consumed by a later reused provider ID.

Full composed-source validation at
`52e700f76fa101e91ad37f546f3a4b18b00b403f` passes:

- Autopilot: 1,051 pass, four ignored, 227.89s.
- Core: all 334 pass, 0.55s.
- TUI: 387 pass, five ignored, 93.07s.

Total: 1,772 pass, nine intentionally ignored, zero failures. Official native
artifact/runtime collection succeeds at
`codex-rs/target/alab-goal-full-libs.kewMrx/`; log:
`~/.ubd/codex-goal-invocation-identity-full.log`. The original
controller fully exits zero. No source changes occur during validation, and
this final evidence update changes documentation only. The Rust implementation
is identical to the canonical and focused-test source `7562936116` above.

No authenticated application certification, package adoption or installation
is claimed. The existing compiler boundary remains deferred, while this
verified source work and the bounded next investigation can proceed.
