<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Observe the real deferred writer before its first history file

A fresh persistent Core can have actual creation metadata and buffered canonical
items without a materialized rollout file. A retained UI replay must observe that
state without forcing persistence or substituting an invented empty history.

The new read command joins the original writer queue. It returns the actual
SessionMeta and whole local pending sequence at that ordered cutoff, without
flushing or creating a file. Unresolved writer failures and stopped writers stay
errors; materialized and resumed writers return None. None supplies no history.

LocalThreadStore holds its existing per-thread writer lifecycle guard through the
observation and checks metadata against the same live registration. The result
retains both logical thread ID and physical rollout ID. LiveThread uses its stored
backend and identity; CodexThread delegates through its own Session live handle.
The existing public history/read policy is unchanged. Stores without this
observation return None, and ephemeral history keeps its separate journal path.

The snapshot preserves history_base. Local pending items are not complete inherited
fork history: a consumer must resolve the actual lineage through existing policy
or refuse it. The cutoff can be followed by real materialization; it is not a
lifetime freeze, durable cursor, Native permission or effects-settlement proof.

## Validation

Root applied the independently reviewed 16-file patch 449bab86410652133a9e00cf29ceb5254bd1304182ac27431a43bb2a9a0092ca
on b8ac6cab0cb6eb9a9aa5804c2b8216a71b2196ce, verifying every base/result hash.
Source-selected Trust 15698 used the existing protected profile and ordinary
unverified test lane, with the canonical open-file capacity of 65536.

The full rollout library passes all 169 tests, including four real writer
controls. Its input patch was f11628b6605dbd6822f765322787dd70a5d1381645a90339f6fdd0eea410dc51,
and job 38031's rollout command exited zero. Store tests initially failed to compile
because their fixture imported an unavailable convenience dependency. Only that
owned test file changed, using standard-library fallible errors and preserving
cleanup; no manifest or lockfile changed.

On the corrected input c667bb2745f84c9a7d310aa5e2d8bfe6a92dcb2d5705919c4cefb26cc4f93aa2,
job 35575 exited zero: all 297 store tests, the actual fresh-Core/name-RPC control,
and all 42 task regression tests passed. Across both stages there are 509 passing
executions over 508 distinct package/test cases; the Core control also runs in the
task filter. All seven new controls executed. They cover actual writer ordering,
no file creation, materialization/resume/stop, real I/O failure and recovery,
inherited boundaries, cold-store absence, discard and the same actual Core.

Four-package production/test Tippy for rollout, thread-store, Core and app-server
exited zero. The maintained classifier found zero owned, spanning, failing-target
or missing-script diagnostics and set aside 11 upstream diagnostics. Dependencies
and the source-selected compiler lock are unchanged. Evidence, exact inputs and
immutable command logs remain outside Source in codex-live-update-evidence,
including retained-deferred-final-validation.json.

This qualifies the writer/store/Core component only. The actual retained replay
consumer, paired Native SDK/UI handoff, live editor ownership and certified
application build remain unfinished. Ordinary retained target activation stays off.
