# Go-granted successor prefix crash

`successor-prefix-crash` targets an actual native launcher process after its
Execute/Go reply and before `exec_until` enters the canonical Codex executable.
It does not claim that a model-admitted Codex process crashed. The existing
0149 helper's `forward-after-go.pause` checkpoint returns an actual error after
its bounded 60-second wait, causing that real Go-granted process to exit.

The controller binds the actual checkpoint PID to the persisted attachment
candidate's kernel birth and original operation's completed Execute RPC. A real
hold prevents fallback while the prefix dies and the original 120-second native
budget expires. The later actual operation observation must report
`retired-cohort-exited`, retained source death, a quiescent/available closed
witness and drained input. Native retirement derives that state only after
retained death watches and group-gone checks for every Go-granted child; the
absence of a process or a no-Go receipt alone cannot satisfy this case.

Only after this positive closure proof does the controller remove the pause and
lift the hold. Recovery must keep the original logical root/Prepare/expiry and
admit a directly linked generation-one Resume-only child with a fresh budget.
Exactly one physical SIGTERM may target the original vendor; the child cannot
Signal. Exactly two Execute operations succeed: the crashed original prefix and
the fresh continuation. The final canonical Codex request must preserve the
original turn, input and completed tool output, use the new token, and reach
Consumed with only one additional model request.

The case uses the unchanged qualified 0149 native helper, with 450 seconds inside
a 480-second guardian. Its oracle controls are not actual crash qualification;
a fresh actual paired run remains required. Production gates remain disabled.
