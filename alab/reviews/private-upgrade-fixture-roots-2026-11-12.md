<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Shared private roots for native upgrade fixtures

## Finding and decision

Two upgrade test modules duplicate a temporary-directory helper. Each checks
the preferred root's lexical socket length, but neither checks its canonical
path or the fallback's length. Both inherit tempfile's default directory
permissions. A public `/tmp` fallback can therefore create a `0755` root rather
than an owner-only one.

Centralize this policy in ALab's existing test-path support. Create each root
with explicit `0700` permissions. Prefer the configured temporary directory;
consider `/tmp` only if the full socket path is too long. Propagate creation,
canonicalization, and permission errors rather than treating them as reasons
to leave the configured location.

Check both lexical and canonical paths, including the longest fixture case
and a maximum-width native instance number, against the 103-byte Unix socket
budget. Reject an overlong fallback too. Dropping the rejected candidate
cleans it before trying the fallback.

Keep retention decisions at the existing callers: their `disable_cleanup(true)`
still preserves diagnostic witnesses. Ordinary callers retain `TempDir` cleanup.
The stricter 32-byte helper used by other tests keeps its existing behavior and
shares only the owner-only allocation primitive. This change does not widen
timeouts, change test assertions, or choose a real account.

## Checks

The unit cases exercise preferred-root selection and real deepest-socket
binding, private fallback and cleanup, canonical expansion through a short
symlink, an overlong fallback, and creation errors at both locations. They
pass explicit private parents and never mutate the process environment.

Independent source review confirmed the atomic permission mode, bounded
paths, error propagation, retention, and Unix-only call sites. The new work
lives entirely in ALab-owned files.

## Validation

Before this change, the full native TUI suite passed at
`f376ccc57fc6dc269527522e1b176c58a07de992`: 6,386 passed, zero failed,
and six ignored in 789.56 seconds. This includes every earlier failing fixture.
The log is `$HOME/.aterm/.ubd/codex-native-post-setup-full-20261112.log`.
Three retained roots from that run had mode `0755`, confirming the permissions
issue in the callers' actual layout.

At `e55e30a208`, all seven test-path unit cases passed. All five native caller
checks then passed sequentially in 23.66 seconds: the three upgrade parents,
the startup footer, and the default control-socket probe. The three retained
upgrade roots were all `0700`; their lexical and canonical longest socket
paths were within 103 bytes.

The earlier four-thread caller run passed three tests and failed two active
upgrade setups with the already observed launch-handshake timeout. Their
roots were also `0700`, and their private hosts recorded the initial version
request. Keep that result visible: this directory change does not eliminate
every intermittent startup timeout, and no timeout or assertion was weakened.
The complete suite has not been rerun after this directory-only change.

The focused logs are `$HOME/.aterm/.ubd/codex-private-fixture-unit-20261112.log`,
`codex-private-fixture-native-20261112.log`, and
`codex-private-fixture-native-serial-20261112.log` in the same directory.
Before/after metadata is in `fixture-root-permissions-before-20261112.json`
and `fixture-root-permissions-after-20261112.json` there. Tests used the Trust
unverified lane with `--locked --offline` and an 8 MiB test stack. Whitespace
checks pass; there is no release-gate or compiler-proof claim.
