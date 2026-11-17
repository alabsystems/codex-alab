<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve renewal outcomes through teardown and CLI reporting

Decision: after the renewal body selects its outcome, attempt stop publication,
each listener's safe cleanup and each trace export independently. Preserve the
selected cancellation through that work and the final result save. A failed
cleanup/export leaves a failed report; an incomplete observation cannot become
a successful CLI exit.

Public main/entry controls at `49d17b516e` used existing inert World records,
two fake listeners and a configure-time fault, with process creation forbidden.
A first-listener shutdown failure replaced KeyboardInterrupt, skipped the
remaining listeners and lost all request/result files. SystemExit(0) saved
`passed: false` but escaped the original entry with zero status and no
interruption metadata. The ordinary-error control returned one and cleaned up.
Private before/after witnesses remain under
`~/.codex/.ubd/renewal-tail-negative-20261116`.

The existing capacity support now has one multi-listener/export cleanup
implementation; `cleanup_case` remains its single-listener wrapper. Exception
selection and diagnostics stay shared. A failed shutdown skips only that
listener's join, while its close, the other listener and every export still get
their safe attempt. Renewal saves its final verdict after this work and rethrows
the exact API cancellation even if saving fails.

A thin CLI boundary applies the existing exit policy after parsing. The original
pre-parse monotonic timestamp is passed through, retaining the case budgets.
Help, unsupported combinations and argument errors stay parse outcomes; native
case proofs, host ownership and guardian command construction remain intact.
Independent review caught a parser reference left by the extraction; the
lost-login/`--quota` conflict now stays with its parser and has a code-two,
no-observation control. No undefined-parser path remains.

The outer guardian controller also returned zero after its existing adjudication
marked a case failed, or when a required case/guardian report was absent. It now
returns one for those incomplete zero-status outcomes, including guardian timeout,
while preserving genuine nonzero statuses. Review also exposed incomplete or
mistyped receipts accepted through truthiness: success now requires exact case
`passed: true`, integer zero statuses and guardian `timed_out: false`.
The existing CLI helper is added to
renewal-matrix custody. A real mutation of a copied helper during an inert
matrix run refuses the source manifest before accepting any case; the shared
source tree is untouched by that control.

The first eight regression methods exposed 23 assertion failures and seven
errors before the tail/CLI repair. A separate outer-controller control exposed
three additional false zero-status outcomes. The final thirteen methods cover
each required cleanup/export phase, simultaneous failures, actual cleanup
cancellation, final-save failure, CLI/API exits, parsing, helper custody and
positive multi-listener cleanup and malformed receipt refusal. All 66 selected renewal/shared-cleanup/quota/
request methods pass on managed Python 3.14 with `-I -B`; the thirteen new methods
also pass on macOS Python 3.9 with `-B`. Existing request/quota field behavior
and diagnostic transport remain covered. `git diff --check` passes.

The thirteen new tail controls execute no real listener, child process, native
host, application, compiler or provider account. Matrix records are controlled fixture inputs,
not native qualification. Acquisition failures before the existing body guard
remain a separate startup-scope followup; no complete native renewal or startup
cleanup claim is made by these tail controls.
