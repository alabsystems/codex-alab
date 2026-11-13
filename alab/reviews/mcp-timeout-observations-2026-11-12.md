<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Observe the MCP registration timeout without changing its decision

## Finding and decision

The full native TUI run at `ecf11e2e44` selected the five-second timeout in
`local_daemon_registers_approval_gated_mcp_tools_for_both_start_paths`; an
unchanged isolated retry passed in 9.16 seconds. The failure did not retain
task/channel readiness or which backend requests the proxy had observed.
The existing evidence cannot identify a component responsible for the delay.

Keep the original timer, selection order, pass assertions, and panic prefix.
Replace only the timeout panic with one marked call into ALab-owned test
support. The helper samples task completion flags, channel closure and buffered
count, and a nonblocking snapshot of the existing request recorder.

The recorder snapshot retains its total count and at most the final 16 method
labels. Known protocol names map to fixed labels; every other name maps to
`other`. It never formats a request, parameter, ID, credential, response, or
error value. Busy and poisoned locks report unavailable data, not an empty
request history. The guard is released before any formatting.

## Interpretation and boundaries

The observation is explicitly non-atomic and occurs after the timeout branch
was selected. A finished task may have failed. A closed channel may still have
buffered events. The helper does not receive an event, poll or join a task, or
wait for the recorder lock.

The proxy records incoming methods before forwarding them, so a method label
establishes arrival, not successful completion or a currently blocked request.
These observations narrow a future investigation; they do not turn a passing
retry into proof of a fix or assign the timeout to upstream. The test and MCP
bridge match upstream, but the execution also traverses ALab Core hooks.

The macro keeps the upstream change to one marked hook. It borrows each supplied
handle once and preserves the same failure. The generic helper and its tests
live in the existing test-support feature; ordinary builds do not include them.

## Validation

Four focused tests passed after replacing a single-yield scheduling assumption
with a bounded, non-consuming readiness wait. They cover retained events and
task results, bounded private output, a held recorder lock, a poisoned recorder,
and the macro's original failure prefix. The busy-lock case always releases its
guard and joins the worker before reporting failure. Library Tippy passed with
warnings denied in a separate cache. That run also found a pre-existing
`expect()` in the PTY owner; a separate commit replaces it with the same explicit
invariant failure.

A temporary negative control constructed the original HTTP request future but
parked its one creation task before awaiting that future. The original
five-second selection was retained. The real test failed with the new message:
creation and proxy unfinished, channel open with zero queued events, and one
recorded `thread/list` request. No creation request was sent. The test failed in
10.74 seconds including its earlier setup; the independent 300-second process
bound did not expire. The upstream source was restored byte-for-byte afterward,
with matching SHA-256 values. This proves that the actual timeout path emits
metadata while remaining a failure; it does not reproduce or explain the
original intermittent timeout.

After restoration, the original MCP integration test passed in 11.48 seconds.
The positive run is separate from the controlled failure and does not establish
the cause of the earlier intermittent timeout. Its log is
`codex-mcp-timeout-positive-20261112.log` in the private log directory below.
The actual controlling-PTY input regression also passed in 6.36 seconds after
the separate lint cleanup; its log is
`codex-mcp-timeout-pty-regression-20261112.log`.

Logs and the restoration record are under `$HOME/.aterm/.ubd/`:
`codex-mcp-timeout-unit-final-20261112.log`,
`codex-mcp-timeout-lint-final-20261112.log`, and
`codex-mcp-timeout-negative-20261112.log` with its adjacent JSON. All Rust tests
used the Trust unverified lane, the committed lockfile, offline resolution,
and an 8 MiB test stack. No release gate or compiler proof is claimed.

The source inventory tool could not run because this checkout has no `upstream`
remote. Direct before/after file diffs show that the original five-second timer
and surrounding control flow are unchanged and that only the marked panic hook
was added to the upstream test file. The new topic belongs to
`qualification-fixtures`. A direct call to the inventory validator found no new
ownership error; five existing unmapped topics were recorded separately for
repair. No workspace-wide formatting or lockfile change is involved.

## Concurrent lifecycle follow-up

The existing `app::tests::session_lifecycle_requests::` family ran with four
test threads at `df71789e11d83b5b6736f4ac6a043012a6b5073e`: all 71 tests passed,
with none failed or ignored, in 217.12 seconds. This includes the MCP
registration case. No timeout snapshot was triggered in this run. The source
and its five-second timeout remained unchanged.

This is a passing concurrent-family observation, not a repair or explanation
of the earlier full-suite timeout. The log and its source/checksum record are
`$HOME/.aterm/.ubd/codex-mcp-lifecycle-concurrency-20261112.log` and the adjacent
JSON file. No broader test repetition was needed to validate the catalog-only
wording correction made after this run.
