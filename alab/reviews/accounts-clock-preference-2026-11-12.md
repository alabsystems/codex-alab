<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Use the same clock preference in account views

At `d64acb4320`, the subscription account list formats reset and observation
times with a fixed 24-hour pattern. The adjacent account details view already
uses the TUI's detected clock preference. A person using a 12-hour clock sees
inconsistent timestamps between those views.

Capture the widget's clock preference before starting a snapshot request and
pass it to the account formatter. Use the existing shared date/time pattern
and retain the local time zone. Native timestamp values, unavailable-value
fallbacks, stale-data labels, request ordering, and account operations are
unchanged.

This is a small presentation correction in one ALab-owned file. Validation
uses the existing account-pane and clock-format tests; no new test or host
preference change is needed.

All 29 account-details tests and four clock-format tests passed. This includes
the existing adjacent-account test that exercises both clock preferences.
The account group was built with Trust's unverified lane, `--locked --offline`,
and the repository's 8 MiB test stack; the clock tests used that same resulting
test binary. Logs are `codex-accounts-clock-20261112.log` and
`codex-accounts-clock-format-20261112.log` under `$HOME/.aterm/.ubd`.
Formatting and whitespace checks passed. No upstream file or lockfile changed.
