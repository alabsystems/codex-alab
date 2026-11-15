<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Supervisor callbacks release retired runtime resources

A callback that keeps only a weak Supervisor reference must also let that owner's
runtime retire. The release, terminal-context and mail callbacks were installed in
long-lived Shared state with a separate strong Tokio Handle. Leaked test Shared
instances therefore retained retired runtimes' I/O-driver registry and waker
descriptors even after their Supervisor owners and tasks had ended.

Decision: keep one lazily initialized Handle in a Supervisor-owned OnceLock.
Each callback upgrades its existing Weak<Supervisor>, then borrows the owner's
Handle. Direct installation still binds the current runtime without requiring
Supervisor::start, as the exit-order fixtures require. A missing handle returns
false from release, None from context, and an error from mail. No new task or
strong callback ownership cycle is introduced.

The regression uses the existing exact-child native-test collector and a blocking
worker. It warms a real current-thread Tokio runtime, retains the Shared callbacks,
then constructs and shuts down 32 more runtimes. A pending task keeps each
Supervisor alive; shutdown must drop that owner. The process's descriptor count
must return exactly to baseline. Retained dead-owner callbacks must then return
their existing release/context/mail results. The crate-private native-test facade
is compiled only for tests on macOS/Linux, matching the descriptor test's scope.

Behavioral RED at `0df4823f17505a0bbd52b1b6aa363c1751693aaa`, session 61806:

- Descriptors rose from 11 to 75 after 32 shutdowns: 64 remained. All 32
  owner-live and owner-dropped controls reached the descriptor assertion.
- Dead-callback checks follow that assertion and did not execute in RED. Unit
  exit was 101; expected-RED controller exit was 0. Two empty sibling selections
  preserved the feature graph and were not counted as passes.
- Archive: `.git/alab-validation/0df4823f17505a0bbd52b1b6aa363c1751693aaa/runtime-owner-red`
  (19 indexed files); record SHA-256:
  `061b284ab45d81b5af2b7df8e1da6a0502ff3e22e174e77a4f5b696a0ad3eb06`.

The earlier `d0db964b7b` attempt failed compilation on the helper's module import
before executing tests. Its separate 12-file archive supplies no descriptor result.

At `62c30a1b63305546c9760420578e487f9fc45b7b`, session 35180, five complete
library suites passed 1,952 tests with 12 declared ignores. The ownership
regression passed, including its dead-callback checks. Overall controller exit
was still 1: Tippy's command/build succeeded, but classification found 10 owned
diagnostics. It had 1,465 canonical artifacts and four upstream diagnostics set
aside. The 32-file archive is
`.git/alab-validation/62c30a1b63305546c9760420578e487f9fc45b7b/recovery-clock-runtime-lint`;
record SHA-256:
`f3734e738e6f9bb7c0ead521aa460194d13e74aee21a925d9514ade11328b0d7`.

The lint repair makes descriptor_count return io::Result and uses try_fold to
validate every directory entry. Enumeration errors now fail inside the test.
The 32-runtime lifecycle and exact equality oracle are retained; the final helper
body is not byte-identical to RED. Production missing-handle lookups also fail
closed. No lint suppression or synthetic descriptor count was added. This fix
changes no descriptor limit, native admission, protected profile or timer budget.
A parent shell's observed soft limit of 256 is not a measured libtest limit.

Full library runtime and scoped lint at
`e3758046432f67e032b5dbaa66062ca8ab48c835`, session 97555:

- Five complete libraries passed: Aterm 223, Autopilot 998, Core 291, TUI 383 and
  Exec 74, totaling 1,969 passes with 12 exact declared ignores. All 41 required
  identities passed once. Unit, lint and controller exits were zero.
- The strict descriptor-entry regression passed, including weak-owner shutdown
  and post-FD callback checks. Seven executable identities and both host aliases
  matched the retained runtime evidence and during-run hash sample.
- Six-package all-target Tippy had 1,469 canonical artifacts, zero owned,
  spanning, failing or script diagnostics, and four upstream diagnostics set
  aside. Goal declares lib.test=false; its compiled artifacts are not a Goal
  library unit-suite pass.
- Archive: `.git/alab-validation/e3758046432f67e032b5dbaa66062ca8ab48c835/recovery-boot-clock`
  (35 independently verified indexed files); record SHA-256:
  `2f0c2462d3491db8ee566ca62a19745e2235d8348925460a9289277dc8327713`.

The later guarded goal-publication composition was checked separately at
`acd8435633ae8ef7af26437b571203eae8264d0a`, session 12107. Its 16 OR filters
kept the five-package feature graph and executed 478 tests: Aterm 7, Autopilot 296,
Core 69 and TUI 106. Exec selected zero tests and supplies no executed-test pass.
All 29 required identities passed; failures, ignores and measured cases were zero.
The runtime, controller and six-package Tippy exits were zero. Tippy again had
1,469 canonical artifacts, four upstream diagnostics set aside and no owned,
spanning, failing or script findings. The strict FD lifecycle regression remained
in this selection; its ownership and dead-callback success controls passed.
Archive: `.git/alab-validation/acd8435633ae8ef7af26437b571203eae8264d0a/recovery-latest-composition`
(33 indexed files); record SHA-256:
`e1d16c04b5b9b751a316d6c0d6b801240666419ae7ea617a1ed4705b2e6a2e47`.
The complete 1,969-test result remains attributed to e375.

The two publication Python modules separately passed 26 tests at acd, with no
network publication. Their three-file `publication-python` archive has record
SHA-256 `b3f6c88382da2776093fbf807d9bdb7ae6cd6ff8bf8aebca5a1bd11399ef366e`.

The publication composition `36fb3144fd3a0d9d87ca8f0ca0efd1ff50cf3771` adds
main `55e23f738049693b16f02fd2ff266e4c4c27ac3d`. Source review confirms the runtime
owner and generic native-test body are unchanged from acd. The peer's fixture
constructor/hook support is preserved. This is static composition evidence;
earlier runtime and lint results keep their recorded source labels.

These are runtime and test-lifetime claims. They do not establish application
certification, an accepted external actor, or the cause of an external BadRequest.
