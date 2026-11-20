<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve feature evidence scope and committed UTF-8 metadata

The portable audit on main `4cda30f86c78586b01fadf09e90009010d5a21ad`
found three reproducible validation gaps. Direct evidence validation and export
accepted an unexpected original `base`, although combining the same record
refused the scope loss. Declared command arguments containing NUL passed the
schema despite being invalid OS arguments. Inventory read committed UTF-8 TOML
through the process locale, rejecting valid accented metadata under ASCII.
These are metadata/API correctness gaps; the existing feature gate always
supplies its scoped expectation and no Native admission bypass was demonstrated.

Decision: keep each fix in its existing ALab-owned shared reader, with one
topic per commit. `11fc0b256b` moves the existing original-base rule into the
shared record validator and removes the redundant combine-only guard.
`a0d0054b0c` rejects NUL arguments while retaining literal backslash-zero and
Unicode arguments. `9fb4d34802` explicitly decodes committed catalogs as strict
UTF-8. No upstream file changes, visibility widening or new compatibility layer
is required. The combined source changes only two validators and their tests;
other production blobs and modes are identical to the starting main.

New controls exercise healthy scoped/unscoped records and missing, different
or unexpected bases through validation, combination, export and the CLI.
Invalid arguments cover argv[0], an entire argument and an embedded NUL, with
a tripwire proving rejection before log reads. Base errors retain the existing
error-accumulation behavior and may still inspect observations. Both invalid
cases refuse before claiming a bundle destination. The locale control runs the
actual copied CLI against committed accented TOML under `LC_ALL=C` and Python
`-X utf8=0`, then requires refusal of an invalid UTF-8 comment byte.

On exact `9fb4d34802228387e3ba3b98dee7c2e0d8676683`, all 159 distinct tests
from `test_feature*.py` plus `test_catalog_feature_ids` passed: zero failures,
errors or skips, including all four new methods. Original34650/1221→c843d1
closed with status0. The source is canonically clean before and after, and all
selected validator/test-file hashes remain unchanged. The normal inventory CLI
also returns0 with ownership complete for 111 hook topics and fourteen features.
Actual test log SHA-256:
`b90b8d61e701ddf8137a0dd925a54539f1df80acc59fb02261af749cf7b34b46`.
Structured result SHA-256:
`8f4cc318d9f92b21bee740fa1394abedd2185a6a769d915d02ac3e4a59695b43`.

Independent source reviews `62acd1f7…` and `f206f7db…` found no blocker and
preserve the distinction between scope-error accumulation and the argument
tripwire. These portable suites use real local Git fixtures, synthetic declared
logs and controlled gate boundaries; they do not execute Rust or grant Native
authority. Their success is not semantic qualification or an application
certificate. The earlier full gate on 3c8c0346 remains failed with 940 unresolved
integration warnings. Python-only changes justify the complete affected
portable suites rather than repeating that unchanged Rust failure.
