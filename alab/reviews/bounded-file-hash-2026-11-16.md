<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Bounded descriptor hashing for runtime observations

Decision: share weekly reset's bounded descriptor reader with the runtime controller
and both history digest boundaries. `alab/file_hash.py` uses only the standard library;
callers load its exact source path. Weekly retains its deliberate `RuntimeError`
refusals, while runtime retains `ValueError`. History's isolated guardian prefix is
byte-for-byte unchanged, and weekly's early exec-wrapper runs before helper loading.

At base `5f8aa27052699540c0d4fc089ebfb565ff988ba4`, a finite control using the
existing inert history CLI fixture overwrote the binary with different, same-size
bytes after the final reader returned its first 33 bytes. The original CLI exited
zero, emitted success and saved `passed: true` with a SHA different from the current
file. No app, PTY or listener was launched. An owned FIFO blocked the initial reader
until its two-second interpreter timeout killed and reaped that owned process.
Runtime's reader already refused nonregular files, but a real 100-byte file appended
after each read reached a four-call guard after reading 102 bytes. Weekly refused
after two calls and 101 bytes. The new same-size history and runtime append regressions
were run against unchanged base source: both failed for these exact reasons, with no
test errors.

The shared reader opens with `O_NOFOLLOW | O_NONBLOCK`, checks the opened descriptor
is regular, reads at most its initial size plus one byte in streaming chunks, and
requires the initial byte count and stable device, inode, mode, size, modification
time and change time. The acquisition is inside its cleanup guard. Finite caller-line
cancellation controls exposed and now cover the immediate post-open ownership edge.
Read/stat failures and cancellations retain their exact object even when close also
fails; formatting and advisory notes cannot replace it. A close-only failure prevents
a digest return. Ordinary failures, genuine nonzero exits and `KeyboardInterrupt`
propagate unchanged. The deliberate close-only exception is a success-shaped
`SystemExit`: it becomes `RuntimeError` with that exact object as its cause. The local
standalone classification matches `alab/cli_status.py`, including native POSIX wrapped
zero, and adds no smoke-module dependency.

The runtime input list and publication hook gate now include the helper. Dispatch
controls substitute it during either probe and require a failed runtime record; a
publication control requires refusal before the native gate. Existing history cleanup
controls now observe the weekly digest API so their cancellation seams remain active.
The renewal matrix imports neither weekly, history nor runtime, so its closure is
unchanged; capacity's imports are unchanged.

Validation: all 99 selected methods pass without skips on managed Python 3.14 and
macOS Python 3.9 with `-I -B`: `test_history_file_hash`, `test_runtime_file_hash`,
`test_check_runtime`, `test_history_cli_status`, `test_history_main_cleanup` and
`weekly_reset_test`, plus `ReleaseRecordTests.test_pre_promote_binds_every_consumed_history_runtime_helper`
and `ReleaseRecordTests.test_pre_promote_refuses_a_substituted_shared_hash_helper_before_native_gate`
from `test_version_ledger`. `git diff --check` is clean. Independent integration review
confirms the exact-path imports, isolation and two source-closure updates. Independent
hostile review approves the implementation and passed 41 focused methods on each
Python version without skips. Root reviewed the code and decision note and approved.

Limits: these checks concern a bounded, stable opened descriptor. They establish no
pathname continuity, executed-image identity, build certification or protection from
changes after the final descriptor observation. All new controls use finite owned
temporary files or inert process/listener seams; no native application, real PTY,
socket, provider or compiler was run.

The root's private composition at `63804fc9d6989c05a35fb312bcb7eb2c572248c4`
includes the landed renewal acquisition and shutdown-refusal controls. All 368
selected Python methods pass in 27.390 seconds, with zero failures, errors or
skips; the exact source remains clean and unchanged. This includes the two named
pre-promote fixture controls above. Native-required release-hook cases retain
their existing boundaries and were not selected as substitute qualification.
Discovery alone finds 1,915 unique IDs, no discovery errors, five history-hash
and ten runtime-hash identities, without executing any body or native prerequisite.
