<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Stable, bounded matrix input digests

The requested session-ownership audit initially found nothing unlanded: the two
owned worktrees were clean and detached at `77f52ab23614cbb6570d20a24cae8aed70943db3`,
there was no session topic branch or stash, and all prior owned tips and the
intentional pristine upstream tree were ancestors of verified remote main
`88e8f5a6c22af1ce018aa0829479a9aa63e1cdcf`. The only local branch was another
session's `main`; it was left untouched. Independent local-only review agreed.
The initial record is `codex-matrix-digest-initial-land-audit.json`.

The next selected gap is the matrix's whole-file `read_bytes()` digest. Decision:
use the already established adjacent `file_hash.py` public reader, loaded from
its exact source path. Each matrix observation performs a fresh uncached read.
This avoids duplicating snapshot/error handling or depending on renewal's
private image cache. The hash helper already appears in matrix `pinned_inputs`.
The separate SHA of captured case-result bytes remains unchanged.

The finite controlled proof on starting main 88e8 uses owned real files and
legal partial reads capped at 8,192 bytes. After the first chunk, an actual
same-size rewrite changes A to B. The public matrix digest returns SHA-256
`31f36416dcf63f8390bff07d0eaabbde83dc520b66513435eae607c9ba2cc932`,
matching its saved actual A-prefix/B-tail stream and neither complete file.
The existing shared reader consumes the identical stream and refuses changed
descriptor metadata. Additional controls show the matrix accepting a finite
100-to-101-byte growth and following an owned leaf symlink; the shared reader
refuses both. Stable controls pass and acquired descriptors are closed.

Original proof completion `be1a4a` exits 0. Its private packet is
`codex-matrix-digest-proof-v2-qvj8wtdg`, index
`716f0cc997f7c3186cbc8eb085e425d25fe483c3f723f33382f6eef436e810ab`.
All nine production inputs and source identity stayed unchanged. This is a
controlled short-read interleaving, not an observed production race. No host,
application, provider, compiler or native qualification ran.

The first observer attempt, `583673`, exits 1: it read the whole file in one
chunk, returning coherent A after the rewrite, so its mixed-stream assertion
was not established. That failed attempt remains intact in
`codex-matrix-digest-proof-3mdb9stz`, index
`064e2d37bdd8fc2536d1bce0f4860b323dd5451a8c6492403e0c323ed5fc6109`.
The V2 observer changes only the real-read request cap; returned bytes, hashes
and metadata are not fabricated. Its finite guard is never reached.

The original inventories remain unchanged. A no-follow custody supplement in
`codex-matrix-digest-proof-seal-ndftcwqh` records the intentional fixture links
as links rather than treating their target bytes as regular files. V1 inventory
SHA-256 is `64f08226c843104f50746d403ea1f35580d513656d22872cdbd29d53fa1dae79`;
V2 is `d3320d7390bb8a100587d5570d0908bf3454aae87e771332ab379f97c7d35f52`.
Supplement verification `8b03e3` exits 0; both retain nine exact Git bindings.

Six focused methods were frozen before the repair, test SHA-256
`197820dd23de288c3c10014c45c226879ab07d644f03af1bdb346a17cdb406bc`.
They cover exact large/empty digests, actual fresh reads for repeated stable
observations, a subsequent same-size rewrite, mixed-read refusal, finite growth,
symlink refusal and safe directory refusal. Test-only source
`be659cdfe37ed3f361db4177083007c579b85624` genuinely fails: original tool session
10398/completion abe109 exits 1 with three passing and three failing methods,
without test errors. Packet `codex-wide-composition-python-5lgygna9` retains 929
Git inputs and stable tools, index
`67feec364ffa41818a30eda7fc1200bb47b510ef87960917335dcc4d01eef489`.

The repair uses the helper's regular/no-follow/nonblocking descriptor checks,
one-MiB chunks, initial-size-plus-one read bound and before/after device, inode,
mode, size, mtime and ctime comparison. Its established acquisition/primary/
close-failure policy stays in the shared implementation. Independent source and
fixture review approved this integration. No cache or new hash authority is
introduced. This does not establish ongoing pathname or executed-image custody,
nor change `native_host_record.measure`'s separate earlier read.

GitHub's maintained fetch was temporarily unavailable: completion `db5d6a`
exits 1 with native rate refusal 11 and `side_effect: none`, reserving the
remaining core allowance for the release cutter until 2026-11-17 01:39:03 UTC.
The earlier successful literal ref read and local object graph suffice for the
initial audit; that refused fetch is not reported as successful. Work proceeded
locally; while that reserve was active, no retry, alternate route or identity,
or person request was attempted.
The clock read 01:39:55 UTC before the next network operation. The maintained
fetch then completed 0 (`406696`), freshly verifying main
`50a63d6d6685a45fdb0ace365025d3edb0d7ed09` through its authoritative ref
snapshot, without a redundant separate API read.

After the repair at `b66609d5d37c6803303ab1dc12c5e4551e6c6f2a`, original tool
session 55904/completion d8a659 exits 0: all 68 distinct managed-Python methods
pass across the six matrix-digest, seven result-byte, nine strict-status, 26
rotation-controller, ten runtime-hash and ten image-digest controls. Original
52356/completion 171f74 exits 0 on Python 3.9: the overlapping 42 methods pass.
All eleven runner children are 0, without skips, failures or errors. Both runs
retain 929 exact Git inputs and stable executable maps; the frozen tests and
shared hash implementation are unchanged.

The managed packet is `codex-wide-composition-python-jfgduro1`, index
`cb829a7428b61d90c06d2f5c2273e08782e8703e02130822356cdb704e58d6cb`;
the compatibility packet is `codex-wide-composition-python-dm3ucf4m`, index
`f49947b9df3e7e98e206c579cc16f8338b420dbf997bd00178a7c0e5b56457fb`.
These are affected Python observations, not a new full-workflow, native gate or
application certificate. Existing human holds remain in force.
Independent packet audit `25c98e` exits 0 and verifies all three indices, exact
inventories, Git bindings, frozen tests, counts and recorded executable maps.

Main's later startup retry-budget tests were retained by a normal merge into
`4d6a932ff002314d7e79086998ecaaf87a138b15`. Within `alab`, `publish`,
`trust.lock` and `VERSION`, only an existing review document changes from the
tested b666 source. All executable Python inputs, the hash helper and the frozen
tests are unchanged. Results retain their exact recorded source scopes; this
merge adds no new Rust or native qualification claim. Publication uses the
maintained guarded publisher with a literal expected main, followed by a fresh
remote ancestry check and retirement of only this topic's branch.

The first publisher, original 75692/completion 5b6877, exits 1 and its completed
guardian records command status 1: main advanced before publication, so the
guard performed no ref update. Fresh maintained fetch `0b0639` exits 0 and
verifies forward main `1205d3459e688fd017c6f3d014127decb99eba61`. Its shared
JSON-writer/weekly-alias changes were normally merged at
`f7145d81ed1f1213036a9c85fbb315c38ffb22d4`; the digest repair, hash helper and
frozen tests are unchanged. This changed dependency justified a new check.

Originals 1097/completion edc3e1 and 71436/completion bf0ad2 both exit 0:
58 matrix/rotation/writer methods plus all 52 weekly methods, 110 distinct
managed-Python methods. Originals 20835/completion 9f8aac and 22245/completion
e19258 both exit 0 on Python 3.9: the overlapping 32 plus 52 methods, 84 total.
All eleven runner children are 0, without skips, failures or errors. Each
capture retains 933 Git source bindings and stable executable maps. The packets
and their indices are:

- `codex-wide-composition-python-6mkncmyq`:
  `3c840572422783786aa6d7ab7ef1031be66efc5d2d201bc80b5630aa588c9f44`.
- `codex-wide-composition-python-cou9855c`:
  `4d602f72ec0084f2906050636368082e932add1b57c63a8114aeac30ec894247`.
- `codex-wide-composition-python-txbgnguy`:
  `cbc16f8237051472bdc5bf31cb6cfe864adc9ad7879271158df5775201068bf0`.
- `codex-wide-composition-python-hqg02m0j`:
  `87d83b704a6c00e19dfaa404899340b78e2c21a44706daaeb43d419e33fd3040`.

These affected integration results preserve their exact f714 source scope;
they do not replace the earlier bounded-reader evidence or qualify a native
application. The refused publication receipt is retained separately from the
next guarded attempt with its fresh expected main.

The second publisher, original 83719/completion 08c1d0, also exits 1 with
supervised-command status 1. Native Agh refused a fetched source that differed
from its expected raw object ID, explicitly leaving cache refs unchanged and
reporting no side effect. This refusal is retained as a failed transport binding,
not relabeled a successful or ordinary advanced-main result. Fresh Agh read
`8b7c9d` and a new maintained snapshot fetch `6a66c8` both complete 0 and agree
on `37ce487652b090925f9b508cc4549289db2f77af`; its graph proves a forward
advance from 1205. No stale fetch packet or weakened guard is reused.

A normal merge produced `c687f9379297ca800c2da83c26e2af09725086b0`, preserving
the incoming weekly/runtime controls and Rust changes. The matrix, shared hash
reader and frozen digest tests remain unchanged. The new Python dependency
changes were checked on that clean merge. Originals 19009/b2a5b5 and 99395/3ab7ed
exit 0: 79 matrix/writer/runtime methods plus 52 weekly methods, 131 distinct
managed-Python methods. Originals 35121/069f3b and 6382/b9e37f exit 0: the
overlapping 53 plus 52 compatibility methods, 105 on Python 3.9. All fifteen
runner children are 0, with no failures, errors or skips. Each capture retains
933 Git input bindings and stable executable maps. The four packet indices are:

- `codex-wide-composition-python-e9n730no`:
  `385a0bf8ac5a2f8dbe891503e2ccad03f9e4b05ec890bf35428849ac6dd8517e`.
- `codex-wide-composition-python-mwfrdltp`:
  `275b8fbf8f0a28b52aec339b4988f94b0b8a3b119cd951b03faf713d6584ac60`.
- `codex-wide-composition-python-_y3i8sab`:
  `d101c17d395618a150cb318c41a481000a714827438f25dbbce1cc5ec55cba26`.
- `codex-wide-composition-python-v6msuxr8`:
  `659b1e85459d8ee9c2a3db34727dead5ed7b0118556d0a9b7ed46f4092a44991`.

These remain selected Python results on c687, not execution or certification
of incoming Rust. A new guarded publication uses the freshly verified main.
