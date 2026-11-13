<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Full native TUI check after the account refresh changes

## Scope and result

Run the complete native TUI library suite at
`a17b1b8b68ef2d6f075a416fedb2a86a7da1f39a`, tree
`8c0b58eeac20f443b9a3120342fc22069430ec6d`, before making the follow-up
account-history presentation change:

```sh
CARGO_NET_GIT_FETCH_WITH_CLI=true RUST_MIN_STACK=8388608 \
  targo --unverified test --locked --offline -p codex-tui --lib \
  -- --nocapture --test-threads=4
```

The build completed. Of 6,391 tests, 6,379 passed, six failed, and six were
ignored. The test run took 782.62 seconds and returned 101. This is not a
green full-suite result or a completed release gate. Trust's unverified lane
does not make a compiler proof claim.

The full log is `$HOME/.aterm/.ubd/codex-tui-full-20261112.log`.

## Failure analysis

1. `app::agents_overview::tests::worktrees_overview_groups_by_project_or_directory`
   changed only seven relative-age labels from `2m ago` to `3m ago`.
   The fixture keeps a process-wide lazy timestamp while the renderer reads
   the current time. The test passes alone in 1.04 seconds. Its fixture,
   normalization helper, renderer, and expected snapshot are unchanged from
   imported upstream `d9a32c178951695f365faa36a39d542e06a96e4f`.

2. `app::tests::fork_releases_a_provisional_saved_chat_on_its_original_connection`,
   `app::tests::saved_chat_replays_approval_after_a_stale_runtime_close`, and
   `app::tests::sidebar_reopens_saved_side_chat_after_restart` all reach the
   same inline snapshot in `reopen_saved_side_chat`. Insta rejects repeated
   calls to that assertion in one process unless its supported duplicate
   scope is used. The helper is unchanged upstream. The fork and approval
   tests pass in separate processes (31.57 and 29.94 seconds respectively).

3. `app::tests::session_lifecycle_requests::startup_defaults_tests::fresh_startup_reads_destination_and_cleared_model_uses_catalog`
   renders its footer at width 80 before replacing the full temporary path
   with `<PROJECT>`. The runner's longer macOS temporary path is already
   truncated, so that replacement cannot match. Both expected and actual
   output have `GPT-6.1-Sol high`; preceding model/config assertions pass.
   The failing block is unchanged upstream. The test passes alone with the
   ordinary temporary root (45.50 seconds), then fails with the same path-only
   difference when explicitly given the runner's longer temporary root
   (16.70 seconds).

4. `tests::default_daemon_auto_connect_probes_socket_only` fails while binding
   its fixture socket: the path exceeds macOS's `SUN_LEN`. The bind occurs
   before the daemon probe or ALab's attachment hook. The test, socket path
   construction, and socket-directory implementation are unchanged upstream.
   The same test binary passes with a short private temporary root, which is
   removed afterward.

The standalone replay logs are alongside the full log:
`codex-overview-time-isolated-20261112.log`,
`codex-fork-snapshot-isolated-20261112.log`,
`codex-approval-snapshot-isolated-20261112.log`,
`codex-startup-path-isolated-20261112.log`,
`codex-startup-managed-path-isolated-20261112.log`, and
`codex-daemon-short-path-isolated-20261112.log`.

## Decision and limits

Keep these failures visible. Do not accept timing or machine-path snapshot
changes, skip assertions, or modify production behavior to satisfy fixtures.
Source comparisons and isolated reproductions identify fixture causes;
they do not substitute for a full pristine-upstream run or a passing gate.
The native account tests passed in this run. The small follow-up clock change
gets its own targeted validation, recorded separately.

Potential fixture repairs remain work to evaluate under the thin-layer rule:
an explicit test clock for relative ages, a narrowly scoped repeated-snapshot
assertion, and short owned destinations for socket and width-sensitive tests.
No such repair is included here.

## Fresh registry check

All 13 read-only registry-probe tests passed again. A live stable-key query
at 2026-11-12 09:04 UTC completed all five registry reads successfully and
still found one OpenAI account associated with node-bound m3. Asking for two
correctly returned `insufficient-accounts` and exit 1, with no read errors.
It retained the observation timestamp and did not authenticate or change an
account. The private metadata report is
`$HOME/.aterm/.ubd/codex-m3-registry-proof-20261111.noindex/probe-m3-followup-20261112.json`.

## Publication composition

Remote main advanced to `1e50c85722` with paired terminal-ownership capture
and its qualification records. Both local commits rebased cleanly onto it.
A Git comparison confirmed the tested account implementation, account tests,
assignment and details-view integration, and clock-format sources remained
byte-identical after that rebase. The intervening changes affect terminal
capture, so the account checks are retained without repeating them. This
does not claim a full-suite qualification of the combined source.
