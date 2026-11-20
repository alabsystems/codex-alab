<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Report an exited startup leader without reaping it

The failed 105-method updater run spent three one-minute hang detectors waiting
for builder witnesses after the public updater had already refused the 20 GiB
disk preflight. The intended writer lifecycle assertions were never reached.

Decision: change only the fixture's initial builder-start wait. Check the witness
first, then observe the child with `waitid(P_PID, WEXITED | WNOWAIT | WNOHANG)`.
If it exited, fail with raw status and an advisory journal reason, read through
the existing bounded, nonblocking, no-follow metadata reader and clipped to
1,024 characters. Keep the leader unreaped for the existing process-group
cleanup. Later cancellation waits intentionally expect exit and stay unchanged.
No production updater or admission policy changes.

Two actual owned Python leaders verify the helper: one exits 23 before creating
a witness, remains observable through repeated `WNOWAIT` calls with
`Popen.returncode` unset, and is subsequently reaped as 23. The other writes its
witness and stays alive on an owned pipe until ordinary group cleanup. An initial
five-method run recorded four passes and one cleanup error: macOS refused the
signal to the already-exited group. The negative control now catches the same
missing/permission errors as the existing fixture, without retrying, and still
requires the actual final status 23. That original failed run remains retained.

The final targeted five methods pass in 2.634583 seconds, with no failures,
errors or skips. Separately, the three real resource-refused methods remain hard
failures, now in 1.186642 seconds total (0.393, 0.386 and 0.385 seconds). Their
retained journals still report the actual 20 GiB floor before allocation, with
builder/controller/selector witnesses absent. This measures faster diagnosis,
not successful lifecycle coverage, and does not make the original 105 run green.
The other deferred controller bodies remain unverified.

HEAD and all 398 Python plus 12 shell hashes matched before/after both final
phases and root's review. Removing the three added methods and reversing the
one startup call reproduces the original module byte for byte; later waits and
real cleanup are unchanged. Independent source and evidence reviews approved
the change. No full-suite, native application, compiler or package check ran.

Evidence: `~/.codex/.ubd/startup-wait-latency-5nz0nq_e/`, retaining all outcomes,
kernel observations, source bookends, reviews and verified fixture archives.
Focused-pass log SHA-256:
`d8ea2d5ac1ad199cf96f1105817b7d40e83eb9115acec55402d3d3f1aa4f069e`.
Resource-failure log SHA-256:
`25740393a51e9fc27746bbad7298cff7d2b906a565d894570c3e15be368f41db`.
