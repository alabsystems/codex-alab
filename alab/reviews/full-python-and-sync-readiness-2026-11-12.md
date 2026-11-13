<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Close the wider Python check and restore upstream sync readiness

## Validation and fixture repair

The full Python sweep started from `1a542da9b598225cc691ffacf28775cfdc7900aa`
and ran each module in a separate isolated interpreter, with at most two
modules active. It covered 70 modules and 873 test cases. The initial run's
only failure was two setup subcases in the package-source module: independent
child fixtures produced the same commit within one second, so the parent had
nothing new to commit. No source validator was reached in those failures.

The [fixture identity repair](gitlink-fixture-identity-2026-11-12.md) gives
each child a label-specific commit message and freezes Git dates in the
existing multi-child test. Its complete 48-method module replay passed, with
the explicit native cache test skipped. Replacing only that failed module
with its complete replay leaves no failed module in the 70-module result
set. The ten skips are seven native opt-ins, two portable-Python account-boundary
cases requiring a different interpreter location, and the macOS filename
subcase. The separate native unit-runner module was excluded explicitly. These are
accounted-for results from the original run plus a replay, not a claim that
the initial uninterrupted run passed.

No production source changed for the fixture repair. The retained native
unit-runner adapter was unchanged and supplied explicitly to the protected
lint-step fixtures. This sweep is Python workflow validation, not a fresh
Rust build, native gate verdict or package adoption.

An independent source-content audit also found no false admission in its
effective-attribute controls. Failed or truncated conversion output,
malformed metadata and a blocked conversion all refused while retaining the
source and both indexes; every child process was reaped. Its evidence is
under `$HOME/.aterm/.ubd/source-content-audit-fksd2o4y.noindex`.

Raw module logs, the original failure, the corrected result map and the
commands are retained under
`$HOME/.aterm/.ubd/python-wide-after-tls-20261112-dw9wodbh.noindex`.
The manifest SHA-256 is
`5164b01687846dd6398f0400b12595934083e1566a734d3d9465662e2e38307c`.

## Repository readiness

This clone had only `origin`, no upstream refs, no local rerere settings and
none of the expected snapshot exclusions. `alab/sync.sh --status` therefore
refused before it could report a sync state. Anonymous Agh reads established
that the public upstream was reachable; this was missing local setup, not
an established credential or service failure.

Run the maintained `alab/setup.sh`, followed by its documented
`--train-rerere` mode, with a capable Python and inherited Git redirection
removed. Both runs completed successfully. The configured upstream push URL
is the refusal `NEVER-PUSH-TO-UPSTREAM`; main fetches without force, and
release tags use only `refs/upstream-tags/`. The setup fetched 1,507 release
tags, enabled rerere and the snapshot exclusions, and learned resolutions
from seven historical sync merges. Training's scratch worktree was removed.

The root checkout's HEAD, index bytes and clean working state were identical
before and after setup and training. No incoming source was merged, no build
ran, and no installed artifact or existing checkout was selected or replaced.
Before/after snapshots, commands and logs are under
`$HOME/.aterm/.ubd/upstream-setup-20261112-hkaeqch4.noindex`.
Its manifest SHA-256 is
`f7ab0d51b8f90e0a4601f45e56031a9fba107a10f114cb3c659315ab6f53011d`.

## Next upstream review

The exact fetched target is `ce3031018ef130e653ed4d9c5bb971652de97161`.
The actual integrated frontier is `c1934fc99e6b40ddff4261c910ce7733e3abea26`,
leaving 40 upstream commits absent. The earlier API comparison from the last
formal sync counted 47; seven of those commits were already integrated later.
The complete local diff has 428 paths, including 68 current hook-bearing
paths (29 in TUI) and 75 paths modified by ALab. None of the 20 committed
intake records covers a pending commit.

Next review should cover permission profiles, TUI task/backend/reconnect
ordering, MCP and auth refresh, history ownership, and transport changes.
The prepared `intake-template.json` beside the setup logs is metadata only:
its decisions are blank and it is not an admission record. The complete local
assessment is under
`/private/tmp/.ubd/alab-upstream-compare-xi45190p.noindex`.
Review and commit intake decisions before invoking the supported sync and
native gate. Readiness and a fetched ref do not establish merge compatibility
or qualification.
