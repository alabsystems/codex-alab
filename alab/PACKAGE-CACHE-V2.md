<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->

# Package-cache v2 validation, 2026-10-31

The preservation and clone-namespace changes were checked at
`6d13b4509ec3bd0c8b524eb1748db38c84f4b74c`, based on current main
`76c628303a0346cbe4fcbfb3b6309e0d2ec3f4d8`. The existing source-session,
surviving-custodian and lifeline machinery stays intact. The guardian adds only the
preservation context; the existing native protocol gains the v2 marker, distinct
clone namespace and exact admitted-path handoff.

The default minimum budget remains 64 GiB with the native learned cold-build allowance.
Older helpers retain the existing allocator. A capable helper's refusal is not converted
into unbounded allocation. Late preservation is enforced by the native receipt, including
old generations and interrupted retirement; a schema-3 receipt prevents older helpers
from silently ignoring that contract. New output never adopts an existing legacy target.

## Actual checks

Ninety focused Python cases passed, covering the cache frontend, source sessions,
guardian lifetime and post-fork failures, disk wiring, and build registration. Bash syntax,
Python AST and diff checks passed. The earlier prototype based on an obsolete local
checkout was not landed and its results are not counted here.

All four optional native bridge controls then matched and passed once, with no ignored
case or retry: actual target reuse across commits, settlement authority after group
retirement, physical/shim registrar compatibility, and the default-contained build.
The contained leg uses the real account-home lookup and the existing sandbox profile.
The built program printed its actual path, and the schema-3 guard and completed
settlement were read back from the private native receipt.

The optional source-session integration also passed once. It made four offline Trust
builds of a tiny app and local path dependency and executed all four outputs. Both units
were fresh on the stable second build; both fresh-source controls rebuilt. Its retained
stable target allocated 1,085,440 bytes and its final fresh-source target 1,667,072 bytes.
Those figures describe this tiny fixture, not a general saving or full-Codex benchmark.

This is 95 distinct passing cases, with no remaining native skip in this slice. No Rust
source in Codex changed, and no full Codex build or full check.sh gate is claimed.

## Native helper and closure

The helper was a private physical `atpkg` copy of the qualified aterm development image
from source `193bc5f5d76d913494809af8228e695ac2ad8dd3`. Its SHA-256 was
`397190ac56f7537cf916a15207334f5633d3815f3d023e2008518d17c5dca7f8`.
The original and copy matched, passed ARM64/signature/system-loader checks, and the copy
printed the exact v2 capability before use. The physical Trust 15698 toolchain ran the
offline builds in private homes with unset compiler flags and bounded concurrency.

Afterward the helper, original archive and toolchain hashes were unchanged; all test
processes were gone and the helper/eight target directories had no open users. Those
target directories allocated 2,203,648 bytes in total. All fixtures and executable images
were retained. No existing user cache, installed application or tool selection was changed.

The closed native bridge receipt has SHA-256
`77890c22c524a532595630432ac4d85928d7c23a5b4c291a9c2586204fa987f2`;
the source-session receipt has
`bd1b687e1f075b3b6ee6ce19f556a8d63cc8f130b7956fd09acc467e67919444`.
Initial fixture and audit-command errors remain in the private logs; none was hidden by
rerunning an unchanged runtime test. Operational v2 protection needs a v2-capable aterm;
v1-only installations continue through the legacy compatibility path.
