<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Trust 27009 async trait-depth admission

The signed ARM Trust 27009 compiler includes the previously missing proc-macro
worker fix. Its first normal adoption stopped at lint admission before any
certified application result. Three deep async graphs exceeded the newly
corrected trait-depth accounting; test code also had one redundant clone and
three unnecessary heap pins.

Rust's [tracking issue](https://github.com/rust-lang/rust/issues/159228) explains
that cached trait results previously undercounted depth and explicitly permits
a finite recursion-limit increase for deeply nested async functions. App-server
and ALab Autopilot already selected 256. They now select 512; login selects 256
instead of the compiler default. The two upstream edits are marked source
defaults. Trait checking, lint admission and certified proof requirements remain
enabled. The limits provide finite analysis capacity, not an assumption that an
unproved obligation succeeded.

An initial experiment added erased future boundaries and a separate test runtime
helper. It cleared the first locations but exposed the same warning in other
existing async paths, including an already boxed auth reader. Those experimental
changes were removed. The selected limits add no runtime allocations, API,
ownership or scheduling changes. The pin replacements stay inside their original
lexical blocks, preserving the point where the actual waiter is destroyed.

Protected source-selected Trust 27009 lint over app-server, login, ALab Core,
ALab Autopilot and TUI production/test libraries passed. The repository classifier
found zero owned, spanning, failed-target or missing-script findings and set aside
56 upstream diagnostics. Exact working inputs and logs remain outside Source in
`trust-27009-finite-first.{patch,json}` and
`trust-27009-finite-first-lint-verdict.json`. This check is not a certified
application build or completed adoption. The source fixes are committed separately
from the native-generated working `trust.lock`; only successful native acceptance
may commit that lock. Ordinary live-update activation remains unqualified.
