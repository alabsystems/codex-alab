<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Resuming a native goal after a dated usage limit

On November 19 a restored goal in Tab 1 reached its account limit at 2:23 a.m.
Pacific. The provider advertised a 3:19 p.m. reset. The next recorded turn
started at 6:36 p.m. from the user's `continue`. The goal returned to Active
at 6:39 p.m. The original session establishes the missed deadline and the
sequence of turns. It did not record the scheduler's internal decision.

The startup race in `a110c84176` gives a concrete path to that outcome. A
native goal turn can start before the resume scan reads history. The scan
correctly yields to the newer turn, but Autopilot previously learned that it
had a task from the discarded history or from user input. The goal's own
turn could therefore end at a dated limit with no task eligible to schedule
the reset. At native turn start, an Active goal in the thread database and
the host's goal trigger now establish the task. The existing reset decision
retains the provider deadline and uses GoalService to continue that goal.

The regression provider dates its reset when it receives the first request.
The test observes the failed turn and checks the arrival time and native goal
attribution of the next request. This exercises the same transition even when
test setup takes longer than the simulated usage window.

A dated reset on macOS arms a continuous-time kernel timer. System sleep counts
toward the deadline. The existing scheduler checks the deadline and its
post-wake settling period before acting. Replacing or cancelling the pending
act drops its timer. Unreadable clocks and platforms without this timer keep
the existing sleep-aware wait.

A person can also enter `continue` before the reset act takes ownership. The
first model output or tool call confirms service is available. At that point
Autopilot can reactivate the original usage-limited goal during the running
turn and publish its status to the UI. Other input keeps its ordinary
meaning. Goal identity, edits, stop generation and remaining budget are
checked before the mutation.

## Verification on macOS

All four provider-wall tests passed with the combined Autopilot source. The
dated-reset test observed two provider requests. The second arrived after the
advertised time, carried the original objective, and belonged to a native goal
turn. All 41 wake tests passed, including delivery of the one-shot timer through
Tokio's I/O driver. All 28 Autopilot timer lifecycle tests passed.

All seven human and native goal recovery tests passed with the combined source.
The manual recovery regression holds the second provider response open. This
allows it to observe the goal row and the UI event before the human turn ends.
Its mock invokes the enabled shell tool and checks the returned output. An
earlier fixture invoked the disabled plan tool, which produced a second request
without exercising the tool-start hook.

These runs used the pinned Trust compiler and cached dependencies. For the
combined integration run, a temporary adapter connected the new timer to the
same wake state the cached dependencies use. The full Autopilot integration
suite passed all 167 tests in 395 seconds on `ec8e98fb28`. A first run exhausted
the shell's 256-descriptor limit. The successful run gave the test process 4,096
descriptors. The repository's Cargo admission
check deferred the regular build at its 20 GiB free-space floor. During
verification, Tab 1 was already working with an Active goal on installed
build `2026111701`.

## A second missed reset on November 19

In a separate session, a native goal turn ended on November 18 at 10:21 a.m.
Pacific with a usage-limit error. The provider gave November 19 at 10:47:54
a.m. Pacific as the seven-day reset. The 54 consecutive completed turns
immediately preceding the failure were also goal-triggered. The host journal
kept the original child run open until 7:58:35 p.m. on November 19. Six
seconds later the user reactivated the goal in a new process and the next
turn began, nine hours and ten minutes after the advertised reset.

The provider error retained the deadline even though the session had also
received a premium-credit snapshot without reset windows. Account refreshes
timed out after the deadline. The original process did not record whether it
armed a timer. The sequence is consistent with the native-goal startup race
fixed in `a110c84176`. It also rules out process exit before the deadline as
the explanation for this session.

The signed package offered on this Mac after recovery was `2026111701`.
Its build manifest identifies source `70d6391259`, which predates the fix.
A resolved executable path for the original process did not survive.
A release build from `2377090098` reached the source-selected Trust 15698
compiler. Certification stopped at admission of the CLI's in-process
procedural macros, before the compiler produced proof evidence. The signed
package therefore still needs a release containing the fix.

### Account refreshes after the reset

The original process logged three `account/rateLimits/read` timeouts on
November 19. They occurred at 11:32:29 a.m., 11:45:48 a.m. and 2:14:01 p.m.
Pacific, respectively 44 minutes 35 seconds, 57 minutes 54 seconds and
3 hours 26 minutes 7 seconds after the provider reset. The warnings came
from the TUI's background account poll. The trace contains no scheduler
decision linking them to the missed goal turn.

On signed-package source `70d6391259` and managed source `fc5d5b5fbe`,
`account/rateLimits/read` uses no app-server serialization queue. The TUI
bounds its periodic and recovery requests at 15 seconds. During recovery it
holds queued user messages. Autopilot checks for drafts, queued input and
open dialogs before admitting a native goal. A fresh allowance reading can
authorize an early retry. Once Autopilot arms the dated retry, it can resume
the goal without a new reading, including after relaunch on the same account.

All eight focused native checks passed on integration commit `f2bd5feecc`.
Two showed the Core sampler returned control at the known reset when the
usage endpoint returned HTTP 503 or delayed its response for 60 seconds.
Three exercised TUI account-read failures,
coalescing and queued user input. Two restored the saved retry deadline after
a witnessed relaunch, including one with no new account reading. The final
check observed a dated 429 followed by a new goal-triggered provider request
and a completed answer for the original goal. These checks exercised the
HTTP outage, relaunch and goal turn separately. Both HTTP controls also
passed independently on managed commit `fc5d5b5fbe` with pinned Trust 15698.

The `weekly_reset.py --case natural-reset` loopback check also ran against the
signed `2026111701` executable and the native test CLI from `f2bd5feecc`.
Both retried 30.04 seconds after the dated refusal, completed their original
`exec` request and spent zero reset credits. Goal recovery requires a new
native goal turn on the original thread.
Packaging managed `fd69ca619b`, which includes the wake-timer correction,
then stopped on Trust 15698 because it refused the CLI’s in-process procedural
macros during dependency-inclusive certification (zero obligations proved,
one unknown, package command exit 1).

### Reproducing the failure

A private aterm host ran the signed package against a loopback provider. An
app-server process created an Active goal on an unloaded thread and exited.
A new Codex TUI resumed that thread. Its first native turn received a weekly
limit with a reset four seconds away. The original goal entered
`usage_limited` and the provider received just one request in 180 seconds.
The resumed TUI received no input. In a separate control, manually resuming
the goal produced the second native turn and saved its answer on the
original thread.

Run the packaged check with `alab/smoke/tui_host_rotation.py --goal-reset`
and its required `--aterm`, `--aterm-lab`, `--package` and `--result`
arguments. The result records the executable digest, the goal ID, the failed
turn and each provider request. A passing run also records the successful
turn and its answer.

The native Autopilot integration regression passed at `2377090098` with the
source-selected compiler and the regular Cargo dependency graph. It observed
a second provider request after the deadline, a native goal turn, the
original goal ID and the original objective. All 43 wake tests passed at
`cdffbeb524`, including both tests for an initially unavailable clock sample.

The native regression also passed with the two recovery changes together at
`4c294eb761`. The subsequent test at `10069b72a7` let the provider return
its answer. It checked that the same goal-attributed turn completed without
an error and delivered that exact answer. A third turn waited at the existing
test gate so it could not supply the successful completion by accident.

Removing only the change that makes Autopilot recognize a native goal made
the same regression fail. The first turn hit its dated limit. Autopilot
then recorded that it had no user task or native goal continuation. The
test timed out waiting for the next turn. This isolates the failure in the
reproduction. The original session did not record its scheduler decision.

At `4bd085c810`, the full native Autopilot integration suite passed all
167 tests in 265 seconds with four test threads. An earlier run on
`10069b72a7` timed out in a separate API-key renewal test. That test
passed both alone and in the full rerun.

A release build of managed main `169e0ab5bc` reached the same certification
refusal as `2377090098`. Trust 15698 rejected in-process procedural macros
in the CLI. The release command exited 1 before producing a package.

## Follow-up

A sleep-clock sample can be rejected if the sampling thread is descheduled.
Previously, one rejected reading while arming a dated reset selected a one-second
poll for the whole wait. A seven-day reset could then wake the task 604,800
times. Retry sampling until a baseline and a current reading are available,
then park on the kernel timer. Preserve an earlier baseline captured for the
pending action so the wait credits exactly the measured sleep.

The timer follow-up in `b6af164841` passed 43 wake tests, 28 timer lifecycle
tests, seven goal recovery tests, four provider-wall tests and two account-handoff
tests. A separate macOS check armed 32 real timers after initially missing
samples and observed the descriptor count return to baseline on cancellation.
Separately, installed build `2026111701` passed the loopback CLI probes for a
natural quota reset, repeated stale reset timestamps and SIGINT during a distant
reset. The natural-reset fixture finished its original turn without using a
reset credit.

Core can accept a person's next instruction while the model is still responding
to `continue`. Both inputs belong to the same turn. Use Core's input acceptance
order when deciding whether the earlier `continue` can reactivate the goal.
This lets a newer direction take effect as soon as Core accepts it, even if the
model has yet to see it.

The regression stops a real tool call on either side of Autopilot's lifecycle
callback. It submits a second instruction to the running turn, waits for Core
to acknowledge the steer, then releases the first tool call. Before the fix,
the callback changed the SQLite goal to Active despite the newer instruction.

An assistant message can also be the first evidence that service is back.
The native test reproduced a stack overflow in that callback on an 8 MiB worker.
Pinning the goal write on the heap keeps it on the same task under the same
authorization and mutation permit. The test now exercises both message-first
and tool-first responses.

At `78b695f2bc`, the standalone native run rebuilt the changed crates and their
consumers with pinned Trust and used cached dependencies for the rest. Core
and Autopilot shared one ALab core library throughout. All 167 Autopilot
integration tests passed in 400 seconds, alongside eight goal recovery tests,
28 timer lifecycle tests and 43 wake tests. The full Cargo workspace gate still
awaited its 20 GiB disk floor.

After merging the completed-answer regression from `4bd085c810`, its test
binary was relinked against the same verified production libraries. All four
provider-wall and seven goal-recovery integration cases passed, including
the expected answer from the original goal's resumed turn.

## Restoring a saved retry deadline

Autopilot can save a pending usage retry even when it has no separate recovery
record for that thread. On relaunch it restores the retry and its deadline.
The restored turn has no quota-error details, so the previous timer sampled the
clock every second until that same deadline. `af04abba9b` lets macOS use its
one-shot timer when the selected goal retry matches the saved deadline. At wake,
Autopilot checks the current goal and quota policy before dispatch. The focused
Trust run passed 29 timer and decision tests and four provider-wall tests.

The packaged TUI check now watches the original Codex process for exit or
re-execution through completion of its resumed answer. It disables automatic
package upgrades in the private test session so the original process can own
the wait. A macOS regression re-executed the same binary under the same PID and
confirmed the check detected the replacement. All 17 related host tests passed.

The local package inventory contained 182 completed manifests covering 143
source commits. None contained the native goal-start fix in `a110c84176`.
Packaged verification of the corrected executable remains pending.

The uninterrupted TUI check reproduced the failure on signed build
`2026111701` with package upgrades disabled. The host verified the original
process, and the kernel reported no exit or re-execution before timeout.
The provider received one request and no retry through 176.6 seconds
past its weekly reset.

A native test build from `4bd085c810` also opened the saved thread and
started its goal in the real TUI. The private package resolver selected
that executable. Aterm requires a completed ALab package to recognize the
running TUI as its supervisor, so this run ended before the reset check.
The passing completed-answer result above comes from the native Autopilot
integration suite.

## Qualifying the same goal across reset

The provider-recovery campaign already exercised natural quota release. Its
evidence could pass without running the native goal that failed in Tab 1.
`dd34c063f1` adds that exact integration test to the existing natural-reset
scenario. For sources declaring the requirement, the collector must observe
the named test passing. Older sources retain their original requirements. The
test checks the provider deadline, native turn attribution, original goal ID
and objective, and the answer delivered by that turn.

All 51 campaign tests passed, including rejection of a successful test log that
omitted the named native case. The inventory, recipe merge, scenario contract
and evidence suites passed 83 more tests.

## Host sleep and the age of a quota reading

A denial sampled before the Mac slept for an hour could look two seconds old
on the awake clock when a new premium limit arrived. A relaunch could then
carry that old denial into the new wall and retry on ordinary allowance before
the premium reset. `58bc015b74` checks the original 30-second limit on both
the awake and continuous clocks when capturing a denial and authorizing an
early retry. The regression reproduced the stale saved denial with the former
predicate. All 481 ALab core unit tests passed with pinned Trust on macOS.

## Showing when the retry is due

The provider's reset and Autopilot's scheduled retry can be different. Live
notices now show the provider time and add the local date when it falls on
another day. A restored retry shows its scheduled time, including any later
pacing deadline. Its label says “automatic retry” because GoalService can resume
the goal directly. Existing saved records with time-only labels use the
verified reset instant to recover the local date.

The notice checks passed for saved retries, raw five-hour and weekly resets,
local midnight, year and daylight-saving boundaries, and host settling. On
`1850198a14`, standalone Trust rebuilt Core, Autopilot and their affected
dependencies. All 167 native integration tests passed in 204 seconds, plus all
eight human goal recovery tests and 58 quota, timer and wait-display cases.

The full Autopilot library run on that source passed 1,164 tests, failed 17
and ignored six. All 17 failures were in live upgrade or handoff tests,
including the one-minute wake-settle handoff. A representative fixture
reported that its running image had no exact native published build and
failed identically before the notice change. An earlier run with cached Core
also failed two manual continuation tests. Both passed with rebuilt Core.
Five Code Mode overlap tests passed with the native host supplied through
the same two environment variables as the repository runner. The host,
runtime and protocol sources matched those in package `2026111701`.

## A live update offered during the wait

The positive live-handoff fixtures require a published Codex image and a host
that can replace it. The installed host reported `execution=unavailable`.
The wake-settle fixture used to wait a minute for a terminal request after the
updater had already refused the image. In `1663737675`, it waits for either
the request or the updater's result. With pinned Cargo, the fixture reported
the missing published image in 0.54 seconds, and the separate refusal and
rollback test passed in 0.64 seconds. All 167 native Autopilot integration
tests passed through the locked Cargo build in 213 seconds.

A pending provider reset reaches an earlier decision. The new regression
starts a native goal, observes its dated 429 and asks the actual updater to
upgrade while the retry is pending. The updater defers before asking the
terminal to leave. The test checks the original goal row, turn identity,
goal-resume action and precise deadline after deferral. It passed five
consecutive runs on `8ae44cfae5`. The natural-reset qualification scenario
now requires that result alongside the existing resumed-answer test for
sources that declare both requirements.

All 134 campaign, inventory, scenario and evidence tests passed. The gate on
`1663737675` obtained its locked dependencies and completed its all-target
lint pass with Trust 15698. It classified 963 warnings in ALab-owned code and
set aside 70 diagnostics. None of the 963 referenced the three handoff test
files changed by that commit. The gate stopped at lint before attempting a
certified CLI build. Signed index 166 still selected Codex `2026111701`.

## An early kernel wake and the displayed retry

The macOS timer takes its absolute clock reading before sampling how long the
host has slept. If the process is suspended between those readings, its first
notification can arrive early. In a regression with a real kernel timer and
a week-long selected delay, the former code armed once and handed the rest of
the delay to the one-second polling path. `322164bb5a` rechecks the deadline
and arms a new one-shot when more than one sample interval remains. Rapid
early notifications are limited to one registration per sample interval.
A failed registration retains the existing sleep-aware fallback.

The Waiting row can also outlive the provider's reset while the retry grace
period runs. It now distinguishes the advertised reset from the later retry.
When a wake sets the retry time, the row shows that time, including the local
date for another day. The previous row could still say it was waiting for a
reset that had already passed and truncate the reason for the remaining wait.

Fresh primary provider limits persist their boot-clock anchor before the timer
arms. A historical record with only a civil retry deadline cannot distinguish
two seconds of elapsed time from a ten-minute system-clock correction. Adding
timing data at its first restored read would change the saved act that the
successor authenticates. Correcting this legacy case requires an explicit
record migration, and the lost elapsed interval cannot be recovered from the
old record alone.

A packaged smoke failure now survives an additional host shutdown failure.
The JSON report keeps the original stall or process change and includes the
cleanup diagnosis separately. A shutdown failure alone still fails the run.

On the combined source `e3cc89195b`, the locked Cargo run with Trust 15698
passed all 482 ALab core tests, 54 targeted Autopilot tests and all 167
Autopilot integration tests. The integration run took 205 seconds. Its
loopback provider observed the original native goal resume after the dated
limit and deliver the expected answer without a user turn. All 186 decision
tests also passed on the display change, and all 42 Python TUI tests passed.

A fresh inventory found 185 held package manifests covering 143 source
strings. None identified a descendant of the native goal-start fix. The
signed store still selected build `2026111701`. Tab 1 was working with its
original goal active on that build.

## Watching the packaged retry and qualifying two recovery races

In `d8c369f96d`, the packaged check sleeps on the second provider request,
the original Codex process watch and private host exit in a single kernel
wait. A request recorded before the observer arms stays readable. The
original process must still be present when the request arrives. A one-second
quiet interval took seven blocking waits in the former observer and one in
the new one. All 44 Python TUI tests passed, including an actual re-exec
under the same PID. The available October debug launcher rejected the private
host's `--world` option before starting the packaged TUI, so this run produced
no new signed-binary reset result.

Two further runtime changes are staged together in
`qualification/goal-reset-cancellation-hosted-c89d6d1977` at `c89d6d1977`.
They await native verification.

- A timer that expires during cancellation can put its goal retry back in
  the thread's fired slot between two clears. A concurrent native turn can
  then write that superseded retry back to the session record. `7c12c5603a`
  takes the timer lock before clearing both slots, matching expiry's lock
  order, on native turn start, stop and a held decision. Enumeration of the
  lock operations produced a stale act in ten of fifteen possible orders
  before the change and neither of the two possible orders afterward. The
  native regression holds evaluation, expires the timer during cancellation
  and checks the session record after the new turn starts.
- Successful provider-hosted search and image generation can be the first
  evidence of service after a person says `continue`. They do not start a
  local tool. `cc502f151a` observes their completed provider items while
  the original status is still available, then applies the pending human
  turn's existing goal and input checks. The new cases include failed and
  partial provider results and a newer instruction accepted during search.

The protected Cargo attempt reached the 20 GiB free-space floor before the
hosted test ran. Bounded disk waits subsequently expired. The available
fleet machines with this checkout advertised read access. On this Mac the
guarded red and green runners are `/private/tmp/reset-timer-cancel.xQUFolVI/run.sh`
for cancellation and `/private/tmp/hosted-goal-qual-ckcfa6ej/native.sh`
for hosted results, each taking `red` or `green`. The integrated source
is on the remote qualification branch so a fresh checkout can also run the
locked Autopilot library tests for `timer_tests` and `human_goal_recovery_tests`.

The packaged probe now records a private launcher that exits before opening
its control socket. The reset report previously lost the launcher's reason
when the fixture removed its temporary directory. The new regression failed
on that generic error, then passed with the fix alongside the other 44 Python
TUI checks. Repeating the probe with the October launcher and signed build
`2026111701` retained exit code 2 and the launcher's rejection of `--world`
in the result JSON. The failure occurred before the first provider request.
The wider run caught an account-card test still expecting an untyped launch
error. After updating that assertion to the shared report format, all 124
tests for the TUI, rotation, goal output and child continuity passed.
