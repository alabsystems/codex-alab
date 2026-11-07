<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Early usage restoration

The root sampler also checks authenticated allowance before issuing each model request.
At 95% usage it can redeem an earned
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

## Proactive reset qualification (2026-11-06)

The installed build `0.165.0-dev.13586+g133286d36` predates the quota-recovery
module. The previous implementation on main also waited for a weekly wall and
refused a reset with less than twelve hours remaining. The new preflight checks
both ordinary windows before each root sampling request and retains the same
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
