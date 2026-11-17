<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Include the smoke unittest regressions in the maintained inventory

Decision: include the three existing smoke unittest modules in the maintained
Python workflow. Keep their files and case IDs intact, use standard unittest
discovery, and retain the genuine native prerequisite before any remaining
test body. The required filenames are fixed in source; no CLI or environment
option changes that inventory or waives native admission.

At source `433a952e2c`, discovery walked only `alab/tests/test_*.py`. No maintained
full-suite command selected `history_pagination_test.py`, `renewal_follow_test.py`
or `weekly_reset_test.py`. They had separate focused/manual evidence, but their
137 current methods were outside the maintained manifest. Historical `full_scope`
results remain truthful for the fixed tree they declared; they do not gain this
additional coverage retroactively.

The same TestLoader now walks each explicit smoke filename after the existing
tree. Missing or empty required modules refuse discovery. Standard module-origin
checks reject a foreign cached module, and accumulated loader errors still fail
before native or remaining test bodies. Ordinary class/module cleanup and the
existing duplicate-case checks remain in force. No files move or new loader
mechanism is introduced.

`smoke/prune_test.py` is a standalone program with actual copied-process controls,
not a unittest suite. It remains separate and is named as such in the maintained
README. The existing `tests/test_prune.py` custody regressions remain in the
ordinary workflow; this change does not claim the standalone process check ran.

Five new real-loader methods exposed six assertion failures before the fix.
Afterward they pass, along with 33 existing coordinator methods and ten
failure-only CLI controls: 48 methods on managed Python 3.14 with `-I -B`.
The bounded fixtures check original IDs and module sources, no discovery-time
body execution, native success and failure ordering, class/module cleanup,
missing and empty modules, import errors, and a foreign cached module. CLI
fixtures keep their genuine native class and required IDs unchanged.

Actual discovery on this worktree selected 1,753 unique IDs with no loader errors,
including exactly 9 history, 84 renewal and 44 weekly methods. Each smoke module's
loaded source matched its expected file. This observation imported modules only;
it invoked no unittest runner or native prerequisite. Private inventory evidence
is under `~/.codex/.ubd/smoke-unittest-inventory-20261116/discovery.json`.

`git diff --check` passes. The complete maintained workflow remains deferred
under the recorded native CPU-authority refusal. The coordinator's inert unit
fixtures do not replace that native run, application qualification or packaging.
