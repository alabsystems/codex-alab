<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve Kimi quota retry advice

Review after the general recovery work found that Kimi's adapter retained
`Retry-After` when converting a transient rate limit, but discarded it when
converting a recognized quota message to `QuotaExceeded`. The ordinary API
bridge preserves this metadata, and the quota recovery path reads it when
choosing its next deadline.

Retain the already captured advice on the quota error. Keep the existing
classification: quota still has no ordinary request retry, while recognized
transient limits still map to server overload. Unrecognized, inactive and
other-provider paths retain their generic mapping.

The regression covers HTTP 429, typed rate-limit errors and retryable stream
errors, each carrying the same one-hour advice, for both quota and transient
messages. It compares the exact captured advice and checks that only quota
remains nonretryable by the ordinary retry policy. The test failed before the
change because quota advice was `None`; all six Kimi library tests passed
afterward. The test-only direct HTTP-client dependency changes only the
ALab-owned Kimi lock entry.

Focused Tippy over the Kimi library and tests passed without diagnostics.

This is source/runtime-test evidence. It does not claim a newly installed
application or alter the recorded certified-build blocker.
