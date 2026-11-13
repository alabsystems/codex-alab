<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Apply exact ident observation to source admission

Git's builtin `ident` conversion can erase executable text within an expanded
`$Id: ...$` marker. Ordinary clean status alone therefore admitted altered
package source and let an altered qualification gate complete. Disabling
external filters does not disable this builtin conversion.

Call the shared `source_content.validate_ident` observer after seeding each
consumer's private committed-tree index and before querying status. It uses
the same effective attribute and Git environment policy as that consumer.
Raw committed bytes and exact Git checkout bytes remain valid, including
ordinary ident expansion combined with CRLF. Both consumers now discover and
suppress smudge-only drivers as well as clean/process drivers, since expected
checkout conversion must not invoke an external program.

Load the exact adjacent helper with `runpy.run_path`; a fetched controller must
not reuse another checkout's already imported module. Fixture copy lists carry
that dependency, including the fixture that refreshes the current package source
inside a clone of the previously committed tree. The qualification environment
builder is extracted without changing its existing Git configuration or
redirection policy.

Qualification records advance to schema 5. A successful observation produced
without this content check must not remain evidence of qualified source. Package
bindings need no schema change because every admission and return rechecks
their current physical content.

The package and qualification negative controls both failed against the old
consumer code. With the shared helper present, three focused package tests
passed in 7.306 seconds and six qualification tests passed in 11.518 seconds.
These cover the original false admissions, valid ident with CRLF, suppression
of smudge-only callbacks, ordinary line endings, the existing environment
boundaries, schema-5 output and rejection of older/malformed record schemas.
The root agent reviewed the consumer diff without finding a blocker, and
`git diff --check` passed.

Exact commands and baseline/final logs are retained under
`/private/tmp/alab-ident-consumers-0ta30807.noindex`. The shared helper has its
own implementation and validation record; it is a separate dependency of this
consumer commit. Full affected suites are reserved for the composed change.
No native build, live account registry or installed-package mutation was used.

## Combined validation

A public-controller regression adds executable shell text inside the ident
expansion in `lib.sh`. The original controller returned success and ran that
text; the combined change refuses before executing it or calling the builder
or selector, and preserves the file, index and registration. The focused fixed
run passed in 2.837 seconds. The independent guardian audit found the same
false admission and execution, but final Git removal preserved the source;
that audit is not evidence of deletion.

All 15 affected suites passed: 340 test executions with two skips, for the
explicit native compilation opt-in and the filename subcase this macOS
filesystem cannot represent. The suites include both consumers, source-cache
and controller paths, pristine retention, the protected-source lint step,
daily checks, upstream intake/status and the new helper. Independent reviews
verified the complete filter suppression, fresh-index environment, exact
adjacent helper loads and fixture dependencies in the combined tree.

The package regression and a direct production qualification observation
also passed under Python 3.9. The latter accepted the canonical form, rejected
the changed form and reported schema 5. The whole qualification test module
imports the separate status CLI's Python-3.11 TOML dependency, so its failed
Python-3.9 import attempt is retained without a test-pass claim.

The wider baseline ran 69 modules and 860 tests. Its only failure was an
existing smoke test's 30-second wall-clock assertion: a reverse lookup of
numeric loopback took about 35 seconds before the fixture started. That
separate fixture dependency was investigated independently; it does not
change the ident validation result.

Combined logs, commands and tested-file hashes are retained under
`$HOME/.aterm/.ubd/ident-composition-20261112-xggrdvhi.noindex`.
The evidence manifest SHA-256 is
`27fc7c269023d99dd547705e621f9b5679a0e426d23fb94ff3d4ffc0d4ad585c`.
