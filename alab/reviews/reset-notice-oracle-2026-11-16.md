<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Qualify coherent printed reset decisions

The first section records the original 48-hour implementation and evidence at
candidate `5781393531`. The current adaptation to published main's 24-hour policy
is recorded separately below; the earlier 43-counterexample proof is historical.

## Historical 48-hour oracle

Decision: tighten only the existing notice oracle for `recovery`,
`preflight-weekly`, and preflight/recovery weekly 25h/48h. Keep the completed,
nonfatal error-item extraction, fatal-error refusal, and unselected-case behavior.
Usage, consume eligibility and successful native-turn requirements remain the
public case's independent checks. No Rust or runtime policy changes are made.

The bounded audit on main `88e8f5a6c` called actual public
`case(..., "preflight-weekly")` with the existing inert process/listener seams.
Actual Mock backend requests showed a 72h refresh and one consume POST, yet five
bad notice variants returned and saved `passed: true`: a negated automatic
decision, contradictory preservation, unsupported expiry, stale extra inventory,
and reversed stages. Four standalone policy checks also accepted automatic-use
and restoration notices despite actual denied usage at 25h/exact 48h and no
consume POST. The valid public control passed. These are Python oracle results,
not application or native delivery evidence.

Complete inventory bodies now match recorded backend snapshots, including every
reported expiry. The initial snapshot remains required. A monotonic match allows
duplicates backed by the same listing and later snapshots in request order;
unbacked extra claims and replay of an older snapshot are refused. Malformed
numeric/count text cannot hide beside a good inventory. Only the four complete
current Rust informational `Considered...:` bodies are exempt from numeric
inventory matching; arbitrary suffixes on those families are not exempt.

Action classification uses bounded current canonical bodies. Existing abbreviated
positive messages remain valid, as do production messages containing the optional
opaque refresh token and earliest-expiry sentence. No clock comparison or required
refresh timestamp is added. Negated automatic/preservation text and opposite
actions are refused. Every inventory must precede every decision; every spending
decision must precede every restoration. The optional canonical applied-result
notice belongs between decision and restoration, and is forbidden for preservation.
Checking all relevant positions prevents one good chain from hiding early or late
contradictory stages. Unrelated warnings/tool output and the existing extraction
shape remain valid.

Hostile draft review proved additional false passes using actual case/backend
evidence: malformed comma inventory, negation within canonical action prefixes,
`not preserving`, the exact applied-result notice in all four preservation cases,
and an inventory claim appended to an informational prefix. The regressions cover
each rather than treating a poisoned execution seam as sufficient rejection.

The new module imports the existing fixture module by alias; it exposes none of
that module's TestCase classes to discovery. Manual fixture setup and output
creation share an enclosing registered-cleanup guard. Public bad-case checks require a notice-specific
refusal after actual model/consume evidence, and independently assert forbidden
Popen/process/socket calls unused after observations. Handwritten canonical
messages and real backend replies supply expectations independently of the oracle.

Validation: the final ten-method module against unchanged candidate
`5781393531c2a9a9b6070e0a7e8ddd6dc3ac6e1c` produced 43 expected failing
subcontrols and no errors. All 83 selected methods pass without skips on managed
Python 3.14 and macOS Python 3.9 with `-I -B`: `test_reset_notice_oracle`,
`test_reset_smoke_cli_status`, `test_check_runtime`, `test_cli_status_binding`,
and all unchanged 52 methods in `weekly_reset_test`. Everything outside
`check_reset_notice_events` is byte-identical, including the isolated wrapper,
extraction, six-case lists and CLI. `git diff --check` is clean. Independent hostile
review approves the final fixture and passed all 16 pure notice tests (the new ten
plus six existing methods) on each interpreter without skips. Root reviewed and
approved the final production, tests and note after the setup-cleanup and test
hygiene corrections.

Limits: this checks the current six-case renderer contract, not arbitrary natural
language, all possible notice kinds, or a new backend schema. The refresh token is
not independently timestamp-qualified. New notice controls use local real fixture
calls with no process/socket/guardian; adjacent maintained CLI/runtime fixtures use
their isolated interpreters and inert programs. No Codex application, native
guardian, compiler, remote service or GitHub operation is used. No native reset
behavior, certification or installed-adoption qualification follows from these
checks.

## Adaptation to published 24-hour main

Decision: retain the published policy in
`58b0a50f7d77de907876be64b94cb44a3d7bac35` and adapt the oracle's canonical
notice bodies. That source spends automatically beyond 24 hours, including both
25-hour cases, and preserves at exactly 24 hours. No Rust threshold, usage rule
or native behavior is changed by this adaptation.

The selected spending cases are `recovery`, `preflight-weekly`,
`preflight-weekly-25h` and `recovery-weekly-25h`. Preservation covers
`preflight-weekly-24h` and `recovery-weekly-24h`. Complete inventory matching,
the exact informational exceptions, contradiction rejection, stage ordering and
backed duplicate observations are retained. Action grammar now says 24 hours;
its refresh token remains optional and opaque. The ten meaningful notice methods
are preserved, with case labels corrected to match the current selection.

The merge's incoming parent is pinned to
`3234fd0c3e8596051b0c1a75fe9be10ee227018d`, which adds only the separate final
startup-retry Rust test and review text to the 58b0 tree. Conflict resolution
retains the complete oracle and incoming case assignments. Every unrelated
incoming path is byte-identical to that immutable parent, including intake,
matrix digest qualification and the startup-retry addition. Only the three
notice-topic paths differ relative to 3234. Weekly source outside the oracle,
including extraction and the isolated CLI wrapper, is byte-identical to both
58b0 and 3234. The original 578/48-hour proof is not reused as 24-hour validation.

Current validation keeps those phases separate. The adapted ten-method module
against unchanged published `58b0a50f7d` produces 31 expected false-acceptance
subcontrols and zero errors. The adapted tree passes all 84 selected methods on
managed Python 3.14 and macOS Python 3.9 without skips: ten notice methods, nine
reset CLI methods, eleven incoming runtime controls, two CLI-source bindings and
52 weekly methods. Frozen test time makes the real backend fixture's 25-hour
spend and exact-24-hour preservation observations explicit; both 25-hour traces
perform one actual fixture consume POST, and both preservation traces retain
denied usage with no consume POST. These are fixture requests, not native runs.
The tested source/test bytes were privately saved and hash-compared across the
merge-parent update; Python inputs are unchanged from the validated tree. The
incoming Rust test is preserved, without a Rust qualification claim from this work.

Independent hostile re-review passed the adapted ten plus six existing pure
notice methods on both interpreters without skips and approves the final pinned
merge provenance and three-path comparison. Root reviewed and approved the
adapted code, tests and final provenance note. No application, native guardian, compiler,
remote or GitHub operation is used for this adaptation. Existing adjacent CLI
fixtures retain their isolated interpreters and inert programs. No application
qualification or installed-artifact claim follows.


## Final composition

Root's clean composition `7dc2df6985c14624388dc6b5217129a6d1600773`
preserves published main `3234fd0c3e8596051b0c1a75fe9be10ee227018d`, including
its 24-hour policy, intake, runtime, matrix-digest and startup-retry work. The
only differing paths are the three notice paths and four independently reviewed
host-cleanup paths. Both 25-hour spending cases and both exact-24-hour
preservation cases remain covered by the real fixture observations above.

All 480 selected Python methods passed in 29.038 seconds without failures,
errors or skips, with unchanged source HEAD and a clean worktree. The selection
includes ten notice methods, twelve host methods, all 52 weekly methods, eleven
runtime controls, six incoming matrix-digest methods, the existing cleanup and
finite-process controls, and two explicit pre-promote fixtures. Separate
discovery found 2,054 unique identities with no discovery errors; it ran no test
bodies. The retained execution log is
`~/.codex/.ubd/reset-notice-host-composition-tests-20261116.log`.

This is selected Python evidence. The full native-gated workflow, remaining
Core reset tests, certified application build, package and installed behavior
retain their separate unresolved qualification requirements.
