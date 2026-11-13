<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Account usage history follows the clock preference

## Finding and decision

The account list already uses the captured `ChatWidget.clock_format`, but
the current-account inspector still formats its weekly-history endpoints
and estimated limit time with a hard-coded 24-hour pattern. On a 12-hour
system this produces two clock formats in the same account interface.

Capture the existing clock preference when opening the account inspector.
Use its shared date/time pattern for all three history timestamps, retaining
the local time-zone suffix. Converting each timestamp to `chrono::Local`
before formatting preserves its own daylight-saving offset. The date order
also follows the shared formatter.

The header still reads the current interface snapshot each time it renders.
Capturing the preference does not capture quota data, replace the open pane,
or reset scrolling when observations change. Production code captures this
preference at startup and does not change it while the view is open.

Only the ALab-owned account implementation and its existing test constructors
change. No upstream hook, lockfile, or account-selection behavior changes.
Independent source review found no correctness issue. This is a presentation
change; existing tests are used without adding tests that repeat its formatter.

## Validation

The native TUI rebuilt with Trust's unverified lane, `--locked --offline`,
the repository's 8 MiB test stack, and four test threads. All 32 account-pane
tests passed, including the existing weekly-history, live-header, stale-data,
scrolling, refresh-ordering, and assignment cases. The rebuilt test executable
also passed all four clock-format tests. There were no failed or ignored tests
in either selection. Whitespace checks pass.

The logs are `$HOME/.aterm/.ubd/codex-account-history-clock-final-20261112.log`
and `codex-account-history-clock-formats-20261112.log` in the same directory.
These checks cover the focused change; the separate wide-suite record retains
its six fixture failures rather than representing the whole suite as passing.
