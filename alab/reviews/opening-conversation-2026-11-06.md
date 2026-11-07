<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Opening conversation auto-prompt

Current status: the greeting policy and its restart/publication follow-ups are
landed and verified. The latest complete protected native gate passed on
`fb725e80a3`, including incoming native account-source lifecycle changes: 1,757
ALab tests passed, four ignored, and no ALab lint diagnostics. Seventeen other
failures reproduced on pristine upstream and were non-gating under the maintained
attribution rules. See the [latest source verification](native-lifecycle-composition-2026-11-06.md)
and [earlier gate history](native-gate-provisioning-2026-11-06.md) for exact scopes.
This session's selected signed package still predates the fix (build 2026110601,
index 102); source landing is not runtime adoption. The early limitations below
are history.

Incident: the first user message, `sup`, received `Hey—what are we working on?`.
Cruise mechanically answered it with `Nobody is here to answer…`, creating work
where no task had been requested.

Decision: reuse the existing brief opening-exchange boundary in Cruise when no
tool work or question box ran. Preserve ordinary continuation after work, on long
turns and on subsequent messages. This changes an autonomous policy default
reversibly; it does not add a greeting vocabulary or inspect user wording.

Comparison: claude-code-alab's README says turn-end supervision still belongs to
aterm's built-in Claude lane. It has no separate classifier to transplant. Codex
owns its native policy in alab-autopilot.

Follow-up source review found two integration gaps in the first patch:

- `record_left` and its caller admitted only Land, so the quiet Cruise point was
  not published to the host. Both now admit a point the current mode leaves quiet,
  with the existing publication and native-turn fences intact.
- The restart record dropped `ran_tools`. A Land opening that did actual work
  could become taskless after restarting and switching to Cruise. Carry the tool
  evidence end to end; absent evidence in an older record does not prove idleness.

Regression coverage includes the incident, policy refreshes, real tool work,
question boxes, duration, later interruptions, wall recovery, mode changes,
host publication once only, record serialization and restored tool evidence.
These tests are authored but have not run on this host.

Validation and unavailable operations:

- `trustfmt --edition 2024 --config skip_children=true --check` passed for changed
  Rust files. `git diff --check` passed.
- The first narrow test attempt was stopped during dependency fetching. An
  offline attempt failed loading rusty_v8's missing `third_party/icu` submodule,
  before compiling tests.
- The current repository gate, `alab/check.sh`, refuses because a qualified
  `atpkg-publish` is unavailable. No compiler admission was bypassed.
- `aterm init --check --only base.clt --json` reports Apple's tools are absent,
  rather than a broken installed toolchain. No CLT directory or Xcode exists.
- The authorized installer through `aterm ctl sudo -- … machines here apply
  --plain` was refused by askpass with `not-frontmost`; no password was supplied
  and no installation ran. That operation is deferred without changing focus or
  bypassing the refusal.

No replacement binary has been built or installed, so this session still runs
the previous policy. A restart alone cannot adopt the source changes.
The tag `atpkg-codex-2026110601` contains the first greeting fix, but the signed
index is still 92: `aterm pkg update codex` reports build `2026110301` already
current. Package adoption is deferred until the signed index offers it; no
unpublished artifact was installed and no pointless restart was performed.

Later, signed index 94 selected build 2026110601 and a self-restart completed.
The artifact's alab-build.json and the new live session record both identify
source 5c1ea61e73, predating the greeting fix, despite the local package tag's
newer target. The tag-based adoption inference was incorrect; see
signed-codex-adoption-2026-11-06.md. Source remains landed, deployment deferred.

## Native validation on fleet m157

Used an owned plain shell, with the existing drive grant, Apple CLT and an isolated
checkout. No other agent was driven. Scratch CODEX_HOME and logs are under
`/tmp/.ubd/codex-native-f0a4e96f.8bYgIz` on m157. The first Cargo SecureTransport
dependency fetch stalled; its command was cancelled and confirmed exit 130.
Retrying with `CARGO_NET_GIT_FETCH_WITH_CLI=true` completed dependency resolution.
No compiler override or admission bypass was used. These are `--unverified`
Trust unit tests, not the qualified repository gate; atpkg-publish remains absent.
The shim reports Trust 0.11.0 / compiler 20565c325, matching the repository pin.
The help diagnostic also lists the globally selected shim directory 16387; that
directory alone does not establish the compiler used, because native shim dispatch
honors the repository's trust.lock.

At source `2a1f6d95b6`, the original greeting regression passed (1/1). The full
autopilot suite reported 862 passed, 4 failed, 2 ignored. The failures exposed
remaining contradictory test controls rather than a clean suite:

- The seeded-work hold test still expected Land to preserve a later exchange;
  it now checks that the hold preserves it and Land carries it after the hold.
- The runtime hold control now seeds earlier work, so it verifies that Cruise
  resumes a later exchange. Taskless opening publication has separate coverage.
- The Off-to-Cruise matrix now expects an opening to stay quiet and later work
  to resume, instead of requiring both to resume.
- The firmlink fixture's root is `/homes` and its destination column is `Users`;
  the expected path now follows that destination column, as the parser does.

These fixture corrections are committed in `fbd2b6de7d`. The full autopilot suite
then passed on that exact source: **866 passed, 0 failed, 2 ignored**, in 59.86s;
the owned shell's retained command block 8 independently confirms exit 0.
The original greeting, hold, mode-change, publication and restore tests all ran.
The separate TUI restart-record serialization regression passed (1/1; block 9,
exit 0). The full TUI unit suite also passed on `fbd2b6de7d`: **329 passed,
0 failed, 0 ignored**, in 87.60s; retained block 10 confirms exit 0. Combined with
the autopilot suite, this is 1,195 passing unit tests and two ignored tests.
Both logs remain in the scratch directory on m157. Subsequent merge `793c9daaa7`
adds only Python/tooling/docs changes relative to this tested Rust source.

## Opening retry review

Hostile review after the initial validation found that a service-error retry's
`by_autopilot` path accumulated duration but ignored new tool work and question
boxes. A greeting that initially hit a service error could start real work on
retry and still be left quiet in Cruise. That contradicted the initial-turn
policy and could leave an actionable question box unanswered.

Regression `an_opening_retry_that_starts_work_is_carried_in_cruise` covers both
Cruise and Land, with a plain reply, tool work and a question box. At source
`c03fd893a6`, the native test failed specifically on `Cruise tools=true
questions=false` (left=true, expected false); retained block 0 in owned shell
`s-47f29378879f348617d0` on m157 confirms exit 101. The fix `984bac6d6c` adds the
same Cruise task evidence check to the retry path. Land retains its behavior,
and a plain retry remains quiet within the existing duration budget. The full
autopilot suite passed at `984bac6d6c`: **867 passed, 0 failed, 2 ignored**, in
54.65s. Retained block 1 confirms exit 0. This is the same unverified Trust lane
and scratch CODEX_HOME as the earlier checks; the qualified gate remains deferred.
Signed index 96 still pins build 2026110601, so no further restart was attempted.

The subsequent resume review found no additional runtime gap: restored points
carry both tool-work and question-box evidence, and the mode rechecks them.
Updated stale resume comments that claimed Cruise always reclaimed the point,
and documented the opening, duration and retry boundaries in NATIVE-CONTROLS.md.
This documentation-only follow-up changes no runtime expressions. Validation:
`git diff --check` and inspection of the complete diff; native suites were not
rerun for comment and documentation edits.

## Host handoff review

Read aterm source at `0cd6015d1002892e5e1bcdb8c38d1f3b7735b3ec` against Codex
source `afac881e80`. No host runtime change is needed for quiet Cruise openings:

- `crates/aterm-gui/src/control_session.rs` admits a self-claim act by the live
  proven holder and its lease, not by requiring Autopilot Land. A bounded quiet
  publication still requires a fresh at-rest snapshot. It rejects stale claims,
  expired leases and a program that has begun another turn.
- `harness_task::self_claim_act_face` maps `left-to-person@v1` to `TaskFace::Theirs`
  independently of mode. The timeline remembers that standing face and suppresses
  duplicate publication at the same program-turn count.
- Existing `bounded_harness_acts_prepare_and_publish_through_real_meta_dispatch`
  test exercises the real dispatcher, checking the claim, lease, Theirs face,
  last act and landing mark. Codex's already-run publication regression separately
  covers Cruise and Land and checks one publication over repeated evaluations.
- `claude_lights::ChipFace::of` intentionally keeps the Cruise glyph under a
  Theirs task face. Its existing face matrix expects that combination. The chip
  describes the configured mode; it does not itself inject a continuation.

This was source and existing-test inspection, not an executed aterm GUI test or
a live handshake from a new Codex binary. No foreign session or real claim was
mutated to simulate adoption. Keep deployment deferred until a signed artifact
actually contains the source fix; changing the glyph or weakening host claim and
at-rest checks would not address the reported false continuation.

## Whole-exchange evidence across Land retries

The next mode-change review found that a Land opening could do tool work or open
a question box, hit a service error, and recover through a plain retry. The latest
turn then said no tools and no box, so a later switch to Cruise could incorrectly
leave the opening quiet. Re-exec would persist the incomplete evidence as well.

At `f0b4682247`, native regression
`an_opening_keeps_task_evidence_across_land_retries_and_reexec` failed because the
recorded latest point's tool evidence was false after an earlier tool-using turn
(expected true); owned remote block 0 confirms exit 101. The test covers a
taskless opening and openings with prior tools or a box, then a plain retry and
the live mode switch and restart paths.

Fix `0f05be318c` accumulates tool and question-box evidence over the exchange.
The typed LeftPoint snapshot carries aggregate tool work and `hadQuestions`,
separately from the latest turn's `questions`, so earlier boxes cannot reopen a
current box. Missing `hadQuestions` in an older record is treated as unknown
work, not proof of a taskless opening. Seed and snapshot use the same typed
record instead of positional fields. The regression also checks a handoff before
and after materialization; TUI serialization covers all four evidence combinations.

Ran full autopilot and TUI unit suites on that exact source, using the same
unverified Trust lane, owned fleet shell and scratch CODEX_HOME as earlier.
Autopilot passed: **868 passed, 0 failed, 2 ignored**, in 54.18s. TUI reported
328 passed and one failure: the exact restart JSON fixture omitted the newly
written `hadQuestions: true` field. The combined command therefore exited 101
(retained block 1), rather than being recorded as a passing run. Fixture-only
commit `2e7843891a` adds that expected field. The full TUI suite then passed:
**329 passed, 0 failed, 0 ignored**, in 89.60s; retained block 3 confirms exit 0.
Production and autopilot test sources are unchanged from the 868-pass run.
Combined coverage is 1,197 passing unit tests and two ignored autopilot tests;
the qualified full release gate and signed deployment remain deferred.

The subsequent wider check ran `targo --unverified test --locked -p
codex-alab-core --lib` at `2e7843891a` in the same isolated environment:
**196 passed, 0 failed, 0 ignored**, in 0.51s. Owned shell
`s-b194d4436188345ede40` retained block 0 confirms exit 0. This brings the relevant
core, autopilot and TUI unit coverage to 1,393 passing tests and two ignored tests.
The exact source scopes above still apply; this is not a full workspace gate.

Tippy across all targets of `codex-alab-autopilot`, `codex-alab-tui` and
`codex-alab-core` initially exited 0 but reported two owned test setup warnings
(`field_reassign_with_default`). Fixture-only `fcc44b071e` initializes those
mode fields directly. Rerunning the same lint command produced no warnings in
the three selected ALab crates; upstream dependency warnings remain and were not
changed. The 19 focused `opening` tests then passed (0 failed/ignored, 0.29s).
Retained block 3 confirms both commands exited 0 on `fcc44b071e`. No production
source changed after the full unit-suite runs. Logs are `core-wide.log`,
`exchange-tippy.log`, `exchange-tippy-clean.log` and `opening-after-lint.log` in
the same remote scratch directory. This remains a development check, not an
authenticated qualified release-gate result.

## Post-gate adoption and hostile review

Rechecked the signed package after the lifecycle composition gate. Index 102
still pins build 2026110601; `aterm pkg update codex` reports it current. Managed
lookup resolves that store root, and its actual alab-build.json still names
source `5c1ea61e73ab3a32f211b5e6879d177d6a318d2f`. Decision: defer runtime
adoption rather than restart into the same payload. No package was published.

Reviewed the current opening boundary and serialization after the full source
gate. No additional runtime defect found: initial tool work and question boxes
prevent Cruise quieting; retry evidence accumulates rather than being replaced
by a later plain reply; saved points carry that evidence; and publication/mode
changes recheck it. A left point is already wall-free and within its opening
work budget, so restoring it does not itself authorize another retry. Later
person turns use the ordinary new-turn path. Retain the landed policy and its
existing regression matrix; no wording classifier or compatibility branch added.
This is a source review and metadata check, not a new native test run or a
claim that the installed image contains the fix.
