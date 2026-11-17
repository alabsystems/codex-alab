<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Align the maintained application gate with reset policy — 2026-11-16

Decision: the application gate must request real fixture names and independently
require the owner's strict 48-hour weekly rule. Keep the production expectation
table fixed and reviewed; do not derive acceptance from the probe's own category
lists. This repairs the gate's contract without granting native application
qualification to a fixture-only result.

A read-only preparation for remaining native qualification found that
`check_runtime.py` still selected `preflight-beyond-24h`, which the real weekly
parser no longer accepts. Its table also expected five-hour-only, 24-hour and
25-hour cases to consume a reset and succeed, contrary to the committed policy.
The inert dispatcher fixture copied those older expectations, so its five tests
could pass while a real gate would be rejected by the actual parser or policy.

The maintained table now selects 16 cases. Five-hour-only, 24-hour, 25-hour and
exactly-48-hour scenarios require two model requests, zero new consumption POSTs
and a nonzero application exit. The renamed `preflight-beyond-48h` fixture has a
real 49-hour window and requires one consumption before two model requests.
Reactive recovery has separate 48-hour refusal and 49-hour positive controls;
the latter requires its original two model requests plus one recovered request.
Existing usage and post-selection identity/cycle/restoration controls remain.
The maintained README's gate description now names the same rule.

Three new contract methods bind dispatch to the actual weekly CLI parser and
actual HTTP fixture model. With a frozen clock, the five preservation controls
offer one earned credit, serve the real plan and quota wall, reject a deliberately
attempted fixture POST at the numerical weekly/48-hour guard, and retain their
credit. The two 49-hour controls accept the real consume payload and produce the
fixture answer after the expected two or three model requests. The authoritative
expected tuples and time/window requirements are explicit in these tests, not
copied from category membership. A separate parser control dispatches every
maintained case through the real CLI's choices while case execution is mocked.
The old production table produces seven assertion failures and one
actual parser error across these three methods.

The five inert dispatch methods remain infrastructure controls and now also
refuse a zero-exit report that redeems at 48 hours or preserves at 49 hours.
All eight runtime methods pass on managed Python 3.14. `git diff --check` passes.
The recent `cli_status.py` dependency is already in the runtime input snapshot;
its existing mutation-refusal regression remains passing. No new runtime import
or custody-list omission was found. No compiler, Codex binary, account, reset
credit, certified build, package or installation was used or qualified.

Independent source review confirmed that all 16 names are real parser choices
and that their tuples match the actual fixture model paths. Root independently
ran all eight runtime methods successfully. Neither review nor fixture execution
is native application qualification.
