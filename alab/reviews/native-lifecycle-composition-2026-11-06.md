# Protected native lifecycle composition gate

Decision: qualify the current composition after main incorporated owned member
retirement and contextual registered refresh. The prior source gate at
451dc29941 predates these changes. The owned m157 checkout remained detached
and clean at `fb725e80a3d1ece0cc19792de0af804de7a7d4f8` throughout this run.

The normal executable `bash alab/check.sh` completed exit 0, recorded by retained
block 44 of owned shell `s-a845a19f856134e74492`.

- CLI build and execution passed (`codex-cli 0.0.0`).
- ALab: 1,757 passed, zero failed, four ignored across 25 target reports.
- Tippy: no ALab diagnostics; 41 upstream diagnostics set aside.
- Hooked-library tests completed. Seventeen failures persisted when rerun alone,
  and all 17 also failed alone on pristine upstream
  `3026a8e54e0f48c4d8074cd700f35b0697abc329`. They were non-gating under the
  maintained attribution rules; this is not an all-green upstream test claim.

Passing tests explicitly include exact attempted-preload retirement after work
EOF, immutable retirement metadata across EOF and waiter cancellation, refusal
to claim Closed without a runtime, and lost-preload-ACK retirement without
reusing an older registration. The existing local/remote weekly-reset compaction
regression also passed in the full hooked-library run.

The controller stayed fixed: real atpkg-publish from aterm source 08d1cd2d3b,
with the existing pinned compiler admission and protected profile, managed Python,
four build jobs and four test threads. No compiler/native-target override or
containment relaxation was introduced. The explicit unverified lane produces
no formal Trust proof claim. Optional nested-lane marker writes were refused
without changing the outer authorized lane.

Complete tee log on m157:
`/tmp/.ubd/codex-native-f0a4e96f.8bYgIz/full-gate-native-lifecycle-fb725e80a3.log`.
Size 2,460,465 bytes; SHA-256
`17068d40cdc2a9e0db6c90049b522653869adbaf047be9416a6de366ce8b5e9d`.
Retained block 46 read the whole log for counts and checksum and confirmed the
source identity and clean checkout. All gate and inspection commands finished.

Scope: this qualifies the exact client source above. The separate native
controller/consumer qualification required by the
[SourceV1 contract](../design/accounts-source-use.md) is still required before
activation. No capability mark, package publication or installed-image adoption
was performed by this check.

## Admission companion checks

After the full gate, the unchanged project-toolchain fixture suite passed all
ten methods on m157 in 1.494s (owned shell block 47, complete exit 0). Its source
remained fb725e80a3; later local commits are review-only. The fixture executes
real shell dispatch with an inert native recorder that returns 75 before any
compiler/cache work. It verifies working and historical-source selection,
private Cargo-home consistency, inherited Git redirect isolation, override
refusal and missing-authority refusal. This checks frontend ordering, not the
separately Rust-tested native authentication implementation. Full output remains
in the same remote log directory as project-toolchain-postgate.log.

The unchanged capacity-result admission suite also passed all 11 methods locally
under managed Python in 0.484s. Independent adversarial producer processes
exercise the real result reader, and fixture assertions require that producers
ran and were reaped. No Codex, compiler or server is started. Its full output is
retained privately as capacity-admission-postgate-2026-11-06.log.
Decision: retain both admission implementations; these checks found no new defect.

## Retention and package-copy companions

The unchanged retention suite passed all 16 methods on m157 in 8.667s (owned
shell s-cac9139e17a5f6b2ab4b, block 0, complete exit 0, source fb725e80a3).
Coverage includes clone/worktree preservation, unknown-policy refusal to clean,
exact upstream-revision reuse, occupied or dirty destination refusal, retained
failed reservations and contained scratch cleanup. Full output remains in the
same remote log directory as retention-postgate.log. The tests retain their
recorded evidence rather than running deletion of protected fixture trees.

The unchanged package-copy suite also passed all eight methods under managed
Python locally. It exercises real copy helpers with inert files, checking binary
bytes and execute modes, root/descendant links and special-file refusal, raced
stat/open substitution, root rename, overlapping staging and a growing source
file. Failed copies survive the normal cleanup hook. Output is retained privately
as package-copy-postgate-2026-11-06.log. Decision: retain the current retention
and copy implementations; these checks found no new defect. No package was
published and no compiler ran during these companions.
