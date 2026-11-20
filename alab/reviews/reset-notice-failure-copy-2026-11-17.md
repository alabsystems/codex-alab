<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# State reset recovery failures without inventing their cause

The reset-lock fallback handles both actual contention (`Ok(None)`) and failure
to open or acquire the lock (`Err`). Its notice nevertheless asserted another
process held the lock. After a confirmed reset, the retry-release fallback also
claimed the retry was already accounted for, although `release` can return
`Stop` for unreadable, malformed or unwritable receipts and failed syncs.

Decision: change only those two message literals. Say the account reset lock
could not be acquired, and the recorded retry could not be released. These
statements cover every existing fallback outcome without asserting a holder or
successful accounting. The second follows the truthful wording already used
when recovery discovers restored allowance before a new reset is needed.
All lock, receipt, accounting, retry and redemption behavior stays identical.

Two independent source reviews confirmed the outcome distinctions. An exact
whole-file comparison against `a27fb81de5e7302c0285b7ae234931587d101481`
proves that only two ASCII literal contents differ; reversing those replacements
reproduces the original bytes. Types, control flow, delimiters and formatting
are unchanged. Existing notice tests do not depend on the superseded wording.
No implementation-mirroring copy tests were added.

This is static validation of a wording correction. No Rust test, compiler,
formatter, native application gate, package or live reset ran. Native build
custody and the qualified publisher remain unavailable; release qualification
and error-path runtime checks remain deferred. Evidence is
`~/.codex/.ubd/reset-notice-failure-copy-review-20261117/review.json`.
Final Rust source SHA-256:
`0784d7a7cd0ef3e98deee68c5faa381d15ef868a96c4ddf2506b8f9b537a330e`.
