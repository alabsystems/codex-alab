<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Retain the adopted runtime frame through staging errors

Runtime staging consumed the sole adopted frame before fallible decoding and
terminal restoration. A failure dropped those bytes; a second public staging
call then observed absence. The actual CLI propagates its first error and exits,
so this was retained-data loss, not a demonstrated ordinary-startup bypass.
The successful Pending parser also continues to refuse ordinary resume until
all-root startup is available.

Decision: commit a successful data installation under the existing custody
mutex. Source `3685c1285ec278e0d5051fcf546b1c16b60d73c6` adds a crate-private
transaction with a pure namespace predicate and separate fallible borrowed-byte
installer. Mismatch, error or unwind preserves the same original Vec allocation;
only installer success clears custody. Existing consuming APIs stay unchanged.
The real staging caller checks Pending inside this serialized transaction,
decodes and allocates its exact retained bytes before terminal side effects,
restores startup and publishes Pending before clearing the old slot.
Existing Recovery cleanup ordering remains intact. The documented synchronous
lock order and non-reentrancy requirement apply to the installer. Byte custody
does not promise rollback of arbitrary callback side effects or grant SDK roles.

Only two ALab-owned files changed, 203 additions and 27 deletions; only those
files were formatted through source-selected Trust 15698. Wire formats, native
constructor requirements and the CLI's all-root refusal are unchanged.
Independent source review `4efd1b95…` found no blocker in the transaction,
borrowed lifetimes, panic handling, Pending publication or recovery ordering.

One maintained singleton `codex-alab-tui` invocation on exact 3685c128 passes
five distinct parent methods, with no failures or ignored methods and no sibling
libraries. Two new parents execute the existing same-PID child/lease fixture:
malformed-frame and too-late restoration errors repeat without losing the
original pointer, capacity, bytes or file lease; namespace refusal, installer
error, predicate/installer panics and success-only consumption are also checked.
Three existing controls cover kernel custody, descriptor bounds and released
runtime-frame compatibility. The receipt and frozen terminal data are explicit
local fixtures; these tests supply no genuine Native SDK or application grant.

Original73800/14042→0f69e4 closed0. Actual test, collector, coverage, source
and guardian statuses are zero. Complete before/after records match over
12,580 source inputs and eleven tools, SHA-256
`757cef407eb2bdc6da4cc3cfafa762dd7e85003a2ae80a9bb84c80f58738f745`.
Raw unit log: `e4bfdb26992bcb972f585f24ae3bef111b7541670b10a4f504305a5fc6368339`.
Independent closed-evidence audit `433a4f75…` reconciles the exact methods,
Source collector joins, immutable bindings and complete bookends.

A fresh canonical gate is warranted by the new Rust production path. The
normal executable gate on exact 3685c128, original55051/30133→2cd5e4, closes1
at owned Tippy policy. Its entire qualification identity matches before/after:
tree `c042d3a8…`, actual main base `68378009…`, integrated upstream ce303 and
clean=true. Qualification remains null, before certification, application
runtime and subsequent full suites. Closed gate log SHA-256:
`1f11434aa5344b07af63a0c64d2b422beff92966f90c803b9eac8a16cf3d3147`.

The complete closed audit `9408b502…` finds 940 source warnings: 937 dead-code
and three unused imports, zero source errors. Multiplicity-preserving normalized
path/family/message and severity Counters against 3c8c0346 add and remove zero
findings. The full heading inventory retains both locationless tool warnings
and eight compiler note annotations separately. Nothing is hidden or promoted
to a green gate.

Decision: land this tested custody repair with the failed-gate record and keep
the genuine integration backlog open. The production C1 whole-App capture to
E1 assembly is still missing; existing C2 entry/decoder callers do not supply
that producer. Empty frames, recreated Core state or fabricated Native grants
cannot substitute for the original graph and its completion witnesses.
## Merged-tree verification

Fresh authoritative main `081a4e534187ac39830ffb76dc0305699a5610fe` was
merged normally, preserving both intents, to
`dd6755147ef382a840b6b463ef76cfec058e0d38`. Both custody production files
remain byte-identical to tested 3685c128. Incoming native deadline and status
changes warranted five adjacent controls in addition to the original five.
This finite check exercises those contracts without repeating an unchanged
940-warning gate failure or representing that failure as merge qualification.

One maintained singleton invocation on exact dd675514 passes ten distinct
methods, zero failures or ignored methods, one library and no sibling libraries.
The added controls cover queued-permit physical deadlines, expired-operation
refusal, observed handoff-unavailable status, and a shifted runtime clock with
scripted host-capability refusal. FakeAterm responses remain explicit fixtures;
no genuine Native grant, certified CLI or whole-scene result is inferred.

Original12302/96901→dd0fe3 closes0. Test, collector, coverage, source and guardian
statuses are zero. The full before/after records match over 12,586 source
inputs and eleven tools, SHA-256
`72a49076d234b5345d866487e084dee7148ccd5cd66d754915f8250115da55d7`.
Raw unit log: `9b73da300b14ebf4e0976be44ac1bf4503daa21abf13bce1a12a20934c8d7dbc`.
Independent closed-evidence audit `08880f12…` reconciles exact method rows,
Source joins, forty selected bindings, immutable tools/scripts and the unchanged
original coverage block. The source-specific canonical gate failure above and
its null qualification remain unchanged. Decision: publish this bounded source
repair after a new authoritative read and expected-main check; preserve peer
work and retain the genuine integration backlog for subsequent work.

## Integrate the shared bounded frame writer

The next fresh authoritative read returned
`7c06690adb5b665dd7a0e07aa79641c00de55d1f`. Its shared bounded writer now
serves runtime-frame encoding, and its compatibility test asserts exact output
bytes. Normal merge `bd2de5292c7c0c4f42218d37565e8de739bfb72a` preserves that
change and this custody transaction. Independent source review `16f30a5f…`
confirms the entire custody file and staging transaction remain unchanged,
while the encoder and compatibility changes match incoming main.

Decision: rerun the five custody/frame controls on exact bd2de529 because the
actual encoder and one selected method changed; preserve the already tested
deadline/status intent without repeating its unchanged controls. Actual
original1655/51113→e9ad87 closes0: five passed, zero failed or ignored, one
library and no sibling libraries. Test, collector, coverage, source and guardian
statuses are zero. Complete before/after records match over 12,587 inputs and
eleven tools, SHA-256
`cec3f801e1bbe057e4ad632a93859399d99f35a498dc55e3165dc48a0e1ecf64`.
Raw log: `cba9d5155555bb9ea22a075e0540aae85a79ae53662ceea9547de25d92c0ea17`.
An upstream deprecation warning and refused peak-record write are retained;
successful execution does not mean a warning-free run. This check covers the
bounded encoder's runtime-custody integration, not exhaustive allocation/limit
errors or other editor consumers. The prior failed canonical gate remains
source-specific and supplies no certification of this merge or application.
Independent closed audit `b902d470…` also reconciles all five literal methods,
Source joins, thirty-one selected bindings and complete unchanged bookends.
