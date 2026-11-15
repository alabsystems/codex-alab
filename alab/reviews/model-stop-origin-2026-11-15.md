<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# A duplicate report preserves the original committed source

After landing duplicate-block recovery, the next audit revisited its conservative
same-turn history boundary. A host/API pause or goal edit can precede a genuinely
fresh Active-to-Blocked model stop in one turn. Turn-only saved provenance then
rejects that earlier event after restart. Native regressions reproduced this
limit: both proposed earlier-event recovery cases failed at import, while the
28 existing controls and a new later-hold negative passed (29 pass, two expected
failures). No production change had been made.

A request position could supply a conservative execution lower bound, provided
the saved proof identifies the original request. Core attempts request persistence
before dispatch, but does not require persistence success. Tool outputs are a
later and unsafe boundary: a pause can occur after the database commit while a
PostToolUse hook waits, before the output is saved or replaced.

Call IDs are reusable. If the original request fails to persist and a later
same-turn request reuses the ID, searching saved history for a unique matching
call can accidentally identify the later request and hide an intervening hold.
Ordinary current request IDs are Text values: Core generates UUID strings when
the provider omits an ID and otherwise preserves the provider's ID. Their shape
does not attest a non-reusable host allocation. The invocation snapshot is
immutable and useful, but does not itself expose an acknowledged durable request
receipt. Host-origin lookup is best effort, not a stronger persistence guarantee.

Decision: do not add call-ID-only chronology or an allocation-only path that
cannot refine ordinary production requests. Keep the existing conservative
turn boundary and human-stop rules. More precision requires an acknowledged,
non-reusable native receipt or transactional source witness, not inference from
requested arguments, timestamps or output order. An atomic witness also needs
durable human-intent fences before cancellable instruction recording, explicit
accounting preservation and serialized admission. The existing auth-goal store
is a useful architectural precedent, not a substitute for those guarantees.

The audit found a separate concrete defect which can be fixed within the
existing authority model. On Blocked-to-Blocked, the completion observer checked
its prior binding but published the duplicate's new goal snapshot. That advanced
the saved turn even though all authority came from the original stop. A host/API
hold between the original and duplicate could fall before the new import scan
and disappear from consideration after restart.

Decision: a genuine Active-to-Blocked native commit still establishes a fresh
binding. A matching Blocked-to-Blocked report preserves the entire existing
binding unchanged, including its original turn and person-stop generation.
Other predecessors and mismatched identities create no authority. Pending-source
preservation and the unknown-source latch are unchanged. The history and native
goal validators can now continue to examine the original interval.

Native tests keep earlier same-turn host updates conservative, verify a hold
between commit and visible output, and exercise duplicate reports both with and
without an intervening persisted host/API hold. Their saved proof passes through
the actual record codec and a fresh Core/Shared/GoalService restart. No model
notification is invented as durable history.

The duplicate-origin regression then reproduces the concrete defect: 31 tests
pass and both duplicate cases fail at complete saved-source equality. The only
different field is the source turn, replaced by the later report's turn. All
28 original controls and the three conservative chronology controls pass.
Evidence: `~/.ubd/codex-goal-duplicate-origin-red.log`; official
receipt `codex-rs/target/alab-goal-full-libs.46PrTW/collected.json`.

Certified installation remains deferred at the independently reproduced compiler
proc-macro TCB restriction. Tests retain the source-selected protected native
lane, without a shipping proof claim or compiler/profile exceptions.

Validation on `9a26f008522f3e53278bc78f6f9b8ada92381b79`:

- GREEN passes all 33 focused native recovery tests in 3.25s. Both duplicate
  reports retain the complete original source through record encoding and a
  fresh restart; the unheld goal resumes and the held goal stays stopped.
- Full Autopilot passes 1,025 tests with four ignored in 241.09s. Core passes
  all 295 in 0.55s. TUI passes 387 with five ignored in 85.05s. Zero failures;
  1,707 tests pass in total, with nine intentionally ignored. The official
  artifact/runtime collector succeeds at
  `codex-rs/target/alab-goal-full-libs.qHxQgN/collected.json`. FD remains 65,536.
- Canonical all-target Tippy passes in 2m34s with zero ALab findings and 41
  upstream diagnostics set aside. Certified CLI construction then exits 2 at
  the unchanged in-process proc-macro TCB refusal, without authenticated proof
  inventory. No application package is built for shipping or installed.
- Two independent reviews approve the scoped branch split and realistic native
  controls. Trust formatting and whitespace checks pass. All validation and
  hook processes exit. The only new main advance composed before validation,
  `677e0751439e33776ade2a2df674d8d70c165be4`, changes Python smoke accounting
  tests and their review; there is no Rust overlap.

Evidence under `~/.ubd/`:
`codex-goal-duplicate-origin-{red,green,full,gate}.log`.
