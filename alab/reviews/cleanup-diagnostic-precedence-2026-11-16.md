<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep diagnostic failures out of cleanup authority

Decision: select the authoritative body or cleanup exception before rendering
secondary diagnostics. A failed message, note hook or optional note transport
cannot replace that exception or prevent later safe cleanup and request-trace
retention. Ordinary cleanup failures still fail their case, and an actual
cleanup cancellation retains its existing precedence over an ordinary body error.

At source `0491766d84`, public quota-wait case/main controls demonstrated four
failures using only bounded fake process/listener actions. A shutdown exception
whose `__str__` raised `SystemExit(0)` replaced the original KeyboardInterrupt,
stopped after shutdown and lost the request trace. A failing `add_note` or
fallback attribute setter replaced cancellation with an ordinary error outcome.
A primary cancellation's failing `__str__` escaped as `SystemExit(0)` before its
summary was saved. Private before/after controls are retained under
`~/.codex/.ubd/cleanup-diagnostic-negative-20261116`.

The existing shared support now uses small nonthrowing exception-label,
string-note reading and note-attachment helpers. Unavailable messages get an
explicit type-based placeholder. Working Python 3.11+ native notes retain
`errorNotes`; older Python and unavailable native hooks use the existing
`cleanupErrors` transport when available. If both optional note mechanisms
refuse attachment, the original exception still wins and its failed/interrupted
summary and request trace remain valid. No report schema, loader or import is added.

Eight public case/main regression methods cover formatting failures, refused
native and fallback note hooks, unavailable note reads, actual cleanup
cancellation, ordinary failures and working diagnostic fields. They produced
26 assertion failures and one error before the fix. The four retained controls
now preserve the original exception object, finish every later safe cleanup
action, retain requests, and save failed interrupted summaries.

Managed Python 3.14 with `-I -B` passes all 53 selected methods: eight new
diagnostic methods, sixteen quota-wait methods and twenty-nine request-recovery
methods including cleanup. macOS Python 3.9 with `-B` passes the same eight
methods, including the real older-interpreter diagnostic transport control.
`git diff --check` passes. No child process, listener, compiler, provider account,
native application or full-workflow qualification was run by these controls.
