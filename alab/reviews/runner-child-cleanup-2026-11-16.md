<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep shared-runner failures through child cleanup

Decision: preserve the runner's active body error through guardian stop, bounded
retirement observation and independent safe pipe closes. This supersedes the
child-cleanup precedence limitation deliberately retained in the earlier signal
ownership topic. Handler restoration still runs after child cleanup and preserves
that phase's selected primary.

At `26b87abc26`, inert public-run controls showed that terminate/wait failures
replaced interaction cancellation and skipped both owned streams. A wait could
publish a return code and then raise, yet safe pipe closes were still skipped.
Close cancellation replaced the primary and could skip the later close; ordinary
close OSError was silently ignored, allowing a healthy run to return zero. Eight
initial regression methods exposed 35 failing subtests against the original
runner. The healthy interaction/receipt control passed already. An independent
24-case inert audit also observed the masking, skipped streams and unbounded
cleanup wait, using actual-close-before-error regular-file wrappers.

The runner requests stop only through its existing owned Popen.terminate. An
accepted stop or ProcessLookupError permits one cleanup wait with a separately
snapshotted five-second bound. Other stop refusals skip that wait. One owned
nonblocking poll may observe retirement if returncode remains absent. Status is
checked again after wait/poll errors, since either operation may have already
published it. No raw PID/group signal, guardian kill, communicate fallback or
new authority is introduced. Normal command timeout, command construction,
interaction flow and guardian receipt validation remain unchanged.

If retirement is confirmed, each buffered pipe gets its own close attempt. Every
failure remains evidence and an active body exception keeps its exact identity.
Without an active primary, child-cleanup failure prevents a successful return;
ordinary/genuine control identity and success-shaped SystemExit normalization
reuse the existing local policy. The selected primary is recorded before all
saved signal handlers are independently restored.

If retirement is unconfirmed, cleanup records that failure and explicitly skips
both buffered closes. stdin close can flush into a live nonreading guardian;
either buffered stream can wait on a generic callback's IO lock. Skipping them
keeps this refusal path bounded without numeric-FD/double-close tricks or a pipe
supervisor. It leaves unresolved ownership, not a released-resource claim. The
guardian retains its original command and group authority; this code does not
claim that refusal or timeout retired its child or writers.

Ten new methods cover 42 finite controls with inert Popen objects and owned real
regular handles: stop refusal, ProcessLookup compatibility, bounded wait, state
publication before wait/poll errors, independent closes, healthy cleanup refusal,
normal output/raw status and body/child failure combined with signal-restoration
failure. Fixture fallback closes only its own regular handles after observations.
Forbidden escalation mocks are asserted uncalled after run; explicit recorded
calls independently prove one bounded wait and no wait after refused stop, so a
caught test AssertionError cannot hide those violations.

All 39 selected methods pass without skips on managed Python 3.14 and system
Python 3.9 with `-I -B`: ten new child-cleanup methods, eight signal-ownership
methods, five capacity methods and sixteen package-reader methods.
`git diff --check` passes. These controls launch no child process, application,
native guardian, socket, PTY, provider or compiler. They establish Python cleanup
and refusal behavior, not native retirement, application qualification or build
certification. Existing input closures already bind the modified capacity file.

Root composition `5490bcdd70336189646d3b6117ed0ba982fba7f1` preserves concurrent
main's auth-bootstrap coverage and passes all 425 selected Python methods in
29.171 seconds on managed Python 3.14 with `-I -B`, without failures, errors or
skips. The exact source remains clean and unchanged. The selection includes four
existing finite process-tree methods and nine runner-interaction methods, plus
the ten new cleanup methods and the two named pre-promote fixture controls.
Discovery alone finds 1,968 unique IDs, no discovery errors and all ten new child
cleanup identities, without executing any test body or native prerequisite.
These selected observations do not replace native-required app/build qualification.

Both independent reviewers passed the same 39 methods on both Python versions
and approved the frozen source, fixture guards and stated limits. Root reviewed
the production policy and note and approved these three paths for composition.
