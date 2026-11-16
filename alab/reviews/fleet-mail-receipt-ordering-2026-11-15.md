<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve unread fleet tasks in receipt ordering

Decision: retain numeric inbox rows without broker offsets. Native aterm's mesh
mail uses `msg <id> off=- ... fleet=f<id>` in the same handled-watermark domain as
broker mail. The previous parser discarded those rows. A later broker receipt
could therefore mark an unread fleet task handled, hiding it from the badge and
restart refill. Native rendering and watermark controls were independently read
in `aterm-gui-fabric`; legacy `msg f<id>` mailbox rows have a separate domain.

Represent the broker offset as optional, keep every numeric ring row in receipt
ordering and the badge, and display a fleet blocker by local row id. Broker
triggers select only actual offsets; absent offsets never become `@0`.
Outbound posts and legacy fleet mailbox rows remain outside numeric ordering.
Reject unreadable numeric rows and unknown row forms rather than returning an
incomplete snapshot. Reject duplicate/bare snapshot fields and ambiguous `seen`
headers locally, without changing the general control-wire parser. Additive
unique fields remain accepted. A failed snapshot leaves receipts owed because
Autopilot awaits it before removing anything from its owed set.

Two regression methods exercise the public client against real Unix socket
frames: mixed fleet/broker/post/legacy rows, held receipts, badge counts, broker
body reads, no synthesized fleet trigger, resumed eligibility after a watermark
advance, malformed rows, duplicate kind/offset/watermark fields, and recovery on
the next valid snapshot. Against original production code both methods fail
with exit 101. The corrected exact-command fixtures are committed in
`c911b55462`; an initial unselected override did not reproduce the bug and is
not negative evidence.

Direct source-selected Trust 15698 runs the final full `codex-alab-aterm --lib`
suite in the explicit unverified lane: 237 passed, zero failed, three existing
ignored subprocess entrypoints, zero filtered. Autopilot's library check passes
with one existing upstream deprecation warning. Separate-target scoped Aterm
Tippy with `-D warnings` passes. An independent read-only review approves the
protocol and consumer changes. These are ordinary library observations, not
native gate qualification, application proof, packaging or installation.

Logs remain in the isolated mail worktree's `codex-rs/target/`: `mail-red.log`,
`mail-malformed-red.log`, `mail-final.log`, `mail-autopilot-check.log`, and
`mail-tippy-final.log`. Final implementation SHA-256 values are
`f3347e23d97a87c18784fc530d6694297f4d3086c7b2967ea0df6615ded9c12b`
for `mail.rs`, and
`7eee357db02fd75a78586012500738dbf5271bb272637fea531889c8d4317d2d`
for `supervisor.rs`.

## Composition and current gate availability

Compose the mail change with the independent aggregate-reply bound, both intake
extensions and incoming main `1412fc16f1`. The complete Aterm library now passes
241 cases, with the same three declared helper ignores and no failures or
filters. Separate-target scoped Tippy again passes. The committed intake
validator accepts the exact 61-commit range through `d17887a8d1`. No upstream
source is merged by those review records.

An actual maintained `alab/check.sh` entry attempt on clean `aa69f03382` exits
one before compilation: `a qualified atpkg-publish is required
(ALAB_ATPKG_PUBLISH)`. The frontend is absent from this machine's current PATH;
the retained earlier application-certificate refusal is a separate historical
barrier, not a new execution here. Preserve `native-gate-attempt.log`, do not
change gate or proof requirements, and claim neither a qualified application
nor an upstream integration from these available library checks.

A later exact-name scan finds an existing frontend outside PATH at
`~/.ubd/codex-native-codex-publication-s2.noindex/target/.ubd/native-codex-publication.noindex/debug/atpkg-publish`.
Its native `toolchain run --repo <isolated-mail-tree> -- /usr/bin/true` exits zero,
authenticating and leasing the unchanged Trust 15698 source selection. Preserve
`native-frontend-probe.log` separately from the original failed gate entry. This
restores the selection operation; it is no compilation, native unit result or
application certificate. The compiler/proc-macro certification inputs have not
changed, so that retained proof refusal is not rerun or reclassified here.

The startup-sweep source composes cleanly with these changes. Autopilot's library
check passes again in 14.51 seconds, with the same upstream deprecation warning.
Feature ownership validation accepts all 91 hook topics and 14 feature contracts,
including the new mail, reply-budget and startup-publication scenarios. Separate
session-test and lint observations retain their own source attribution in the
startup-sweep review.
