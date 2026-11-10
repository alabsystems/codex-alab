<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# High-risk intake notes for 064b2d9bca

This is a partial semantic review, not range-wide approval or qualification.
Base is `1ad8b4f8ec`; target is `064b2d9bca2b5ba2f9f399ffefd06a75d97852f0`.
The complete pending range has 395 commits. A local machine-readable snapshot is
at `${HOME}/.ubd/codex-upstream-064b2d9bca-intake.json`; recreate it with
`upstream_intake.py snapshot` when the refs change. Its decisions remain pending.

## Fresh-session model defaults: 4b94e6cf260e

Decision: adapt ALab startup glue to upstream's server-owned bootstrap. Preserve
explicit profile/launch choices and destination ownership. Do not restore the
old embedded-only implicit local model, which can hide newly available models.

Evidence read: commit rationale and TUI startup/bootstrap diff. Upstream extends
server defaults to embedded connections, makes config/read mandatory for fresh
startup, and prefetches the catalog with hooks/list. The diff removes
FreshStartupDefaults and changes the startup_model argument from whether defaults
were read to whether this is a fresh start. ALab hooks using the removed wrapper
or assuming the old bootstrap condition need semantic adaptation, not just compile
fixes. Remaining diff and affected ALab glue require review at resolution time.

Required scenarios: fresh embedded/remote/daemon start uses advertised model and
reasoning defaults; explicit profile and launch model survive; config/read failure
cleans up backends; trust startup fetches one catalog; destination account/config
assignment retains its own current catalog rather than a stale source snapshot.
No scenario has been run against this candidate yet.

## Auth ownership after async budgets: 6bd17ba6ae6c

Decision: adopt upstream current-owner revalidation and retain ALab account
assignment/resource fencing. Upstream ownership and ALab destination publication
are related but do not yet prove equivalent behavior.

Evidence read: client.rs diff and added HTTP/WebSocket runtime-auth tests. HTTP
request setup restarts after an awaited budget check if the retained auth owner
is stale. WebSocket setup uses owner.is_current(), replacing equality against the
client's owner. Regression coverage changes credentials while setup is paused,
and covers both dual-header and assertion-only runtime auth.

Required scenarios: switch accounts during a budget await; externally replace
credentials before WebSocket reuse; validate request header/assertion identity;
retain ALab no-cross-account background reads and destination publication fences.
Run upstream regressions plus ALab account contracts after the runtime-auth series
is integrated. Do not retire ALab ownership logic from this diff alone.

## Local-compaction retry policy: d3147716e071

Decision: adapt ALab compaction recovery to preserve upstream error admission,
retry limit and context-window trimming while retaining provider deadlines,
host-sleep accounting and verified weekly-reset recovery.

Evidence read: compact.rs diff and new manual compaction regression. Upstream
replaces broad retries with retry_delay admission, preserves RetryAfter's original
deadline, and returns terminal errors. Invalid requests and unadvised overloads
must not enter a generic recovery loop. ALab weekly-reset recovery still needs
its explicit verified eligibility path, not a broad exception to terminal policy.

Required scenarios: invalid input and unadvised overload send once; advised overload
retries within configured limits; reporting delays and host sleep do not extend
provider deadlines; cancellation ends waits; foreground earned-reset recovery
retries the same compaction only after verified redemption. These are pending.
