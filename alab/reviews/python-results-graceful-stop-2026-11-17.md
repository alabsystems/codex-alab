<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# A graceful unittest stop cannot produce complete successful evidence

The maintained Python result CLI discovered two owned tests. The first, running
inside an owned CLI child, installed unittest's SIGINT handler, sent SIGINT to
its own process and passed. The
second selected test, which would fail, never ran. unittest returned normally
with `wasSuccessful()` true and `shouldStop` true. The CLI nevertheless exited
0 and recorded `completed: true`, one passed test and `exit_status: 0`. Both
`passed_tests` and the real feature-evidence consumer accepted that captured
record. This was observed on source
`53ea901e3889cbc17053f6eb775e3bc57c2e85c8`, with unchanged source bookends.

Decision: derive the producer's existing completion field from
`not result.shouldStop`. The existing validator already rejects an incomplete
record and makes its exit status 1, so the CLI and feature consumer refuse it
without a schema change, new fallback or duplicated workflow controller. Keep
completed test rows and captured output as diagnostics. A stop after the final
case also remains incomplete. Ordinary failed tests and class/module fixture
errors retain completed failed records; abrupt cancellation handling is
unchanged.

Four new maintained methods cover API stops before a remaining test and after
the last test, an actual CLI child handling its own SIGINT plus a healthy
two-test control, and rejection through the existing feature consumer even
when its outer command status is incorrectly reported as success. The stopped
case proves that the second body did not run; each retained started row still
finishes and reports its actual outcome. An existing fixture-failure test now
also asserts that its failed record is completed.

Managed Python 3.14.8 ran the same final tests on the old production code:
actual exit 1, four assertion failures across four distinct methods, zero
errors/skips. With the one-line production change, five complete affected
modules passed all 73 distinct methods: Python results, Python result CLI,
feature evidence, feature gate and upstream gate. Actual exit 0 in 22.034
seconds; zero failures, errors, skips, expected failures or unexpected
successes. The four new methods are included in the 73 and are not additive.
HEAD and all 240 ALab Python input hashes matched before and after each run;
the only baseline-to-candidate Python change was the producer line. All 73
actual verbose OK records match the unique selected IDs. Owned fixture scratch
was empty afterward. `git diff --check` passed.

The original public reproduction, consumer acceptance and copied-source
experiment remain under `~/.codex/.ubd/python-evidence-stop-yw5a9lql/`, alongside
the maintained `baseline.*` and `candidate.*` logs, result records, source
bookends and exact selected IDs. Production SHA-256:
`94135009a62a9e5fffc78bf0f0702fa40096626a9d045aec62122e19a5ae7a49`.
Final test hashes are
`ae42298f895f205a1ca246c8e6babcc65d98bf9b28a9bf8894cb439608eff18a`
and `85ad48f4c917912ff22f24a0d12cf573f506cd0103529a614a44a81e812e47b7`.
Full-run log SHA-256:
`f2a2ced1b8eeda60b67f43605aa464977f7f776a0224a5657debba20b94af939`.

This validates Python execution and local Git/evidence fixtures. No Rust
compilation, native unit runner, certified gate, package build or reset redemption
ran. The existing authenticated resource and publisher refusals remain in
force; this evidence does not replace those unavailable operations.
