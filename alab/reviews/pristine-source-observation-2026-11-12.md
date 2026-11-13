<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Validate pristine source before reuse and after checkout

## Findings and decision

At `8f1a4e47552aac9ad22f04e04f7e4e5b1c248730`, the pristine helper accepted
tracked content hidden by assume-unchanged, skip-worktree or a stale stat cache,
and executable-mode changes hidden by `core.filemode=false`. It also accepted
a deleted intent-to-add index entry even though ordinary status reported that
entry as deleted. The helper returned success for reuse and listed these trees
as clean for target cleanup. No source overwrite was observed: the incorrect
result was accepting a changed source as the immutable upstream baseline.

Fresh creation had a separate missing check. A private post-checkout hook wrote
visible tracked source changes, but `alab_pristine_at` returned the successful
Git worktree-add result without validating the resulting source. An immediate
clean check then correctly refused that tree.

Use the existing qualification worktree observer for pristine validation,
including its real-index staged comparison and fresh private comparison index.
The thin pristine adapter also checks that the observed source belongs to the
requested repository. Existing ignored build outputs remain allowed. The
observer accepts an explicit scratch parent so a pristine check inside the
compiler profile writes only to admitted TMPDIR, outside source and Git
metadata. Recursive submodule comparisons use the same scratch parent. The
default qualification scratch location and record schema are unchanged here.

Validate again after fresh checkout. Failure leaves the whole source and its
registration in place; it does not repair, reset or remove them. Moving the
TOML import into the intake CLI keeps this data-only observation path usable
with the existing system Python 3.9, while the TOML intake command still needs
Python 3.11 or newer. No new interpreter or profile authority is selected.

Private worktree refs were also accepted in the audit, but no ref loss was
observed. This change concerns source correctness and makes no new metadata
custody or authenticated byte-attestation claim.

## Validation and limits

The six-method negative control against the previous implementation reported
seven failing cases in 6.754 seconds: the hidden-source cases, deleted
intent-to-add, checkout-hook changes and system-Python-3.9 hidden edit. Clean
detection-flag controls and the contained read-only reuse control passed. That
run is recorded in the review agent's tool transcript, not a separate raw log.
The revised twelve-method
pristine suite passed in 18.280 seconds. It checks actual source, index and
ignored-output witness preservation, and refuses changed source under both
Python 3.14 and system Python 3.9.

Full module runs also passed: retention 23 tests in 31.409 seconds;
qualification 29 in 27.102 seconds; daily cleanup 13 in 23.584 seconds;
upstream intake 9 in 3.726 seconds; upstream admission 18 in 20.869 seconds.
All 39 check-lint integration tests passed in 179.357 seconds, including the
contained comparison paths. That suite used the existing Trust-built
`alab-unit-runner` (SHA-256
`eb83cda1b09cd68e5c10fae21a27ee094565560c0d4e33a61d6e23e0ea982287`)
and inert compiler fixtures; no compiler was built or invoked.

The original audit evidence is retained under
`$HOME/.aterm/.ubd/pristine-custody-audit-nbq1x0_o.noindex`.
Raw validation logs, commands and source hashes are under
`$HOME/.aterm/.ubd/pristine-validation-20261112-7tfot6yz.noindex`.
Independent read-only production review found no blocker. The tests use
private local Git repositories, text build-output witnesses, and inert compiler
fixtures. They run no Rust build, release gate, remote update or live package
selection. These are local source observations; they do not fence unrelated
concurrent writers.

## Composition with external-filter protection

The composed observer also carries the
[external-filter fix](qualification-external-filters-2026-11-12.md), including
schema 4 for qualification records. Pristine checks use those same filter
overrides with their external scratch directory.

A direct integration regression configures a filter that makes the old
`git diff --quiet HEAD` check accept substituted committed bytes. The old
pristine helper incorrectly reuses the changed source. The combined helper
refuses reuse, preserves the changed bytes, and does not invoke the filter.
Builtin text/EOL conversion and ignored build outputs remain supported.

Combined checks passed: 35 qualification tests in 45.377 seconds, 24 retention
tests in 44.941 seconds, 13 daily tests in 32.583 seconds, and the protected
pristine lint comparison in 24.707 seconds. These supplement the full 39-case
lint suite above; no new Rust compilation was needed. Whitespace checks pass.

The five selected logs are retained at
`$HOME/.aterm/.ubd/pristine-filter-composition-20261112-7lv7m91i.noindex`.
Their evidence-manifest SHA-256 is
`895b1a1d401169d3338e582fb9bdbc70b03ba5441da91c84e8ec6f329f1ee21c`.
