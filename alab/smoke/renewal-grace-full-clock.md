# Full real grace and policy observation budget

The g4 run demonstrates actual stale Ready retirement and fresh admission rather
than a native liveness fault. The initial Prepare waited at native monotonic
293491907795 and became Ready at 293551966210, approximately fixture 181s. The
real person gesture followed at 293552059817. The 121s human grace becomes ready
near 293673059817, but legitimate 60s scheduling intervals round observation up.
The next Prepare returned the exact old typed Retired at 293732229921, about
fixture 361s. The mandatory subsequent policy recheck produced a distinct Ready
at 293792313001 and Commit at 293792370208, approximately fixture 421s. The new
root became Committed with the correct human sequence and an actual successor.
The 450s detector then stopped the case before that successor's next 60s policy
recheck near fixture 484s. No new-token request or Consume was observed, so g4 is
not a successful complete recovery.

Budget all of the actual phases: the initial real 121s grace rounded up by the
60s policy schedule (up to 180s), the new gesture's real 121s grace likewise
rounded (up to 180s), a further 60s poll after typed Retired, and the successor's
60s policy recheck. These sum to 480s. Add 120s bounded startup, dispatch and
completion-observation headroom, giving a 600s inner hang detector and a 630s
guardian with 30s cleanup. The clock schedules observation only; unchanged
positive original-budget, person-grace, immutable Prepare, fresh admission,
original-turn/input/tool and Consumed assertions still decide qualification.

Only `grace-beyond-budget` changes. Rollback/prefix-crash remain 450/480s,
post-Signal cases remain 390/420s, and ordinary cases remain 150/300s. The
production native budget remains exactly 120s. A fresh actual positive case is
required; the historical g4 is preserved as a detector failure with partial
native liveness evidence.
