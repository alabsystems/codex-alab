# Failed forward followed by a live rollback renewal

`failed-forward-rollback` exercises the existing real launcher checkpoint rather
than changing a command, capability, session record or production timeout. The
first Forward prefix pauses before Attach. Its existing 60-second bounded wait
returns an actual launcher error. The production coordinator's existing 60-second
phase rule then starts Rollback. Once that real rollback checkpoint exists, the
controller removes the Forward pause so a later genuinely fresh renewal can run.

The old operation must complete its captured Rollback command, become Retired
with launch_attempted still true, and name the exact old-auth rollback vendor's
PID and kernel birth. Its original Prepare and expiry remain immutable. The
retained native ledger must report rollback completion, and the live rollback
vendor must acquire its own actual proven supervisor claim. This is a live
source, never an expired dead-source continuation witness.

Recovery then requires a distinct root/native operation bound to that vendor,
a new deadline, and the same already selected credential generation and row.
The old terminal capability is never reused. The final actual request must use
the new token and preserve the original turn, user input and completed tool
output, with exactly one additional foreground model request and a Consumed
receipt. Exactly two physical SIGTERMs are allowed: one bound to the original
vendor and one to the live rollback vendor, in that order. Only two Execute
operations may succeed: old operation Rollback and fresh operation Forward.
Ordinary cases retain their exactly-one-physical-signal oracle.

This case uses the unchanged 0149 helper and a 450-second inner detector with a
480-second guardian, allowing the real initial policy checks, failed prefix,
rollback and fresh source policy checks. It has not yet run; tests of its oracle
are not actual rollback qualification. Production activation remains disabled.
