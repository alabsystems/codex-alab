<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Visible earned-reset decisions and the 48-hour policy

The owner requested Sol 6.1 ultra in both local settings and packaged defaults,
and a visible decision before stopping at a usage wall despite earned resets.
Local `config.toml` now selects `gpt-6.1-sol` with `ultra`; packaged defaults were
committed separately in `87023d47a7`. Explicit user/profile selections still win.

Interpret the owner's 48-hour threshold as time until the weekly allowance
refreshes. Automatic redemption requires the same authenticated ordinary weekly
window to be at least 99% used with its natural refresh strictly more than 48
hours away. Preserve credits at exactly 48 hours, nearer refreshes and five-hour
walls alone, explaining that decision. Pending-operation retries and the final
read immediately before POST obey the same policy. Existing account custody,
idempotency, earliest-expiry selection and confirmed-restoration checks remain.

The root sampler and compaction paths forward reset decisions immediately as
transcript-visible warnings. Messages show the backend's available count and
expiration timestamps, automatic selection or a refusal reason, and redemption
and restoration outcomes. Unread inventory stays unknown. A pending operation
explains why inventory is not re-listed; its immutable key stays in custody.
Unchanged notices are deduplicated within a turn. No account or credit IDs are
printed. Final notices drain before the operation returns.

The printed notices are live presentation, not durable thread items. Upstream's
`rollout/src/policy.rs` excludes `EventMsg::Warning`, and its history projection
does not rebuild warnings. Reopening a saved thread therefore does not restore
these rows; the independent redemption ledger still retains custody and receipts.
The existing extension item envelope is a closed enum for image generation,
sleep and web search, with typed app-server wrappers. It offers no generic notice
item. Adding faithful durable notices requires a new display schema and the
corresponding history and presentation support; a synthetic agent message would
misrepresent the program's accounting decision. No such protocol change is made
by this work.

New behavior lives in ALab-owned files. The single upstream change calls the
ALab visibility predicate for reset warnings; ordinary warnings retain upstream
behavior. Hook preservation against `origin/main`, including the working diff,
retained all 94 topics; the weekly-reset topic gained one marker. Risk ranking
could not run because this clone has no `upstream` remote; the one-line upstream
diff was reviewed directly.

## Validation and unavailable execution

- All 17 Python weekly-reset smoke-fixture tests passed, including strict
  48-hour boundaries and rejection of five-hour-only redemption.
- Changed TOML parses, local model/effort values were checked without exposing
  credentials, changed Rust parses with `trustfmt`, and `git diff --check` passes.
- Added Rust HTTP regressions for visible inventory/expiry on preservation,
  earliest-expiry redemption and restored allowance, unread inventory, quiet
  healthy preflight, and crossing the policy boundary during credit listing.
  Compaction/sampling tests assert delivery of inventory, decision and final
  restoration through the real event channel. Forwarding tests cover both
  pending operations and messages queued as the operation completes.
- Rust tests, tippy, full gate and binary smoke were not run. Source-selected
  Trust build 15698 is missing locally. Its native atpkg installation repeatedly
  reported HTTPS no-progress timeouts. Shared staging also had concurrent
  download processes and grew beyond the expected archive size. Only this
  task's installer and its own curl child were stopped; unrelated downloads and
  shared staging were left alone. No compiler override or substitute build was
  used. The empty verification worktree was removed.

No updated binary was shipped or installed by this change. The source requires
the normal certified package/gate once the selected compiler is available.

## Follow-up qualification (2026-11-16)

Native installation of source-selected Trust build 15698 subsequently completed
successfully without changing the global current compiler. An isolated validation
worktree now runs the focused quota-recovery suite. Its first libgit2 dependency
fetch was ended after a public Chromium fetch waited; the documented CLI Git
fallback continues fetching the unchanged upstream dependencies. No source or
lockfile substitution is used.

Review found and fixed a misleading fallback warning for healthy 95–98% preflight
usage: this now has an explicit `NotNeeded` outcome while retaining the fresh-read
requirement and visible reports of real ledger/auth failures. New regressions
cover both quiet near-threshold usage and a corrupt ledger at 96%. Old near-reset
unit expectations were updated to the new policy, and identity, permission,
spend-denial and utilization checks now start from eligible positive controls.

The production smoke also verifies count, expiry, decision and restoration in
actual exec JSON events for representative positive/preservation cases; warnings
remain nonfatal. Its 23 Python fixture tests pass. The installed 2026111601 binary
was a negative control at exactly 48 hours: it attempted four consumption POSTs,
which the loopback oracle rejected. No live reset credit was used. Retained
classification and request traces are under the private evidence directory
`~/.codex/.ubd/weekly-reset-strict48-installed-negative-20261116`.

Rust execution and package/gate outcomes are recorded separately after these
checks finish; the earlier unavailable-execution note describes the initial
attempt, not a claim that the newly added Rust regressions have passed.

The Sol 6.1 Ultra packaged default also required explicit High reasoning in both
opt-in Kimi profiles. K3's catalog supports High as its default; without a profile
override, the root Ultra selection survived configuration merging. Although
request normalization converted that to supported High on the wire, local Ultra
also enabled proactive delegation. The profile defaults now retain K3's own
behavior, and their tests check the merged profile overrides root Ultra while
leaving the base Sol default intact.

## Focused Rust execution and resource refusal

The focused quota suite compiled with source-selected Trust 15698 and ran 95
unit tests: 87 passed and eight failed. The new reporting, boundary, quiet
preflight and broken-ledger regressions passed. Seven failures retained the old
expectation that policy-forbidden five-hour or near-weekly recovery would list
credits without an attached reporter. The eighth waited for a credit-list signal
on an ineligible one-hour weekly fixture. These fixtures now expect zero lists
when policy forbids redemption, or use an eligible 72-hour week when list
observation is the point of the test. Handoff, identity, cancellation and no-POST
assertions remain.

The next admitted Rust run (`build-slot -- ... targo --unverified test -p
codex-alab-core --lib --locked`) was refused before execution: native CPU slot 0
expected device 16777231 and observed 16777230 for the same inode 5262350.
`aterm ctl @self resources cpu` likewise reports the shared pool unreadable.
Managed package doctor is healthy; package repair does not repair CPU authority.
Trust's `docs/BUILD_SLOT.md` explicitly reserves device-identity recovery for the
resource authority. No supported operator command was found. No lease manifests
were edited/deleted, no alternate pool selected, and no direct heavy run was
used to bypass the refusal.

Consequently the corrected 95-test quota suite, the updated core/default-profile
suite, full gate, certified package and positive new-binary smoke remain
unexecuted. The initial compiled test run is not a claim that the final fixture
changes or the new Kimi profile checks passed. All 23 Python smoke-fixture tests
pass and changed source passes scoped formatting/diff checks. The owned clean
validation worktree is retained for recovery; no check remains running.
