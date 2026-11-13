<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Separate exact hook identities from their display

## Finding and decision

Hostile review after the committed-source inventory change found three ways
that human formatting changed ownership checks. A topic beginning with a
space was discarded as a source-detail line. A tab in a topic was mistaken
for a field separator and truncated its identity. Git's binary-file summary
became a fabricated `(untagged)` topic, so a catalog owning that placeholder
could hide an unowned named hook in a file marked binary. A carriage return
also changed identity during text-mode newline conversion.

Do not narrow the marker grammar to hide these cases. `ALAB_MARKER_RE` says
any nonempty topic counts. One ALab-owned Python extractor now reads byte
records from `git grep` and returns exact topic identities and occurrences.
Feature inventory consumes those identities directly; `delta.sh --topics`
formats the same records for people. It no longer acts as a machine API.

Git emits NUL-delimited path and line-number fields. Parse those before the
source line's LF terminator, force text matching even for binary attributes
or NUL-containing blobs, and retain the existing owned-path exclusions.
Every distinct marker on a line contributes an occurrence. Reject malformed
records and contradictory exit-status/output combinations before displaying
anything. No-match remains an empty success; other Git failures propagate.

Keep ordinary output and source tabs unchanged. Quote topic names with edge
whitespace or controls, and quote paths/source lines when controls could
split or alter the display. The underlying identity is never trimmed,
normalized, or decoded through universal-newline conversion. Source selection
still ignores Git environment redirects and replacement objects.

The topic-only shell path uses the existing Python 3.11+ selector. This is a
documented dependency increase for that mode; history summary and patch modes
remain in Bash. A second escaped delimiter protocol would add another parser
without a benefit to the two current callers.

## Validation and limits

The old source failed all five new ownership regression cases in an isolated
temporary tree: leading-space/tab topics, binary attributes and NUL source
could falsely pass, while an exact carriage-return owner falsely failed.
The temporary tree was removed. Real Git tests also exercise newline paths,
display settings, repeated markers, old revisions, and empty/failing scans.
All 154 selected workflow tests passed after the change, including 23 feature
inventory cases and 16 topic-report cases. Injected malformed Git responses
produce no partial or successful topic report. Bash syntax and whitespace
checks passed, and independent review found no remaining issue in this scope.

At committed source `5dc01ad743998fe444ef902a1ef640998f39e845`, the old and new
normal reports are byte-for-byte identical: 246,698 bytes, SHA-256
`8b15f4fb4e5f7b8c863066ba6c1365ef72c7195e777a6a5826b96fc9a66e89e3`.
Canonical ownership remains 79 topics across 11 features.

The broader pre-change workflow run passed 142 tests. Seventeen additional
unit-result tests did not start because their prebuilt native runner was
absent at the configured path and in this session's retained target tree.
That is an incomplete wider check, not a pass. No Rust build was started for
this Python/Bash change, and no native or release qualification is claimed.

The missing helper was recovered later, and all 17 deferred unit-result tests
passed; see [the recovery record](native-unit-runner-recovery-2026-11-12.md).

Review separately found that the schema equality check accepts boolean/float
values equal to one. That does not explain missing hook identities and is
left for a separate schema change. GitHub issue reads remain unavailable with
the current development token; no credential or permission bypass was tried.
