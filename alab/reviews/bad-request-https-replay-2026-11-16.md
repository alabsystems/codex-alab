<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# The opaque Bad Request arrived on HTTPS

Two decisions.

1. An opaque `{"detail":"Bad Request"}` over HTTPS is replayed once unchanged.
   Rejected again, the replay drops the turn's `x-codex-turn-state` token. The
   transport is kept. This replaces the rule of
   [the 2026-11-13 evening review](bad-request-2026-11-13-evening.md), which
   moved the session to HTTPS on the first opaque rejection.
2. An HTTPS fallback that followed failed requests ends once requests have
   gone 10 minutes without failing. Upstream keeps it for the rest of the
   session.

The backend's reason for the 400 is still not observed. What is new is where a
rejection is known to have arrived, and what the client does there.

## What the records show

Three root sessions on this Mac ran package `2026111301` (source `d75c9b47196`,
upstream base `ce3031018ef`) on 2026-11-15. That source has no opaque-400
handling: a 400 maps to `CodexErr::InvalidRequest(body)` and ends the turn, as
upstream's does.

| | A | B | Control |
| --- | --- | --- | --- |
| Thread | `01a1d735` | `01a1dcce` | `01a1dcd1` |
| Turn began (UTC) | 11-14 23:02:43 | 11-15 02:44:14 | 11-15 02:14:38 |
| Stall ending 09:06:16–09:06:18 | 168.9 s | 149.4 s | 167.5 s |
| Turn ended | 13:39:48, Bad Request | 12:34:50, Bad Request | 15:40:53, completed |
| Requests after the stall, same turn | 539 | 484 | 1,333 |
| Rejection after last input | 633 ms | 282 ms | none |

A's root thread was on HTTPS when it failed.

- Its threadless log partition is complete from 11-14 23:20. The root's five
  compactions before the stall (01:50 to 07:36) left no span-less SSE rows. The
  two after it (09:42 and 12:15) did, `keepalive` among them. The HTTPS SSE
  reader is spawned without a span and only it sees `keepalive`; a WebSocket
  compaction logs inside `responses_websocket.stream_request`, as the same
  process's child threads do. The switch falls between 07:36:45 and 09:41:59.
- The backend closes a Responses WebSocket at 60 minutes (a child thread logs
  the limit and reconnects). The root logs no connect and no limit warning in
  its last 2 h 19 min of retained rows.
- The five `connecting to Responses WebSocket` rows at 12:08–12:24 look like the
  root's socket and are not: their span is
  `thread_spawn:session_init:model_client.websocket_connection`, a child prewarm.
- No connect row exists at 13:39:48, so the 400 was not a rejected handshake.

The text agrees: a WebSocket error frame surfaces as the whole
`{"type":"error",...}` payload, which the opaque classifier refuses; a bare
`{"detail":"Bad Request"}` is an HTTP response body.

No row records the fallback itself, in any of the three processes. That the
stall is when it happened is inferred from the latency floor, which steps up at
09:06 in all three and stays up (A: first model item 1.1–1.6 s before, 2.0–2.9 s
after).

B's transport at its failure is inferred, not shown. Its rows from that hour are
pruned. Its latency floor has the same step and holds it through 12:34, and its
process shows HTTPS compactions five hours later. A rejected WebSocket handshake
cannot be excluded for B, because a handshake's body also arrives bare.

The earlier incident (thread `01a1ce34`, 2026-11-14 01:30) left no transport
evidence and is not on this Mac.

Ruled out for A and B: request size (A failed near 1.1 MB and 150,744 of 258,400
tokens, in a turn whose earlier windows passed 2 MB), content (B's 46 KB output
is valid UTF-8 without control bytes; A's was 46 bytes of ASCII), idle time (five
earlier 60 s waits in A were followed by accepted requests), compaction, and
credentials (the access token was unchanged and more than 52 hours from expiry;
the rate-limit identity is the same before and after). A child thread of another
process completed requests 0.8 s before and 1.5 s after A's failure, which rules
out an account-wide outage. That thread was on WebSocket, so it says nothing
about the HTTPS path.

## What the client replays, and what is not known

Every HTTPS request of a turn carries the routing token the turn's first
response issued, if it issued one (`core/src/client.rs`,
`build_responses_headers`). The token is a `OnceLock`: set once, never
refreshed, and kept by `try_switch_fallback_transport`. Upstream's contract is to
send it unchanged within a turn and never across turns, and upstream drops it
mid-turn when auth ownership changes.

Both failed turns began before the 09:06 switch and failed 3.5 and 4.5 hours
after it. If the backend had issued them a token, each replayed a
WebSocket-issued token on every POST for that long. Whether it had is not
recorded anywhere: no log row and no rollout carries the header.

The follow-up requests do not settle it either. B's next request was accepted
3 m 13 s later in the same process, but it also carried a new user message and
began a new turn. A's was 16 h 20 min later, in a new process of a newer package,
over WebSocket.

So the token is a candidate, not a finding, and a weak one: a token the HTTPS
path refused would be expected to fail at once, not after 500 accepted requests. The control turn crossed the same
switch and made 1,333 more requests without a rejection. Turns that began on
HTTPS that day made 3,310 requests without one (B 1,918 over five turns, the
control 1,392), against two rejections in 2,356 requests in the turns that
crossed. That does not separate the token from a transient rejection on the
full-body POST path.

Both rejections came hours into a fallback that one three-minute outage had
made permanent. Whatever the cause, a session that returns to its WebSocket
after ten quiet minutes is not on that path hours later.

## What was wrong in the recovery

`responses_retry.rs` asked for `try_switch_fallback_transport` on any opaque
rejection, "to replay without reusing websocket continuation state". It assumed
a WebSocket without transport evidence.

- Where the session is already on HTTPS, as A's was, the switch returns `false`.
  The retries repeated the identical request with the identical token, so a
  rejection the token caused would have ended the turn as before.
- An opaque rejection cannot be a WebSocket frame. On a WebSocket session the
  rule could fire only on a rejected handshake, and it then left the session on
  HTTPS for good.

## The replay

`request_failed` (`alab-core/glue/responses_retry.rs`), at the top of the retry
handler, counts opaque rejections of the request over HTTPS:

- the first is retried unchanged;
- a later one, with retry budget left and a token held, calls
  `ModelClientSession::alab_discard_routing_state`
  (`alab-core/glue/transport_model_client.rs`), which replaces the turn-state
  cell as upstream's auth-owner branch does. The next request carries no token
  and adopts the one its response issues;
- the one that spends the budget changes nothing: nothing is replayed;
- a rejected WebSocket handshake changes nothing: a handshake carries no token.

The immediate switch is gone. The retry budget, provider deadlines and the
exhausted-budget fallback are upstream's. A rejected handshake now spends that
budget on the socket before HTTPS. With a budget of one, no
replay without the token happens. A background compaction started earlier in the
turn took the token by value and can still send it once.

Each opaque rejection logs `Opaque backend rejection` with `transport`,
`replay`, `turn_state`, `https_rejections` and `retries`
([fields](../REQUEST-DIAGNOSTICS.md)). The next occurrence says whether a token
was held at all and which replay cleared it.

## The way back from HTTPS

Upstream's fallback sets `disable_websockets` and never clears it. Here one
outage of three minutes left three sessions re-sending 1–2.5 MB of history on
every request for the rest of their lives, at about twice the latency floor.
Any failure that spends the stream retries on a WebSocket session does this, in
every build, and since upstream `bc3ccba6e1c` so does one oversized request.

Upstream built a way back (`0a2dda4b5e7`, #58950: a background handshake during
fallback, then a verified request) and withdrew it the same day (`44604499214`,
#58989) without giving a reason. Its rules are kept here and its machinery is
not: no background probe, no second socket, no change to how a request is built.

`codex_alab_core::https_fallback` holds the policy, and
`alab-core/glue/transport_model_client.rs` drives it from hook lines:

- `force_http_fallback` records the fallback. If a request failed since the
  last one began, the fallback is the client's own and will end. Otherwise the
  server refused the upgrade (HTTP 426) and it stays, as it did upstream.
- `stream` asks, before choosing a transport, whether the fallback is due to
  end. A retry the shared handler ordered never ends it: it stays on the
  transport its request failed on. Otherwise, once the current interval (10
  minutes at first) has passed since the fallback and since the last failure
  that postpones it, the flag is cleared.
- The request that returns sends what a turn's first request sends: its full
  input on its own socket, without the routing token HTTPS issued. Upstream's
  recovery kept the token across the return; dropping it is the cautious choice
  while a token carried across transports is a candidate cause.
- That request is a trial. If the transport does not carry it, the retry
  handler's hook orders the fallback again and the request is replayed over
  HTTPS with its retry budget unspent and without a token the socket may have
  issued, as upstream's recovery did. That holds even where core calls the
  socket's error terminal, such as a 403 from a middlebox that refuses the
  upgrade: the refusal is the socket's, not the request's. A failed return costs
  one attempt: the 15 s connect timeout, or the stream idle timeout where a
  middlebox accepts the upgrade and answers nothing.
- "Does not carry it" means a stream failure, a timeout, a connection failure,
  or an HTTP status with the URL it was asked of, which is a refused upgrade. A
  status inside an error frame, a refusal, a filter and a capacity or rate
  limit are answers the server gave over the socket, and are not a failed
  return. An unclassified `response.failed` surfaces as a stream failure and
  counts as one, whichever side produced it.
- A return counts as working once a request begins on it 10 minutes or more
  after it. One that falls back sooner doubles the interval (20, 40, 80
  minutes), and after four such returns in a row the session keeps HTTPS.

Stream failures, timeouts, connection failures and server errors (5xx) on HTTPS
postpone the return, so an outage both transports share does not spend the four
returns. Refusals, capacity answers and opaque rejections do not postpone it.
The intervals are measured on a clock that stops while the Mac sleeps.

These limits are known and left:

- The legacy local compactor has its own retry loop and its own session. Its
  existing hook marks its failures and gives up a returned socket on trial,
  after which the loop replays the request only if it has a retry left. A
  capacity wait there is not seen as a retry, nor are the weekly-reset replays
  in any loop: such a request can end a fallback that is due, and is rebuilt in
  full either way. When the compactor's session is the one that returns, the
  turn's own session keeps the token HTTPS issued it.
- A request that failed and was not retried (the turn ended on it, or steering
  replaced it before the handler ran) leaves its mark or misses it. The next
  request is then taken for a retry, or a failed trial is not seen and its
  socket gets upstream's ordinary budget.
- A review delegate or guardian reviewer spawned during a fallback copies its
  parent's transport into its own provider configuration, as upstream wrote it
  for a fallback that never ended. It stays on HTTPS for its own lifetime.
- A 426 answered to a turn's prewarm, right after a turn that ended on a failed
  request, is recorded as the client's own fallback. The return then meets the
  426 inside `stream`, where it is recorded as the server's.

## Validation

Run with the unverified test lane (`targo --unverified test`,
`RUST_MIN_STACK=8388608` as the gate sets it). The first three were run before
landing and again after merging main. The last three were run on the landed
commit `a72fe094764`, with the helpers the gate prepares
(`codex-code-mode-host`, `test_stdio_server`) and the V8 inputs `alab/v8.py`
resolves:

- `-p codex-alab-core --lib`: 368 passed, the eleven `https_fallback` policy
  tests among them.
- `-p codex-alab-autopilot --test websocket_return`: 4 passed. Real core turns
  behind one origin: the return and what it sends, a returned socket that dies,
  a returning upgrade refused with 403, and a 426 fallback that stays.
- `-p codex-alab-autopilot --lib -- recovery::tests::`: 38 passed, with the
  token-drop replay and the handshake budget among them.
- `-p codex-core --lib`: 3,267 passed, none failed.
- `-p codex-alab-autopilot`, every target: 1,075 unit tests and 270
  integration tests passed, none failed.
- The other eleven `alab-*` crates, every target, on `8fc6d97b528`: 1,267
  passed and one failed, `codex-alab-cloud-contracts --test reader`
  (`retained_cloud_transport_owns_its_actual_reader_and_destination_policy`:
  "managed credential refresh is unavailable"). That failure was the
  invocation's, not main's. The fixture keeps its credential file in a
  `tempfile` directory, and the native reader refuses a path that is not its
  own canonical form (`auth_native_source.rs`, `read_revision`); the shell's
  `TMPDIR` was macOS's `/var/folders/…`, a symlink into `/private`. With a
  canonical `TMPDIR`, as the gate's containment gives every test
  (`alab_contained` resolves its scratch directory), both of that binary's
  tests pass on `e4b1953ca39`.
- `targo tippy --all-targets` over `codex-alab-core`, `codex-core` and
  `codex-alab-autopilot` (before the merge): no diagnostic in ALab's files or on
  a hook line.

`alab/check.sh` then ran, and does not pass on main for a reason this change
does not own. It needed three things this Mac lacked: `atpkg-publish`, built
from an owned aterm worktree as the README says (aterm `2c577af0c81`); the
`upstream` remote, from `alab/setup.sh`; and a filled private Cargo home, from
the README's preparation for a cold one.

`alab/setup.sh` has a cost on a clone that pushes through `agh`: with the
`remote.upstream.*` keys it writes, `agh repo push` refused ("the clone's
config is outside agh's transport security policy: remote.*"). Removing that
config section (`git config --remove-section remote.upstream`) restored the
push and keeps the fetched `refs/remotes/upstream/main`, which is all the gate
reads. This clone is left without the section.

- On `19ab3f8e527` it stopped at its lint step on ten findings in
  `codex-alab-autopilot`, none in a file this change touches: nine `expect()`
  calls in `tests/autopilot/goal_recovery.rs` and `policy_continuation.rs`, and
  a redundant closure in `supervisor.rs`. `c194029e557` clears them.
- On `c194029e557` its hook, intake and lint steps pass ("tippy finds nothing
  in ALab's code"). Its certified build then stops, exit 2: Trust 15698 refuses
  unit `codex-cli::codex` for in-process proc-macro dependencies, the refusal
  `b38f8251047` records (tracker RL-05). That is fixed in Trust, not here, and
  the gate cannot pass on any commit of main until it is.
- The steps after the build did not run in the gate. To exercise them, the
  gate ran twice more on `e76e0349949` with one uncommitted line changed: the
  certified build replaced by `targo --unverified build` of the same binary.
  That is test evidence under the gate's containment, not a gate pass. The
  edit was not committed; it replaces the `alab/certified-build.sh` call in
  `alab/check.sh` with
  `alab_build_command "${cargo}" --unverified build --locked -p codex-cli --bin codex || passed $? 2`.
  - The binary runs and the app runtime smoke passes.
  - Four `terminal_wall` tests of `codex-alab-autopilot` failed: their
    `sleep 300` dev servers were started through `sandbox-exec`, which macOS
    refuses inside the gate's own sandbox. `b33c9476e79` runs them without
    core's command sandbox. With it every `alab-*` crate's tests pass under
    containment: 2,649 passed, none failed.
  - The unit tests of the 41 hooked upstream crates: 15,715 passed and 18
    failed, in `codex-app-server-daemon` (3), `codex-app-server-transport`
    (5), `codex-core` (2), `codex-exec-server` (3), `codex-otel` (1) and
    `codex-protocol` (4). Each message is a denial by the gate's containment,
    in an upstream test of upstream code: a Unix socket or a `/tmp` path
    refused with "Operation not permitted", an updater that could not listen,
    an exporter with no transport, a nested seatbelt sandbox
    (`guardian_tests::exec_command_allows_sticky_turn_permissions…`) and a
    request to the real models endpoint
    (`multi_agents::tests::spawn_agent_reapplies_runtime_sandbox_after_role_config`).
    None is in code this change touches, and the two in `codex-core` pass
    outside containment. The gate's own judgment of them against pristine
    upstream did not run.
  - That judgment was refused: "unit-test attribution refused: one unit-test
    copy reported more than one outcome without distinct provenance".
    `alab/unit_results.py` requires each test name once per library, and four
    libraries print a passing name twice. Three run a test again as a child
    and pass its output through: `backend::stderr_log::tests::pruning_resets_the_write_position_of_a_legacy_nonappend_writer`,
    `auth::manager::alab_manager_catalog::tests::real_sync_managers_defer_cache_and_restore_only_after_explicit_pre_effect_cancel`
    and `oauth::tests::fallback_file_is_private_at_creation`. In `codex-core`
    six cases of `guardian::input_budget::tests` and
    `guardian::request_budget::tests` are each listed twice. This refusal and
    the 18 failures are the check topic's next work once the certified build
    passes; they are left to it.

No live request has exercised either rule. The next opaque rejection and the
next fallback write the log rows above.
