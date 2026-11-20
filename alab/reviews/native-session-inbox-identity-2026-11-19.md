<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Reading local inbox rows after a broker log change

Keep every distinct local row in the badge and receipt ordering. Read a body
for a typed `@offset` only when exactly one metadata row has that offset.
Reject the entire snapshot when two numeric rows claim the same local ID,
because local IDs identify both the body to read and the handled watermark.

In aterm at `6b9844f57c`, replacing the broker log clears the old row's
`on_log` flag and retains the row. The host's offset lookup checks that flag.
The `inbox --peek --meta` listing shows both local IDs and offsets but omits
the flag. Broker and fleet deliveries increment the same local ID counter.
A reused broker offset can therefore identify two distinct retained tasks in
the listing. A duplicated numeric local ID violates the host's row identity.
The host's own regression delivers different bodies at offset 40 across two
logs and expects its offset lookup to return the newer body.
[Broker log change](https://github.com/alabsystems/aterm/blob/6b9844f57cfb796edbf6e922076c3ae150ff494f/crates/aterm-gui-fabric/src/lib.rs#L1389),
[current log lookup and fleet IDs](https://github.com/alabsystems/aterm/blob/6b9844f57cfb796edbf6e922076c3ae150ff494f/crates/aterm-gui-fabric/src/lib.rs#L1481),
[broker IDs](https://github.com/alabsystems/aterm/blob/6b9844f57cfb796edbf6e922076c3ae150ff494f/crates/aterm-gui-fabric/src/lib.rs#L5598),
[metadata rendering](https://github.com/alabsystems/aterm/blob/6b9844f57cfb796edbf6e922076c3ae150ff494f/crates/aterm-gui-fabric/src/lib.rs#L6052),
[host offset-reuse control](https://github.com/alabsystems/aterm/blob/6b9844f57cfb796edbf6e922076c3ae150ff494f/crates/aterm-gui-fabric/src/lib_inbox_hold_tests.rs#L5195).

The two controls use the production client on a temporary Unix socket served
by the repository's in-process aterm fixture. One snapshot contains two task
IDs at the same offset and an ask at another offset. The other assigns one
local ID to a fleet ask and a broker task. The fixture records body requests
and receipts and restores its normal metadata before the recovery read.
[Runtime controls](../../codex-rs/alab-aterm/src/tests.rs),
[inbox client](../../codex-rs/alab-aterm/src/mail.rs).

From `codex-rs/` in the source-selected protected test shell, the following
command runs the two new controls.

```bash
alab_build_command "$ALAB_CARGO" --unverified test --locked -p codex-alab-aterm --lib -- \
  tests::repeated_broker_offset_keeps_ring_work_visible_without_reading_the_wrong_body \
  tests::duplicate_local_row_identity_refuses_the_snapshot_and_recovers_on_refresh --exact
```

On main source `a60939f2a2` with only the two controls added
(`43def6f970`), both failed at their target assertions. The offset read
failed to return a refusal, and the repeated local ID failed to return a
malformed snapshot. The protected test process exited 101.

On `f23cba4d1e`, both controls and eight surrounding tests passed. Every
requested name appeared in the test output and the guardian recorded exit 0.
The colliding offset caused no body request, the unrelated ask remained
readable, and the badge counted both tasks. The older task continued to hold
the newer receipt. A duplicate local ID prevented the snapshot, badge and
body read. Both fixtures read the expected body after a valid refresh.
The surrounding tests covered mixed fleet and broker rows, malformed metadata,
ordinary body access, previously handled work, badge counts and receipts.

Both native runs used source-selected Trust 15698 under the protected
test profile. The table records SHA-256 digests for the retained output.

| Source | Test log | Guardian completion |
| --- | --- | --- |
| `43def6f970` | `92290ea5dca00e344a20fd3f1e7c4109adaab31b2279a69f87d841b57eb9d684` | `2f011c6f5fd0761b8185092c6f314605bebe71061f08a540e00a2d41ee160abf` |
| `f23cba4d1e` | `cbec6d595cac4419e15ca294e14b5240052933891a0cbb5acd7949d9416e57ff` | `a9b3bfd6245b9e5c2c4378a0b7fbba8351f4e811b042a659cc85c3a536396e5c` |

These controls address snapshot identity in the third of the eleven
`native-session` scenarios. The existing fleet ordering control covers when
an older task releases a pending receipt. The metadata can establish whether
the offset is unique in this snapshot. Identifying the log of a single row
would require additional host metadata. A host process replacing its own log
was outside this execution. The registry at `a60939f2a2` has no
`native-session` entry. A complete feature recipe must also exercise the
other ten scenarios.
[Feature contract](../features.toml),
[recipe registry](../feature_recipes/scenarios.json),
[earlier fleet receipt controls](fleet-mail-receipt-ordering-2026-11-15.md).
