<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Capacity recovery and build identity

Current codex-alab retries recognized model-overload failures (`server_is_overloaded`)
inside the active turn. It keeps the chosen model, request input and completed tool
output. A retry does not require someone to type `continue`, and it does not spend
the ordinary stream-retry budget.

The first wait is between 0.5 and 1 second. Later waits grow to between 30 and
60 seconds. A later provider `Retry-After` deadline takes precedence. Retries continue
while that turn remains active. Steering input is queued without shortening the
selected capacity wait; an explicit interrupt or shutdown cancels it. The displayed
message is `Model is at capacity; retrying automatically (interrupt to stop)...`.

Recognized failed-response events reach their handler as soon as the complete SSE
frame arrives, even when the response body stays open. Quota, model-access and
policy failures retain their terminal meaning; rate-limit failures retain their
original provider deadline. A later completion cannot hide a recognized failure.
Unknown or malformed failures keep upstream's handling.

## Collaboration workers

The active-turn retry applies to spawned workers too. A recognized capacity
failure leaves the worker running while it waits, then retries the same turn
and selected model. Completed tool work and the delegated input stay in that
turn; a temporary overload is not a successful result or a terminal
`Agent errored` notification. Interrupting the worker still cancels its wait.
The real-worker regression is
`codex-rs/alab-autopilot/tests/subagent_capacity_retry.rs`: it spawns through
the collaboration tool, completes a tool call, injects overload, and observes
the worker and its parent through recovery.

Diagnose a terminal capacity-looking message using its structured error and
producer, rather than retrying its text. An explicit permanent HTTP refusal
must retain that meaning even when its body mentions capacity. Quota,
authentication and safety failures do not become automatic child retries.

For an older conversation, its creation-time CLI version does not identify the
executable that produced every later turn. `/alab version` distinguishes the
running image from the selected launch target. Installing a newer build alone
does not upgrade an existing process: the live-upgrade gate can remain held by
an active goal, a running thread or a background terminal. Preserve the work
and exact resumption state before using aterm's supervised agent restart when
that gate prevents adopting a needed fix; never replace an executable in place
or treat an unfinished worker as complete.

## Retry and recovery boundaries

Queued steering cannot turn an already received refusal or an exhausted ordinary
retry verdict into a successful follow-up. An exhausted transient failure still
passes through the retry handler's existing fallback decision and provider deadline.
Ordinary transient waits and unbounded connection retries keep their steering behavior; an explicit
interrupt still stops the turn.

Capacity recovery also covers the sampling request used by legacy compaction and
remote compaction v2. Login, quota, permission and invalid-request failures have their
own handling. A permanent client refusal must not become a capacity retry merely
because its text mentions overload. WebSocket error frames retain their explicit
HTTP status even when optional envelope headers have the wrong JSON type; valid
nested retry advice still takes precedence.

Duplicate `status` and `status_code` fields with the same valid HTTP status retain
that status. Contradictory aliases that report a permanent refusal cannot enter the
capacity loop: they use ordinary bounded protocol retry and transport fallback,
retaining validated provider advice without inventing an HTTP status.
Repeated raw fields and invalid error metadata cannot hide an earlier reported
refusal during this fallback. Nested fields remain subject to strict decoding.
The malformed-metadata fallback retains the longest validated nested retry deadline,
including repeated raw header fields; envelope advice applies only without valid
nested advice.

The native harness's `retry_api_errors` switch controls continuation after a terminal
API error; it does not disable the active turn's capacity retry. Quota and usage-window
resumption follow `resume_limits` and the applicable wait policy.

Claim loss and temporary suspension publish their passive state before releasing
the supervisor's state lock. If a later refresh takes or resumes the claim while
old endpoint cleanup finishes, that cleanup cannot strand the recovered claim in
passive mode. Endpoint cleanup stays outside the lock.

An ownership event that arrives while a refresh proves a new claim waits for that
refresh before deciding whether to give back an unaccepted claim. It rechecks local
ownership after waiting. Cancelling its caller leaves the cleanup task responsible
for the release until it completes; a fresh refresh waits for that cleanup.

## Failed automatic goal admission

An active goal normally continues through the goal runtime's atomic idle admission.
A transient local error while reading goal state or submitting that continuation can
leave the goal active without a turn. ALab schedules one coalesced recheck for that
runtime: waits are 1, 2, 4, 8, 16 and then 30 seconds, with no attempt limit while
the same runtime still owes recovery. Each wait is logged and counts host sleep.

Every recheck acquires the existing goal mutation permit, reads the current goal,
and uses the ordinary atomic admission. Goal mutation, thread teardown, intentional
deferral and a successful or intentionally rejected admission retire the pending
recheck. Cancellation does not drop a callback after admission has started; its
goal accounting completes before recovery retires.

Only typed local contention and interrupted reads qualify. Start errors qualify
only for local interrupted, would-block or timed-out I/O without an HTTP status or
provider retry advice. Permission, corruption, decode, policy, quota and provider
failures do not enter this loop. Existing in-turn provider retries retain their own
deadlines and budgets. This admission recovery is independent of model-capacity
retry and does not establish the cause of an earlier stalled session.

## Sleep and wake

On 2026-10-24 a usage-limit continuation in another harness fired inside a 19 s macOS
dark wake (the lid closed, the network down). Its request then sat in backoff across
the sleeps that followed, and the person had to press Esc to get the session going.
codex-alab had the same exposure. Every wait it arms runs on the `Instant` clock, which
stops while the host sleeps, so a wait armed before a sleep came due only after the
wake had given it all the awake time it still had. A wait could also come due inside a
dark wake, and nothing held an act there. Since the fix, codex-alab measures the sleep
and acts on it (`codex-rs/alab-core/src/wake.rs`):

* **The measure.** The kernel counts the time asleep (macOS: `CLOCK_MONOTONIC_RAW`
  minus `CLOCK_UPTIME_RAW`; Linux: `CLOCK_BOOTTIME` minus `CLOCK_MONOTONIC`), unaffected
  by wall-clock steps. A reading that finds the count grown by a second or more records
  a wake. Concurrent readings share one order and ignore stale lower totals, so a
  sleep counts only once. Clock reads bracket the continuous clock with awake reads;
  samples spanning 1 ms or more are rejected. Supported-platform waits keep polling
  after a rejected sample. If the initial count was unavailable, the first valid
  count establishes the baseline; only later measured sleep is credited. A retained
  provider receipt keeps its known origin through a rejected selection sample.
  Provider advice is normalized and armed from one accepted reading, while a fresh
  positive local minimum keeps its own origin, so older provider sleep cannot consume it. Historical
  wakes are never inferred from their observation time. This bounds sampling skew
  below the one-second wake threshold; it is not an atomic time measurement.
  Where a platform has no such count, nothing changes.
* **The wake is dated when the host woke.** Nothing reads while nothing waits (a
  streaming turn, an idle session), so the first reading can come long after the wake.
  On macOS the kernel's own record of the last wake (`kern.wake_abs_time`, dark wakes
  included) dates it, whenever it is read; a record from before the reading before is
  not believed. Where the kernel keeps none (Linux), the reading's own time stands in:
  the latest the wake can have been, so a settle counted from it is longer, never shorter.
* **Waits count the sleep.** The ALab capacity wait, ordinary stream retries and
  transport fallback, core's own wait for the network
  (`unbounded_connection_retries`, one `ALAB(wake)` hook), legacy compaction's retry
  wait and every scheduled continuation timer treat their deadline as real time. A sleep counts
  toward it, including sleeps other readers observed between this waiter's polls.
  A wait the sleep outlasted ends soon after the wake, allowing up to 2 s for the
  network without extending the selected deadline. A server's `Retry-After` still
  counts its full real-time interval. A cut-short connection wait starts its
  doubling over at 5 s, so a request the network was not yet up for retries within
  seconds, not after the minute the dark wakes climbed to.
* **Deadlines that name a moment of real time move back by the time slept.** These are
  a usage window's reset, a server's retry advice, a restored wait's deadline and a
  crash's provider wait, including an act owed by the previous image. Provider
  advice retains its receipt-time sleep count across delayed error hooks. Selected
  waits, including ordinary and unbounded connection retries, capture their count
  before reporting; error handoffs and restored points
  retain it before awaiting a route or their first decision. Each interval counts
  once, and sleep preceding a fresh deadline does not shorten it. A reset the host
  slept through is due at the wake. Valid provider advice also avoids evaluating
  an unused local fallback that could overflow after delayed reporting.
  A measured cut updates that exact wait's durable crash pacing before it can
  finish; superseded guards cannot alter or clear a newer wait's deadline.
* **After a wake the host settles before any act.** An act codex-alab would type (a
  resume, a retry, a continuation) waits until the host has been awake 30 s plus the
  session's share of the reset jitter (up to 60 s), aterm's `WAKE_SETTLE`. A dark wake
  ends before that, and the wake after it settles again.
* **A due usage resume waits for the API.** Where the route is direct and aterm's
  `probe_api` is on, the resume asks the API reach measure (`netwatch`) once it comes due
  (with the switch off it goes as decided). It waits up to 11 s for the
  first measure, never types into an API measured down, and goes the moment the measure
  reads up. If the measure stays down, it tries once after 15 min, the network wall's
  own hold. An unknown measure never holds it.

* **A stream's idle timeout counts the sleep.** A turn that was streaming when the lid
  closed keeps upstream's 300 s idle timeout (SSE and WebSocket, two `ALAB(wake)` hooks),
  counted in real time: a sleep longer than what was left of it gives the stream up once
  the host has stayed awake 30 s after the wake (the settle above), and the turn retries,
  where the awake clock alone left the person waiting up to 300 s or pressing Esc. The
  settle rather than 2 s, because giving a stream up sends a new request: in a dark wake
  it would be cut by the next sleep and sent again at every dark wake through the night,
  spending the usage the limits count. Each wake settles again, so a stream is never given
  up in a dark wake. A shorter sleep only shortens the timeout by the sleep, so a stream
  the server still holds is not discarded for a nap.

Nothing here needs a key.

## Continue permitted work after a security refusal

Inside aterm, an active Cruise or Land harness treats a native `cyber_policy`
refusal during regular sampling separately from capacity errors. It can submit one policy-aware continuation
for the current task. That instruction excludes the refused action and asks for
independent, permitted work, such as collecting completed build results. It does not
repeat or rephrase the refused request, change models or accounts, or enable Daybreak.
An eligible account's automatic Daybreak offer stays informational under the active
harness. The person can still open its settings with `/daybreak`.

Later automatic prompts retain that exclusion. Native conversation history preserves
the boundary and the one-continuation limit across restarts; another security refusal
raises attention instead of starting a retry loop. A new explicit person turn starts
a new task sequence. Existing input, hold, claim and native goal checks still apply.
If the continuation reports `POLICY-WORK-EXHAUSTED` on its own line, including in an
earlier assistant message before the final reply, the harness stops nudging. This
stop also survives restart.
An accepted new root turn retires the superseded refusal alert.

Refused context compaction remains terminal, whether it runs before, during or after
a reply, or as a standalone operation. Another automatic turn could resend the denied
history before reaching its new instruction. Native compaction item identities preserve
this distinction across restart; only the matching completion clears a pending item.
A post-turn compaction refusal retains the answer and records the terminal policy error.

Other permanent refusals, including account permission errors and invalid requests,
keep their separate handling. The security continuation never uses the capacity retry
ladder.

## Establish which build is running

Three build identities can differ:

| Identity | What it establishes |
| --- | --- |
| Public package | The package published through atpkg as `codex`, from `alabsystems/codex-alab`. |
| Selected launch target | The complete package that a new process will launch. A local development link can select a newer build than the public package. |
| Running process | The executable already loaded by this session. Selecting a newer package does not replace its running image. |

Use `/alab version` (or `/alab update`) in the TUI to compare the running build with
the installed launch target and read the pending-update reason. The report works
with the optional ALab interface turned off. A development version includes its source
commit; an upstream stable version number alone does not identify the ALab build.
codex-alab's own version (`codex --version`, `0.1.0` on) names the build; the report's
"Codex client version" line is the upstream-numbered version OpenAI's servers see
(alab/README.md, Decision 8).

Inside aterm, the native harness checks for a changed launch target at startup and
every minute, with file notifications providing earlier hints. It hands off only when every loaded
thread is idle, no active goal or background terminal remains, the conversation can
be resumed, and its input and attention checks permit the handoff. An active or held
session can therefore keep running older code after the launch target has advanced.
Read the pending reason before treating a successful package selection as adoption.
When a manual restart is needed, stop at a safe point and resume the conversation
through the selected launch target. Do not replace the executable in place.

Missing presentation features can also reflect configuration. `/alab on` enables the
ALab interface, including its account details, tasks and prompt stash; this switch
does not change automation policy or permissions. Checklists with the embedded server
also require `enabled = true` under `[tools.update_plan]` in the Codex config. ALab's
packaged defaults disable apps, plugins and some other optional upstream features.
User and project config layers can override those defaults. See
[`alab-core/defaults.toml`](../codex-rs/alab-core/defaults.toml).

## Verify recovery without using an account

aterm's jailed capacity lane (`aterm-lab codex-alab-proof --capacity-only --package
<package>`, in an aterm checkout) is the off-network proof of this recovery on an existing
package. It runs the package's `codex exec` against the lane's own loopback Responses model
in a `sandbox-exec` jail that allows that one loopback port and no other network, with fresh
homes and no account. It checks that a completed tool call survives, in the same turn, a 503
capacity failure and its provider wait, and a 429 account-quota wall with and without a
reset time (the retry no sooner than the reset, or 30 seconds on), and that a 403 with
overload- or quota-looking text, and each terminal 429 credit or spending-cap type, fails
after one request. It witnesses egress (a jailed probe refused a loopback canary port, and
`codex`'s sockets sampled at each request, each sample seeing its connection to the model),
and it kills `codex`'s whole process group before it reaps it, at its exit, a hang bound or
an interrupt. See the
[headless capacity and quota recovery](README.md#headless-capacity-and-quota-recovery)
instructions.

For `codex exec --json`, an `error` retry notice is not a final turn verdict. Check
`turn.completed` or `turn.failed` together with the process exit status. Preserve the
package source, version, binary hashes and probe results; a probe of an older selected
package does not validate a newly built candidate.

Implementation: [`capacity.rs`](../codex-rs/alab-core/src/capacity.rs),
[`responses_retry.rs`](../codex-rs/alab-core/glue/responses_retry.rs), and
[`upgrade.rs`](../codex-rs/alab-tui/src/upgrade.rs).
