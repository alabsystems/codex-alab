<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->

# Recheck usage immediately before consuming an earned reset

Credit listing and durable custody writes can make the earlier usage response
stale. Every consumption POST now follows another authenticated usage GET and
checks the same owner, identity generation and reset cycle. One ordinary
five-hour or weekly window must still have finite utilization of at least 99%
and a future refresh no more than 24 hours away. Reactive recovery additionally
requires ordinary usage to remain denied. Preflight may still redeem at 99%
while ordinary usage is permitted.

A failed or ineligible read sends no new POST. An earlier uncertain POST may
already have consumed its credit; its original key, receipt and custody remain
retained. This client check is not an atomic server transaction.

The old production control at `252eb621cc2e` failed: all four actual loopback
cases posted after credit listing changed utilization to 98% or moved the
refresh beyond 24 hours. The repaired regression also checks restored allowance
at 99%: recovery sends no POST, while eligible preflight remains usable.

At current-main composition `b6f3305c564342090717b44510af91735fb42362`, all
67 quota unit tests passed with no ignores. All-target, package-only Tippy
passed with `-D warnings`. The commands used the unchanged source-pinned Trust
15698 toolchain, `--locked --offline`, the private task Cargo home and the
normal four-CPU build slot. The test command explicitly used `--unverified`;
these are development checks, with no proof claim. Exact commands and hashes
are in `$HOME/.ubd/codex-quota-prepost-publish-controls-s12.json`.

The actual development executable built from `cf450114caf7` passed all 33
application quota cases in one run, with no guardian timeouts. Its SHA-256 is
`6d4af8c795730d602989da7bac7d862ff3ea7ce7ef66cedb677ae8ba30c9c7b0`.
The result is `$HOME/.ubd/qp11.noindex/result.json`; authentication and servers
were isolated fake credentials and loopback. This older source cut has the
same repaired quota implementation; it is not an application-build result for
the later current-main composition.

Two earlier application runs remain failed reports. Their old assertions
expected two usage GETs where the fix correctly adds a third after listing.
The revised assertions retain the original 29-second polling requirement and
also require the fresh read strictly between listing and consumption. Backend
propagation now depends on 60 seconds since actual consumption, so extra policy
reads cannot shorten the simulated delay. All ten fixture controls pass,
including repeated immediate reads and both sides of that time boundary.

An independent runtime check of the same executable created 260 completed
turns through the app server, resumed that persisted thread in a PTY, loaded
five older-history pages and navigated at widths 38 and 76. The oldest and
latest turn markers rendered; the child exited zero and was reaped. This
checks current pagination behavior without claiming reproduction of the old
crash. The scoped evidence is
`$HOME/.ubd/codex-history-pty-runtime-handoff-s2.json`.

The Rust change and fixture changes received independent bounded source
review. No certified application build, full repository gate, package
installation or public binary promotion is claimed. Shipping still requires
the live certified build and native publication/admission boundary.
