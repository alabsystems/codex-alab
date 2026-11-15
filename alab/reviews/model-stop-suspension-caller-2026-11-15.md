<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Accepted suspension after its public caller disappears

Continue the bounded native check selected at landed `b555c74f6e`. The prior
suspension fixtures keep and await the public reply. Prepared-child/checkpoint
caller-drop coverage does not prove that the owning root's accepted suspension
finishes native goal delivery when its public reply receiver disappears.

Decision: add two ALab-owned native controls before changing production. Start
one actual public suspension while the claimed goal completion and streamed
item callbacks are held. Poll through the real item Drop, prove the request is
still pending, then drop only that caller. Independently poll session-loop
termination and require it pending until the finish gate opens. Release only
finish, then require native ShutdownComplete and all existing original-source,
invocation/outcome, single ThreadStop, unfinished durable history, exact closed
writer error, retired owner and no-extra-work postconditions. Repeat with a
real committed/applied API pause; the pause must remain and admit no source.

Termination alone is not successful suspension: the shared termination future
also resolves after a loop panic. Event senders can outlive the loop, so consume
queued native events only through ShutdownComplete under a timeout, never to
EOF. Reject terminal turn events while reading. Native shutdown can also emit
ShutdownComplete; the fixture sends no alternate operation before verification
and requires exactly one ThreadStop, rejecting a fallback normal shutdown after
a late suspension error. Cleanup opens every gate before shutdown_and_wait,
after capturing the scenario result, so it cannot manufacture passing evidence.

The native handler owns accepted work: it awaits the internal suspension
result, determines exit from that result, then ignores failure to send the
public reply. Do not send a second suspend, invent receipt of the lost
Suspended reply, or derive recovery authority from a stopped owner. Associate
the original turn through the real closed rollout and its exact start event.

Only the existing ALab test fixture changes; no dependency, harness option,
production source or upstream hook is required. Preserve all six earlier
controls and existing policy, epoch, budget and human-hold checks. Current
production is expected to pass. Previously recorded certified-build and nested
sandbox limitations remain separate, without another unavailable attempt or
compiler/profile exception.

The two controls are committed at
`3d2d9f20b07e4f0747da8807485cdd2329d1c3cd`. Independent exact-source review
confirms that dropping the sole boxed public future destroys its actual reply
receiver after acceptance, while the fixture retains the native session. The
six existing controls still await their original public replies. No alternate
operation can mask the caller-drop result before its assertions finish.

Validation on that clean committed source:

- All eight native partial-finish controls pass in 0.79s, including both caller-loss cases. Official artifact/runtime/summary report: `codex-rs/target/alab-goal-full-libs.1sINOX/collected.json`.
- Full Autopilot library passes 1,071 tests, zero failed, four ignored in 249.10s. All 69 native model-block cases and the existing epoch, owner, policy, person-stop, history, budget, restart, pacing and accounting controls are included. Official report: `codex-rs/target/alab-goal-full-libs.bvYZmU/collected.json`.
- Scoped all-target Autopilot Tippy completes in 56.28s with zero ALab findings. Three deprecated-fetch_update warnings are on diagnostics/lib35, executed_tool_calls401 and apps_test_server568, each verified unchanged from integrated upstream ce3031018ef1. The protected controller uses `--locked --no-deps --all-targets -p codex-alab-autopilot --cap-lints warn`; actual diagnostics are inspected, not inferred clean from exit0.

Logs: `~/.ubd/codex-goal-suspension-caller-{focused,full,tippy}.log`.
All original controllers fully exit before this record is updated. No new
production, dependency, harness, upstream hook, compiler/profile or permission
change was needed; unchanged Core/TUI and prior canonical evidence retain their
original source attribution. Previously unavailable certification and nested
sandbox operations are not retried or represented as newly qualified.

Decision: native accepted suspension correctly survives loss of its public
caller even while goal completion is held. Land the tests, keep existing
production, and preserve the API-pause veto. No second-operation fallback,
synthetic reply or new recovery authority is justified.

Next investigation selected: fresh-successor source import from the actual
nonterminal suspended history, where the original sampling owner never
collected its FunctionCallOutput. Keep provenance preservation separate from
execution authority. Current model-block recovery starts a new goal-triggered
turn; same unfinished-turn-ID execution uses the separately admitted exec
handoff path, captured options and live receipt/account/policy admission. Saved
WallGoal data or a fresh mutation epoch cannot supply that admission, and a
running historical point must not be fabricated into a completed one.

First inspect the existing saved-record custody/history validator and establish
the expected import result. A bounded successor fixture may use the real closed
rollout, same goal database and genuinely saved native provenance, never the
old Binding/epoch, a hand-made marker or a prompt-normalized aborted output as
native completion evidence. Preserve original goal identity/usage and reject
an actual API pause. Require no execution without an ended point or admitted
handoff. Do not broaden the production contract merely to force same-ID
recovery; this is a selected coverage investigation, not a demonstrated defect.
