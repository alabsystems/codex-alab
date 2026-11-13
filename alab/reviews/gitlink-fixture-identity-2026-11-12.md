<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Give each gitlink fixture a deterministic child identity

The wide Python run at `1a542da9b5` failed two subcases while preparing their
parent commits, before source validation ran. The retained `ignored`,
`assume-unchanged` and `staged` child repositories had identical trees, author
and committer identities, second-resolution timestamps and the message
`child`. All three therefore had commit
`27a33f52f3389983627ee96ce728f5df25613ac5`. Updating the parent gitlink to the
same object again changed nothing, so the next parent commit correctly failed.

Include the fixture label in the child commit message. This gives each
intended child a deterministic distinct identity without sleeping or relying
on wall-clock progress. Freeze both Git author and committer dates in the
existing multi-child test, so it continuously checks that independence even
when every child is created at the same instant. No production code, new test
method or validation assertion is changed.

With the fixed dates and old child message, the existing test produced five
setup failures in 2.095 seconds. After the label change, all six subcases
passed in 6.308 seconds and reached their intended source-validation checks.
The original wide-run log remains at
`$HOME/.aterm/.ubd/python-wide-after-tls-20261112-dw9wodbh.noindex/test_package_source.log`;
its retained fixture is `/private/tmp/.ubd/package-source-4cz1mxzl`.
The captured child identities and focused before/after logs are under
`$HOME/.aterm/.ubd/gitlink-fixture-validation-20261112-a2p5tsy1.noindex`.

The unchanged-source full package-source module ran 48 test methods in
210.889 seconds: 47 passed and the explicit native Trust cache fixture was
skipped. The run exited zero. Its raw log, source hash and before/after
results are retained in the same evidence directory.

This is a deterministic test-fixture repair, not a changed Git or source
custody policy. No Rust build or native compilation opt-in was run.
