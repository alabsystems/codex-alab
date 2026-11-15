<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# SQLite contract child custody review

Reviewed frozen test-only commit `b356e4f9e21fe9f742f8ec9566ff741253e3a579`
against `fd86643f6219b57c7305094fb32b99c1b91b5c73`. No blocking source
finding. Independent source review and the native runtime results below are separate evidence.

The diff touches only five ALab-owned test sources: a shared 71-line process
helper, its test-only module registration, and the three callers. There are no
production, hook, manifest, dependency, or filter changes. Endpoint and recovery
child bodies are byte-identical beyond their launcher branches. Queue case
bodies and assertions are unchanged, and both queue test names/dispatches remain
unchanged. All four public test names and activation placement are preserved.

The helper derives the exact filter from the caller's module path and test name,
checks the existing internal sentinel against that exact name, and returns the
child branch before constructing its wait runtime. Thus synchronous child bodies
continue to own and destroy their runtimes without parent-runtime nesting. A
tracked-source search finds no other consumers of the three sentinel keys.

Parent custody retains the child handle while concurrently waiting and reading
both bounded pipes. Its 30-second deadline and five-second kill/reap deadline,
with kill-on-drop fallback, cover wait/capture failure and cancellation. Normal
success requires both a successful process status and `1 passed; 0 failed`, so a
wrong exact filter selecting zero tests does not pass. Output beyond the 64 KiB
per-pipe limit may cause bounded failure rather than unbounded capture; it does
not create a success path. This is child-process custody, not a process-tree
supervisor; these test children do not spawn subprocesses.

The decision is to reuse the existing bounded queue-test custody for the two
older SQLite parents that called unbounded `std::process::Command::output()`.
This expected-green change adds no artificial RED or new implementation-mirroring
tests. At the reviewed source, all four existing state-contract tests pass,
with zero failures or ignored tests. Scoped all-target Tippy reports zero
diagnostics, all 809 artifact paths use the separate canonical lint cache, and
ownership remains complete for 86 hook topics and 13 features. Exact-source
records and hashes remain in the private common Git validation archive.

The passing run exercises ordinary child completion; it does not claim a newly
executed timeout or fault-injection control. Independent review compared the
preserved child bodies and inspected deadline/capture/cleanup control flow.
Prior 548 core/state results concern the preceding production source and are
not claimed as a rerun here. No production code changed. Runtime tests use the
explicitly unverified lane under source-selected native execution; application
delivery remains deferred at the [Targo macro-admission boundary](targo-macro-admission-2026-11-14.md).
