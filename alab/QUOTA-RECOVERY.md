<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Early usage restoration

The root sampler also checks authenticated allowance before issuing a model
request. The first request of every turn reads current backend usage; a later
request of that turn reuses a reading that permitted it only while the reading's
headroom proves the 95% threshold out of reach, and reads again otherwise (see
"Reusing a clear usage reading" below). At 95% usage it can redeem an earned
reset before either the five-hour or weekly window is exhausted. Either window
must be at least five minutes from its natural reset. Unknown identity or permission, nonordinary
quota and explicit spend denial do not authorize this action. Already denied
usage requires an actually exhausted window, rather than inferring its cause
from a near-full counter. No AI prompt chooses or redeems the reset.

Preflight uses the same account-bound custody lock and idempotent redemption
record as reactive recovery. A consumed credit cannot lead to another preflight
redemption before fresh usage has been observed below the threshold. The original
request, input and completed tool results stay in place. Reactive recovery also
handles ordinary five-hour exhaustion, including a request that consumes its
remaining allowance before another preflight can run.

A live usage wall normally waits for its provider reset and retry backoff. It can
resume earlier when an accepted full account usage read explicitly permits
ordinary usage and includes the matching quota bucket without an explicit denial. The request must
start after the wall, match its native account and identity generation, and be
less than thirty seconds old. Display counters, absent windows and credit balances
do not override the backend permission. Paid credits alone never authorize recovery;
missing permission, upsell or an explicit limit blocks it.

A credit-only usage error can also receive one ordinary-usage retry after a
witnessed backend permission transition from false to true. This does not alias
its missing premium bucket to the ordinary bucket or claim premium capacity was
restored. Already-true polling cannot keep retrying that wall; a new failure needs
a new transition. A matching bucket's explicit denial always takes priority.

The wake goes through the existing Autopilot policy, input, goal and submission
checks. Another limit retains the normal retry ladder. Account changes invalidate
the evidence; rolling token notifications, graph history, failed reads and
unknown permission cannot wake the wall. The recovery evidence is checked again
after asynchronous preparation before submission.

New recovery records retain account, native user and bucket scope plus whether an
ordinary-usage denial was witnessed, never permission. A restart binds matching
current authentication and requires a new request started after restoration.
Older persisted recovery records lack this scope. They
keep their original reset deadline after restart; no account scope is inferred
from a later usage read. A new live wall can bind the known native account even
when no usage read preceded it. The bridge does not poll separately, buy credits,
sign in or switch accounts. Root TUI and exec samplers redeem earned resets
before ordinary five-hour or weekly exhaustion, using the preflight described
above. They also retain reactive recovery if a request exhausts its remaining
allowance. Redemption binds the actual account, user and allowance cycle, retains
the chosen credit and idempotency key before sending, and continues only after
a fresh read confirms restored included usage. It never purchases credits.

## Reusing a clear usage reading (2026-11-11)

Until this change every root sampling request and every foreground compaction
did one usage `GET` before its model call, under the per-account custody lock,
after taking the account resource lease and loading the ledger: one read per
tool round. A slow usage endpoint could add up to 45 seconds to each request,
and cost every peer session on the account a five-second lock wait per request.

A preflight read now excuses the later preflights of the same turn while it
proves headroom. A reading counts as clear only when it permitted the request:
the identity matched, ordinary usage was explicitly allowed, nothing was
explicitly denied, and every ordinary five-hour and weekly window was below 95%.
It holds while

    0 <= now - read_at < min(60 s, (95 - used) * 4 s)

where `used` is the larger of that reading's highest ordinary window and the
highest window in the session's latest response rate limits for the ordinary
(`codex`) bucket.

| `used` | The reading is reused for |
|---|---|
| 80% or less | 60 s |
| 85% | 40 s |
| 90% | 20 s |
| 94% | 4 s |
| 95% or more | never: every request reads, as before |

The constants are `PREFLIGHT_SECONDS_PER_POINT = 4.0` and
`PREFLIGHT_CLEAR_MAX_SECONDS = 60.0` in
`codex-rs/alab-quota-recovery/src/unix.rs`, beside the existing 95% threshold.
The 60-second cap matches the TUI's resting usage poll.

### Why this revisits the per-request read

Commit `ae76084e34` (2026-11-06) removed a fixed 30-second interval one minute
after it landed, because "a fixed sampling interval could miss rapid allowance
consumption". That objection stands against a fixed interval and is the reason
this rule is not one:

- The window is not fixed. It shrinks continuously with headroom, from 60
  seconds at 80% or less to nothing at 95%, where the every-request read
  returns unchanged.
- Response evidence overrides it. Model responses report the ordinary bucket's
  windows and the session keeps the latest report; the next preflight judges
  the reading against the higher of the two values. A response at 95% or more
  ends the reuse at once.
  That evidence carries no account, permission or observation time, so it can
  only shorten the window: a stale or foreign high value costs extra reads, and
  a low value excuses nothing. An unreadable value counts as full, a window of
  unknown length counts as an ordinary one, and another limit family is ignored.
- Redemption is unchanged. A reused reading never redeems. Every redemption
  still follows a fresh authoritative read, in the same call and under the
  custody lock.

### The burn-rate assumption

Usage grows only as requests complete. A reused reading can let a request start
past 95% only if the whole account, across every session on it, consumed more
than `95 - used` points within the window, which is more than 0.25 points a
second since the reading. For the five-hour window that rate spends an entire
allowance in under seven minutes, 45 times the rate that would last the window;
for the weekly window it is about 1,500 times. This bound is an assumption from
arithmetic, not a measurement of real accounts. Many heavy parallel sessions on
a small plan are the case that could approach it; halving the constant to two
seconds a point is the conservative alternative.

Two granularities loosen the bound slightly, and both are recorded rather than
compensated:

- The usage endpoint reports whole percentages (`used_percent` is an integer in
  the payload). If the backend truncates, a reading can understate usage by
  just under one point, so at a reported 94% the four-second window is nominal
  rather than proven. Whether it truncates or rounds was not measured.
- Times are whole wall-clock seconds, so a comparison can run up to one second
  long. `read_at` is taken before the request is sent, which errs the other
  way. Host sleep counts toward the age. A wall clock stepped back past
  `read_at` forces a read; one stepped back by less would have made the reading
  look younger, so since 2026-11-12 the reading also carries a monotonic
  instant and its age must be inside the window on both clocks.

A miss is not a lost turn. The threshold is five points short of the wall, and
a request that does meet the wall degrades to the existing reactive recovery:
it redeems an earned reset and resumes the same request with its input and
completed tool results.

### Scope and what did not change

- The clearance lives in the turn's `State`, in the turn store. The first
  request of every turn reads, and nothing is shared between sessions, written
  to disk or added to the custody ledger.
- It is recorded at exactly one place: a preflight read that permitted the
  request, after that read restored and released what the ledger held. A read
  that redeemed, was denied, failed, timed out or belonged to another identity
  records nothing, and any preflight that has to read discards the previous
  clearance first.
- `recover_weekly` discards it on entry. A wall contradicts a clear reading,
  and recovery changes the ledger.
- A reused reading takes no custody lock, no account resource lease and no HTTP
  request.
- Settling another process's ledger state (an uncertain acknowledgement that
  fresh usage proves restored, or a committed credit receipt) can now wait for
  this turn's next read, at most 60 seconds, and still precedes any redemption.
- Lock contention still fails open: after five seconds without the custody lock
  the request proceeds unchecked, and the lock is still held across the read.
  Fewer reads make that rarer; changing it is a separate decision.
- When an account has further metered limit families, the session keeps the
  last family's response limits, so the response evidence sees nothing and the
  rule falls back to the time bound alone.

No upstream line changed. The response limits are read by ALab glue already
mounted inside `crate::session` (`exec_quota_observation.rs`), which can see
`Session.state` and `SessionState.latest_rate_limits`; an upstream rename of
either breaks that glue's build at a sync.

Rejected: a fixed interval (the removed design); excusing a read on the response
limits alone (no time, identity or permission); a reading shared between
processes or kept on disk (it would cut reads further, but adds shared state to
a custody ledger, and remains a possible later step).

### Validation

`targo --unverified test -p codex-alab-quota-recovery` passed all 39 tests,
including the three new ones in `clear_tests.rs`:
`clear_reading_freshness_scales_with_headroom` (the pure rule and both usage
extractors), `clear_preflight_reuses_one_usage_read_within_a_turn` (one `GET`
for a turn's back-to-back preflights, a response at 96% forcing a second, a
peer holding the custody lock not delaying a reused reading, and a new turn
reading for itself) and `pressure_and_recovery_never_reuse_a_reading` (a
reading at the threshold, exhausted, denied, for another account or under
spend control records nothing and replaces an earlier clearance, and
`recover_weekly` discards one). Removing either discard makes the third test
fail. The usage endpoint cannot report a fractional percentage, so the
sub-second window at 94.9% is asserted on the pure rule only.
`targo --unverified test -p codex-core --lib
weekly_reset_retries_original_local_and_remote_compaction` passed with its
usage-read counts unchanged, since each compaction binds once.
`targo tippy -p codex-alab-quota-recovery --all-targets` reported nothing in
ALab's code.

These were Trust `--unverified` checks on loopback fixtures, with no proof
claim. Not run: `alab/smoke/weekly_reset.py`, the only coverage of the real
sampler path, which needs a packaged binary and must be rerun before this rule
ships. By reading, the one preflight it now skips is the healthy read before a
case's second model request, which no case counts, and recovery discards the
clearance before any later round. No packaged case yet asserts the reuse
itself. The other account-handoff probes and the full sync gate were not run.

## Proactive reset qualification (2026-11-06)

The installed build `0.165.0-dev.13586+g133286d36` predates the quota-recovery
module. The previous implementation on main also waited for a weekly wall and
refused a reset with less than twelve hours remaining. The new preflight checks
both ordinary windows before each root sampling request (since 2026-11-11 a
clear reading is reused within its turn, see above) and retains the same
request and completed tool results. The backend can refuse early redemption;
reactive recovery remains available when a request exhausts its allowance.

Quota source `76eacbc29b` passed nine unit tests, strict all-target quota lint,
and scoped sampler/core lint. The repository filter reported no owned,
spanning or failing diagnostics and separated five upstream warnings. The
implementation was cleanly composed with main `766acd06bb` as
`015959bd4baa039d85973c9bb2f447bb700f7df9`; those incoming changes touch none of
the five quota-change files.

The canonical package `0.1.0-dev.14964+codex.0.167.0.g015959bd4` passed its
version, signature, system-library and layout checks and all fourteen raw CLI
recovery scenarios. Both proactive scenarios redeem exactly one credit before
the first model request, then complete in two model requests with the original
input and completed plan output retained. The other scenarios cover repeated
resets, lost acknowledgements, pending operations, stale reads, identity
mismatch, unavailable credits, near-reset refusal and unrelated providers.
Its `bin/codex` SHA-256 is
`33770c9b22ab6fb03c15a15aed1737855cdb3593fc6972bcf3d4641bcb5d7b3c`.

The installed old binary was a negative control: the five-hour preflight
scenario rejected its first model request because no reset had occurred.
That failure was the HTTP fixture's ordering assertion, not a missing binary
or a build failure. Reproduce the runtime suite with
`python3.14 -I -B alab/smoke/weekly_reset.py --package <package> --output <new-directory>`.

The qualified package was selected through the conditional atpkg link, then
the native restart resumed the same conversation. Kernel executable mapping
confirmed that the new process loaded this package, and the executable hash
matched the runtime qualification above. This verifies adoption as well as
selection; the broader aterm rollout-format measurement was not run here.

These checks used Trust's unverified lane, with no new formal-proof claim.
Runtime scenarios used isolated fake ChatGPT authentication and loopback HTTP;
they did not spend live reset credits. This focused qualification did not run
the complete upstream sync gate.

## Earlier allowance-wake validation

Validated the code at `d76a3bc95c`, composed onto private main `5b7a00447a`:
1,174 tests passed: core 141, Autopilot 714, ALab-TUI 299 and the focused allowance
suite 20. The existing Land holdout measurement remained ignored. The CLI built
and its version smoke passed. Scoped all-target lint reported no owned, spanning
or failing findings; seven upstream diagnostics were set aside by the repository
filter. Changed Rust formatting, diff checks and the upstream hook audit passed.
Only two upstream hook lines were added; the recovery behavior remains in
ALab-owned code.

These were native Trust `--unverified` checks, with no proof claim. No new formal
model, TLA check, proof lane or full sync gate was run. Validation used scratch
account homes and loopback fixtures; it did not change the installed binary,
package index, pin, selected runtime or credentials.

## Automatic earned resets

An active ALab root turn can redeem an existing earned Codex reset credit when a
fresh authenticated usage read reports a five-hour or weekly window at least
95% used. An explicitly denied allowance must be at least 100% used. The natural
reset must be at least five minutes away, including that exact boundary. An unknown account or user,
explicit credit or spend-control refusal, and missing available reset credits do
not authorize redemption. The operation does not purchase credits.

The account, native user and authentication ownership generation are bound
before sampling or foreground compaction. Recovery runs inside the original
request loop, retaining its
input and completed tool results. A fresh matching usage read must explicitly
permit ordinary usage and report restored ordinary windows before that request
continues. Another genuinely exhausted allowance can use another earned credit,
even within the same turn and natural reset deadline. Consumed-credit history
and separate retry evidence prevent stale reads from replaying those credits.
Cancellation interrupts the recovery.

A cross-process lock and durable receipt retain the selected credit and stable
account-bound redemption key before sending the request. Uncertain replies
reuse that key. An ambiguous operation is reconciled with its original key before another
credit can be selected. Positive consumption, definitive rejection and usage
permission are recorded separately; a healthy usage read never proves a credit
was spent. Other quota handling remains in place when recovery cannot establish permission.

The replies the rotation smokes serve for these endpoints, and the test that
decodes them with this client, are described in `alab/PARALLEL-LOGINS.md`,
"Rotation smokes against a recorded host".

### Earlier weekly-only qualification

The sampling path at `3cc2166e51` passed twelve raw CLI runtime probes
using isolated fake ChatGPT authentication and a loopback backend, plus seven
unit tests. Runtime traces retain the original input and completed tool results;
they cover repeated resets within one turn and deadline, lost acknowledgement,
pending operations, rejected credits, stale credit lists and usage, identity
changes, the eleven-hour refusal and unrelated-provider isolation. The unit
tests cover the inclusive twelve-hour boundary and explicit spend denials.
The runtime fixture disables request compression so its HTTP JSON is inspectable.
These are unverified Trust checks, not a formal proof or live account redemption.
The final composition at `08f50645e0682b9c2cf08da92227620a9a5817e1` passed
its CLI build, all twelve raw CLI runtime probes, all seven quota unit tests,
and the isolated core test covering both local and remote foreground compaction.
Each compaction quota error redeemed an earned credit and retried exactly the
same input once. Its owned command guardian completed without timing out.
Scoped all-target lint for core and the recovery crate reported zero owned,
spanning, failing or script findings; eight upstream diagnostics were set aside.
Changed Rust formatting, diff checks and the marked-hook audit passed.

The final CLI image SHA256 is
`4d4484b2044282cf91b5750679bd9010a7812503637ff6ced45ceaa276ca55b7`.
The retained runtime manifest is
`/tmp/.ubd/weekly-reset-08f50645e0/qualification.json`; local check logs are
`/tmp/weekly-reset-08f-{tests,compaction,lint}.log` and the structured lint verdict
is `/tmp/weekly-reset-final-checks/verdict.json`. This validates the source and
isolated runtime behavior; canonical package qualification and installed-runtime
adoption remain separate.

### Canonical package and local adoption

The canonical package from remote-main commit
`6d32580f104636fbdef9df85c656747a245d7e92` passed the standard package build,
compiler admission, layout and strict code-signature checks. All twelve runtime
cases then passed against the packaged executable, with every owned guardian
terminal and no timeout. Package version:
`0.1.0-dev.14471+codex.0.166.0.g6d32580f1`; executable SHA256:
`4f2fef09371e90ea88ad592ce8520ad31c61baa2b87364b71388470c58d9fb33`.
The retained manifest is
`$HOME/.aterm/.ubd/weekly-reset-canonical-runtime/qualification.json`.

This package was selected locally through `aterm pkg link codex` and adopted by
the authoring session after its supervised restart. The supervisor's actual
executable mapping (`lsof -a -p 53827 -d txt -Fn`) named that package's
`bin/codex`, and its SHA256 matched the qualified executable. This establishes
adoption by that session, not a public release or adoption by every existing
session. Selection and adoption evidence is retained in
`$HOME/.aterm/.ubd/weekly-reset-adoption.json` and
`$HOME/.aterm/.ubd/codex-auth-goal-handoff.json`.
It remains fixture-based redemption evidence; no live account credit was spent
for qualification.

### Account enrollment package regression

On 2026-11-06, all twelve `alab/smoke/weekly_reset.py` runtime cases passed
against the canonical enrollment-UI package
`0.1.0-dev.14833+codex.0.167.0.g1d57fabe8`, source
`1d57fabe87c98a9b68d3a9fcf683338bb78ae10b`, executable SHA256
`8ad804c4ab9ea2e9a7e4e9a945324adc45039e85dde67130b147ab5a5cb83318`.
Every command guardian terminated without timing out. The checks covered earned
redemption, lost acknowledgements, uncertain and repeated operations, stale
credit and usage observations, identity changes, unavailable credits, the
eleven-hour refusal and provider isolation. Successful retries retained the
original input and completed tool results. The report is retained at
`$HOME/.aterm/.ubd/codex-accounts-enrollment-reset-runtime/result.json`.
This used synthetic authentication and a loopback backend. It neither spent a
live credit nor changed the selected runtime, and does not qualify unfinished
account switching or native enrollment integration.
