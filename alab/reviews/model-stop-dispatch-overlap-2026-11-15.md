<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve original recovery across overlapping goal dispatch

Decision: reproduce the recorded direct/Code Mode publication overlap using the
native test harness before accepting a production change. The two dispatchers
own different tool-execution locks. A yielded Code Mode cell can leave a fresh
Active-to-Blocked mutation waiting in PostToolUse while a direct duplicate
Blocked-to-Blocked call commits and finishes. The fresh receipt is then stale,
but the duplicate has no accepted original source to transfer. Current code
fails closed and loses continuation availability.

The candidate repair retains only accepted duplicate receipt edges in bounded,
process-local custody. An original source still requires its own qualifying
completion. It may advance only through exact native before/after epochs,
unchanged goal identity, objective, budget and person-stop authority. No SQL row
comparison, call-ID ordering, pending unaccepted marker or sampled latest epoch
can supply a missing edge. API changes, failed mutation intent, mode changes and
native owner replacement remain boundaries. Existing history, admission, policy
and final-resume checks remain required.

Native RED, outcome controls, independent review and validation results will be
recorded here. Certified application packaging remains separately deferred at
the previously reproduced Trust proc-macro TCB refusal; this work does not waive
that boundary or install an unverified binary.

The native RED confirms the intended loss: the actual V8 Code Mode host invokes
fresh stop A, whose native PostToolUse command waits after SQL commit. Exec
returns Yielded and direct B commits and finishes successfully. After releasing
A, both real callbacks report Completed success with distinct call IDs in the
same native turn, yet the recovery source is absent. The regression fails at
that assertion in 1.51s (official receipt
`codex-rs/target/alab-goal-full-libs.eGM02r/`; log
`~/.ubd/codex-goal-dispatch-overlap-red.log`). The original controller
fully exits before implementation changes. The dormant Core primitive in that
test tree has no production caller.

Implementation decision: retain accepted Active origins as well as duplicate
edges, each bounded to 256 per thread. A source callback can arrive while a
third mutation is still unaccepted, so retaining only edges would lose it
again. A later callback can resolve a cached original only through exact
accepted epochs and must stage full history adoption unless that same source
was already live and validated. The originating Active callback may publish
its own source directly, comparing its immutable original receipt epoch.

Unknown duplicates preserve passive custody rather than closing it while an
earlier callback is outstanding. Actual rejection retires only accepted
origins whose exact path reaches the rejected binding's epoch. Blanket cache
clearing could erase a newer unresolved original source that arrived during an
old history read. Goal text equality cannot distinguish those native origins;
the receipt epochs do. These facts remain process-local and cannot be imported
from the saved record codec.

Validation of the implementation at
`68a0461aaab8d0cafe9b5966fbc042353751cede`:

- All 53 focused native recovery tests pass in 34.58s, including all 48 prior
  cases and the five actual mixed-dispatch cases: fresh origin, existing Known
  origin, API hold, additive usage accounting, and third unaccepted commit.
  Receipt: `codex-rs/target/alab-goal-full-libs.FJhwqt/`.
- All 14 Core receipt tests pass in 0.00s. They cover exact source/edge identity,
  missing API/mode/owner links, metadata conflicts, cycles, bounds, accounting
  and retirement of an old source while a newer equal-text origin is pending.
  Receipt: `codex-rs/target/alab-goal-full-libs.j2qHzD/`.
- The first canonical gate compiles all targets in 7m16s but identifies one
  redundant clone in the new unit fixture. It does not reach certification.
  The fixture moves the receipt instead at
  `0189d977f4857393e165590d05a9af26d71823dd`; production behavior is unchanged.

Full validation on the corrected `0189d977f4` source:

- Autopilot passes 1,045 tests, with four ignored, in 210.18s. Core passes all
  317 in 0.55s. TUI passes 387, with five ignored, in 89.10s. Total: 1,749
  pass, nine intentionally ignored, zero failures. The original controller
  exits zero and the official native/runtime collection succeeds at
  `codex-rs/target/alab-goal-full-libs.xlLKf3/`.
- Corrected canonical all-target Tippy passes in 6m49s with zero ALab findings
  and 41 upstream diagnostics set aside. The gate exits 2 at the unchanged
  in-process proc-macro TCB refusal. No authenticated proof inventory, compiler
  exception, application package or installation is claimed.
- Two independent implementation reviews find no blocking issue in source
  retention, replacement/history adoption, weak native-owner lifetime, exact
  cleanup, bounded conflict refusal and final epoch admission. Formatting and
  whitespace checks pass. Native tests retain FD 65,536.

Logs under `~/.ubd/`:
`codex-goal-dispatch-overlap-{red,green,core,full,gate,corrected-gate}.log`.

Before publication main advances to
`7b01e772a3973052a9ea1faad72c7ae894be1bbb`: stored credential handling for
go-ahead decisions and execution-wall classification. Read-only composition
review confirms no receipt, source, custody, mutation-epoch or ModelBlocked
admission file overlap. A credential act still passes through the existing
ModelBlocked goal resume branch and its exact native fence. Decision: compose
main, repeat focused model recovery plus credential/go-ahead decision tests,
and repeat the canonical gate. The broad results above remain tied to their
exact corrected implementation source; no broad rerun is justified solely by
this reviewed composition.

Next adversarial investigation selected: native marker identity when a provider
reuses an observed nested call ID for a direct call. Ordinary same-ID direct
calls serialize through completion; nested native IDs are host-generated UUIDs.
No ordinary production collision or new hold bypass is established. The
preexisting pending-marker map would lose precision under a true simultaneous
key collision. A future native test should capture the actual nested ID and
reuse it in a direct provider response while keeping real handlers, hooks and
receipts. Do not infer missing chronology from IDs or change receipt authority
before that experiment.

Composition checks at `6fc2902c308aef555352f29fe32eca94d9397ff1` pass all
53 native model recovery cases in 34.71s and 49 go-ahead cases (one ignored) in
40.01s. The composed canonical gate finishes compilation in 6m40s but exposes
four ALab lints in main's execution-wall additions: a duplicate child-helper
module, one redundant test clone and two method closures. Certification is not
reached on this failing gate. Decision: repair these findings before landing.
The shared helper moves to the existing ALab retry parent, preserving its
implementation, both test modules' paths and exact child-custody assertions.
The other findings use the suggested move and method-reference forms. All five
changed files are ALab-owned; no upstream hook or lint allowance is added.
Targeted native Core tests will exercise both helper users and quota observation,
followed by the credential decision tests and another canonical gate.

A separate read-only budget-race audit confirms the existing final-permit row
and remaining-budget read, atomic SQL BudgetLimited clamp, and continuation
status reread still protect accounting updates that do not change mutation
epochs. The earlier native budget-exhaustion regression passes in the recovery
suite. No new budget behavior or narrower callback-to-SQL runtime claim is made.

Lint repairs at `bbb5cdf51fdfbe8d80110f1afbe67e88008fab8a` pass:

- All 25 ALab capacity/retry tests in native Core pass in 0.21s, with one
  private child entry ignored in the broad invocation; its parent supplies the
  isolated environment. Both actual self-child helper users
  execute their parent controls. Receipt:
  `codex-rs/target/alab-goal-full-libs.WpksfC/`.
- All ten native Core quota-observation tests pass in 0.01s. Receipt:
  `codex-rs/target/alab-goal-full-libs.AeFMRr/`.
- All 13 credential decision tests pass in 0.15s. The 49 go-ahead tests already
  passed before the equivalent lint repairs; receipt:
  `codex-rs/target/alab-goal-full-libs.7yMsvX/`.
- Canonical all-target Tippy passes in 5m49s with zero ALab findings and 41
  upstream diagnostics set aside. Certification separately exits 2 at the
  unchanged proc-macro TCB refusal; no authenticated inventory or installation.
  Both original controllers settle before further composition.

Main subsequently advances to
`0af0a13ade015bee9277e8deb22ef24b43deb2ae`. Its two changes remove only a
matched old account-handoff Usage wall from Shared recovery. Independent
read-only review confirms no model source, custody, epoch, saved pace or live
Binding change. Decision: compose this narrow fix, run the model recovery
regressions and the real `rotation_decline` integration module, and repeat the
canonical gate. Do not treat a `--lib` filter as integration evidence.

Final composed validation at
`04dee1f77107e6cc504891d7fe27bfa07581bd94`:

- All 53 native model recovery cases pass in 34.99s, including the five real
  overlap controls and the existing hold, budget, cancellation, owner and
  restart cases. Receipt:
  `codex-rs/target/alab-goal-full-libs.9JNzVG/`.
- The actual `--test autopilot rotation_decline` integration suite passes all
  21 cases in 236.08s, with none ignored. Maintained focused-results validation
  accepts one nonempty suite and every required exact test name. Capture:
  `codex-rs/target/alab-focused-results.HSYo2d`. Managed V8 and the actual
  Code Mode host are source-qualified through metadata/native_helpers, with
  both runtime aliases bound; qualification state:
  `codex-rs/target/alab-goal-integration.GmFgxF/`.
- Final canonical all-target Tippy passes in 2m39s with zero ALab findings and
  41 upstream diagnostics set aside. Certification separately exits 2 at the
  unchanged proc-macro TCB restriction, with no authenticated proof inventory.
  Log: `~/.ubd/codex-goal-dispatch-overlap-final-gate.log`.
- All original controllers fully exit. Both native check owners report
  `sys left` complete with nothing left running. No source changes occur while
  those checks run. This final record changes documentation only.

The corrected implementation's full 1,749-test result remains tied to
`0189d977f4`; the final merged interaction checks above, the 35 targeted native
Core checks, and the 49 go-ahead/13 credential checks are separately qualified.
No broad rerun, compiler exception or application installation is claimed.
The next bounded adversarial marker-identity experiment remains selected above.
