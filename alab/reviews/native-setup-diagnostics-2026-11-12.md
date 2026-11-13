<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Finish native fixture host setup before model work

## Problem and decision

The native upgrade fixtures test completion ordering and target refusal, but
could fail before reaching those boundaries. They started model bootstrap
before observing the independently started host handshake. Earlier failures
retained only a missing launch witness, obscuring the concrete reason.

Capture the existing supervisor diagnostic inside each exact child, then await
the normal launch probe immediately after `native.start_thread()` and before
submitting model work. Keep the real probe, its five-second deadline, and the
same launch-witness assertion. The readiness fixture still does this only for
its active target-refused case. No later Core, UI, time-advance, target-refusal,
no-claim, or continuation assertion moves or changes.

The diagnostic subscriber enables only the ALab Autopilot target at debug level
and writes to the child's existing retained stderr. It is installed after
private environment and exact PID checks. It does not expose the installed
supervisor as a new global API or alter the parent process's subscriber.

The separate backend fixture asks its proxy task to abort, then waits for EOF.
Preserve that sequence and its five-second deadline. On timeout, retain whether
the proxy task has finished when the error is observed, along with the original
Elapsed error as the source. Do not infer from this one observation that the
proxy was finished throughout the deadline, or that aborting it joins all
embedded-server tasks. The earlier dropped event poll uses a cancel-safe Tokio
receiver; source review found no evidence that it consumes an EOF event.

All changes affect test support or fixtures. No production deadline, protocol,
model request, or account behavior changes.

## Evidence before the setup change

With diagnostics alone, the same three-case parallel run passed the backend
case and failed both upgrade parents in 26.65 seconds. Their retained child
stderr reports the exact error:

```text
launch probe: aterm unreachable: aterm did not answer in time
```

The private host had recorded the initial version request. This locates the
failure in the authenticated handshake, before UI completion or upgrade
capture. It does not establish which executor or I/O component caused the
reply to miss its deadline.

The build and failed run are retained in
`$HOME/.aterm/.ubd/codex-native-setup-diagnostics-20261112.log`.
The two child records are `/tmp/ac.v7QOOV/target-refused/child.stderr` and
`/tmp/ac.wxoJZU/delivered-first/child.stderr`. These are isolated fixture
records, not a failure of the live account registry.

## Validation after the setup change

With the handshake observed before bootstrap, the same three tests passed in
parallel in 9.68 seconds, then passed again in 10.18 and 9.61 seconds: nine
passing test executions across three runs. Successful launch
probe messages are present for the delivered-first, held-completion, and
target-refused children; the logger therefore remains exercised on success.
The retained logs are `$HOME/.aterm/.ubd/codex-native-setup-barrier-20261112.log`
and `codex-native-setup-barrier-repeat-2-20261112.log` and
`codex-native-setup-barrier-repeat-3-20261112.log` in that directory.

These runs use Trust's unverified lane with `--locked --offline`, an 8 MiB test
stack, and three test threads. They use the native producer and private host,
not live provider authentication. The original backend cancellation, EOF,
surviving-source, and replacement checks remain in the passing run. No backend
timeout occurred in these runs, so they do not establish a backend root cause.

Independent review confirmed the ordering change retains all assertions, the
tracing scope is confined to the validated child, and the timeout wrapper keeps
its original error source. Whitespace checks pass. `alab/delta.sh --topics`
remains unavailable because this clone has no upstream remote; no remote was
changed to bypass that limit. This is scoped fixture validation, not a complete
release gate.

## Integration

Remote main advanced to `85ccf1a9a7` with terminal-startup restoration while
the focused runs were in progress. The rebase was clean. Because those changes
touch related TUI paths, all three checks were rerun on combined source
`9f6473e3f748d41ea2bf82227225607963b58f4c`: three passed, none failed or
were ignored, in 14.20 seconds. The log is
`$HOME/.aterm/.ubd/codex-native-setup-integrated-20261112.log`.
