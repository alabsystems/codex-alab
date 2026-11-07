# Combined Python companion discovery

Decision: after finding a test file whose direct invocation ran zero cases,
exercise the companions through unittest discovery to include import composition.
The clean source was 75fb092b3ee5b11d0a08d58ed809be2bb8f745de. Discovery found
525 cases; 478 were selected. Four whole modules were excluded: the previously
documented test_check_lint_step profile-context deferral, test_unit_runner_native
(local Apple CLT unavailable; qualified remote execution already recorded), and
test_package_cache_budget_native/test_package_controller_native (require a
qualified native producer). No source was changed during the run.

The run exited 1 after 473.855 seconds. Unittest reported 461 tests run,
9 failures, 18 errors, and one skip. This was not a green aggregate result.
Seventeen errors were Git initialization in project-selection and pristine
retention fixtures; two failures were V8 wiring using system-only Git; seven
failures were release-line fixtures falling back to Apple's optional Python.
The remaining error was UnitResultsTests.setUpClass requiring an actual built
alab-unit-runner. That class did not execute; the default artifact is absent in
both this worktree and the shared checkout. No substitute executable or skip
was introduced. Local native compilation and the remote artifact lookup remain
deferred under the already recorded tool/machine conditions.

Complete initial log: ~/.codex/handoffs/companion-discovery-2026-11-07.log,
83683 bytes, SHA-256
5dbe00fc9d00f0fcf8dd61b2ce9764f7a2adfa7c44a7b6cfc584873d4b562ff7.

Corrections are confined to four fixtures: discover the caller's Git bin in
project-selection, pristine-retention and V8-wiring restricted PATHs; explicitly
bind the release-line interpreter to sys.executable. The retention environment
also carries the caller's ATPKG_BIN so the existing profile's managed-store read
rule can reach the selected Git. Without that context the actual Seatbelt test
correctly refused the store executable. No sandbox rule, write grant, compiler
selection, source-admission or production release behavior changed.

Individual verification after correction: project selection 10/10 in 3.840s;
retention 16/16 in 13.305s; V8 wiring 4/4 in 7.967s; upstream base 16/16 in
12.649s. The four affected modules are also rerun together after discovering all
modules, preserving the import context of the original run: all 46 passed in
34.263 seconds, no skips. The initial red log
is retained separately from that focused post-fix log; no full aggregate green
verdict is inferred from focused verification. Diff whitespace checks passed.

## Subsequent scoped aggregate

The corrected tree b568c9f4f6abab2924a23b54f3e0064559556365 was then held
clean and fixed for another complete discovery run. Selection explicitly also
excluded test_unit_results, whose required actual native artifact is unavailable
locally. Thus 525 cases were discovered, 461 selected and 64 outside this run's
scope across the five named modules. No fake artifact or implicit passing
substitute was supplied, and the underlying prerequisite error was not hidden by
changing that test class.

The selected run completed exit 0: 461 cases in 488.030 seconds, 460 passed,
one existing opt-in skip, no failures or errors. This checks the available local
companions together, including the four corrected modules, source/guardian
custody, containment, promotion, retention, sync, versioning and renewal oracles.
No files changed during execution. The actual native source gate, producer
qualification and package smoke remain separately recorded; this local result
does not complete the deferred m157 rollout proof or installed adoption.

Complete final log:
~/.codex/handoffs/companion-discovery-final-2026-11-07.log,
51617 bytes, SHA-256
55307c9ae8d69cd5fa2a62ee541ec0860696ff726d73e65c98a1fe5bec666462.
The original red run and focused post-fix log remain intact. Decision: retain
the fixture fixes and this scoped green result; do not repeat the same local
checks absent another source change or finding. Resume the unavailable native
operations only through their supported artifact and fleet ownership paths.

## Native follow-up status

The exclusions above describe that local aggregate, not the current availability
of all native prerequisites. Subsequent independent-member runs resolved four
of its five excluded modules without rerunning or relabeling the aggregate:

- test_unit_results: 17 passed in 7.105s against the actual built Rust helper.
- test_unit_runner_native: 3 passed in 6.217s after correcting an isolated-mode
  sibling import; actual Cargo fixtures ran under the existing protected profile.
- test_package_controller_native: 2 methods covering 4 actual cases passed in
  61.856s after supported managed staging of the authenticated Trust 15698 tree.
- test_package_cache_budget_native: 4 passed in 12.338s with the real development
  atpkg CLI and physical Trust sysroot, including default containment.

Artifacts, exact sources, initial failures and block completion evidence are in
[the native runner recovery](native-unit-runner-recovery-2026-11-07.md) and
[native cache qualification](native-cache-budget-recovery-2026-11-07.md).
These separate executions do not constitute a single all-module discovery run.
The nested test_check_lint_step profile-context deferral remains; the existing
profile was not broadened. Installed adoption and the interrupted m157 rollout
proof also remain unverified. Decision: update the summary to distinguish
historical exclusions from resolved native prerequisites, preserving the original
red and green aggregate evidence and their exact scopes.
