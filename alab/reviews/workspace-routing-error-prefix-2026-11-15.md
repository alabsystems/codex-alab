<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Render the terminal account/read failure once

The original launch failure said `account/read failed` twice. The typed
app-server client error already renders the method and failure phase, while the
ALab retry's terminal branch wrapped it in another literal `account/read
failed` context. Exhausting the bounded routing retry therefore preserved all
error information but duplicated the user-facing prefix.

Decision: after a non-retryable error or the third transient timeout, surface
the typed request error directly. Its server, transport and decode variants all
retain the method and their distinct phase, and `color_eyre::Report::new`
retains the typed source chain. Retry classification, fresh request IDs, the
three-read bound, delays and structured-reason precedence are unchanged. The
change lives entirely in existing ALab-owned TUI glue; vanilla upstream behavior
and hooks are untouched.

The five focused unverified Trust tests pass. The exhausted-timeout case now
requires exactly one `account/read failed`, the legacy timeout message and code
`-32603`; the non-retryable routing-integrity case also requires one method
prefix and no second request. The remaining tests continue to cover fresh IDs,
the three-read bound, structured-reason authority, and exact method/code
classification.

Scoped all-target Tippy for `codex-tui` exits zero in its dedicated cache; it
reports only pre-existing upstream diagnostics outside the changed glue. Two
independent retry-path audits and a hostile diff review found no blocker. The
review confirmed that transport and decode errors retain their nested sources
through `TypedRequestError`, while server errors retain message, code and data.

The separately recorded live model-stop mutation-epoch gap remains the next
correctness topic; it is not mixed into this user-facing rendering fix.
Application certification and package installation remain deferred at the
independently reproduced Trust 15698 in-process proc-macro admission boundary.
This record makes no certification or installed-update claim.
