<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve shared JSON writer outcomes through close

Decision: guard the shared writer's returned handle directly and retain its
selected serialization/write error through close. Remove weekly reset's
identical writer and import the shared function through its existing capacity
import list. Weekly callers retain their module-global monkeypatch seam; this
adds neither an import dependency nor a runtime input edge.

The read-only audit on main `3a4846e3a5` used owned real output files. A real
json.dump TypeError was replaced by close OSError or SystemExit(0). Write
cancellation, nonzero exit and StopIteration were likewise replaced. A healthy
close-only zero exit escaped after complete JSON had been written. Every audit
descriptor was actually closed before the injected fault; healthy bytes and
exclusive-open collision controls passed. The later final ten-method regression
module, run before production edits against `cfa21acd9b`, produced 42 expected
failing subtests without errors, including the two original public writers.

`write_json(path, value)` now stores the returned handle inside its guard. The
original `path.open("x")`, `json.dump(value, handle, indent=2)` and final newline
are unchanged. An active open, serializer, write or cancellation error keeps its
exact identity. Secondary close diagnostics use the existing nonthrowing labels
and native-note/Python 3.9 fallback transport. The returned file gets exactly one
close attempt; failed opens transfer no handle to close.

Without an active primary, a close failure prevents successful return. Ordinary
errors, StopIteration, KeyboardInterrupt and genuine nonzero exits retain their
identity. A success-shaped close-only SystemExit becomes RuntimeError with that
exact exit as its cause, using the existing standalone classifier. No generator
context manager or traceback-attribute manipulation is introduced.

Weekly's duplicate definition is deleted and `write_json` joins its existing
from-capacity import after the early isolated exec-wrapper. Identity and actual
healthy/active-error/close-only controls cover both public entries. Existing
weekly tests still patch `weekly_reset.write_json` independently; capacity's
own global writer binding and the shared JSON/Path modules behave as before.
General JSON acceptance, including non-object values and default NaN handling,
text-open/encoding defaults, indent and newline bytes remain covered exactly.

Ten new methods use real files and the actual JSON encoder. They include real
unsupported/circular-value errors, a real closed-file write ValueError,
serialization/final-newline cancellation, hostile diagnostics, close-only
ordinary/control/zero exits, exclusive collisions, visible returned-line
cancellation and public alias integration. Fault wrappers close the actual
underlying FD before raising their close error. Assertions observe closure and
EBADF before independent fallback; counters require one production close per
returned file. Forbidden process/raw-signal calls are independently asserted
unused. The visible post-open and healthy/exclusive controls already passed the
old writer; this topic makes no additional publication-leak claim there.

All 110 selected methods pass without skips with `-I -B` on managed Python 3.14
and system Python 3.9: ten new writer methods, nine output-owner, ten child-cleanup,
eight signal-ownership, five capacity, sixteen package-reader and all 52 weekly
methods. `git diff --check` passes. The new writer controls launch no subprocess,
thread, socket, guardian, application, PTY or compiler. The existing weekly suite
uses its maintained loopback and isolated interpreter fixtures; no Codex
application, native guardian or compiler was launched by this selection.

Limits: close failure is an unsuccessful attempt, not proof of closure; the
controlled faults explicitly close before raising. A failed write can retain
partial JSON as before. This policy establishes neither atomic publication nor
durability, application qualification or build certification. A resource an open
operation conceals without returning remains outside the writer's ownership.

Independent integration review passed all 110 selected methods on both Python
versions. Independent hostile review passed the 58 bounded adjacent methods on
both versions and approved the ownership and stated limits. Root reviewed all
four paths and approved this topic for composition.

Root's composition at `4aef8195d38cb5ebd6c95f95f75cab18cfcf4ab8`
preserves incoming main through `88e8f5a6c2` and the separate gate-frontier
records. All 451 selected Python methods passed in 28.689 seconds with zero
failures, errors or skips; source HEAD and worktree stayed unchanged. This
includes the ten writer methods, prior output/child/signal/package controls,
the complete 52-method weekly fixture module, seven incoming matrix-result
methods, four finite capacity-process methods and two explicit pre-promote
fixtures. The unrelated incoming request-timeline source is preserved and is
outside this selection. Separate discovery found 2,025 unique identities with
no discovery errors and all ten writer identities; it executed no test bodies.

The selected log remains at
`~/.codex/.ubd/write-json-composition-tests-20261116.log`. This is selected
Python evidence, not the full native-gated workflow or Rust reset/application
qualification. Publication waits for the native Agh background quota reset at
2026-11-17 01:39:03 UTC; the last attempt was refused before any side effect.
Compose with freshly observed main through the normal expected-main guard
after that deadline, retaining each check's source attribution.

After the deadline, native Agh reads resumed. Composition preserves incoming
main `50a63d6d66`, which adds Rust startup-account test cases and their review
note. It changes no selected Python input, so the 451-method evidence above
keeps its original source attribution without an unnecessary repeat. No Rust
execution or application qualification is claimed for this composition.
