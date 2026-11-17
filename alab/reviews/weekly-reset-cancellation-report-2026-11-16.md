<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve reset-smoke cancellation during report failures

Decision: retain the original cancellation after the weekly-reset case has
unwound, even if hashing the probe, saving the summary or printing it also
fails. The secondary failure remains the cancellation's exception context.
Ordinary report failures continue to fail normally.

At source `08d0e3934a`, the CLI caught `KeyboardInterrupt` or `SystemExit`,
recorded interrupted coverage, then ran three report operations before
re-raising the interruption. An error in any operation replaced that original
outcome. Nine fault-injected calls to the real `main()` reproduced this for
KeyboardInterrupt, SystemExit(143) and SystemExit(0), across probe hashing,
summary writes and output printing. The existing incomplete-coverage and
completed-case evidence stays intact whenever the report can be written.

Wrap final reporting in a cancellation-preserving `finally`, matching the
request-recovery smoke's ownership of its original interruption. The two new
test methods verify the exact exception object, secondary exception context,
saved interrupted report after output failure, and ordinary report failures.
Before the fix they report nine errors; afterward all 31 weekly-reset fixture
methods pass with managed Python 3.14 and macOS Python 3.9. Independent read-only
review found no blocking issue, and `git diff --check` passes.
No provider account, Rust build,
application certificate or installed-binary execution is involved.
