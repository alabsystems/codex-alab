<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve package metadata ownership and failure precedence

Decision: protect descriptor acquisition and retain the selected package-reader
failure through its close attempt. Reuse the existing nonthrowing case labels
and note transport, with no new runtime input or import dependency.

At `89b3209b73`, a public `package_record` control cancelled at the first actual
caller line after `os.open` returned. The descriptor remained open because the
cleanup guard had not begun. Separate controls performed a real close and then
raised: that secondary failure replaced the original read/stat cancellation or
IO error, and real UTF-8/JSON decoding errors. A close-only `SystemExit(0)` also
escaped as a success-shaped exit. Seven initial regression methods exposed 30
failing subtests on the original source; fixture fallback closed each owned
descriptor left by the post-open control.

The reader now initializes its ownership and pending-error slots before the
guarded open. The original open-OSError wrapper and exact cause remain inside
that guard. Read, validation and decode errors select the primary before close;
secondary close failures attach advisory diagnostics through `_case_label` and
`_attach_case_notes`, preserving the exact active error or cancellation.
Unavailable formatting, native notes or Python 3.9 fallback transport cannot
replace that selection.

A close-only failure prevents returning parsed metadata. Ordinary errors,
KeyboardInterrupt and genuine nonzero exits keep their exact identity. Only a
success-shaped close-only SystemExit becomes RuntimeError, with that exact exit
as its cause. A small local stdlib classifier matches `alab/cli_status.py`,
including native POSIX wrapped-zero exits; no smoke dependency is introduced.

The original regular/no-follow/nonblocking open, one-MiB bound plus one-byte
overflow observation, six-field descriptor comparison and UTF-8/JSON semantics
are unchanged. The single-record reader still accepts general JSON values;
package-level source, version, target and entrypoint checks remain where they
were. Both maintained probe aliases still use the same reader.

All 16 methods in `test_smoke_package_records` pass without skips on managed
Python 3.14 and system Python 3.9 with `-I -B`: eight existing contract controls
and eight ownership/precedence controls. The new tests use owned real JSON files
and descriptors, actual post-open line cancellation, actual decoder exceptions
and actual-close-before-error injection. They launch no child process, thread,
application, socket, PTY, provider or compiler. `git diff --check` passes.

This policy attempts close once and preserves evidence of failure. A close error
does not by itself prove closure; the fault controls close their owned real FD
before raising so they can verify the precedence boundary without leaking it.
No package certification, executed-image identity or application qualification
claim follows from reading these statements. Existing runtime and matrix input
closures already include the modified capacity support file.

Independent review passed the same 16 methods on both Python versions and found
no blocker. Root reviewed the production policy and approved this three-path
topic for composition.
