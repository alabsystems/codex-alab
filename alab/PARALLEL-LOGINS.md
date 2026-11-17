<!-- Copyright 2026 Andrew Yates -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Parallel logins and quota rotation

## Requirement audit, 2026-11-03

The target is an unbounded account roster, simultaneous account logins, and
automatic continuation on another account when a usage allowance runs out,
integrated with aterm's email authentication, shared roster and machine identity.
Passing same-account renewal or quota-wait tests alone does not meet that target.

Current source and installed-host evidence:

| Requirement | Evidence | Status |
| --- | --- | --- |
| Separate credentials with shared conversation storage | `CODEX_AUTH_PATH` selects the auth file; 40 actual exec processes passed a simultaneous-request barrier with distinct account identities and one shared `CODEX_HOME` | Runtime isolation qualified for 40 concurrent logins; credential files unchanged and no shared default login created |
| Refresh serialization per login | `alab-core/src/auth_lock.rs`: a separate `<auth-file>.lock` for every slot | Busy/refused locks now return transient failures instead of sending without a lock; all eight lock tests pass, including retry and 32 simultaneous slots |
| Roster and session capacity | Aterm removes fixed account, preparation, history and session-count truncation; bounded storage writes remain atomic | Count caps removed from roster, sessions, claims and activity; 40 concurrent processes and 256-claim round trips passed; storage byte budgets remain |
| Account selection on launch | Installed `aterm harness accounts --help` describes Codex pool selection with `CODEX_AUTH_PATH` | Host supports selection; not evidence of mid-task rotation |
| Move a running interactive task to a different account after quota exhaustion | Separate typed rotation protocol, two-identity receipts, native quota selection and original-turn continuation; real-process happy path passed | Fixture-qualified on 2026-11-13: `weekly-quota`, `weekly-quota-chain`, `weekly-quota-exhausted` and `weekly-quota-rollback` passed against a `built-from-clean-commit` host from aterm main `40e85159240` with the signed 0.7.0 package ("Qualifying runs of 2026-11-13"). Not live: this Mac has one signed-in Codex account. In the installed aterm by its tag's ancestry, not run there: aterm 0.201.0, installed here by 2026-11-14 (tag `v0.201.0`, `3793545d7da`), contains aterm main `40e85159240`, the commit the qualifying host was built from, and so the in-place claim renewal (`2020f39c770`) that 0.199.0 and 0.200.0 lacked; no installed aterm and package were run together, and codex-alab's seal for older aterms (`1eb3459fbe`) is in no release |
| Move a running interactive task off a five-hour wall that is far from its reset | Since `4b4a67dc1d` (in 0.7.0) a creditless turn refused on its five-hour window ends and asks the host, where the host's policy prints `codex_rotation_five_hour=<seconds>` and the reset is further off than that bound plus a minute; aterm main moves such a session as it moves one at a spent week (`6380d03f686`). A nearer reset is waited out on the session's own account | Fixture-qualified on 2026-11-13: `five-hour-wall-far` passed three times, and again in a six-case `renewal_matrix.py` run that passed, against the `built-from-clean-commit` host from aterm main `753cca585a4` with the signed 0.7.0 package ("The five-hour handoff, 2026-11-13"); `five-hour-wall-soon` passed on the same host. With the walled account's title request refused too, as a provider refuses it, the session was moved about 30 s later than with it answered: 45 s from the refusal to the answer, against 14 s. Not live: this Mac has one signed-in Codex account, and whether the live backend's 429 names the five-hour window's own reset is not determined; since `986bb020b1` (in no release) that no longer decides whether the wall is offered (gap 6 below). Not in an installed aterm, for the reason in the row above |
| Move a running interactive session to another signed-in account on request | `/accounts use <label>` and `/accounts auto` ask aterm `restart account=<label>` or `account-auto`, which restarts the session on its conversation under that login (`alab/design/accounts-assignment.md`); the in-process assignment request path is removed. It is asked and carried only at the session record's rest, so a running background terminal (a dev server, a watch build) must be stopped first; nothing ends it for the person. A session moved while it waits at a recorded usage wall comes back at that wall bound to the new login, and that login's next accepted usage read that permits the wall's bucket resumes it at once, whatever rung the old login's tries had reached (since `c1688363b1` and `1d171802e3`; gap 3 below). codex-alab's client, its schema 2 snapshot reader, the TUI refusals and that resume are tested against alab-aterm's fake host only | Unqualified: aterm's side is on aterm main since `9bd84e2a0c2` (2026-11-13), and its rest check (`account-busy`) since `d20a22e9ed2` (2026-11-13); aterm 0.201.0, installed here by 2026-11-14, carries `9bd84e2a0c2` and the rest check by its tag's ancestry and its `ctl help restart` lists `account=<label>`; the installed codex-alab 0.7.0 lacks the client (`05950d02ef`, in 0.8.0's source). aterm 0.199.0 answered schema 2 `ERR codex-accounts invalid request` and `restart account=` `ERR bad restart argument`, at which codex-alab refuses with "Account changes are unavailable on this host." No live move was run |
| Move a running headless exec task after quota exhaustion | Native contract 3 reserves through aterm before Core suspension; four/eight shared-pool sessions, repeated moves and lost acknowledgements pass. Codex `7a6cae40a6` also retains the original managed checkout, owner and local edits across repeated handoffs | Ordinary and managed-worktree exec continuation qualified locally; ephemeral/review modes and failures after suspension remain incomplete. Only a run that leads its tab's foreground process group moves; every other shape waits out its reset (gap #2 in "Known gaps", the planned admission in `CONTRACT.md` §1.1) |
| All eligible accounts exhausted | Real-process `weekly-quota-exhausted` checks both accounts at 100%, two native prepare polls, retained original task and no native actions | Passed without a rotation charge or replayed model request |
| Consecutive quota handoffs | Real-process `weekly-quota-chain` exhausts the source and first replacement, then continues on a third account | Passed with the same original turn, one completed plan, two native handoffs and two charges |
| Email authentication | Codex and Claude use aterm's common mailbox reader; 75 mailbox tests and the cross-CLI sender-scope test passed; the updated native CLI reached OpenAI | Live email-code login remains unverified because Cloudflare stopped the authorization flow before OTP retrieval |
| Shared roster and machine identity | Aterm discovers registered roster/mailbox pointers; Codex uses shared claim publication, home-account discovery and upkeep; two-machine claim and native-commit tests passed | Local replicated-table tests passed; live fleet control excludes this installation, so live cross-machine placement remains unqualified |

The native account report selected `main` and excluded `codex-m22` as not enrolled.
Its signed Commons state excludes this installation from control. The registered
secrets inventory and native enrollment flow were consulted without displaying
secret values. The unchanged provider challenge has not been bypassed, and no
live login was changed by the stopped enrollment attempt.

## Current integration and qualification

Aterm source `96d05c4c3b` claims a native quota destination through the shared
machine-claim service before committing the pool handoff. It validates the move
before the claim and again afterward. A peer that acquires the destination after
plan preparation prevents the handoff and its rotation charge. All 104
account-pool tests passed, including that case and native receipt retry accounting.

The shared claim path now includes Codex in publication, home-account discovery,
periodic upkeep and shutdown handling. The public-entry-point test publishes a
Codex claim and checks that another simulated machine refuses that account.
All 61 shared-claim tests and 38 signed-table tests passed. Account count no
longer truncates a claim set at 16 or local activity at 4,096. Canonical signed
rows fit the existing encrypted transport's 60 KiB payload budget; local activity
has a 2 MiB budget. Oversized updates refuse rather than discard live entries.

The native recovery qualification now also covers lost consume acknowledgement,
post-signal expiry and resumption, a successor crashing before acceptance, and
Stop after Signal. The latter passed with the cleanup assertion in the controller:
no replacement launched, one charge remained, the pool binding was removed, and
`launch_attempted` stayed true. Results are retained under:

- `$HOME/.aterm/.ubd/codex-quota-lost-consume-ack/result.json`
- `$HOME/.aterm/.ubd/codex-quota-post-signal-resume/result.json`
- `$HOME/.aterm/.ubd/codex-quota-successor-prefix-crash/result.json`
- `$HOME/.aterm/.ubd/codex-quota-owner-stop-inline/result.json`

The full Codex gate passed before merging main `86459bf183`. The merge completed
without conflicts, and the combined ALab library run passed 1,251 tests. A later
full gate was stopped while adding the exec suspension interface and must run
again on the finished source.

Aterm at `e75b1aeb3f794b0714b2890aa0b89c851d8162cf` passed formatting, native
lint and Linux ARM, Windows ARM, WebAssembly CPU and WebAssembly GPU cells for
the selected agent, GUI, database, types and PTY crates. Its ordinary dev app
built without installation. A private instance reported `codex_rotation=1` and
no generic native constructors. That is a production policy check, not a quota
handoff result; the report is in
`$HOME/.aterm/.ubd/aterm-production-quota-instance/production-policy-result.json`.
Newer aterm main has since been fetched and still needs composition.

These are unverified Trust checks and isolated runtime fixtures, not live
enrollment or installed fleet activation. Deployment and the remaining
interactive/exec integration are still required.

## Known gaps, audit of 2026-11-14

A read-only audit of codex-alab main `c8d9cbc17c` and aterm main `188272b333a`
asked where a session or exec run, on a Mac with two or more accounts already
signed in, still does not move where the goal says it should. It left out what
only a person can give (a second sign-in, a real wall, a certified package),
work then on branches, and recorded decisions. Each of the six gaps below
survived a skeptic who read the code to refute it. They were found by reading
only; gap 3 has since been reproduced by a failing test and fixed. The full findings are kept locally at
`~/Developer/.ubd/goal-gap-audit-2026-11-14.json`. Order of work: 3, 1 and 6
in codex-alab once the second-thread and one-ask branches land (they share
`alab-autopilot`), then 5, then 4, then 2, which needs a design first. Gap 6
is fixed (below, with what it leaves).

1. **A background terminal holds off the move until the reset.** A unified-exec
   terminal (a dev server, a watch) keeps the frame Working and the record
   `busy`. `quota_handoff_ready` (`alab-autopilot/src/renewal_follow.rs`) does
   not look at terminals, so the walled turn ends and gives up its earned
   reset; aterm's `Intent::matches_record` admits only `idle` or `waiting`, so
   every ask is answered `wall changed`, which `follow_renewal_point` drops
   without a word. aterm's rest fence stays: a move never silently ends a
   running server.
   - **Fixed in `306e87479f` (codex-alab only).** While the count the TUI's
     footer hands over (`Shared::background_terminals`, the count the record's
     `backgroundTerminals` and its `busy` come from) is above zero,
     `quota_handoff_ready` answers no, so the walled turn keeps its in-turn
     poll for an earned reset and asks again every 30 s. Its first look after
     the count falls to zero ends it on its usage error, and the move is asked
     for under a record at rest. Where the move would otherwise follow (the
     policy, the wall and the host's pool all say so), the person is told once
     per walled turn, in a warning in the thread's history sent through
     upstream's `ExtensionEventSink::emit_warning` (the app server sends it to
     the TUI as `warning`): "The usage limit is reached, and another
     signed-in account has room. Background terminals run in this session,
     and the restart would end them, so the move waits until they finish or
     you stop them." After that, while the terminals still run, the question
     sends the host nothing. `follow_renewal_point` now journals each closed
     refusal word (`wall changed` among them) once per ask series and keeps
     the point, as before. No upstream file changed. Tests:
     `renewal_follow_handoff_tests.rs`
     `background_terminals_hold_the_handoff_and_the_person_is_told_once` and
     `tests/autopilot/terminal_wall.rs`. The second runs a `Cli`-sourced
     primary thread against the fake aterm and a loopback model and usage
     backend: the walled turn waits under a busy record with no ask sent, the
     warning is said once, the turn ends at the first poll after the terminal
     ends, two `wall changed` refusals give one journal line, and the next ask
     and its `commit` meet a record at rest. Both fail with the fix taken out.
   - **Its review (verdict: land after fixes) found the hold read the wrong
     count, fixed in `af5f4fcc54`.** It read the TUI's footer count, which is the
     thread on screen's alone (none with no frame count), and only the primary
     thread's question read it. A side chat that started a dev server after the
     primary asked had its walled turn end for the live move, giving up its
     poll for an earned reset, while every ask was refused; a terminal in a
     thread off screen was not counted at all, so the record could rest and the
     move end it; and the warning went to the primary thread only, which the
     TUI buffers unseen while the person views another. Now "the session's
     background terminals" is one reading, `session_background_terminals`
     (`renewal_follow.rs`): core's own count (`list_background_terminals`)
     over every thread the thread manager lists (`list_thread_ids`): the
     primary's, every other root thread's and a sub-agent's, but no internal
     thread's (Guardian's, memory consolidation's, an exec review's), which
     that list leaves out. Every path of the usage wall's move waits on it: the
     primary's `quota_handoff_ready`, another root thread's
     `session_leaves_account` (no while one runs, even with the move live;
     yes at the first look after they end with the move live again), and
     `follow_renewal_point`'s ask (an unbound `prepare` is not sent, the
     point is kept with no word to the host, so the move is not live and no
     side turn is woken for it). The person is told once per wall (the
     primary's walled turn, whichever path was held) in every root thread the
     Autopilot keeps (the TUI's own), rather than only in held ones: the wall
     and the move are the session's, and the host routes a warning to the one
     thread it names, so whichever root thread is on screen shows it, in any
     host and with no frame fact read. A sub-agent's thread is never a root
     thread, and is not told. The TUI's footer count stays what the record
     writes (`backgroundTerminals`, the frame's `working`): the record is the
     TUI's frame fact, written by its own writer, while core's count is an
     asynchronous per-thread read that no event announces, and a TUI on a
     remote app server has no core to read. No upstream file changed. Tests:
     `renewal_follow_handoff_tests.rs`
     `background_terminals_hold_the_handoff_and_the_person_is_told_once`
     (amended: the footer counts none) and
     `another_root_threads_walled_turn_waits_while_the_sessions_terminals_run`
     (new), against a test-set count; and `tests/autopilot/terminal_wall.rs`,
     whose terminals are now real (the model starts `sleep` through
     `exec_command`; the test stops it through core): the primary's terminal
     in a session whose frames hand no footer count, the primary's terminal
     while the person views the side chat (its footer counts none, and the
     side chat is told too), and the review's scenario (the side chat starts
     its server after the primary asked: its turn waits, it is told once, no
     ask goes out while the server runs, and the first ask after it stops
     ends the side turn and moves). Each fails with its part of the fix taken
     out.
   - **Its re-review (verdict: land after fixes, nothing major) found the
     warning told with no pool read, fixed in `36a312c4a6`.** Only the
     primary's question read the pool (`rotation_target_in_sight`) before it
     told; the follow pass and another root thread's question told the same
     line ("another signed-in account has room") with no read, and the follow
     pass makes its intent from the usage wall alone, for a turn no yes ended
     (a 429 with no reset ends its request on its own). With no account able to
     run, the person stopped their server for a move the host then answered
     `not-needed`. Now `tell_terminals_hold` reads the pool on every path and
     tells only where it shows somewhere to go; the hold stands either way, as
     an ask with nowhere to go costs nothing. The re-review also found this
     entry and CONTRACT overclaiming what the count covers and who is told: the
     count covers the threads `list_thread_ids` names, and the warning reaches
     root threads only (both reworded above; the gaps are listed below). Tests:
     the side-thread case now first holds with no account able to run, with
     nothing journaled or warned, then tells once when one has room;
     `terminal_wall.rs`'s
     `the_follow_pass_tells_of_the_hold_only_where_another_account_has_room`
     holds the primary's pass on a side chat's dev server with the room gone
     (no ask, nothing told), tells once in each root thread when room returns,
     and asks once the server ends. Both fail with the check taken out.
   - What remains:
     - `/accounts use` and `/accounts auto` still read the footer count, and
       aterm's fence the record's, so a person's own move under a record at
       rest ends a terminal in a thread the TUI does not show. Its check runs
       in the TUI's glue, compiled inside `codex-tui`, which can reach core's
       count only through the app server (`thread/backgroundTerminals/list`
       per thread) or a reader the Autopilot would install in `Shared`; that
       is a change to the person's command with its tests in `codex-tui`'s
       binary, left for its own change.
     - A bound receipt's `status` is read while terminals run, and a terminal
       that starts between a pass's check and the `commit` that follows the
       host's `ready` in the same pass is not held; aterm's fence then sees
       only the shown thread's.
     - A lost login's move (`login-rotate@v1`) and a login's own renewal are
       not held by terminals.
     - A person viewing a sub-agent's thread sees no warning: the warning is
       said only in root threads, and a sub-agent's thread never becomes one
       (`on_thread_start` adopts it before `note_root_thread`). They see it
       on switching to any root thread.
     - Internal threads (Guardian's, memory consolidation's, an exec
       review's) are not counted: `list_thread_ids` leaves them out, so a
       terminal one of them ran would not hold the move.
     - A session with no frame at all has no primary thread and moves
       nothing, so nothing there is held or needs to be.
     - The move comes up to one 30 s poll after the terminals end for a
       walled primary turn (a side turn is woken by the next pass that asks):
       nothing announces a terminal's end.
     - How the TUI draws the warning is upstream's (`on_warning`), and is not
       tested here; the app server keeps 256 bytes of a warning, and the line
       is under that.
     - Not run against aterm or a real wall.
2. **A `codex exec` run moves only as the proven foreground leader of an aterm
   tab with Autopilot on.** A run started by a script, inside a pipeline, under
   `timeout`, in the background, from an agent's tool shell, or outside aterm
   never moves and waits out its reset with no visible line. aterm's exec verbs
   require the kernel peer to lead the tab's foreground process group and hold
   its one supervisor claim; a tab has one prepare slot. A fix needs an
   exec-specific admission and a per-process slot, and must be weighed against
   `alab/CONTRACT.md`'s laws on what a descendant proves.

   The fix was designed and critiqued on 2026-11-14 and is recorded in
   `alab/CONTRACT.md` §1.1, "Exec process admission". The claim's rule, that a
   descendant proves nothing, stays as it is. An exec move gets an admission of
   its own, which claims nothing. aterm proves four facts for it: the kernel
   peer pid, an ALab image in `exec` mode, the process's kernel identity, and a
   kernel binding to the tab, which is checked only up to `reserve`. aterm
   judges the Codex-scoped policy and the hold itself, and uses one mover key
   for each process. Its settlement never reads the tab's human input or the
   claim revision.

   The planned steps, in order:

   - C1, codex-alab: a run that will not move prints one stderr line per wall,
     and a run that does not lead its tab sends no claim.
   - A1 to A3, aterm: per-process tables, the admission, and fixture launch
     shapes.
   - C2, codex-alab: the client that uses the admission.
   - C3: a smoke for each run shape.
   - A4 and C4: retire the claim path.

   C1 is built (`alab-aterm/src/exec_route.rs`). The other steps are not,
   so the run shapes today are:

   - **Moves:** a run that leads its tab's foreground process group, in a tab
     whose policy is on for it (Autopilot on, `codex_rotation=1`, `accounts`
     `early` or `wall`, `relaunch`, `resume_limits`), signed in with ChatGPT.
     That is a `codex exec` typed at the tab's prompt, or one that a shell or a
     wrapper exec's in place (`exec`, `nice`, a `sh -c` whose last command it
     is).
   - **Waits, with one line on stderr per wall (C1):** every other shape
     but one, below. That includes a child of a script that forks it, a
     pipeline member that is not its group's leader, a run under a wrapper
     that forks it (`/usr/bin/time`, `timeout`), a run started with `&`, a
     run from an agent's tool shell, and a run outside aterm that names no
     session. Such a run sends no claim.
     A run in the tab that does not lead is checked again locally every 5 s,
     and its line says the move waits for it to lead: brought to the front
     with `fg`, it claims and moves. A leader of another terminal (tmux,
     codex's unified-exec PTY) claims once per wall: aterm does not prove it,
     or the program that leads the tab (a codex-alab TUI) holds the tab's
     proven claim. A refusal that does not say what aterm serves (a dialog
     aterm is showing, a main-thread stall) and a socket that refuses for a
     moment (an aterm seamless update) print nothing and are asked again in
     5 s, so a leader still moves after them.
   - **Waits with no line:** a run that carries the session of a tab that
     has since closed, as a detached tmux or screen server, or an orphaned
     run, that outlived its tab does. aterm removes a tab's rendezvous entry
     when the tab closes, and a missing entry is no answer, because an aterm
     restart or seamless update shows one too. So the run reads its entry
     again every 5 s for the wall, silently, as a run whose aterm is gone
     does.

   After C2 with A2, a run in the tab moves in all the shapes that wait with
   a line but a run outside aterm: a script's child, a pipeline member, a
   wrapped or background run, and a tool shell's `setsid` run whose lineage
   reaches the tab. These still wait, each with the line:

   - a hold, Autopilot Off, or another Codex-scoped off;
   - a run outside any aterm tab;
   - a detached or orphaned run whose tab is open;
   - a tool shell whose sandbox denies the socket;
   - a run that does not lead its tab, under an aterm without the admission.

   A run whose tab has closed still waits with no line, as above.
3. **Fixed by `c1688363b1` and `1d171802e3`: `/accounts use` or `/accounts auto` at a
   recorded usage wall waited out the old account's reset.** The successor restores the
   record's `Wall::Usage{reset_at}` of the account it left (`recovery::restore`
   checks no identity, rightly: the wall is the failed turn's);
   `restore_quota_scope` (`alab-autopilot/src/extension.rs`) dropped the
   binding for the other identity, so no usage read of the new account woke it,
   and nothing was typed until the old reset. Now it binds the restored wall's
   bucket, with any ordinary-usage denial the old login witnessed, to the login
   the process runs under, as a wall seen at the restart. The new login's next
   accepted usage read that permits the bucket wakes the wait and types
   `usage-resume@v1` at once; a read that denies the bucket keeps it; a read of
   the old login answers nothing; the deadline on record stays the latest. At
   once also where the old login's own tries had climbed its ladder: the rung
   the record's pace carried is dropped at the move, and the restored wall starts
   the new login's ladder at no try, which that login's own try then climbs and
   the record then carries; no rung is kept per login, so a move back to the old
   login starts its ladder over too (`1d171802e3`, after its review: a session
   moved a minute after its resume met the old login's weekly wall waited 538 s
   "for retry pacing"). Another login is another account or another native user:
   one person in two workspaces keeps one ChatGPT user id under two account ids.
   Without a fresh permitting read the deadline on record still floors the try.
   The same-login rules (a read must start after the restart; the carried rung
   paces it) are unchanged. Reproduced, then tested against alab-aterm's fake
   host through the real resume scan, in `alab-autopilot/src/quota_tests.rs`:
   `a_move_at_a_recorded_usage_wall_resumes_on_the_new_logins_read`, which
   fails without `c1688363b1` (the new login's read never wakes the wait), and its
   control `a_relaunch_on_the_walled_login_with_no_new_read_waits_out_its_reset`;
   `a_move_at_a_retried_usage_wall_resumes_at_once_on_the_new_logins_read`, which
   fails without `1d171802e3` (the 538 s wait), and its control
   `a_retried_usage_wall_on_its_own_login_waits_its_rung`;
   `a_move_to_another_account_of_the_same_user_starts_the_ladder_over` and
   `a_move_to_another_user_of_the_same_account_starts_the_ladder_over`, which
   fail where a move must change account and user both (the 538 s wait again);
   `a_move_at_a_witnessed_entitlement_wall_resumes_on_the_new_logins_ordinary_usage`
   (`ab1f960a45`), which fails where the old login's witnessed denial is carried
   only to a login of the same account.
   What remains: no move has run against a real aterm with two signed-in
   accounts (the requirement row above), and none at a real wall. The resume
   comes at the new login's next usage read, not at the restart: the TUI reads
   at most 15 s apart while a usage recovery is on record, by reading
   `alab-tui/src/motion.rs`, not measured. Nothing is bound, and the old reset
   still stands, where the record predates quota scopes (images before
   2026-10-30) or codex has no account for the login when the thread resumes.
   An entitlement wall restored on the new login wakes on that login's permitted
   ordinary usage where the old login's denial was witnessed (a
   `UsageNotIncluded` wall's always is), or after a denial the new login's own
   reads show after the restart; otherwise it waits its blind try, no earlier
   than the deadline on record, on the new login's own ladder, as the same login
   waits on its carried one.
4. **Walls outside the ordinary `codex` bucket never move.** A model's own
   limit bucket, a workspace's reached type, spend control and a credit-only
   plan end the turn, but aterm's `walled_window` (`codex_rotation.rs`) leaves
   only a window at 100%, so it answers `not-walled` and codex-alab declines
   and waits out that wall's reset. In exec, before S3, a workspace reached
   type ended the run and a model-bucket wall published no observation. The
   fix is designed and critiqued in six steps, each landing alone (kept locally at
   `~/Developer/.ubd/gaps-245-design.json`, key `otherWalls`; where the
   critique changes a step, the critique holds):
   - **S1, done in `abb82cfc67` (codex-alab, test only).** Golden provider
     replies for each wall kind, under `alab/smoke/fixtures/provider`, with
     field names taken from upstream's own models and decoders: usage reads
     at a member's spend cap (`spend_control`), a workspace out of credits
     (its reached type), a credit-only plan out of credits (no window,
     `credits`), a model's own limit (`additional_rate_limits`,
     `codex_other`) and an ordinary five-hour wall that also names
     `rate_limit_reached`; 429s for the workspace credits, the spend cap, the
     credit-only plan, the model's limit (its own header family), Reserve
     (`base_model_inference`, named `gpt-reserve`) and `usage_not_included`.
     `alab-quota-recovery`'s `fixture_tests` decode each through codex-api
     and backend-client and assert what that crate makes of it today.
     `ordinary_weekly_wall` stays false for the workspace and model-limit
     429s through S3, which is right (no earned reset answers them), so
     those tests pin a reading S3 keeps. Exec's gap is in
     `alab-core/glue/responses_quota_retry.rs`: `reset_delay` ends the run at
     a workspace reached type (pinned by that file's own test), and `wait`'s
     publish gate, which reads `ordinary_weekly_wall`, withholds the
     observation for a model-limit or Reserve wall (no unit test reaches it;
     it needs a live session). Only the Reserve test's reading turns at S3.
   - S2 (aterm): leave a wall when the fresh read refuses ordinary use, with
     `rate_limit_reached` taken as ordinary and "spent" judged as
     `reached_reset` judges it; advertise `codex_rotation_refusal=1`. No wire
     change. It alone moves interactive spend-control, workspace and
     credit-only walls.
   - **S3, done in `e19efdede4`, repaired after review in `01e52c10b4`,
     `c76bd01639`, `1955a20cc4` and `18fddfb2ab` (codex-alab).** One
     wall-kind classifier, `alab-core/src/wall_kind.rs`, which
     `alab-quota-recovery`'s `wall_kind` feeds from upstream's error. A
     workspace's reached type, on the refusal or on its bucket, comes
     first; then the bucket's spend control; then a limit id that is
     neither `codex` nor named `gpt-reserve`, a model's own limit, by its
     id. Everything else, Reserve and a credit-only plan included, is the
     ordinary bucket. `ordinary_weekly_wall` is that kind read as
     ordinary, so it keeps its meaning but for Reserve, which now takes an
     ordinary earned reset. Exec publishes its observation, with
     the wall's kind, for every usage limit it waits on. A workspace or
     spend-control wall (`alab-core/src/exec_wall.rs`, CONTRACT §1.1 "Exec
     walls of each kind") keeps the request only while an exec runtime reads
     the run's observations and its host may move it. The verdict and the
     first wait are kept per root turn and wall kind, in state every turn of
     the exec tree carries. The first wait is 30 s, whatever reset the
     refusal names. Each later wait needs the host's `waiting` or `selected`
     to a `prepare` that is sent only where the policy carries
     `codex_rotation_refusal=1`. The whole keep ends 10 min after the first
     wait unless a grant for the root turn is armed or reserving, and while
     one is, a kept wait does not end on its timer. No host reached, a
     policy without the word, `not-needed` and a definite refusal end the
     run at its next refusal, and an unanswered wall ends at its second.
     Such a run ends with the provider's message and no `account move
     unavailable` line, as it did before S3. A model's own limit is
     published but asked of no host (no `prepare`, never `limit=`): the run
     waits out its reset, as before. The ordinary bucket's exec move is
     unchanged: the tree's watch carries an ordinary wall before a
     workspace's and a workspace's before a model's own limit, so a member's
     wait at another kind of wall never hides the root's ordinary wall from
     the runtime; while one stands, a workspace wall in the same tree gets
     no verdict of its own and ends at its second refusal unless the tree's
     grant is held. A host whose policy lacks the word is sent no claim at a
     workspace wall. An exec review's own turn, which only its delegate
     samples for, is the tree's root turn the keep and the grant name.

     What this moves, by aterm. Against an aterm that does not print
     `codex_rotation_refusal=1` (every aterm today): nothing new moves; an
     exec run at a workspace or spend-control wall ends after one 30 s wait,
     not at once, and an interactive turn there ends as before. A Reserve
     wall is ordinary: exec asks its host to move it, as it asks for any
     ordinary wall. Once aterm's S2 lands: interactive workspace,
     spend-control and credit-only walls move (S2 alone); in exec, a
     workspace or spend-control wall moves within the 10 min bound, and a
     credit-only wall, which is ordinary, takes the ordinary exec move where
     S2's reading also answers exec's `prepare`. Still not moving after S2:
     a model's own limit, interactive or exec (exec waits out its reset),
     until S4 and S5; a run at a workspace wall that the bound or a refusal
     ended; and the Entitlement class, below. S2, S4, S5 and S6 remain.
   - S4 (aterm): per-limit reads, kept apart from the ordinary walls, an
     optional `limit=` on prepare, and `codex_rotation_limits=1`.
   - S5 (codex-alab): the wall's limit id on the wire, only for a model-limit
     wall bound to the current identity and only to a host that advertises
     S4; after gap 3, which rebinds that bucket.
   - S6 (aterm, optional): aterm-lab stub modes that drive each kind to a
     committed move.

   Known unknowns. No real payload at these walls has been captured; the
   goldens follow upstream's models, and each states its wall by one signal
   (whether a real read at a workspace wall carries both spend control and
   a reached type is not known). The plans, the member's (not the owner's)
   reached types, `codex` as a workspace 429's active limit and the spend
   cap's 20-day reset named by its 429 are the fixture's choices.
   `codex_rotation_refusal=1` is the design's word for S2, which aterm has
   not built; S3 reads exactly that key, so aterm's S2 must print it so. No
   exec run has kept a workspace wall against a real host. Out of
   scope for this gap: Entitlement-class 429s. `usage_not_included`,
   `insufficient_quota` and the spend and credit codes codex-api folds into
   `QuotaExceeded` are `Wall::Entitlement` to the Autopilot, which never asks
   a host to move one; if a provider states credit or workspace depletion
   that way, nothing moves. `refusal-usage-not-included.json` pins today's
   path.
5. **Several sessions walled on one account move one every 10 minutes.** The
   first move makes the destination the pool's active account, unproven, with
   a 10 minute canary hold; nothing in the interactive protocol ever proves it
   (exec has a `complete` verb, `codex-rotation` has none), so each follower is
   answered `waiting` until the hold lapses and the next takes the canary
   again. The fix is a `complete` verb on `codex-rotation`, sent once by
   codex-alab after the resumed turn is served, which proves the destination
   with aterm's existing `record_completed_rotation`. It is designed and
   critiqued in five steps (kept locally at
   `~/Developer/.ubd/gaps-245-design.json`, key `canaryProof`; where the
   critique changes a step, the critique holds). The 10 minute hold stays, as
   the ceiling for a canary that never reports. The five-hour move shares the
   gap and the fix; the lost-login move has no canary and refuses the verb.
   - A1 (aterm, built in `9fb6fb041d7`, as its commit and source say; not
     reviewed here): serve `codex-rotation complete` only to the bound
     successor of a Consumed usage-wall receipt (its session, process,
     credentials and thread), never for an exec operation's receipt. A busy
     receipt writer, sign-in or auth lock is typed Busy in `aterm-agent` and
     answered `waiting`; a source login re-signed or held since the wall
     answers `selection changed`.
   - A2 (aterm, built with A1 in `9fb6fb041d7`): a completion proves a
     destination that still runs, Low included, not only a fresh one.
   - **C1, done in `0b4ab96070` (codex-alab, `alab-aterm`).**
     `Client::rotation_complete(id, turn)` returns `rotation::Completion`
     (`Recorded`, `Waiting`, `Unsupported`); `account_recovery::complete`
     sends it only for a usage wall's move (a renewal and a lost login's move
     answer `Unsupported` and send nothing). The receipt id, the turn and the
     native self-claim are checked before anything is sent, and the reply is
     read by the same receipt reader as every other `codex-rotation` reply.
     The wire A1 must serve, one status line per request:
     - request: `@<sid> codex-rotation complete id=<receipt id> turn=<turn>`,
       those two fields only: the id is 32 lowercase hex digits and the turn
       the walled turn's own lowercase uuid (the receipt's `turn`, which the
       resumed turn reuses);
     - `OK codex-rotation state=completed id=… thread=… turn=…
       source_account=… source_user=… target_account=… target_user=…`: the
       receipt's eight fields in any order, each once and nothing else, `id`
       and `turn` equal to the request's, identities as 64 hex digits, the
       target account not the source's. Read as recorded; a repeat for a
       destination already proven answers the same;
     - `OK codex-rotation state=waiting`, no other field: not yet, asked
       again. A1 answers it for a busy receipt writer, sign-in or auth lock,
       and aterm's shared lane already answers it before any dispatch while
       it cannot read the claimed process (its environment, or the claim
       copy's record or resume);
     - `ERR codex-rotation <words>`: a refusal, read as its closed word in
       `rotation::REFUSALS`, or `other`. A well-formed report can be refused
       with `denied` (the shared lane, before it reads a claim; A1, with
       quota rotation off), `wrong recovery purpose` (the shared lane's guard
       in `serve_claimed`, which A1 extends to `complete`, for a receipt of
       another purpose), and A1's own `unknown receipt`, `wrong session`,
       `receipt changed`, `wrong process` and `selection changed`. A new
       word A1 adds goes into `REFUSALS` with it, or it reads as `other`;
     - `ERR codex-rotation invalid request` and `ERR unknown verb …`: a host
       without the report, read as unsupported. A1 must keep `invalid
       request` for a request it cannot parse, never use it for a receipt it
       refuses;
     - `ERR busy …`: busy. Any other line, a `completed` naming another
       receipt or turn among them, is unreadable and never recorded.
   - **C2, done in `23e92507d3` (codex-alab, `alab-autopilot`,
     `renewal_completion.rs`).** The process on the destination sends the
     report once per receipt, after any of the three consume confirmations:
     the resumed turn's stop, `follow_renewal_point`'s terminal branch (a
     restored process's included) and the lost acknowledgment's retry. It
     goes only for a usage wall's move, only when the resumed turn ended with
     no error (at the stop, no turn error at all, walled or not; from history,
     `resume_history::served`, and in the process that ran the turn's stop
     that stop's verdict as well, since core persists no error a turn met
     outside its terminal), and only while that end is no older than the
     hold (`decide::CANARY_HOLD`, aged on the wall clock as aterm's hold is,
     from the stop's moment or the terminal's `completed_at`, when the report
     is decided and again before every ask). It is spawned off every
     caller's path with no strong reference held over a wait. It has 5 s in
     all on the claim's connection, and `waiting` is asked again after
     250 ms. It writes one journal line with the answer and the resumed
     turn's length. The owed act, the seal and the schedule never depend on
     the answer. The thread keeps whether the host recorded the report, so C3
     can make this one its fallback. Tests: `rotation_complete.rs`.
     Repaired after review in `3205262cdb`: every later path judges the
     turn by this process's stop of it as well as by history, the age is
     the wall clock's and is read again before every ask, and the error
     mark, the re-check and the once-per-receipt guard each have a test
     that fails without them.
     Waits on A1 and A2: every aterm today answers the report unsupported
     (`invalid request` or `unknown verb`), so followers still wait out the
     10 minute hold until A1 and A2 land, and then S1 shows the change.
     aterm built both in `9fb6fb041d7` (on aterm main by `7540b6d94f2`). In
     the trial runs of 2026-11-15 a host built from `7540b6d94f2` answered
     the report `state=completed` in each usage-wall move whose resumed turn
     was served, and its pool then held the destination proven with no canary
     ("Trial runs against aterm `7540b6d94f2` and codex-alab `cdfe65abe3`,
     2026-11-15"). S1 has since shown the change, in trial runs only.
   - **S1, written in `314f0ccd89` (codex-alab, test only):
     `alab/smoke/tui_shared_pool.py`.** Three real codex-alab TUIs run, each
     under its own native host fixture, over one `shared_pool_root`. A
     loopback barrier refuses every session's second request on account A
     within a second, and B is never spent. The `follow` verdict requires:
     - one charged rotation;
     - the canary's `complete` answered `completed`, after its last consume;
     - every follower turned away after the canary's `ready` and before its
       report: it asked then and was not moved;
     - each follower's first commit taken only after the report, within
       60 s of it, with its own report answered `completed`;
     - no `canary:` row;
     - the pool proven once, with no canary.

     In this lane a follower that is not moved asks again every 5 s and is
     turned away in one of two ways. Either its `prepare` is answered
     `waiting`, or it is answered `ready` and its `commit` is answered
     `retired`. While the destination is unproven, aterm's `commit_pool`
     refuses the selection and the receipt is retired
     (`codex_renewal_control.rs`, `commit`). Every run below shows this; it
     is not a reading of the advice code. Neither answer names its cause.
     The host answers `waiting` too while a follower's own read of the
     accounts is pending, after the pool was proven as well (`s/p2`), and
     `commit_pool` refuses a selection that lapsed or changed. So being
     turned away shows a follower was not moved, not that the canary held
     it. What ties a follower's move to the report is its order: taken only
     after the report, and within 60 s of it. A `commit` answered a bare
     `waiting`, naming no receipt, takes nothing either: aterm answers it
     when `commit_pool` refuses and the receipt cannot be retired.

     `--control` withholds the report and nothing else. Every host's first
     consume ack is dropped and paused (the fixture's own switches). The
     canary's resumed turn is served before that consume, with nothing
     held. Once the canary's ack is traced, the controller holds the
     destination's `auth.json.lock` (aterm's AuthLock flock) across
     codex-alab's consume retry and its 5 s report budget, so the host
     answers each report `waiting`. It then lets the lock go and watches for
     120 s with nothing held. From the canary's `ready` to the watch's end,
     every `prepare`, `commit` and `status` of a follower must be answered
     `waiting`, `ready` or `retired`. A follower refused for any cause, an
     after-effect of the lock included, fails the control by name. Before
     `6e0631276d` such a refusal was recorded (`otherAnswers`) and passed.
     `--lost-consume-ack` is the same drop and pause with no lock, so a
     passing run differs from the control by the lock alone.

     Trial runs of 2026-11-15 ("The shared-pool follow, trial runs of
     2026-11-15", below), two of each mode:
     - `s/p2` and `s2/p` passed. The followers' commits were taken 1.18 s
       and 6.34 s, and 0.64 s and 0.83 s, after the canary's report, which
       came 9.84 s and 10.39 s after the walls.
     - `s/l1` and `s2/l` passed with the canary's consume ack lost. The
       followers were taken 4.52 s and 4.79 s, and 4.85 s and 10.00 s,
       after the report.
     - `s/c1` and `s2/c`, the control, passed. Twenty and 19 reports were
       answered `waiting` in the 7.75 s and 7.64 s the lock was held. Over
       the next 120 s the followers were turned away 39 to 41 times each,
       were answered nothing but `waiting`, `ready` and `retired`, and no
       follower's commit was taken. The pool stayed unproven with 461.7 s
       and 461.6 s of the hold left, although the canary's resumed turn had
       been served on B.

     So the followers' move in the follow runs is the report's doing:
     without the report, no follower was moved or refused in 120 s, with
     the canary hold standing. Not shown: that they would wait until the
     hold's lapse at 600 s, or the lapse itself (a follower taking the
     canary), since the control is capped well short of it; more than two
     runs of a mode; and a live run.
   - A3 and C3 (the critique's addition, after A1, C1 and C2): the same
     report at the resumed turn's first served response, on a still
     Committed receipt, so a long resumed turn no longer holds followers for
     the whole hold. The wire above is unchanged by it.
6. **Fixed in `986bb020b1` (in no release): a five-hour wall whose 429 named no
   reset, or a reset more than 60 s from the usage read's, was never
   offered.** `offerable` (`alab-quota-recovery/src/unix.rs`) offered a
   five-hour wall only when the 429's `resets_at` agreed with the read's,
   because the Autopilot took its wall's reset from the 429; otherwise the turn
   polled in place until the window reset, up to five hours. `offerable` is
   gone: quota recovery offers the read's wall whatever the 429 named. The
   turn's handoff keeps the wall it answered yes for, and the ended turn's
   usage wall takes that read's reset, for a spent week too. Its point, the
   bound on asking the host, the wall carried across a restart, and the resume
   after a `not-needed` or a lapse all run on the moment the host judged
   (`alab/continuous-earned-resets.md`, "Offer a five-hour wall on its usage
   read alone"; `alab/CONTRACT.md`, the usage window row). Tested against the
   repo's fakes only: quota recovery's `handoff_tests.rs` and the Autopilot's
   `renewal_follow_handoff_tests.rs`. The smoke case
   `five-hour-wall-far-unnamed` (`07b896c40b`) is written and its fixture tests
   pass. Its one run, a trial on 2026-11-15, passed ("Trial runs against aterm
   `7540b6d94f2` and codex-alab `cdfe65abe3`, 2026-11-15"); it is not in
   `renewal_matrix.py`.
   What remains:
   - a usage wall no yes ended still carries the 429's reset, or none. That
     covers a 429 whose reset had already passed when it arrived, exec (which
     never consults the handoff), a provider quota recovery does not serve, and
     a point a restart rebuilt from history alone. `follow_renewal`'s ask at
     such a wall with no reset is still unbounded;
   - a five-hour wall the host declines keeps its request polling until a yes,
     a restored allowance or the 429's own reset (with no reset named, one of
     the first two). That is the turn's poll for a late credit, as before;
   - which reset a live 429 names at a five-hour wall is still not determined.
     The journal now records it beside the read's at every turn a yes ended
     (`the turn ended on the host's yes; its usage wall resets at the usage
     read's …`);
   - no live run, and in no release or installed package. The one host run
     is the trial of `five-hour-wall-far-unnamed` on 2026-11-15.

## Implementation history

The following sections record earlier intermediate states. The current audit
above takes precedence over their then-pending work.

## Next implementation boundary

Cross-account rotation needs its own host-authorized transition. Do not relax
same-account renewal's identity checks: those protect a different operation.
The host must select an eligible roster row under current policy and machine
placement, bind both old and new identity to the failed native turn, and derive
the new auth path itself. The successor must consume a durable receipt exactly
once before continuing the original task. A user cancellation or intervening
turn must invalidate the transition, and a crash must not duplicate accepted
tool work. Quota exhaustion of every eligible row must retain the task and wait
for a measured reset without cycling accounts indefinitely.

Qualification must include simultaneous independent processes, two accounts
exhausting in sequence, all accounts exhausted, fresh roster changes, renewed
credentials, crash/restart at receipt boundaries, cancellation, and unchanged
conversation provenance. Test both interactive and exec hosts. Keep synthetic
protocol qualification separate from live provider/email evidence.

## Refresh-lock validation

Commit `a833c8a8f3` removes the unlocked fallback when another process holds or
storage refuses a file login's refresh lock. The native auth glue maps the
failure to `RefreshTokenError::Transient`, so it does not record a permanent
credential failure. Its new integration case checks that a refused lock sends
no token request and preserves both stored and cached credentials.

The actual `alab-core/src/lib.rs` and its modules passed all 151 unit tests in
an isolated Cargo harness seeded with this repository's committed dependency
lock. This includes eight refresh-lock tests. The harness lives at
`$HOME/.aterm/.ubd/codex-parallel-login-core-check`; it only redirects the
manifest and cache and does not copy or replace the source files. The first
exploratory eight-test run used newly resolved dependencies; the 151-test run
then used the repository's versions. These checks do not compile the login
crate's glue or establish that the product binary contains this change.

The original workspace test process remains queued behind another build's
package-cache lock. Native login integration, full product build, landing,
and the cross-account transition remain outstanding.

## Account-count implementation, pending qualification

The companion aterm worktree is
`$HOME/.aterm/worktrees/aterm-codex-accounts-s26`, branch
`codex-accounts-s26`, commit `2c01634247`. It removes the fixed account count
from roster parsing/sync, retained quota and login state, shared pins, native
preparation, and account reporting. File byte budgets still apply; direct
snapshot writes now refuse oversize images before replacing readable state.
The rotation breaker retains all observations in its time window rather than
losing evidence after 128 rotations. Native scheduler models now require new
accounts to remain eligible while preserving cooldowns and identity admission.

New cases cover 257-row Codex/Claude parsing, 96-account state and scheduler
behavior, 192 retained rotation observations, shared adoption and roster growth,
and a sibling survey with 8,192 account facts. These are test definitions, not
passing results: aterm validation is still fetching dependencies. The Codex
workspace run is also still fetching dependencies. The isolated core lint
check did pass with warnings denied. Neither branch has been merged or installed.

## Cross-account protocol work

`alab-aterm/src/rotation.rs` adds a separate `codex-rotation` client. Prepare
names the failed thread/turn and source account/user digests. A receipt also
names a distinct destination account/user; it carries no path or command.
Ready, committed, consumed, retired and waiting responses preserve those
bindings. Invalid or ambiguous responses are rejected without echoing peer
text. All calls require the client's native self-claim.

The standalone client harness passed all 80 tests, including rejection of
same-account, ambiguous, malformed and renewal-protocol receipts and rejection
of unclaimed calls before any request reaches the host. Its lint check passed
with warnings denied. This is client protocol validation only. The host handler,
durable quota intent, destination authentication check, private restart and
original-turn continuation still need integration before rotation can run.

## Native pool planning entry point

The companion aterm branch now exposes `accounts_pool::native_quota_plan` for a
self-supervised Codex session at a stamped weekly wall. It uses the existing
pool selection and delayed commit checks, while ordinary window-driven planning
still refuses self-supervised sessions. Captured source/destination rows are
planning data only. The host still must prove the native wall and identities,
authenticate the destination and authorize the restart. Regression definitions
cover separate window/native behavior and policy or target-generation changes
before commit. These tests are pending the aterm dependency build; no runtime
rotation or host endpoint is claimed yet.

## Receipt and usage identity preparation

On the aterm branch, `f8f351cd8b` adds a purpose to the existing durable account
recovery receipt. Existing renewal records retain their same-account encoding;
a weekly-quota receipt requires an explicit different destination account.
Changing that destination changes the immutable intent. Typed usage-failure
history and the `rotation` marker remain distinct from login failure and the
`renewal` marker. This metadata work does not activate the new host endpoint.

`303d3cf873` adds identity-bound usage reads using the exact credential snapshot
sent in the successful HTTP request, including refresh/reload paths. Its new
loopback case replaces the login file before replying and checks that the old
account still owns the usage result. These aterm changes await compilation and
runtime tests. The original Codex workspace run has now passed all eight
refresh-lock tests; the native login-manager regression is building.

## Host-side candidate preparation

The aterm branch's `0bfd8edf35` adds `codex_rotation::prepare`. It requires a
managed self-supervised weekly wall, confirms fresh exhaustion on the exact
source identity, and uses the existing pool's candidate selection with a fresh
identity-bound usage check per candidate. Unusable or same-account candidates
are excluded without allocating a rotation. The entire probe sequence shares
one deadline, and disabled rotation makes no provider request. The returned
proposal retains the delayed pool commit; it neither switches accounts nor
restarts a process.

The native Codex login test build found two missing imports in the added test
case; those are corrected and that test is building again. Both the aterm
build and native Codex test have progressed beyond dependency fetching. The
control handler, idempotent pool commit linked to the durable receipt, private
restart, and original-turn continuation still need completion and qualification.

## Durable pool admission and first native test results

The native Codex auth-manager regression now passes, including refusal to send
refresh credentials when the account lock cannot be acquired. The isolated
core harness passed 151 tests and its warnings-denied lint; the isolated aterm
client harness passed 80 tests and its warnings-denied lint. These results do
not substitute for the complete workspace gate.

Aterm commit `5485aeffdd` adds a native receipt-to-binding map in guarded login
schema 5. The pool records this binding in the same durable transaction as its
rotation accounting. A retry of that receipt returns the prepared handoff
without charging a second rotation; a different binding for the same receipt
is refused. Existing login schemas remain the encoding when no native receipt
is present. Ordinary metadata edits cannot erase or change retained receipts.

The new receipt retry, schema guarding, identity-bound candidate preparation,
and disabled-policy tests passed in the first native account run. That run had
733 passing tests and one failure in an old hostile-state size assertion; the
fixture now checks the actual snapshot byte budget instead of a size derived
from the removed account count. The corrected run and the aterm verifier are
in progress. Native receipts still need terminal lifecycle cleanup, and the
host must bind these commits to its private restart receipt before exposing a
quota-rotation endpoint. No installed runtime or live account switch is claimed.

## Shared recovery state and continuation protocol

`alab-core::account_handoff` now represents both login renewal and weekly-quota
rotation. A quota receipt retains the original request identity and a distinct
destination; receipt binding cannot replace an existing selection or repair an
invalid admission state. The original native turn, start count and goal remain
part of the same handoff. The TUI writes quota custody under `owed.rotation`,
keeps login custody under `owed.renewal`, and refuses records naming both.
Its cold-restart custody checks now compare destination identity as well.

The autopilot's existing continuation lifecycle uses a purpose-aware protocol
adapter, including receipt status, commit and consumption. Destination identity
is checked before admitting recovered work. This is shared continuation
plumbing: automatic weekly-wall intent creation, quota goal provenance and the
aterm quota handler are still required before this path can activate.

The isolated core harness passed 154 tests. The handoff model's three focused
tests and warnings-denied lint passed, and the TUI/autopilot all-target compile
check passed. Their full library test run is still building. The earlier full
Codex gate was deliberately stopped while integrating these changes and does
not count as a passing gate. Aterm's revised native-commit guards passed all five
quota proposal/retry tests and its formatting, lint, Linux and Windows checks.
The wider aterm host run is still in progress; its Linux cell found a benchmark
import warning already corrected on origin/main, outside the account changes.

## Quota intent and goal provenance

The autopilot now prepares quota recovery at a native usage wall only when the
host advertises `codex_rotation=1`, account routing is `early` or `wall`, and
relaunch/resume policy allows it. The source still needs the live subscription
request scope. A `not-needed` prepare reply releases this intent to ordinary
quota recovery and suppresses another rotation attempt for the same accepted
turn, so a five-hour-only wall cannot become an indefinite weekly rotation wait.
No current host capability is advertised by this work yet.

Amended 2026-11-12. Only a spent week rotated when the paragraph above was
written. aterm's rotation branch, as the design for this amendment read it,
also leaves a five-hour wall whose reset is more than 900 seconds away, and
codex-alab now ends a creditless interactive turn at such a wall for a host
that says so (`alab/continuous-earned-resets.md`, the two sections dated
2026-11-12):

- The host says so with `codex_rotation_five_hour=<seconds>` in its `harness
  policy` reply. aterm 0.198.0 printed `codex_rotation=1` without it
  (measured on this Mac, 2026-11-12), so every five-hour wall waited in its
  turn there. aterm main prints it since 2365b983b6c (2026-11-12), as
  `codex_rotation_five_hour=900`, with the handoff that leaves such a wall
  (6380d03f686). aterm 0.200.0, installed here since 2026-11-13, is the
  first release with both: its `harness policy` reply read
  `codex_rotation=1 codex_rotation_five_hour=900` on this Mac that day.
  0.201.0, installed by 2026-11-14, reads the same, with no
  `codex_rotation_login` (measured 2026-11-14).
- `not-needed` still releases the intent to ordinary quota recovery and
  suppresses another attempt for the same accepted turn.
- The ask is now bounded by the wall's own reset: an unbound handoff that the
  host keeps answering `waiting` or an error for is given up at the reset
  plus the reset grace, counted across host sleep, and the session resumes on
  the account it is on. An unbound intent carries its wall's reset in the
  record, so the process after a restart keeps the limit (repaired after
  review, 2026-11-12; the re-exec shape is tested on loopback fixtures, a
  crash relaunch is not). Still asked about without limit: a wall whose 429
  names no reset, and an intent that was bound before a restart on the same
  account and retired after it.
- A `codex exec` run keeps its request at every wall and asks aterm through
  its own protocol. For a wall that follows an earlier `not-needed` in the
  same run it needs a fix in aterm's exec control, which the design found
  keeps that answer for the whole root turn.

Not run when this was written: any native qualification of the five-hour
handoff. The two cases planned for it in `alab/smoke/renewal_follow.py`
(`five-hour-quota`, `five-hour-near-reset`) needed a `native_host` fixture
built from an aterm that prints the word. Both are in the tree now:
`five-hour-wall-soon` since 2026-11-12 (`2d19fd2e13`; its first run is in
"Results of 2026-11-12") and `five-hour-wall-far` since 2026-11-13
(`42688c7af4`). Both passed against such a fixture with the signed 0.7.0
package ("The five-hour handoff, 2026-11-13"). Those are fixture runs, not
live ones.

The existing goal witness transaction now records an owned quota failure as
`limited` alongside the `UsageLimited` goal state. Restoration retains the
original goal ID, objective, usage accounting and budget. The same edit-revoking
triggers apply, and a generic usage-limited goal without that witness cannot be
restored by account recovery. Upstream receives only marked enum/match hooks;
the implementation remains in ALab glue. `alab/delta.sh --topics` could not run
because this checkout has no upstream remote; the two upstream diffs were
inspected directly and contain only these marked hooks.

The goal runtime's four focused tests and goal store's 24 tests pass, as do all
80 native client tests and the latest autopilot all-target compile check. The
prior shared-handoff component's full library run passed 735 autopilot tests
(one ignored) and all 315 TUI tests. A focused run on the newly changed autopilot
is still compiling. The companion aterm branch merged origin/main, including
the benchmark import correction, and its new non-weekly outcome passed all five
quota proposal/retry tests. The native quota host handler, private restart
integration, terminal receipt cleanup and live qualification remain unfinished.

## Host admission and remaining qualification

The focused quota intent run finished with four passing tests. Aterm commit
`7d5ba47be1` adds the selected-account guard used by private restart admission:
it checks both credential identities, the target generation, current policy and
pin state, and the immutable pool receipt binding. Ready preparation has a
separate guard and cannot authorize execution. Six quota proposal tests,
25 agent renewal tests and 15 host renewal tests passed. Its formatting check
passed; its later lint/platform run was stopped before completion while work
continued on the endpoint, and is not a passing verifier.

The missing `upstream` remote is now configured for OpenAI's repository. The
hook audit ran successfully against this commit: the newest contained upstream
base is `3026a8e54e`, and all three account-quota hooks appear under `accounts`.
The audit found 102 newer upstream commits, which have not been merged here.
This restores the audit prerequisite without claiming a full Codex gate pass.

The native quota endpoint is under development in the companion aterm
worktree. Capability advertisement remains off until native restart and
original-turn continuation are qualified. Terminal pool-receipt cleanup also
remains required. The current recovery loop requires Autopilot to be acting;
manual-session behavior must be addressed before the broader parallel-login
and seamless rotation goal is complete. No live quota switch, installed runtime
support, or remote-main landing is claimed by these checks.

## Shared email authentication boundary

Email authentication belongs to aterm's provider-neutral mailbox service.
Codex enrollment and renewal must use aterm's account methods, as Claude Code
does; this project must not grow a separate Proton Mail client or OTP reader.
In current aterm, provider sign-in receives `MailboxConfig` through
`VendorApprover`, and the shared `headless_oauth` mailbox reader delegates
Proton retrieval to the native mail layer. Codex-specific code handles OAuth
and account identity, while that shared layer owns mailbox connections,
message selection, decryption and authentication-code retrieval.

After pulling current main in both task worktrees, the installed aterm's
`accounts list --cli codex` reports `main` usable and `codex-m22` not enrolled.
A native `accounts renew codex-m22 --cli codex --here` attempt reached the
headless-browser prerequisite and stopped because that installed build found
no browser. It did not test live OTP retrieval or change the live login. The
current aterm source has native headless-browser preparation; live validation
must use that updated build and its existing shared mailbox/auth flow.

The updated aterm CLI (0.151.0, built from the pulled source) subsequently ran
that native renewal flow. It reached OpenAI's authorization page and stopped
when the provider's Cloudflare check did not clear in 90 seconds. No live login
changed, and no bypass was attempted. This is not evidence of live mailbox OTP
retrieval. The shared-mailbox suite passed 75 tests, and a new cross-CLI test
passed with OpenAI and Anthropic codes in the same inbox, in both read orders,
using aterm's common reader and each CLI's sender scope.

The private canonical package built successfully and passed its version, layout,
signature and bundled-tool checks without promotion. The first real-process
weekly-quota run stopped during bootstrap because the fixture used HTTP and
Codex requires an HTTPS workspace backend. The fixture now uses its generated
CA for both clients: Codex's existing CA setting and an explicit debug-only
aterm fixture transport restricted to loopback HTTPS. Production platform trust
and OAuth refresh verification remain unchanged. The TLS fixture rejects an
untrusted connection; all 15 controller tests and 51 aterm auth-reader tests
pass. This correction is not yet evidence of a completed native quota switch.

The real-process run then exposed an incorrect exhaustion threshold in aterm:
`PCT_MAX` is the parser's upper bound (1,000), not a full quota (100 percent).
The rotation reader now uses 100 percent for exhaustion and availability.
Its proposal tests now parse the real backend response shape instead of
inventing samples at the parser bound; 44 quota tests pass. The fixture also
uses separate emails and user IDs, because aterm correctly rejects duplicate
account rows for one provider email. The native handoff remains under test.

## Native weekly-quota qualification

The `weekly-quota` real-process case passed on 2026-11-03. It used the private
canonical package from `fcd1de322e`, an aterm native host built from
`029eb3d8ed`, and controller `2e56bf5d25`. The isolated result is at
`$HOME/.aterm/.ubd/codex-weekly-quota-complete/result.json`; its guardian
returned zero without a timeout.

The source process (PID 48998) received the typed weekly-quota refusal. Aterm
selected the distinct destination account and restarted the same conversation
as PID 49395 with its separate auth file. The destination accepted the original
turn ID, preserved the original input and completed plan/tool output, and made
one additional model request. The receipt reached `consumed`, one rotation was
charged, and its terminal pool-binding entry was removed. Both process births
and the canonical executable were checked against the native receipt.

This proves the isolated happy path with synthetic credentials, not live quota
exhaustion or enrollment. The subsequent five-second quota polling change needs
a fresh package measurement, and the compensation path needs separate fault
qualification before the production capability can be enabled.

The package built from `b8a9f83eca` also passed the same happy-path case with
native host `ddafa62ae9`. In that isolated run, Signal occurred 11.694 seconds
after host start and the native resume at 14.554 seconds, compared with 121.258
and 124.035 seconds in the preceding run. Both runs preserved the same original
turn, input and completed tool output and charged one rotation. The newer result
is `$HOME/.aterm/.ubd/codex-weekly-quota-fast/result.json`.

The `weekly-quota-rollback` case passed against host `2b7e41b868`: after Signal,
the controller removed the destination credential before its launcher attached.
The native engine restored the original auth path and conversation, did not
accept or replay the failed turn, and left the target unavailable. Its retained
pool-binding entry exposed the next cleanup requirement. The current host adds
cleanup only when the matching native completion ledger proves that rollback
finished; that strengthened case remains under test.

The strengthened rollback case subsequently passed against host `371042c61c`
with the fast package. It restored the source without replay, preserved the
single rotation charge, and removed the terminal binding. The host's background
service now also retains cleanup custody until the native completion record is
available, so cleanup does not depend on another client poll. Its regression
test verifies that a retired receipt without completion evidence keeps that
service alive. The result is
`$HOME/.aterm/.ubd/codex-quota-background-rollback/result.json`.

## Rotation smokes against a recorded host, 2026-11-12

No rotation case is qualified by this section. It records how the rotation
smokes are run against today's aterm test host, what their fixture now serves,
and what was run on 2026-11-12 with the only package this Mac had: the
installed `2026111101`, built from `49f3c2ba233b`, which predates the rotation
branch (it holds neither `c7641e9bad` nor `d2f4338223`). A TUI quota case needs
a package whose source contains `c7641e9bad`. None could be built:

```sh
alab/package.sh --commit ae9b0eb15e --any-commit --no-promote \
  --out "$HOME/.aterm/.ubd/codex-rotation-2026-11-12/pkg"
# alab: a qualified atpkg-publish is required (ALAB_ATPKG_PUBLISH)   exit 1
```

`aterm pkg which atpkg-publish` reported it absent from the signed index. The
refusal was not worked around. The package holds no Python, so it is not
rebuilt per smoke commit.

### What the fixture serves

Before this work a walled TUI turn against the fixture never ended. The
fixture's usage windows had no `reset_after_seconds`; the client's
`RateLimitWindowSnapshot` requires it, the whole usage read failed, and the
turn polled until the smoke's hang detector. `alab/smoke/provider_fixture.py`
now builds the usage, earned-reset list and redemption replies, and `check()`
refuses one the client would not decode. A reply the fixture cannot build is
recorded in `model.fixture_faults` and fails the run at once instead of at the
detector, in the TUI controller and in both exec smokes
(`alab/tests/test_rotation_smoke_runs.py` runs each controller whole against a
stand-in host and requires it).

The refusal of a model request at a wall is built there too (`refusal`): the
429's body and its `x-codex-*` headers come from one reading of the account's
usage, so the reset the body names is the spent window's own, and `check`
refuses a refusal the client would not read as that wall. The three 429s of
`renewal_follow.py` are built this way; the exec smokes' own 429s (a body with
no plan and no windows) are as they were.

The module's golden replies (`alab/smoke/fixtures/provider/*.json`: three
usage readings, the credit list, the redemption reply and the weekly and
five-hour refusals) are decoded by the real client in the gate, with no
package. The usage replies go through `codex-backend-client`; the refusals go
through `codex_api::map_api_error`, as a refused model request's status,
headers and body do, and must come out as a usage limit with the plan, the
reset, the reached type and both windows the fixture stated:

```sh
cd codex-rs && targo --unverified test -p codex-alab-quota-recovery fixture_tests
# 8 passed; whole crate: 49 passed; 0 failed
```

The fixture also now states each account's windows in rate-limit headers on
every model response, keeps the source login healthy until the provider has
refused it, routes the redemption, token and usage endpoints by path, gives
each fixture login a JWT access token, and records which program sent each
request (`client`: `host` for aterm's usage reader, whose User-Agent is
exactly `codex-cli`, else `codex`).

A fixture login carries no `tokens.login_owner_id`, as before this work. A
real sign-in writes one, and `09cd86ba9d` gave each fixture login its own; it
was withdrawn. With distinct owner ids the client's `same_owner()`
(`codex-rs/login/src/auth/change_state.rs`) answers false for two logins of
one account and user, where with none it answers true, and that is what every
same-account case presents. Those cases last passed with no owner id, and no
run here could judge the change: the one `happy` trial made with it
(`t15-happy-step8`) differs from the two made without it. It is to be applied
again alone, once `happy` has run with a rotation-branch package before and
after it.

### Recording the host

The host binary says nothing of its source. `native_host_record.py` writes
`build.json` beside it: the image's sha256 and size and the verdict of the
host's own no-argument self-run are measured; the aterm commit is attested, with
a mandatory sentence saying how it is known and a field saying what that
attestation is worth. `"attestation": "trial"` says the commit is not proven to
be the binary's source. `"built-from-clean-commit"` says the host was built
from a clean checkout of it, and the record then names who built it and when
(`builder`, `built_at`). A host that fails its self-run gets no record, and a
record is never replaced; a record of an earlier schema, which no longer loads,
is kept beside the new one as `build.json.legacy`.

Only a `built-from-clean-commit` record whose self-run was made qualifies a
run. `renewal_matrix.py` refuses any other before a case runs. For the requested
cases, retain the invocation's observed zero exit, `qualification.json` with
`passed: true`, its source/image hashes and the referenced case results. The
file alone is insufficient: a failed final save can leave passing bytes while
the CLI exits nonzero. This evidence covers the requested cases; the report's
coverage fields state whether the base or full available case set ran. A
single case can still be run against a trial record with `renewal_follow.py`;
its `result.json` carries `nativeHost.attestation` and
`nativeHost.qualifies`, as the exec smokes' reports do.

The host file must be named `aterm` or `aterm-*`. aterm's live snapshot accepts
a session only when the process that opened its terminal has such a name
(`owned_by_aterm`), and here that process is the host. Under the name
`native_host` every snapshot is refused as `terminal` and each TUI case ends at
its hang detector; `renewal_follow.py` and `renewal_matrix.py` now refuse such
a host by name.

```sh
cp <aterm target>/native_host <dir>/aterm-native-host
python3.14 -B -I <wt>/alab/smoke/native_host_record.py \
  --host <dir>/aterm-native-host \
  --source 414e1ae381d4bbd4272a161ca20a24eb1a714961 \
  --aterm-checkout <aterm checkout> --source-evidence '<text>' \
  --attestation trial
# a host that qualifies a run:
#  --attestation built-from-clean-commit --builder '<who>' \
#  --built-at 2026-11-12T09:30:00-08:00
```

The record used by the runs in the table below is
`$HOME/.aterm/.ubd/codex-rotation-2026-11-12/native-host-trial-aterm-name/build.json`,
written under schema 2, which had no attestation field; the runs made after
the review use a second byte copy of the same image recorded as a trial under
schema 3, `.../host-trial-s3/build.json`. Both say:
sha256 `08c2e3978e363336df385f1c8784f6c87eff3f6bbc1a94306f9c910023687841`,
178839392 bytes, self-run `PASS real-host shipping-qualified policy and
wrong-token refusal; worker EOF joined`. Its evidence text is: "TRIAL,
provenance unproven: a byte copy of
codex-rotation-2026-11-11/native-host-5h/native_host (mtime 2026-11-12 00:17:48
-0800), renamed aterm-native-host so that its process name begins with aterm-.
That binary was handed over as built from aterm 414e1ae381d, but the commit is
dated 00:18:27, after the binary was written, so it was built from a working
tree that only later became that commit, or from an earlier one. It was not
rebuilt from the commit and reports no source of its own."

A record that qualifies a run needs a host built from a clean checkout of the
landed aterm commit, that commit as `--source`, `--attestation
built-from-clean-commit` with the builder and the build time, and evidence
text saying how that is known; the hash, size and self-run verdict are then
measured from that file, named `aterm-*`.

### A trial package

No package can be built on this Mac: `alab/package.sh` needs a qualified
`atpkg-publish`, which the signed index does not name. A fix that is on main
therefore cannot be run against a smoke, and the `happy` fix cannot be shown
to work. The package side now has the lane the host side has: a package that
says of itself that it is a trial, which a single case may use and which
qualifies nothing.

`alab/smoke/package_standing.py` reads a package's own `alab-build.json` and
says whether it qualifies a run. It reads the record and verifies no
signature. A record qualifies only when it holds what a certified packager
always writes:

- `source` is a full 40-hex commit, and `version`, `target`, `packager`,
  `ripgrep` and `ripgrep_features` are stated;
- `compiler.admitted` is `true`, `compiler.rustc_vV` names `binary: trustc`
  and begins with `compiler.rustc`, `compiler.sysroot` is stated,
  `compiler.binaries_sha256` is 64 hex digits, and `compiler.id` is the first
  16 hex digits of the sha256 of those three joined by NUL, as both packagers
  compute it;
- `compiler.lane`, where present, is `certify`;
- its packager's own evidence of a certified build. codex-release's native
  layout, which wrote the signed releases, names itself
  (`packager: "codex-release/native-layout-v1"`) and records
  `build_observation_sha256` and `ripgrep_build_observation_sha256`.
  `alab/package.sh` names its own 40-hex commit as `packager` and records
  `rusty_v8`, `built_at` and `compiler.verification`: a `certify` build of
  the same source with dependencies included, with the report's hash and the
  built and packaged hashes of `codex` (`alab/proof_build.py`). A
  `verification` on any record must be that;
- there is no `trial` key, the version does not name a trial in any case,
  and every string in the record is Unicode text.

`compiler.admitted: true` alone shows no certified build. package.sh wrote
it while its release build was still `targo --unverified build --release`,
which it was until `18a4e8404b` (2026-11-12 22:19) made it a certified build
and added `verification`. A package.sh record without `verification` is
therefore an unverified-lane build and does not qualify. One is on this Mac:
`~/.local/share/aterm/dev-builds/artifacts/codex/0.1.0-dev.13916+codex.0.165.0.g1d44d6108`,
built by package.sh at `1d44d61087` (2026-10-28), with `admitted: true`. Its
record was read: `renewal_matrix.package_refusal` answers `its record
carries no build verification (compiler.verification)`.

The signed 0.6.0 and 0.7.0 records qualify (both read with the predicate as
it is now). A record with a `trial` key never does, whatever else it holds,
and an unknown or malformed record does not either; the predicate answers
for any JSON value and raises on none. Every `result.json` of
`renewal_follow.py`, `exec_accounts.py`, `exec_shared_pool.py` and
`parallel_logins.py` carries `package: {qualifies, why, trial}`, beside
`nativeHost` where the smoke takes a host; a run's `passed` does not depend
on it, so a `passed: true` from a trial package is trial evidence and its
`package.qualifies` says so. `request_recovery.py` (under `package`) and
`weekly_reset.py` (under `packageRecords.build`) carry the whole
`alab-build.json` and no standing: a trial shows there as the record's
`trial` object. `renewal_matrix.py` refuses a package that does not qualify
before any case runs, as it refuses a trial host, with `the package's own
record qualifies no run: <why>; run a case singly with renewal_follow.py for
trial evidence`, and its `qualification.json` carries that as `error`.

A trial package is assembled from a development build:

```sh
cd <wt>/codex-rs && targo --unverified build -p codex-cli --bin codex
python3.14 -B <wt>/alab/smoke/trial_package.py \
  --binary <wt>/codex-rs/target/debug/codex --checkout <wt> \
  --source <40-hex commit> --output <new dir> --builder '<who>' \
  --built-at 2026-11-13T09:30:00-08:00 --evidence '<how it was built>'
# prints <new dir>
```

The binary's sha256, size and modification time are measured. Its source is
the builder's statement, not a proof, and the record says so. The tool
requires what can be read without building:

- `<wt>` is a worktree root at `--source` that `alab/qualification.py` finds
  clean (nothing staged, changed or untracked, compared through a private
  index).
- The binary lies in `<wt>`, in a path Git ignores: what a build left in
  that worktree. The codex of an installed package, a build from another
  worktree, a link in the build directory that leads out of it and an
  executable file of the commit are each refused. A symbolic link as the
  binary is refused too.
- Beside the binary is the dependency list its build wrote (`codex.d`; cargo
  writes one with every binary), the list names this binary, it names files
  of `<wt>`, each is still there, and none was written after the binary.
  That is how cargo judges a build up to date, so a binary refused for it
  is one that building again in `<wt>` rewrites (reasoned from cargo's rule;
  no build was run for this). A build directory outlives the trees its
  worktree moves through, and this is what tells: a checkout of another
  commit rewrites the files that differ.

The binary is not compared with its commit's time. An earlier form of the
tool refused a binary written before its stated commit was made. No build
writes the binary again for a commit that changes none of its inputs (every
commit of the branch that added this tool is one), nor for a commit made
after the build of the tree that was built, so that refusal could not be
met by building again. Both times are in the record.

What this does not notice: the list is the build's own statement, the one
real list read (below) names no `Cargo.toml` and no `Cargo.lock`, and a file
edited while the build ran, before the binary was linked, is older than the
binary. A binary that passes may still be of another tree.

The tool also refuses an `--output` that exists, or that resolves under an
aterm package store (a `pkg` or `dev-builds` directory inside one named
`aterm`, in any case; this account's two; the root `ATPKG_BIN` names) or
under the checkout. The store roots and the checkout are told by the
directory itself (device and inode), not by how the path is spelled: this
Mac's volume ignores case. It copies the binary, refuses one that changes
while it is copied, signs nothing, links nothing, never runs the binary, and
leaves nothing behind when it refuses.

The package it writes:

- `bin/codex`, a copy, and nothing else. The binary takes ripgrep, its
  resources and its helper programs from its package only when they are there
  (`codex-rs/install-context/src/lib.rs`: `path_dir` and `resources_dir` are
  optional, and `rg` otherwise comes from `PATH`), so none is copied in and
  `codex-package.json` names only the entry point. This was read from the
  source; no session has been started from such a package.
- `alab-build.json` with `version` `<VERSION>-trial+g<sha9>` (the commit's
  `VERSION` file; a semver prerelease, so no comparison takes it for the
  release), `compiler: {admitted: false, lane: "unverified"}`, and `trial:
  {attestation: "unverified-development-build", statement, source,
  source_committed_at, binary_sha256, binary_bytes, binary_modified_at,
  build_inputs, newest_build_input_modified_at, builder, built_at, evidence,
  layout}`. `build_inputs` is how many files of the checkout the dependency
  list names.

A case run with it is trial evidence, as a run against a trial host is. The
first one was assembled and run later that day ("Trial runs of 2026-11-13").
Before that, the one
development build found on this Mac on 2026-11-13
(`codex-alab-admission/codex-rs/target/debug/codex`, 1105649128 bytes,
written 19:36:16 -0800) ran outside a package: `--version`, with a private
`HOME` and `CODEX_HOME`, printed `codex-cli 0.0.0`. It was not packaged, and
the tool was not run on it: the worktree is another agent's, in use and not
clean. That binary has since been rebuilt (1105653224 bytes, written
21:48:26 -0800; not run). Its dependency list was read with the tool's own
functions, which only read: it names 3822 files of that checkout, and 16 of
them were written after the binary, the newest at 22:04:24. The tool would
refuse it for that too, and a build there would rewrite it.

### Trial runs of 2026-11-13

One trial package has been assembled, from codex-alab main `81ec56a7b9`, the
commit that put the recovered turn's admission retry on main. `codex` was
built there with `targo --unverified build -p codex-cli --bin codex` in a
clean worktree detached at that commit, and `trial_package.py` wrote
`trial-81ec56a/pkg` under `$HOME/.aterm/.ubd/codex-rotation-2026-11-13/`:
version `0.8.0-trial+g81ec56a7b`, binary sha256 `2d60f46a06cf26d6…`,
1105943352 bytes. Its record says it is a trial.

Each case below was run once with `renewal_follow.py`, harness at
`7f3b96b296`, against the host built from clean aterm main `7f1450f9dda`
(`host-q-7f1450f/`). Every `result.json` says `package.qualifies: false`.

| Case | Run | Result |
| --- | --- | --- |
| `happy` | `trial-81ec56a/runs/happy` | passed |
| `weekly-quota` | `trial-81ec56a/runs/weekly-quota` | passed |
| `weekly-quota-chain` | `trial-81ec56a/runs/weekly-quota-chain` | passed |
| `weekly-quota-rollback` | `trial-81ec56a/r/wqr` | passed |
| `weekly-quota-exhausted` | `trial-81ec56a/r/wqx` | passed |
| `five-hour-wall-soon` | `trial-81ec56a/runs/five-hour-wall-soon` | passed |
| `five-hour-wall-far` | `trial-81ec56a/runs/five-hour-wall-far` | passed |

What this shows, and what it does not:

- `happy`, the same-account login renewal, failed with the signed 0.7.0
  package at its hang detector ("Why `happy` fails"). With main's build it
  passed. That is one run of an unverified build: evidence that the fix
  works, not a qualification of it.
- The six usage-limit cases pass with main's build as they do with 0.7.0, so
  nothing landed since 0.7.0 was seen to break them in one run each.
- Nothing here ran live, was repeated, or used a certified package, and the
  matrix refuses this package.

Two runs do not count: `trial-81ec56a/runs/weekly-quota-rollback` and
`.../weekly-quota-exhausted` ended at once with the host's `path must be
shorter than SUN_LEN`. The output directory's path, which holds the host's
control socket, was too long under those two case names. They were run again
under the shorter `r/wqr` and `r/wqx` and are the rows above.

### Trial runs of the lost-login move, 2026-11-13 and 2026-11-14

A session whose own login is lost for good (its refresh token has ended) asks
the host, with `codex-login-rotation`, to move it to another account that is
already signed in (`alab/design/accounts-lost-login.md`). `renewal_follow.py`
has seven cases for it since `023a7188d0`. A review of that commit changed
where six of them serve the logins' issuer and tightened both oracles
(`ab3be75c24`); the runs under "Counted runs at `ab3be75c24`" are the ones
that stand, and the first runs are kept below them with what they are worth
now. Every run is trial evidence: the package is the unverified development
build `trial-81ec56a/pkg`, every `result.json` says `package.qualifies:
false`, and nothing ran live. Each account, login file, token and reply was
generated inside the run's output directory. The host is the one built from
clean aterm main `7f1450f9dda` (`host-q-7f1450f/`).

What the fixture serves in these cases: two accounts, each with its own login
file. The source's first model request is answered (the plan call). Its
second, which carries the finished plan, and every later request made with
the source's access token are refused with 401, its usage reads too, and a
refresh of the source's refresh token is refused with the case's own answer.
The other account is usable from the start, no window of either account is
ever spent, and nothing holds a reading back, so only the lost login can
explain a move.

What the provider's record keeps for these cases:

- For each token request, its status, whose token it presented (source or
  target, never the token), which listener it arrived at, and who sent it.
  The sender is read from the request's User-Agent and held to each
  program's own form: aterm's exact `codex-cli`, codex's
  `<originator>/<version> (<os>; <arch>) <terminal>`, or neither. A token
  request or a usage read from neither fails the run at once. Before
  `ab3be75c24` anything that was not exactly `codex-cli` counted as
  codex-alab's.
- Each request the fixture has no route for, on either listener. One fails
  the run at once.
- Each TLS handshake a client did not complete. A program that does not
  trust the run's test CA sends no request the provider could count, so its
  refused handshake is all the record can show of it.

| Case | The refresh is refused with | Host's policy prints `codex_rotation_login=1` | Logins' issuer | Must end |
| --- | --- | --- | --- | --- |
| `lost-login-expired` | 401 `refresh_token_expired` | yes | the fixture's TLS listener | moved (the host from `7f1450f9dda` cannot; one from `7540b6d94f2` does, see below) |
| `lost-login-expired-plain-issuer` | 401 `refresh_token_expired` | yes | a second loopback listener, plain HTTP | moved |
| `lost-login-invalidated` | 401 `refresh_token_invalidated` | yes | the fixture's TLS listener | not moved, nothing asked |
| `lost-login-reused` | 401 `refresh_token_reused` | yes | the fixture's TLS listener | not moved, nothing asked |
| `lost-login-invalid-grant` | 400 `invalid_grant` | yes | the fixture's TLS listener | not moved, nothing asked |
| `lost-login-unrecognised` | 401 `account_deactivated` | yes | the fixture's TLS listener | not moved, nothing asked |
| `lost-login-no-word` | 401 `refresh_token_expired` | no | the fixture's TLS listener | not moved, nothing asked |

The host prints the word when the fixture's config says `lost_login`.
`lost-login-no-word` uses the usage-limit cases' config instead
(`weekly_quota`): the same two accounts, and a policy without the word.

Where the logins' issuer is served. Every other case of `renewal_follow.py`
names the fixture's one TLS listener as the logins' issuer, so a refresh is
posted there, and six of the seven lost-login cases now do the same.
`lost-login-expired` is the specified case in that layout.
`lost-login-expired-plain-issuer` is the one departure, and is named for it:
it serves the issuer on a second loopback listener without TLS and writes
that address into both login files. Both programs take a login's token
endpoint from its recorded issuer and accept a loopback one without TLS
(aterm `refresh_endpoint`, `codex_app_login.rs` line 1609). It exists
because the host recorded from `7f1450f9dda` cannot present a token under
the fixture's certificate (the first finding below), so with that host the
standard layout cannot show a move at all. aterm closed that gap in
`2e7aa348a76`. A pass of it would be evidence of the move with the host's
refresh over plain HTTP and of nothing in the standard layout. It leaves its
own record, `lostLoginMovedOverPlainIssuer`, so nothing that looks for
`lostLoginMoved` is satisfied by it. Decided on 2026-11-14: a pass of it is
accepted as trial evidence that the move's own steps work (the host's
refusal, its settling minute, one read of the destination, the receipt, the
successor on the other login) and of nothing more. The case that qualifies
the move is `lost-login-expired` in the standard layout. It waited on
aterm's fixture presenting a refresh under the run's test CA, which the
fixture does since `2e7aa348a76`, and a host built from aterm `7540b6d94f2`
moved it in a trial run on 2026-11-15 ("Trial runs against aterm
`7540b6d94f2` and codex-alab `cdfe65abe3`, 2026-11-15"). Before
`ab3be75c24` six cases used the second listener and the move over it carried
the name `lost-login-expired`.

A run of a case that must move passes only if all of this holds:

- The source was answered once and refused with 401 from then on, and no
  other login was ever refused.
- Every token request arrived at the case's issuer, and every token request
  and usage read came from codex-alab or the host by its own User-Agent. No
  request went to a path the fixture has no route for, and no client refused
  the fixture's TLS handshake.
- The host's `harness policy` reply printed `codex_rotation_login=1`.
- After the loss, and not before, the host traced `codex-login-rotation
  prepare`. It answered `waiting`, and only `waiting`, until it answered
  `ready` for the run's receipt.
- The source's refresh token was presented once at most by codex-alab and
  exactly once by the host, each time refused with `refresh_token_expired`.
  `ready` came at least 60.0 seconds after the host's own token request, on
  the provider's clock.
- The run went on watching for 70 seconds after it saw the move complete
  (`LOGIN_MOVED_WATCH_SECONDS`), and in all that time no refresh token was
  presented.
- The host read the other account's usage exactly once in the whole run,
  as usable, after its own refusal and before `ready`. Nobody else read it
  before `ready`, and its refresh token was never presented.
- There is one receipt. Its purpose is `login_rotation`, it names two
  different accounts, and it goes from the source's login file to the other
  account's own login file. The successor ran on that file and answered the
  original turn (the checks every handoff case makes), and it sent that turn
  exactly once, the watch included.
- The host's pool was not rotated: `active` is still the source's row, no
  rotation was charged, no canary set, no rotation bound to a native
  operation. No `codex-rotation` and no `codex-renewal` line was traced.
- Neither login file changed by a byte, and the source's still holds the
  token that was refused: nobody was signed in and no token was copied.
- The session's own record held the lost login's intent for this wall (rule
  `login-rotate@v1`, under `rotation`). codex-alab kept no refusal of the
  token.
- The thread's own record, read after the watch, shows its turn ended once
  at the login wall and once answered, with one typed user message, the
  original prompt. No model request carries any other typed text, and the
  recovered request's input is the refused request's.
- Last, how often codex-alab asked. No two `codex-login-rotation prepare`
  lines are nearer than 4.5 seconds or further apart than 10: one ask a
  pass, a pass every 5 seconds (`alab/CONTRACT.md`, "A sealed claim"; the
  plan's smoke says the session "sends exactly one" line and "may send that
  line again on rechecks"). A run that fails only here keeps everything
  else it showed under `lostLoginHeldBeforeItsAsks`.

Two of these read the plan's smoke more narrowly than its words, and the
controller says so where it judges them. "Target /usage: exactly 1 hit" is
held for the host over the whole run. The successor reads the usage of the
login it now runs on, as any session does; those readings are counted
(`destinationUsageReadsBySuccessor`) and fail nothing after `ready`. A
literal single hit could not pass once a successor runs. "Waiting for at
least 60 s" is measured from the host's own refusal, which follows the first
ask, so the answer was `waiting` for longer than that. aterm dates its
refusal in whole Unix seconds (`codex_app_login_refusal.rs`, `settled`), so
by its own arithmetic it could answer up to a second sooner; the controller
holds the whole minute and would fail such a run by name. The measured
settle was 65.3 to 65.6 seconds in every run.

The row's `auth` in the host's pool after the move (`signed-out`) is
recorded as `hostSourceAuth` and is not required.

A run of a case that must not move passes only if all of this holds:

- The same loss, with the case's own refusal of the refresh, and the same
  conditions on the provider's record (issuer, senders, routes, handshakes).
- The host's policy printed the word, or did not, as the case is run under.
- What stopped the move is on record. For the four vetoed refusals that is
  codex-alab's own kept refusal: one file under
  `<CODEX_HOME>/alab/login-refusals/`, named by a digest of the refused
  token, holding `revoked` (invalidated), `exhausted` (reused) or `other`
  (`invalid_grant`, an unrecognised code). For `lost-login-no-word`, which
  keeps none, it is the policy without the word.
- Nothing of a move or an ask exists: no `codex-login-rotation` line, no
  `codex-rotation` line, no reading of the other account, no presentation of
  its refresh token, no receipt, no native action, no session process on the
  other login file, no model request from it. The controller fails the run
  the moment any of these appears.
- The host did nothing with the refused login that a move's job does. It
  presented the source's refresh token never, and no client refused the
  fixture's TLS handshake, which is how a presentation by this host would
  show in this layout. It read the source's usage no more than once for each
  time it was asked to renew that login.
- The login wall took its own path and stood on it: the host traced
  `codex-renewal prepare`, answered `waiting`, on the pass after the refusal
  and again on the next look a minute later, and the run watched for 70
  seconds after the refusal.
- The turn ended once, at the login wall, and was never answered. The pool
  and both login files are as they were.

How often `codex-renewal prepare` was asked is recorded
(`renewalAskGapsSeconds`) and not judged: the plan's smoke bounds nothing of
the login wall's own path but that it is taken.

#### Counted runs at `ab3be75c24`

One at a time, harness and tree at `ab3be75c24` (controller sha256
`949c3b59af38…` in every `result.json`), under
`$HOME/.aterm/.ubd/codex-rotation-2026-11-13/ll/`, 2026-11-14 00:17 to
00:28 -0800:

| Case | Run | Result |
| --- | --- | --- |
| `lost-login-expired` | `ll/t1` | failed by name: the host answered `not-needed`, and one TLS handshake was refused |
| `lost-login-expired-plain-issuer` | `ll/p1` | failed by name, on the last proof only: two asks 13 ms apart |
| `lost-login-invalidated` | `ll/iv2` | passed, veto `revoked` |
| `lost-login-reused` | `ll/ru2` | passed, veto `exhausted` |
| `lost-login-invalid-grant` | `ll/ig2` | passed, veto `other` |
| `lost-login-unrecognised` | `ll/un2` | passed, veto `other` |
| `lost-login-no-word` | `ll/nw2` | passed, no veto, policy without the word |
| `happy` | `ll/rh2` | passed |
| `weekly-quota` | `ll/rw2` | passed |

The last two are cases that existed before, run again because the fixture
server they share changed (it now counts refused handshakes).

No run shows a move that passes its case. The two findings follow.

**The recorded host cannot move a lost login whose issuer is under the
fixture's TLS certificate.** In `ll/t1` codex-alab's refresh was refused at
0.264 s after the first 401, it asked at 0.304 and 0.325 s (`waiting`), and
the host read the source's usage once at 0.314 s (401). At 0.345 s a client
opened a connection to the fixture's listener and closed it without
completing the handshake (`UNEXPECTED_EOF_WHILE_READING`). No host token
request reached the fixture, the host answered `not-needed` at 5.346 s, and
codex-alab went to `codex-renewal prepare` at 5.388 s. In `ll/p1`, where the
issuer is plain HTTP, the host's own token request arrives at that same
point: 10 ms after its usage read. The uncounted run `ll/y1` shows the same
refused handshake at 0.353 s, 24 ms after the host's read. The cause is read
from aterm's source at `7f1450f9dda`, and the host was not instrumented:
aterm's fixture reader (`read_identified_login_with_fixture_roots`,
`crates/aterm-agent/src/harness/codex_app_login.rs` line 2908) trusts the
run's test CA for the usage read only (its client, line 2928), and says so
("without changing platform trust or OAuth refresh transport"). The refresh
is posted at line 1858, inside `refresh_admitted` (which starts at 1789),
through `http_client` (lines 1393 to 1400), which verifies against the
platform's trust store, and the fixture's certificate is not in it. This is
a gap in aterm's fixture, not a wrong answer by codex-alab or by the verb.
What would close it is the fixture's roots reaching the refresh request as
well as the usage read; `lost-login-expired` should then pass unchanged.
Nothing in aterm at that commit drives the fixture's `lost_login` switch
(`git grep lost_login 7f1450f9dda -- crates/aterm-gui` finds it only in
`native_renewal_fixture.rs`), so aterm itself has not met the gap.

Closed in aterm by `2e7aa348a76` (2026-11-14, on aterm main through
`935adf75a93`), as its commit and source say: the fixture's read now posts
the refresh over the same client that trusts the run's test CA, and only to
a loopback token endpoint; every other path builds `http_client` as before,
and no release build compiles a fixture client. `lost-login-expired`, its
oracle unchanged since `ab3be75c24`, then passed in its first run against a
host built from aterm `7540b6d94f2`, which holds that commit (`r/le` of
2026-11-15). The host's token request reached the fixture's TLS listener
5.32 s after the first 401 and was refused `refresh_token_expired`, no
handshake was refused, and the host answered `ready` 65.47 s after its
refusal ("Trial runs against aterm `7540b6d94f2` and codex-alab
`cdfe65abe3`, 2026-11-15"). That is one trial run with an unverified
package; it qualifies nothing.

**codex-alab asks twice on its first pass.** In `ll/p1` the host traced
`codex-login-rotation prepare` at 0.180 and 0.194 s after the first 401,
each after its own session-status read, and then every 5.02 to 5.16
seconds: 15 asks, 14 answered `waiting`, the fifteenth `ready`. The host
answered both first asks alike and opened one job (one presentation, one
reading of the destination, one receipt), so nothing else failed. The same
pair is in every earlier moved run (`e1` 16 ms, `e2` 21 ms, `e3` 16 ms, `v1`
11 ms, `tls` 20 ms), and all five of today's must-not-move runs show it for
`codex-renewal prepare` (13 to 23 ms). It is a departure from the contract's
"one pass every 5 s" and from the plan's "exactly one" line, small in
effect. The suspected cause is read from codex-alab's source at
`81ec56a7b9`; the session was not instrumented, and its journal is not in
the run's files. The first follow pass of a login wall reads the login
(`alab-autopilot/src/renewal_follow.rs` line 1624, `login_for`), which arms
the watch on the login file (`extension.rs` lines 1778 to 1793). The watch's
task starts with `was_known_profile` false (line 1855), takes its initial
wake where a login file exists (lines 1860 to 1866), finds the profile
known, and starts a second evaluation (lines 1949 to 1961, "the original
login profile is known; checking native renewal"). Evaluations of a thread
are serialised and not merged (`evaluate`, line 3231), so that second one
runs the moment the first pass returns, reads the session's status again
and sends `prepare` again (`renewal_follow.rs` line 1796). The first pass's
five-second timer is replaced by the second's. One earlier run
(`ll/iv`) shows a single first ask, so the pair depends on timing. The
usage-limit traces show similar pairs at later looks and a single first
ask; those were not traced.

codex-alab's `1702dbe1df` (2026-11-14) seeds the watch's
`was_known_profile` from the decision that armed it, so the watch owes the
profile no second pass. In the seven lost-login runs of 2026-11-15, with a
package that holds it, no two asks were nearer than 5.02 s:
`codex-login-rotation prepare` in the two moved runs, `codex-renewal
prepare` in the five that stayed. The first ask came 0.04 to 0.05 s after
codex-alab's own refused refresh in four of them (`lp`, `liv`, `lig`,
`lnw`) and 5.05 s after it in three (`le`, `lru`, `lun`), one five-second
pass later; both oracles allow either, and why it varies was not traced. At
a usage wall the runs of 2026-11-15 show two `codex-rotation prepare` 0.11
to 0.30 s apart on the first look in every run that asked, as `ll/rw2` and
`trial-81ec56a/runs/weekly-quota` did (0.11 and 0.08 s); of the earlier
runs read again for this, only `trial-81ec56a/runs/five-hour-wall-far` asked
once there. No usage-wall oracle judges how often it is asked, and that pair
was not traced either.

What `ll/p1` measured, in seconds after the first 401, with the earlier
moved runs for comparison (they ran under the earlier oracle, below):

| | `p1` | `e1` | `e2` | `e3` |
| --- | ---: | ---: | ---: | ---: |
| codex-alab's refresh refused | 0.15 | 0.15 | 0.20 | 0.17 |
| first `codex-login-rotation prepare` | 0.18 | 0.18 | 0.24 | 0.19 |
| second, on the same pass | 0.19 | 0.19 | 0.26 | 0.21 |
| host's own refresh refused | 0.20 | 0.20 | 0.26 | 0.21 |
| host confirms the source (two usage reads, both 401) | 60.70 | 60.43 | 60.51 | 60.47 |
| host reads the other account, once | 60.89 | 60.53 | 60.57 | 60.63 |
| `ready` | 65.80 | 65.50 | 65.59 | 65.54 |
| successor's turn answered on the other account | 75.51 | 74.65 | 75.03 | 74.70 |
| the run stopped watching | 146.66 | 74.65 | 75.03 | 74.70 |

In `ll/p1` everything but the asks held, and `result.json` keeps it under
`lostLoginHeldBeforeItsAsks`: `ready` 65.61 seconds after the host's own
refusal; one presentation each by codex-alab and the host, both at the
issuer's listener; one reading of the destination by the host in the whole
run; the receipt's purpose `login_rotation`; the pool's `active` still
`main`; one recovered turn. The run watched 80.85 seconds past `ready`. In
that time the provider saw one request, the successor's own usage read at
130.50 s: no token was presented, the host read nothing, and the turn was
not sent again. Before `ready` the host read the source's usage once (401)
just before it presented the token, the refused request was sent twice and
refused twice before codex-alab presented its refresh token, and codex-alab
read the source's usage once more at 59.80 s (401). The successor read the
destination's usage four times. The whole run took 150 seconds.

In the five runs that must not move, codex-alab's refresh was refused 0.17
to 0.25 seconds after the first 401 and arrived at the fixture's TLS
listener. `codex-renewal prepare` was traced 0.03 to 0.05 seconds after
that refusal, again 13 to 23 ms later, and again 60.07 to 60.09 seconds
after the refusal, each answered `waiting`. The host sent no token request,
read no usage, and no handshake was refused. codex-alab read the source's
usage once more at 59.6 to 59.8 seconds (401). Each run took 72 to 74
seconds. The load average was 12.2 after the last of them.

#### The first runs, at `023a7188d0`

Counted then, one at a time, controller sha256 `a525c362652b…`, 2026-11-13
23:31 to 23:41 -0800. In these, six cases served the issuer on the second,
plain-HTTP listener, and `lost-login-expired` was the name of the move over
it:

| Case as named then | Run | Result then | Worth now |
| --- | --- | --- | --- |
| `lost-login-expired` (plain issuer) | `ll/e1`, `ll/e2`, `ll/e3` | passed | today's `lost-login-expired-plain-issuer`; each shows the doubled first ask and stopped watching 9 s after `ready`, so none would pass today's oracle |
| `lost-login-invalidated` (plain issuer) | `ll/iv` | passed, veto `revoked` | superseded by `ll/iv2` |
| `lost-login-reused` (plain issuer) | `ll/ru` | passed, veto `exhausted` | superseded by `ll/ru2` |
| `lost-login-invalid-grant` (plain issuer) | `ll/ig` | passed, veto `other` | superseded by `ll/ig2` |
| `lost-login-unrecognised` (plain issuer) | `ll/un` | passed, veto `other` | superseded by `ll/un2` |
| `lost-login-no-word` (plain issuer) | `ll/nw` | passed, no veto | superseded by `ll/nw2` |
| `lost-login-expired-tls-issuer` | `ll/tls` | failed by name: `not-needed` | today's `lost-login-expired`; superseded by `ll/t1` |

Other run directories under `ll/`:

- `ll/v1` (`lost-login-expired`, plain issuer, passed), `ll/v2`
  (`lost-login-invalidated`, passed) and `ll/rh` (`happy`, passed) were made
  by the two reviews of `023a7188d0`, from the committed tree at
  `735f85c6f1` (controller `e5f0e6e57569…`, which differs from
  `023a7188d0`'s by a comment), 23:47 to 23:57. They ran under the earlier
  oracle.
- `ll/x1` to `ll/x5` were made while the cases were being written, before
  `023a7188d0` existed. `x1`, `x2` and `x3` ran controllers that were never
  committed. `x4` and `x5` carry `a525c362652b…`, the controller that was
  then committed unchanged, but ran before the commit. None is counted.
- `ll/y1` (`lost-login-expired`, failed by name) ran the working tree that
  became `ab3be75c24` (controller `302fdb539150…`). It is not counted.

What this shows, and what it does not:

- With main's client (`81ec56a7b9`) and aterm main's verb (`7f1450f9dda`),
  no run of a case that must move passes. In the standard layout the
  recorded host cannot present the token. Over a plain-HTTP issuer the
  session was moved to the other account in five runs of five (`e1` to
  `e3`, `v1`, `p1`) by the handshake the design describes, with no sign-in,
  no copied token and nothing typed, and in each codex-alab asked twice on
  its first pass. Only `p1` was watched after the move and held to where
  its requests arrived and who sent them.
- In one run each, in the standard layout, the four refusals that must stay
  a stop for a person, and a host without the word, were neither moved nor
  asked about, and the host did nothing with the refused login.
- It is trial evidence. The package is an unverified build, the host is one
  build, and the provider is a fixture. No run was live: whether the real
  backend answers a refresh with these codes, and how a real `account
  deactivated` reads, is not determined here.
- None of the seven cases is in `renewal_matrix.CASES`, and a test names
  them as the only cases with a proof of their own that are outside it. The
  matrix refuses a trial package, so no matrix run could hold them, and no
  qualified package has the client (0.7.0 predates it). They go in when a
  certified package passes them.
- One run a case measures no failure rate.
- The host's fixture policy for these cases prints `codex_rotation_login=1`
  without `codex_rotation=1`. A host with both was not run.

### Trial runs against aterm `7540b6d94f2` and codex-alab `cdfe65abe3`, 2026-11-15

Every run here is trial evidence and qualifies nothing. The package is an
unverified development build, and every `result.json` says
`package.qualifies: false`. The host was built on Trust's unverified lane,
the provider is the fixture, and nothing ran live. Each account, login file,
token and reply was generated inside the run's output directory.
`renewal_matrix.py` refuses this package, so each case was run singly with
`renewal_follow.py`.

- **Host.** `host-7540b6d94f24/aterm-native-host`, built from a clean
  checkout of aterm main `7540b6d94f2` with `targo --unverified test -p
  aterm-gui --test native_host --no-run`. sha256 `719902cb207d0d5b…`,
  205412688 bytes, self-run `PASS real-host shipping-qualified policy and
  wrong-token refusal; worker EOF joined`. It is a trial host, built on
  Trust's unverified lane, and qualifies nothing, as the package qualifies
  nothing. Its record is `built-from-clean-commit` (schema 3), so every
  `result.json` made with it says `nativeHost.qualifies: true`. That field
  weighs only the record's attestation and self-run
  (`native_host_record.not_qualifying`). The record has no field for the
  lane: only its evidence sentence says that every run with it is trial
  evidence. Paired with a qualified package, it would give a run a
  top-level `qualifies: true` that no run here supports. The host of the
  qualifying runs of 2026-11-13 (`host-q-40e8515`) was built with the same
  command, so whether this lane can qualify a host is for the record's rule
  to settle, not for these runs. Its source holds the
  fixture's refresh under the run's roots (`2e7aa348a76`, through
  `935adf75a93`), `codex-rotation complete` (A1 and A2, `9fb6fb041d7`) and
  the fixture's launch shapes (`494e3f9976c`). Its fixture policy prints
  `codex_rotation_login=1` alone for the lost-login cases. For every other
  case it prints `codex_rotation=1 codex_rotation_five_hour=900
  codex_rotation_refusal=1 codex_rotation_limits=1`.
- **Package.** `pkg-cdfe65abe3`, version `0.9.0-trial+gcdfe65abe`,
  assembled by `trial_package.py` from `targo --unverified build -p
  codex-cli --bin codex` in a clean worktree at codex-alab main
  `cdfe65abe3`. Binary sha256 `8ff7875159df8a58…`, 1112796504 bytes. It
  holds the one ask at a lost login's wall (`1702dbe1df`), the background
  terminals' hold (gap 1), the exec wall classifier (gap 4, S3) and the
  `complete` client and report (gap 5, C1 and C2).
- **Harness.** `renewal_follow.py` from that same clean tree. Its
  controller sha256 is `70e34cf26af0…` in every `result.json`. The
  lost-login oracle is the one that failed `ll/t1`: since `ab3be75c24` the
  controller has changed only for `five-hour-wall-far-unnamed` and a
  listener's close.

Each case was run once, one at a time, under
`$HOME/.aterm/.ubd/codex-rotation-2026-11-15/r/`, from 16:39 to 17:05
-0800. The load average was 8 to 17. Every guardian returned 0 with no
timeout, and every host exited 0 without stalling.

| Case | Run | Result |
| --- | --- | --- |
| `lost-login-expired` | `r/le` | passed (`lostLoginMoved`): moved in the standard layout |
| `lost-login-expired-plain-issuer` | `r/lp` | passed (`lostLoginMovedOverPlainIssuer`) |
| `lost-login-invalidated` | `r/liv` | passed, veto `revoked` |
| `lost-login-reused` | `r/lru` | passed, veto `exhausted` |
| `lost-login-invalid-grant` | `r/lig` | passed, veto `other` |
| `lost-login-unrecognised` | `r/lun` | passed, veto `other` |
| `lost-login-no-word` | `r/lnw` | passed, no veto, policy without the word |
| `weekly-quota` | `r/wq` | passed; `complete` answered `completed` |
| `weekly-quota-exhausted` | `r/wqx` | passed; no move, so no `complete` |
| `five-hour-wall-soon` | `r/fs` | passed; waited out on its own account, so no `complete` |
| `five-hour-wall-far` | `r/ff` | passed; `complete` answered `completed` |
| `five-hour-wall-far-unnamed` | `r/ffu` | passed, in its first run against a host; `complete` answered `completed` |
| `weekly-quota-chain` | `r/wqc` | passed; `complete` for the second move only |
| `weekly-quota-rollback` | `r/wqr` | passed; rolled back, so no `consume` and no `complete` |
| `lost-consume-ack --quota` | `r/lcq` | passed; `complete` after the retried `consume` |

The last four are the other cases of `renewal_follow.py` that move a
session at a usage wall and so can reach the `complete` step. Run plainly,
the file's fault cases move at a login wall, where codex-alab sends no
report. With `--quota` they get distinct accounts and a usage wall
instead. One was run that way: in `lost-consume-ack --quota` the host drops
the first `consume` acknowledgment, so the report can only follow a
confirmation that comes later. The other fault cases were not run with
`--quota`. No case failed, so none was run again.

**The lost-login move in the standard layout.** In `r/le` both token
requests arrived at the fixture's one TLS listener. codex-alab presented the
source's refresh token once and the host presented it once, and each was
refused `refresh_token_expired`. No handshake was refused and no request was
unrouted. Seconds after the first 401, beside the plain-issuer run:

| | `le` | `lp` |
| --- | ---: | ---: |
| codex-alab's refresh refused | 0.25 | 0.27 |
| first `codex-login-rotation prepare` | 5.30 | 0.31 |
| host's own refresh refused | 5.32 | 0.33 |
| host confirms the source (two usage reads, both 401) | 65.72 | 60.60 |
| host reads the other account, once | 65.77 | 60.65 |
| `ready` | 70.79 | 65.64 |
| successor's turn answered on the other account | 79.93 | 74.94 |
| the run stopped watching | 151.15 | 146.14 |

`ready` came 65.47 s (`le`) and 65.31 s (`lp`) after the host's own
refusal. Each run had 14 asks, 13 `waiting` and then `ready`, between 5.019
and 5.088 s apart, so the doubled first ask of `ll/p1` is gone. The watch
ran 80.36 and 80.50 s past `ready`, and in that time no token was presented
and no ask was made. Other figures:

- The host read the source's usage three times: once just before it
  presented the token, and twice to confirm. It read the other account
  once. The successor read the other account four times.
- The receipt's purpose is `login_rotation`, and it names two accounts.
- The pool's `active` is still `main`. No rotation was charged, and no
  canary was set.
- Both login files are unchanged, and one recovered turn was sent.
- No `complete` line was traced. C1 sends none for a lost login's move.

The whole of `le` took 164 s and `lp` took 150 s, inside the 270 s budget.
In `le` the first ask came one five-second pass later than in `lp`, so the
whole move came 5 s later. Its oracle bounds the first ask at 30 s
(`LOGIN_ASK_SECONDS`).

**The lost logins that stay.** In each of the five runs:

- codex-alab's refresh was refused 0.21 to 0.29 s after the first 401, at
  the fixture's TLS listener.
- Two `codex-renewal prepare` asks came 60.02 to 60.03 s apart, both
  answered `waiting`. The first came 0.04 s (`liv`, `lig`, `lnw`) or 5.05 s
  (`lru`, `lun`) after that refusal.
- The host presented no token and read no usage, and no handshake was
  refused.
- No `codex-login-rotation` or `codex-rotation` line was traced.
- The wall was held for 70.02 to 70.09 s, and each run took 73 s.

**The usage walls and `complete`.** In `wq`, `ff`, `ffu`, `lcq` and the
second move of `wqc`, the host traced one `codex-rotation complete` 67 to
129 ms after the last `consume`, each answered `OK codex-rotation
state=completed` with the receipt's id. The pool's ledger then records
`proven` for the destination, and its `state.json` holds `proven: true` and
`canary: null`. In `lcq` the first `consume` came at 14.98 s and its
acknowledgment was dropped. codex-alab read `status` (`consumed`) at
16.03 s, sent `consume` again at 16.06 s and sent `complete` once, at
16.12 s. Which of C2's three paths sent it is not in the run's files:
codex-alab's journal is not kept there. In `wq`, for example:

- the first 429 came at 0;
- the earned-reset read at 0.13 s;
- two asks at 0.27 and 0.41 s, both `waiting`;
- `ready` at 5.46 s;
- `committed` at 9.28 s;
- the successor's turn answered at 14.67 s;
- `consume` at 14.75 s;
- `complete` at 14.82 s.

In `wqc`, the first move's resumed turn was refused at the second account's
own wall at 14.26 s. No report was sent for that receipt, as C2 specifies
for a resumed turn that ended with an error. The pool holds the third
account proven and the walled second never proven (`previous_proven:
false`). `wqr` ends with its receipt `retired` at 70.76 s and no `consume`.
Its pool's `active` stays the destination under a canary naming that
receipt, with `proven: false`, as `trial-81ec56a/r/wqr` left it on
2026-11-13. `ffu`'s refusal named no reset (`refusalNamedReset: false`).
The host first asked 0.45 s after it and answered `ready` at 5.80 s, as
`ff` did for a refusal that named one (0.31 s and 5.52 s).

**A finding for S1, the shared-pool smoke (gap 5).** `renewal_follow.py`'s
validators do not take `consume` to be the last rotation verb, and nothing
failed on `complete`. Nor do they judge it:

- No proof reads a `complete` line, its answer, or the pool's `proven` and
  `canary` after a usage-wall move. A run whose report was never sent, or
  was refused, would pass these cases as these runs did.
- The loop stops the host once it sees the consumed receipt and the
  successor at rest, and it does not wait for the report. In these runs the
  report happened to reach the host before it was stopped.

S1 needs a proof of its own:

- the line;
- its `completed` answer naming the receipt and the walled turn;
- the pool proven with no canary;
- a wait for the report, bounded by C2's 5 s.

It also needs the follower walled on the same account that gap 5 is about.
The controller was not changed here.

Since then, `314f0ccd89` has changed it:

- **The loop waits for the report.** It no longer stops a usage-wall host
  when the successor comes to rest. It waits up to 10 s
  (`COMPLETION_WAIT_SECONDS`) for `complete` to be answered `completed`.
- **Weekly-quota moves are judged on it.** Each weekly-quota move with a
  native receipt records `completionReported` (`completion_verdict`). That
  requires:
  - the report after the receipt's last consume;
  - only `waiting` before the report, and nothing after it;
  - the answer naming the receipt, its thread and the walled turn;
  - the pool proven once on its active account, with no canary held or
    taken.

  A host without A1, or a package without C2, now fails these cases by
  name. `s/wq` (`--case weekly-quota`, trial) passed with the report
  answered 0.109 s after the consume.

The shared-pool proof is `tui_shared_pool.py` (S1, gap 5).

What this shows, and what it does not:

- `lost-login-expired` moves in the standard layout with a host that holds
  `2e7aa348a76`. The fixture gap found on 2026-11-14 is closed by aterm.
  The case passed unchanged: the oracle that failed `ll/t1` passed `r/le`.
  That is one trial run with an unverified package.
- With the one-ask client, the four refusals that must stay a stop for a
  person, and a host without the word, were neither moved nor asked about
  in one run each. codex-alab asked once a pass in all seven lost-login
  runs.
- aterm's `codex-rotation complete` answered codex-alab's report in every
  usage-wall move whose resumed turn was served, after a dropped `consume`
  acknowledgment too, and it was sent once per receipt. The report proved
  the destination in a pool of one session. Whether a follower walled on the
  same account then moves at once, which is gap 5's point, was not run: no
  case of this file has a follower. `tui_shared_pool.py` ran it afterwards
  (next section).
- `five-hour-wall-far-unnamed` passed its first run against a host, the
  first evidence beyond the repo's fakes that a far wall whose 429 names no
  reset is offered on its usage read alone (gap 6).
- The six usage-limit cases that were run here before (2026-11-13, with
  `trial-81ec56a/pkg`) pass with this package and host. No change since
  then was seen to break them, in one run each.
- Not run with this pair:
  - `happy` and every same-account case;
  - the fault cases with `--quota`, other than `lost-consume-ack`;
  - a report from a successor restarted after `consume`;
  - the exec smokes;
  - any case more than once.

  One run a case measures no failure rate. None of these runs counts toward
  `renewal_matrix.py`.

### The shared-pool follow, trial runs of 2026-11-15

These are trial evidence and qualify nothing. The inputs are the same
trial host and package as the runs above: `host-7540b6d94f24` and
`pkg-cdfe65abe3`. Every `tui_shared_pool.py` result says `qualifies:
false` and `package.qualifies: false`. `s/wq` is a `renewal_follow.py`
result, which has no top-level `qualifies`; it says `package.qualifies:
false`. Every result also says `nativeHost.qualifies: true`: that is the
host record's own rule, not what these runs are worth (the host entry
above). Every account, token and reply was generated under the run's
output directory and served from loopback. The runs went one at a time,
with three sessions each, under
`$HOME/.aterm/.ubd/codex-rotation-2026-11-15/`. Times are seconds after the
first wall.

The controllers, each named by its result's `controllerSha256`:

- `s/p2`, `s/l1` and `s/c1` ran `alab/smoke/tui_shared_pool.py` at
  `314f0ccd89`, sha256 `1fab6fe248f5f6d0…`. `0a9570334b` changed only its
  docstring after them.
- `s/p1` ran an uncommitted first draft, sha256 `e6b31b47b602cdac…`, which
  no commit holds.
- `s2/p`, `s2/l` and `s2/c` ran it at `6e0631276d`, sha256
  `f8e2b252d0ac678a…`, after the review's repairs to its oracles (below).

| Run | Controller | Mode | Verdict | Walls spread | Canary ready / report | Followers' commit taken after the report |
| --- | --- | --- | --- | ---: | --- | --- |
| `s/p2` | `314f0ccd89` | follow | passed | 0.74 ms | 0.71 s / 9.84 s | 1.18 s, 6.34 s |
| `s/l1` | `314f0ccd89` | lost-consume-ack | passed | 0.54 ms | 5.49 s / 16.41 s (2 consumes) | 4.52 s, 4.79 s |
| `s/c1` | `314f0ccd89` | control, 120 s watch | passed | 0.72 ms | 5.74 s / none taken | none in 137.5 s after the canary's ready |
| `s2/p` | `6e0631276d` | follow | passed | 0.98 ms | 0.74 s / 10.39 s | 0.64 s, 0.83 s |
| `s2/l` | `6e0631276d` | lost-consume-ack | passed | 5.14 ms | 5.60 s / 16.80 s (2 consumes) | 4.85 s, 10.00 s |
| `s2/c` | `6e0631276d` | control, 120 s watch | passed | 1.16 ms | 5.60 s / none taken | none in 137.4 s after the canary's ready |

In these six runs, the guardian and each of the three hosts exited 0. In
`s/p1` the three hosts exited 0 and the guardian returned 1, the
controller's own exit for its failed verdict. Each session's walled input
was resumed unchanged on B, except the control's followers, which never
moved.

- **`s/p2`.** Session 0 was the canary, and its commit was taken at
  0.86 s. The followers were turned away differently:
  - Session 2 was answered `ready` at 5.64 s, and that commit was
    `retired`. Its next ask was taken at 11.02 s.
  - Session 1 was told `waiting` four times before the report, two of
    them after the canary's `ready`, and twice more just after the report,
    with the pool proven. It was taken at 16.18 s. Nothing shows the
    canary held it: the host answers `waiting` while a follower's own read
    is pending too, as those two after the report show. What ties its
    move to the report is the order: taken 6.34 s after it.

  The ledger has one `rotate` (the canary's), one `proven` 9 s later, and
  no `canary:` row. The pool's state ends `active: target`, `proven:
  true`, `canary: null`, `rotations: 1`. Each successor's own report was
  answered `completed`.
- **`s/l1`.** The canary's first consume ack was dropped and released at
  once. codex-alab asked again, and the report followed the second consume
  by 0.08 s. The followers had 3 and 2 commits retired before it.
- **`s/c1`.** The canary's successor resumed its walled turn, which was
  served on B at 15.21 s, and then consumed its receipt at 15.30 s. The
  host dropped and paused that consume's acknowledgment. The controller
  took the destination's `auth.json.lock` at 15.37 s and let the ack go.
  codex-alab read the receipt's status (`consumed`) at 16.48 s and
  consumed again at 16.53 s. It then sent 20 reports from 16.54 s, over
  4.97 s, all answered exactly `OK codex-rotation state=waiting`, and none
  after. The lock was let go at 23.13 s, 7.75 s after it was taken. So the
  lock covered the consume's retry and the reports; the turn had been
  served before it, with nothing held. The run then watched for 120.09 s.
  Over that time the two followers were turned away 41 and 39 times, the
  last 4.7 s before the end. Of those, 5 and 6 were a `commit` answered
  `retired`, and the rest a `prepare` answered `waiting`. After each
  follower's last `ready` (46.58 s and 51.65 s), and the commit and status
  read that followed it, every ask to the end was a `prepare` answered a
  bare `waiting`. Every `prepare`, `commit` and `status` of theirs after
  the canary's `ready` was answered `waiting`, `ready` or `retired`
  (`otherAnswers` is empty, and the repaired oracle passes the run's
  records). No follower's commit was taken, and nothing proved the pool.
  The ledger ends at the canary's `rotate`, with no `proven` row and no
  `canary:` row. The state ends `proven: false`, with the canary hold
  naming the canary's sid and 461.7 s left.
- **`s/p1`** is not counted. Under the smoke's first verdict, which took a
  follower's first `ready` for its release, it failed: `follower 0 was
  answered ready 9.755 s before the canary's report`. Its trace is what
  showed the retire-at-commit hold described above. Judged again by the
  committed verdict, at `314f0ccd89` and at `6e0631276d`, its records
  pass, with request-arrival stamps standing in for the refusal stamps
  that run did not record.
- **`s/wq`.** This is `renewal_follow.py --case weekly-quota` at
  `314f0ccd89`, and it passed. Its `completionReported` puts the report
  0.109 s after the consume, with the pool proven once.

A review of `314f0ccd89` found the oracles weaker than this section
claimed, and `6e0631276d` repaired them:

- The control recorded each follower's answers other than `waiting`,
  `ready` and `retired` and never failed on them. A follower refused for
  most of the watch, by an after-effect of the custody lock or anything
  else, passed if it was turned away once in the last 15 s. Now every
  `prepare`, `commit` and `status` of a follower, from the canary's `ready`
  to the watch's end, must be answered `waiting`, `ready` or `retired`, and
  any other answer or verb fails by name.
- A `commit` answered a bare `OK codex-rotation state=waiting`, naming no
  receipt, counted as taken. aterm answers it when `commit_pool` refuses
  the selection and the receipt cannot be retired, which takes nothing. Now
  a commit is taken only when its answer names the receipt. None of the
  runs here had such an answer: each of the 17 `commit`s answered
  `waiting` in their traces names its receipt.
- The follow's check that a follower was turned away before the report
  failed as "never held behind the canary". It shows the follower asked
  and was not moved, not what held it (`s/p2`'s session 1, above), and it
  now says so.

Judged again from their traces by the repaired oracles, `s/p2`, `s/l1`
and `s/c1` still pass, and so do `s/p1`'s records. The smoke was then run
again in each mode:

- **`s2/p`.** Session 0 was the canary, its commit taken at 0.90 s and its
  report answered `completed` at 10.39 s, 0.10 s after its consume. Each
  follower was answered `ready` at 5.6 s and 5.7 s and that commit
  `retired`. Their next commits were taken at 11.21 s and 11.03 s.
- **`s2/l`.** Session 2 was the canary. Its first consume ack was dropped
  and released at once. codex-alab asked again, and the report followed
  the second consume by 0.14 s. Each follower had 3 commits retired before
  it.
- **`s2/c`.** Session 1 was the canary. Its resumed turn was served on B
  at 15.11 s, it consumed at 15.22 s, and the controller took the lock at
  15.24 s. codex-alab read the receipt's status (`consumed`) at 16.31 s,
  consumed again at 16.32 s and sent 19 reports from 16.33 s, over 4.79 s,
  all answered `waiting`. The lock was let go at 22.89 s, after 7.64 s,
  and the run watched for 120.07 s. From the canary's `ready` to the
  watch's end, follower 0's `prepare` was answered `waiting` 39 times and
  `ready` 6 times, its `commit` `retired` 6 times and its `status`
  `retired` 6 times. Follower 2's were answered 38, 7, 7 and 7 times.
  Nothing else: no refusal and no commit taken. After each follower's last
  `ready` (47.07 s and 46.89 s), and the commit and status read that
  followed it, every ask to the end was a `prepare` answered a bare
  `waiting`. The ledger ends at the canary's `rotate`, with no `proven`
  row and no `canary:` row. The state ends `proven: false`, with the
  canary hold naming the canary's sid and 461.6 s left.

What this shows:

- A1, A2 and C2 together release followers walled on the same account
  within seconds of the canary's report: 0.64 to 10.00 s in the four
  follow and lost-ack runs.
- With only the report withheld, no follower was moved or refused for
  120 s in either control, with the pool unproven and the canary hold
  standing. The report is the difference, and that is the gap 5 change.
- Being turned away before the report shows that a follower asked then and
  was not moved. It does not show what held it. A bare `waiting` names no
  cause, and after each follower's last `ready` in the controls every
  answer was one. What ties the follow runs' moves to the report is their
  order against it, and the controls.

What it does not show:

- that the followers would wait until the hold's lapse at 600 s, or the
  lapse itself;
- more than three sessions;
- more than two runs of a mode;
- a host or package that qualifies;
- a live provider.

### Running a case

```sh
python3.14 -B <wt>/alab/smoke/renewal_follow.py --host <host> --package <pkg> \
  --output <out> --case <case>
```

`<out>` must not exist. It receives `result.json`, `requests.json`,
`http-events.json`, `unrouted-posts.json`, `host-stdout.jsonl`,
`host-stderr.txt` and `renewal-control-trace.jsonl`; the guardian's verdict is
the sibling `<out>-guardian-result.json`. `result.json` names the package
source and what the package's own record is worth (`package`), the host
record, and the sha256 of the controller, of both fixture modules and of
`package_standing.py`. Do not pass `-I`: the controller imports its
siblings. Keep `<out>`
short: the host puts its control socket under it and exits with `path must be
shorter than SUN_LEN` when `<out>/control.sock` exceeds 103 bytes (seen with
a 93-byte output directory). Budgets are in
`alab/smoke/renewal-case-budgets.md`: 270/300 s for `weekly-quota`,
`weekly-quota-exhausted`, `five-hour-wall-soon`, `five-hour-wall-far` and the
two lost-login cases that must move, 180/210 s for the five lost-login cases
that move nothing, 450/480 s for the chain and rollback cases.

The shared-pool follow (gap 5, S1) runs the same way, one run at a time:

```sh
python3.14 -B <wt>/alab/smoke/tui_shared_pool.py --host <host> --package <pkg> \
  --output <out> [--sessions 3] [--control [--control-watch 120] | --lost-consume-ack]
```

Each session's host has its own root, `<out>/w<i>`. That root holds its
fixture, control trace, stdout, stderr and PTY log, and its control socket,
so `<out>/w0/control.sock` must stay under 103 bytes. The shared pool is
`<out>/host-state`. The budgets are `BUDGETS` in the controller: 270/300 s
for `follow` and `--lost-consume-ack`. The control gets 150/180 s plus its
watch, which runs 30 to 420 s so that it ends well short of the 600 s hold.

A case that reaches the hang detector says what it saw: the host's
live-snapshot outcomes and, for a quota case, what followed the first 429
(codex usage reads, codex credit-list reads, prepares traced) and which of
four causes that points to; for a lost-login case, what followed the first
401 (token requests and usage reads by sender and status, the
`codex-login-rotation` and `codex-renewal` calls traced, and any TLS
handshake a client did not complete).

### Results of 2026-11-12

All ran against the host record above and package source `49f3c2ba233b`, and
are trial evidence only. Paths are under
`$HOME/.aterm/.ubd/codex-rotation-2026-11-12/`; the commit is the smoke tree
that ran, and the hash is the first 16 hex digits of `controllerSha256`.

| Result | Command | Smoke tree | Outcome |
| --- | --- | --- | --- |
| `trial-01-step1-weekly-quota-oldpkg` | `--case weekly-quota` | `c9e9fb3a32`, `2ec949df9e0b22b7` | failed at the detector: 13 usage reads, no credit read: the old fixture's reply did not decode |
| `t09-wq-step5` | `--case weekly-quota` | `d91b6f91ab`, `5d4404f589b0b2bb` | failed at the detector, as the negative control must: 61 codex usage reads, 9 codex credit-list reads, no prepare; `unrouted-posts.json` empty |
| `t11-wq-step6` | `--case weekly-quota` | `054b78fdb1`, `74810eeec9999fd4` | failed at the detector the same way: 50 usage reads, 9 credit-list reads, no prepare |
| `trial-03-step3-happy-oldpkg` | `--case happy`, host named `native_host` | `fcb210a427` before its amendment, `cafeadd65c7e7985` | failed at the detector: live snapshot `terminal`, `renew` never published |
| `trial-04-step3-happy-oldpkg-aterm-name` | `--case happy` | `fcb210a427` before its amendment, `cafeadd65c7e7985` | failed at the detector: snapshot published, prepare ready, source signalled at 122.0 s and exited `signal:15`, attach refused twice (126.0 s, 182.3 s), receipt left `committing`; then 1088 `resume` actions from 126.9 s to 269.9 s, 877 answered ok and 211 refused |
| `t15-happy-step7` | `--case happy` | `6f6211d0b1`, `ec95469941359264` | failed at the same point as `trial-04`: signalled at 121.9 s, attach refused twice (124.2 s, 182.1 s), receipt left `committing`; then 1099 `resume` actions from 125.0 s to 270.1 s, 891 ok and 208 refused |
| `t15-happy-step8` | `--case happy` | `09cd86ba9d`, `cd980bb6a9340264` | failed at the detector with the receipt left `committing`, but not as the two runs above: signalled at 61.9 s, attach refused three times (64.2 s, 122.2 s, 242.3 s), one `threads-ambiguous` snapshot; then 1572 `resume` actions from 65.0 s to 270.1 s, 1108 ok and 464 refused |
| `t16-5h-soon-step9` | `--case five-hour-wall-soon` | `2d19fd2e13`, `1d38329080bd8b6c` | passed: refused at 0.8 s, one walled usage read, retried and answered 30.33 s later, three requests, no prepare traced |
| `trial-05-exec-accounts-baseline-oldpkg` | `exec_accounts.py` | `ab9f144248` | passed, 3 requests; the exec fixture's old reply |
| `trial-06-exec-accounts-step4-oldpkg` | `exec_accounts.py` | `dc63e73112` | passed, 3 requests; the client now reads its credit list after each usage read |
| `t07b-exec-chain`, `t10-exec-step6-chain` | `exec_accounts.py --chain` | `dc63e73112`, `054b78fdb1` | passed, 4 requests |
| `t08-exec-step5`, `t10-exec-step6`, `t12-exec-step7`, `t14-exec-step8` | `exec_accounts.py` | `d91b6f91ab`, `054b78fdb1`, `6f6211d0b1`, `09cd86ba9d` | passed, 3 requests each; the last with the per-login owner ids since withdrawn |
| `t13-parallel-logins-step7` | `parallel_logins.py --package <pkg> --output <out> --count 40` | `6f6211d0b1` | passed: 40 concurrent requests, credential files unchanged |
| `t17-weekly-reset` | `python3.14 -I -B alab/smoke/weekly_reset.py --package <pkg> --output <out>` | `2d19fd2e13` | passed, 27 of 27 cases; this suite has its own fixture |

The exec reports name the host record but not their controller or fixture
hashes; the commit above is what identifies them. The two `trial-0[34]` runs
used the controller as it was before `b98f89d549` was amended to exempt the
exec fixture from the reply check (`checked_replies`); the TUI path is the
same in both.

Runs made after the review of this work, with the same image and package.
`f1-5h` used the schema-2 record; the others used the trial record
`host-trial-s3/build.json`, and their reports say `"attestation": "trial",
"qualifies": false`.

| Result | Command | Smoke tree | Outcome |
| --- | --- | --- | --- |
| `f1-5h` | `--case five-hour-wall-soon` | `3c3f812923`, `7639ccc0d7e2ea54` | passed: one walled usage read, retried and answered 30.27 s after the refusal, three foreground requests and one title request, no prepare traced, `renew` never published |
| `f2-exec` | `exec_accounts.py` | `b7020b0107` | passed, 3 requests; the host read the source's usage (not allowed), then the target's (allowed) |
| `f3-5h` | `--case five-hour-wall-soon` | `21ddd4d585`, `fc2dabd5c09117d1` | passed, with the 429 built by `provider_fixture.refusal`: its headers stated the five-hour window at 100% and the week at 10%; retried and answered 30.28 s later, three foreground requests, no prepare traced |
| `f4-exec` | `exec_accounts.py` | `21ddd4d585` | passed, 3 requests, the same order of reads as `f2-exec` |
| `f5-weekly` | `--case weekly-quota` | `21ddd4d585`, `fc2dabd5c09117d1` | failed at the detector as `t09` and `t11` did: 51 codex usage reads after the 429, 9 codex credit-list reads, no prepare; no fixture fault; the 429's headers stated the week at 100% |

`renewal_matrix.py --case weekly-quota` against the trial record exits 1
before any case with `the native host record qualifies no run: its
attestation is `trial``.

What the runs show:

- The negative control holds. With a package that cannot hand a walled turn
  over, `weekly-quota` fails at the detector with codex credit-list reads and
  no prepare. Before the fixture was corrected the same run made no credit
  read at all. It still holds with the refusal built by `provider_fixture.py`
  (`f5-weekly`).
- `five-hour-wall-soon` passes with this package (`t16`, `f1-5h`, `f3-5h`).
  Since the review its other account reads usable throughout and answers a
  model request at once, so a turn that waits its wall out had somewhere to go
  and did not; the controller no longer publishes `renew` in this case, and it
  fails the run by name, while it runs and again after its loop, when a
  charged rotation, a handoff receipt, a native action or a model request from
  another login appears. No run exercised that failure: package `2026111101` hands no
  five-hour wall over, so it is covered by
  `alab/tests/test_rotation_smoke_runs.py` against a stand-in host only.
- `happy` does not pass with this package against this host: the source ends
  by signal 15 and the host refuses both the forward and the rollback launch's
  attach. What follows is the outcome rotation must never have. No successor
  attaches, so the session has ended with nobody resuming it, and the host
  then issues `resume` without bound: 7.6 to 7.7 actions a second in all three
  runs (`native-actions.jsonl`), from the first refused attach until the
  detector stopped the run. Each run shows the same cycle: about 57 s of
  `resume` answered ok on the signalled operation, a refused attach, the same
  again, and then `resume` on a second operation id, refused; the run
  signalled a minute earlier went one step further, to a refused attach and
  then accepted `resume` on that second operation. Nothing in the run ends it
  but the detector. On 2026-11-12 a reading of aterm's source named two
  causes. An independent check upheld both, but no run has reproduced them:
  - The refused attaches are the fixture's. aterm-gui's renewal fixture
    (`native_renewal_fixture.rs`, which `native_host` serves) admits a renewal
    Attach against a source registry that it never publishes, so it refuses
    every attach. `happy` cannot pass against a `native_host` built before
    that is fixed.
  - The unbounded `resume` is aterm's own. The renewal drive
    (`drive_connection`, aterm-agent `codex_renewal.rs`) resends `resume`
    every 100 ms until each operation's 120 s deadline. The renewal service
    (`run_closed`, aterm-gui `codex_renewal_service.rs`) then appends one
    continuation after another with no cap. That matches the cycle above: an
    operation, a refused attach, the next operation.

  aterm branch `s51fb4f/renewal-bounded-resume` fixes both: one `resume` per
  phase, polled with backoff, and a renewal that ends once when its successor
  never attaches. Its first review found a blocker in that branch: a
  connection-scoped source registry that the app's own renewal path does not
  set. The branch has not landed. Until it lands and `happy` passes against a
  `native_host` built from that aterm main, `happy` is not qualified: a
  source ended by the host must be followed by an attached successor, and
  `resume` must stop.
- Whether the refused attach is the old package or the unproven host is not
  determined. It was the same with and without the JWT access tokens
  (`trial-04` against `t15-happy-step7`: the signal at 122 s, two refused
  attaches, 211 and 208 refused `resume` actions). It was not the same with
  the per-login owner ids (`t15-happy-step8`): the source was signalled at
  61.9 s, one 60 s poll earlier, attach was refused three times, 464 `resume`
  actions were refused, and the host traced a `threads-ambiguous` live
  snapshot that neither other run has. The extra refused attach and the larger
  counts are what one more 60 s cycle before the detector produces; why the
  signal came a poll earlier, and the ambiguous snapshot, are not explained.
  One run on each side decides nothing, so the owner-id commit was reverted
  rather than kept on reading alone.
- What the host's pool holds at a wall was not shown by any run; what follows
  is the evidence and where it stops.
  - Recorded: in the exec runs the host's first provider request comes after
    the first 429. It reads the source's usage, then one destination's; in the
    chain run its first read of the final account follows the second wall. In
    the TUI quota runs (`t09`, `t11`, `t16`) the host made no provider request
    in 270 s and `host-state/accounts-codex/state.json` held no account entry.
    That the host reads usage only when asked to move a session is inferred
    from this order of events, not observed as a rule.
  - Not exercised: package `2026111101` never sends `codex-accounts snapshot`.
    No control trace of any run holds that verb, so no run tested what the
    snapshot shows or what the client decides from it.
  - By reading only: on the aterm rotation branch (`414e1ae381d`,
    `crates/aterm-gui/src/codex_accounts_control.rs`) `snapshot_in` builds its
    rows from `peek_account_records`, a read of the roster, logins and state
    files that makes no provider request, and reports
    `assignment.account_id` as null. The client's
    `Snapshot::another_account_can_run`
    (`codex-rs/alab-aterm/src/accounts.rs`) then needs two rows that are
    enrolled, not held and `usable` or `unseen`. At a TUI turn's first wall
    the rows `main` and `target` would both be unseen, which is two.
  - Untested, and a likely place for a rotation-branch run to stop: a second
    wall. By then the host has sampled the source and the middle account, so
    in `weekly-quota-chain` only the final row is still able, which is one
    where two are required. A hang there is the pool gate, and this section
    does not rule it out.
  - Changed on branch `alab/account-move` (2026-11-12), by reading and fake
    host tests only: the gate now asks `codex-accounts snapshot schema=2`,
    which names the session's own row (`assignment.account_id`), and then one
    other able row is enough. An aterm that answers schema 2 with
    `ERR codex-accounts invalid request` is asked schema 1 once, and the gate
    keeps the two-row rule there. No run used it.

  The oracles of `weekly-quota`, `weekly-quota-exhausted` and
  `weekly-quota-chain` were left as written; none was run with a package that
  can end a walled turn.

### Results of 2026-11-13: the first rotation commit, and why it stops

On 2026-11-13 a package that hands a walled turn over was available: the
signed release 0.7.0, installed by atpkg at
`~/Library/Application Support/aterm/pkg/store/codex/2026111301` (source
`d75c9b4719`). It contains `c7641e9bad`, `4f63d6154a`, `064060139e` and
`4b4a67dc1d`, but not the account move (`05950d02ef`). `alab/package.sh`
still refuses on this Mac, for the reason above.

Three hosts were used, each with `--case weekly-quota`:
- `host-main-f78a392` was built from a clean detached worktree of aterm main
  `f78a392ed72`. It is recorded `built-from-clean-commit`: the first record
  that qualifies a run.
- `host-branch-7cacd8a` was built from the unlanded renewal branch head
  `7cacd8a7083`. It is a trial.
- `host-trial-s3` is the trial host above.

Paths are under `$HOME/.aterm/.ubd/codex-rotation-2026-11-13/`.

| Result | Host | Outcome |
| --- | --- | --- |
| `b-wq` | `host-main-f78a392` | Failed at the detector. Prepare was `ready` at 7.3 s and the commit was answered `waiting`; the ledger recorded `rotate main -> target`. The source was never ended: its record stayed `idle` with no exit until teardown (`signal:1`). No native action reached the host. `status` answered `waiting` until it was `retired` at 130.7 s, and 56 later prepares answered `waiting` |
| `r-wq` | `host-branch-7cacd8a` | Failed the same way |
| `o-wq` | `host-trial-s3` | Failed the same way |

This is the first time a rotation reached a commit in these smokes. In every
earlier `weekly-quota` run the package could not hand the turn over.

The cause was found by reading the source and the three runs' artifacts. An
independent refuter upheld it. It is in production code on both sides, so the
real app would stop the same way:

- codex-alab's Autopilot starts every follow tick with `ready_for_admission()`.
  That ends in `client.reconfirm()`, a same-holder `meta set supervisor`.
  - It does so after the commit as well: the first reconfirm came about 10 to
    20 ms after the commit's reply.
  - It does so although `renewal_follow.rs` already notes that a second
    readiness call after Ready would mutate the claim it sealed.
- aterm's `claim_supervisor_with` calls `set_supervisor` on a renewal, and
  `set_supervisor` raises `supervisor_revision` every time.
- The sealed rotation is bound to the revision at its prepare.
  - aterm's codex-renewal commit worker replays that prepare before it
    signals the source.
  - The replay is refused once the revision has moved
    (`capture_native_authority`, `Seal::admits`).
  - The worker retried about 700 times until its 120 s deadline and never
    signalled.
- The earlier `happy` runs did signal their source because their package
  reconfirmed rarely. The revision did not move between commit and signal.

The fix has two halves:
- aterm `s51fb4f/claim-renew-keeps-seal` (`2020f39c770` and two repairs): a
  same-holder renewal that changes nothing (the same holder, connection and
  proof, or only a longer `ttl=`) no longer moves the revision. It was not
  yet on aterm main when this was written.
- codex-alab, on main since `1eb3459fbe`: from the host's `ready` for a
  handoff until the receipt leaves the record, the Autopilot sends no
  same-holder `meta set supervisor`. The seal ends with the follow that kept
  its point, and at the latest 150 s on (aterm's 120 s restart deadline plus
  30 s). It is for aterms without the other half, 0.199.0 and 0.200.0 among
  them, and is to be dropped once every aterm codex-alab runs against carries it
  (`alab/CONTRACT.md` §5, "A sealed claim"). It is in no release: 0.8.0's
  source (`91f18dcf9f`) predates it.

Diagnosis: `~/Developer/.ubd/rotation-commit-diagnosis.json`.

#### Trial runs with aterm's half

The host was a trial: `cd315be8542`, an unlanded scratch merge of
`2020f39c770` into the renewal branch `s51fb4f/renewal-bounded-resume`
(`502d003b173`). The package was the unchanged signed 0.7.0, which still
reconfirms on every tick. So these runs test aterm's half alone.

| Case | Outcome |
| --- | --- |
| `weekly-quota` | Passed. The claim revision stayed at 1 from the source's first claim through every reconfirm, the prepare (6.6 s) and the commit (6.8 s). The source got SIGTERM 0.22 s after the commit. A successor on the destination login was resumed, attached and consumed. Same thread, original turn kept, 3 requests (2 before the move + 1) |
| `weekly-quota-chain` | Passed. Two hops (`main -> target -> target-1`), each signalled about 0.25 s after its commit, each successor consumed. Two receipts, two charges, 4 requests |
| `weekly-quota-exhausted` | Passed. Both accounts at 100% of the week: the prepare only ever answered `waiting`, nothing was committed and the original process was kept. This case does not reach the commit, so it says nothing about the race |
| `weekly-quota`, control | Failed like `b-wq`. Same tree with only the `set_supervisor` guard reverted: the revision went from 5 to 107, the receipt was retired at 130.7 s, and nothing was signalled. The pass comes from the fix, not from the 830 main commits the merge also brought in |

One run per case. `happy`, `weekly-quota-rollback` and `five-hour-wall-soon`
were not run on that host. codex-alab's half was not run live, since no
package containing it can be built here; it is tested against alab-aterm's
fake host only. None of these runs qualifies: that needs both aterm
branches on main and a `built-from-clean-commit` host. Full report:
`~/Developer/.ubd/rotation-sealed-claim-fix-result.json`.

### Qualifying runs of 2026-11-13

The host was built from a clean detached worktree of aterm main
`40e8515924021e18dd40b342390d8298a24fbb8c`. That commit carries the in-place
claim renewal (`40e85159240`), the bounded renewal resume (`cf70e391599`) and
the account move's rest check (`d20a22e9ed2`). The host is recorded as
`built-from-clean-commit`: sha256 `f24afd9959c69bad…`, 190411760 bytes,
self-run `PASS`. The package was the unchanged signed 0.7.0 (`d75c9b4719`),
so codex-alab's seal (`1eb3459fbe`) was not in play. Paths are under
`$HOME/.aterm/.ubd/codex-rotation-2026-11-13/`: the host record is
`host-q-40e8515/build.json` and the runs are under `q-40e8515/`. Every
`result.json` below says `nativeHost.qualifies: true`.

| Case | Run | Outcome |
| --- | --- | --- |
| `weekly-quota` | `wq` | Passed. One handoff: signal, attach, execute, resume. Same thread, original turn kept, completed plan kept, one quota charge, 3 requests (2 before the move + 1) |
| `weekly-quota-chain` | `wq-chain` | Passed. Two handoffs (two signal, attach, execute, resume runs), two charges, 4 requests |
| `weekly-quota-exhausted` | `wq-exhausted` | Passed. Both accounts at 100% of the week: nothing moved and the original process was kept, with no replayed request |
| `weekly-quota-rollback` | `wq-rollback` | Passed. Its first run. The failed forward move was rolled back and restored (`quotaRollbackRestored`), one charge, no replayed request |
| `five-hour-wall-soon` | `5h` | Passed. A five-hour wall that resets in ten minutes is waited out on the session's own account |
| `happy` | `m/happy` | Failed at the hang detector. This is the same-account login renewal, not a rotation. The source was signalled 0.22 s after its commit, and a successor started on the renewed login. The successor read models, credits and usage, but never sent the original turn again and never consumed its receipt. `status` answered `committed` until the end. Cause below |

The weekly cases were run one by one with `renewal_follow.py`.
`renewal_matrix.py` stops at its first failed case, and `happy` comes
first, so its `m/qualification.json` says `passed: false` having run only
`happy`. A matrix qualification of the rotation cases waited on the `happy`
fix, or on running the matrix without `happy`. It has since been run
without `happy`, on the later host, and passed ("The five-hour handoff,
2026-11-13"). A matrix that includes `happy` still waits on that fix.

None ran live, and none used codex-alab's seal.

#### Why `happy` fails

The cause is in codex-alab, in 0.7.0 and on main alike. It was found by
reading the run and the source, and an independent refuter upheld it
(`~/Developer/.ubd/happy-successor-diagnosis.json`).

- Before a successor reruns its turn, its admission check requires one shared
  counter, `Shared::upgrade_generation`, to stay unchanged.
- The TUI moves that counter on every app event and app-server event
  (`alab-tui` `upgrade::native_event`), whether or not anything changed. The
  reply to the TUI's own usage poll is one such event.
- A Login follow rechecks every 60 s, and the usage poll also runs every
  60 s. Both start with the process, so each recheck met a poll reply and
  was refused.
- The refusal is dropped without a log line.
- A weekly-limit successor rechecks after 5 s, before any poll reply, which
  is why the rotation cases pass with the same package.

In a real aterm window an unfocused pane polls less often, so collisions are
rarer there, but the defect is the same. The fix is on main since
`81ec56a7b9`: a refused admission is logged and retried within seconds, and
admissions get their own counter that only real changes move. No certified
package carries it, since none can be built here. With a trial package of
that commit `happy` passed once ("Trial runs of 2026-11-13"), which is trial
evidence and not a qualification.

#### Repeat runs

Each passing case was run twice more on the same host (`<run>-r2`,
`<run>-r3`), while heavy builds loaded the machine. 13 of the 15 runs
passed. Both failures were in round 3, and in both the product behaved
correctly. Each diagnosis was upheld by a refuter
(`~/Developer/.ubd/rotation-smoke-flakes-diagnosis.json`).

| Run | Error | Cause |
| --- | --- | --- |
| `wq-exhausted-r3` | `native host did not exit cleanly` | aterm's `native_host` fixture. At teardown it stopped pumping its event loop before its control server had joined. A status request arriving in that gap waited up to 60 s for the main thread, past the smoke's 15 s exit wait. The session had been kept, as the case requires. Fixed on aterm branch `s51fb4f/fixture-teardown` |
| `wq-rollback-r3` | `rollback lacks exact old operation, auth, image and completed phase proof` | The smoke harness. It took a retired receipt as proof that the ledger already held the native completion. aterm lets the successor's own `status` retire the receipt first. The native actions matched the passing runs. Fixed on codex-alab branch `alab/smoke-rollback-race`, which waits for the completion and fails fast on a terminal non-completion |

Both fixes have since landed: the fixture's teardown in aterm
`753cca585a4`, the smoke's wait in codex-alab `5cc21442b1`.

The repeats were then rerun on a host built from clean aterm main
`753cca585a42767cfc023e060db25c87085e3b59` (`host-q-753cca5/`, attested
`built-from-clean-commit`, sha256 `7d02fa3200695e35…`), with the smoke
harness at `5cc21442b1` and the same signed 0.7.0 package. Three rounds of
the five passing cases: 15 of 15 passed, and each run's guardian result
says it qualifies. The runs and `repeats.txt` are under `q-753cca5/`. A
Claude Code restart cut off `5h-r2` before it wrote a result; `5h-r2b` is
that run made again, and `5h-r2` left no result to drop.

Fifteen runs do not measure a failure rate: the first set's two failures
came only in its third round, under heavy builds, and the machine's load
during the rerun was not recorded. `happy` was not rerun, since the package
is the same.

After aterm main gained the lost-login move's verb (`7f1450f9dda`), a host
was built from a clean checkout of that commit (`host-q-7f1450f/`, attested
`built-from-clean-commit`, sha256 `955d0801d3fd6f25…`) and one
`renewal_matrix.py` run of the four weekly-quota cases and the two five-hour
cases was made on it with the same signed 0.7.0 package and the smoke
harness at `9a5dffd5c4`: all six passed (`q-7f1450f/m1/qualification.json`).
That is one run per case. It shows the usage-limit handoff still completes
on an aterm main that carries the new verb; it does not exercise the verb,
which 0.7.0 never asks for.

### The five-hour handoff, 2026-11-13

`five-hour-wall-far` is the case this file said was missing: a session
refused at a five-hour wall whose reset is far off is carried on by another
signed-in account. `five-hour-wall-soon` covers only the near reset, which is
waited out.

The fixture (`Model(wall="five-hour-far")`) refuses the source login's second
request on its five-hour window, two hours from its reset, with the week at
10%. From then on it refuses every model request of that login, its
thread-title request too, as a provider at that wall does. The other login
is the weekly cases': held until the controller publishes `renew`, then
usable. The weekly wall is never served, so only the five-hour rule can
explain a move.

As first written (`42688c7af4`) the fixture answered the walled login's
title request 200, 0.05 to 0.07 s after the 429, as the weekly cases and
`five-hour-wall-soon` still do. `r1` to `r3` and `m` below ran so. A review
of the case found that, and gaps in its oracle which the list below names;
`14f0f3a67c` repairs them, and `r4` to `r6` and `m2` are the runs that
count.

The handoff is judged by the `weekly-quota` oracle, unchanged: a consumed
receipt bound to the original thread and turn, to both processes' kernel
births and to two different accounts, with `old_auth` and `new_auth` the two
login files; the exact native operation completed without rollback; one
signal to the original process; the original turn completed by a successor
on the other login, with the completed plan and the user's input kept;
one rotation charged and no binding left. The receipt's purpose reads
`weekly_quota` here too. The rotation protocol has one purpose for both
walls, so the receipt does not say which wall was left. The case adds:

- The provider's record. The 429 stated the five-hour window at 100% on a
  week under 100%, with its reset beyond the host's bound plus 60 s. After
  it the walled login was answered no model request of any kind. No reading
  of the source ever stated its week spent. After the refusal every reading
  of it stated that wall, the walled turn and the host each read it, and the
  host read the other account as usable.
- The host's record. Its `harness policy` reply, asked over the run's own
  control socket, prints `codex_rotation=1 codex_rotation_five_hour=<n>`.
  Its pool ledger holds the source row at a `five_hour` wall alone, beside a
  sample with the five-hour window spent and the week not, and the pool
  moved from `main` to `target`. It answered `ready` for this receipt only
  after it had read the wall.
- The turn ended at its refusal. The host was first asked to move the
  session within 20 s of the 429 (`FAR_YIELD_SECONDS`). A turn that keeps
  its request reads its credits again 30 s later and only then may ask
  (`alab-quota-recovery`, `wait_for_reset`), so a first asking past 20 s is
  a session that waited in its turn, whether or not it is moved later. As
  first written the bound was 90 s and applied only while the host had
  never been asked: a session moved after 60 to 89 s in its turn would have
  passed. No run did that.
- One process on the walled login. The session processes the run's loop saw
  on the source login are exactly the one the receipt moved. A session
  record does not outlive the handoff (in `r1` the moved session's was gone
  14.6 s after the refusal), so the count is kept over the loop. As first
  written each reading counted only the records present at that moment, and
  the verdict's came after the moved session's was gone.
- No sign-in and no copied token. Nothing but model requests was posted, and
  neither fixture login file changed from what the controller wrote. Both
  files are required, by the receipt's naming of them: as first written an
  empty map would have passed.
- No pass without this proof. `passed` is refused unless the report holds
  the case's own record (`fiveHourWallHandedOver`;
  `renewal_follow.CASE_PROOFS`), and `renewal_matrix.py` counts no
  five-hour case without its record. A controller that lost its call of
  this oracle fails every run of the case. As first written it would have
  passed them on the weekly oracle alone.

It fails by name while it runs, without waiting for the hang detector: at
once when the session is put back on the login it is walled on (a receipt
whose destination is the source, or a second process on the source login),
when the walled login sends the turn's request again, when the host answers
`not-needed`, or when the host's policy prints no bound; and when the host
is first asked more than 20 s after the refusal, or has not been asked by
then. A refusal at the host's policy quotes what the reply says of
rotation, and the report keeps the reply (`hostFiveHourPolicy.reply`, its
first 4096 bytes) whether or not it was accepted.

Runs. The host was `host-q-753cca5/aterm-native-host`, the one the repeat
runs used: built from clean aterm main `753cca585a4`, attested
`built-from-clean-commit`, sha256 `7d02fa3200695e35…`. The package was the
unchanged signed 0.7.0 (`d75c9b4719`) at
`~/Library/Application Support/aterm/pkg/store/codex/2026111301`. Paths are
under `$HOME/.aterm/.ubd/codex-rotation-2026-11-13/q5f/`. `r4` to `r6` ran
the smoke tree `14f0f3a67c` (`controllerSha256` `339a8a258f6add75…`), and
each `result.json` says `nativeHost.qualifies: true` beside a guardian
result of 0. Times are from the 429.

| Run | Outcome |
| --- | --- |
| `r4` | Passed. The host was first asked at 0.40 s and answered `ERR codex-renewal wall changed`, 11 times in all; `waiting` at 31.01 s, `ready` at 36.20 s, `consumed` at 45.39 s. The successor on the other login answered the original turn at 45.26 s |
| `r5` | Passed. First asked at 0.40 s, 11 refusals, `waiting` at 31.00 s, `ready` at 36.21 s, `consumed` at 45.05 s, answered at 44.99 s |
| `r6` | Passed. First asked at 0.30 s, 11 refusals, `waiting` at 30.72 s, `ready` at 35.88 s, `consumed` at 44.91 s, answered at 44.85 s |
| `t1` | Passed, as `r4` did (first asked at 0.27 s, answered at 44.86 s). The first run with the title refused, made before `14f0f3a67c` was committed. Not counted |
| `c2` | A control, on the trial host `codex-rotation-2026-11-12/host-trial-s3`. Failed by name 0.59 s after the run's first request, as soon as the wall was served: `the host's policy prints no five-hour bound (codex_rotation_five_hour), so it hands no five-hour wall to another account; of rotation its reply says codex_rotation=1, and begins: OK gen=1 effective=on why=enabled contract=2,3 codex_rotation=1 enabled=true …`. Two foreground requests and a refused title, nothing moved, host exit 0 |

In each of `r4` to `r6`:
- The host's policy printed `codex_rotation=1 codex_rotation_five_hour=900`
  and `limit_wait=true`.
- The 429 stated the five-hour window at 100% and the week at 10%, with the
  reset 7200 s off. The session read that wall ten times and the host once.
- The host's ledger held `main` at a `five_hour` wall standing 7200 s from
  the refusal, with a sample of 100% and 10%, and one rotation charged.
- Three foreground requests were served (source 200, source 429, target
  200) and one title request, from the source 0.05 to 0.06 s after the 429
  and refused. Nothing else was posted, and both login files were unchanged.
- The loop saw one session process on the source login, the one the
  receipt names as moved.

What `c2` shows, and what it does not. Its `result.json` keeps the trial
host's reply: `codex_rotation=1` is followed directly by `enabled=true`,
where the qualifying host prints `codex_rotation_five_hour=900` between
them, and the controller, which reads the whole reply, found no such word.
So the cause of the refusal is in the run's own record: this host's policy
carries rotation without a five-hour bound. Why it does is not in the run. That
host's record says `TRIAL, provenance unproven`, handed over as built from
aterm `414e1ae381d`; that commit does not contain `2365b983b6c`, which
added the word, so a host built from it would print what this one printed.
`c1` was the same control with the controller as first written, which kept
nothing of the reply and said only that no bound was printed.

The refused title delays the handoff by about 30 s. This is the one
difference between `r1` to `r3` and `r4` to `r6`, and it was not looked for:

- The walled turn ends at the refusal in both sets. The session record's
  first reading after the 429 (0.6 to 0.7 s) says `nativeOrigin.phase:
  complete`, and the host is first asked at 0.27 to 0.45 s.
- With the title answered (`r1` to `r3`), that reading says `status: idle`.
  The host answers `waiting` at once and `ready` 5 s later, and the
  successor answers the turn 14.3 to 14.5 s after the 429.
- With the title refused (`r4` to `r6`), the record says `status: busy`
  until 29.7 to 29.9 s after the 429. aterm's `prepare` accepts only a
  record at `idle` or `waiting` (`Intent::matches_record`,
  `aterm-agent/src/harness/codex_renewal.rs` at `753cca585a4`) and answers
  `ERR codex-renewal wall changed` until then. The first `waiting` comes at
  30.7 to 31.0 s and the successor answers at 44.9 to 45.3 s.
- What holds the session busy for those 30 s was traced afterwards from the
  run records and the source, and an independent refuter upheld it with
  corrections
  (`~/Developer/.ubd/title-refusal-delay-diagnosis.json`).
  Nothing was built or run for it:
  - In 0.7.0 the TUI asks for the thread's title as a turn on a hidden
    ephemeral thread. That thread's session source is `Cli`, so codex-alab
    books its turn like a session's own, and the record says `busy` while
    it runs.
  - The title turn's 429 enters the same quota recovery as any turn. The
    recovery asks whether a handoff is ready, is told no because the title
    thread is not the primary thread, and sleeps on its 30 s poll. Left
    alone it would poll until the wall's reset.
  - The TUI's own 30 s timeout for such a turn interrupts it
    (`STRUCTURED_TURN_TIMEOUT`, `tui/src/temporary_structured_request.rs`).
    The record went idle 30.06 to 30.09 s after the title turn's id was
    minted, in `r4`, `r5`, `r6` and `t1`. No credit read was made near
    30 s, so the 30 s is that timeout and not the credit re-read first
    guessed here.
  - The session's own turn is not held: its handoff is ready, so its
    recovery returns at once and the turn ends on its usage error.
- It is a delay and not a failure: each run handed over, and codex-alab
  asks again every 5 s without a count limit. The hold is bounded only by
  the TUI's interrupt being delivered.
- Scope, from the code and not from a run:
  - It needs a wall met within 30 s of the first message of an unnamed
    thread, as when a session starts on a spent account. A later turn sends
    no title.
  - A weekly wall would show the same delay; the weekly cases' fixture
    still answers the title.
  - Main no longer sends the automatic title while ALab is active
    (`60156939fa`, after 0.7.0), so a package built from main does not show
    this delay, and `five-hour-wall-far` run with it would send no title
    for the fixture to refuse.
  - The mechanism remained on main for any other hidden helper turn (a
    rename suggestion, a manual recap) that is running when a wall is met,
    until the fix below.
  - A visible second thread, such as a side chat, is booked the same way,
    and its turn parked at the wall held the primary thread's handoff for
    the whole wall. That wait is fixed on `f3756a3cc6`, with the repairs
    its review asked for after it. It is read from the code and tested
    against the fakes, not run against aterm.
    - The autopilot takes a thread by its session source alone
      (`tui_threads`, `alab-autopilot/src/extension.rs`). A side chat is a
      fork that names no session source, so it takes the TUI's `Cli`
      (`tui/src/app/side.rs`, `effective_sources` in
      `core/src/thread_manager.rs`). Each such thread gets a `ThreadRt`,
      and each of its turns is booked as running and given a handoff
      question (`on_thread_start`, `on_turn_start`).
    - Showing a side chat does not make it the primary thread: only a
      started, resumed or forked session assigns one
      (`enqueue_primary_thread_session_with_presentation`,
      `tui/src/app/thread_routing.rs`), and the side chat's path does not
      call it. Ending a side chat's turn starts no move, so with no move
      under way its handoff still answers no, and its turn at a creditless
      usage wall still polls every 30 s for an earned reset. The record
      says `busy` while it does.
    - What was lost: aterm refuses the primary thread's handoff under a
      busy record, at `prepare`, in its preparation job, at `commit` and
      before its signal. No timeout interrupts a conversation's turn, so
      one side chat left open at the wall held the move off until the
      wall's reset.
    - What ends such a turn now: the primary thread's move being live.
      That is all of: the usage wall's intent (`usage-rotate@v1`) from this
      process's own login is on record, codex-alab acts under its claim, no
      upgrade is under way, neither thread is stopped, the side thread is
      not in the vanilla experience, and the last pass of the primary's
      ask kept its point and either sent the host `prepare` or holds the
      intent bound to the receipt the host granted
      (`session_leaves_account` and `follow_renewal`,
      `alab-autopilot/src/renewal_follow.rs`). A bound receipt counts
      whether or not that pass reached the host: its `commit` may have
      gone, and the host restarts the session only under a record at
      rest. The side turn's recovery asks this after a look that found no
      earned reset to redeem, and on a yes ends the turn on its own usage
      error. After a refused ask the primary asks again within 5 s, and an
      ask after the side turn's end no longer meets that turn's busy
      record. A credit earned meanwhile is redeemed at the look and the
      turn goes on.
    - How soon. A side turn that asks while the move is live ends at that
      look. One that still waits looks again only every 30 s, so it is
      woken: each of its looks leaves word that a turn waits as the look
      begins, before it reads anything (`walled_look_begins`, told by
      quota recovery through `WallHandoff::look_begins`), and the
      primary's next pass that leaves the move live, while codex-alab
      acts, sends the wake (`note_move_followed`). That holds whatever
      turned the answer to yes, such as the first ask, a hold lifted, or a
      claim taken again after an Autopilot Off (the wake follows the pass,
      not a change of its mark), and whatever the look did: it was told
      no, or it ended before it asked, as a look that finds the credit
      list unread does. The intent's arrival on the record wakes it too. A
      woken turn looks again (at least 1 s after its previous look began)
      and, with no earned reset found, asks again. A pass wakes nobody when
      no side turn's look began since the last wake, while codex-alab does
      not act, or for another handoff (a login's own renewal, a lost
      login's move): each wake costs every waiting turn a usage read and a
      credit list read.
    - An intent that only stands on record ends nothing. It stays there
      through an Autopilot Off, when a pass read a host whose policy no
      longer permits rotation (as aterm's reads `effective=off` while it
      folds a person's hold into it), and, for an intent no receipt binds,
      when a pass could not read the host. A side turn ended for it would
      give up its look for an earned reset with no move to follow. A lost
      login's move (`login-rotate@v1`) ends no turn either: this fix
      covers usage walls only.
    - A kept point no receipt binds ends nothing either, by itself. A pass
      of the ask keeps such a point and sends the host nothing whenever its
      checks before an act say wait (a person's hold that `status` shows
      while the policy still permits, a draft, a popup or queued input in
      the TUI, a person at the terminal, a `status` that cannot be read),
      while a turn runs on the primary thread, while the session's background
      terminals run (gap 1), and when a read or a write of its own failed.
      Nobody is asking for the move then, so the side turn
      keeps polling, and it is woken when the ask goes out again. The first
      build of this fix read "the pass kept its point" as "the move is live"
      and ended side turns under a hold (found in review, by a probe).
    - What still holds the record busy against the move, for as long as it
      lasts. None of these is ended, because each is a turn at work or a
      person's own input:
      - a side turn that is working (the model answering, a tool call
        running) or waiting on an approval;
      - a side turn retrying for another reason, such as the capacity
        retry or a connection retry;
      - input the person queued in the displayed side chat, which the TUI
        holds while its own rate-limit recovery is pending;
      - a recovery that cannot read the account's usage, which never
        reaches the handoff question, and one that cannot read the credit
        list, which asks only at a look that finds the list unread 30 s
        (a whole poll) or more after the first unread look of the run. A
        look the wake brings does not shorten that: it can come a second
        after the last, and counted as a poll it would end a turn over a
        two-second list outage with a credit behind it (`wait_for_reset`,
        `alab-quota-recovery/src/unix.rs`);
      - a five-hour wall the 429 did not name a reset for, which was never
        offered to a host when this fix landed. Since gap 6's fix
        (`986bb020b1`, "Known gaps" above) quota recovery offers the usage
        read's wall whatever the 429 named, so such a wall holds the record
        no longer;
      - a background terminal in any thread of the session (a dev server, a
        watch), for as long as it runs. The move itself waits for it (gap 1
        in "Known gaps, audit of 2026-11-14"): no walled turn ends for the
        move, a side turn's included, the ask is not sent, and the person is
        told once why.
    - Tests: `renewal_follow_handoff_tests.rs` (each condition of the rule,
      a kept point with and without a bound receipt, and which pass wakes)
      and `tests/autopilot/side_wall.rs`. The second starts a side thread
      `Cli`-sourced with a loopback usage backend. Eight of its ten cases
      first show the turn parked, with the written record at `busy`, and
      where an ask or a `commit` goes out meanwhile, that request refused by
      aterm's rule; then they show what releases it: parked before the
      primary asks, under a bound receipt's `commit`, with a credit earned
      meanwhile, with an intent that only stands on record (a host unread, a
      host that stopped permitting), with the ask stopped by a person's
      hold, through an Autopilot Off and On, and after looks that found the
      credit list unread and never asked. The other two cannot show
      it parked, because under the rule their turn never parks: a turn
      walled after the ask, and one walled under a bound receipt whose host
      cannot be read. For those the evidence that the turn would have parked
      is a run made by hand with the rule taken out (not in the tree), in
      which both fail on a turn that does not end.
- Fixed for the hidden helper turns on `332417b431`: the glue's
  `recover_weekly` (`alab-core/glue/responses_retry.rs`) returns at once
  for an interactive turn on an ephemeral thread whose source is `system`
  or `thread_title`, which is upstream's own test for such a thread. The
  helper's 429 then ends its turn, and no recovery holds the record busy.
  Every other thread keeps its recovery. A visible second thread's kept
  recovery ends while the primary thread's move is live, as above, and
  otherwise waits its wall out. A headless exec turn keeps
  its recovery whatever its thread is called (`codex exec --ephemeral
  --thread-source system`), since `46859710dd`: its usage error does not
  end it, so returning early only stopped its look for an earned reset
  (found in review; no caller passes exec those sources). It is unit-tested
  only (`alab-core/glue/weekly_compaction_tests.rs`). No smoke run shows
  it, because a build from main sends no automatic title for the fixture
  to refuse.
- Not determined: whether the real provider refuses the title request at a
  wall.

The earlier runs, with the fixture and oracle as first written (`42688c7af4`,
`controllerSha256` `b0d388e7dc6c7663…`), on the same host and package:

| Run | Outcome |
| --- | --- |
| `r1` | Passed. The host was first asked 0.34 s after the refusal and answered `waiting`; `ready` at 5.39 s, `consumed` at 14.54 s. The successor on the other login answered the original turn 14.46 s after the refusal |
| `r2` | Passed. First asked at 0.35 s, `ready` at 5.41 s, `consumed` at 14.44 s, answered at 14.35 s |
| `r3` | Passed. First asked at 0.32 s, `ready` at 5.39 s, `consumed` at 14.40 s, answered at 14.30 s |
| `x1` | Passed. The first run, made before the case was committed: its controller (`c6df75117f68e8ff…`) differs from the committed one in one variable's name |
| `c1` | The control on the trial host, as `c2`. Failed by name 0.75 s after the run's first request with the same sentence and no reply kept. Two foreground requests, nothing moved, host exit 0 |
| `m` | `renewal_matrix.py` from the committed tree `c6c67e410c` with the six cases below: exit 0, `passed: true` |

In `r1` to `r3` the walled login's title request was answered 200 after its
429, with no limit headers, and the session read its wall three times. None
of the repaired gaps changed their outcome: the first asking was inside
20 s, one process ran on the source login, both login files were held, and
each result carries `fiveHourWallHandedOver`.

The matrix was run from the committed tree `14f0f3a67c`, without `happy`:

```sh
python3.14 -B alab/smoke/renewal_matrix.py --host <host> --package <pkg> \
  --output <q5f>/m2 --case weekly-quota --case weekly-quota-rollback \
  --case weekly-quota-exhausted --case weekly-quota-chain \
  --case five-hour-wall-soon --case five-hour-wall-far
# exit 0
```

`m2/qualification.json` says `passed: true` with those six as
`requestedCases` and no error, and each case's result says `passed: true`
and `nativeHost.qualifies: true` beside a guardian result of 0. It also says
`allSixCasesPassed: false` and `allAvailableCasesPassed: false`: those two
fields are about the six same-account base cases and the whole of
`renewal_matrix.CASES`, neither of which was requested. Its `inputs` pin the
controller at `339a8a258f6add75…`, the matrix at `61e70c7e16e0ff4e…`, the
host at `7d02fa3200695e35…` and the package's codex at `6bd514a71736c899…`.
`five-hour-wall-soon` waited 30.49 s with no prepare traced.
`five-hour-wall-far` read as `r4` to `r6` did: first asked at 0.38 s, 11
refusals, `waiting` at 31.06 s, `ready` at 36.38 s, `consumed` at 45.40 s,
answered at 45.32 s. The matrix counted both five-hour cases on their own
records (`fiveHourWallWaitedOut`, `fiveHourWallHandedOver`). The earlier
`m`, from `c6c67e410c`, passed the same six with the fixture and oracle as
first written.

Both five-hour cases are now in `renewal_matrix.CASES`.
`five-hour-wall-soon` had passed six times on hosts built from a clean
commit with the signed 0.7.0 package before it was put in: once in
"Qualifying runs of 2026-11-13" (`q-40e8515/5h`), twice in the first set of
"Repeat runs" on the same host (`5h-r2`, `5h-r3`) and three times in the
rerun on `host-q-753cca5` (`q-753cca5/5h-r1`, `5h-r2b`, `5h-r3`). Commit
`b595115568`, which put it in, says four; the record holds six.
`five-hour-wall-far` was put in after `r1` to `r3` and stays in after `r4`
to `r6`.

Limits:
- Fixture only. No run was live: this Mac has one signed-in Codex account,
  and every account, credential and reply was generated inside the run's
  output directory.
- The fixture's 429 names the five-hour window's own reset. Whether the
  live backend's does is not determined
  (`alab/continuous-earned-resets.md`). If it names a reset more than 60 s
  from the usage read's, codex-alab keeps the turn polling and this handoff
  does not happen on a real account. (Since `986bb020b1`, in no release, that
  no longer decides the handoff: the wall is offered on the usage read alone
  and the ended turn takes the read's reset, gap 6 under "Known gaps, audit
  of 2026-11-14". `five-hour-wall-far-unnamed` serves a 429 that names no
  reset; it has not been run.)
- Not run in an installed aterm. 0.200.0 printed the bound but lacked the
  in-place claim renewal; 0.201.0, installed by 2026-11-14, prints it and
  carries the renewal by its tag's ancestry, as the table above says of the
  weekly handoff.
- A real session that waits in its turn was not observed failing the case.
  No package here keeps such a wall: 0.6.0 and 0.7.0 both hold `4b4a67dc1d`.
  That failure, a session moved only after it waited, the retried request,
  the declined handoff and the move to the same account are covered by unit
  tests on records (`renewal_follow_test`). Whole runs against the stand-in
  host cover a session that waits, a receipt that names the walled login,
  and a host whose policy has no bound. Only the last was also run for real
  (`c1`, `c2`).
- No whole run against the stand-in host passes the case. A pass needs two
  real session processes with kernel births, a live snapshot, a consumed
  receipt bound to both, the native ledger and signal effect, the pool
  ledger, a control trace and an answering control socket: a stand-in
  larger than the case. So no test reaches the controller's call of this
  case's verdict. What is tested is that a run without the verdict's record
  cannot pass.
- The count of processes on the walled login is taken at the loop's passes,
  a tenth of a second apart. A process that started and ended between two
  passes is not counted. Such a successor would still fail the run when it
  sent the turn's request on the walled login.
- One reset distance, two hours. The bound itself (900 s, and the 960 s
  codex-alab applies) is tested in Rust, not here.
- The 20 s that separates a turn that ended from one that waited rests on
  the 30 s poll in the 0.7.0 source. A product whose poll were shorter than
  20 s would need a shorter bound.
- Three counted passes and one matrix run, after four passes and a matrix
  run with the fixture as first written, measure no failure rate. The load
  average was 12.7 after `r3`, 21.6 after `m`, 15.7 after `c2` and 21.1
  after `m2`.

### Not run

- Any case with a trial package more than once, or any case other than the
  seven of "Trial runs of 2026-11-13". The exceptions are the lost-login
  cases of "Trial runs of the lost-login move", and the fifteen runs of
  "Trial runs against aterm `7540b6d94f2` and codex-alab `cdfe65abe3`,
  2026-11-15", each once with a second trial package.
- `weekly-quota-*` with a package that carries the seal (`1eb3459fbe`),
  against aterm 0.199.0, 0.200.0 or a host without `2020f39c770`: no such
  package can be built here.
- `lost-consume-ack` and every other same-account fault case. Only
  `lost-consume-ack --quota`, with distinct accounts, ran once on
  2026-11-15.
- `exec_shared_pool.py`, `exec_review.py` and every `exec_accounts.py` option
  other than `--chain`.
- `exec_accounts.py --chain` since the review.
- A same-account case that passes with a certified package: `happy` fails
  with 0.7.0 as above, and has passed only once, with a trial package.
- A five-hour handoff whose destination is itself walled, one in a chain,
  one whose forward move fails and is rolled back, and one whose reset is
  near the host's bound. `five-hour-wall-far` has one usable destination and
  a reset two hours off.
- A five-hour handoff of a `codex exec` run.
- A weekly handoff with the walled account's title request refused. Only
  `five-hour-wall-far` refuses it; the four `weekly-quota` cases answer it,
  so whether they show the 30 s delay it brought is not known.
- A five-hour handoff of a session past its first turn, which sends no
  title request.
- A move of a session whose login was lost for good
  (`alab/design/accounts-lost-login.md`) with a certified package, in the
  matrix, or live. codex-alab's client is on main since `8bd759e3b6` and
  aterm's verb on aterm main since `7f1450f9dda`; 0.7.0 predates the client.
  The only runs are trial ones, each with an unverified package, the
  fixture and one host:
  - those of "Trial runs of the lost-login move", where no case that must
    move passes;
  - those of 2026-11-15, where both such cases pass once,
    `lost-login-expired` in the standard layout.
- Of the lost-login move, these were not driven by any case:
  - a source account that is pinned (the plan's `lost-login-expired-pinned`
    and its variant). The fixture's config has no switch for a pin
    (`NativeRenewalFixture`, aterm `native_renewal_fixture.rs` lines 24 to
    49 at `7f1450f9dda`, which refuses unknown fields), and the run's
    control socket serves no verb that sets one: its `harness` verb answers
    `policy` and nothing else (`harness_policy_verb.rs` `cmd`, line 273).
    One route exists in the source and was not tried: the fixture
    executable hands any `harness …` command line to aterm's own harness
    CLI (`crates/aterm-gui/tests/native_host.rs` lines 47 to 89), whose
    `accounts use <label>` sets the pool's pin (`harness/cli.rs` line 138,
    `harness/accounts.rs` `pin`, line 11687). Which stores that command
    opens under a run's environment was not established, and no run may
    touch aterm's real account store, so it was not run. Whether it reaches
    the run's pool, and what a pin on the source does to the move, is not
    known;
  - a successor that never attaches, and the rollback that follows (the
    plan's `unattached`). Not attempted;
  - a destination that is itself signed out or walled, or a pool with no
    other account;
  - a login refused with 401 while its usage still reads (the plan's third
    negative), and a refusal that arrives in a later turn or in a `codex
    exec` run;
  - a move with the issuer under the fixture's TLS more than once. The host
    from `7f1450f9dda` cannot serve it (`lost-login-expired` fails by name).
    One from `7540b6d94f2` moved it in the one run of 2026-11-15;
  - a host that prints the word and declines for a reason of its own
    (`not-needed` after a real read), other than through the TLS gap of
    `7f1450f9dda`;
  - each case more than once at `ab3be75c24`.

Since `4b4a67dc1d` (in 0.7.0), `alab-quota-recovery` ends a turn refused on
its five-hour window only when aterm's harness policy carries
`codex_rotation_five_hour=<seconds>` (aterm main since `2365b983b6c`) and the
reset is further off than that bound plus a minute. Such a wall is handed to
another account, which `five-hour-wall-far` asserts for a reset two hours
off. A nearer reset is waited out on the session's own account, which
`five-hour-wall-soon` asserts for a reset ten minutes off.

### Unit tests

```sh
python3.14 -B -m unittest discover -s <wt>/alab/tests -p 'test_*.py'
cd <wt>/alab/smoke && python3.14 -B -m unittest renewal_follow_test
```

At `314f0ccd89`, `renewal_follow_test` runs 84 tests and all pass. The 4
in `UsageWallCompletion` hold `completion_verdict` and `completion_seen`:

- a report answered `completed` after the consume, and a busy host's
  `waiting` before it;
- a dropped consume asked again;
- each wrong report, by name: missing, refused, completed twice, naming
  another receipt or turn, before the last consume, or sent before the
  consume;
- a pool left unproven, holding a canary, with a canary taken, or proven
  other than once.

`alab/tests/test_tui_shared_pool.py` (23 tests) holds the shared-pool
smoke's oracles on records shaped as `s/p1` and `s/c1` traced them:

- `pool_verdict`: a passing follow, and by name a follower that took the
  canary, waited the hold out, was committed before the report, was never
  turned away before it, or committed another receipt; also a second
  rotation, a rotation charged to another session, walls more than 1 s
  apart, and a missing report;
- `commit_outcome`: a commit answered a bare `waiting` is turned away, not
  taken, in a follow too, and a `waiting` that names a receipt is no
  turn away;
- `control_verdict`: a passing control, and a follower committed, a report
  answered or sent outside the lock, the pool proven, the canary taken, the
  hold lapsed or not the canary's, a follower that stopped asking, and a
  follower refused after the lock was let go, refused while it was held,
  or sending a verb no unmoved follower sends; what the hosts answered
  after the watch's end is not judged, and a bare `waiting` commit does
  not end the control;
- `requests_verdict`;
- the provider's barrier, and a barrier never met as a fixture fault;
- the custody lock's exclusion and mode.

Earlier, `renewal_follow_test` ran 78 tests and all passed (`ab3be75c24`); 72 ran
at `023a7188d0`, 53 at `14f0f3a67c`, 49 at `c6c67e410c` and 34 before
`five-hour-wall-far` was written. The 19 added with the lost-login cases
hold the fixture's replies (what each login is answered before and after
the loss, each refusal's status and body, what a served request records),
the reading of the host's policy word, the name and content of a kept
refusal, and both oracles: one record each accepts, and every wrong outcome
each names. The 6 added at `ab3be75c24` hold what the review found
unproven: who a request is attributed to and that one from neither program
fails, that a request with no route fails on either listener, that a
refused TLS handshake is recorded (by a real handshake against the
fixture's listener) and fails a case, where each case's token requests must
arrive, the ask's cadence (a doubled first ask, a flood of two thousand
asks and a skipped pass each fail by name, after everything else was
judged and kept), the whole-minute settle, the watch after a move with what
it is for, and, for the cases that must not move, a host that presents the
refused token or reads the source's usage more often than it was asked.
Each of the review's replays that passed the earlier oracle is one of
these and fails now. The error described next no longer
occurs: the fixture's certificate chain was corrected in `477225a5e3`
(`alab/reviews/fixture-tls-chain-2026-11-12.md`). As first recorded, the
module ran 28 tests with one known error, present at the
baseline: `test_usage_server_requires_its_explicit_tls_root` fails with
`CERTIFICATE_VERIFY_FAILED` (self-signed certificate) under python3.14, so its
last assertions do not execute. It is not python3.14's strict flag: clearing
`VERIFY_X509_STRICT` changes nothing. The fixture CA and its server
certificate share the subject `CN=127.0.0.1`, and the OpenSSL 3.5.9 in this
python reports the leaf as self-signed, while `/usr/bin/openssl verify` and
the client's rustls accept the same chain. Correcting it changes the
certificates every smoke serves, which needs a smoke run with a package that
can pass, so it was left.

`alab/tests/test_rotation_smoke_runs.py` runs the controllers whole, in
process, in about forty seconds (26 tests; 24 at `023a7188d0`, 17 before
the lost-login cases): `renewal_follow.py --case
five-hour-wall-soon`, including that a run cannot pass without its own
case's record and that its result carries its package's standing without
its verdict depending on it, the three wrong ends of `--case
five-hour-wall-far` a stand-in can play (a session that waits in its turn, a
receipt that names the walled login as its destination, and a host whose
policy prints no bound), the lost-login cases a stand-in can play (below),
`exec_accounts.py`, `exec_shared_pool.py` and
`renewal_matrix.py` (with what it counts as a passed case) against
`alab/tests/fixtures/rotation-host/stand_in_host.py` and a stand-in package.
The stand-in is a script that signs in with the run's own fixture login and
sends a session's requests to the controller's loopback provider. It is no
host and says nothing about aterm or codex; what it tests is that a fixture
fault, a missing or misnamed host record and a handoff end a run where and
how the controllers say they do.

For the lost-login cases the stand-in loses its login as a session would
(one answered turn, a 401, its refresh token presented to the issuer and
refused), keeps the refusal as codex-alab keeps it, writes the
`codex-renewal prepare` lines a host would trace, and answers `harness
policy` with or without the word. Its requests carry a User-Agent of
codex's own form. Against it, `lost-login-invalidated` and
`lost-login-no-word` run whole to a passing verdict, with their clocks
shortened, so the controller's listener, its loop and
`login_stayed_verdict` are reached by a test. A traced
`codex-login-rotation` line fails `lost-login-reused` by name at once, and
so does a refresh token presented under a User-Agent that is neither
program's; a session that never asks, and a host whose policy has no word,
each fail `lost-login-expired` by name before the hang detector;
`lost-login-expired-plain-issuer` opens its second listener, receives the
refresh there and says which layout it is; and a refusal kept under another
word fails the verdict after the loop ended. No stand-in run passes a case
that must move, for the reason no stand-in run passes `five-hour-wall-far`:
a pass needs two real session processes and a consumed receipt bound to
both. Its verdict is tested on records, and the 70 seconds the loop watches
after a move are reached only by a real run (`ll/p1`). `MatrixCaseTests`
names the seven lost-login cases as the only cases with a proof of their
own outside `renewal_matrix.CASES`, so a new case with a proof fails that
test until it is put in the matrix or in that list.

`alab/tests/test_package_standing.py` (8 tests) holds the standing of the
signed 0.7.0 record's own shape (its sysroot replaced by a path that names
no home), of package.sh's with and without its `verification`, of a trial
record and of malformed ones (half a surrogate pair among them), and the
matrix's refusal of each package that qualifies nothing.
`alab/tests/test_trial_package.py` (14 tests) holds `trial_package.py`'s
record and its refusals against a three-file repository, a stand-in script
and a dependency list written by hand; nothing in it is codex, and nothing
is built. `ParallelLoginsStandingTests` in
`alab/tests/test_rotation_smoke_runs.py` holds the `package` field of a
`parallel_logins.py` result, from a run that fails at its stand-in codex.

The whole `alab/tests` directory ran 668 tests in 821 s at `21ddd4d585` with
one failure, three errors and seven skips, the same four as at `2d19fd2e13`
(647 tests) and none in a file this work touches: three classes need a built
unit runner (`ALAB_TEST_UNIT_RUNNER`), and `test_contained_budget`'s
`test_runtime_grant_does_not_read_its_siblings_or_account_canaries` fails the
same way at `ae9b0eb15e`. The outputs are
`$HOME/.aterm/.ubd/codex-rotation-2026-11-12/unit-all-2d19fd2e13.log` and
`unit-all-21ddd4d585.log`.

## Concurrent credential isolation

`alab/smoke/parallel_logins.py` ran the canonical `b8a9f83eca` package with 40
independent auth files and a shared conversation directory. The loopback HTTPS
server held every response until all 40 account requests were outstanding,
checked each bearer credential against its account header and original task,
then returned a distinct answer to each process. All 40 completed successfully,
their auth files were byte-for-byte unchanged, and no `auth.json` appeared in
the shared `CODEX_HOME`. The result is
`$HOME/.aterm/.ubd/codex-parallel-40-final/result.json`. The final run also
checks 40 distinct conversation IDs and records the tested executable hash.

The same package passed `weekly-quota-exhausted` with both provider usage
responses full. Across repeated native prepare requests, the original task
remained pending on its source account. No native action, committed receipt,
rotation charge or additional model request occurred. The result is
`$HOME/.aterm/.ubd/codex-weekly-quota-exhausted/result.json`.

`weekly-quota-chain` passed against native host `2403ea6607`: the source and
first replacement both received typed quota refusals, and a third login resumed
the same original turn. The two receipts linked the exact process births and
account identities across both handoffs, both were consumed, and both terminal
bindings were removed. Four model requests covered the initial plan, two quota
refusals, and the final answer; the plan ran only once. The result is
`$HOME/.aterm/.ubd/codex-weekly-quota-chain/result.json`.

The shared fault controller now accepts `--quota`. Its lost-Signal-ACK,
pre-Signal panic and late-hold cases passed with distinct accounts, including
single-charge cleanup after continuation. Stopping before Signal also passed
against host `85fa67146f`, with cleanup requiring the native operation's
effect-free terminal state and drained input before clearing its attempted
launch marker. Unknown operations retain that marker. The retained result is
`$HOME/.aterm/.ubd/codex-quota-owner-stop-cleanup/result.json`.

Stopping after Signal prevented any successor from launching and passed the
existing stop oracle. That case still retains a pool-binding entry despite its
observed closed, no-execution cohort; retirement from that stronger closure
proof remains under review before production activation.

The test supports a configurable count; 40 crosses the old 32-account boundary.
This complements the roster/session capacity tests and the per-login refresh
locking tests. It does not claim that arbitrary concurrent loads need no memory
or that the synthetic credentials performed real enrollment.

## Accepted root input qualification, 2026-11-04

The private `71d149396b` package passes the regression that lost accepted child
mail in the earlier package: the message is present once after native quota
rotation, with the original process and turn. Human output, a dropped consume
acknowledgment and an intervening model/configuration change also pass. Eight
concurrent sessions pass two shared-pool rotations with delayed preparation.
The full repository gate passed on `4329f1a5d1`; details and artifacts are in
[the input handoff record](EXEC-INPUT-HANDOFF.md).

The account/mail boundary remains aterm's shared service. These isolated
provider fixtures do not establish live email-code enrollment or fleet
placement. Complete runtime preservation, ephemeral/review rotation and the
other unresolved requirements above remain open. No package was promoted.


## Unread controller mail qualification, 2026-11-04

Native exec handoff now retains unread controller mail for unloaded recipients,
including accepted eviction transfers that finish after queue sealing. Restore
requires original-turn admission and preserves per-recipient order, receive IDs,
and shared message-consumption state. Seven focused tests and the full gate on
`23d8563e0b` passed; see [the controller-mail record](EXEC-CONTROLLER-MAIL.md).
Active child turn recovery remains incomplete. The private combined package and the internal-start barrier passed their
[recorded qualification](EXEC-TREE-STARTS.md#qualification-2026-11-04), including
eight shared-pool sessions and post-consumption cancellation. The active-child
probe still reproduces interruption of the original child turn. No package
was promoted.
