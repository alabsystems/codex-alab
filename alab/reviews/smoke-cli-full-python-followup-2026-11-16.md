<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Shared smoke CLI cancellation and full Python follow-up

The follow-up audit found the earlier source repairs already landed. The live
opaque-recovery review closes the installed positive-smoke deferral, and the
request-recovery integrity review closes the shared teardown repair. The native
identity-case timeout still lacks a failing trace with the added phase markers;
the later focused and full library checks passed. That conditional diagnosis is
kept separate from a demonstrated production defect. Existing application,
Trust adoption, producer and release-validation holds remain.

The useful verification gap was a full maintained Python workflow on the newer
tree. Before running it, two sibling smoke CLIs exposed the same incomplete-exit
problem already repaired in request recovery: quota-wait and weekly-reset could
write `passed: false`, `interrupted: true` and exit 0 after a successful-shaped
SystemExit. A neutral paired observation ran each original public entry and real
main with only the case callback replaced. All ten false successes change from
0 to 1 for no-code, false, zero and POSIX-wrapped integer exits. Twelve other CLI
controls and fourteen exact API cancellation identities stay unchanged. The same
frozen observer captures five inputs, including the newly imported shared helper;
its completed status is observation evidence, not a native smoke qualification.

Decision: extract the existing request guard into `cli_status.run_operation`,
and have all three CLIs call it after argument parsing. API main bodies keep
their cancellation objects and partial reports. Help and invalid selections
finish before observation accounting. Quota's exec startup remains before parsing
and the guard, preserving the PID, literal arguments and stdin EOF. Weekly's
isolated wrapper still execs before adjacent module imports. The source adds no
compatibility fallback or duplicated guard.

Test-only `c750f5e1317b1037478247068461d82f22400938` retains the landed production
bytes. Original 43773/completion e82f20 exits 1: 89 distinct methods finish,
85 pass and four have failed subtests, with 24 assertion failures and no fixture
errors. After `6318f6ae8950346ae633680ba92ee3d7c5f8d7fa`, original
53079/completion e53ff3 passes the same 89, and 78380/completion 835dc3 passes
52 weekly methods. The 141 IDs are disjoint, all five runner children exit 0,
and the frozen new tests remain unchanged. Each run binds 879 Git inputs.

The new shared helper must also belong to the runtime recorder's input set.
An owned fixture changes only that helper after an otherwise valid success and
then completes history successfully. On test-only
`c3cbe809cff027512c9a3e5af5b18f813fd479a5`, original 20730/completion 63238d
exits 1: six methods finish, four pass and two expose missing input custody.
The one-line binding fix at `fb0ea612cdd4bc33694821fb23ba672d782d61f7` passes
the unchanged six methods in original 5277/completion 14d59f. These are private
fake-application fixtures; the held application runtime gate is not invoked.

Concurrent main `49d17b516e15e171ea0bafe6fa58251b1736eb46` independently added
equivalent CLI and runtime-input repairs, plus diagnostic and history fixes.
The normal merge retains both test sets, one shared operation guard, the helper
input binding, and the stronger original-hash/history assertions. The merged
runtime fixture legitimately incorporates the other implementation's actual
helper copy and mutation subcase; the earlier frozen test claims remain bound
to their original commits. On clean `e6b07dd0b9c2c0eaa9fe43240711e22bc7627eb6`,
original 34117/completion ee150d passes all 27 selected methods: ten CLI, nine
concurrent CLI, two quota exec-wrapper and six runtime-recorder methods. All
four runner children exit 0, with 884 stable input bindings and three stable tools.

The genuine full workflow uses a freshly built, authenticated support adapter
under the source-selected Trust15698 test lane, the existing protected profile
and admitted Cargo prefix. No native substitute, optional experiment, fixture
deadline or phase-order change is permitted. The required NativeRunner3 class
must pass and retire before the remaining bodies run.

Preparation attempt one, original support56043/completion 98d177, exits 1 before
Cargo metadata or compilation: its observer hashes three private controllers,
but the protected invocation omitted a declared read for one of them. The exact
failed log, receipt, scripts and partial snapshot are retained. The V2 preparation
changes only private version references and adds that exact readonly file through
the existing protected API; no directory or profile is widened. The independent
read-closure review approves this correction. No test pass is inferred from the
failed preparation.

Support attempt two, original 44492/completion df6b44, exits 0. Its metadata,
build, helper authentication and source-check components all return 0, and the
fresh maintained helper selection authenticates the same B3 runner. The
corrected controllers and exact E6 source remain unchanged.

The complete maintained workflow then runs as original 10774/completion 43d8c6,
with actual outer, typed guardian and workflow child status 0. Its manifest
discovers 1,845 unique methods across 143 modules; every method starts and
finishes exactly once. Digest
`bef00135ab34e164a5fec305b80d96ca1e27863b2f7c4aba068f606504642c6a`
matches the final summary. Success and full scope are true; missing, unfinished,
unexpected, integrity and preflight lists are empty.

All three genuine NativeRunner methods pass first, without skips, and retire
before the remaining bodies. The remaining phase runs 1,842 cases with 1,831
successful method callbacks. Total successful method callbacks are 1,834;
eleven skip events comprise ten whole-method skips and one filesystem-dependent
skipped subtest. Eight optional native-experiment methods and two certified-hook
methods remain whole-skipped. Those outcomes do not establish executed coverage
of the skipped work. There are no failures, errors, expected failures or
unexpected successes. The selected 141-method and controlled CLI observations
are not added to the full-workflow total.

All 11,508 tracked source payloads match exact E6 Git objects and remain
unchanged before and after, together with seven distinct tool paths and three
private controllers. One tracked symlink is represented as its link bytes;
none of the captured source follows it. Independent audits reconcile raw
callbacks, source blobs, tool/script maps and actual statuses. Observer elapsed
time is 1,555.756 seconds, an observation rather than a comparison with the older,
smaller inventory.

This establishes the unchanged maintained Python workflow on E6 with genuine
native support probes in the unverified test lane. It is not an application
certificate, installed adoption, full Rust-library sweep or execution of held
release/producer work. Native environment, descriptor and context evidence stays
opaque and private. The actual application runtime gate was not invoked.

The durable native seal under common Git's
`alab-validation/e6b07dd0b9c2c0eaa9fe43240711e22bc7627eb6/`
is `smoke-cli-full-python-private-n4lzl9z1`: 34,603 payloads, 421,402,218 bytes
and 34,524 exact Git bindings. Index SHA256:
`e7301a625dd9634b733cfcb65b2d38df7195c8c3fcbe7316ed6998058e39e445`.
It preserves the successful support build and full workflow, together with the
failed V1 preparation, without relabeling that failure. The portable historical seal at the same source is
`smoke-cli-portable-history-private-j80k1lny`: 5,903 payloads, 78,861,345 bytes,
5,293 Git bindings; index
`f5e2d06526d0892aa0130cbf566eedeb2c9205811d996da04fa51d62f9405b7c`.
Both original archivers complete with actual status 0. Files are owner-only
0400 and directories 0500, with no symlinks. The native context target and
compiled helper binary are not copied. Original evidence remains intact.

Main advanced during the full run to
`ba8930758a4bc6018bf2a60fbdb0cd4178ddc9c8`, adding runtime policy alignment,
history CLI and renewal-tail lifecycle changes. A normal merge produced
`4903b25859a2fa0c3a87589d2902ed3b7d98062b`. Original 34865/completion c6e5f3
passes 168 affected methods; original 14507/completion 4d61cd passes 158 across
the three maintained smoke modules (weekly 52, history 22, renewal 84).
All 326 IDs are distinct, all fourteen runner children and both original outer
statuses are 0, with no skips or errors. Each captures 893 exact source inputs
and three stable tools. The full workflow remains bound to E6; this is a later
affected-suite result, not another complete workflow or application/native gate.
The other sessions' Rust diagnostics and routing results keep their own scope.

The later original packets are retained privately as
`codex-wide-composition-python-yo42li2p`, index
`6160dbeccfea568df98c917fce40a13ed9b345d279621b683faed8ea8a3ced9a`,
and `codex-wide-composition-python-o7q1zjv0`, index
`1cd26823c80872cdf4841c9047b164ad5a80353e03b9dd29220ffe5f49690d9f`.
No earlier selected result is added to those 326 methods or to the full total.

Final remote ancestry and retirement of only this session’s branch are recorded
separately after the maintained guarded publication.
