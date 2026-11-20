<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Admit the retention fixture's exact private scratch for reading

The existing contained pristine-reuse regression failed when this run's fixture
parent was moved beneath the real account home. Its direct profile call supplied
scratch as the first `alab_contain_profile` argument, and set contained `TMPDIR`
to it. That argument admits scratch writes and its own socket, while the profile's
home read fence permits only separately named roots. The private-index observer
`lib.sh` -> `pristine_state.py` -> `qualification.worktree_clean` must create and
read `scratch/alab-status-*`. A scratch under the real account home therefore
lacked read admission. Production `alab_contained` normally allocates its owned
scratch at canonical `/tmp/alab.*`, outside that home fence.

This was a runner-location assumption in the direct-profile fixture, not a prune
regression, new privacy consent requirement or resource-capacity stop. The same
case already failed with current source and a strict Git-object export of
`4a12d0b8e8dbd7650610a3b41aa908dc92707144` using the same under-home fixture
parent. Those source-bound failures remain retained in
`~/.codex/.ubd/prune-marker-custody-review-0r6ilrt4/retention-current-standalone.*`
and `retention-strict-base-standalone.*`.

Decision: in that fixture only, also name the exact first, already write-admitted
scratch as a positional root. Existing profile rules then admit reads of that
same scratch; this adds no writable path. No production library, profile
generator, HOME allowance, host grant or role changes. Using `--read scratch`
would be wrong because the later read-only fence would prohibit its writes.
Keep the existing source/index/witness snapshots and refused Git-metadata write.
Add a real `/bin/cat` read attempt against an unrelated file created inside the
owned fake HOME; the contained command must refuse that read and the host checks
the file remains unchanged. No private user file is probed.

Managed Python 3.14 ran the same single regression twice beneath one explicit
under-account-home `TMPDIR`. With the new negative control already present but
without scratch read admission, baseline failed one assertion in
1.0235594999976456 seconds, with no errors or skips. Adding only the exact
scratch admission and its comment made the case pass in 1.097296499996446
seconds, with no failures, errors or skips. The success requires both negative
controls to deny access, the pristine-reuse output to be correct, and original
source, index and retained build witness snapshots to remain unchanged.
Source HEAD stayed `4a12d0b8e8dbd7650610a3b41aa908dc92707144`; all Python-source
hashes and `lib.sh` matched before/after each run. The broader prune/retention
selection is checked separately after all Python edits are frozen.

Evidence is `~/.codex/.ubd/retention-scratch-admission-hrce187k/`, including both
phase logs, results and source bookends. Candidate log SHA-256:
`186dcbd8173e0d14ea6e31cdec67c2d4ed61b3c13c94b15815c6c9270796ae6c`.
The fixtures are owned retained Git/text worlds. These checks use actual macOS
`sandbox-exec`, the system shell/Git and selected Python; their controlled PATH
contains no Rust toolchain. No Cargo/Rust command, native project admission,
certification, package build, live account, resource-75 retry or permission waiver
ran. This kernel-fixture result does not qualify the application or resolve the
separate unavailable native-builder/capacity conditions.

The sole final combined prune/promotion/retention run subsequently passed all 68
unique methods in 23.179685084003722 seconds, with zero failures, errors, skips,
expected failures, unexpected successes or loader errors. Its source stayed at
`4a12d0b8e8dbd7650610a3b41aa908dc92707144`; all 240 Python-source and two
controller hashes matched before/after. An independent read-only audit matched
all 68 actual `ok` identities to the exact selected order and confirmed current
inputs still match. A separate agent approved this fixture change without edits.
The earlier failed 68-method result and both strict-baseline counterfactuals are
retained separately, not relabelled. Final combined evidence is
`~/.codex/.ubd/prune-marker-custody-review-0r6ilrt4/candidate-final.*`; log SHA-256:
`0e6a5037e1b4310ec76a6b2d7b20f85ffc6777769d63939c53b857de636d3bac`.
These additional checks retain the same Python/owned-fixture qualification limits.
