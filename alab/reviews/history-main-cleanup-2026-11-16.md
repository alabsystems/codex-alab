<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Own history teardown through its final verdict — 2026-11-16

Decision: history main owns a successfully transferred listener inside its
observation guard and uses the existing shared `cleanup_resources` authority.
Every later safe cleanup and HTTP/identity export gets its attempt. Only the
join following a failed shutdown is skipped. The final result is serialized
after cleanup has selected the authoritative failure and adjudicated its state.

At source `ba8930758a4bc6018bf2a60fbdb0cd4178ddc9c8`, bounded public-main controls
reproduce the earlier audit: a shutdown error replaces body cancellation and
skips close and both exports; an HTTP export error replaces cancellation and
skips the final report. The initial nine new methods expose 37 failing subcases.
The controls run actual main and its original CLI entry with inert listener,
RPC and process boundaries; attempted application processes and PTY forks fail
the fixture. Shared fixture source remains unchanged.

State and report helpers now exist before acquisition. An empty listener slot
becomes the single returned pair inside the guard, so cancellation before
unpacking still closes that owned pair. A factory failure saves failed state
without invoking shutdown on an absent listener. Failures internal to the
factory remain that factory's separate responsibility.

Body and cleanup cancellation keep their exact exception objects, including
when the final save fails. Genuine cleanup cancellation retains the shared
helper's precedence over an ordinary body error. Ordinary body and cleanup
failures fail the verdict and cannot reach the final success packet. The original
binary identity check remains after listener cleanup as an independent export, so an
HTTP export failure cannot prevent that check or subsequently restore success.

Both publication boundaries are guarded. Interrupting a healthy result write
gets one bounded failed correction; cancellation while emitting final success
marks state failed and attempts its corrected save. The selected API exception
stays exact even if correction fails. A failed correction can leave an earlier
healthy file: the API does not promise every failed write replaced that file.
The original CLI still exits nonzero, so the runtime consumer's exit-plus-final
JSON conjunction cannot qualify that incomplete observation.

Eleven new methods cover acquisition and immediate post-transfer cancellation,
each teardown/export phase, failure priority, before/after-write faults, late
success emission, bounded corrections and original-entry status. All eleven
pass with managed Python 3.14 (`-I -B`); the combined run with nine existing
history CLI and eight runtime methods passes 28 methods. The guardian and early
startup prefix is byte-for-byte unchanged. `git diff --check` passes.

Root reviewed and approved the acquisition guard and publication limits.
Independent managed-Python review passes all 20 focused methods (11 new main
and nine existing CLI), without skips, and found no production or test blocker.

Private audit and corrected witnesses are retained under
`~/.codex/.ubd/history-cleanup-audit-20261116` and
`~/.codex/.ubd/history-main-cleanup-20261116`. No native application, actual PTY
child, provider account, compiler, package or product qualification was run.

The root's private composition at the exact implementation commit
`9dc614f4a958befcddbd0ca17ba4b0a9e439e9bf` passes all 320 selected Python
methods in 25.897 seconds, with zero failures, errors or skips. This includes
all 158 maintained smoke methods and the new cleanup controls. Source stays
clean and unchanged. Separate discovery-only observation finds 1,868 unique
IDs, no discovery errors and all eleven new history identities. These remain
selected checks and discovery, without a full native-gated workflow claim.
