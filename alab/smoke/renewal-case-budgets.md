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
well inside the normal 270-second bound. It has not been qualified by a run
with a package from the rotation branch, and is not in `renewal_matrix.CASES`.
