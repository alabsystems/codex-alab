# Explicit paired-case observation budgets

The controller's `CASE_BUDGETS` table is the single source for CLI cases, inner
hang detectors and process guardians. Each result records the selected pair.
These are test observation bounds; production policy, grace and the immutable
120-second native operation deadline are unchanged.

| Cases | Inner | Guardian |
| --- | ---: | ---: |
| Happy, ACK, pre-Signal panic, late hold, both OwnerStop cases | 270s | 300s |
| Grace beyond budget | 720s | 750s |
| Failed forward/live rollback, Go-prefix crash | 450s | 480s |
| Post-Signal continuation and diagnostic | 390s | 420s |
| Five-hour wall that resets soon | 270s | 300s |
| Five-hour wall whose reset is far off | 270s | 300s |
| Lost login that is moved (both issuer layouts) | 270s | 300s |
| Lost login that stays (four vetoed refusals, host without the word) | 180s | 210s |

Normal cases allow two 60-second initial policy/admission/reseal checks, two
60-second cold-successor observation checks and 30 seconds of dispatch headroom:
120 + 120 + 30 = 270 seconds, plus a separate 30-second cleanup margin.

The real g5 grace run adds an independently required quiet period after the
cold successor acquires its own claim. `Supervisor.ready_for` explicitly waits
`since + human_grace` after every claim transition (supervisor.rs3689–3698).
`follow_renewal` reschedules a readiness Wait at RECHECK=60 seconds rather than
its exact returned deadline (renewal_follow.rs844). The native person's clock
is not reset and is not sufficient to waive that new-claim grace.

Actual g5 Ready occurred near fixture181s, the person gesture at181s, typed old
Retired at361s, distinct fresh Ready at421s, and the successor's first proven
claim at native294606971385 (~424s). Its claim revision remained8 through native
294695988014; no reconfirm or renewal Status was admitted. Its121-second claim
grace expires near fixture545s, rounded onto the60s retry grid near604s, after
the old600s detector. The fresh native root was already Committed; that failed
run does not establish either a permanent hang or full recovery.

Budget121-second grace rounded up to180s three times: initial claim, person
gesture, and cold-successor claim. Add60s for a typed-Retired-to-fresh-Prepare
poll: 180 + 180 + 60 + 180 = 600s causal phases. Preserve120s bounded startup
and completion-observation headroom, yielding720s inside a750s guardian.
Fresh actual qualification is still required. Rollback, crash and post-Signal
bounds remain independent, avoiding accidental extension through shared flags.

The five-hour wall that resets soon hands nothing over. Its fixture lifts the
wall 20 seconds after the refusal and the walled turn retries at its next usage
poll, so the case needs one policy check, that wait and one completion check,
well inside the normal 270-second bound. It is in `renewal_matrix.CASES` since
it passed six times on hosts built from a clean aterm commit with the signed
0.7.0 package (`alab/PARALLEL-LOGINS.md`, "Qualifying runs of 2026-11-13" and
"Repeat runs").

The five-hour wall whose reset is far off is handed over as a spent week is,
so it takes the weekly case's bound. Its fixture states the reset two hours
off and refuses every model request of the walled login, its title request
too. The walled turn ends at the refusal and the host is asked within half a
second. The session's record then stays busy for about 30 seconds, during
which the host refuses to prepare it, and a 5-second recheck after that
brings the successor's turn: about 45 seconds from the refusal to the answer
in the runs of 2026-11-13. Inside the bound the controller fails the run by
name without waiting for the detector: at once when the session is put back
on the login it is walled on, when the walled login sends the request again,
when the host answers `not-needed` or when its policy prints no five-hour
bound, and when the host is first asked to move the session more than 20
seconds after the refusal (`FAR_YIELD_SECONDS`), or has not been asked by
then. A turn that keeps its request reads its credits again 30 seconds later
and only then may ask, so 20 seconds separates a turn that ended at its
refusal from one that waited, whether or not the session is moved later. It
is in `renewal_matrix.CASES` since three runs passed on a host built from a
clean aterm commit with the signed 0.7.0 package (`alab/PARALLEL-LOGINS.md`,
"The five-hour handoff, 2026-11-13").

A lost login that must move (`lost-login-expired`, and
`lost-login-expired-plain-issuer`, the same case with the logins' issuer on a
second loopback listener without TLS) takes the normal bound. The host
presents the refused refresh token itself and moves nothing until that
refusal has stood for a minute, so `ready` came 65 seconds after the host's
refusal in the trial runs of 2026-11-13 and 2026-11-14, and the successor's
turn was answered about 10 seconds after that. The controller then watches
for 70 seconds more (`LOGIN_MOVED_WATCH_SECONDS`) before it says nothing
followed the move: 150 seconds for a whole run of the plain-issuer case.
Inside the bound the controller fails the run by name without waiting for
the detector: at once when the host's policy prints no
`codex_rotation_login=1`, when the host answers `not-needed`, or when a
request arrives that the record cannot attribute or route, and when
codex-alab has not asked the host within 30 seconds of the refusal of its own
refresh (`LOGIN_ASK_SECONDS`; it asked 0.02 to 0.04 seconds after it in those
runs). With the host recorded from aterm `7f1450f9dda`, `lost-login-expired`
fails by name at the host's `not-needed`, about 5 seconds after the loss:
that host cannot present a token under the fixture's certificate
(`alab/PARALLEL-LOGINS.md`, "Trial runs of the lost-login move").

A lost login that stays (`lost-login-invalidated`, `-reused`,
`-invalid-grant`, `-unrecognised`, `-no-word`) hands nothing over and ends
when the login wall has stood on its own path for 70 seconds
(`LOGIN_WALL_HOLD_SECONDS`): codex-alab asks the host for the same account's
renewal on the pass after its refresh is refused and looks again 60 seconds
later, so both looks are inside the 70. With the first answered turn that is
72 to 74 seconds for a whole run, well inside 180. The controller fails the
run by name the moment anything of a move or an ask exists.

None of the lost-login cases is in `renewal_matrix.CASES`: their only runs
used a trial package, which the matrix refuses (`alab/PARALLEL-LOGINS.md`,
"Trial runs of the lost-login move").

## The shared-pool follow (`tui_shared_pool.py`)

These budgets are that controller's own, in its `BUDGETS` table, and every
result records the pair it ran with.

| Mode | Inner | Guardian |
| --- | ---: | ---: |
| `follow`, `--lost-consume-ack` | 270s | 300s |
| `--control` | 150s + watch | 180s + watch |

The watch defaults to 120 s and runs 30 to 420 s, so that it ends well short
of aterm's 600 s canary hold. In the trial runs of 2026-11-15 at
`314f0ccd89`, the follow and lost-ack runs ended 28.5 s and 33.4 s after the
controller started. The control ended after 145.5 s, of which 120.1 s was
its watch. At `6e0631276d` they ended after 24.5 s, 40.1 s and 145.3 s
(120.1 s of watch).
