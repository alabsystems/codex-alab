<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Retain observed SQLite queue-loss evidence

When the SQLite writer rejects an entry because its queue is full or closed,
enhanced ALab mode now marks the existing shared incompleteness flag. Previously
the entry was dropped and telemetry counted it while `has_write_failure()` could
remain false after later successful writes and flushes.

Production source reviewed: `e889d1445b2932c76e75982dbeb91f8799a64fad`.
No production blockers were found. The 15-line owned `log_health` module checks
the existing activation predicate and stores `true` with relaxed ordering.
The upstream change is exactly one marked two-line hook before the existing
Full/Closed reason match and telemetry. It adds no callback, logging, allocation,
timer, reset, queue behavior, flush behavior, schema, or retry-policy change.

This records observed enqueue rejection, not an end-to-end completeness or
recovery guarantee. Inactive and vanilla losses retain upstream behavior and
are not retroactively marked on reenable. An active loss remains sticky through
clones, successful later writes, and a subsequent mode change.

The arbitrary batch-failure reporter remains outside `on_event`; its existing
background SQL-insertion-failure behavior is unchanged. There is no new inline
reporter invocation, recursive log event, or UI alert. The flag can inform
existing consumers without claiming immediate notification of every loss.

## Validation

Native RED at `b96afd472a6a9d81941bc1b98b27dbade5999034` exited 101. Both
new queue-contract functions reached their real-storage and inactive/vanilla
controls, then failed only because active rejection left the flag false.

At the reviewed source, native runtime validation passed **548 tests**, with
zero failures or ignored tests: 274 `codex-alab-core`, four
`codex-alab-state-contracts`, and 270 `codex-state` tests. The two queue fixtures
exercise six real temporary databases across inactive, vanilla, and enhanced
modes, including accepted-write controls, full/closed queues, clone visibility,
later successful flushes, mode changes, and an untouched batch reporter.

All-target `--no-deps` Tippy classification for those three packages found no
owned, spanning, failing, or script diagnostics, with zero set aside. All 875
artifact paths were verified beneath the canonical Tippy child. The inventory
reports complete ownership for 86 hook topics and 13 features.

The before/after delta reports describe committed sources `b96afd472a` and `e889d1445b`.
They show exactly two added upstream lines in the existing state hook path;
the earlier precommit after-report is not used as candidate evidence. No
upstream reformat, new dependency identity, or lockfile change occurred. The
test harness enables existing Tokio `process`/`io-util` features.

Exact-source records and hashes are retained privately under
`.git/alab-validation/e889d1445b2932c76e75982dbeb91f8799a64fad/sqlite-queue-loss/`.
The reviewer checked all eight recorded artifact hashes and made no repository
edits or builds. Runtime execution used the explicit unverified lane; certified
application delivery remains deferred at the recorded source-selected Trust
15698 [procedural-macro admission boundary](targo-macro-admission-2026-11-14.md).
No package, installation, or running
application upgrade is claimed.
