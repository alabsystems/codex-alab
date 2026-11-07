<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Selected Python and update handoff on a Mac without Apple tools

During follow-up validation of the opening-conversation fix, the update suite
exposed an independent deployment prerequisite. `local_update.py` selects a
capable Python and passes ALAB_PYTHON to its controller, but `alab_account_home`
ignored that selection and called `/usr/bin/python3`. On this Mac that program
is an Apple developer-tools stub. The helper therefore refused before the
controller could finish its sandbox setup.

Decision: honor the same selected interpreter that the other lib.sh helpers
already use. Retain `-I`, the passwd database lookup, absolute-home checks and
refusal on failure. This does not derive the account home from the caller's
HOME, change sandbox rules or grant a new compiler selection.

Validation on the working managed Python 3.14.8 and managed Git:

- The two new account-home tests passed. They prove selection through a path
  containing spaces, isolated arguments, the actual passwd home despite a
  scratch HOME, and refusal without falling back to the system interpreter.
- The previously failing public update/cache-lock handoff test passed alone.
- `bash -n alab/lib.sh` and `git diff --check` passed.
- The 53-test `test_local_update*.py` suite ran on commit 071f7bf3fe. Its current
  handoff and interrupted-writer cases passed. The suite remains red: 11 failure
  entries and 2 errors, compared with 30 failure entries and 2 errors before the
  change. Some are subtest entries rather than distinct test methods.
- Remaining reports include `cannot locate the clone's common directory` in
  fixtures that explicitly replace PATH with system directories, where Apple's
  Git is absent, and missing status files in historical-bootstrap fixtures.
  Those prerequisites remain unavailable; this is not a full-suite pass.

The installed signed Codex selection was still build 2026110301 under index 92.
Another update request waited without output on its curl child and was cancelled
after several minutes; no new binary was selected or restart attempted. Rust
validation remains blocked by missing Apple tools and the qualified compiler
runner described in the opening-conversation review.

Failed fixtures were retained by the suite's own custody policy for inspection.
No installed selector, compiler admission, account home or macOS consent was
modified by this change.

## Fixture dependency follow-up

The remaining disk and default-interpreter fixtures explicitly replaced PATH
with system directories. That discarded the managed Git used by their parent
runner and failed before the intended disk-space or Python-discovery assertion.
Bind that same Git through a private fixture directory containing `git` alone;
the selected Git's other tools, including any Python alongside it, stay off this
restricted PATH. Production interpreter and Git discovery are unchanged.

All six affected tests passed. On committed fixture change 0ea77c8091, all 52
available `test_local_update*.py` test methods then passed together in 56.135 s.
The loader explicitly excluded only
`test_old_bootstrap_cannot_adopt_a_controller_requiring_guardian_lifetime`, and
printed that deferral before running. That historical-source fixture still
requires the missing system Python; it was not changed or reported as passing.
This supersedes the current-fixture failures above, while preserving their
original observed results and the historical/bootstrap and Rust limitations.

## Full fleet validation with Apple's Python available

The previously unavailable historical-bootstrap test was run on m157, which has
Apple CLT and a working `/usr/bin/python3` 3.9.6. Its normal `python3` names that
system interpreter; managed CPython build 1 separately exposes `python3.14`.
Used the latter as the test runner without changing global interpreter settings.

On exact source `fcc44b071e`, `python3.14 -I -B
alab/tests/test_local_update_job.py -v` passed **all 37 tests**, without exclusions,
in 47.300s. Retained block 3 in owned plain shell `s-cadee9625204f4ae5ff4` confirms
exit 0. This includes the historical-bootstrap guardian-lifetime refusal case
that was explicitly deferred on the reporting Mac. Tests use private repositories,
package prefixes and fake selectors; no real installed package was selected.
Fixtures the suite retains for process-custody evidence were left in place.

The 16 companion `test_local_update.py` methods also passed, without exclusions,
in 8.559s; retained block 4 confirms exit 0. All **53 update methods** therefore
passed on this host, including the previously unavailable historical case.
This supersedes the historical test deferral for the fleet result; the reporting
Mac still lacks Apple's tools. Logs are `update-job-full.log` and
`update-companion-full.log` under `/tmp/.ubd/codex-native-f0a4e96f.8bYgIz` on m157.
