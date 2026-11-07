<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Package checks with a managed Python path containing spaces

The promotion suite exposed an invalid fixture shebang: the selector was written
as `#!<sys.executable>`, but the installed managed Python lives under Application
Support. The kernel splits the interpreter path at its space and reports ENOENT,
although the selector file exists. The publication-order test could not run.

Decision: use the quoting-safe shell/Python launcher already used by the update
fixtures for the selector, inert cache authority/driver and fsmonitor callback.
Bind the cache fixture's selected Git through a directory containing only Git,
preserving its deliberately restricted PATH without requiring Apple's Git.

The fsmonitor case also now executes its callback once and checks its protocol
output before testing suppression. An unexecutable callback can no longer make
the suppression assertion pass without testing the intended behavior. Its
control artifacts are removed before the custody operation under test.

Validation with managed Python 3.14.8 and managed Git on this Mac:

- The promotion suite changed from 1 failure to all 16 tests passing.
- The updated fsmonitor test passed, including both callback thresholds.
- All 10 package-cache budget/lifecycle tests passed against the inert native
  authority; no real compiler was used.
- The source-custody suite before the fixture edit ran 26 tests successfully,
  with its one opt-in native Trust test skipped. Only its changed fsmonitor case
  was rerun afterward; the unchanged cases' result is not a native build proof.
- `git diff --check` passed.

Apple's tools and the qualified compiler runner remain absent. A read-only
base.clt probe now also observes an Apple installer window waiting, which was
left alone. Native validation remains deferred.

The signed package selection advanced to Codex build 2026110601 under index 94.
The local package tag points to 8c20cbf8e9, but that is not proof of the artifact's
source. Post-restart inspection of alab-build.json and the live session record
identifies 5c1ea61e73, which predates the opening-exchange fix. The initial adoption
inference from the tag was wrong. See signed-codex-adoption-2026-11-06.md for the
verified identity; the greeting fix still awaits a package containing it.

## Full final-source fleet run, including the native fixture

Ran the full source-custody suite on m157 at exact source `fcc44b071e` with
managed `python3.14 -I -B alab/tests/test_package_source.py -v` and the declared
`ALAB_PACKAGE_SOURCE_NATIVE=1` opt-in. Retained fixtures live under the owned,
mode-0700 directory
`~/Library/Caches/aterm/.ubd/source-custody-s0-3b6ef368.noindex`.
No actual Codex package was built, installed or selected.

Result: **26 passed, 0 failed, 0 skipped**, in 116.158s. Owned plain shell
`s-3b6ef36885b88f39cb50` retained block 0 confirms exit 0. This closes the full
final-source gap after the quoting-safe launcher change, and executes the native
fixture that was skipped in the original run. The native case builds and runs a
tiny private app with an external path dependency through `targo --unverified`,
offline and locked, without compiler overrides or an explicit native target.
It is a cache-reuse experiment, not a qualified Codex release build.

The fixture compiler report identifies Trust 0.12.0, commit
`29ebb403de7a102850d6cb10a62d5cc3baf723ed`, on aarch64-apple-darwin. Its tiny
workspace uses the installed default Trust, independently of Codex's own pin.
Both the app and external path dependency were compiled on each first build.
On the second commit, which changes no compilation input:

| Checkout strategy | Fresh units reused | Target allocation after two builds |
| --- | ---: | ---: |
| Reuse the owned source path | 2 of 2 | 1,093,632 bytes |
| Create a new worktree path | 0 of 2 | 1,679,360 bytes |

The stable source kept the device, inode and nanosecond mtime of all four
manifest/source inputs. Every built app ran and printed `13`. The new-path
control used the same target directory for its two builds, so it demonstrates
the source-path effect rather than simply discarding a target directory.
These are one-run functional observations, with allocations scoped to the
fixture's target directory. Raw observations remain in
`package-source-winpkmet/native-cache-measurement.json` under the retained root.
The suite log is `/tmp/.ubd/codex-native-f0a4e96f.8bYgIz/source-custody-full-native.log`
on m157 (SHA-256 `3fdef48e37911e48d976190afcdf740eb21ed8c9182a48d66802830085b0185d`).
