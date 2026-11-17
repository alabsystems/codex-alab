<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Smoke CLI publication alignment

Main advanced to `ce6ed387cf208ceec207bb014f22f7006a4a87c9` during final review.
Its executable Python change owns history teardown through final publication;
the other source changes are independently reviewed SSE test fixtures. A normal
merge produced `c373746b312c65b3b7fae9a7271e41ac92433d8a`. The shared CLI guard,
request/quota/weekly entries and `trust.lock` are unchanged from the earlier
verified composition.

Original 93860/completion 10a462 exits 0: eleven history-cleanup, nine history-CLI
and nine runtime-recorder methods pass. Original 94103/completion 417ddc exits 0:
all 22 maintained history smoke methods pass. All 51 IDs are distinct, all four
runner children are 0, with no skips or errors. Both captures retain 897 exact
Git input bindings and three stable executable maps on that clean source.

The retained original private packets are `codex-wide-composition-python-1vmzumsa`,
index `7aea22e6eda9bc265d4d410178b5022904a0d081b0a77d0f7d9855d5f27204d5`, and
`codex-wide-composition-python-sbj2ch0z`, index
`7d04156a6f04834809626fa107ff386e24a8362a01657efa499cfa3f71619f52`.

This is an affected-fixture result. The [full workflow and its archives](smoke-cli-full-python-followup-2026-11-16.md)
keep their exact E6 source scope; no newer full-workflow, Rust-library or
application-certificate claim follows. No held operation is retried. Publication
uses a fresh literal main lease, followed by remote ancestry and owned-branch
retirement checks.
