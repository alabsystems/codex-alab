<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Share authenticated native test prerequisites with the raw ALab phase

The widest-check record left a possible cold-host prerequisite gap unproven.
A bounded same-source observation at `c8339b136d81` now establishes the missing
dependency: quarantine the actual host lookup artifacts and clear both runtime
aliases, then execute the existing native CodeMode identity case through raw
Cargo and the maintained result adapter. Its one case fails with Cargo 101 in
300.60s; the qualified retained rollout explicitly reports that the host
executable was not found. Restore the original host identity and execute the
same case from the actual failed-copy plan: one pass, Cargo 0, accepted collector.
The observer restores every moved host and finishes with no pending restoration.
Its initial 180-second harness-preparation timeout remains separate evidence;
it occurred before quarantine. This is a missing-helper control in an exclusive
warm target, not a complete from-empty gate observation.

Decision: factor the existing helper preparation in `alab/unit-tests.sh`, rather
than duplicate discovery or replace the raw phase with library-only execution.
The shared function resolves the trusted V8 pair, builds the required host/stdio
union with locked/configured Cargo, and authenticates metadata and artifacts
through the existing `native_helpers.py`. Both host aliases receive that exact
artifact. Preparation or authentication failure prevents test execution.

The raw ALab call retains its complete package union and exact default Cargo
scope: `--unverified test --locked --no-fail-fast`, without library, target or
case restrictions. An empty selection refuses before creating outputs, so it
cannot accidentally select the entire workspace. `unit_tests` retains its
library-only collection/comparison logic and the existing codex-mcp CLI helper
branch. The shared function installs no EXIT trap and changes no caller cwd;
its metadata and build subprocesses enter the declared tree. Each caller owns
its existing scratch cleanup. No upstream, manifest, lockfile, compiler,
permission, proof or application-shipping rule changes.

Four new gate-fixture methods make the fake raw Autopilot consumer actually
invoke its host and check exact scope, both conflicting inherited aliases,
preparation order, fail-closed V8/build/metadata/artifact paths, empty selection,
and caller trap/cwd preservation. Existing cases retain coverage of pristine
configuration, tree/target identity, native result publication and the MCP branch.
These fake Cargo fixtures compile no Rust and establish no application result.

The first focused draft run, original `8437`, reports five methods with one
failure in 7.138s: the new standalone helper fixture exposed dependence on the
caller's cwd. The interim direct-cd draft passes five in 7.627s (`70152`). Neither
draft is attributed the final source hashes. The final implementation scopes cd
to metadata/build subprocesses and adds an unchanged-caller-cwd assertion.

Final managed Python `-I -B` observations on base `e0bffb6d3575` plus this patch:

- Focused five-method subset: pass in 8.382s, original `25406` exits 0.
- Complete gate-fixture class: all 54 pass in 309.740s, original `13235` exits 0.
- Runner-publication class: all four pass in 12.194s, original `46449` exits 0.

All final runs have zero errors/skips. The focused subset repeats cases in the
full class; the two complete classes establish 58 distinct passing methods.
Logs are `<evidence>/codex-native-test-prerequisites-{focused,focused-corrected,focused-final,full,publication}.log`.
`codex-native-test-prerequisites-final-context.json` beside them records the base,
five source hashes and reused qualified native runner hash. It was captured
during the final focused invocation, before both full-class runs; all recorded
hashes still match after the original processes exit. Bash syntax and diff checks
pass. Independent read-only review approves the final frozen implementation.
Root verifies and retains the twelve-file fixture record under the common Git
directory's
`alab-validation/e0bffb6d3575add5e59a2a519cdea11afcea20eb/shared-prerequisite-fixtures-10f6z9lp/`,
with index SHA-256
`04952f5fa40259ecff1f45b5fd2a1dc2e2f9f922dfa963fb7a87b59364610da6`.
The original and interim draft logs retain their separate scopes there.

The exact-commit native observation now passes at
`de6c8f713b602efab68e4098c430dead1d5371eb`: the actual maintained comparison
helper replaces both conflicting inside-profile aliases with the authenticated
host and passes the original case in 1.41s. The native collector accepts one
passing copy; original command `3006` and its typed receipt exit zero. The
merged-source `8717930` composition also passes eight incoming Aterm cases and
the same Autopilot case after rebuilding its changed dependency graph.
See [the follow-up record](deferred-native-checks-2026-11-15.md) for archived
native contexts, source bindings, independent audits and retained controller
setup failure. Certification and complete gate/runtime equivalence remain
unavailable; these focused observations preserve that evidence boundary.
