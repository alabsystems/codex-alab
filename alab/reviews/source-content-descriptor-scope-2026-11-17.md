<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Ident source descriptors have one owner through validation

The public `validate_ident` API at
`01a0d0ddd0fc8a002f7cd9868fec6427f616e7cd` leaked its opened source descriptor
when the first post-open `fstat` raised. `_open_regular` owned the integer file
descriptor, but had not returned it to the caller's closing `finally`. A real
owned Git/ident/private-index proof controlled only that selected-module failure:
both OSError and KeyboardInterrupt preserved the exact exception while original
`os.fstat` immediately proved the actual source descriptor remained open. The
proof then closed only that owned descriptor and observed EBADF. Success closed
once; an open refusal acquired and closed nothing. Source and both index bytes,
identities and timestamps stayed unchanged, as did 414 source input hashes.

This retains one descriptor per failed caught call until fallback or process
exit. Short CLI termination reclaims it; a persistent caller can accumulate
descriptors and retained inodes. No failed call produced successful validation
or false qualification. The small ownership repair is warranted without
changing source admission rules.

Decision: make the internal opener a context manager and keep acquisition,
immediate metadata validation, yielded consumption and final close under one
owner. The consumer uses `with`, with no raw-descriptor return handoff or
`fdopen` transfer. Parent/leaf checks, flags, identity comparisons, Git commands,
raw/canonical content policy and checkout subprocess/timer behavior are unchanged.

The source reader deliberately preserves the exact first primary exception when
close also raises. Close diagnostics are advisory and fully guarded, including
formatting or note-transport failures. A sole close exception propagates unchanged;
this does not add a new successful-exit classifier. This follows the source/hash
reader's first-primary policy, rather than importing the artifact observer's
different cancellation promotion policy. A close is attempted once, never retried
against a possibly reused descriptor.

Four public API methods check real descriptor state, success/open-refusal
controls, post-open failures, real noncanonical-content refusal with a close
failure, advisory errors and sole close failure. Actual source/index bytes remain
intact; module-local seams avoid affecting unrelated descriptors. Old-source
leaks are observed and then safely retired by the test's explicit owned fallback.

The strict baseline uses literal Git-object source bytes and identical final
tests. Its ten methods recorded seven terminal OK results, eight assertion
failures across two new parent methods, zero errors and one existing filesystem
subtest skip. The sole four-module affected run selected and ran 67 unique
methods in 42.803551 seconds: 66 terminal OK results, zero failures/errors, and
the identical preexisting non-UTF8-filename EILSEQ subtest skip. All four new
methods ran and passed. The unavailable filename branch is not claimed covered.
All 416 Python/ALab-shell/README hashes, HEAD/status and both exported file hashes
matched within both phases. Root and peer independently checked actual ordered
results, old Git bytes, final test identity and whole production-file reversal.

Evidence: `~/.codex/.ubd/source-content-fd-proof-20261117-az9q6bg4/` and
`~/.codex/.ubd/source-content-fd-fix-20261117-315i752q/`. Affected stderr SHA-256:
`218d992491b0984eda2ef7f4136e8ace5faffcda7986564a41680b8d178d1337`.
The unit runner inherited TMPDIR and did not record exact unit-fixture locations;
per-test TemporaryDirectory/addCleanup scopes were used, but no launch-bound
unit-fixture attribution or cleanup manifest can be reconstructed. Explicit
public-proof and export ownership is separately known. Only those four explicit
leaves were type/mode/content/no-holder/identity verified and retired into archive
SHA-256 `8f99a3df577f1a2e95d34e822fc80ec6195e0adb99a3890e9c940d9177c48d67`.
Raw proof and source/index copies remain outside. No rerun was used to
replace that evidence limit. These checks ran actual owned Git and finite
fixture gates, not `alab/check.sh`, a compiler, a native application or release.
