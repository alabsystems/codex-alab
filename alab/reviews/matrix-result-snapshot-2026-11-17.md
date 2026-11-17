<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Matrix result reads use one bounded stable snapshot

Decision: the session-owned landing audit found no remaining work to land, so
close a different concrete reader gap beside the existing matrix result binding.
Fresh main was `bef9ffc4faaf418899e21c838b9581be11e9f3c4`. OWN, WARM and the
retained pristine tree were clean, with their histories already reachable there;
no owned topic branch or stash remained. Of 327 owned-worktree reflog tips, 315
were landed and twelve were superseded amendments with landed replacements.
The last three alternatives were a stronger test matrix and two clarified
reports. No old identity was merged merely to make it reachable. Shared-root
and main ownership and all other actors' state were left alone. Private audit
`codex-session-owned-landing-audit-20261117.json`, SHA-256
`f7a102039741e439a9625c697f8abac829c6711bd5ff09067b20336a6408ba76`,
records that completed audit before this new topic began.

Matrix result processing already decoded JSON and computed resultSha256 from
one returned byte string. Its remaining `Path.read_bytes()` could still assemble
that string from a file rewritten during the read, or follow finite growth past
the file's original size. The new fixture retains real partial reads, owned-file
rewrites/appends and kernel metadata; admission and controller callbacks are
inert. No native host or Codex application is executed by these new controls.

Test-only source `4a587a16f6f01d0e3a98060f2688a73cee0693a8` adds three methods
and adapts only the existing seven-method result fixture helper. Actual RED
69763/c8497a returns one, with child, fixture and typed status one and source
status zero. All three IDs finish: the stable partial-read control passes and
two mutation assertions fail naturally, without errors or skips. In the
same-size case both complete JSON records fail the matrix predicate, but their
actual combined bytes pass: the public matrix returns zero, publishes one
passing case and hashes the 24,675-byte hybrid as
`4f890f017f593979b80562fb4a7aba9a4cc3e446f07a8081a3c2b9c2fa67a92f`.
The finite-growth control also returns zero/passed after reading 286,819 bytes,
where the original-size-plus-one bound is 24,676. Appending is finite: this
control demonstrates the overread without an unbounded writer or hang claim.
RED source/tool/script/plan bookends agree at
`32f843f63d75c528b93748233b9dfb902f52f42486c68e7721a9e1127e24a14a`.

Only after RED closed, source `74399b46270796deb832af0124501eb5f2f7a15e`
replaces the result read with the existing uncached file_bytes_snapshot helper,
using RuntimeError for the existing failed-matrix path. JSON decoding, verdict
checks and resultSha256 still consume the same returned bytes. No shared helper,
qualification predicate, absent-result behavior, schema, source/image pinning
or cache policy changes. This bounds a read by its initial size plus one and
checks the opened descriptor's metadata around that read; it imposes no fixed
maximum on the size of an otherwise stable file.

The seven existing result tests keep their method ASTs. Their helper's
replacement hook also runs after the real completed snapshot, so the two
post-read replacement controls remain meaningful: a later replacement does not
change the already observed bytes or their digest. There is no ongoing pathname
watch or guarantee about future pathname contents.

Actual GREEN 96533/686989 returns zero, with typed guardian, fixture and source
statuses zero. Eight complete maintained modules run 64 distinct methods: all
eight actual child statuses are zero, every ID finishes and passes, and no
errors, skips or failure IDs remain. This includes all three new controls,
seven existing result controls, six digest controls, nine status controls,
13 log-cleanup controls, ten manifest-cleanup controls, eleven runtime hash
controls and five history hash controls. Raw stdout agrees byte-for-byte with
maintained NamedResult output. Direct stderr is retained as ordinary evidence;
no empty-stderr or diagnostic-text gate defines a pass. GREEN bookends agree at
`ce932210e5a9e0b99ab8d5a1424cdcf691f02e2685b60ac4883f732b351cf51d`.
The same frozen new-test bytes are
`85e8607a2c52afd8dd1b056d2dac6d8836066fa6daf1c08850c0df041d95fc05`;
the adapted helper bytes are
`ab75997bef08652c8e9d7d21b551cc5d5a125e6e6e64709ab9336dd596d4b10b`.

These tests establish their bounded descriptor-read and report-binding
contracts, not pathname custody, producer identity, which image an application
executed, or actual paired native/Codex qualification. Matrix manifest scope
wording is production wording, not authority for the inert test callbacks.
The 26-method stand-in controller family is excluded because it launches Python
fixture hosts and loopback providers; no new actor proof is required for this
reader repair. Runtime/history controls retain their existing controlled fixture
scope. No native build, application certificate, installed activation, Trust
adoption, protected producer or ignored Land measurement is claimed. Existing
human holds stand.

Fresh fetch e45ddcdf65dc2df4c90a9c1204c7f75a0c788a0e brings separate HTTP
operation Rust diagnostics, documentation, a nonselected request-timeline test
and one request_timeline.py TARGETS entry. It is normally merged at
`81c7bb82c8ef743d01738cc76ca0969f96acdc51`. Immutable Git objects for the entire
smoke subtree, eight selected test modules, guardian/hash/CLI/collector/source
helpers and trust.lock are exactly unchanged from 74399 to that merge. The
checked flow references neither changed request-timeline Python file; Cargo.lock
also has no incoming change. The narrow closure record is
`codex-matrix-result-snapshot-incoming-closure.json`, SHA-256
`e5c3fb8e64a58f391250cbf2d7a29cfde645980473f640487c5d1a9a4de268fd`.
Original results remain source-bound to 74399; this unchanged Python closure
needs no replay for the alignment and supplies no new-source Rust or application
certificate.

Storage decision: after checks close, seal their original copied source/data in
place with a finite index and known status, rollout and capsule extras. No extra
full source copy or tool/binary/Cargo target payload is required. Closed evidence
processing does not hold the source tree or delay integration. Seal and
publication identities are recorded only after their actual outcomes.
