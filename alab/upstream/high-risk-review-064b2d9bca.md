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

## Catalog normalization and retained origins: 33ffb6f905b9 / 44ccf394573c

Decision: retain upstream's successful-current-identity update notifications,
unsupported-setting normalization, and in-memory child catalog preservation.
Explicit supported choices must survive; unsupported effort/tier choices follow
the refreshed provider metadata. Do not copy bundled fallback into a successful
remote observation or equate a failed refresh with an empty provider catalog.

Evidence read: 33ff's manager/update diff and the child session/catalog-selector
diffs in 44cc. The ALab enhanced worker uses OnlineIfUncached, while upstream's
ModelCatalog::list_models refreshed retained originators only for Online. That
combination silently skipped originator updates in enhanced mode. The candidate
now includes both online strategies in that branch; offline reads remain local.
The owned cache-aware poll regression creates a retained Work endpoint, reads a
fresh default cache, and checks that two polls fetch Work twice and default zero
times. The fixture now uses upstream's actual startup-model factory.

Required evidence remains pending: run the new regression, child remote-model
capability tests with failed cache writes/empty responses, catalog normalization
scenarios, and account-switch invalidation. No test pass is claimed here.

## Provider continuity and selected discovery: e115d5f90d56 / daa69c411c1e

Decision: adopt saved-provider continuity and explicit model/list provider selection.
A changed default is not authority to reroute an existing thread. Keep managed
provider validation, missing-definition errors, and refusal of explicit provider
changes on resume. Temporary selected-provider catalogs retain runtime auth and
do not replace the default catalog's disk cache.

Evidence read: config-manager and turn-processor diffs in e115; catalog-processor
and thread catalog construction diffs in daa. ALab source/destination account
admission must still surround actual credentials and catalog I/O; provider
continuity is not proof of account equivalence. Required scenarios: resume after
default change, recover after restoring a missing provider, preserve a draft on
failed resume, list a nondefault provider with runtime auth, reject unknown or
managed-forbidden providers, and prove unchanged default cache. These remain
unqualified on this full candidate.
