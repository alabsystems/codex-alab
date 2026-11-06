<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Early usage restoration

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
sign in or switch accounts. Root TUI and exec samplers now redeem an earned reset automatically when a fresh authenticated ordinary weekly limit is exhausted and its next reset is at least 12 hours away. Redemption binds the actual account/user and weekly cycle, retains its chosen credit and idempotency key before sending, and retries the original in-flight request only after a fresh read confirms restored included usage. It never purchases credits.

## Validation

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

## Automatic earned weekly resets

An active ALab root turn can redeem an existing earned Codex reset credit when a
fresh authenticated usage read reports ordinary usage denied, the weekly window
at least 100% used, and its natural reset at least 12 hours away. The 12-hour
boundary is inclusive. Session-only exhaustion, an unknown account or user,
explicit credit or spend-control refusal, and missing available reset credits do
not authorize redemption. The operation does not purchase credits.

The account, native user and authentication ownership generation are bound
before sampling or foreground compaction. Recovery runs inside the original
request loop, retaining its
input and completed tool results. A fresh matching usage read must explicitly
permit ordinary usage and report a restored weekly window before that request
retries. Another genuinely exhausted weekly allowance can use another earned credit,
even within the same turn and natural reset deadline. Consumed-credit history
and separate retry evidence prevent stale reads from replaying those credits.
Cancellation interrupts the recovery.

A cross-process lock and durable receipt retain the selected credit and stable
account-bound redemption key before sending the request. Uncertain replies
reuse that key. An ambiguous operation is reconciled with its original key before another
credit can be selected. Positive consumption, definitive rejection and usage
permission are recorded separately; a healthy usage read never proves a credit
was spent. Other quota handling remains in place when recovery cannot establish permission.

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
