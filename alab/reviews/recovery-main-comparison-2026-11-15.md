<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Measure the recovery follow-ups against their starting main — 2026-11-15

Decision: compare the completed follow-ups with starting main
`702431f83ab2b1eb1b17f27bf9cb233f56d674a9`, distinguish peer imports, and fix
observed regressions without changing application authority or admission.
The comparison target is landed `5670b816f8a13e8abcc7fc3e84a92eebdaa47f76`.

Nine explicitly identified own non-merge commits touch nine paths: the new TUI
fixture and its test-module registration, the Python runner and its tests, README,
and four reviews. The entire main-to-main delta has 47 paths because it includes
peer diagnostics, guardian and quota work. That larger diff is not attributed to
this session. Own Rust changes are behind cfg(test); no existing Rust production
code, dependency, unsafe operation, compiler pin or application default changes.
The Python runner is operational tooling with intentional native-first admission.

Four byte-identical Python test modules select the same 24 method IDs on both
sources: interpreter selection, named Python results, native-timeout evidence
and pre-compiler result admission. Each source was run three times through ordinary isolated managed Python, using
paired runs with alternating pair order (AB/BA/AB). All 144 method executions pass,
without errors, failures or skips; both sources stay clean. Wall-time medians are
2.943s at base and 2.899s at the landed target. CPU was 12/12 cores busy; these are
descriptive observations, not controlled speedup or application latency claims.
The common-suite record hash is `e88788152ce09a33cc81121959eee9c4b583c124dacef52e56c423db90f3cae0`.

Full-workflow observations are deliberately not used as a speed comparison:
source702 records 1,049 case executions and a native setup failure, with all
three mandatory native methods unrun; later sources add cases,
change producer/cache context and succeed with native prerequisites first. The
previous source-c532 1,079-case result remains tied to that source. The native
fixture's approximately twelve-second cost is intentional real timer coverage,
not application overhead. Those earlier records remain unchanged.

The audit found a Linux portability regression in the added fixture. Linux
ProcessIdentity contains boot ID and start ticks, while PID is stored separately.
Distinct processes born during the same kernel tick can have identical identity
payloads. Requiring unequal payloads can therefore reject valid distinct births;
requiring unequal PIDs alone also rejects a legitimate PID-reuse/new-birth case.
This is a source-backed counterexample, not an observed failure of the prior Mac
runs, whose kernel unique IDs satisfied their assertions.

Repair `211561aa6f00913e2b5bbd3eead241993207ce55` compares the full PID and
kernel-identity tuple at both actual-process checks and between the three readers.
It preserves real kernel identity reads, dead-source/reaped-reader checks, resume
eligibility, original clocks, timer Pending observations, native wake bounds,
child custody and deadlines. A small Linux-enum control covers same-tick/different
PID, identical birth, and PID reuse with a later identity. The real process fixture
still uses actual PIDs; the enum control invents no native observation.
The first replay at source211 retains two actual passed methods and its lint
compiler status zero, but its outer zsh wrapper used the reserved variable status
for exit bookkeeping. The tool observes wrapper exit one; the controller's exit
is unavailable. The twenty-file failed-wrapper archive has record hash
`245d1a54b3a533133aa6fec84ffc0b8748436db9cf73bdc06867fddad3061066`.
It is not complete controller qualification and its outcome is not overwritten.

The current merged source `4660240293e6d453b8962078c9a95d1122f10ca1` was
replayed through the unchanged source-selected native project controller and the existing
normal command guardian's durable completion receipt. A Python launcher records
the guardian's observed exit separately from the typed supervised-controller
status. Both are zero, with exact command binding and private mode 0600. The
receipt is a local completion observation, not source authority or certification.

Both selected methods pass in 12.05 seconds, with 375 other methods filtered and
zero failures/ignores/measured methods. The three reader births differ as full
PID/identity tuples. Actual native wakes are 2,938,334 and 3,007,292 nanoseconds
past the original ten/twelve-second kernel deadlines. These are observed
correctness margins, not latency benchmarks. The Linux-enum control executes on
macOS; this does not qualify a live Linux kernel run.

Current-source all-target scoped TUI Tippy completes with zero owned, spanning,
failing-target or build-script findings and two upstream diagnostics set aside.
Both library/test profiles, actual successful build completion and 1,358 unique
canonical artifact paths are verified against the raw producer output. Both
unit/lint exits are zero, and source stays clean. The twenty-five-file archive
record hash is
`648bfe080e84a768402989e4eb4909873a3df3d25d7372c8a69164806f8ea42d`.
Independent read-only review verified all 25 final archive entries and the exact
receipt/source binding, native observations and raw lint evidence. It also verified
all 20 original-attempt files and the explicit unknown-controller-status limit.

The matched inert Python comparison exposed four quadratic list-membership
filters used to report missing and unfinished IDs. The rest of the accounting
already uses Counter/set operations. Repair
`11961f088c289975f1ba3b2d2a978bec8ae73d97` precomputes lookup sets while keeping
manifest order and duplicate-sensitive Counter checks. There is no new filter,
CLI option, skip policy, deadline, native command or environment change.

The same private probe retains ten identical semantic rows before/after (five
scenarios through two runners), including ordinary lifecycle, failure continuation
and the deliberate prerequisite policy differences. Timing probes use three
repeats per size. At 1,000 inert
remaining cases, runner median wall time changes 24.542 to 11.623ms; at 3,000 it
changes 164.765 to 30.665ms. The ordinary unittest control also varies (4.004 to
8.440ms and 15.236 to 20.329ms), with 11.989–12.000/12 cores busy. These samples
support the scoped algorithmic fix; they are not full-suite or application
benchmarks. All 21 existing orchestration controls pass on the clean repaired
source without failures/errors/skips. The 33-file private scaling archive's
comparison hash is
`fdf3b213c8ccb5a714afc823ee3d809a5e38d9854fe5519dfd45f5483f70fad2`.

Composition `a49c0b4b954e99515f3527b38056e16bf6e479de` keeps the exact native
and Python source blobs from their checked topic commits. All 21 existing Python
controls pass again without failures/errors/skips; its two-file record hash is
`e9192c52c832062672bc825a43792eb1aad20937f64e73456457f4e873090317`.
Main subsequently advances to `cdfe65abe31e6cf27ed2ea3e0028a07c283ad63a`:
release 0.9.0 metadata, documentation path spelling and one unrelated test-only
fake token change. Composition `4660240293e6d453b8962078c9a95d1122f10ca1`
preserves both own source blobs. Its orchestration and release-ledger selection
runs all 66 cases with zero failures/errors and two explicit certification-note
skips. Both skipped identities are retained; neither is a pass for application
certification. Source stays clean and actual exit is zero. The two-file record
hash is `2ca93ca6b94201c57c74562077c74cf3ea29d6f4976b75ca0e4fbb6a7052522d`.
Decision: land both measured regression fixes and their evidence record. Independent
reviews accepted the two source changes, common-case comparison, scaling archives,
composition controls and final native receipt. The source-bound checks above are
not a replacement for the unavailable certified application gate.

The previously named full-workflow and actual-process follow-ups are landed;
this turn fixes two regressions found while measuring them. Certified application
delivery remains unavailable at the unchanged Trust 15698 procedural-macro TCB
boundary. No unverified application, installation/link, provider retry, physical
host sleep or wall-clock step is claimed. The original opaque HTTP400 backend
cause remains unknown from the retained incident data.
