<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Main comparison, descriptor custody and current pending work

Decision: compare the landed historical-skip reader with starting main, then
close concrete gaps beside it. The measured existing contracts showed no
regression. The next work closed
a descriptor-custody test blind spot and a real Cruise status-reading gap.

The comparison selects immutable evidence from starting main
`5a6c49b39db8093349bf6ce1580063ce0114e201` through landed
`d5df1272b3985288894ca5052d093386304d54e2`; it runs no parser replay or new
native test. All 13 common native methods pass in every observed phase. The
old-reader test-only RED and corrected GREEN establish 72 intended Cruise
historical-skip corrections, with all 72 corresponding Land positive controls
unchanged. Seventeen negative scenarios preserve their asserted nonempty
open-work contract; their complete tag vectors were not compared. Only the
first mixed row was measured in RED: two later rows were not reached. GREEN
passes all three mixed rows. These are bounded functional contracts, with no
allocation, latency, global regression or performance claim.

The four-payload comparison index is
`2729700529f6f4ed40051e36f8e5940d50fd7fe8618567c74fdffd33d1e63019`
(`codex-cruise-main-comparison-qr_dwyfa`, result
`54c171e0be8b0cdc86221f599691e82fc14392e4ff9f328984e11fc711f5c6a4`).
It binds actual earlier native observations and exact parser/test source bytes;
[the earlier review](cruise-historical-skip-recovery-2026-11-17.md) records their
original executions and source-specific limits.

The existing collision fixture checked refusal and unchanged file contents,
but could accept a guard that closed a caller's descriptor before returning
one. Production already refused correctly; this was a test sensitivity gap.
Commit `39b771684bd4f207f2549f65a28458190f487bdb` adds 80 lines only to the
existing fifth method. All five method IDs remain. Eight caller-owned read/write
descriptors (8–13, 18 and 19) now retain their original inode, access and offset
through refusal. A duplicate at 43 witnesses the same open file description:
after the target reads the remaining marker bytes, its duplicate must share
the advanced offset. Before-probe assertions establish the physical setup, and
post-probe EBADF is recorded as data for an assertion failure rather than a
fixture import/setup error. Existing status and content checks remain.

Original 6871/a7c04e completes the four-cohort experiment with actual and typed
outer status zero; fixture and source statuses are zero. Its child statuses
remain distinct:

| Whole five-method cohort | Child status | Actual method results |
| --- | ---: | --- |
| Original fixture, clean source | 0 | 5 passed |
| Original fixture, copied mutant | 0 | 5 passed |
| Strengthened fixture, clean source | 0 | 5 passed |
| Strengthened fixture, copied mutant | 1 | 4 passed; 1 failed-subtest method |

The mutant changes only two literal guards in exclusive owned copies of
`check.sh`: each occupied reserved descriptor is closed before the same refusal
status. The strengthened fixture records eight intended failures, one per
reserved descriptor, with natural errno 9. There are 20 executions of five
method identities, 19 passing executions and one intentional mutant failure;
this is not a 20-test production pass. The actual production guard, profiles
and protected producer are unchanged. The experiment measures close-on-refusal
sensitivity; no separate reopen-mutant result is claimed.

The next real parser gap was `The audit remains pending.`. Native RED on
`1beef4aa654cebd43ef58084fcb31b0658685777`, original 95141/b4cc13, returns
101 with 17 passed, one failed and zero ignored methods. The other six
component statuses are zero, and the typed guardian retains 101. Its aggregate
diagnostic records three mismatches: plain pending work, pending work after a
recovered historical probe, and pending review until approval. Cruise returns
an empty vector while the logged Land result contains `pending`.

The parser repair adds six lines and removes one. It tries the original shared
status phrases first, then a private Cruise-only `remains pending` phrase
through the existing status reader. Original status priority, including
`not verified`, is preserved. The shared `OPEN_STATUS` table and Land branch
are unchanged; negation, quoted wording, history and no-work reasons retain the
existing status-reader guards. The two new native functions collect all 11
scenario results before their aggregate assertions. They prescribe Cruise
results; Land is diagnostic data, not a new Green tag-vector assertion.

The fixture commit is normally merged with the parser repair before GREEN at
`e70d55712a519e10ffb7fa4da42f914208de0736`. Original 8569/cd0c17 returns zero
with typed guardian and all seven component statuses zero: all 18 ordinary
module methods pass, including the 11 new scenario rows, with no ignored test.
RED and GREEN use the same test bytes,
`3279e2eeef00747928cb74ab051f4fc5849472c6447a96ce62d20ada69bfc04c`. The new Green
Land vectors are neither logged nor asserted; unchanged Land behavior is
supported by source/shared-table parity and the existing native controls.

All native phases use the maintained protected, source-selected Trust 15698
route, actual pinned V8 inputs, Code Mode host and native runtime adapter.
Default capture, four build jobs, four test threads and the 8 MiB stack remain.
No result is a full-library, complete Python workflow, application certificate,
installed activation, Trust adoption or protected-producer qualification.
Existing human holds, including the held release-validation route, stand.

Private common-Git seals retain the actual failures and successes separately,
with source/tool/script bookends, raw outcomes, original process evidence,
typed receipts and opaque native receipt bytes. Compiled binaries and whole
Cargo targets are excluded:

- Descriptor experiment: `fd-custody-four-cohort-private-ymuqzhzs`, index
  `e3cc9028fcd7fe5f2cb01894d9d971c321803c270f0afff12303eb4c40aa0971`.
- Pending RED: `cruise-pending-red-private-g9wtlky5`, index
  `9127054c72be653815a7e4c24739b6682f112cb5cc660330f2b7ee7456acc98d`.
- Pending GREEN: `cruise-pending-green-private-t7mt49hx`, index
  `c5868a4178a3d4bbeecfc0c6526406ebad445af74b29a6a23c063ba81d7d22b0`.

Final focused all-target Tippy on e70, original 73989/771aeb, returns zero
with typed guardian and all nine component statuses zero. All 14 fresh
metadata-derived target/profile variants are present exactly once: normal and
test-profile libraries plus 12 integration targets. The maintained classifier
finds zero owned, spanning, failing-target or build-script findings; three
upstream diagnostics are set aside. Source/tool/script bookends agree,
SHA-256 `8e76e8e844a4dee0314d2648018c0ac0125d928a7bdf3a4cf7b20608bb136e6b`.
Verified V8 inputs agree before and after. The finite
`cruise-pending-tippy-private-idcxv6po` seal has index
`e4daf68ddf52a6b66b62a1da4d6f440859543c61ad0a6262c8c6f44a7afa6fc0`.
This is focused compile/lint coverage, separate from the native runtime and
controlled Python fixture experiments.

An initial normal alignment takes main
`4da443a5f0b8b29ea78c3174057e0a1e10081d31` into
`d9267fb98606817fd692cc9add47d821c2f0cacd`. It adds only two upstream-intake
record files. The checked Rust graph, parser, test, Python and tool inputs are
unchanged, so this alignment warrants no repetition of the completed focused
checks. Both session branches are included in the final candidate, preserving
their commits and the failed evidence alongside the successful results.

The first guarded publication returns one because main advanced before the
write; its stale lease is refused. A fresh fetch finds
`11c09ff6d2af7f3458ccc02677575f990015557a`, normally merged at
`03556b53edb2eb39cf77ed2a15e7ed17a5013a84`. The new changes belong to the
standalone API transport-test crate and its review. The sole lockfile change
adds its dependency on an already locked `zstd`; no package version or package
set changes.

A conservative walk of the immutable lockfiles, including locked development
edges, proves equal reachable records and edges for Autopilot (1,263 nodes),
Code Mode host (810) and the native adapter (18). The changed API-test package
is unreachable from all three roots. Selected Rust/helper sources and
manifests, the parser, and Python fixture inputs are unchanged. The source-bound
focused results therefore need no replay for this independent test-crate merge;
this graph comparison supplies no new-source application certificate. The
analysis is retained as `codex-cruise-concurrent-api-test-closure.json` in the
private task evidence. The refused publication and its typed nonzero result
remain distinct from the eventual successful publication.

The second guarded publication also refuses before writing because main advanced.
Fresh main `d472bce5ad98ce71bd5f9639df3fa6ba08674bf1` changes linked Rust
and control-plane Python sources. It is normally merged at
`2d88f7ae0b696b5dbb957f6faf3f73fee094fef8`. Unlike the preceding independent
API-test merge, this alignment requires fresh affected checks rather than
reusing the earlier source's results.

On 2d88, native original 63021/a9240b returns zero with the typed guardian
and all seven component statuses zero. All 18 exact ordinary module methods
pass, with zero failed or ignored. Source/tool/script bookends agree,
`819cf86cbf25b28fc560a54ebbe9cc4a61e4a33ca54b38c2c0a7214089a88436`.
This uses the same protected source-selected Trust route, actual V8 inputs,
host and runtime adapter described above; native receipt bodies remain opaque.

Qualified Python original 69410/f3e826 also returns zero with typed guardian,
fixture and source statuses zero. Fourteen complete affected modules execute
168 distinct maintained method IDs, each finished and passed once, without
skips. Every actual child status is zero; raw output matches its maintained
result. This covers the incoming classifier, cancellation, receipt, file-hash,
publication diagnostics and their direct process-cleanup endpoints, plus the
current five-method descriptor fixture. The owned Python PTY and local fake
transport controls supply no new native or application certificate. Source,
tool and script bookends agree,
`7505873821770e4e52c05cedd11482ee0c090dfab99f85c69d0f6a756419663c`.

Fresh focused all-target Tippy on 2d88, original 5128/ae9f0c, returns zero
with typed guardian and all nine component statuses zero. Fourteen variants
match the fresh metadata-derived target/profile inventory exactly once, with
no coverage error and a successful build-finished record. The classifier
finds no owned, spanning, failing-target or build-script findings; three
upstream diagnostics remain set aside. Source/tool/script bookends agree,
`a6c841f753253d27438138211c07c5fccd09c4e7bde68c8ceaa648f4d688bb5b`;
verified V8 bookends retain
`10f6d7a277b02c21e0c4fbe58782580713600477eea9c106729f1106e7d06bf8`.

The refreshed native finite seal `cruise-realigned-native-private-1akt0vxl`
has index `391a3b8fae72c6dc836856ba87e6608b604c412c8233db3683e85a4b282d1c5f`.
All checks completed against committed 2d88 before this documentation update.
Closed Python and lint outputs retain their source copies, actual process
provenance and typed receipts for finite archival in parallel with publication;
archival reads need no repetition of the passed checks. Publishing after the
actual checks and source bookends avoids delaying integration for independent
read-only evidence processing. This decision preserves the original failed
experiments and both refused stale leases. Existing human holds stand.
