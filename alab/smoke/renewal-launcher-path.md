# Native launcher fault boundary

The actual native_host launcher callback resolves every stage checkpoint from
its inherited `HOME/.native-renewal-fixture`. The host-level control faults use
the case root, which is a different directory. The first rollback/crash controller
used the host root for launcher pauses and polls, so r4/c4 performed ordinary
successful recovery without activating either fault. Those runs are invalid
fault evidence and are retained as failures.

The controller now derives launcher paths from the exact fresh process
environment's HOME, creates that directory before prelaunch pause publication,
and uses it consistently for both stages' pause, reached and removal operations.
The existing Rust callback and its 60-second timeout are unchanged. A focused
boundary test checks that a pause is visible to the callback's HOME contract and
that its actual reached receipt is visible to the controller, independently of
the host fault root. Replaying r4/c4's existing callback files resolves their real
PID receipts at the corrected path and confirms the old root has none; this is
path evidence, not successful fault qualification. Fresh actual cases remain
required, including their strict intermediate phase and final recovery checks.
