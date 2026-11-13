<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Daybreak safety buffering, interruption, refusal, and recovery audit

The original audit inspected working-tree source based on commit
`9a2fb340cc34d87a501e5aa1a7d5711134d117b4`. The bounded-recovery follow-up began at
`49f3c2ba233ba8f6891d6918309cd035b9257d30` and integrates `origin/main` through
`908df42bb56117d4a544c717876b8e750f58d3e5` in an isolated worktree. Unrelated changes
in other checkouts are not attributed to Daybreak or modified by this work.

## Executive conclusion

The feature most likely meant by “Codex interrupts work when it flags a Daybreak
issue” is **safety buffering**, but that description is not literally what the code
does.

There are three separate paths:

1. **Safety buffering:** the model server says output is waiting for safety review.
   Core forwards a transient notification and keeps the turn running. The TUI says
   “Giving this request a little extra thought.” Nothing is interrupted locally.
   In ordinary interactive use, a person must select retry and then explicitly
   confirm “Stop and retry.” Under the active claimed root harness, no person is
   available: an eligible server-selected faster-model treatment now goes directly
   through that same validated interrupt/fork/resubmit path, once per changed model
   offer and at most twice per command lineage, without a confirmation popup. With
   no faster treatment, status stays non-modal while the server—not a person—owns
   the next outcome.
2. **Terminal cybersecurity refusal:** the server returns `cyber_policy`, which is a
   fatal, non-retried provider error. The turn ends as failed. The ordinary TUI can
   offer Daybreak for the next request and, when valid recourse metadata arrived,
   safer rewrites in a fork that excludes the refused turn. Active-harness recourse
   remains informational rather than becoming another human-blocking picker.
3. **ALab active-harness recovery:** after a regular-turn `cyber_policy` refusal, an
   active, claimed Cruise or Land harness can send one same-thread continuation for
   independent permitted work. It does not retry the refused action, change model
   or account, or enable Daybreak. A second refusal stops that unsafe sequence, not
   every independent root task. Compaction refusals are terminal for the same reason:
   denied history must not be replayed automatically.

Daybreak enablement is related but not the detector. Buffering can appear regardless
of the user's Daybreak toggle. Detection and authorization are server-owned; this
repository contains no local classifier that decides a request is a “Daybreak
issue.”

No critical safety defect was found in the traced implementation. The important
limits are intentional but easy to miss: already-executed side effects are not
rolled back, recovery can fail after the source turn has been interrupted, the
one-click buffering/recourse state is transient, and ALab's independent-work
boundary is a model instruction rather than a client-side proof that subsequent
work is safe. Backend policy remains the ultimate enforcement boundary.

## 1. Components and ownership

| Layer | Responsibility | Main source |
| --- | --- | --- |
| Responses transport | Parse safety-buffering payloads and optional faster-model treatment | `codex-rs/codex-api/src/sse/responses.rs:161`, `codex-rs/codex-api/src/endpoint/responses_websocket.rs:789`, `codex-rs/codex-api/src/safety_buffering.rs:4` |
| Core | Forward a typed transient event; do not interrupt or retry | `codex-rs/core/src/session/turn.rs:3298` |
| App server | Add thread/turn identity and publish `model/safetyBuffering/updated` | `codex-rs/app-server/src/bespoke_event_handling.rs:432` |
| TUI buffering state | Show/hide waiting UI and gate retry eligibility | `codex-rs/tui/src/chatwidget/safety_buffering.rs:31` |
| TUI retry | Confirm, interrupt, validate history, fork, and resubmit | `codex-rs/tui/src/app/safety_buffering.rs:26` |
| Moderation recourse | Validate block metadata, explain it, and offer safer rewrites | `codex-rs/tui/src/chatwidget/safety_recourse.rs:20`, `codex-rs/tui/src/chatwidget/safety_recourse/metadata.rs:53` |
| Rewrite fork | Verify the failed-turn boundary and submit the selected rewrite | `codex-rs/tui/src/app/safety_recourse.rs:18` |
| Daybreak selection | Select Standard/Daybreak per new turn and persist preference | `codex-rs/tui/src/daybreak.rs:8`, `codex-rs/tui/src/app/daybreak.rs:6`, `codex-rs/core/src/cyber_access_program.rs:34` |
| ALab recovery | Permit one bounded independent-work continuation | `codex-rs/alab-autopilot/src/decide.rs:4860`, `codex-rs/alab-autopilot/src/extension.rs:8590` |

The buffering UI is implemented in the interactive TUI. Core and app-server expose
the event for other clients, but Core itself has no faster-model recovery policy.
`codex exec` has Daybreak access-program selection but no corresponding buffering
picker or automatic fork-and-retry implementation.

## 2. Safety-buffering detection and wire propagation

### 2.1 Accepted server representations

`ResponsesStreamEvent` has an optional top-level `safety_buffering` JSON value. A
valid object deserializes into:

```text
use_cases: Vec<String>
reasons: Vec<String>
retry_model: Option<String>       // becomes faster_model
show_buffering_ui: bool           // skipped on deserialize, then forced true
```

The transport also accepts the older nested form on a `response.metadata` event:

```json
{
  "type": "response.metadata",
  "metadata": {
    "type": "safety_buffering",
    "use_cases": ["cyber"],
    "reasons": ["policy-check"],
    "retry_model": "optional-faster-model"
  }
}
```

Top-level presence wins over nested metadata, including an explicit `false`, `null`,
or malformed value. Those invalid forms produce no safety event and suppress the
nested fallback. A valid object forces `show_buffering_ui = true`. Consequently,
`false` can clear/hide the UI when injected through app-server, but a normal valid
transport payload does not carry that boolean through from the server.

See `codex-rs/codex-api/src/common.rs:136` and
`codex-rs/codex-api/src/sse/responses.rs:274`.

### 2.2 Faster-model treatment and precedence

HTTP/SSE response headers and WebSocket metadata headers can supply:

```text
x-codex-safety-buffering-enabled
x-codex-safety-buffering-faster-model
```

Only the faster-model value is retained. The “enabled” header's value is not parsed
and does not gate either the event or the fallback model; tests explicitly pin this
behavior. If the event object contains `retry_model`, that field wins, including an
explicit null. Only when the key is absent does the header-supplied model become
`faster_model`.

SSE captures treatment from response headers. WebSocket processing retains a mutable
treatment and updates it when event headers contain either safety header. See
`codex-rs/codex-api/src/safety_buffering.rs:8`,
`codex-rs/codex-api/src/sse/responses.rs:289`, and
`codex-rs/codex-api/src/endpoint/responses_websocket.rs:1029`.

Malformed objects are ignored rather than synthesized into a retry. Missing model
metadata means no faster-model offer, not an invented model name.

### 2.3 Event ordering and Core behavior

`metadata_events` emits independent metadata in this order: model verification,
turn-moderation metadata, safety buffering, then Guardian assessment. The ordinary
response event is still processed, so adding buffering does not consume text,
completion, or other metadata. Multiple buffering notifications can be delivered.

Core handles `ResponseEvent::SafetyBuffering` only by sending
`EventMsg::SafetyBuffering`. It fills `model` from the active turn's configured model
slug, carries `use_cases`, `reasons`, `show_buffering_ui`, and `faster_model`, and
continues the same turn loop. It does not cancel sampling, change models, fork,
retry, or persist a pending recovery operation (`core/src/session/turn.rs:3298`).

The rollout policy classifies `SafetyBuffering` and `TurnModerationMetadata` as
transient, non-durable events (`codex-rs/rollout/src/policy.rs:162`). This is why the
one-click recovery state is not reconstructed from a rollout after restart.

App-server wraps the Core event as `ModelSafetyBufferingUpdatedNotification`, adding
`thread_id` and the event's `turn_id`, and publishes method
`model/safetyBuffering/updated`. The protocol retains the classification arrays even
though the current TUI buffering popup does not display them
(`app-server-protocol/src/protocol/v2/model.rs:216`).

## 3. TUI buffering state machine

`SafetyBufferingState` contains:

```text
submitted_turn: Option<(turn_id, AppCommand)>
active: Option<ActiveSafetyBuffering {
    turn_id,
    last_retry_model: Option<String>,
    agent_message_started
}>
```

The original `AppCommand` is cloned only after `turn/start` succeeds
(`tui/src/app/user_turn_submission.rs:179`). The display prompt and attachments are
captured separately during composer submission. Thus the retry path uses an accepted
turn ID and the exact structured turn command, not a reconstruction from rendered
terminal text.

### 3.1 When a notification is ignored

`on_model_safety_buffering_updated` rejects a notification when any applies:

- its `thread_id` is not the widget's thread;
- it is initial resume replay;
- no agent turn is running;
- safety recourse is already pending;
- its `turn_id` is not the widget's latest turn;
- visible assistant output already started for this buffering episode.

`ThreadSnapshot` replay can restore waiting presentation, but retry requires
`replay_kind.is_none()`, so replay never supplies the one-click retry command. Side
conversations can show waiting status but never offer retry.

App routing targets notifications to the owning thread's channel. The follow-up fix
also added `ChatWidget::alab_safety_buffering_matches_thread`, called before any
buffering state mutation, so direct delivery cannot show or hide another thread's UI.
The existing stale/hidden/historical-notification regression now includes a
cross-thread negative control.

### 3.2 Waiting and retry eligibility

A visible notification sets `active` and the status header. Retry is offered only
when all are present:

- a server/header-supplied `faster_model`;
- the accepted original `AppCommand` for the same turn;
- the original structured `UserMessage`;
- a current thread ID;
- a live, non-replayed, non-side-conversation submission.

`can_retry_safety_buffered_turn` additionally requires that the turn is still running
and no visible assistant message has started (`chatwidget/safety_buffering.rs:79`).
Any nonempty assistant delta marks output started, dismisses the shared popup, and
permanently removes retry eligibility for that episode. Turn completion and terminal
cleanup clear all buffering state. A hidden update for the current turn dismisses
the popup and restores the normal reasoning status.

Repeated offers for the same model do not reopen the picker or schedule another
headless retry. A changed server-selected model identity is a new offer and can emit
one new automatic action. Removing and later restoring an offer can also rebuild the
picker. “Dismiss and keep waiting” leaves the request running; “Learn more” opens the
fixed Daybreak help URL. The footer explicitly says no action is required and Codex
will keep waiting.

## 4. The actual interruption and faster-model retry

### 4.1 Two explicit user decisions

Selecting “Retry with a faster model” emits `ConfirmSafetyBufferedRetry`; it does not
interrupt. The confirmation view warns:

- the current attempt will stop;
- retry occurs in a new thread;
- file changes and other actions already taken remain;
- the message is sent again with the named model, possibly less capable.

“Keep waiting” closes the confirmation without interruption. “Stop and retry” emits
`RetrySafetyBufferedTurn` and requires explicit confirmation in the selection widget.
Both the confirmation function and the app retry function revalidate turn identity
and eligibility, closing race windows where output or completion arrived while events
were queued (`chatwidget/safety_buffering.rs:240`, `app/safety_buffering.rs:39`).

The headless branch bypasses only the presentation of those two choices, not any
engineering validation. `alab_autonomous_root_active` requires an active Autopilot
mode, an existing claim, and a registered root thread; this mode is off by default.
When a retry offer first appears or its model changes, the widget emits the same
`RetrySafetyBufferedTurn` event used by the explicit confirmation, marked
`autonomous: true`. Duplicate updates are fenced by `last_retry_model`, so they cannot
interrupt/fork twice. The app checks authority and the retry budget again immediately
before interruption. At most two automatic retries are accepted for one original
user-turn command lineage; the count travels in `AppCommand::UserTurn`, is excluded
from serialization, and resets for each new human turn. At the cap, later offers stay
non-modal. No-treatment and recourse states likewise remain non-modal, and no access
escalation or server-suggested rewrite is submitted merely to keep work moving.

### 4.2 Pre-interrupt validation

`App::retry_safety_buffered_turn` first rechecks active-root authority and the
automatic retry budget when `autonomous` is true. It requires active, displayed, and
primary thread IDs all to equal the source thread and `can_retry...` to pass. It then:

1. clones current config and captures the full thread input state;
2. rejects if a permission profile update is pending;
3. requires the saved command to be `AppCommand::UserTurn`;
4. converts the current permission selection to turn overrides, failing before any
   interruption if it cannot be represented safely;
5. resolves the faster model's Daybreak/Standard access program using the current
   account, current toggle, and current model catalog;
6. rewrites the turn's model to the server-selected faster model and reasoning effort
   to `Low`, also updating collaboration-mode model/effort fields when present.

The Daybreak check is significant: enabling Daybreak on the source does not blindly
copy a program unsupported by the faster model. Incompatible advertised support
rejects the retry before interruption. Missing catalog program metadata preserves
server automatic treatment.

### 4.3 Interrupt acknowledgment is terminal, not merely queued

The app calls `turn/interrupt` with exact thread and turn IDs. App-server rejects a
mismatch or missing active turn. For a non-startup interrupt it holds the RPC response
until Core's `TurnAborted` arrives; pending interrupt requests are answered just before
app-server emits the corresponding `TurnCompleted` notification with status
`Interrupted` (`app-server/src/request_processors/turn_processor.rs:1475`,
`app-server/src/bespoke_event_handling.rs:1269`).

Thus successful return from the TUI's `turn_interrupt` is strong evidence that Core
aborted the source turn, but not proof that the UI already consumed the completion
notification. The later history read and explicit not-`InProgress` check establish the
source state needed for forking.

### 4.4 History validation and input reconstruction

After interruption the app reads the source thread and hydrates history:

- Legacy history is hydrated through the ordinary initial-history path.
- Paginated history fetches the latest turn page and the interrupted turn's item page.
  A further item page is an error: the code refuses to retry with incomplete input.

`safety_retry_fork_point` requires the interrupted turn to exist, be latest, and not
be running; an immediately previous turn, if present, also must not be running
(`app/safety_buffering.rs:280`). These checks prevent forking over newer work or an
unresolved predecessor.

The new fork is created with `before_turn_id = interrupted_turn`, excluding the whole
interrupted turn. The retry input starts with the original accepted command's items.
The app then finds user-message items in the interrupted source turn, skips the first
(the original prompt), and appends later user content with newline separators. This
preserves committed steering input while dropping assistant/tool items from the new
conversation history. Tests verify the original prompt is not duplicated and accepted
steers survive.

Only conversation history is excluded. Filesystem changes, processes, network calls,
and other already-performed side effects are not undone.

### 4.5 Fork, goal, permission, and draft handling

The fork uses the cloned current config, inherited selected server profile, and
`ForkGoalContinuation::DeferUntilNextTurn`. This copies goal state without starting
an automatic goal continuation ahead of the user's explicit retry. Tests verify the
retry runs first and goal continuation follows afterward, with prior usage preserved.

The app adopts approval/reviewer/active-permission values returned by the fork,
detaches the source, attaches the fork with lineage presentation, restores the saved
composer/queue state, and marks the retry submission pending. The normal turn
submission path runs again, including Daybreak selection and permission resolution.
Only after acceptance does it commit the retry display and record the new accepted
turn for a possible later buffering retry.

A faster turn can itself buffer and enter another fork. Ordinary interactive attempts
require fresh user confirmation and have no new client-side cap. An active claimed
root accepts at most two automatic retries for the original command lineage; tests
cover the second retry and suppression of a third. The accepted command carries the
count through later offers, while each new human turn starts a fresh lineage.

### 4.6 Failure semantics

Validation failures before `turn/interrupt` leave the original turn running. Failure
after interruption can leave it stopped without a completed retry. Interactive failure
handling restores the captured input state and original prompt to the composer and
reports “Failed to retry with a faster model.” A missing source rollout test verifies
that an unsent draft survives alongside the restored prompt.

An authorized automatic failure instead calls ALab's bounded continuation helper.
It captures the widget's current thread (the source before a fork, or the attached fork
if a later step failed), rechecks root authority there, and returns without submission
if a different tracked turn is active. The acknowledged
source interruption is explicitly exempted: its completion notification can still be
waiting in the local cache even though the server has terminated that turn. The helper
clears that exact ID from the submission cache because non-user harness submissions
otherwise refuse to start while any active ID is present. It clears no other turn. A
failure before interruption passes no such exemption and therefore cannot displace the
running source. Otherwise the helper clones the original command, applies
`turn_trigger = "alab_autopilot"`, assigns a fresh
`alab-autopilot-<uuid>` client ID, increments the same retry budget, and prepends a
recovery instruction to the original structured input. That instruction requires
state inspection, preservation of completed work, no repeated side effects, no human
prompt for routine work, and only independently authorized work. Native submission
attribution is bound to the complete text and fresh ID.

Successful fallback submission leaves the new recovery turn running and preserves
only the pre-existing unsent draft, not a machine-generated prompt that would block
the supervisor. Rejected turns owned by the active harness likewise skip composer
restoration; other rejected messages keep the existing behavior. If authority has
disappeared, the original prompt is restored for interactive handling. If fallback
submission fails, the original input state is restored and an error is reported. The
end-to-end regression deletes the source rollout, dispatches the actual automatic
event, and observes a distinct live recovery turn without rereading the deliberately
corrupted history file.

This is not an atomic transaction and cannot be made one merely by forking: the
source interruption and external side effects have already happened. Automatic
continuation reduces the avoidable idle window but cannot guarantee admission by an
unavailable or refusing service, and it never invents authority after the root claim
is gone.

## 5. Terminal `cyber_policy` refusal

### 5.1 Provider error to failed turn

A `response.failed` error with code `cyber_policy` maps to `ApiError::CyberPolicy`;
blank messages get a fixed cybersecurity-risk fallback. Retry advice does not make
this error retryable (`codex-api/src/sse/responses_error.rs:56` and its tests).
Protocol conversion produces `CodexErrorInfo::CyberPolicy`, and app-server exposes a
failed turn/error notification.

The TUI deduplicates the non-retry error and later failed completion using
`last_non_retry_error`. `on_cyber_policy_error` disables automatic resubmission of
pending steers, finalizes turn UI state, and adds a dedicated safety notice instead
of displaying an arbitrary provider fallback (`chatwidget/turn_runtime.rs:440`).
Ordinary queued follow-ups are subject to the normal queue gates; if a Daybreak or
recourse picker is shown, `defer_input_until_settings_applied` holds autosend until
the selection close has been fully processed.

### 5.2 Notice selection

The notice depends on current account eligibility, model catalog, toggle, model, and
side-conversation state:

- `Apply`: authorized users can apply for Daybreak;
- `Disabled`: Daybreak is available but off, so offer enablement for the next turn;
- `Astra`: the selected Astra model has Standard but no Daybreak;
- `Enabled`: some requests remain limited even with Daybreak on.

The enable action does not resubmit the refused prompt. It stores a preference for
new turns. Replay, parent-owned/blocked input, and side conversations suppress the
ordinary modal enable picker. ALab has an additional active-harness suppression
described below.

## 6. Safety recourse after a block

Safety recourse is not the same signal as safety buffering. It uses
`openai_chatgpt_moderation_metadata`, usually after a buffering/loading phase and
before the terminal policy error.

### 6.1 Metadata validation

`chatwidget/safety_recourse/metadata.rs` accepts schema version 1 only and denies
unknown fields in its typed wire structures. It supports `bio` and `cyber` domains,
an allowlisted set of source/source-message combinations, and an allowlisted set of
topics. The explanation is generated locally from those allowlists, not accepted as
arbitrary server prose.

A loading `safety_block` creates turn-scoped pending state and displays “Finding an
explanation and safer ways to continue….” A full `safety_recourse` must include
exactly three wire suggestions, although unsafe entries can subsequently be filtered
out. Validation includes:

- bounded nonempty source ID, labels, and prompt text;
- unique trimmed prompt text and unique case-insensitive labels;
- no control/new-row tricks in labels;
- effective prompt limit of 1,000 bytes;
- no leading `!` or `/`, async-question reply markup, or tool mentions;
- server-supplied “Enable Daybreak” suggestions are discarded for cyber recourse;
  enablement is added only by trusted client code.

A matching terminal error is deferred while recourse is pending so the user does not
first see a generic failure. Recourse renders only when the final error domain
matches and no queued follow-up supersedes it. It then finalizes the turn, displays
the explanation, and offers the surviving rewrites. Trusted client code can also add
“Enable Daybreak” when the current notice is `Disabled`.

### 6.2 Rewrite fork verification

Selecting a rewrite emits `ContinueSafetyRecourse`. The app requires embedded or
remote app-server version at least 0.151.0 because older remote servers silently
ignore the required `safety_recourse_follow_up` turn trigger. It also requires the
active primary thread, matching latest turn, no running turn, and no queued
follow-ups (`app/safety_recourse.rs:9`).

Before forking it verifies that the source is non-ephemeral and the target is still
the latest failed turn. It forks before that turn with `StartIfIdle` goal semantics,
then verifies:

- the fork is attached to the expected parent and does not block direct input;
- the source's latest turn is still the same failed turn;
- the fork boundary is exactly the source turn before the blocked turn.

It preserves current model, reasoning effort, Daybreak setting, and inherited
permissions, saves source input state, attaches the fork, and submits the selected
text with shell escaping disabled and `turn_trigger = safety_recourse_follow_up`.
The blocked turn and its outputs are absent from the rewrite fork. Submission errors
restore the selected prompt to the composer rather than dropping it.

Unlike ALab's same-thread continuation, this fork does not carry the refused request
into the new model context.

## 7. What `/daybreak` actually changes

`daybreak = true` in config is the default for new threads/non-interactive turns;
per-thread nullable metadata overrides it for resumed threads. The TUI writes both
thread metadata and the default config preference. SQLite migration 0054 adds the
thread column; ephemeral threads reject explicit Daybreak metadata. Forks inherit
source metadata.

Both `cli_daybreak` and `api_key_cyber_access_programs` are stable and default-enabled
in the inspected tree, but API-key use requires both. The provider must advertise
support, and the account must be ChatGPT or API-key. Unknown account state preserves
a resumed preference temporarily; known ineligibility disables effective use.

For each new turn, `tui/src/daybreak.rs::program_for_turn` applies these rules:

- ineligible account plus enabled preference: reject with an actionable error;
- ineligible account plus disabled preference: send no explicit program;
- missing model/program metadata: send no explicit program, preserving backend
  automatic behavior;
- enabled: prefer `daybreak_blue`, then `daybreak_red`, and reject if the catalog
  affirmatively offers programs but no Daybreak program for that model;
- disabled: select `standard` if advertised, otherwise omit the field.

Core filters provider support again, resolves against current authentication, and
serializes the request field as:

```json
"access_programs": {"cyber": "standard|daybreak_blue|daybreak_red"}
```

Catalog metadata is discovery, not an entitlement grant. The server still enforces
account authorization and model-tier restrictions. A model being absent from the
catalog likewise preserves automatic treatment rather than inventing a denial or
grant.

`codex exec` implements the same broad selection with app-server account/catalog
RPCs. Its explicit `--cyber-access-program` option bypasses this discovery helper and
supplies the selected program directly; the option is rejected for `review` and for a
fork with no prompt. After the follow-up fix, an exec model-list RPC failure is an
error when Daybreak was explicitly requested, rather than silently dropping that
selection. With Daybreak disabled, the same failure still returns no explicit program
and preserves backend automatic treatment. Unit tests cover enabled failure,
disabled fallback, and successful replies for both preferences.

## 8. ALab active-harness recovery

### 8.1 Eligibility and the one-shot rule

ALab classifies `CyberPolicy` as `Wall::Policy`; Bio and misalignment policy failures
remain fatal (`alab-autopilot/src/extension.rs:7855`). A policy continuation is
considered only for a regular native turn with regular user input. If compaction is
active/pending or no regular input was seen, the error becomes a fatal compaction
wall because another request could replay denied history before reaching the new
instruction (`extension.rs:8633`).

At `Wall::Policy`, `decide_wall` requires `continue_on`. If no policy continuation has
yet been spent, it chooses `policy-continue@v1` with `POLICY_CONTINUE`. The act still
uses the ordinary pacing and pre-act authority: active proven claim, current policy
and native point, no hold or newer turn, human grace, empty composer/no pending input
or modal, goal admission, and final revalidation immediately before Core submission.
It is not sent by the buffering notification itself.

The exact instruction tells the model to abandon the refused action; not repeat,
rephrase, work around, change model/account, or enroll/enable Daybreak; preserve
existing work; and continue only clearly allowed, independently authorized work. If
none exists, it must emit `POLICY-WORK-EXHAUSTED` alone on a line and stop
(`alab-autopilot/src/words.rs:77`).

The continuation uses the same thread, model, account, and retained access program.
It therefore does not create the clean-history boundary that the TUI recourse fork
creates. Its protection is the explicit instruction plus continued backend policy,
not removal of the refused request from model context.

### 8.2 Later prompts, second refusal, and exhaustion

`ThreadState::acted` marks `policy_continued` for this sequence. Subsequent automatic
text/resume acts receive the longer `POLICY_BOUNDARY` suffix, prohibiting retry,
rephrasing, splitting, disguise, delegation, or another model/account/access program.
No aterm fact rider is fetched or appended after that boundary
(`decide.rs:3696`, `extension.rs:4492`).

A second `CyberPolicy` after the continuation produces standing attention explaining
that the independent-work continuation was also refused; there is no further retry
or follow-up timer. A new explicit person turn begins a new sequence. Goal-triggered
turns do not rearm the one-shot continuation.

If an assistant reply contains `POLICY-WORK-EXHAUSTED` as a trimmed whole line in the
latest or an earlier assistant message, the decider replaces automatic type, usage
resume, or compaction acts with attention. Resume reconstruction retains this stop
marker only from native assistant evidence at a complete point, not a user's quote.

### 8.3 Persistence and compaction boundary

Native rollout history records turn starts, attribution, user input, access-program
turn context, and terminal error. `resume_history::policy_continued` conservatively
reconstructs a spent continuation after a refusal from an attributed harness start,
even if its user message had not reached disk before a crash. Explicit `user` or
`queue` turns reset the sequence; copied prompt text by itself is not proof.

The special owed-intent recovery path can retain the exact policy instruction only at
the matching completed CyberPolicy origin with regular input and no compaction. A
newer turn, correction, cancellation, or incompatible origin revokes it. This is
intent custody, not authority to bypass current policy/input/goal checks.

Pre-, mid-, and post-turn compaction refusals are deliberately fatal: the tests show
that no automatic sampling follows because the denied context would otherwise be
replayed. A post-turn refusal retains the completed answer and records the terminal
policy error.

### 8.4 The Daybreak offer is informational under an active harness

The ordinary TUI can show a modal “Turn on Daybreak for your next request?” picker.
ALab suppresses that modal only when its shared state says Autopilot is active, a
claim exists, and the current thread is a registered root. The refusal notice stays
in history, `/daybreak` remains available to the person, and no automatic enable or
browser action occurs (`alab-tui/glue/chat_hand.rs:33`).

This prevents the access picker from stealing the composer while the harness advances
permitted work. Tests cover active/passive/off, claimed/unclaimed, root/non-root,
unsent drafts, queued corrections, and the app-level close/settle handshake.

## 9. Security and reliability assessment

### 9.1 Properties the implementation actually enforces

- Buffering alone never issues `Op::Interrupt`; interactive faster retry requires
  two user actions. Only an active claimed root may emit the same validated retry
  automatically, and only once per changed server-selected model.
- Automatic authority and the two-retry budget are revalidated at execution time,
  after the widget queues the event and before any interruption.
- Retry eligibility is bound to the running turn and revoked by visible output,
  completion, hidden updates, side conversations, replay, or thread mismatch at the
  app layer.
- The interrupt RPC validates active turn identity and acknowledges terminal abort.
- The retry/recourse fork excludes the target turn and verifies source/fork history
  boundaries before submission.
- Original structured input and accepted steers are preserved without scraping
  rendered output or copying tool results into the retry.
- Permissions and Daybreak compatibility are validated before interruption where
  possible; server authorization remains authoritative afterward.
- Post-interruption automatic failure makes one bounded continuation attempt with
  fresh native harness attribution rather than leaving a supervisor-blocking draft.
- Recourse suggestions are bounded and interpreted as text, never as slash commands,
  shell commands, question replies, or tool mentions.
- Safety-buffering and moderation metadata are transient rather than silently
  reauthorized after restart.
- ALab cannot use this path to enable Daybreak, choose an arbitrary model/account,
  create a refusal retry ladder, or continue an unsafe sequence after a second
  refusal/exhaustion marker. Only the server-selected faster treatment is automated.
- Compaction refusals fail closed against replaying denied history.

### 9.2 Residual risks and limits, not confirmed vulnerabilities

1. **Side effects survive.** The confirmation is accurate: interruption/forking is
   not rollback. A flagged turn may already have changed files or run commands.
2. **Recovery still has external failure limits.** Rollout reads, hydration, forking,
   attachment, or submission can fail after the source is interrupted. The bounded
   fallback can itself be rejected or fail to start when storage, transport,
   authentication, or the server is unavailable. It reports that outcome and restores
   the original input state; it cannot truthfully guarantee progress in those cases.
3. **One-click state is ephemeral.** Buffering/recourse notifications and the saved
   original command are not durable. A restart can retain the failed/interrupted
   thread but not the same one-click retry or recourse picker.
4. **The server is the classifier and treatment authority.** The client does not
   independently assess `use_cases`/`reasons`, prove that the faster model is safer,
   or prove that a rewrite is appropriate. It constrains presentation and history
   mechanics while backend policy enforces access.
5. **ALab's safe-work boundary is instructional.** Same-thread continuation retains
   history, and Core does not statically prove the next action is independent. This
   is intentionally different from recourse's excluded-turn fork; backend refusal
   remains the backstop.
6. **The faster treatment is not proven to succeed.** Interactive retries can repeat
   with fresh confirmation. Automatic retries stop after two accepted attempts per
   command lineage, and the bounded fallback still depends on server admission.
7. **Header naming is not semantics.** `x-codex-safety-buffering-enabled=false` does
   not disable event-driven UI or the faster-model fallback. This is pinned behavior,
   not a toggle users should rely on.

### 9.3 Follow-up resolutions

The audit's engineering observations were addressed rather than left as mere
routing/discovery caveats:

- `ALAB(daybreak)` now calls an ALab-owned thread-match guard before handling a
  buffering notification, with a cross-thread regression case.
- `ALAB(daybreak-discovery)` now rejects catalog RPC failure only for an explicitly
  enabled Daybreak turn; disabled turns preserve automatic treatment. Focused unit
  tests cover the three result classes.
- The active root no longer waits for a person to answer Daybreak safety UI. A
  changed faster-model offer emits one validated retry; no-treatment and recourse
  states remain non-modal. Duplicate events do not fork twice, unsent drafts remain,
  and interactive confirmations are unchanged.
- The later lifecycle audit found and fixed five additional gaps: failed automatic
  recovery no longer restores a blocking draft, automatic retries have a two-attempt
  circuit breaker, authority and budget are rechecked at execution, changed model
  identity is recognized as a new offer, and tests exercise the actual queued event
  through app dispatch and its fallback submission lifecycle.

The earlier TUI SIGABRTs were diagnosed separately as insufficient libtest stack for
large Core/TUI futures. Upstream already prescribes `RUST_MIN_STACK`; running with
16 MiB made all 27 initially targeted TUI safety tests pass, so no speculative
production patch or upstream test rewrite was added.

## 10. Existing regression coverage

The inspected tests cover substantially more than the happy path:

- SSE and WebSocket parsing, header fallback/precedence, malformed/top-level forms,
  independent metadata ordering, and Core event forwarding;
- popup wording, duplicate and changed-model updates, hidden/stale/historical
  notifications, side conversations, completion/output races, and mandatory
  interactive confirmation;
- first-turn and later-turn retries, accepted steers, the two-automatic-retry cap,
  execution-time authority loss, interrupted predecessors, goal
  continuation/accounting, unsupported permissions, pending permissions,
  incompatible Daybreak models, and interactive draft restoration after failure;
- actual automatic-event dispatch, post-interruption fallback to a distinct running
  recovery turn, native recovery attribution, and rejected harness turns that do not
  become blocking drafts;
- cyber/bio recourse, domain mismatch, locally generated explanations, suggestion
  filtering, fork history exclusion, and turn-trigger metadata;
- Daybreak toggle persistence, auth/provider/model gates, and request serialization;
- ALab one-shot continuation, identical model/account/program, second-refusal stop,
  exhaustion, restart custody, goal non-rearmament, nonmodal draft/queue behavior,
  and pre/mid/post-turn compaction refusals.

Key files are `codex-rs/core/tests/suite/safety_buffering.rs`,
`codex-rs/tui/src/chatwidget/tests/app_server.rs:287`,
`codex-rs/tui/src/app/tests/safety_buffering.rs`,
`codex-rs/tui/src/app/safety_buffering_tests.rs`,
`codex-rs/alab-autopilot/tests/autopilot/policy_continuation.rs`,
`codex-rs/alab-autopilot/tests/compaction_recovery.rs:978`, and
`codex-rs/alab-tui/glue/daybreak_*_tests.rs`.

### Validation status

Static document checks passed on the bounded-recovery update: balanced code fences,
no trailing whitespace, and 42 full and crate-relative file/line citations verified
to exist and be in range. The work remains isolated from unrelated edits in other
checkouts.

On the bounded lifecycle implementation integrated with
`4b44e195a0c8667762b9b4c1ef98ddd96c12c049`, `targo --unverified check -p codex-tui
--tests` and the standard `targo --unverified build -p codex-cli --bin codex` passed.
The focused matrix passed 76 unique tests: 34 TUI safety tests (including actual
automatic dispatch, the two-retry cap, authority loss, and live post-failure
continuation), one rejected-harness-turn regression, 13 TUI Daybreak tests, seven ALab
policy-continuation tests, ten API safety-buffering tests, two Core safety-buffering
tests, and nine exec Daybreak tests. `RUST_MIN_STACK=16777216` remained set for the
large Core/TUI test futures.

The initial test campaign exposed default-stack SIGABRTs in two large TUI tests and
one Core test. Rerunning with `RUST_MIN_STACK=16777216`, consistent with upstream's
documented enlarged test stacks, resolved all three without an upstream test patch.
The following 70-test result applies to the earlier automatic-retry implementation,
before the bounded lifecycle follow-up:

- 29 TUI safety tests passed, including both original abort sites, the cross-thread
  negative control, automatic headless retry with duplicate suppression, and
  non-modal active-harness recourse;
- 13 TUI Daybreak tests passed with the broad `daybreak` filter (the narrower
  `daybreak_` run's 11 tests are included, not double-counted);
- 7 ALab policy-continuation tests passed (4 library and 3 integration);
- 10 API safety-buffering tests passed;
- 2 Core safety-buffering tests passed with the enlarged stack;
- 9 exec Daybreak tests passed (3 new discovery-policy units and 6 integration).

That is 70 unique passing tests and zero remaining targeted failures. After merging
current `origin/main` at `53ef18977e5d3657b202e2ec50abb46a1ae39636`, the complete
build and all six test commands passed again with the same counts. A direct
Tippy run found an unrelated upstream `needless_late_init` error in
`tui/src/bottom_pane/async_questions/layout.rs:55`; it was not rewritten.

On the bounded-recovery tree integrated through
`1c6984189da8108b4a39f318ea3c9ded34be5560`, locked source/pristine preparation
succeeded and the protected `alab/check.sh` ran its real native producer. Tippy found
nothing in ALab's code (41 upstream diagnostics were set aside), the gate built and
executed `codex-cli 0.0.0`, and every new Daybreak recovery test passed again in that
phase. The complete protected gate **did not exit zero**. Unrelated protocol,
app-server-transport, and other hooked tests encountered sandbox `PermissionDenied`
failures for temporary paths and local sockets. Its final attribution refusal named
`app::alab_session::upgrade_finish_order_tests::a_native_finish_waits_for_tui_catch_up_before_continuing_past_an_upgrade`:
that upgrade test failed its idle-TUI handoff assertion and matched zero tests in the
pristine upstream copy. Neither that test nor the other failing components are in the
Daybreak delta. They were not rewritten or skipped to manufacture a green gate, and
this paragraph is explicitly not a passing-gate claim.

The documented locked metadata preparation completed for both source and pristine.
On implementation merge `8e0983d280017c8aa78456a6ecff590b42cab3b4`, the protected
`alab/check.sh` used the real native producer and reported no Tippy finding in ALab's
code (40 upstream findings set aside). It built and executed `codex-cli 0.0.0`.

The first complete protected gate **did not pass**. Two `codex-alab-release --lib`
tests, `actual_kernel_policy_preserves_sources_and_credentials_and_permits_outputs`
and `native_wrapper_runs_with_scrubbed_environment_and_read_only_source`, failed
inside the gate's outer sandbox with a missing source read and `PermissionDenied`
creating `/tmp/.ubd/cr.ExLO7Y`. Both passed when rerun directly outside that outer
sandbox. The Daybreak merge made no `codex-rs/alab-release` change. This was an
unrelated nested-sandbox test-environment failure, not a Daybreak test failure. That
gate stopped in the ALab test phase, before its hooked-upstream unit-test phase. No
unrelated release code was changed or test skipped by this work to manufacture a
green gate.

`c6aa4ee58e9c819bcbc9bd1ccb81f162c761c493` subsequently moved the native kernel
controls outside the inherited sandbox. The rerun on merge
`443c09908d96120a64d152c75d64d027d90a2ada` used that native-stage handling instead
of failing the original two nested-sandbox tests. It then stopped at a different,
unrelated `codex-alab-release` test:
`reuse_detects_extra_files_permissions_and_executable_replacement` did not observe
an error after its setuid-permission change inside the outer sandbox. That test
passed when rerun directly outside the gate. The second gate again reported no ALab
Tippy findings and built and executed the CLI, but did not reach the hooked-upstream
unit-test phase. The Daybreak change contains no release-package code; this remaining
repository-wide gate blocker is explicitly not a passing-gate claim.

## 11. Future-self map

When revisiting this code, do not start at `/daybreak` and assume it contains the
interruption. Start with these four symbols:

```text
ResponsesStreamEvent::safety_buffering
ChatWidget::on_model_safety_buffering_updated
App::retry_safety_buffered_turn
decide_wall(... Wall::Policy ...)
```

Do not conflate:

- waiting for review with a terminal refusal;
- a user-confirmed interrupt with automatic interruption;
- a same-thread ALab continuation with a recourse fork that excludes the refused
  turn;
- catalog discovery with entitlement;
- conversation forking with filesystem/process rollback;
- transient UI state with durable permission to retry.

The critical user-visible invariant is: **buffering waits by default; only explicit
confirmation stops the attempt.** The critical ALab invariant is: **one bounded
independent-work continuation, never the refused action again, and never automatic
Daybreak escalation.**
