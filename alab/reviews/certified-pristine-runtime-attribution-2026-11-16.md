<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Certified pristine attribution is runtime-only

## Decision

`alab/check.sh` may attribute a failure to ALab or the Trust lane only after the
candidate has certified successfully and the guardian has authenticated a
failure from that exact certified `codex --version` process.

- Candidate runtime failure followed by pristine certification and runtime
  success is an ALab fault (check exit 2, sync exit 4).
- Candidate runtime failure followed by pristine certification and an
  authenticated exact-binary runtime failure is a Trust lane fault (exit 6).
- Every nonzero candidate or pristine certification status is unresolved.
  Resource status 75 and signals propagate; every other status is generic
  failure. No certification failure selects a source or lane verdict.

The pristine attempt stays inside the candidate gate's one native compiler
selection and verified containment. It uses a fresh canonical report, the
fixed proof policy, an internally derived pristine vendor configuration, and
runs the exact binary from its own certified target. Sync only maps the gate's
authenticated public 2/6 outcomes; it no longer runs an unverified pristine
fallback.

## Why certification status cannot be attributed

The removed path compared a candidate certified failure with
`targo --unverified run` on pristine. Different proof policies made that
comparison incapable of distinguishing an ALab fault from a Trust lane fault.

Using the same certified command on both trees fixes the policy mismatch but
does not make raw statuses phase evidence:

- `targo-trust/src/cli.rs` documents 1 as verification failure or inconclusive
  after compiler success, 2 as setup/internal/report failure, and 3 as
  COULD NOT RUN.
- `targo-trust/src/main.rs` also returns 1 before compiler execution for oracle
  compiler identity, retained-daemon build, host binding, and worker failures.
- `src/tools/targo/src/bin/cargo/main.rs` uses 101 both for ordinary Cargo
  compilation outcomes and inability to spawn the protected Trust child.

A saved `report.json` is explicitly observational and source-writable; it is
not the same-process live canonical capability and cannot authenticate the
phase to the shell. Therefore neither 1 nor 101, nor any other nonzero
certification status, safely proves a source failure.

## Control protocol

Private and public verdicts and both exact-binary completion receipts use
fresh unlinked regular inodes with independent fixed read/write descriptors.
Source compiler, build-script, test, and runtime children do not inherit those
descriptors. Exact bounded bytes and paired private statuses are both required;
resource stops and signals take precedence. Missing, malformed, extra, stale,
or status-mismatched bytes remain generic.

`command_guard.py --result-fd` writes its exact completion only after its owned
process group retires. It excludes the writer from the command and closes it in
the custodian. The anonymous protocol is the macOS contained boundary; Linux's
documented uncontained mode is not treated as a security boundary.

## Deferred Trust follow-up

Trust/Targo should export a live, typed, unforgeable phase receipt with disjoint
states for precompiler/setup, compiler/source failure, proof verdict, resource
stop, and completed success. When the native selected controller can relay that
receipt without exposing its authority to source, ALab can reconsider certified
compile/proof attribution. Until then, runtime-only attribution is deliberately
fail closed.

## Focused validation

The change is covered by shell/Python tests for fixed proof policy and pristine
vendor derivation, no unverified sync lane, runtime success/failure mapping,
all certification nonzero statuses, 75/signals, exact anonymous verdict
transport, descriptor collisions and non-inheritance, command completion
success/nonzero/signal results, malformed descriptor refusal, and completion
invalidation on launch/resource/observer/write failures.

## Post-landing widest gate (2026-11-17)

The canonical `alab/check.sh` ran on exact landed source
`9ee12e330bb9a41a1355aa8772b9808e4342b804` with authenticated Trust 15698,
tree `9e3ecca1051a2231f7a9ed4fce5f5904f77e5f43b0f9dd18da59204efe3040bb`.
The documented cold-private-Cargo-home preparation completed successfully for
the source and pristine dependency graphs. Tippy passed, setting aside 44
upstream diagnostics under the existing attribution rules.

The full gate then exited 1 when candidate certification returned 2 at Trust
15698's documented refusal of in-process procedural-macro dependencies. The
effective command, reconstructed from the committed scripts, remained
`alab/project-targo.sh trust build --certify` with a fresh report directory,
`alab/proof-policy.toml`, and
`--locked -p codex-cli --bin codex`; dependency verification was required. The
selected compiler commit was `20565c3255adf151f9eb920e89cdf83c38f84abe`.

The primary refusal listed 66 procedural-macro dependency units, including
`serde_derive` 1.0.228, `clap_derive` 4.5.55, and `tokio-macros` 2.7.0. Trust
rejected them at its no-proc-macro trusted-computing-base boundary because an
in-process macro shares the compiler's arguments, memory, diagnostics, and
artifact filesystem. The refusal preceded proof measurement, so no
authenticated per-function transport rows, coverage summary, or terminal
target inventory existed. Runtime and later test phases were not reached, and
the gate issued no source or Trust-lane verdict. This is the intended
fail-closed outcome for a nonzero certification status.

The retained gate log was
`/private/tmp/codex-check-9ee12e330bb9-cold-prep-r28hkz78.noindex/gate.log`
on qualified fleet host `m79-corgi.local`, SHA-256
`1d5591eda8e4b598f277c1f61c3d818b30601a5ff9b22a863a567d4ff3fd47eb`;
the refusal and missing coverage transport are at lines 1144-1145, and the
terminal no-verification diagnostic is at line 1182.
Source and `trust.lock` were unchanged. The limitation is already analyzed in
`targo-macro-admission-2026-11-14.md`; Trust 27009's later build-script
refusal is recorded in `trust27009-adoption-build-script-refusal-2026-11-16.md`.
Decision: preserve the shipping proof policy and defer a complete gate verdict
until an authenticated Trust build supports both isolated procedural macros
and required build-script execution verification.
