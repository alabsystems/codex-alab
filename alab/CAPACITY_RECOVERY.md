<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Capacity recovery and build identity

Current codex-alab retries recognized model capacity failures inside the active
turn, including `server_is_overloaded` and `policy_capacity_unavailable`. It keeps
the chosen model, request input and completed tool output. A retry does not require
someone to type `continue`, and it does not spend the ordinary stream-retry budget.

The first wait is between 0.5 and 1 second. Later waits grow to between 30 and
60 seconds. A later provider `Retry-After` deadline takes precedence. Retries continue
while that turn remains active. Steering input is queued without shortening the
selected capacity wait; an explicit interrupt or shutdown cancels it. The displayed
message is `Model is at capacity; retrying automatically (interrupt to stop)...`
for server overload. For policy-restricted capacity it begins with
`Policy-restricted capacity is unavailable`.

When an HTTP stream opens successfully and later reports overload, the client
uses valid retry advice from the error object first, then from the event's
top-level `headers`. This applies to both `error` and `response.failed` events.
If neither supplies advice, main uses the opening HTTP `Retry-After` deadline.
Time already spent streaming counts toward that opening deadline.

Recognized failed-response events reach their handler as soon as the complete SSE
frame arrives, even when the response body stays open. Quota, model-access and
policy failures retain their terminal meaning; rate-limit failures retain their
original provider deadline. A later completion cannot hide a recognized failure.
Unknown or malformed failures keep upstream's handling.

## Check a waiting tab

From another aterm tab, run `aterm ctl ls` to find the session ID, then run
`aterm ctl @SESSION_ID text` with that ID. Both commands leave the active turn
and its input alone. A later tool call or answer in that turn confirms the
model resumed. Another capacity notice can mean the retry reached the provider
and received another wait.

To check the retry log, use the Codex thread UUID shown as `Session` in
`/status` in place of `THREAD_UUID`. On the Trust machine this command lists
retained rows from the capacity retry module, even in a busy thread.

```sh
python3.14 -IB alab/request_timeline.py --db ~/.codex/logs_2.sqlite \
  --thread-id THREAD_UUID --target codex_core::responses_retry::alab_capacity_retry
```

The module also logs other events. To identify capacity waits and their delays,
open the tab's logs database with `sqlite3 -readonly` (on the Trust machine,
`sqlite3 -readonly ~/.codex/logs_2.sqlite`). The first query shows the retained UTC
time range. The second shows the turn ID, attempt, reason and selected delay
for each recorded capacity wait in that range.

```sql
.parameter set @thread "'THREAD_UUID'"
.headers on
BEGIN;
SELECT count(*) AS retained_rows, datetime(min(ts), 'unixepoch') AS first_utc,
       datetime(max(ts), 'unixepoch') AS last_utc
FROM logs WHERE thread_id = @thread;

SELECT strftime('%Y-%m-%dT%H:%M:%S', ts, 'unixepoch') ||
       printf('.%09dZ', ts_nanos) AS wait_utc,
       substr(feedback_log_body, instr(feedback_log_body, 'waiting to retry the active task')
              + length('waiting to retry the active task') + 1) AS retry_details
FROM logs
WHERE thread_id = @thread AND target = 'codex_core::responses_retry::alab_capacity_retry'
  AND instr(feedback_log_body, 'selected model is at capacity; waiting to retry the active task') > 0
ORDER BY ts, ts_nanos, id;
COMMIT;
```

Each thread retains up to 1,000 log rows and 10 MiB of log content. When
`first_utc` is later than the incident, an empty wait list leaves the earlier
outcome unknown. A saved session rollout can still show subsequent tool calls
and turn outcomes. Match its turn ID to the recorded wait when available.
A tool result can arrive from work already running when the refusal occurred.
Look for a new model-issued call or answer after the refusal to confirm recovery.

Locate the thread's saved file with `rg --files ~/.codex/sessions -g '*-THREAD_UUID.jsonl'`,
replacing `THREAD_UUID` with the ID from `/status`. Set `ROLLOUT` to its path.
This Trust incident query shows calls, assistant and user messages, and turn boundaries
in file order. For another incident, change the inclusive start and exclusive
end to UTC rollout timestamps with three fractional digits. Keep other turns
in the output so a new turn or user input is visible.

```sh
jq -c --arg first '2026-11-18T10:35:07.000Z' --arg past '2026-11-18T10:46:18.000Z' '
  select(.timestamp >= $first and .timestamp < $past)
  | .payload as $p
  | if .type == "response_item" then
      select(($p.type | IN("function_call", "custom_tool_call", "local_shell_call",
                           "tool_search_call", "web_search_call", "image_generation_call")) or
             ($p.type == "message" and ($p.role == "user" or $p.role == "assistant")))
      | $p.internal_chat_message_metadata_passthrough as $m
      | {recorded: .timestamp, event: ($p.role // $p.type), turn: $m.turn_id,
         created_unix: $m.create_time}
    elif .type == "event_msg" then
      select($p.type == "task_started" or $p.type == "turn_started" or
             $p.type == "task_complete" or $p.type == "turn_complete" or
             $p.type == "turn_aborted" or $p.type == "user_message" or
             ($p.type == "item_completed" and $p.item.type == "UserMessage"))
      | {recorded: .timestamp, event: ($p.item.type // $p.type), turn: $p.turn_id}
        + (if ($p.type | IN("task_complete", "turn_complete")) then
             {failed: ($p.error != null),
              error_kind: ($p.error.codex_error_info |
                           if type == "object" then keys[0] else . end),
              http_status: (try $p.error.codex_error_info.http_connection_failed.http_status_code catch null),
              has_answer: (($p.last_agent_message | type) == "string" and ($p.last_agent_message | length) > 0)}
           else {} end)
    else empty end
' "$ROLLOUT"
```

Match the call's `turn` to the retry log. Compare its `created_unix`
(fractional Unix seconds) with the wait before drawing a conclusion from its
later recording time. Missing creation or turn metadata leaves that part of
the comparison unresolved. A user-role record can carry environment context.
To check for steering, inspect the saved `user_message` or completed
`UserMessage` events and the provenance of each user-role content part.
The capacity smoke cross-checks both against the model requests. A second
submitted message rules out unattended recovery even if the model never saw it.

A terminal refusal also writes `task_complete`. On a completion record,
`failed: true` identifies a saved error. For a turn expected to answer,
look for `failed: false` and `has_answer: true` on the matching turn ID.
If a capacity retry ends with `error_kind: "http_connection_failed"`, inspect
`http_status`. In two loopback runs on signed build `2026111701`, an unattended
retry after HTTP 503 completed when the next request succeeded and stopped
when it received HTTP 403 with the same overload message and retry advice.
Reopening each thread in a fresh process preserved its outcome and issued no
model request. The failed turn still reported HTTP 403.

The timer beside the notice measures elapsed turn time across attempts. On
main, the detail line gives the delay selected when the notice was issued.
Reconnecting shows the remaining delay at reconnect time. For headless exec,
the text renderer prints a warning and JSON emits an advisory item. Read
through to the turn outcome.

On signed build `2026111701`, a 76-column terminal showed `20s` and then
`58s` during a 61-second provider wait. When the provider then asked for three
seconds, the timer still showed more than a minute and the detail still
suggested changing models. With no input after the initial prompt, the provider
saw the retries after 61.004 and 3.004 seconds and the original turn answered.

## November 17 Trust incident and package verification

In the Trust session, Astra returned an overload with a 324-second wait. The
same turn resumed tool calls without new input. The suggestion to change models
came from the client's generic error description. The live notice on main now
reports the scheduled delay alongside the structured overload reason.

The same Trust tab later provided a longer unattended check. On November 18 at
10:35:07 and 10:40:47 UTC, turn `01a1ede9-d178-7892-9441-71a2e018ae1a`
logged consecutive overload attempts with waits of 330 and 302 seconds.
The next function call on that turn has a
creation time of 10:46:10.420 UTC and a rollout timestamp of 10:46:17.096 UTC.
The rollout contains no intervening user input or turn boundary. For a suspected stall, match
the retry log's turn ID to subsequent tool activity or turn completion. The
elapsed timer alone cannot distinguish a provider wait from resumed work.

At 22:05:00 UTC on November 18, the Trust tab recorded a final assistant answer
and `task_complete`. At the same timestamp the goal extension reported
`goal usage remains held after an ambiguous write`. The saved goal still read
`active`, and the rollout's later events through 22:41 UTC were completions of
existing background work. This explains why automatic goal continuation was
held after that turn. A manually submitted turn can still run while automatic
admission is held. The retained warning does not identify the original write
or its outcome. Restarting the process would discard its in-memory
accounting evidence. See the [goal storage recovery design](design/goal-storage-recovery.md).

Package `2026111701` also recovered in a loopback test with ordinary request
and stream retries both set to zero. After a completed plan update, the server
returned three consecutive HTTP 503 overloads, each advising a two-second wait.
The next requests arrived 2.004, 2.003 and 2.748 seconds after the respective
response headers. All three retries preserved the model, prompt and completed
tool output. The fifth request completed the original turn and exec exited zero.
This package emitted a top-level JSON `error` for each wait. A separate
plain-text run printed `ERROR:` before recovering. These observations cover
the installed package and explain the subsequent display changes on main.

The same signed package recovered on the provider WebSocket in two loopback
runs with both ordinary retry budgets set to zero. The provider sent a plan
call, completed the response and closed that connection. It refused the next
two upgrade requests with HTTP 503 `server_is_overloaded` and `Retry-After`
values of 2 and 3 seconds. The next handshakes arrived 2.100 and 3.106 seconds
after the provider prepared each refusal (2.103 and 3.110 on repeat). On the
accepted connection, the client sent the original model and input followed by
exactly one plan call and result. The original turn completed without new
input or an HTTP fallback. Two controls returned HTTP 403 with the same
overload body and retry header. Each failed the active turn on that refusal.

When diagnosing closely spaced handshakes, inspect `request_kind` and
`turn_id` in the `x-codex-turn-metadata` header. All four runs began with a
separate `prewarm` handshake with an empty turn ID. The first `turn` handshake
followed that speculative connection's two-second 503 advice after only
0.098–0.130 seconds. The startup path on this build does not carry that wait
into the first turn. Measure unattended active-turn recovery between requests
with the same nonempty turn ID. Keep the prewarm interval visible when
investigating startup load.

The signed package also started generating 0.108 seconds after a successful
upgrade delivered a three-second overload frame to its warmup request.
For both a refused speculative upgrade and a capacity error on the open
warmup socket, the client now keeps the provider's original deadline in the
thread. The next matching request hands the typed overload to the active
turn's retry loop before contacting the provider. This keeps the
wait visible and interruptible even when ordinary retries are set to zero.
The handoff checks the effective model and tier, destination, credential
revision and owner, runtime, and routing headers and affinity. A first turn
on a different route can start immediately. A permanent refusal or elapsed
deadline creates no wait for that turn.

The native first-turn regression on m79 saw the foreground handshake after
0.254 seconds on unmodified source `286eccec2e` despite two seconds of advice.
With the handoff, both startup paths completed without steering. The socket
fixture received the generation request 2.153 seconds after it sent the
two-second policy-capacity frame. The upgrade fixture also checked a model
change, a 403 carrying overload text, and zero-second advice.

In a signed-build control, a speculative handshake returned HTTP 403 with
`server_is_overloaded` and `Retry-After: 600`. The first foreground handshake
arrived 0.154 seconds later. It received HTTP 426, and the turn continued over
HTTP. After the plan completed, the provider returned HTTP 503 with two seconds
of advice. The same turn retried after 2.004 seconds, retained the plan result
and answered without input. The two refusals belonged to different requests.

The package smoke also delivers a plan call and a `response.failed` overload
in the same stream. The retry must carry the original input followed by
exactly that call and its result. This catches lost or repeated tool work
when the provider fails before it completes the response. Signed build
`2026111701` completed the turn in two requests. It retried after 0.508
seconds with a two-second opening HTTP header alone, and after 2.006 seconds
in a control where the overload event also carried that advice.
The native API regression delays the error frame by two seconds. It checks
that an eleven-second opening deadline stays eleven seconds from receipt,
and that seven seconds of event advice instead expires nine seconds from opening.
All 287 API library tests passed at the fix in `03d6154653`. Removing the
opening deadline from the error path made that regression fail, and restoring
it passed. The API lint run produced no diagnostics.

The maintained package smoke also tests two consecutive HTTP 503 responses
after the same completed plan call. They carry two- and three-second
`Retry-After` headers. Both the probe and the source gate check each retry
against its own response headers. The probe checks the reasoning settings
and completed tool history at each retry, and requires one advisory notice
per refusal before the final answer. With two general overloads, signed
build `2026111701` retried after 2.003 and 3.005 seconds and completed one
turn. Its two top-level JSON errors failed the advisory check.

The probe timestamps each complete JSON line as a reader receives it on stdout.
For both refusals, the advisory must arrive after the provider starts sending
the refusal headers and before the advertised deadline and the next request.
This catches a client that buffers its notices and flushes them just before
the answer. The runtime gate checks the retained stdout against its receipt
times in `exec-observed.json` and the provider's request trace. On signed
`2026111701` the two notices were visible within a millisecond of the response
headers. Their top-level error classification made a continuing wait look fatal.
A test renderer translated those two events into timed advisories. The probe
passed when it flushed each advisory immediately and failed when it buffered
both until the answer. The underlying signed executable exited successfully in
both controls.

The same check compares the announced seconds with the provider time still
remaining when stdout delivered each line. In the immediate-renderer trace,
both lines arrived within a millisecond of their respective headers. Changing
either countdown to one second must fail for those two- and three-second waits.
A one-second countdown can pass when the line arrives during the final second.

The package smoke also sends two consecutive HTTP 503 responses without a
`Retry-After` header. Here the client chooses the waits. The provider trace
must show both refusals without advice and at least 0.5 and 1 second,
respectively, before the next request. The smoke checks the *announced* waits
against the client's local backoff range. Process scheduling can delay arrival
at the provider. Signed build `2026111701` waited 0.754 and 1.823 seconds, kept the
completed plan and finished the turn. Its JSON stream still emitted the old
top-level error notices.

The same smoke now follows those refusals with a third HTTP 503 carrying
`Retry-After: 1`. By this attempt the local backoff is two to four seconds.
The gate checks the two-second minimum, the exact one-second header and the
announced wait against the local range of two to four seconds. On signed build
`2026111701` the three waits were 0.551, 1.887 and 3.794 seconds. The
original turn completed with its plan result intact.

The probe timestamps each advisory in this three-refusal case. Both the probe
and the source gate require the line to reach stdout between its refusal and
the next request. This catches buffered notices even when the final answer and
all three retry intervals are correct.

In a separate signed-client run, two consecutive HTTP 503 refusals explicitly
sent `Retry-After: 0`. The provider observed waits of 0.626 and 1.661 seconds.
The same turn answered with its completed plan intact and no steering input.

Local backoff starts over when the next sampling request begins after a tool
call. In a signed-package run, two overloads before a successful plan call
produced waits of 0.511 and 1.674 seconds. Two more overloads after the call
produced waits of 0.600 and 1.515 seconds. All six requests belonged to one
unattended turn, and the later requests contained one plan call and its result.

The maintained package smoke sets both ordinary retry budgets to zero for
capacity cases. After a completed plan step it tests the same two-second
`Retry-After` on an HTTP 503 and on an HTTP 200 stream carrying a
`response.failed` event with no retry advice. On signed build `2026111701`,
the loopback provider observed retries after 2.003 seconds and 0.689 seconds,
respectively. Both turns completed with their original input. Measuring from
the opening headers lets the smoke flag the premature retry even when the
binary also emits the older JSON error notice. A separate HTTP 503 run used
an absolute HTTP date and observed the retry 5 milliseconds after that deadline.

The streamed-overload package check keeps the HTTP response open after it
flushes the complete SSE failure frame. The provider releases that response
when the retry arrives. The source gate checks the recorded frame-flush time,
the retry arrival and whether the failed response was still open. A client
that waits for the connection to close can otherwise appear healthy when a
fixture closes it immediately after every event.

On signed build `2026111701`, the client retried on the open stream and
completed the turn. With two seconds of advice only in the opening HTTP
headers it retried after 0.880 seconds. With the same advice also present
in the failure event, it retried after 2.004 seconds. This isolates the
early retry on that build to its handling of the opening header.

A second open-stream case lets the two-second HTTP deadline expire before it
sends the failure frame. It measures the local backoff from the first write
of that frame. On signed build `2026111701`, the failure was sent 2.010 seconds
after the opening headers and the retry arrived 0.889 seconds after the frame
flushed. The original turn completed. The older JSON renderer still emitted
a top-level error. The gate requires an advisory announcing the first local
wait (rounded up to one second). This catches reuse of the expired provider
interval without setting an upper bound on when the OS schedules the retry.

The repeated-overload case returns HTTP 503 `server_is_overloaded` and then
HTTP 503 `policy_capacity_unavailable`, with identical provider message text.
The gate checks that each live advisory names the reason for its own refusal
and reports the corresponding scheduled delay. Signed build `2026111701`
retried after 2.006 and 3.005 seconds (2.005 and 3.005 in an earlier run).
The original turn completed with the same model, input and plan result.
In a control the second response
was HTTP 403 with the same policy-capacity code and three seconds of retry
advice. The turn failed with the provider's permission diagnostic and made
no further request.

A generic HTTP 502 uses the ordinary stream retry budget. With that budget set
to one, the signed build recovered through a 503 overload, a 502 and another
503, preserving the model and completed tool output. When a second 502 followed,
the turn failed with that gateway error. The intervening capacity waits neither
spent nor replenished the ordinary budget.

A separate stdio app-server run on that package tested the effect of sending
a message during the wait. After receiving the overload notification, the client
submitted `turn/steer` and received the original turn ID before the two-second
deadline. The retry arrived 2.009 seconds after the response headers and carried
both user messages exactly once, the selected model and the completed plan
output. The same turn completed.

During automatic mid-turn compaction, a new message can be acknowledged
before the model sees it. On the same signed build, legacy and remote v2
both acknowledged `turn/steer` within one millisecond of the first overload.
They retried compaction with its original input after the advised two and
three seconds. The first sampling request after compaction continued the
pending tool work. The following request carried the clarification once,
and the original turn delivered the clarified answer. With no steering,
both controls answered on the first sampling request after compaction.

The interactive terminal on signed build `2026111701` showed both “retrying
automatically” and “Please try a different model” during the same wait. Four
private PTY runs used a loopback provider, a completed plan call and two
consecutive HTTP 503 responses. The provider recorded when requests arrived
and the saved session identified which turn completed.

| Terminal input during the first wait | Provider advice | Observed retry intervals | Completed work |
| --- | --- | --- | --- |
| No further input | 2 s, 3 s | 2.005 s, 3.003 s | The original turn displayed its answer. |
| Draft entered 0.032 s after the refusal and left unsent | 4 s, 3 s | 4.003 s, 3.006 s | The original turn completed. Pressing Enter afterward sent the intact draft once in a second turn. |
| Enter, 0.274 s after the refusal | 4 s, 3 s | 4.011 s, 3.002 s | Both retries included the new message exactly once. The original turn displayed its answer. |
| Tab, 0.285 s after the refusal | 4 s, 3 s | 4.005 s, 3.003 s | The original turn completed first. A second turn carried the queued message. |

All retries retained the chosen model, original prompt and completed tool
result. During the wait, an unsent draft stayed in the composer, Enter
steered the original turn, and Tab queued work for the next turn.

Two signed builds also ran a shell command that appended one line in a fresh
workspace. In one case the provider returned HTTP 503 after receiving the
command result. In the other it sent the tool call and a capacity failure in
the same stream. With two seconds of advice, build `2026111701` retried after
2.006 and 2.005 seconds respectively, and `2026111601` after 2.004 and 2.005
seconds. Each of the four turns completed with a single line in the file, a
single successful command event and the original call ID and result in the
next request. Each run received one initial prompt and no steering.

Both builds also completed the original turn when that command appended the
line, wrote a diagnostic to stderr and exited 42. The retry carried the original
exit code and diagnostic under the same call ID. The file held one line after
each run, whether the capacity failure followed the command result or arrived
in the stream that delivered the tool call.

A separate check on each build launched a shell job that returned a session ID.
When the provider sent HTTP 503, the job had written its start marker. By the
retry, it had also written its finish marker and exited 42. The provider saw
the same input after the three-second wait and issued one `write_stdin` call
with the original session ID and empty input. It received the job's final
output and exit code. Both turns completed without another prompt or shell
launch. A repeat sampled the shell PID during the wait and confirmed the
process exited between the refusal headers and the retry. Across the four
runs, the observed retry intervals were 3.004 to 3.006 seconds.

Disconnecting the only client also preserved the wait while the app-server
stayed running. Two Unix-socket trials on package `2026111701` used a six-second
deadline. A client that reattached early received the overload notice for the
original turn, with no delay detail on this build. In the second trial the
provider received the retry with every client disconnected. A fresh client
then read the recovered answer from that same turn. Both retries preserved
the request input and arrived after 6.004 seconds.

An explicit interrupt produced a different outcome on the same package. With
a one-hour provider deadline, the client steered a clarification and then sent
`turn/interrupt`. The original turn reported `interrupted` before any retry.
The next explicit turn completed under a new ID. Its request arrived within
four milliseconds of the overload headers and carried no cancelled clarification.

Changing thread settings during a wait was verified separately on the same
installed package. The app-server acknowledged a model and effort change
before the two-second deadline. The retry arrived after 2.005 seconds with the
same input, model (`gpt-5`) and effort (`medium`). The subsequent turn used
`gpt-5.4` at `high` effort. On main, the sampling retry loop retains the step
settings it captured for that request. The smoke compares the encoded reasoning
settings across the failed request and its retry to catch changes at the wire.

On November 18, native tests at `8627703a6c` exercised both exec renderers.
The stderr test observed a warning with the 324-second delay during a retry
and an error for a terminal notification. The JSON test observed an advisory
item and verified that it preserved the last terminal diagnostic. Five TUI
tests exercised the delay on screen, replacement by a later retry, clearing
on progress or completion, and the ordering of a queued retry and terminal
event. These were test-profile runs with Trust 15698. The package observations
above used the earlier signed build.

The native app-server reconnect suite also passed at that commit. For a
600-second provider deadline, both the original WebSocket client and a client
that immediately rejoined received `Retry scheduled in 600s.` with the same
thread ID, turn ID, `serverOverloaded` reason and `willRetry: true`. A separate
case observed the retry reach the provider and the notice clear on actual
response progress.

The reconnect regression also interrupts an HTTP 503 wait and starts a new
turn on the same thread. That request opens an HTTP 200 stream and then reports
overload. Only its opening HTTP header supplies the 240-second delay. The
captured WebSocket messages showed 600 seconds live and on replay for the
first turn, and 240 seconds live and on replay for the new turn ID. The
provider received exactly two requests. Distinct deadlines catch both lost
header advice and accidental replay of the previous turn's wait. All nine
tests in the native reconnect suite passed with this fixture on m79.

The maintained request-recovery smoke checks the configured model and one exact
copy of the operator prompt on every request. Comparing attempts alone can miss
a prompt lost before the first request or a different user message such as
`continue` copied into every request after the tool call. The smoke rejects
either condition. The permanent-refusal fixture returns
a tool call followed by an error, so its trace must show one turn starting and
failing without an assistant answer.

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

Capacity recovery also covers legacy compaction and remote compaction v2.
When diagnosing `thread/compact/start` through the app-server, follow the
turn ID on the `contextCompaction` item. The preceding user turn may already
have reported completion. An `error` notification with `willRetry: true`
leaves compaction active. In successful runs the same item and turn then
reported completion without another client request. An HTTP 403 control
reported `willRetry: false` and the compaction turn failed.

On signed build `2026111701`, both paths compacted a thread containing a
completed plan call. The provider first opened an HTTP 200 stream with
`Retry-After: 2` and sent a `server_is_overloaded` failure frame. It then
returned HTTP 503 `policy_capacity_unavailable` with `Retry-After: 3`.
Legacy compaction retried after 0.942 and 3.005 seconds. Remote v2 retried
after 0.696 and 3.005 seconds. Both preserved the model and the compaction
input, including exactly one plan call and result. The premature first retry
on that build also occurs during ordinary sampling. With four seconds of
retry advice inside the failure frame and the same two-second opening header,
both compaction paths waited 4.004 seconds before retrying.

In a separate signed-build replay, moving the four-second advice from
`response.error.headers` to the `response.failed` event's top-level `headers`
shortened the measured wait from 4.007 seconds to 0.657 seconds. The original
turn completed in both runs. The mid-turn frame-advice probes now exercise
this envelope location on both compaction paths.

Opening-header probes measure from the HTTP headers, using the interval sent
by the fixture. Explicit compaction omits frame advice. Automatic compaction
sends an invalid frame `Retry-After` to exercise fallback to the opening
deadline. With a seven-second opening wait and a failure frame delayed by
three seconds, the old probe accepted signed-client retries after 3.831 and
3.558 seconds because it checked only two seconds. The corrected probe
rejects an early retry even when the turn subsequently completes.

To repeat these checks on a package, run
`python3 alab/smoke/request_recovery.py --compaction-only --package <directory> --expected-source <full-commit> --output <new-directory>`.
Each case starts its own app-server with a loopback provider and ordinary
retries disabled. An explicit compaction records the seed and compaction
turn IDs. A `mid-turn-` case records the ID of the single turn that ran the
tool, compacted, and either resumed sampling or received a terminal refusal.
`requests.json` and `app-stdout.jsonl` retain the provider trace and protocol
notifications for checking the timing and the final outcome.
If the probe stops before it can verify the case, the report includes the
provider request count and the observed `appServerEvents` in order. An
`error` with `willRetry: true` identifies the turn that was waiting. When the
client includes a numeric capacity schedule, `scheduledSeconds` gives the
delay it announced at that notification. Older clients can omit the delay.
A later `turn/completed` gives its outcome if the probe received it. For a failed turn
the report includes the final error kind and HTTP status when available, even
if the separate error notification is missing. A 403 can carry capacity
wording, so use the final status when diagnosing a stopped turn.
The check reads notifications in wire order. It requires one `userMessage`
with the original prompt on the initial turn. An additional message on the
same thread disqualifies the run as evidence of unattended recovery.
Both capacity decisions must arrive while the original compaction item and
turn are active. Grouping notifications by type would also accept a retry
announced after completion.
The check also compares the failed turn's error with the last terminal
notification. A successful compaction or seed turn must finish without an error.
On signed build `2026111701`, a fresh app-server reopened all eight
frame-advice and permission-denied sessions across legacy, v2, manual and
mid-turn compaction. Reading the saved turns reproduced all four successes
and all four HTTP 403 refusals without a provider request.

### Automatic compaction during a tool-using turn

On signed build `2026111701`, four loopback runs finished the original turn
after two HTTP 503s during automatic compaction. Each run started one turn
and received no further client input. The first model response called
`update_plan` and reported 210,000 tokens against a 200,000-token compaction
limit. With ordinary retries disabled, both legacy and remote v2 compaction
retried after the advised two and three seconds. All four first waits were
2.003–2.006 seconds and all four second waits were 3.004–3.006 seconds.

To distinguish this path from a new turn, check the provider's turn ID across
the initial tool call, compaction retries and final answer. The compaction
requests reported `trigger=auto`, `reason=context_limit` and `phase=mid_turn`.
Each retained the one completed plan call and its result. Final sampling used
the returned summary on legacy and the returned encrypted compaction item on
v2. Both permission controls ended the original turn after HTTP 403, with
`willRetry: false` and no further provider request.

Legacy compaction sends two client-generated texts with the user role. The
smoke sets `compact_prompt` to a known instruction for the compaction requests.
For a successful mid-turn compaction, it reads the selected thread's
`compacted.message` and checks that the resumed sample contains that exact
summary once. All remaining user text must already appear in the initial
sample. To replay an older capture, pass the instruction from that client's
configuration or source as `legacy_prompt` and its recorded summary as
`legacy_summary` to `check_compaction`.

After an explicit interrupt, use the turn ID and the compaction `phase` to
identify whose request reached the provider. In eight loopback runs on signed
build `2026111701`, the interrupted turn's last request had `phase=mid_turn`.
The next user turn compacted the same completed plan with `phase=pre_turn`
under its own ID. No request from the interrupted turn arrived during the
5.6-second observation window, which included its five-second retry deadline.

In four runs the new turn completed within 132 ms of the original refusal.
In the other four, the provider also refused the new turn's first compaction
request. It retried after 2.004–2.006 seconds under its own two-second
deadline and answered without another prompt or plan update. Both sets
covered legacy and v2 compaction and both recognized capacity reasons.

The six `mid-turn-` package checks use the same context-limit trigger and
start the outage with a streamed overload. On build `2026111701`, legacy and
v2 retried after 0.745 and 0.651 seconds when only the opening HTTP header
advised two seconds. Both respected four seconds of advice in the failure
frame. The checks require one completed plan update and verify that the
recovered turn uses the compacted context and delivers the expected answer.

For frame-specific advice, the fixture now waits one second after sending the
opening headers before writing the failure frame. It measures the four-second
deadline from that write. The recovery cases open with a 324-second wait and
require the next request before that deadline. The permission controls open
with two seconds and require the full four seconds after the frame. Together
they check that the frame can shorten or extend a wait. Opening-header-only
cases still use the header timestamp.

The numeric-header checks require the specified integer as the sole HTTP
`Retry-After` field on each refusal. A second field can change the deadline.
In a signed v2 mid-turn run, the provider sent both `3` and a later HTTP
date on the second refusal. The client retried about 4 ms after the date
and finished without further input. A date-only control retried about 5 ms
after its deadline.

Login, quota, permission and invalid-request failures have their own
handling. A permanent client refusal must not become a capacity retry merely
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
* **A dated usage reset on macOS can use a one-shot kernel timer.** The timer counts
  time asleep. The scheduler checks the deadline and wake settling period before
  resuming the same usage-limited goal. A native goal turn can arm this wait even
  if restored history is still loading.
  [Tab 1 incident and regression coverage](reviews/goal-usage-reset-2026-11-19.md)
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

An active claimed root never waits for a person to settle Daybreak safety UI. When a
safety-buffering notification offers a faster model, the TUI emits the validated
interrupt/fork/retry automatically instead of opening a confirmation. Duplicate offers
for the same model are suppressed; a changed server-selected model is a new offer.
Accepted automatic retries are capped at two per original user-turn command lineage.
The retry count travels with the accepted command, is not added to persistent record
formats, and resets for a new human turn. Immediately before interruption the app
rechecks the active claimed root and this budget, closing the queued-event authority
race. With no faster treatment, buffering and recourse stay non-modal while the server
determines the next outcome; no rewrite or Daybreak escalation is submitted merely to
keep work moving. Ordinary interactive Codex keeps its existing confirmations and is
not subject to the automatic cap. The active-root predicate and cross-thread guard are
in `alab-tui/glue/chat_hand.rs` and `tui/src/chatwidget/safety_buffering.rs`.

If history loading, forking, attachment, or submission fails after interruption, an
authorized automatic retry makes one bounded continuation attempt on the thread that
is current after that failure, rather than restoring its prompt as a supervisor-blocking draft. The continuation has
native ALab attribution and a fresh client ID, retains the original structured input,
and adds an instruction to inspect current state, preserve completed work, avoid
repeating side effects, and continue only independently authorized work. It rechecks
thread identity and root authority and will not replace a different turn that is active.
The acknowledged source interruption is exempt even if its completion is still queued
in the local cache; only that exact ID is cleared so the harness-attributed submission
can start. A failure before interruption receives no such exemption. A pre-existing
unsent draft is preserved. A rejected harness-owned turn likewise does
not become a blocking composer draft. If authority has changed, or the continuation
cannot be submitted, the original input state is restored and the failure is reported;
storage, transport, authentication, and server admission failures remain external
limits rather than reasons to invent approval or bypass policy.

Inside aterm, an active Cruise or Land harness treats a native `cyber_policy`
refusal during regular sampling separately from capacity errors. It can submit one policy-aware continuation
for the current task. That instruction excludes the refused action and asks for
independent, permitted work, such as collecting completed build results. It does not
repeat or rephrase the refused request, change models or accounts, or enable Daybreak.
An eligible account's automatic Daybreak offer stays informational under the active
harness. The person can still open its settings with `/daybreak`.

When that refusal blocks an active goal, the `Resume paused goal?` view cannot hold back
recovery: the supervisor dismisses it without changing the goal through the UI. The native
policy continuation then resumes the same goal to `Active`. Policy is a recoverable wall for
this binding only when the turn began with that goal active, was not aborted, called no goal
tool, and saw no person's stop. Identity, objective, budget, status, current authority, and
person-stop count are revalidated before mutation. The native continuation includes both the
original objective and the policy instruction to abandon the refused action; it is not a
generic unguarded resume. The existing one-continuation limit and second-refusal stop still
apply.

The goal runtime retains the policy instruction as live admission custody until Core accepts
the turn. A declined admission, a transient read or start failure, and the ordinary idle
recheck therefore carry the same exclusion instead of falling back to a generic continuation.
Native admission, a terminal turn, disabled execution, a newer turn, or an external goal
mutation clears that custody, so a replacement goal cannot inherit the old task's refusal.

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

For `codex exec --json`, keep reading until `turn.completed` or `turn.failed`
and check the process exit status. Package `2026111701` emitted a top-level
`type: "error"` during a capacity wait. On main, the corresponding notice has
`type: "item.completed"` and `item.type: "error"`. Its message includes the
scheduled delay. The [request recovery smoke](smoke/request_recovery.py)
checks this event sequence against a candidate executable. Run it with Python
3.11 or newer, `--codex <executable>` and `--output <new-directory>`. The report
records the binary digest alongside the case results. For a packaged candidate,
use `--package <directory> --expected-source <full-commit>` to check its build
record before starting exec. The probe and the package can come from different
commits. A mismatch leaves both source IDs in the report and runs zero cases.
When the package includes a compiler verification record, the probe also
checks its source and compares the executable's SHA-256 with the recorded
built and packaged digests before starting either suite.
Each completed probe records the observed `turn.started`, `turn.completed`
and `turn.failed` events in order under `turnEvents`. All seven capacity cases on signed package
`2026111701` showed one turn starting and completing with exit zero, even
though the verifier rejected their retry timing or JSON error notices. The
permanent-refusal controls showed one turn starting and failing with exit one.
The default suite also returns HTTP 403 with the same overload body and
two-second retry advice as the ordinary HTTP 503 case. After the completed
plan, exec must stop at the 403, report that refusal in `turn.failed` and emit
no automatic retry advisory. Both the current and older JSON forms of a retry
notice count for this last check.

If the probe reaches its deadline, the case fails and retains the request count
and turn events available at shutdown. A six-second probe against a 30-second
provider wait recorded two requests and only `turn.started`. The guardian
stopped exec before its scheduled retry, so the case has no client exit code.
Any malformed stdout records are identified by line number. If exec exits
before the probe encounters damaged stdout, the failed report also retains
the observed exit code, request count and readable turn events. An exit code
of zero and a completion event can both survive a damaged trace. When the
app-server exits after a compaction RPC completes, the failed report likewise
retains the exit code, request count, thread and turn IDs, and the status
received over RPC. The status can be either completed or failed.

Compaction probes allow turn completion to use the guarded command's time
budget (120 seconds by default). Ordinary RPC acknowledgments have a
60-second timeout. A shorter completion timeout previously stopped both
explicit and mid-turn compaction before a provider's 65-second wait expired.
With the full budget, signed build `2026111701` retried both requests after
65 seconds and completed the original turns without further input.
In a separate loopback run, the provider requested the 324-second wait seen
in the Trust incident. With a 384-second command budget, the signed client
retried after 324.005 seconds, finished automatic V2 compaction, and completed
turn `01a1f1db-0cf7-7092-9fa4-5d33cc3b69a4`. Its five provider requests
and single user message are retained in
`capacity-compaction-long.Afltv7zY/trust-length-mid-turn-v2-frame-advice/`.

If the guardian stops an app-server that is waiting for capacity, the reader
may see EOF first. The report identifies a command timeout only when the
guardian exits 124 and its matching receipt records a timeout with a null
client exit code. A client that exits 124 on its own can still be missing
its required protocol reply.

When the probe itself is interrupted during a case, the top-level report
keeps that case's available request count and turn or app-server events in
`activeCaseObservations`. Only cases that returned a verdict appear in
`cases`. The original interruption determines the exit status.
For an unfinished exec run, `capacityNotices` lists each recognized wait's
stdout line, event type and provider reason. A notice includes
`scheduledSeconds` only when the client printed a numeric schedule. Older
clients can announce an automatic retry without printing its delay.
For the startup, repeated-overload and unadvised-overload cases, `exec-observed.json` retains
the monotonic receipt time of each nonblank stdout line read before timeout.
Compare those times with `requests.json` to see whether a notice reached the
reader during the wait, even if the turn never produced an answer.

The WebSocket startup case returns HTTP/1.1 503 with two seconds of
`Retry-After` on the speculative handshake. It measures when the foreground
handshake for the same exec thread reaches the provider, then returns 426 so
the turn can finish over HTTP. The signed `2026111701` executable arrived
after 0.109 seconds and completed successfully. Checking the handshake timing
catches this regression even when a caller sees a successful exit and answer.
The probe also timestamps the startup advisory on stdout. It must reach the
reader after the refusal begins and before the first foreground handshake.
When little provider time remains at handoff, the local backoff can extend
that window. The gate rechecks the receipt time against both handshakes.
It also compares the provider timestamps for the transition to HTTP. The first
POST must arrive after the provider sends the 426 headers, and the continuation
carrying the completed plan must follow the first POST's response headers.

The source gate (`alab/check.sh`) runs the same probe with `--capacity-only`
against the freshly built candidate. It requires automatic completion after
a refused startup handshake, single and repeated HTTP 503 responses, an
overload on an open stream, and an overload in the stream that delivered a
tool call. Checking the executable catches retry timing and
JSON-rendering regressions that the API library tests alone cannot observe.
The gate records the executable and report digests and retains each case's
request trace and exec output under `runtime/smoke/capacity/`.
For each case it reads the exec output and requires exactly one start followed
by one completion. Between them, stdout must contain the fixture's expected
assistant answer exactly once, after the last capacity notice. The summary
must report the same turn events. Each refusal must produce one completed
error item in that turn, before the answer, with the reason and scheduled wait.
A top-level JSON `error` anywhere in the
output fails the check, even when the turn later completes. The gate also
reads each case's `exec-guardian-result.json` to confirm that the expected
command exited zero. A process that prints `turn.completed` and then hangs
until the guardian times out fails this check.
The gate matches the thread ID from exec stdout to the provider's request
metadata. Every model request must belong to the same turn and carry the
configured model and the original reasoning settings in its body. For
startup recovery, the foreground WebSocket handshake must name that turn
before the client falls back to HTTP.
The gate also compares the recorded inputs. The first continuation must
retain the original prompt and add the completed plan call and its result.
Each subsequent retry must carry that same history. A new user message or
a missing tool result disqualifies the run as evidence of unattended recovery.
The saved session must also contain the plan call followed by its successful
result, exactly once between turn start and completion. This catches lost tool
work in the history a resumed client would read, even when the live retry sent
the correct result to the provider.

On signed build `2026111701`, a fresh app-server process reopened a copy of
the unattended session and sent `Plan updated` in its first request. With
only the saved result removed from a second copy, the resumed client sent
`aborted` for the same call ID. Both follow-up turns completed.

The gate checks the HTTP status of each request against the case. An ordinary
overload requires a 503 followed by a successful retry. A failure delivered
inside an open stream has HTTP status 200. A 403 at either point fails the gate.
For sampling and startup it checks the recorded `Retry-After` value, including
its absence when testing local backoff. A duplicate header fails the check.
It also recalculates each retry delay from the provider's two header timestamps
and the next request's arrival. A controller test at `20fbacefe3` accepted a
passing summary claiming a half-second wait. The gate now rejects an early
request or a summary that disagrees with the trace. A retry can arrive on time
even when the elapsed time measured from the *end* of header emission is just
under two seconds.

### November 18 candidate verification

The startup fix in `1ae5fb91c1` passed 40 native recovery tests on m79 and
89 Python controller tests. The earlier compiler-selection failure at
`20fbacefe3` was resolved with the selector built from aterm `d430a46400`,
which authenticated signed Trust 15698. Protected preparation at
`a5957e1876` resolved both locked dependency graphs.

The following records are relative to `~/.cache/` on the named machine.

| Check | Record | Evidence |
| --- | --- | --- |
| Source gate on m79, `b27192edf9` | `capacity-qualified-main.2LiaIA/{source,check.log,exit-status}` | The log shows native resource admission and compilation progress. The final status records the gate verdict. |
| Private package on the local Mac, `f4d1c167fb` | `capacity-release-preflight.7x3HyuXa/{package-resume-2.log,exit-status}` | After disk space recovered, signed Trust 15698 refused the release build at procedural-macro admission. The producer exited 1 before emitting a package. |
| Recovery checks on that package | `cap-verify.tJJjnD3z/current/{result.json,exit-status}` | Verifier `cb1f072e5b` recorded producer exit 1 and an empty suite map. Its eleven sampling and twelve compaction cases require a candidate executable. |

The November 19 UTC release attempt reached `targo trust build --certify`
with dependency verification enabled. Targo refused the in-process procedural
macros in the `codex-cli::codex` dependency graph before collecting authenticated
compiler evidence. The next package attempt requires a source-selected Trust
toolchain that admits this graph under the project's certification policy, or
a source change that removes the dependency causing refusal. See the
[Trust admission investigation](reviews/targo-macro-admission-2026-11-14.md).

The runner checks the package source and executable digest across both suites.
It runs compaction even when sampling reports a recovery failure. The combined
result requires the package producer and both suites to succeed.

On November 18 the installed client and private candidate shared product
version `0.11.0`. The stable Apple-silicon index (164) selected build `2026111701` from
`70d6391259`. On the Trust machine, `aterm pkg update codex` reported it
current and `aterm pkg verify codex` matched the installed tree to the signed
release. That source predates the streamed-response fix (`03d6154653`) and
the startup-handshake fix (`1ae5fb91c1`).

Private candidate `f4d1c167fb` contains both fixes and passed the
release-version check with upstream baseline `0.169.0`. To identify a later
installation, use `aterm pkg which codex` for its build number and read
`source` in that package's `alab-build.json`. In a running tab,
`/alab version` shows the running image and its selected launch target.

Implementation: [`capacity.rs`](../codex-rs/alab-core/src/capacity.rs),
[`responses_retry.rs`](../codex-rs/alab-core/glue/responses_retry.rs), and
[`upgrade.rs`](../codex-rs/alab-tui/src/upgrade.rs).
