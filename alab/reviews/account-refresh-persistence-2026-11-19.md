<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Keep the source credential lock until the file writer finishes

An ordinary OAuth refresh sends its request under the source login’s file lock.
After the issuer answers, Codex runs the compare-and-save on a blocking worker.
Cancelling the async caller leaves an already running worker alive. Previously
this released the pool’s file lock while the worker still owned the separate
storage write lock and could still replace the credential file. The worker now
shares the original file and account activity leases with its caller.

Two protected `login-reader` tests use a loopback issuer and a privately
created file with synthetic credentials. They block the real compare-and-save
with its commit sidecar, observe the storage worker holding `auth.lock`, and
probe the pool’s credential lock through a separate file descriptor. The second
test cancels and joins the caller before probing again. After the sidecar is
released, the next pool lock holder must read the single issuer exchange’s
updated credential from the original file. On the uncorrected parent behavior,
the ordinary refresh passed and the cancellation case found the pool lock free.

Account-isolation scenario 2 also requires a supported assignment. The
installed host owns enrollment, managed refresh and restart. Full
qualification still needs an isolated host that carries the restart across A’s
pending refresh and observes B’s first authenticated read. [Assignment lifecycle](../design/accounts-assignment.md)

### Assignment during an outstanding refresh

On fork source `c1b7d8fd75`, the two-account lost-login fixture comes closest.
Its successor must answer A’s original turn under B. The issuer immediately
refuses A’s refresh, and the host must wait at least a minute after its own
refusal before declaring the move ready. The fixture rejects a move requested
before the login was lost. The ordinary-renewal fixture can hold a successful
issuer response open, but it creates a separate app-server process for each
slot before either refresh starts. The legacy picker fixture records and
refuses each restart request. [Lost-login verdict](../smoke/renewal_follow.py#L1068) · [Ordering controls](../smoke/renewal_follow_test.py#L2785) · [Overlapping slots](../smoke/ordinary_account_renewal.py#L146) · [Legacy host](../smoke/account_switch_legacy.py#L82)

The native host’s private package test observes a replacement PID on the same
conversation using plain `restart pid=…`. Its separate account-restart test
verifies the selected account stamp and the predecessor’s exit. The remaining
case needs one private session where the ordinary host receives a named
assignment while A’s refresh is outstanding. Record the source credential
file through completion or cancellation and B’s first authenticated read on
the resumed conversation. Repeat with the source owner unavailable and count
issuer requests and credential writes. Scenario 2 remains pending.
[Private package restart](https://github.com/alabsystems/aterm/blob/f31a52ae6c7a984c7b7fffe0bd8040ccd60e76f4/crates/aterm-lab/src/codex_alab_restart_proof.rs#L177) · [Account restart](https://github.com/alabsystems/aterm/blob/f31a52ae6c7a984c7b7fffe0bd8040ccd60e76f4/crates/aterm-gui/src/restart_account_tests.rs#L129)

## Exact-source results

| Source | Protected login result | Native log SHA-256 |
| --- | --- | --- |
| Original behavior with both new tests, `13db4eacb9` | Ordinary refresh passed. Cancellation exposed the free source lock while the write was active. | `b92e01ec93c961c8977f2e28b2a568f0120f33ac51bd192e45f7b218e2895df9` |
| Corrected fork source, `9eaaab58d8` | All five tests passed, including rotation and unavailable-owner controls. | `96e3be11492eb7b2d805bbddff1d352688d921e7a0df17f8a067af3ce16779fd` |
| Corrected 0.170 comparison, `dbd4e1f854` | The same five tests passed on the reviewed 0.170 base with the fix applied. | `e60fb25a55491483bd7d3a34ac2fb249adb8556c4094ced26c2e6b946e0cf810` |

The comparison uses the corrected collector's `login-reader` stage with
the comparison source's own dependencies. The strict result reader rejected
a nonexistent test name on both sources. The local three-source receipt
has SHA-256 `c04cfdd257d05ccdd5b675a1d1cb9d943666bde8ef42f35b77fae454b9c45f43`.
[Preserved 0.170 comparison](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-dbd4e1f854)

## Resuming with the corrected CLI

The protected execution-wire stage on `dbd4e1f854` passed all four
checks and produced CLI SHA-256
`6ea0200a145bc45156333f6caacbe510b3279f05323a70aecd09772e3931014f`.
Signed 0.11.0 created an exec conversation under A. With A’s daemon and
its observer still connected, B launched literal `codex resume` from a
different project. Both conversations appeared immediately. Selecting
A’s thread restored its working directory and tool output, and B
authenticated the two model requests that executed and returned the
working-directory check. A’s daemon retained its original account
and observer throughout. It released its socket at the end of the test.
The raw runtime result has SHA-256
`b8570a56ed6b3944b3f4daae2f926ba84a607c9ceaa93b79df0ee096c344f195`.
[Interactive receiver checks](managed-daemon-account-resume-2026-11-19.md)

## Vanilla comparison on the corrected CLI

The same `dbd4e1f854` executable passed all 18 checks against the official
macOS ARM64 OpenAI Codex 0.170.0 binary. Across 77 help pages, the
comparator attributed two differences to ALab and four to subsequent
upstream development. For 26,145 shared app-server schema entries, it
attributed 174 differences to ALab and 70 to upstream. Both executables
emitted the same four exec event types and the same 20 event field paths.
All check outcomes and the full difference list matched the reconciled
`bbd706f710` result byte for byte after the three run-path header lines.
The comparison receipt SHA-256 is
`5dab4080e6d7c141264deb8dc4b02c3b60a8a39d944e55a7ad179dd5f34c47f1`.

## Packaging the same source

The private no-promotion package command selected Trust 15698 from this
source and requested dependency-inclusive certification. The compiler
refused the CLI's 66 in-process procedural macros and returned one unknown
obligation, zero proved and zero failed. The certification gate exited 2,
the package command exited 1, and the output directory contained no package.
The stage had sufficient disk. The package log has SHA-256
`a32d060d64b9d115965edbf553fdc46b7d7e2d789109ed5c4a20302f5e2f6808`.
The compiler report has SHA-256
`772df4ea85094bf0a9c4131e758b1d34a2458d07b20a3514a5140128db2dcc5a`.
