<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Recovery family validation — 2026-11-14

Decision: validate the complete existing `recovery::tests::` family after
composing the maintained Autopilot helper preparation with the peer protocol
change. Qualified source `ef45528c3dd97c36899429e1d86430f6b99928e7` passed all
33 exact family identities in native execution `35415`, exit zero. This is
bounded integration evidence from existing tests, with no behavior patch or
new RED claimed.

The wider family checks classification, definitive refusal matrices, latest-turn
matching, rejection of stale/aborted recovery, deadline serialization, carried
retry ladders, live-policy switches and retained reply/account-handoff state.
The composition includes the production `app-server-protocol` BioPolicy mapping
change as well as the new host preparation; the earlier five-case closure alone
did not cover this complete family.

## Source and isolation review

Actual owner is `codex-alab-autopilot`, library `codex_alab_autopilot`.
`recovery.rs` registers `recovery_tests.rs` under `cfg(test)`. Independent source
review counted 33 unique plain tests matching the retained required-name
inventory, with no ignored/per-test cfg/Tokio attributes, skip guards,
environment selectors or explicit process commands.

Four registered tests exercise real Core sessions through private TestCodex
home/cwd, local Wiremock and the existing large-stack helper. Two wrappers share
one helper body; all four remain distinct tests. That helper joins its owned
thread, propagates panic/error, and uses a two-worker Tokio runtime with 8 MiB
stacks. Each Core session explicitly shuts down. Local model replies keep these
cases inside the fixture; a socket/resource refusal would fail rather than skip.
The other 29 tests use local rollout/state values, serialization and synthetic
clock samples. Idempotent activation in the Core bodies does not toggle global
mode or conflict with an inactive-state expectation elsewhere in this family.
Serial execution preserves that shared-process contract.

The selected bodies need no MCP stdio server, CLI subprocess, real aterm host,
credential, installed application or external provider. Their generated
`update_plan` operation is in process. Maintained preparation supplies the
source-matching Code Mode host only, including its host-graph V8 inputs and both
artifact-bound aliases. The host remains lazy; artifact availability establishes
no host-process execution claim.

## Actual result and artifact custody

Source-selected Trust 15698 ran recorded standalone Autopilot units with filter
`recovery::tests::` and outer `--test-threads=1`. Independent checks matched
every required exact name to one emitted pass line and confirmed one summary:
33 passed, zero failed/ignored/measured, 937 filtered out, 3.34 seconds. This
qualifies 33 family cases, including the earlier five-case closure. The 937
filtered tests were not run in this check.

One recorded library provides artifact attribution separately from the exact
names and summary. Current metadata and helper artifacts independently select
only the Code Mode host binary; both native-receipt aliases match it. Native
unit-runner identity and the actual library executable are also verified.
Three current binary SHA identities match their retained records. Controller
and unit statuses are zero.

The retained source comparison records the complete `codex-rs` tree plus runtime
support and pin inputs unchanged from the prior qualifying compiled inputs.
Scoped all-target lint remains evidence at
`0c176de3b8d2014efd8fb88d1817f496b57d0532`: 1,393 unique canonical artifacts,
normal/test profiles, successful build, empty owned/spanning/failing/scripts
lists and three upstream diagnostics set aside. The 60 tooling passes remain
at `7476baefdbf49aa05e3ef56b06a66e4ebd722a77`. Neither check was repeated or
relabeled as a new result at the family source; their records remain linked by
source and hash in the [helper-preparation review](autopilot-helper-preparation-2026-11-14.md).

The complete private archive is retained at
`.git/alab-validation/ef45528c3dd97c36899429e1d86430f6b99928e7/recovery-family/`,
including `required-tests.json`, the qualified controller, metadata/artifacts,
focused summary and native receipt. All 20 indexed regular-file lengths and
hashes and the three current binary identities were independently checked.
The recorded-run directory contains 13 files. Raw receipts and contained
environment details remain private.

| Artifact | SHA-256 |
| --- | --- |
| `record.json` | `5e15e0cc5f9e8601cb913c4f54cb386837845e4fb943b296364e4bf3e5395106` |

The direct BioPolicy matrix checks downstream classification; it does not
exercise the app-server BioPolicy conversion end to end. Restart/signal cases
use synthetic recorded history rather than real process teardown or re-exec.
Native accepted actors, application certification and installed behavior remain
unproved by this run. The original backend cause remains unknown, and the
application certification boundary remains deferred and unchanged. Publication
identity and receipt remain in the private validation ledger.
